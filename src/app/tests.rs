use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::*;
use crate::config::{CommandSpec, Config};
use crate::gh::GhError;
use crate::runner::{Capture, GhEvent, RunnerEvent};

fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

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

/// 3 コマンド、出力欄の高さ 5 行。時刻は `app.now()` を基準に `set_now` で進める。
fn app() -> App {
    let mut app = App::new(
        "runs test",
        &config(&["build", "test", "serve"]),
        Instant::now(),
    );
    app.set_output_height(5);
    app
}

/// イベントループと同じ手順（イベント → Action → 状態）で 1 件処理する。
fn feed(app: &mut App, event: &Event) -> Vec<Effect> {
    action_for(event)
        .map(|action| app.apply(action))
        .unwrap_or_default()
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

fn output(run: RunId, text: &str) -> RunnerEvent {
    RunnerEvent::Output {
        run,
        bytes: text.as_bytes().to_vec(),
    }
}

fn exited(run: RunId, code: i32) -> RunnerEvent {
    RunnerEvent::Exited {
        run,
        status: exit_status(code),
    }
}

fn selected_lines(app: &App) -> Vec<&str> {
    app.selected_command()
        .map(|c| c.output().lines().collect())
        .unwrap_or_default()
}

fn starts(effects: &[Effect]) -> Vec<RunId> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Start { run, .. } => Some(*run),
            _ => None,
        })
        .collect()
}

/// 選択中のコマンドを実行し、採番された run を返す。
fn run_selected(app: &mut App) -> RunId {
    let effects = app.apply(Action::Run);
    match effects.as_slice() {
        [Effect::Start { run, .. }] => *run,
        other => panic!("Start が 1 つ返るはずが {other:?}"),
    }
}

fn advance(app: &mut App, seconds: u64) {
    app.set_now(app.now() + Duration::from_secs(seconds));
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
        (KeyCode::Char('r'), Action::Reload),
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

    assert!(app.apply(Action::SelectPrev).is_empty());
    assert_eq!(app.selected(), 0);

    app.apply(Action::SelectNext);
    app.apply(Action::SelectNext);
    assert_eq!(app.selected(), 2);

    app.apply(Action::SelectNext);
    assert_eq!(app.selected(), 2);

    app.apply(Action::SelectPrev);
    assert_eq!(app.selected(), 1);
}

#[test]
fn command_names_are_sanitized() {
    let app = App::new("t", &config(&["evil\u{1b}]0;x\u{7}name"]), Instant::now());

    assert_eq!(app.commands()[0].name(), "evilname");
}

// --- 実行と停止 ------------------------------------------------------------------

#[test]
fn run_marks_running_and_requests_start() {
    let mut app = app();
    app.apply(Action::SelectNext);

    let effects = app.apply(Action::Run);

    assert_eq!(
        effects,
        [Effect::Start {
            run: 1,
            spec: app.commands()[1].spec().clone()
        }]
    );
    assert_eq!(app.commands()[1].state(), CommandState::Running);
    assert_eq!(app.commands()[1].run(), Some(1));
    assert_eq!(app.commands()[0].state(), CommandState::Idle);
}

#[test]
fn run_allocates_increasing_run_ids() {
    let mut app = app();

    let first = run_selected(&mut app);
    app.on_runner_event(exited(first, 0));
    let second = run_selected(&mut app);
    app.apply(Action::SelectNext);
    let third = run_selected(&mut app);

    assert_eq!((first, second, third), (1, 2, 3));
}

#[test]
fn stop_requests_only_when_running() {
    let mut app = app();

    assert!(app.apply(Action::Stop).is_empty());

    let run = run_selected(&mut app);
    assert_eq!(app.apply(Action::Stop), [Effect::Stop(run)]);
    // 停止要求中も、通知が来るまでは実行中のまま
    assert_eq!(app.commands()[0].state(), CommandState::Running);
}

