use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::*;

fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn key(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

/// イベントループと同じ手順（イベント → Action → 状態）で 1 件処理する。
fn feed(app: &mut App, event: &Event) {
    if let Some(action) = action_for(event) {
        app.apply(action);
    }
}

fn quits_on(event: &Event) -> bool {
    let mut app = App::new("runs test");
    feed(&mut app, event);
    app.should_quit()
}

#[test]
fn initial_state_is_running() {
    let app = App::new("runs test");

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
        press(KeyCode::Enter, KeyModifiers::NONE),
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
