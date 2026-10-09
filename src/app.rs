//! 状態（`App`）と更新（イベント → `Action` → 状態、ランナーの通知 → 状態）。端末にもプロセスにもファイルにも触らない。
//!
//! 現在時刻は `set_now` で外から受け取る（この中で `Instant::now()` を呼ばない。テストで時間を進められるように）。

use std::ops::Range;
use std::time::{Duration, Instant};

use encoding_rs::Encoding;
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::config::{CommandSpec, Config};
use crate::gh::{self, GhStatus};
use crate::output::{self, OutputBuffer};
use crate::runner::{GhEvent, RunId, RunnerEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandState {
    Idle,
    Running,
    Exited { code: Option<i32> },
    Stopped,
    SpawnFailed,
}

pub struct CommandView {
    spec: CommandSpec,
    /// 画面に出す名前（設定の値を無害化したもの）
    display_name: String,
    state: CommandState,
    output: OutputBuffer,
    /// 現在または直前の実行。これと違う実行の通知は無視する
    run: Option<RunId>,
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    stop_requested: bool,
    /// 停止が終わったら再実行する
    restart_pending: bool,
}

impl CommandView {
    fn new(spec: &CommandSpec, encoding: Option<&'static Encoding>) -> Self {
        Self {
            display_name: output::sanitize(&spec.name),
            spec: spec.clone(),
            state: CommandState::Idle,
            output: OutputBuffer::with_fallback(output::DEFAULT_LIMIT, encoding),
            run: None,
            started_at: None,
            finished_at: None,
            stop_requested: false,
            restart_pending: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.display_name
    }

    pub fn state(&self) -> CommandState {
        self.state
    }

    pub fn output(&self) -> &OutputBuffer {
        &self.output
    }

    /// テスト用（描画では使わない）
    #[cfg(test)]
    pub fn spec(&self) -> &CommandSpec {
        &self.spec
    }

    /// テスト用（描画では使わない）
    #[cfg(test)]
    pub fn run(&self) -> Option<RunId> {
        self.run
    }

    /// テスト用（描画では使わない）
    #[cfg(test)]
    pub fn restart_pending(&self) -> bool {
        self.restart_pending
    }

    /// 実行中なら開始からの経過。
    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        match (self.state, self.started_at) {
            (CommandState::Running, Some(started)) => Some(now.saturating_duration_since(started)),
            _ => None,
        }
    }

    /// 終わっていれば、終わってからの経過。
    pub fn ago(&self, now: Instant) -> Option<Duration> {
        self.finished_at
            .map(|finished| now.saturating_duration_since(finished))
    }

    /// 終わっていれば、所要時間。
    pub fn took(&self) -> Option<Duration> {
        match (self.started_at, self.finished_at) {
            (Some(started), Some(finished)) => Some(finished.saturating_duration_since(started)),
            _ => None,
        }
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
    Reload,
    /// `gh` で PR と CI の状態を取り、右側を status 区画にする
    FetchGh,
    /// 右側の区画を output ↔ status で切り替える（取得はしない）
    TogglePane,
}

/// `App` が `tui` に頼むこと。`App` 自身はプロセスにもファイルにも触らない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Start {
        run: RunId,
        spec: CommandSpec,
    },
    Stop(RunId),
    Reload,
    /// `gh` を実行する。実行先のディレクトリは `tui` が設定ファイルの場所から決める
    FetchGh,
}

/// 右側の区画に出すもの。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightPane {
    Output,
    Status,
}

/// `gh` の取得の状態。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhPanel {
    NotFetched,
    Fetching {
        since: Instant,
    },
    /// 最後に取得した結果と、取得した時刻
    Ready {
        status: GhStatus,
        at: Instant,
    },
}

pub struct App {
    title: String,
    commands: Vec<CommandView>,
    selected: usize,
    scroll: Scroll,
    /// 出力欄の高さ（行）。`tui` が描画のたびに `ui::layout` の結果を渡す
    output_height: usize,
    should_quit: bool,
    /// `set_now` で更新される現在時刻
    now: Instant,
    /// 次に採番する実行番号
    next_run: RunId,
    /// 最下行に出す通知。次の `Action` で消える
    notice: Option<String>,
    /// 右側の区画に出すもの
    pane: RightPane,
    /// `gh` の取得の状態
    gh: GhPanel,
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
        KeyCode::Char('r') => Some(Action::Reload),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::End => Some(Action::ScrollToEnd),
        KeyCode::Char('g') => Some(Action::FetchGh),
        KeyCode::Tab => Some(Action::TogglePane),
        _ => None,
    }
}