#[test]
fn exited_sets_code() {
    let mut app = app();
    let run = run_selected(&mut app);

    app.on_runner_event(exited(run, 3));

    assert_eq!(
        app.commands()[0].state(),
        CommandState::Exited { code: Some(3) }
    );
}

#[test]
fn exited_after_stop_request_is_stopped() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Stop);

    // Windows の taskkill は終了コード 1 で終わらせる。停止を頼んだのだから「停止」と見せる
    app.on_runner_event(exited(run, 1));

    assert_eq!(app.commands()[0].state(), CommandState::Stopped);
}

#[cfg(unix)]
#[test]
fn signal_exit_is_stopped() {
    use std::os::unix::process::ExitStatusExt;
    let mut app = app();
    let run = run_selected(&mut app);

    // 停止を頼んでいなくても、シグナルで終わったら「停止」
    app.on_runner_event(RunnerEvent::Exited {
        run,
        status: ExitStatus::from_raw(15),
    });

    assert_eq!(app.commands()[0].state(), CommandState::Stopped);
}

#[test]
fn spawn_failed_sets_state_and_message() {
    let mut app = app();
    let run = run_selected(&mut app);

    app.on_runner_event(RunnerEvent::SpawnFailed {
        run,
        message: "failed to start \"zsh\": not found".to_owned(),
    });

    assert_eq!(app.commands()[0].state(), CommandState::SpawnFailed);
    assert_eq!(selected_lines(&app), ["failed to start \"zsh\": not found"]);
}

#[test]
fn stop_failed_keeps_running_and_shows_reason() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Stop);

    app.on_runner_event(RunnerEvent::StopFailed {
        run,
        message: "failed to run kill: not found".to_owned(),
    });

    assert_eq!(app.commands()[0].state(), CommandState::Running);
    assert_eq!(
        selected_lines(&app),
        ["runs: failed to stop: failed to run kill: not found"]
    );
}

#[test]
fn rerun_clears_output() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.on_runner_event(output(run, "first run"));
    app.on_runner_event(exited(run, 0));

    let effects = app.apply(Action::Run);

    assert_eq!(starts(&effects), [2]);
    assert_eq!(app.commands()[0].state(), CommandState::Running);
    assert!(selected_lines(&app).is_empty());
}

#[test]
fn output_goes_to_its_command() {
    let mut app = app();
    app.apply(Action::SelectNext);
    let test_run = run_selected(&mut app);
    app.apply(Action::SelectNext);
    let serve_run = run_selected(&mut app);
    app.apply(Action::SelectPrev);
    app.apply(Action::SelectPrev);

    app.on_runner_event(output(test_run, "from test"));
    app.on_runner_event(output(serve_run, "from serve"));

    assert!(selected_lines(&app).is_empty());
    app.apply(Action::SelectNext);
    assert_eq!(selected_lines(&app), ["from test"]);
    app.apply(Action::SelectNext);
    assert_eq!(selected_lines(&app), ["from serve"]);
}

#[test]
fn events_for_stale_run_are_ignored() {
    let mut app = app();
    let old = run_selected(&mut app);
    app.on_runner_event(exited(old, 0));
    let new = run_selected(&mut app);

    // 前回の実行の遅れた出力・終了は、いまの実行に混ざらない
    app.on_runner_event(output(old, "late line"));
    app.on_runner_event(exited(old, 7));
    app.on_runner_event(output(99, "nowhere"));

    assert_eq!(app.commands()[0].run(), Some(new));
    assert_eq!(app.commands()[0].state(), CommandState::Running);
    assert!(selected_lines(&app).is_empty());
}

#[test]
fn output_is_sanitized() {
    let mut app = app();
    let run = run_selected(&mut app);

    app.on_runner_event(output(run, "\x1b[31mred\x1b[0m\x07"));

    assert_eq!(selected_lines(&app), ["red?"]);
}

