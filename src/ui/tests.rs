use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::*;
use crate::app::{Action, App, Effect};
use crate::config::{CommandSpec, Config};
use crate::runner::{RunId, RunnerEvent};

fn config(names: &[&str]) -> Config {
    Config {
        path: PathBuf::from("/proj/runs.toml"),
        root: PathBuf::from("/proj"),
        shell: vec!["sh".into(), "-c".into()],
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

/// 最下行: 左にキーの案内（はみ出した分は切る）、右端にタイトル。
fn help_row(width: usize, title: &str) -> String {
    let help_width = width - title.len();
    format!("{HELP:<help_width$.help_width$}{title}")
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
    expected.push(format!(
        "{:<70}{}",
        "reloaded runs.toml (3 commands)", "runs 1.2.3"
    ));
    terminal.backend().assert_buffer_lines(expected);
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

    // 一覧は 40% = 8 桁、区切り 1 桁、右ペイン 11 桁。はみ出した分は末尾を切る
    terminal.backend().assert_buffer_lines([
        "> 012345 0123456789a",
        "         0123456789a",
        &help_row(20, "t"),
    ]);
}

#[test]
fn pads_fullwidth_names_by_display_width() {
    let mut app = new_app("t", &["テスト", "b"]);
    app.apply(Action::SelectNext);
    run_selected(&mut app);
    let terminal = render(&mut app, 45, 3);

    // 名前の表示幅は 6。一覧は 45 桁の 40% = 18 桁で時間の列が切れるが、状態の列は揃う
    // `{:<w$}` は文字数で埋めるので、期待値も全角を含む部分は手で桁を合わせる（テスト 8 桁 = 全角 3 文字 + 空白 2）
    terminal.backend().assert_buffer_lines([
        format!("  テスト idle      {:<26}", "b  running 0s"),
        format!("> b      running  {:<26}", ""),
        help_row(45, "t"),
    ]);
}