impl App {
    pub fn new(title: impl Into<String>, config: &Config, now: Instant) -> Self {
        Self {
            title: title.into(),
            commands: config
                .commands
                .iter()
                .map(|spec| CommandView::new(spec, config.encoding))
                .collect(),
            selected: 0,
            scroll: Scroll::Follow,
            output_height: 0,
            should_quit: false,
            now,
            next_run: 1,
            notice: None,
            pane: RightPane::Output,
            gh: GhPanel::NotFetched,
        }
    }

    /// 現在時刻を渡す。`tui` が描画の直前に呼ぶ
    pub fn set_now(&mut self, now: Instant) {
        self.now = now;
    }

    pub fn apply(&mut self, action: Action) -> Vec<Effect> {
        // 通知はどの操作でも消える
        self.notice = None;
        match action {
            Action::Quit => self.should_quit = true,
            Action::SelectPrev => self.select(self.selected.saturating_sub(1)),
            Action::SelectNext => self.select(self.selected.saturating_add(1)),
            Action::Run => {
                let index = self.selected;
                let Some(command) = self.commands.get_mut(index) else {
                    return Vec::new();
                };
                if command.state == CommandState::Running {
                    // 実行中は「停止して再実行」。終了の通知を受けたときに起動し直す
                    if command.restart_pending {
                        return Vec::new();
                    }
                    command.stop_requested = true;
                    command.restart_pending = true;
                    return command.run.map(Effect::Stop).into_iter().collect();
                }
                return vec![self.start_run(index)];
            }
            Action::Stop => {
                let Some(command) = self.commands.get_mut(self.selected) else {
                    return Vec::new();
                };
                if command.state != CommandState::Running {
                    return Vec::new();
                }
                // 状態は Exited の通知で変える。それまでは実行中のまま。再実行の予約は取り消す
                command.stop_requested = true;
                command.restart_pending = false;
                return command.run.map(Effect::Stop).into_iter().collect();
            }
            Action::Reload => return vec![Effect::Reload],
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
            Action::FetchGh => {
                self.pane = RightPane::Status;
                if matches!(self.gh, GhPanel::Fetching { .. }) {
                    // 二重に起動しない。進行中の結果が届く
                    self.set_notice("already fetching");
                    return Vec::new();
                }
                self.gh = GhPanel::Fetching { since: self.now };
                return vec![Effect::FetchGh];
            }
            Action::TogglePane => {
                self.pane = match self.pane {
                    RightPane::Output => RightPane::Status,
                    RightPane::Status => RightPane::Output,
                };
            }
        }
        Vec::new()
    }

    /// `gh` の取得結果を取り込む。経過時間の基準は取得側が付けた壁時計（`now_unix`）で、表示の「何秒前」は `set_now` の時刻
    pub fn on_gh_event(&mut self, event: GhEvent) {
        let GhEvent::Fetched { pr, run, now_unix } = event;
        self.gh = GhPanel::Ready {
            status: gh::status_from(&pr, &run, now_unix),
            at: self.now,
        };
    }

    pub fn pane(&self) -> RightPane {
        self.pane
    }

    pub fn gh_panel(&self) -> &GhPanel {
        &self.gh
    }