// --- 時刻 ------------------------------------------------------------------------

#[test]
fn elapsed_while_running() {
    let mut app = app();
    assert_eq!(app.commands()[0].elapsed(app.now()), None);

    run_selected(&mut app);
    advance(&mut app, 12);

    assert_eq!(
        app.commands()[0].elapsed(app.now()),
        Some(Duration::from_secs(12))
    );
    assert_eq!(app.commands()[0].ago(app.now()), None);
    assert_eq!(app.commands()[0].took(), None);
}

#[test]
fn ago_and_took_after_exit() {
    let mut app = app();
    let run = run_selected(&mut app);
    advance(&mut app, 72);
    app.on_runner_event(exited(run, 0));
    advance(&mut app, 180);

    let command = &app.commands()[0];
    assert_eq!(command.elapsed(app.now()), None);
    assert_eq!(command.took(), Some(Duration::from_secs(72)));
    assert_eq!(command.ago(app.now()), Some(Duration::from_secs(180)));
}

#[test]
fn needs_tick_only_after_first_run() {
    let mut app = app();
    assert!(!app.needs_tick());

    let run = run_selected(&mut app);
    assert!(app.needs_tick());

    app.on_runner_event(exited(run, 0));
    assert!(app.needs_tick());
}

// --- 停止して再実行 ----------------------------------------------------------------

#[test]
fn enter_on_running_requests_stop_and_restart() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.on_runner_event(output(run, "old output"));

    let effects = app.apply(Action::Run);

    assert_eq!(effects, [Effect::Stop(run)]);
    assert!(app.commands()[0].restart_pending());
    assert_eq!(app.commands()[0].state(), CommandState::Running);
    // 終了が届くまで出力は残る
    assert_eq!(selected_lines(&app), ["old output"]);
}

#[test]
fn restart_starts_new_run_after_exit() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.on_runner_event(output(run, "old output"));
    app.apply(Action::Run);
    advance(&mut app, 3);

    let effects = app.on_runner_event(exited(run, 1));

    assert_eq!(starts(&effects), [run + 1]);
    let command = &app.commands()[0];
    assert_eq!(command.run(), Some(run + 1));
    assert_eq!(command.state(), CommandState::Running);
    assert!(!command.restart_pending());
    assert_eq!(command.elapsed(app.now()), Some(Duration::ZERO));
    assert!(selected_lines(&app).is_empty());
}

#[test]
fn second_enter_while_restarting_is_noop() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Run);

    assert!(app.apply(Action::Run).is_empty());

    let effects = app.on_runner_event(exited(run, 0));
    assert_eq!(starts(&effects).len(), 1);
}

#[test]
fn stop_cancels_pending_restart() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Run);

    let effects = app.apply(Action::Stop);
    assert_eq!(effects, [Effect::Stop(run)]);
    assert!(!app.commands()[0].restart_pending());

    let effects = app.on_runner_event(exited(run, 1));
    assert!(effects.is_empty());
    assert_eq!(app.commands()[0].state(), CommandState::Stopped);
}

#[test]
fn stop_failed_keeps_pending_restart() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Run);

    app.on_runner_event(RunnerEvent::StopFailed {
        run,
        message: "boom".to_owned(),
    });

    assert!(app.commands()[0].restart_pending());
    let effects = app.on_runner_event(exited(run, 1));
    assert_eq!(starts(&effects).len(), 1);
}

// --- 再読み込み ------------------------------------------------------------------

#[test]
fn reload_action_returns_reload_effect() {
    let mut app = app();

    assert_eq!(app.apply(Action::Reload), [Effect::Reload]);
}

