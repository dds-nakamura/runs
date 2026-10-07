//! 端末の初期化・復元とイベントループ。実端末に触るのはこのモジュールだけ。

use std::io::{self, IsTerminal, Write};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event;
use ratatui::layout::Rect;

use crate::app::{self, App, Effect};
use crate::config::Config;
use crate::runner::{Runner, RunnerEvent};
use crate::ui;

/// 入力を待つ間隔。この間隔で子プロセスの通知も取り込む
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 終了時に、実行中のコマンドが穏やかに止まるのを待つ猶予
const STOP_GRACE: Duration = Duration::from_secs(2);

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

/// 終了するまで「描画 → 入力か子プロセスの通知を待つ → 状態の更新」を繰り返す。
///
/// 戻った時点で、実行中のコマンドはすべて停止し、端末は復元済み。エラーメッセージは呼び出し側がこの後に出す
/// （先に出すと alternate screen と一緒に消える）。
pub fn run(app: &mut App, config: &Config) -> Result<()> {
    // 端末でなければ何も変更せずに返す。パイプやリダイレクトにエスケープシーケンスを流さない
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("stdin and stdout must be a terminal");
    }

    let (mut runner, rx) = Runner::new(config.shell.clone());
    let mut guard = TerminalGuard::new()?;
    let result = event_loop(&mut guard.terminal, app, config, &mut runner, &rx);
    // 端末を復元する前に子プロセスを片付ける（kill / taskkill の出力は捨てているので画面は崩れない）
    runner.stop_all_and_wait(STOP_GRACE);
    drop(guard);
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    config: &Config,
    runner: &mut Runner,
    rx: &Receiver<RunnerEvent>,
) -> Result<()> {
    let mut dirty = true;
    loop {
        if dirty {
            let size = terminal.size().context("failed to get the terminal size")?;
            let panes = ui::layout(Rect::new(0, 0, size.width, size.height), app);
            app.set_output_height(panes.output.height);
            terminal
                .draw(|frame| ui::draw(frame, app))
                .context("failed to draw the screen")?;
            dirty = false;
        }

        // 入力が無ければ POLL_INTERVAL で戻り、子プロセスの通知だけを取り込む（リサイズもイベントとして届く）
        if event::poll(POLL_INTERVAL).context("failed to poll terminal events")? {
            let event = event::read().context("failed to read a terminal event")?;
            if let Some(action) = app::action_for(&event) {
                match app.apply(action) {
                    Some(Effect::Start(id)) => {
                        if let Some(spec) = config.commands.get(id) {
                            runner.start(id, spec);
                        }
                    }
                    Some(Effect::Stop(id)) => runner.stop(id),
                    None => {}
                }
            }
            dirty = true;
        }

        // 溜まった分をまとめて取り込み、描画は 1 回にする
        while let Ok(event) = rx.try_recv() {
            app.on_runner_event(event);
            dirty = true;
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}
