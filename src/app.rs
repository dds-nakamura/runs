//! 状態（`App`）と更新（イベント → `Action` → 状態、ランナーの通知 → 状態）。端末にもプロセスにも触らない。

use std::ops::Range;

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::config::Config;
use crate::output::{self, OutputBuffer};
use crate::runner::{CommandId, RunnerEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    Idle,
    Running,
    Exited { code: Option<i32> },
    Stopped,
    SpawnFailed,
}

pub struct CommandView {
    name: String,
    state: CommandState,
    output: OutputBuffer,
    stop_requested: bool,
}

impl CommandView {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn state(&self) -> CommandState {
        self.state
    }

    pub fn output(&self) -> &OutputBuffer {
        &self.output
    }
}

/// 出力欄の表示位置。`At(n)` は先頭から n 行目を最上段に出す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Follow,
    At(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    SelectPrev,
    SelectNext,
    Run,
    Stop,
    PageUp,
    PageDown,
    ScrollToEnd,
}

/// `App` が `tui` に頼むこと。`App` 自身はプロセスに触らない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Start(CommandId),
    Stop(CommandId),
}

pub struct App {
    title: String,
    commands: Vec<CommandView>,
    selected: usize,
    scroll: Scroll,
    /// 出力欄の高さ（行）。`tui` が描画のたびに `ui::layout` の結果を渡す
    output_height: usize,
    should_quit: bool,
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
    // raw mode では Ctrl+C がシグナルにならずキーとして届く。
    // Caps Lock 中は `q` が `Q` になって効かないので、こちらは大文字と他の修飾キーも受ける
    if matches!(key.code, KeyCode::Char('c' | 'C')) && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        return Some(Action::Quit);
    }
    if key.modifiers != KeyModifiers::NONE {
        return None;
    }
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SelectPrev),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SelectNext),
        KeyCode::Enter => Some(Action::Run),
        KeyCode::Char('s') => Some(Action::Stop),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::End => Some(Action::ScrollToEnd),
        _ => None,
    }
}

impl App {
    pub fn new(title: impl Into<String>, config: &Config) -> Self {
        Self {
            title: title.into(),
            commands: config
                .commands
                .iter()
                .map(|spec| CommandView {
                    name: spec.name.clone(),
                    state: CommandState::Idle,
                    output: OutputBuffer::new(output::DEFAULT_LIMIT),
                    stop_requested: false,
                })
                .collect(),
            selected: 0,
            scroll: Scroll::Follow,
            output_height: 0,
            should_quit: false,
        }
    }

    pub fn apply(&mut self, action: Action) -> Option<Effect> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::SelectPrev => self.select(self.selected.saturating_sub(1)),
            Action::SelectNext => self.select(self.selected.saturating_add(1)),
            Action::Run => {
                let id = self.selected;
                let command = self.commands.get_mut(id)?;
                if command.state == CommandState::Running {
                    return None;
                }
                // 起動に失敗すれば SpawnFailed が届いて戻るので、先に実行中にしてしまう
                command.state = CommandState::Running;
                command.stop_requested = false;
                command.output.clear();
                self.scroll = Scroll::Follow;
                return Some(Effect::Start(id));
            }
            Action::Stop => {
                let id = self.selected;
                let command = self.commands.get_mut(id)?;
                if command.state != CommandState::Running {
                    return None;
                }
                // 状態は Exited の通知で変える。それまでは実行中のまま
                command.stop_requested = true;
                return Some(Effect::Stop(id));
            }
            Action::PageUp => {
                let first = self.visible_range().start;
                self.scroll = Scroll::At(first.saturating_sub(self.output_height));
            }
            Action::PageDown => {
                if let Scroll::At(first) = self.scroll {
                    let next = first.saturating_add(self.output_height);
                    self.scroll = if next >= self.follow_start() {
                        Scroll::Follow
                    } else {
                        Scroll::At(next)
                    };
                }
            }
            Action::ScrollToEnd => self.scroll = Scroll::Follow,
        }
        None
    }

    pub fn on_runner_event(&mut self, event: RunnerEvent) {
        match event {
            RunnerEvent::Started { id } => {
                // Run で先に実行中にしているが、通知が正なので合わせる
                if let Some(command) = self.commands.get_mut(id) {
                    command.state = CommandState::Running;
                }
            }
            RunnerEvent::Output { id, bytes } => {
                if let Some(command) = self.commands.get_mut(id) {
                    command.output.push_raw(&bytes);
                }
            }
            RunnerEvent::Exited { id, status } => {
                if let Some(command) = self.commands.get_mut(id) {
                    // 停止を頼んだ後の終了コードは taskkill / シグナル由来なので見せない
                    command.state = match status.code() {
                        Some(code) if !command.stop_requested => {
                            CommandState::Exited { code: Some(code) }
                        }
                        _ => CommandState::Stopped,
                    };
                    command.stop_requested = false;
                }
            }
            RunnerEvent::SpawnFailed { id, message } => {
                if let Some(command) = self.commands.get_mut(id) {
                    command.state = CommandState::SpawnFailed;
                    command.stop_requested = false;
                    command.output.push_raw(message.as_bytes());
                }
            }
        }
    }

    fn select(&mut self, index: usize) {
        let last = self.commands.len().saturating_sub(1);
        let index = index.min(last);
        if index != self.selected {
            self.selected = index;
            self.scroll = Scroll::Follow;
        }
    }

    /// 追従時に最上段に出る行。
    fn follow_start(&self) -> usize {
        let total = self.selected_command().map_or(0, |c| c.output.len());
        total.saturating_sub(self.output_height)
    }

    pub fn set_output_height(&mut self, height: u16) {
        self.output_height = usize::from(height);
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn commands(&self) -> &[CommandView] {
        &self.commands
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_command(&self) -> Option<&CommandView> {
        self.commands.get(self.selected)
    }

    /// テスト用。描画は `visible_range` を使う
    #[cfg(test)]
    pub fn scroll(&self) -> Scroll {
        self.scroll
    }

    /// 選択中のコマンドの出力のうち、いま出力欄に見える行の範囲。
    pub fn visible_range(&self) -> Range<usize> {
        let total = self.selected_command().map_or(0, |c| c.output.len());
        let start = match self.scroll {
            Scroll::Follow => self.follow_start(),
            Scroll::At(first) => first.min(self.follow_start()),
        };
        let end = start.saturating_add(self.output_height).min(total);
        start..end
    }
}

#[cfg(test)]
mod tests;