#[test]
fn replace_config_keeps_same_names() {
    let mut app = app();
    app.apply(Action::SelectNext);
    let run = run_selected(&mut app);
    app.on_runner_event(output(run, "kept"));

    // 順序を変え、test の command を書き換え、build を消し、deploy を足す
    let mut new = config(&["serve", "test", "deploy"]);
    new.commands[1].command = "cargo test --all".to_owned();
    let effects = app.replace_config(&new);

    assert!(effects.is_empty());
    let names: Vec<_> = app.commands().iter().map(CommandView::name).collect();
    assert_eq!(names, ["serve", "test", "deploy"]);
    let test = &app.commands()[1];
    assert_eq!(test.state(), CommandState::Running);
    assert_eq!(test.run(), Some(run));
    assert_eq!(test.spec().command, "cargo test --all");
    assert_eq!(test.output().lines().collect::<Vec<_>>(), ["kept"]);
    assert_eq!(app.commands()[2].state(), CommandState::Idle);
}

#[test]
fn replace_config_stops_removed_running() {
    let mut app = app();
    let build_run = run_selected(&mut app);
    app.apply(Action::SelectNext);
    let test_run = run_selected(&mut app);
    app.on_runner_event(exited(test_run, 0));

    // build は実行中のまま消える → Stop。test は終わっているので何も無い
    let effects = app.replace_config(&config(&["serve"]));

    assert_eq!(effects, [Effect::Stop(build_run)]);
    assert_eq!(app.commands().len(), 1);
    // 消えたコマンドの通知は無視される
    assert!(app.on_runner_event(exited(build_run, 1)).is_empty());
}

#[test]
fn replace_config_moves_selection() {
    let mut app = app();
    app.apply(Action::SelectNext);
    app.apply(Action::SelectNext);
    assert_eq!(app.commands()[app.selected()].name(), "serve");

    app.replace_config(&config(&["serve", "build"]));
    assert_eq!(app.selected(), 0);
    assert_eq!(app.commands()[0].name(), "serve");

    // 選択中が消えたら先頭
    app.replace_config(&config(&["x", "y"]));
    assert_eq!(app.selected(), 0);
}

#[test]
fn replace_config_drops_pending_restart_of_removed() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.apply(Action::Run);
    assert!(app.commands()[0].restart_pending());

    let effects = app.replace_config(&config(&["test"]));

    assert_eq!(effects, [Effect::Stop(run)]);
    // 停止の通知が来ても再実行しない
    assert!(app.on_runner_event(exited(run, 1)).is_empty());
}

#[test]
fn replace_config_with_empty_list_selects_nothing() {
    let mut app = app();

    app.replace_config(&config(&[]));

    assert_eq!(app.selected(), 0);
    assert!(app.selected_command().is_none());
    assert!(app.apply(Action::Run).is_empty());
}

// --- 通知 ------------------------------------------------------------------------

#[test]
fn notice_is_cleared_by_next_action() {
    let mut app = app();
    assert_eq!(app.notice(), None);

    app.set_notice("reloaded runs.toml (3 commands)");
    assert_eq!(app.notice(), Some("reloaded runs.toml (3 commands)"));

    app.apply(Action::SelectNext);
    assert_eq!(app.notice(), None);
}

#[test]
fn notice_is_sanitized_to_one_line() {
    let mut app = app();

    app.set_notice("reload failed: invalid \x1b[31mruns.toml\x1b[0m\nTOML parse error at line 1");

    assert_eq!(app.notice(), Some("reload failed: invalid runs.toml"));
}

// --- 文字コード（#8） --------------------------------------------------------------

/// 「からの応答」の CP932 表現。
const KARANO_OUTOU_CP932: &[u8] = &[0x82, 0xa9, 0x82, 0xe7, 0x82, 0xcc, 0x89, 0x9e, 0x93, 0x9a];

#[test]
fn config_encoding_is_used_for_output() {
    let mut config = config(&["ping"]);
    config.encoding = Some(encoding_rs::SHIFT_JIS);
    let mut app = App::new("t", &config, Instant::now());
    let run = run_selected(&mut app);

    app.on_runner_event(RunnerEvent::Output {
        run,
        bytes: KARANO_OUTOU_CP932.to_vec(),
    });

    assert_eq!(selected_lines(&app), ["からの応答"]);
}

