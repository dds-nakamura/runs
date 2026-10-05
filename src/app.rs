//! 状態（`App`）と更新（イベント → `Action` → 状態）。端末には触らない。

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

pub struct App {
    title: String,
    should_quit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
}

/// キーバインドはここだけに書く。
pub fn action_for(event: &Event) -> Option<Action> {
    let Event::Key(key) = event else {
        return None;
    };
    // Windows では Press と Release が両方届く。Press 以外を通すと 1 回の打鍵が 2 回処理される
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match key.code {
        KeyCode::Char('q') if key.modifiers == KeyModifiers::NONE => Some(Action::Quit),
        // raw mode では Ctrl+C がシグナルにならずキーとして届く。
        // Caps Lock 中は `q` が `Q` になって効かないので、こちらは大文字と他の修飾キーも受ける
        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(Action::Quit)
        }
        _ => None,
    }
}

impl App {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            should_quit: false,
        }
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

#[cfg(test)]
mod tests;