    pub fn on_runner_event(&mut self, event: RunnerEvent) -> Vec<Effect> {
        let run = match &event {
            RunnerEvent::Started { run }
            | RunnerEvent::Output { run, .. }
            | RunnerEvent::Exited { run, .. }
            | RunnerEvent::SpawnFailed { run, .. }
            | RunnerEvent::StopFailed { run, .. } => *run,
        };
        // 古い実行や、再読み込みで消えたコマンドの通知は捨てる
        let Some(index) = self.commands.iter().position(|c| c.run == Some(run)) else {
            return Vec::new();
        };
        let now = self.now;
        let selected = index == self.selected;
        let Some(command) = self.commands.get_mut(index) else {
            return Vec::new();
        };
        match event {
            RunnerEvent::Started { .. } => {
                // Run で先に実行中にしているが、通知が正なので合わせる
                command.state = CommandState::Running;
            }
            RunnerEvent::Output { bytes, .. } => {
                let dropped_before = command.output.dropped();
                command.output.push_raw(&bytes);
                let dropped = command.output.dropped().saturating_sub(dropped_before);
                // 上限に達して先頭が捨てられたら、遡っている表示位置を同じだけ戻して、見えている行を動かさない
                if let Scroll::At(first) = self.scroll
                    && selected
                    && dropped > 0
                {
                    self.scroll = Scroll::At(first.saturating_sub(dropped));
                }
            }
            RunnerEvent::Exited { status, .. } => {
                if command.restart_pending {
                    return vec![self.start_run(index)];
                }
                // 停止を頼んだ後の終了コードは taskkill / シグナル由来なので見せない
                command.state = match status.code() {
                    Some(code) if !command.stop_requested => {
                        CommandState::Exited { code: Some(code) }
                    }
                    _ => CommandState::Stopped,
                };
                command.finished_at = Some(now);
                command.stop_requested = false;
            }
            RunnerEvent::SpawnFailed { message, .. } => {
                command.state = CommandState::SpawnFailed;
                command.finished_at = Some(now);
                command.stop_requested = false;
                command.restart_pending = false;
                command.output.push_raw(message.as_bytes());
            }
            RunnerEvent::StopFailed { message, .. } => {
                // 状態は変えず、理由だけ出力欄に出す。stop_requested と restart_pending は残す
                // （最終手段の Child::kill で終わったとき、終了コードではなく「停止」と見せ、再実行の予約も生かす）
                command
                    .output
                    .push_raw(format!("runs: failed to stop: {message}").as_bytes());
            }
        }
        Vec::new()
    }

    /// 設定を読み直した結果で一覧を置き換える。同じ名前のコマンドは状態・出力・実行中のプロセスを引き継ぐ。
    /// 消えたコマンドが実行中なら `Stop` を返す
    pub fn replace_config(&mut self, config: &Config) -> Vec<Effect> {
        let mut old = std::mem::take(&mut self.commands);
        let selected_name = old.get(self.selected).map(|c| c.spec.name.clone());

        self.commands = config
            .commands
            .iter()
            .map(
                |spec| match old.iter().position(|c| c.spec.name == spec.name) {
                    Some(position) => {
                        // position は old の添字なので範囲内
                        let mut view = old.remove(position);
                        view.spec = spec.clone();
                        view.display_name = output::sanitize(&spec.name);
                        view.output.set_fallback(config.encoding);
                        view
                    }
                    None => CommandView::new(spec, config.encoding),
                },
            )
            .collect();

        self.selected = selected_name
            .and_then(|name| self.commands.iter().position(|c| c.spec.name == name))
            .unwrap_or(0);
        self.scroll = Scroll::Follow;

        old.into_iter()
            .filter(|c| c.state == CommandState::Running)
            .filter_map(|c| c.run.map(Effect::Stop))
            .collect()
    }

    /// 最下行に出す通知。制御文字を除いた 1 行目だけを保持する
    pub fn set_notice(&mut self, text: impl Into<String>) {
        let text: String = text.into();
        let first_line = text.lines().next().unwrap_or("");
        self.notice = Some(output::sanitize(first_line));
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 通知を消す。`tui` がキー入力のたびに呼ぶ（割り当てのないキーでも消える）
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// 時間の表示があるか（実行中、または一度でも終わったコマンドがある。status 区画を見ていて取得の経過が出ている）。
    /// あれば毎秒描き直す
    pub fn needs_tick(&self) -> bool {
        self.commands
            .iter()
            .any(|c| c.state == CommandState::Running || c.finished_at.is_some())
            || (self.pane == RightPane::Status && self.gh != GhPanel::NotFetched)
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

    pub fn now(&self) -> Instant {
        self.now
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

    /// 新しい実行番号で起動の状態にし、`Start` を返す。
    fn start_run(&mut self, index: usize) -> Effect {
        let run = self.next_run;
        self.next_run = self.next_run.saturating_add(1);
        let now = self.now;
        if index == self.selected {
            self.scroll = Scroll::Follow;
        }
        // 呼び出し側が範囲内の index を渡す。万一外れていても Effect は返す（Runner は知らない run を無視しない
        // が、通知の宛先が無いだけで害は無い）
        let spec = match self.commands.get_mut(index) {
            Some(command) => {
                // 起動に失敗すれば SpawnFailed が届いて戻るので、先に実行中にしてしまう
                command.run = Some(run);
                command.state = CommandState::Running;
                command.started_at = Some(now);
                command.finished_at = None;
                command.stop_requested = false;
                command.restart_pending = false;
                command.output.clear();
                command.spec.clone()
            }
            None => CommandSpec {
                name: String::new(),
                command: String::new(),
                cwd: std::path::PathBuf::new(),
            },
        };
        Effect::Start { run, spec }
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
}

#[cfg(test)]
mod tests;
