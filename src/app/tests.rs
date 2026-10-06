use std::path::PathBuf;
use std::process::ExitStatus;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::*;
use crate::config::{CommandSpec, Config};
use crate::runner::RunnerEvent;

fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

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

/// 3 コマンド、出力欄の高さ 5 行。
fn app() -> App {
    let mut app = App::new("runs test", &config(&["build", "test", "serve"]));
    app.set_output_height(5);
    app
}

/// イベントループと同じ手順（イベント → Action → 状態）で 1 件処理する。
fn feed(app: &mut App, event: &Event) -> Option<Effect> {
    action_for(event).and_then(|action| app.apply(action))
}

fn quits_on(event: &Event) -> bool {
    let mut app = app();
    feed(&mut app, event);
    app.should_quit()
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

fn output(id: usize, text: &str) -> RunnerEvent {
    RunnerEvent::Output {
        id,
        bytes: text.as_bytes().to_vec(),
    }
}

fn selected_lines(app: &App) -> Vec<&str> {
    app.selected_command()
        .map(|c| c.output().lines().collect())
        .unwrap_or_default()
}

// --- #1 からの挙動（変えない） -------------------------------------------------

#[test]
fn initial_state_is_running() {
    let app = app();

    assert!(!app.should_quit());
    assert_eq!(app.title(), "runs test");
}

#[test]
fn q_press_quits() {
    let event = press(KeyCode::Char('q'), KeyModifiers::NONE);

    assert_eq!(action_for(&event), Some(Action::Quit));
    assert!(quits_on(&event));
}

#[test]
fn ctrl_c_press_quits() {
    let event = press(KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(action_for(&event), Some(Action::Quit));
    assert!(quits_on(&event));
}

#[test]
fn ctrl_shift_c_press_quits() {
    // Caps Lock や Shift で文字が大文字になっていても終了できる
    assert!(quits_on(&press(
        KeyCode::Char('C'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT
    )));
    assert!(quits_on(&press(KeyCode::Char('C'), KeyModifiers::CONTROL)));
}

#[test]
fn q_release_is_ignored() {
    // Windows では Press と Release が両方届く
    let event = key(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );

    assert_eq!(action_for(&event), None);
    assert!(!quits_on(&event));
}

#[test]
fn q_repeat_is_ignored() {
    let event = key(KeyCode::Char('q'), KeyModifiers::NONE, KeyEventKind::Repeat);

    assert_eq!(action_for(&event), None);
    assert!(!quits_on(&event));
}

#[test]
fn ctrl_c_release_is_ignored() {
    let event = key(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        KeyEventKind::Release,
    );

    assert_eq!(action_for(&event), None);
}

#[test]
fn other_keys_are_ignored() {
    let events = [
        press(KeyCode::Esc, KeyModifiers::NONE),
        press(KeyCode::Char('Q'), KeyModifiers::NONE),
        press(KeyCode::Char('Q'), KeyModifiers::SHIFT),
        press(KeyCode::Char('q'), KeyModifiers::ALT),
        press(KeyCode::Char('q'), KeyModifiers::CONTROL),
        press(KeyCode::Char('c'), KeyModifiers::NONE),
        press(KeyCode::Char('x'), KeyModifiers::NONE),
    ];

    for event in &events {
        assert_eq!(action_for(event), None, "{event:?}");
        assert!(!quits_on(event), "{event:?}");
    }
}

#[test]
fn resize_is_ignored() {
    let event = Event::Resize(1, 1);

    assert_eq!(action_for(&event), None);
    assert!(!quits_on(&event));
}

// --- キーバインド ----------------------------------------------------------------

#[test]
fn keys_map_to_actions() {
    let cases = [
        (KeyCode::Up, Action::SelectPrev),
        (KeyCode::Char('k'), Action::SelectPrev),
        (KeyCode::Down, Action::SelectNext),
        (KeyCode::Char('j'), Action::SelectNext),
        (KeyCode::Enter, Action::Run),
        (KeyCode::Char('s'), Action::Stop),
        (KeyCode::PageUp, Action::PageUp),
        (KeyCode::PageDown, Action::PageDown),
        (KeyCode::End, Action::ScrollToEnd),
    ];

    for (code, action) in cases {
        assert_eq!(
            action_for(&press(code, KeyModifiers::NONE)),
            Some(action),
            "{code:?}"
        );
        assert_eq!(
            action_for(&key(code, KeyModifiers::NONE, KeyEventKind::Release)),
            None,
            "{code:?} release"
        );
    }
}

// --- 一覧と選択 ------------------------------------------------------------------

#[test]
fn lists_commands_in_config_order() {
    let app = app();

    let names: Vec<_> = app.commands().iter().map(CommandView::name).collect();
    assert_eq!(names, ["build", "test", "serve"]);
    assert!(
        app.commands()
            .iter()
            .all(|c| c.state() == CommandState::Idle)
    );
    assert_eq!(app.selected(), 0);
}

#[test]
fn select_moves_and_stops_at_ends() {
    let mut app = app();

    assert_eq!(app.apply(Action::SelectPrev), None);
    assert_eq!(app.selected(), 0);

    app.apply(Action::SelectNext);
    app.apply(Action::SelectNext);
    assert_eq!(app.selected(), 2);

    app.apply(Action::SelectNext);
    assert_eq!(app.selected(), 2);

    app.apply(Action::SelectPrev);
    assert_eq!(app.selected(), 1);
}

// --- 実行と停止 ------------------------------------------------------------------

#[test]
fn run_marks_running_and_requests_start() {
    let mut app = app();
    app.apply(Action::SelectNext);

    let effect = app.apply(Action::Run);

    assert_eq!(effect, Some(Effect::Start(1)));
    assert_eq!(app.commands()[1].state(), CommandState::Running);
    assert_eq!(app.commands()[0].state(), CommandState::Idle);
}

#[test]
fn run_on_running_is_noop() {
    let mut app = app();
    app.apply(Action::Run);

    assert_eq!(app.apply(Action::Run), None);
}

#[test]
fn stop_requests_only_when_running() {
    let mut app = app();

    assert_eq!(app.apply(Action::Stop), None);

    app.apply(Action::Run);
    assert_eq!(app.apply(Action::Stop), Some(Effect::Stop(0)));
    // 停止要求中も、通知が来るまでは実行中のまま
    assert_eq!(app.commands()[0].state(), CommandState::Running);
}

#[test]
fn exited_sets_code() {
    let mut app = app();
    app.apply(Action::Run);

    app.on_runner_event(RunnerEvent::Exited {
        id: 0,
        status: exit_status(3),
    });

    assert_eq!(
        app.commands()[0].state(),
        CommandState::Exited { code: Some(3) }
    );
}

#[test]
fn exited_after_stop_request_is_stopped() {
    let mut app = app();
    app.apply(Action::Run);
    app.apply(Action::Stop);

    // Windows の taskkill は終了コード 1 で終わらせる。停止を頼んだのだから「停止」と見せる
    app.on_runner_event(RunnerEvent::Exited {
        id: 0,
        status: exit_status(1),
    });

    assert_eq!(app.commands()[0].state(), CommandState::Stopped);
}

#[cfg(unix)]
#[test]
fn signal_exit_is_stopped() {
    use std::os::unix::process::ExitStatusExt;
    let mut app = app();
    app.apply(Action::Run);

    // 停止を頼んでいなくても、シグナルで終わったら「停止」
    app.on_runner_event(RunnerEvent::Exited {
        id: 0,
        status: ExitStatus::from_raw(15),
    });

    assert_eq!(app.commands()[0].state(), CommandState::Stopped);
}

#[test]
fn spawn_failed_sets_state_and_message() {
    let mut app = app();
    app.apply(Action::Run);

    app.on_runner_event(RunnerEvent::SpawnFailed {
        id: 0,
        message: "failed to start \"zsh\": not found".to_owned(),
    });

    assert_eq!(app.commands()[0].state(), CommandState::SpawnFailed);
    assert_eq!(selected_lines(&app), ["failed to start \"zsh\": not found"]);
}

#[test]
fn rerun_clears_output() {
    let mut app = app();
    app.apply(Action::Run);
    app.on_runner_event(output(0, "first run"));
    app.on_runner_event(RunnerEvent::Exited {
        id: 0,
        status: exit_status(0),
    });

    let effect = app.apply(Action::Run);

    assert_eq!(effect, Some(Effect::Start(0)));
    assert_eq!(app.commands()[0].state(), CommandState::Running);
    assert!(selected_lines(&app).is_empty());
}

#[test]
fn output_goes_to_its_command() {
    let mut app = app();

    app.on_runner_event(output(1, "from test"));
    app.on_runner_event(output(2, "from serve"));

    assert!(selected_lines(&app).is_empty());
    app.apply(Action::SelectNext);
    assert_eq!(selected_lines(&app), ["from test"]);
    app.apply(Action::SelectNext);
    assert_eq!(selected_lines(&app), ["from serve"]);
}

#[test]
fn events_for_unknown_id_are_ignored() {
    let mut app = app();

    app.on_runner_event(output(99, "nowhere"));
    app.on_runner_event(RunnerEvent::Exited {
        id: 99,
        status: exit_status(0),
    });

    assert!(app.commands().iter().all(|c| c.output().len() == 0));
}

#[test]
fn output_is_sanitized() {
    let mut app = app();

    app.on_runner_event(output(0, "\x1b[31mred\x1b[0m\x07"));

    assert_eq!(selected_lines(&app), ["red?"]);
}

// --- スクロール ------------------------------------------------------------------

fn app_with_lines(count: usize) -> App {
    let mut app = app();
    for i in 1..=count {
        app.on_runner_event(output(0, &format!("line {i}")));
    }
    app
}

#[test]
fn follows_tail_by_default() {
    let app = app_with_lines(20);

    assert_eq!(app.scroll(), Scroll::Follow);
    // 高さ 5 なので 16..=20 行目が見える
    assert_eq!(app.visible_range(), 15..20);
}

#[test]
fn page_up_leaves_follow() {
    let mut app = app_with_lines(20);

    app.apply(Action::PageUp);
    assert_eq!(app.scroll(), Scroll::At(10));
    assert_eq!(app.visible_range(), 10..15);

    app.apply(Action::PageUp);
    app.apply(Action::PageUp);
    app.apply(Action::PageUp);
    assert_eq!(app.scroll(), Scroll::At(0));
}

#[test]
fn page_down_returns_to_follow_at_the_end() {
    let mut app = app_with_lines(20);
    app.apply(Action::PageUp);
    app.apply(Action::PageUp);
    assert_eq!(app.scroll(), Scroll::At(5));

    app.apply(Action::PageDown);
    assert_eq!(app.scroll(), Scroll::At(10));

    app.apply(Action::PageDown);
    assert_eq!(app.scroll(), Scroll::Follow);
}

#[test]
fn end_returns_to_follow() {
    let mut app = app_with_lines(20);
    app.apply(Action::PageUp);

    app.apply(Action::ScrollToEnd);

    assert_eq!(app.scroll(), Scroll::Follow);
}

#[test]
fn new_output_does_not_move_while_scrolled() {
    let mut app = app_with_lines(20);
    app.apply(Action::PageUp);

    app.on_runner_event(output(0, "line 21"));

    assert_eq!(app.scroll(), Scroll::At(10));
    assert_eq!(app.visible_range(), 10..15);
}

#[test]
fn page_up_with_few_lines_stays_at_top() {
    let mut app = app_with_lines(3);

    app.apply(Action::PageUp);

    assert_eq!(app.scroll(), Scroll::At(0));
    assert_eq!(app.visible_range(), 0..3);
}

#[test]
fn selecting_another_command_resets_scroll() {
    let mut app = app_with_lines(20);
    app.apply(Action::PageUp);

    app.apply(Action::SelectNext);

    assert_eq!(app.scroll(), Scroll::Follow);
}

#[test]
fn zero_height_does_not_panic() {
    let mut app = app_with_lines(20);
    app.set_output_height(0);

    app.apply(Action::PageUp);
    app.apply(Action::PageDown);

    assert_eq!(app.visible_range().len(), 0);
}
