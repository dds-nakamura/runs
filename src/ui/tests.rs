use std::path::PathBuf;
use std::process::ExitStatus;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::*;
use crate::app::{Action, App};
use crate::config::{CommandSpec, Config};
use crate::runner::RunnerEvent;

fn config(names: &[&str]) -> Config {
    Config {
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

/// 3 コマンド: build は未実行、test は実行中で 30 行の出力、serve は exit 0。test を選択中。
/// タイトルはバージョンに依存しないよう固定する。
fn sample_app() -> App {
    let mut app = App::new("runs 1.2.3", &config(&["build", "test", "serve"]));
    app.apply(Action::SelectNext);
    app.apply(Action::SelectNext);
    app.apply(Action::Run);
    app.on_runner_event(RunnerEvent::Exited {
        id: 2,
        status: exit_status(0),
    });
    app.apply(Action::SelectPrev);
    app.apply(Action::Run);
    for i in 1..=30 {
        app.on_runner_event(RunnerEvent::Output {
            id: 1,
            bytes: format!("line {i}").into_bytes(),
        });
    }
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

/// 80 桁の 1 行: 一覧 16 桁 + 区切り 1 桁 + 出力 63 桁。
fn row(list: &str, output: &str) -> String {
    format!("{list:<16} {output:<63}")
}

/// 63 桁。80 桁の画面では右端 10 桁にタイトル `runs 1.2.3` が入る
const HELP: &str = "Up/Down select  Enter run  s stop  PgUp/PgDn/End scroll  q quit";

#[test]
fn renders_list_and_output_at_80x24() {
    let mut app = sample_app();
    let terminal = render(&mut app, 80, 24);

    let mut expected = Vec::new();
    expected.push(row("  build idle", "line 8"));
    expected.push(row("> test  running", "line 9"));
    expected.push(row("  serve exit 0", "line 10"));
    for i in 11..=30 {
        expected.push(row("", &format!("line {i}")));
    }
    expected.push(format!("{HELP:<70}runs 1.2.3"));
    assert_eq!(expected.len(), 24);
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn shows_offset_when_scrolled() {
    let mut app = sample_app();
    // 出力欄は 23 行。PageUp で先頭 7 行目 → 0 行目から表示
    render(&mut app, 80, 24);
    app.apply(Action::PageUp);
    let terminal = render(&mut app, 80, 24);

    let mut expected = Vec::new();
    expected.push(row("  build idle", "line 1"));
    expected.push(row("> test  running", "line 2"));
    expected.push(row("  serve exit 0", "line 3"));
    for i in 4..=23 {
        expected.push(row("", &format!("line {i}")));
    }
    expected.push(format!("{HELP:<70}runs 1.2.3"));
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn shows_all_states() {
    let mut app = App::new("t", &config(&["a", "b", "c", "d"]));
    app.apply(Action::Run);
    app.on_runner_event(RunnerEvent::SpawnFailed {
        id: 0,
        message: "boom".into(),
    });
    app.apply(Action::SelectNext);
    app.apply(Action::Run);
    app.apply(Action::Stop);
    app.on_runner_event(RunnerEvent::Exited {
        id: 1,
        status: exit_status(1),
    });
    app.apply(Action::SelectNext);
    app.apply(Action::Run);
    app.on_runner_event(RunnerEvent::Exited {
        id: 2,
        status: exit_status(101),
    });
    let terminal = render(&mut app, 30, 6);

    terminal.backend().assert_buffer_lines([
        "  a failed                    ",
        "  b stopped                   ",
        "> c exit 101                  ",
        "  d idle                      ",
        "                              ",
        "Up/Down select  Enter run  s t",
    ]);
}

#[test]
fn list_width_is_capped_at_40_percent() {
    let app = App::new("t", &config(&["a-very-long-command-name-indeed"]));

    let panes = layout(Rect::new(0, 0, 40, 10), &app);

    assert_eq!(panes.list.width, 16);
    assert_eq!(panes.output.x, 17);
    assert_eq!(panes.output.width, 23);
}

#[test]
fn layout_reserves_one_row_for_help() {
    let app = sample_app();

    let panes = layout(Rect::new(0, 0, 80, 24), &app);

    assert_eq!(panes.output.height, 23);
    assert_eq!(panes.list.height, 23);
    assert_eq!(panes.help, Rect::new(0, 23, 80, 1));
}

#[test]
fn does_not_panic_at_tiny_sizes() {
    for (w, h) in [(0, 0), (1, 1), (1, 5), (5, 1), (0, 5), (5, 0)] {
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
    let mut app = App::new("t", &config(&["0123456789abcdef"]));
    app.apply(Action::Run);
    app.on_runner_event(RunnerEvent::Output {
        id: 0,
        bytes: b"0123456789abcdefghij".to_vec(),
    });
    let terminal = render(&mut app, 20, 3);

    // 一覧は 40% = 8 桁、区切り 1 桁、出力 11 桁。はみ出した分は末尾を切る。
    // 最下行はキーの案内を 19 桁で切り、右端 1 桁にタイトル `t`
    terminal.backend().assert_buffer_lines([
        "> 012345 0123456789a",
        "                    ",
        "Up/Down select  Entt",
    ]);
}

#[test]
fn pads_fullwidth_names_by_display_width() {
    let mut app = App::new("t", &config(&["テスト", "b"]));
    app.apply(Action::SelectNext);
    app.apply(Action::Run);
    let terminal = render(&mut app, 45, 3);

    // 名前の表示幅は 6。一覧は 2 + 6 + 1 + 8 = 17 桁（45 桁の 40% = 18 に収まる）で、状態の列が揃う
    terminal.backend().assert_buffer_lines([
        "  テスト idle                                ",
        "> b      running                             ",
        "Up/Down select  Enter run  s stop  PgUp/PgDnt",
    ]);
}