#[test]
fn reload_changes_encoding_for_following_lines() {
    let mut app = app();
    let run = run_selected(&mut app);
    app.on_runner_event(RunnerEvent::Output {
        run,
        bytes: KARANO_OUTOU_CP932.to_vec(),
    });

    let mut new = config(&["build", "test", "serve"]);
    new.encoding = Some(encoding_rs::SHIFT_JIS);
    app.replace_config(&new);
    app.on_runner_event(RunnerEvent::Output {
        run,
        bytes: KARANO_OUTOU_CP932.to_vec(),
    });

    // 溜まった行はそのまま（化けたまま）、以後の行は読み直される
    let lines = selected_lines(&app);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains('\u{fffd}'), "{lines:?}");
    assert_eq!(lines[1], "からの応答");
}

// --- スクロール ------------------------------------------------------------------

fn app_with_lines(count: usize) -> (App, RunId) {
    let mut app = app();
    let run = run_selected(&mut app);
    for i in 1..=count {
        app.on_runner_event(output(run, &format!("line {i}")));
    }
    (app, run)
}

#[test]
fn follows_tail_by_default() {
    let (app, _) = app_with_lines(20);

    assert_eq!(app.scroll(), Scroll::Follow);
    // 高さ 5 なので 16..=20 行目が見える
    assert_eq!(app.visible_range(), 15..20);
}

#[test]
fn page_up_leaves_follow() {
    let (mut app, _) = app_with_lines(20);

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
    let (mut app, _) = app_with_lines(20);
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
    let (mut app, _) = app_with_lines(20);
    app.apply(Action::PageUp);

    app.apply(Action::ScrollToEnd);

    assert_eq!(app.scroll(), Scroll::Follow);
}

#[test]
fn new_output_does_not_move_while_scrolled() {
    let (mut app, run) = app_with_lines(20);
    app.apply(Action::PageUp);

    app.on_runner_event(output(run, "line 21"));

    assert_eq!(app.scroll(), Scroll::At(10));
    assert_eq!(app.visible_range(), 10..15);
}

#[test]
fn page_up_with_few_lines_stays_at_top() {
    let (mut app, _) = app_with_lines(3);

    app.apply(Action::PageUp);

    assert_eq!(app.scroll(), Scroll::At(0));
    assert_eq!(app.visible_range(), 0..3);
}

#[test]
fn selecting_another_command_resets_scroll() {
    let (mut app, _) = app_with_lines(20);
    app.apply(Action::PageUp);

    app.apply(Action::SelectNext);

    assert_eq!(app.scroll(), Scroll::Follow);
}

#[test]
fn zero_height_does_not_panic() {
    let (mut app, _) = app_with_lines(20);
    app.set_output_height(0);

    app.apply(Action::PageUp);
    app.apply(Action::PageDown);

    assert_eq!(app.visible_range().len(), 0);
}

#[test]
fn scroll_position_follows_dropped_lines() {
    let (mut app, run) = app_with_lines(crate::output::DEFAULT_LIMIT);
    app.apply(Action::PageUp);
    app.apply(Action::PageUp);
    let Scroll::At(first) = app.scroll() else {
        panic!("遡っているはず");
    };
    let visible: Vec<String> = selected_lines(&app)[app.visible_range()]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();

    // 上限に達した後の新しい行で先頭が捨てられても、見えている行は変わらない
    app.on_runner_event(output(run, "overflow 1"));
    app.on_runner_event(output(run, "overflow 2"));

    assert_eq!(app.scroll(), Scroll::At(first - 2));
    assert_eq!(selected_lines(&app)[app.visible_range()].to_vec(), visible);
}

// --- gh の状態表示 ------------------------------------------------------------------

/// 成功した `gh` 2 回分の結果。
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

