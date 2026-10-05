//! 端末の初期化・復元とイベントループ。実端末に触るのはこのモジュールだけ。

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;

use crate::app::{self, App};
use crate::ui;

/// raw mode と alternate screen を有効にし、drop で必ず元に戻す。
///
/// `Terminal::draw` が隠したカーソルは、`terminal` フィールドの drop が戻す
/// （`Drop::drop` の後にフィールドが drop されるので、alternate screen を出た後になる）。
struct TerminalGuard {
    terminal: DefaultTerminal,
}

impl TerminalGuard {
    /// panic hook の設定・raw mode・alternate screen への切り替えを行う。
    fn new() -> Result<Self> {
        match ratatui::try_init() {
            Ok(terminal) => Ok(Self { terminal }),
            Err(err) => {
                // try_init は途中で失敗しても元に戻さない（raw mode だけ有効なまま返ることがある）。
                // 復元の失敗は捨てる: 利用者に伝えるのは初期化が失敗した原因のほう
                let _ = ratatui::try_restore();
                Err(err).context("failed to initialize the terminal")
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // panic の巻き戻し中は、ratatui の panic hook が復元を済ませている。
        // alternate screen を出るシーケンスをもう一度書くと、端末によってはカーソルが起動前の位置へ戻り、
        // 直前に出た panic メッセージが次のプロンプトで上書きされる
        if std::thread::panicking() {
            return;
        }
        if let Err(err) = ratatui::try_restore() {
            // eprintln! は書き込みに失敗すると panic する。復元の途中では panic させない。
            // stderr にも書けないなら、伝える先はもう無い
            let _ = writeln!(io::stderr(), "runs: failed to restore the terminal: {err}");
        }
    }
}

/// 終了するまで「描画 → イベント待ち → 状態の更新」を繰り返す。
///
/// 戻った時点で端末は復元済み。エラーメッセージは呼び出し側がこの後に出す
/// （先に出すと alternate screen と一緒に消える）。
pub fn run(app: &mut App) -> Result<()> {
    // 端末でなければ何も変更せずに返す。パイプやリダイレクトにエスケープシーケンスを流さない
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("stdin and stdout must be a terminal");
    }

    let mut guard = TerminalGuard::new()?;
    while !app.should_quit() {
        guard
            .terminal
            .draw(|frame| ui::draw(frame, app))
            .context("failed to draw the screen")?;
        // 定期的に更新する表示が無いので、イベントが来るまでブロックする（リサイズもイベントで届く）
        let event = event::read().context("failed to read a terminal event")?;
        if let Some(action) = app::action_for(&event) {
            app.apply(action);
        }
    }
    Ok(())
}
