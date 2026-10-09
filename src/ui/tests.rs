use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::*;
use crate::app::{Action, App, Effect};
use crate::config::{CommandSpec, Config};
use crate::runner::{Capture, GhEvent, RunId, RunnerEvent};

fn config(names: &[&str]) -> Config {
    Config {
        path: PathBuf::from("/proj/runs.toml"),
        root: PathBuf::from("/proj"),
        shell: vec!["sh".into(), "-c".into()],
        encoding: None,
        commands: names
            .iter()
            .map(|name| CommandSpec {
                name: (*name).to_owned(),
                command: format!("echo {name}"),
                cwd: PathBuf::from("/proj"),
            })
            .collect(),
    }
}

fn exit_status(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
}

fn exited(run: RunId, code: i32) -> RunnerEvent {
    RunnerEvent::Exited {
        run,
        status: exit_status(code),
    }
}

fn new_app(title: &str, names: &[&str]) -> App {
    App::new(title, &config(names), Instant::now())
}

/// 選択中のコマンドを実行し、採番された run を返す。
fn run_selected(app: &mut App) -> RunId {
    match app.apply(Action::Run).as_slice() {
        [Effect::Start { run, .. }] => *run,
        other => panic!("Start が 1 つ返るはずが {other:?}"),
    }
}

fn advance(app: &mut App, seconds: u64) {
    app.set_now(app.now() + Duration::from_secs(seconds));
}

/// 3 コマンド: build は未実行、serve は 2 秒かかって exit 0（12 秒前）、test は実行中 12 秒で 30 行の出力。test を選択中。
/// タイトルはバージョンに依存しないよう固定する。
fn sample_app() -> App {
    let mut app = new_app("runs 1.2.3", &["build", "test", "serve"]);
    app.apply(Action::SelectNext);
    app.apply(Action::SelectNext);
    let serve = run_selected(&mut app);
    advance(&mut app, 2);
    app.on_runner_event(exited(serve, 0));
    app.apply(Action::SelectPrev);
    let test = run_selected(&mut app);
    for i in 1..=30 {
        app.on_runner_event(RunnerEvent::Output {
            run: test,
            bytes: format!("line {i}").into_bytes(),
        });
    }
    advance(&mut app, 12);
    app
}

/// `tui::run` と同じ手順で描く: layout で出力欄の高さを App に渡してから draw。
fn render(app: &mut App, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend の作成は失敗しない");
    let panes = layout(terminal.get_frame().area(), app);
    app.set_output_height(panes.output.height);
    terminal
        .draw(|frame| draw(frame, app))
        .expect("TestBackend への描画は失敗しない");
    terminal
}

/// 80 桁の 1 行: 一覧 24 桁（印 2 + 名前 5 + 1 + 状態 8 + 1 + 時間 7）+ 区切り 1 桁 + 右ペイン 55 桁。
fn row(list: &str, right: &str) -> String {
    format!("{list:<24} {right:<55}")
}

/// 最下行: 左にキーの案内（はみ出した分は切る）。タイトルは、案内の右に空白 2 桁を挟んで収まる幅のときだけ右端に出る
fn help_row(width: usize, title: &str) -> String {
    if width >= HELP.len() + 2 + title.len() {
        let help_width = width - title.len();
        format!("{HELP:<help_width$}{title}")
    } else {
        format!("{HELP:<width$.width$}")
    }
}

/// 成功した `gh` 2 回分の結果（取得時刻は 2026-10-08T07:13:56Z）。
fn fetched(pr_json: &str, run_json: &str) -> GhEvent {
    let capture = |json: &str| Capture {
        status: Some(exit_status(0)),
        stdout: json.as_bytes().to_vec(),
        ..Capture::default()
    };
    GhEvent::Fetched {
        pr: capture(pr_json),
        run: capture(run_json),
        now_unix: 1_791_443_636,
    }
}

const TWO_PRS: &str = r#"[
  {"number": 15, "title": "gh status", "headRefName": "feat/15-gh-status",
   "statusCheckRollup": [{"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS"}]},
  {"number": 16, "title": "Fix foo", "headRefName": "fix/16-foo",
   "statusCheckRollup": [{"__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": null}]}
]"#;

const TWO_RUNS: &str = r#"[
  {"headBranch": "main", "status": "completed", "conclusion": "success", "displayTitle": "CI (#14)", "createdAt": "2026-10-08T07:10:56Z"},
  {"headBranch": "feat/x", "status": "in_progress", "conclusion": "", "displayTitle": "wip", "createdAt": "2026-10-08T07:13:56Z"}
]"#;

/// sample_app で `g` を押し、結果を受けてから 12 秒たった状態。
fn status_app(pr_json: &str, run_json: &str) -> App {
    let mut app = sample_app();
    app.apply(Action::FetchGh);
    app.on_gh_event(fetched(pr_json, run_json));
    advance(&mut app, 12);
    app
}