#[test]
fn g_starts_fetch_and_shows_status_pane() {
    let mut app = app();
    assert_eq!(app.pane(), RightPane::Output);
    assert_eq!(*app.gh_panel(), GhPanel::NotFetched);

    let effects = feed(&mut app, &press(KeyCode::Char('g'), KeyModifiers::NONE));

    assert_eq!(effects, [Effect::FetchGh]);
    assert_eq!(app.pane(), RightPane::Status);
    assert_eq!(*app.gh_panel(), GhPanel::Fetching { since: app.now() });
}

#[test]
fn g_while_fetching_sets_notice_without_effect() {
    let mut app = app();
    app.apply(Action::FetchGh);
    app.apply(Action::TogglePane);

    assert!(app.apply(Action::FetchGh).is_empty());
    assert_eq!(app.notice(), Some("already fetching"));
    assert!(matches!(app.gh_panel(), GhPanel::Fetching { .. }));
    // 取得中でも status 区画には切り替わる
    assert_eq!(app.pane(), RightPane::Status);
}

#[test]
fn tab_toggles_pane_without_fetching() {
    let mut app = app();

    assert!(feed(&mut app, &press(KeyCode::Tab, KeyModifiers::NONE)).is_empty());
    assert_eq!(app.pane(), RightPane::Status);
    assert_eq!(*app.gh_panel(), GhPanel::NotFetched);

    feed(&mut app, &press(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.pane(), RightPane::Output);
}

#[test]
fn gh_event_makes_panel_ready_and_allows_refetch() {
    let mut app = app();
    app.apply(Action::FetchGh);
    app.set_now(app.now() + Duration::from_secs(3));

    app.on_gh_event(fetched(
        r#"[{"number": 15, "title": "t", "headRefName": "b"}]"#,
        "[]",
    ));

    match app.gh_panel() {
        GhPanel::Ready { status, at } => {
            assert_eq!(*at, app.now());
            assert_eq!(status.prs.as_ref().map(Vec::len), Ok(1));
            assert_eq!(status.runs, Ok(Vec::new()));
        }
        other => panic!("Ready のはずが {other:?}"),
    }
    // 取得後の g は再取得
    assert_eq!(app.apply(Action::FetchGh), [Effect::FetchGh]);
}

#[test]
fn gh_event_keeps_failed_side_as_error() {
    let mut app = app();
    app.apply(Action::FetchGh);
    let not_found = || Capture {
        spawn_error: Some(std::io::ErrorKind::NotFound),
        ..Capture::default()
    };

    app.on_gh_event(GhEvent::Fetched {
        pr: not_found(),
        run: not_found(),
        now_unix: 0,
    });

    let GhPanel::Ready { status, .. } = app.gh_panel() else {
        panic!("Ready のはず");
    };
    assert_eq!(status.prs, Err(GhError::NotFound));
    assert_eq!(status.runs, Err(GhError::NotFound));
}

#[test]
fn needs_tick_only_while_status_pane_shows_fetch_state() {
    let mut app = app();
    assert!(!app.needs_tick());
    // 取得前の status 区画には時間の表示が無い
    app.apply(Action::TogglePane);
    assert!(!app.needs_tick());
    app.apply(Action::FetchGh);
    assert!(app.needs_tick());
    // output に戻せば従来どおり
    app.apply(Action::TogglePane);
    assert!(!app.needs_tick());
    app.apply(Action::TogglePane);
    app.on_gh_event(fetched("[]", "[]"));
    assert!(app.needs_tick());
}

#[test]
fn reload_keeps_gh_panel_and_pane() {
    let mut app = app();
    app.apply(Action::FetchGh);
    app.on_gh_event(fetched("[]", "[]"));

    app.replace_config(&config(&["build"]));

    assert!(matches!(app.gh_panel(), GhPanel::Ready { .. }));
    assert_eq!(app.pane(), RightPane::Status);
}
