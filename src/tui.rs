//! 端末の初期化・復元とイベントループ。実端末に触るのはこのモジュールだけ。

use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::{cursor, event, execute};
use ratatui::layout::Rect;

use crate::app::{self, App, Effect};
use crate::config::{self, Config};
use crate::runner::{GhEvent, LivePids, Runner, RunnerEvent};
use crate::ui;

/// `Effect` を実行するのに要るもの。プロセスは `runner`、設定ファイルは `config_path`、`gh` は `root`（設定ファイルのある
/// ディレクトリ）で実行して `gh_tx` に結果を送る
struct EffectContext<'a> {
    runner: &'a mut Runner,
    config_path: &'a Path,
    root: &'a Path,
    gh_tx: Sender<GhEvent>,
}

/// 入力を待つ間隔。この間隔で子プロセスの通知も取り込む
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 時間の表示があるとき、描き直す間隔
const TICK: Duration = Duration::from_secs(1);
/// 終了時に、実行中のコマンドが穏やかに止まるのを待つ猶予
const STOP_GRACE: Duration = Duration::from_secs(2);
/// シグナルを受けてから、主スレッドの片付けを待つ上限。全停止の上限（`STOP_GRACE` + `runner` の強制終了の猶予 1 秒）に、
/// 停止コマンド（`kill` / `taskkill`。コマンドの数だけ順に起動する）と描画の分の余裕を足した値。
/// これを過ぎたら主スレッドは端末の読み取りか描画で止まっていると見なし、ハンドラのスレッドが片付けて終了する
const EMERGENCY_GRACE: Duration = Duration::from_secs(10);
/// 緊急終了のとき、別スレッドでの端末の復元を待つ上限。
const RESTORE_WAIT: Duration = Duration::from_millis(500);

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
    // SIGTERM / SIGHUP（Unix）や Ctrl+Break（Windows）でも、下の経路で全停止と復元が走るようにする
    let cleaned_up = Arc::new(AtomicBool::new(false));
    let terminated = install_termination_flag(
        runner.live_pids(),
        runner.gh_cancel_flag(),
        Arc::clone(&cleaned_up),
    )?;
    let mut guard = TerminalGuard::new()?;
    // gh の取得結果は子プロセスの通知とは別のチャネルで受ける（RunId に紐づかないため）
    let (gh_tx, gh_rx) = mpsc::channel::<GhEvent>();
    let mut context = EffectContext {
        runner: &mut runner,
        config_path: &config.path,
        root: &config.root,
        gh_tx,
    };
    let result = event_loop(
        &mut guard.terminal,
        app,
        &mut context,
        &rx,
        &gh_rx,
        &terminated,
    );
    // 端末を復元する前に子プロセスを片付ける（kill / taskkill の出力は捨てているので画面は崩れない）
    runner.stop_all_and_wait(STOP_GRACE);
    drop(guard);
    cleaned_up.store(true, Ordering::Release);
    result
}

