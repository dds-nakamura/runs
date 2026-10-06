//! 描画（状態 → 画面）。`TestBackend` にも描けるよう、実端末には触らない。

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use crate::app::{App, CommandState, CommandView};

const HELP: &str = "Up/Down select  Enter run  s stop  PgUp/PgDn/End scroll  q quit";
/// 状態の表記の最大幅（`exit 255`）。
const STATUS_WIDTH: usize = 8;
/// 選択中の印の幅（`> `）。
const MARK_WIDTH: usize = 2;
/// 一覧の幅の上限（画面幅に対する割合）。
const LIST_MAX_PERCENT: u32 = 40;

/// 画面の区画。`draw` と `tui::run`（出力欄の高さを `App` に渡す）の両方が使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub list: Rect,
    pub output: Rect,
    pub help: Rect,
}

/// 左に一覧、右に選択中の出力、最下行にキーの案内。
///
/// 一覧の幅は「印 + 名前の最大幅 + 空白 + 状態」、ただし画面幅の 40% まで。
pub fn layout(area: Rect, app: &App) -> Panes {
    let [main, help] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // u16 の掛け算は 65535 × 40 であふれるので u32 で計算する
    let cap = u16::try_from(u32::from(area.width) * LIST_MAX_PERCENT / 100).unwrap_or(u16::MAX);
    let wanted = u16::try_from(MARK_WIDTH + name_width(app) + 1 + STATUS_WIDTH).unwrap_or(u16::MAX);
    let [list, _gap, output] = Layout::horizontal([
        Constraint::Length(wanted.min(cap)),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(main);

    Panes { list, output, help }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let panes = layout(frame.area(), app);
    draw_list(frame, panes.list, app);
    draw_output(frame, panes.output, app);
    draw_help(frame, panes.help, app);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    let name_width = name_width(app);
    let lines: Vec<Line> = app
        .commands()
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let mark = if index == app.selected() { "> " } else { "  " };
            Line::raw(format!(
                "{mark}{:<name_width$} {:<STATUS_WIDTH$}",
                command.name(),
                status_label(command)
            ))
        })
        .collect();
    // はみ出した分は Paragraph が切る（折り返さない）
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_output(frame: &mut Frame, area: Rect, app: &App) {
    let Some(command) = app.selected_command() else {
        return;
    };
    let range = app.visible_range();
    let lines: Vec<Line> = command
        .output()
        .lines()
        .skip(range.start)
        .take(range.len())
        .map(Line::raw)
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    let title = Line::raw(app.title());
    let title_width = u16::try_from(title.width()).unwrap_or(u16::MAX);
    let [help, title_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(title_width)]).areas(area);
    frame.render_widget(Line::raw(HELP), help);
    frame.render_widget(title, title_area);
}

fn name_width(app: &App) -> usize {
    app.commands()
        .iter()
        .map(|c| Line::raw(c.name()).width())
        .max()
        .unwrap_or(0)
}

fn status_label(command: &CommandView) -> String {
    match command.state() {
        CommandState::Idle => "idle".to_owned(),
        CommandState::Running => "running".to_owned(),
        CommandState::Exited { code: Some(code) } => format!("exit {code}"),
        CommandState::Exited { code: None } => "exit ?".to_owned(),
        CommandState::Stopped => "stopped".to_owned(),
        CommandState::SpawnFailed => "failed".to_owned(),
    }
}

#[cfg(test)]
mod tests;
