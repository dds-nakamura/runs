use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::*;
use crate::app::App;

/// バージョンに依存しないよう、タイトルを固定して描画する。
fn render(width: u16, height: u16) -> Terminal<TestBackend> {
    let app = App::new("runs 1.2.3");
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("TestBackend の作成は失敗しない");
    terminal
        .draw(|frame| draw(frame, &app))
        .expect("TestBackend への描画は失敗しない");
    terminal
}

fn centered(text: &str, width: usize) -> String {
    format!("{text:^width$}")
}

#[test]
fn renders_centered_at_80x24() {
    let terminal = render(80, 24);

    // 2 行を上下左右の中央に置く。余りはどちらも偶数（縦 22、横 70 と 64）
    let mut expected = vec![" ".repeat(80); 24];
    expected[11] = centered("runs 1.2.3", 80);
    expected[12] = centered("q / Ctrl+C: quit", 80);
    terminal.backend().assert_buffer_lines(expected);
}

#[test]
fn truncates_at_10x2() {
    let terminal = render(10, 2);

    // 幅が足りない行は、行頭から表示して末尾を切る
    terminal
        .backend()
        .assert_buffer_lines(["runs 1.2.3", "q / Ctrl+C"]);
}

#[test]
fn renders_first_cell_at_1x1() {
    let terminal = render(1, 1);

    // 高さが足りなければ上の行から表示する
    terminal.backend().assert_buffer_lines(["r"]);
}

#[test]
fn does_not_panic_at_0x0() {
    // リサイズ直後などにサイズ 0 が届いても落ちない
    render(0, 0);
    render(0, 5);
    render(5, 0);
}