#[test]
fn renders_list_and_output_at_80x24() {
    let mut app = sample_app();
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "test  running 12s"),
        row("> test  running  12s", "line 9"),
        row("  serve exit 0   12s ago", "line 10"),
    ];
    for i in 11..=30 {
        expected.push(row("", &format!("line {i}")));
    }
    expected.push(help_row(80, "runs 1.2.3"));
    assert_eq!(expected.len(), 24);
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn shows_offset_when_scrolled() {
    let mut app = sample_app();
    // 出力欄は 22 行。追従では 9 行目から。PageUp で先頭から
    render(&mut app, 80, 24);
    app.apply(Action::PageUp);
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "test  running 12s"),
        row("> test  running  12s", "line 1"),
        row("  serve exit 0   12s ago", "line 2"),
    ];
    for i in 3..=22 {
        expected.push(row("", &format!("line {i}")));
    }
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn header_shows_took_after_exit() {
    let mut app = sample_app();
    app.apply(Action::SelectNext);
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "serve  exit 0  took 2s"),
        row("  test  running  12s", ""),
        row("> serve exit 0   12s ago", ""),
    ];
    expected.extend((3..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn header_shows_name_only_when_idle() {
    let mut app = sample_app();
    app.apply(Action::SelectPrev);
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("> build idle", "build"),
        row("  test  running  12s", ""),
        row("  serve exit 0   12s ago", ""),
    ];
    expected.extend((3..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn notice_replaces_help_line() {
    let mut app = sample_app();
    app.set_notice("reloaded runs.toml (3 commands)");
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "test  running 12s"),
        row("> test  running  12s", "line 9"),
        row("  serve exit 0   12s ago", "line 10"),
    ];
    for i in 11..=30 {
        expected.push(row("", &format!("line {i}")));
    }
    // 80 桁ではタイトルが出ない（HELP 79 桁 + 2 + タイトル 10 桁 > 80。通知のときも同じ判断）
    expected.push(format!("{:<80}", "reloaded runs.toml (3 commands)"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn help_shows_title_only_when_it_fits() {
    let mut app = new_app("runs 1.2.3", &["a"]);

    // HELP 79 + 2 + 10 = 91 桁から出る。
    // 一覧は 2 + 1 + 1 + 8 + 1 + 7 = 20 桁、区切り 1 桁、右の見出しは未実行なので名前だけ
    assert_eq!(HELP.len(), 79);
    let wide = render(&mut app, 91, 3);
    wide.backend().assert_buffer_lines([
        format!("{:<91}", "> a idle             a"),
        " ".repeat(91),
        format!("{HELP}  runs 1.2.3"),
    ]);

    let narrow = render(&mut app, 90, 3);
    narrow.backend().assert_buffer_lines([
        format!("{:<90}", "> a idle             a"),
        " ".repeat(90),
        format!("{HELP:<90}"),
    ]);
}

#[test]
fn shows_all_states() {
    let mut app = new_app("t", &["a", "b", "c", "d"]);
    let a = run_selected(&mut app);
    app.on_runner_event(RunnerEvent::SpawnFailed {
        run: a,
        message: "boom".into(),
    });
    app.apply(Action::SelectNext);
    let b = run_selected(&mut app);
    app.apply(Action::Stop);
    app.on_runner_event(exited(b, 1));
    app.apply(Action::SelectNext);
    let c = run_selected(&mut app);
    app.on_runner_event(exited(c, 101));
    let terminal = render(&mut app, 30, 6);

    // 一覧は 40% = 12 桁で、時間の列は切れる。右ペインは 17 桁
    terminal.backend().assert_buffer_lines([
        "  a failed   c  exit 101  took",
        "  b stopped                   ",
        "> c exit 101                  ",
        "  d idle                      ",
        "                              ",
        &help_row(30, "t"),
    ]);
}

#[test]
fn status_pane_before_fetch() {
    let mut app = sample_app();
    app.apply(Action::TogglePane);
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "gh status  (press g to fetch)"),
        row("> test  running  12s", ""),
        row("  serve exit 0   12s ago", ""),
    ];
    expected.extend((3..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn status_pane_while_fetching() {
    let mut app = sample_app();
    app.apply(Action::FetchGh);
    advance(&mut app, 3);
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "gh status  fetching... 3s"),
        row("> test  running  15s", ""),
        row("  serve exit 0   15s ago", ""),
    ];
    expected.extend((3..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn status_pane_renders_prs_and_runs_at_80x24() {
    let mut app = status_app(TWO_PRS, TWO_RUNS);
    let terminal = render(&mut app, 80, 24);

    // ブランチの列は最長の名前の幅（PR は 17 桁、CI 実行は 6 桁）。CI 実行の経過は取得時点が基準で、その後は進まない
    let mut expected = vec![
        row("  build idle", "gh status  fetched 12s ago"),
        row("> test  running  24s", "Pull requests (open)"),
        row(
            "  serve exit 0   24s ago",
            " #15    feat/15-gh-status ok        gh status",
        ),
        row("", " #16    fix/16-foo        run..     Fix foo"),
        row("", "Recent CI runs"),
        row("", " main   ok        3m ago  CI (#14)"),
        row("", " feat/x run..     0s ago  wip"),
    ];
    expected.extend((7..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn status_pane_shows_error_for_failed_side_and_none_for_empty() {
    let mut app = sample_app();
    app.apply(Action::FetchGh);
    app.on_gh_event(GhEvent::Fetched {
        pr: Capture {
            status: Some(exit_status(4)),
            ..Capture::default()
        },
        run: Capture {
            status: Some(exit_status(0)),
            stdout: b"[]".to_vec(),
            ..Capture::default()
        },
        now_unix: 0,
    });
    let terminal = render(&mut app, 80, 24);

    let mut expected = vec![
        row("  build idle", "gh status  fetched 0s ago"),
        row("> test  running  12s", "Pull requests (open)"),
        row(
            "  serve exit 0   12s ago",
            " error: gh is not logged in (run: gh auth login)",
        ),
        row("", "Recent CI runs"),
        row("", " (none)"),
    ];
    expected.extend((5..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn status_pane_truncates_with_more_line() {
    let three_prs = r#"[{"number": 1, "title": "one", "headRefName": "a"},
                       {"number": 2, "title": "two", "headRefName": "b"},
                       {"number": 3, "title": "three", "headRefName": "c"}]"#;
    let mut app = status_app(three_prs, TWO_RUNS);
    // 高さ 8: 一覧 7 行、右は見出し 1 + 本文 6。本文は 1 + 3 + 1 + 2 = 7 行なので最後の 2 行が省かれる
    let terminal = render(&mut app, 80, 8);

    let expected = vec![
        row("  build idle", "gh status  fetched 12s ago"),
        row("> test  running  24s", "Pull requests (open)"),
        row("  serve exit 0   24s ago", " #1     a none      one"),
        row("", " #2     b none      two"),
        row("", " #3     c none      three"),
        row("", "Recent CI runs"),
        row("", " ... 2 more"),
        help_row(80, "runs 1.2.3"),
    ];
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn status_pane_truncates_long_titles_and_branches_by_width() {
    let long = r#"[{"number": 7, "title": "日本語のタイトルがとても長くて区画の幅に収まらない場合の確認です", "headRefName": "feature/a-very-long-branch-name-here"}]"#;
    let mut app = status_app(long, "[]");
    let terminal = render(&mut app, 68, 6);

    // 一覧は 24 桁、右は 43 桁（本文 4 行）。ブランチは上限 20 桁で `~`。前置きが 39 桁なのでタイトルは残り 4 桁:
    // 全角 1 文字（2 桁）+ `~` で、全角の途中では切らない
    terminal.backend().assert_buffer_lines([
        row24("  build idle", "gh status  fetched 12s ago", 68),
        row24("> test  running  24s", "Pull requests (open)", 68),
        // `{:<w$}` は文字数で埋めるので、全角を含む行は手で桁を合わせる（右は 42 桁 + 空白 1）
        format!(
            "{:<24} {} ",
            "  serve exit 0   24s ago", " #7     feature/a-very-long~ none      日~"
        ),
        row24("", "Recent CI runs", 68),
        row24("", " (none)", 68),
        help_row(68, "runs 1.2.3"),
    ]);
}

#[test]
fn status_pane_keeps_last_result_while_refetching() {
    let mut app = status_app(TWO_PRS, TWO_RUNS);
    app.apply(Action::FetchGh);
    advance(&mut app, 2);
    let terminal = render(&mut app, 80, 24);

    // 見出しだけ fetching... になり、本文は前回の結果のまま
    let mut expected = vec![
        row("  build idle", "gh status  fetching... 2s"),
        row("> test  running  26s", "Pull requests (open)"),
        row(
            "  serve exit 0   26s ago",
            " #15    feat/15-gh-status ok        gh status",
        ),
        row("", " #16    fix/16-foo        run..     Fix foo"),
        row("", "Recent CI runs"),
        row("", " main   ok        3m ago  CI (#14)"),
        row("", " feat/x run..     0s ago  wip"),
    ];
    expected.extend((7..23).map(|_| row("", "")));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

/// 幅 `width` の 1 行: 一覧 24 桁 + 区切り 1 桁 + 右ペイン。
fn row24(list: &str, right: &str, width: usize) -> String {
    let right_width = width - 25;
    format!("{list:<24} {right:<right_width$}")
}

#[test]
fn list_width_is_capped_at_40_percent() {
    let app = new_app("t", &["a-very-long-command-name-indeed"]);

    let panes = layout(Rect::new(0, 0, 40, 10), &app);

    assert_eq!(panes.list.width, 16);
    assert_eq!(panes.header, Rect::new(17, 0, 23, 1));
    assert_eq!(panes.output, Rect::new(17, 1, 23, 8));
}

#[test]
fn layout_reserves_rows_for_header_and_help() {
    let app = sample_app();

    let panes = layout(Rect::new(0, 0, 80, 24), &app);

    assert_eq!(panes.list, Rect::new(0, 0, 24, 23));
    assert_eq!(panes.header, Rect::new(25, 0, 55, 1));
    assert_eq!(panes.output, Rect::new(25, 1, 55, 22));
    assert_eq!(panes.help, Rect::new(0, 23, 80, 1));
}

#[test]
fn does_not_panic_at_tiny_sizes() {
    for (w, h) in [(0, 0), (1, 1), (1, 5), (5, 1), (0, 5), (5, 0), (2, 2)] {
        let mut app = sample_app();
        render(&mut app, w, h);
        let panes = layout(Rect::new(0, 0, w, h), &app);
        assert!(
            panes.output.width <= w && panes.output.height <= h,
            "{w}x{h}"
        );
        // status 区画（取得前・取得中・結果あり）も同じ
        app.apply(Action::TogglePane);
        render(&mut app, w, h);
        app.apply(Action::FetchGh);
        render(&mut app, w, h);
        app.on_gh_event(fetched(TWO_PRS, TWO_RUNS));
        render(&mut app, w, h);
    }
}

#[test]
fn long_names_and_lines_are_truncated() {
    let mut app = new_app("t", &["0123456789abcdef"]);
    let run = run_selected(&mut app);
    app.on_runner_event(RunnerEvent::Output {
        run,
        bytes: b"0123456789abcdefghij".to_vec(),
    });
    let terminal = render(&mut app, 20, 3);

    // 一覧は 40% = 8 桁。名前は最低 6 桁は出し（切ったので末尾は `~`）、右の列は切れる。右ペイン 11 桁
    terminal.backend().assert_buffer_lines([
        "> 01234~ 0123456789a",
        "         0123456789a",
        &help_row(20, "t"),
    ]);
}

#[test]
fn pads_fullwidth_names_by_display_width() {
    let mut app = new_app("t", &["テスト", "b"]);
    app.apply(Action::SelectNext);
    run_selected(&mut app);
    let terminal = render(&mut app, 70, 3);

    // 名前の表示幅は 6。一覧は 2 + 6 + 1 + 8 + 1 + 7 = 25 桁（70 桁の 40% = 28 に収まる）で、状態と時間の列が揃う。
    // `{:<w$}` は文字数で埋めるので、全角を含む行は手で桁を合わせる（テスト = 全角 3 文字 = 6 桁）
    terminal.backend().assert_buffer_lines([
        format!("  テスト idle{:13}{:<44}", "", "b  running 0s"),
        format!("> b      running  0s     {:<44}", ""),
        help_row(70, "t"),
    ]);
}

#[test]
fn long_names_keep_status_and_time_columns() {
    let mut app = new_app("runs 1.2.3", &["frontend-dev-server", "api"]);
    run_selected(&mut app);
    advance(&mut app, 12);
    let terminal = render(&mut app, 80, 24);

    // 一覧は 80 桁の 40% = 32 桁。名前は 32 - (2 + 1 + 8 + 1 + 7) = 13 桁に切られ（末尾は `~`）、状態と時間は残る
    let mut expected = vec![
        format!(
            "{:<32} {:<47}",
            "> frontend-dev~ running  12s", "frontend-dev-server  running 12s"
        ),
        format!("{:<32} {:<47}", "  api           idle", ""),
    ];
    expected.extend((2..23).map(|_| " ".repeat(80)));
    expected.push(help_row(80, "runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

/// Unix の終了コードは 8 ビットなので、大きなコードは Windows でしか起きない。
#[cfg(windows)]
#[test]
fn huge_exit_code_does_not_shift_columns() {
    let mut app = new_app("t", &["a", "b"]);
    let run = run_selected(&mut app);
    app.on_runner_event(exited(run, 1_000_000));
    let terminal = render(&mut app, 60, 3);

    // 一覧は 2 + 1 + 1 + 8 + 1 + 7 = 20 桁。8 桁に収まらない終了コードは一覧では `exit ?`、見出しに全文
    terminal.backend().assert_buffer_lines([
        format!(
            "{:<20} {:<39}",
            "> a exit ?   0s ago", "a  exit 1000000  took 0s"
        ),
        format!("{:<20} {:<39}", "  b idle", ""),
        help_row(60, "t"),
    ]);
}