/// シグナル・コンソールの制御イベントで立つフラグを登録する。
///
/// ハンドラは別スレッドで呼ばれる。ふつうはフラグを立てるだけで、主スレッドのイベントループが全停止と復元をする
/// （端末やプロセスには主スレッドだけが触る）。
/// `termination` feature により、Unix は SIGINT / SIGTERM / SIGHUP、Windows は Ctrl+C / Ctrl+Break などが対象。
/// raw mode 中の Ctrl+C はキーとして届くので、ここを通るのは `kill` と Ctrl+Break。
///
/// 例外は端末が本当に閉じたとき（SSH の切断など）。crossterm の端末の読み取りが戻らず、主スレッドがフラグを見に来られない。
/// そのときは `EMERGENCY_GRACE` の後に、このスレッドが子プロセスを KILL し、端末の復元を試してからプロセスを終える
/// （`process::exit` で `Terminal` の drop を通らない唯一の経路。rust-safety 2 章の例外として #3 の spec で合意済み。
/// 端末が消えていれば復元の書き込みは失敗するだけで、生きていれば raw mode・alternate screen・カーソルが戻る）
fn install_termination_flag(
    live: LivePids,
    gh_cancel: Arc<AtomicBool>,
    cleaned_up: Arc<AtomicBool>,
) -> Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);
    ctrlc::set_handler(move || {
        handler_flag.store(true, Ordering::Release);
        let deadline = Instant::now() + EMERGENCY_GRACE;
        while Instant::now() < deadline {
            if cleaned_up.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // gh の取得は、動いているものを KILL するだけでは取得スレッドが次のコマンドを起こすので、先に取り消しの旗を立てる
        gh_cancel.store(true, Ordering::Release);
        live.kill_all();
        // 主スレッドが止まっているので、こちらで復元を試す。失敗は捨てる（端末が無いときは書けない）。
        // 主スレッドが stdout のロックを持ったまま書き込みで止まっている場合もあるので、別スレッドで試し、
        // RESTORE_WAIT だけ待ってから終える（復元が終わらなくてもプロセスは残さない）
        let restore = std::thread::spawn(|| {
            let _ = ratatui::try_restore();
            let _ = execute!(io::stdout(), cursor::Show);
            let _ = writeln!(io::stderr(), "runs: terminated by signal (forced)");
        });
        let deadline = Instant::now() + RESTORE_WAIT;
        while Instant::now() < deadline && !restore.is_finished() {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::process::exit(1);
    })
    .context("failed to install the signal handler")?;
    Ok(flag)
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    context: &mut EffectContext<'_>,
    rx: &Receiver<RunnerEvent>,
    gh_rx: &Receiver<GhEvent>,
    terminated: &AtomicBool,
) -> Result<()> {
    let mut dirty = true;
    let mut last_draw = Instant::now();
    loop {
        // シグナルで頼まれた終了。呼び出し側が全停止と復元をしてから、このメッセージを stderr に出す
        if terminated.load(Ordering::Acquire) {
            bail!("terminated by signal");
        }
        // 入力・通知の処理でも現在時刻を使う（開始・終了時刻の記録）ので、描画の有無によらず毎ループ渡す
        let now = Instant::now();
        app.set_now(now);
        // 経過時間を見せている間は 1 秒ごとに描き直す。何も実行していなければ入力か通知があるときだけ
        if app.needs_tick() && now.saturating_duration_since(last_draw) >= TICK {
            dirty = true;
        }
        if dirty {
            let size = terminal.size().context("failed to get the terminal size")?;
            let panes = ui::layout(Rect::new(0, 0, size.width, size.height), app);
            app.set_output_height(panes.output.height);
            terminal
                .draw(|frame| ui::draw(frame, app))
                .context("failed to draw the screen")?;
            last_draw = now;
            dirty = false;
        }

        // 入力が無ければ POLL_INTERVAL で戻り、子プロセスの通知だけを取り込む（リサイズもイベントとして届く）
        if event::poll(POLL_INTERVAL).context("failed to poll terminal events")? {
            let event = event::read().context("failed to read a terminal event")?;
            // 通知はどのキーでも消える（リサイズでは消えない）
            if matches!(&event, event::Event::Key(key) if key.kind == event::KeyEventKind::Press) {
                app.clear_notice();
            }
            if let Some(action) = app::action_for(&event) {
                // 開始時刻の記録に使うので、poll で待った分だけ古くなった時刻を取り直す
                app.set_now(Instant::now());
                let effects = app.apply(action);
                handle_effects(effects, app, context);
            }
            dirty = true;
        }

        // 溜まった分をまとめて取り込み、描画は 1 回にする（終了時刻の記録に使うので時刻を取り直す）
        app.set_now(Instant::now());
        while let Ok(event) = rx.try_recv() {
            let effects = app.on_runner_event(event);
            handle_effects(effects, app, context);
            dirty = true;
        }
        while let Ok(event) = gh_rx.try_recv() {
            app.on_gh_event(event);
            dirty = true;
        }

        if app.should_quit() {
            return Ok(());
        }
    }
}

/// `App` が頼んだことを実行する。プロセスは `runner`、ファイルは `config` に任せる
fn handle_effects(effects: Vec<Effect>, app: &mut App, context: &mut EffectContext<'_>) {
    for effect in effects {
        match effect {
            Effect::Start { run, spec } => context.runner.start(run, &spec),
            Effect::Stop(run) => context.runner.stop(run),
            Effect::FetchGh => {
                context
                    .runner
                    .fetch_gh("gh", context.root, context.gh_tx.clone());
            }
            Effect::Reload => match config::load_file(context.config_path) {
                Ok(new) => {
                    context.runner.set_shell(new.shell.clone());
                    // 消えた実行中のコマンドの Stop が返る
                    let stops = app.replace_config(&new);
                    handle_effects(stops, app, context);
                    let count = new.commands.len();
                    let noun = if count == 1 { "command" } else { "commands" };
                    app.set_notice(format!("reloaded {} ({count} {noun})", config::FILE_NAME));
                }
                // 文脈（`invalid <絶対パス>`）を付けると 80 桁で理由が切れるので、原因だけを出す。
                // 設定ファイルの引用が含まれうるが、set_notice が 1 行目だけを無害化して保持する
                Err(err) => app.set_notice(format!("reload failed: {}", err.root_cause())),
            },
        }
    }
}
