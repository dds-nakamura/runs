//! 描画（状態 → 画面）。`TestBackend` にも描けるよう、実端末には触らない。

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::text::Line;

use crate::app::App;

const QUIT_HINT: &str = "q / Ctrl+C: quit";

/// タイトルと終了キーの案内を、画面の中央に 1 行ずつ描く。
///
/// 位置は `Layout` に決めさせる（`u16` の引き算を自分で書かない）。
/// 高さが足りなければ上の行から、幅が足りなければ行頭から表示する。
pub fn draw(frame: &mut Frame, app: &App) {
    let lines = [Line::from(app.title()), Line::from(QUIT_HINT)];

    let rows = Layout::vertical(lines.iter().map(|_| Constraint::Length(1)))
        .flex(Flex::Center)
        .split(frame.area());

    for (line, row) in lines.iter().zip(rows.iter()) {
        let width = u16::try_from(line.width()).unwrap_or(u16::MAX);
        let cells = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .split(*row);
        for cell in cells.iter() {
            frame.render_widget(line, *cell);
        }
    }
}

#[cfg(test)]
mod tests;
