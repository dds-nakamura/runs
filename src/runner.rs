//! 子プロセスの起動・出力の読み取り・停止。プロセスに触るのはこのモジュールだけ。
//!
//! 停止は外部コマンド（Unix: `kill`、Windows: `taskkill`）で木ごと行う。`libc` や Job Object は `unsafe` が要るため使わない。
//! 外部コマンドが使えないときの最終手段は `Child::kill`（直接の子だけ）。

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::CommandSpec;
use crate::gh;

/// 実行ごとに一意な番号。`App` が採番し、通知の宛先に使う（コマンドの添字ではないので、一覧が並び替わっても再実行してもずれない）。
pub type RunId = u64;

#[derive(Debug)]
pub enum RunnerEvent {
    Started {
        run: RunId,
    },
    /// 1 行分。末尾の改行（LF / CRLF）は含まない。stdout と stderr は区別しない
    Output {
        run: RunId,
        bytes: Vec<u8>,
    },
    Exited {
        run: RunId,
        status: ExitStatus,
    },
    SpawnFailed {
        run: RunId,
        message: String,
    },
    /// 停止の手段（`kill` / `taskkill`）が失敗した。プロセスは動いたまま
    StopFailed {
        run: RunId,
        message: String,
    },
}

/// `gh` のような「1 回で終わる外部コマンド」の結果。出力は全部読んでから返す。
#[derive(Debug, Default)]
pub struct Capture {
    /// 終了状態。起動に失敗したか、タイムアウトで止めたときは `None`
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// 起動の失敗（`NotFound` なら PATH に無い）
    pub spawn_error: Option<std::io::ErrorKind>,
    /// 期限までに終わらず、強制終了した
    pub timed_out: bool,
}

/// `gh` の取得結果。PR の一覧と CI 実行の一覧を 1 回で返す
#[derive(Debug)]
pub enum GhEvent {
    Fetched {
        pr: Capture,
        run: Capture,
        /// 取得した時点の UNIX 秒（壁時計。CI 実行の経過時間の基準）
        now_unix: u64,
    },
}

/// `gh` 1 回あたりの上限。ネットワーク待ちで戻らないときはここで諦める
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// 穏やかな停止から強制終了までの猶予（Unix）。
const STOP_GRACE: Duration = Duration::from_secs(2);
/// 強制終了の後、終了待ちスレッドが終わるのを待つ上限。これを過ぎたら諦めて戻る
const FORCE_GRACE: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// 子プロセスの終了後、出力の読み取りが終わるのを待つ上限。
const READER_GRACE: Duration = Duration::from_millis(500);

struct RunningProcess {
    pid: u32,
    /// 終了待ちスレッドと共有する。最終手段の `Child::kill` に使う
    child: Arc<Mutex<Child>>,
    /// 終了待ちスレッドが終了を確認したら true
    finished: Arc<AtomicBool>,
    /// 停止を頼まれたら true。Unix では、シェルだけが先に終わっても孫が消えるまで `Exited` を送らない
    stop_requested: Arc<AtomicBool>,
    waiter: JoinHandle<()>,
}

/// 生きている子プロセスの pid。シグナルのハンドラのスレッドが、主スレッドが片付けられないときに直接 KILL するために共有する
#[derive(Clone, Default)]
pub struct LivePids(Arc<Mutex<HashSet<u32>>>);

impl LivePids {
    fn insert(&self, pid: u32) {
        self.lock().insert(pid);
    }

    fn remove(&self, pid: u32) {
        self.lock().remove(&pid);
    }

    /// 全部を強制終了する（Unix はプロセスグループごと、Windows は木ごと）。終了コマンドを順に起動するだけで、
    /// プロセスが消えるのは待たない（ロックは起動の間だけ持つ。直後に `process::exit` する緊急時用）
    pub fn kill_all(&self) {
        for pid in self.lock().iter() {
            let _ = force_kill(*pid);
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashSet<u32>> {
        // poison でも中身（pid の集合）はそのまま使える
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct Runner {
    tx: Sender<RunnerEvent>,
    shell: Vec<String>,
    running: HashMap<RunId, RunningProcess>,
    live: LivePids,
    /// `gh` の取得が進行中。取得スレッドが結果を送る直前に下ろす（`Ready` を見た時点で次の取得を受け付ける）
    gh_busy: Arc<AtomicBool>,
    /// いま動いている `gh` の pid（スレッドと共有。終了時に止めるため）
    gh_pid: Arc<Mutex<Option<u32>>>,
    /// 取得の取り消し（終了時）。立っていれば取得スレッドは残りのコマンドを起動せず、結果も送らない
    gh_cancel: Arc<AtomicBool>,
}

impl Runner {
    pub fn new(shell: Vec<String>) -> (Self, Receiver<RunnerEvent>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                shell,
                running: HashMap::new(),
                live: LivePids::default(),
                gh_busy: Arc::new(AtomicBool::new(false)),
                gh_pid: Arc::new(Mutex::new(None)),
                gh_cancel: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    /// 生きている子プロセスの pid の一覧（共有）。
    pub fn live_pids(&self) -> LivePids {
        self.live.clone()
    }

    /// 起動する。失敗は `SpawnFailed` として通知する。実行中なら何もしない
    pub fn start(&mut self, run: RunId, spec: &CommandSpec) {
        self.reap();
        if self.running.contains_key(&run) {
            return;
        }
        match self.spawn(spec) {
            Ok(child) => {
                self.send(RunnerEvent::Started { run });
                let process = self.watch(run, child);
                self.running.insert(run, process);
            }
            Err(message) => self.send(RunnerEvent::SpawnFailed { run, message }),
        }
    }

    /// 穏やかな停止を要求する。実際の終了は `Exited` で届く。
    /// Unix は TERM を送り、猶予の後にプロセスグループへ KILL。Windows は最初から強制終了
    pub fn stop(&mut self, run: RunId) {
        self.reap();
        let Some(process) = self.running.get(&run) else {
            return;
        };
        process.stop_requested.store(true, Ordering::Release);
        if let Err(message) = terminate(process.pid) {
            // 外部コマンドが使えない。直接の子だけでも止める（孫は残る）
            let fallback = lock(&process.child).kill();
            if !process.is_finished() {
                self.send(RunnerEvent::StopFailed {
                    run,
                    message: match fallback {
                        Ok(()) => format!("{message}; killed the direct child only"),
                        Err(err) => format!("{message}; direct kill also failed: {err}"),
                    },
                });
            }
        }
        if cfg!(unix) {
            // シェルだけが TERM で終わり、TERM を無視する孫が残ることがある。
            // 猶予の後は、シェルの終了を見ずにグループへ KILL を送る（グループの誰かが生きている間、その pgid は再利用されない）
            let pid = process.pid;
            thread::spawn(move || {
                thread::sleep(STOP_GRACE);
                let _ = force_kill(pid);
            });
        }
    }

    /// 全部に停止を送り、猶予の後に残りを強制終了して、終わるまで待つ（TUI の終了時用）。
    /// 止められないものがあっても `grace + FORCE_GRACE` ほどで必ず戻る
    pub fn stop_all_and_wait(&mut self, grace: Duration) {
        self.reap();
        self.kill_gh();
        for process in self.running.values() {
            if terminate(process.pid).is_err() {
                let _ = lock(&process.child).kill();
            }
        }
        self.wait_all(grace);

        for process in self.running.values() {
            // Unix はシェルが終わっていても孫が残りうるので、グループへ無条件に KILL
            if cfg!(unix) || !process.is_finished() {
                let _ = force_kill(process.pid);
            }
            if !process.is_finished() {
                let _ = lock(&process.child).kill();
            }
        }
        self.wait_all(FORCE_GRACE);

        for (_, process) in self.running.drain() {
            if process.is_finished() {
                // 終了を確認済みなので join はすぐ返る。panic していても復元することは無いので結果は見ない
                let _ = process.waiter.join();
            }
            // 終わらなかったものは諦める（スレッドはプロセスの終了とともに消える）。端末の復元を待たせない
        }
    }

    /// 次の `start` から使うシェルを差し替える（設定の再読み込み用）。実行中のプロセスには影響しない
    pub fn set_shell(&mut self, shell: Vec<String>) {
        self.shell = shell;
    }

    /// `gh` で PR と CI 実行の一覧を取る。別スレッドで 2 回順に実行し、両方の結果を 1 つの `GhEvent` で `tx` に送る。
    /// 取得中なら何もしない（結果は進行中のものが届く）。`program` は通常 `"gh"`（テストでは偽のコマンドに差し替える）。
    /// シェルは通さず引数を分けて渡す。各コマンドは `FETCH_TIMEOUT` で強制終了する
    pub fn fetch_gh(&mut self, program: &str, cwd: &Path, tx: Sender<GhEvent>) {
        if self.is_fetching_gh() {
            return;
        }
        self.gh_busy.store(true, Ordering::Release);
        self.gh_cancel.store(false, Ordering::Release);
        let program = program.to_owned();
        let cwd = cwd.to_path_buf();
        let gh = GhHandles {
            live: self.live.clone(),
            current: Arc::clone(&self.gh_pid),
            cancel: Arc::clone(&self.gh_cancel),
        };
        let busy = Arc::clone(&self.gh_busy);
        thread::spawn(move || {
            let result = fetch_sequence(&program, &cwd, &gh);
            // 送る前に下ろす。受信側が Ready を見た時点で、次の g が新しい取得を起こせる
            busy.store(false, Ordering::Release);
            if let Some((pr, run)) = result {
                // 1970 年より前の時計なら 0（経過時間が全部 0 になるだけ）
                let now_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                // 受信側が先に終わっていたら伝える先が無いだけ
                let _ = tx.send(GhEvent::Fetched { pr, run, now_unix });
            }
        });
    }

    /// `gh` の取得が進行中か。
    pub fn is_fetching_gh(&self) -> bool {
        self.gh_busy.load(Ordering::Acquire)
    }

    /// 進行中の `gh` の取得を取り消す（終了時用）。動いているコマンドは強制終了し、残りのコマンドは起動されず、結果も届かない
    fn kill_gh(&self) {
        cancel_gh(&self.gh_pid, &self.gh_cancel);
    }

    #[cfg(test)]
    pub fn is_running(&self, run: RunId) -> bool {
        self.running.get(&run).is_some_and(|p| !p.is_finished())
    }

    fn spawn(&self, spec: &CommandSpec) -> Result<Child, String> {
        let Some((program, args)) = self.shell.split_first() else {
            return Err("shell is not configured".to_owned());
        };
        if !spec.cwd.is_dir() {
            return Err(format!(
                "working directory does not exist: {}",
                spec.cwd.display()
            ));
        }
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        push_command_arg(&mut command, program, &spec.command);
        #[cfg(unix)]
        {
            // シェルが起こした孫プロセスごと止められるよう、新しいプロセスグループにする（pgid = pid）
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
            .spawn()
            .map_err(|err| format!("failed to start {program:?}: {err}"))
    }

    /// 出力と終了を見張るスレッドを立てる。
    fn watch(&self, run: RunId, mut child: Child) -> RunningProcess {
        let pid = child.id();
        self.live.insert(pid);
        let streams: [Option<Box<dyn Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ];
        let readers: Vec<JoinHandle<()>> = streams
            .into_iter()
            .flatten()
            .map(|stream| spawn_reader(run, stream, self.tx.clone()))
            .collect();
        let child = Arc::new(Mutex::new(child));
        let finished = Arc::new(AtomicBool::new(false));
        let stop_requested = Arc::new(AtomicBool::new(false));
        let waiter = {
            let tx = self.tx.clone();
            let child = Arc::clone(&child);
            let finished = Arc::clone(&finished);
            let stop_requested = Arc::clone(&stop_requested);
            let live = self.live.clone();
            thread::spawn(move || {
                // ブロックする wait ではなくポーリングにして、Runner 側が Child::kill を使えるようにする
                let result = loop {
                    match lock(&child).try_wait() {
                        Ok(Some(status)) => break Ok(status),
                        Ok(None) => {}
                        Err(err) => break Err(err),
                    }
                    thread::sleep(POLL_INTERVAL);
                };
                // 最後の行が Exited より後に届かないよう、読み取りスレッドの終了を待つ。
                // ただし無期限には待たない: 出力パイプを握ったまま残る孫プロセス（`foo &` など）がいると
                // パイプが閉じず、終了が永遠に通知されなくなる。遅れた行は Exited の後に届いても表示される
                let deadline = Instant::now() + READER_GRACE;
                while Instant::now() < deadline && readers.iter().any(|r| !r.is_finished()) {
                    thread::sleep(POLL_INTERVAL);
                }
                // 停止を頼まれていたら、孫プロセスも含めて消えてから終了を伝える。
                // そうしないと「停止して再実行」で、古いサーバーがポートを握ったまま新しいものが起動する（Unix）
                if stop_requested.load(Ordering::Acquire) {
                    wait_for_group_exit(pid, STOP_GRACE + FORCE_GRACE);
                }
                live.remove(pid);
                finished.store(true, Ordering::Release);
                let event = match result {
                    Ok(status) => RunnerEvent::Exited { run, status },
                    // wait の失敗（ECHILD など）はまず起きない。状態としては「失敗」で見せる
                    Err(err) => RunnerEvent::SpawnFailed {
                        run,
                        message: format!("failed to wait for the process: {err}"),
                    },
                };
                // 受信側が先に終わっていたら伝える先が無いだけ
                let _ = tx.send(event);
            })
        };
        RunningProcess {
            pid,
            child,
            finished,
            stop_requested,
            waiter,
        }
    }

    /// 全部が終わるか期限が来るまで待つ。
    fn wait_all(&self, limit: Duration) {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline && self.running.values().any(|p| !p.is_finished()) {
            thread::sleep(POLL_INTERVAL);
        }
    }

    /// 終わったプロセスの記録を捨てる。
    fn reap(&mut self) {
        self.running.retain(|_, p| !p.is_finished());
    }

    fn send(&self, event: RunnerEvent) {
        // 受信側が先に終わっていたら伝える先が無いだけ
        let _ = self.tx.send(event);
    }
}

impl Drop for Runner {
    /// panic などで `stop_all_and_wait` を通らずに終わるときも、子プロセスを放置しない（待ちはしない）。
    fn drop(&mut self) {
        self.kill_gh();
        for process in self.running.values() {
            if !process.is_finished() && terminate(process.pid).is_err() {
                let _ = lock(&process.child).kill();
            }
        }
    }
}

impl RunningProcess {
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// poison（他スレッドの panic）でも中身は使える。復元すべき不変条件は無い
fn lock(child: &Mutex<Child>) -> MutexGuard<'_, Child> {
    child.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 利用者が書いたコマンド文字列をシェルに渡す。
///
/// Windows の `cmd` は、std が MSVC の規則で付ける `\"` のエスケープを解釈しないので `raw_arg` で渡す。
/// さらに `cmd /C` は引用符が 3 つ以上あると先頭と末尾の `"` を外す規則があり、`"C:\Program Files\x.exe" "a b"` のような
/// コマンドが壊れる。全体を `"` で包み、既定のシェルの `/S`（先頭と末尾の `"` だけを外す）と組み合わせて、中身をそのまま届ける。
/// それ以外（`sh`、`pwsh` など）は通常の引数として渡す。
#[cfg(windows)]
fn push_command_arg(command: &mut Command, program: &str, text: &str) {
    use std::os::windows::process::CommandExt;
    let name = std::path::Path::new(program)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(program);
    if name.eq_ignore_ascii_case("cmd") {
        command.raw_arg(format!("\"{text}\""));
    } else {
        command.arg(text);
    }
}

#[cfg(not(windows))]
fn push_command_arg(command: &mut Command, _program: &str, text: &str) {
    command.arg(text);
}

/// 行ごとに `Output` を送る。パイプが閉じるか受信側が終わるまで。
fn spawn_reader(
    run: RunId,
    stream: impl Read + Send + 'static,
    tx: Sender<RunnerEvent>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if tx
                .send(RunnerEvent::Output {
                    run,
                    bytes: line.clone(),
                })
                .is_err()
            {
                return;
            }
        }
    })
}

/// プロセスグループが消えるまで待つ（上限付き）。Windows は `taskkill /T` が木ごと止めるので待たない。
/// pgid が別のグループに再利用されていた場合は上限まで待ってしまうが、害は遅れだけ
fn wait_for_group_exit(pid: u32, limit: Duration) {
    if !cfg!(unix) {
        return;
    }
    let deadline = Instant::now() + limit;
    // シグナル 0 は送らずに存在だけを確かめる。グループに誰かいれば成功する
    while Instant::now() < deadline
        && run_quietly("kill", &["-s", "0", "--", &format!("-{pid}")]).is_ok()
    {
        thread::sleep(POLL_INTERVAL);
    }
}

/// 穏やかに止める。Unix はプロセスグループへ TERM、Windows は木ごと強制終了（穏やかな手段が無い）。
fn terminate(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    return run_quietly("kill", &["-s", "TERM", "--", &format!("-{pid}")]);
    #[cfg(windows)]
    return run_quietly("taskkill", &["/T", "/F", "/PID", &pid.to_string()]);
}

/// 強制終了する。
fn force_kill(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    return run_quietly("kill", &["-s", "KILL", "--", &format!("-{pid}")]);
    #[cfg(windows)]
    return run_quietly("taskkill", &["/T", "/F", "/PID", &pid.to_string()]);
}

/// `gh` の取得スレッドと `Runner` が共有するもの。
#[derive(Clone)]
struct GhHandles {
    live: LivePids,
    /// いま動いている `gh` の pid
    current: Arc<Mutex<Option<u32>>>,
    /// 取り消しの旗。`current` と同じロックの下で立てる（起動の直後と行き違わない）
    cancel: Arc<AtomicBool>,
}

/// `gh` を 2 回（PR の一覧、CI 実行の一覧）順に実行する。途中で取り消されたら残りは起動せず `None`
fn fetch_sequence(program: &str, cwd: &Path, gh: &GhHandles) -> Option<(Capture, Capture)> {
    let pr = capture(program, gh::PR_ARGS, cwd, FETCH_TIMEOUT, gh);
    if gh.cancel.load(Ordering::Acquire) {
        return None;
    }
    let run = capture(program, gh::RUN_ARGS, cwd, FETCH_TIMEOUT, gh);
    if gh.cancel.load(Ordering::Acquire) {
        return None;
    }
    Some((pr, run))
}

/// 取り消しの旗を立て、動いている `gh` があれば強制終了する。
/// 旗と pid は同じロックの下で扱うので、`capture` が起動した直後に登録する前に取りこぼすことは無い
fn cancel_gh(current: &Mutex<Option<u32>>, cancel: &AtomicBool) {
    let pid = lock_pid(current);
    cancel.store(true, Ordering::Release);
    if let Some(pid) = *pid {
        let _ = force_kill(pid);
    }
}

/// 1 回で終わる外部コマンド（`gh`）を実行し、stdout / stderr を全部読んで返す。
///
/// シェルは通さず引数を分けて渡す。stdin は null（認証や確認で端末入力を待たせない）。色・ページャー・対話・更新通知は
/// 環境変数で抑える（利用者の `CLICOLOR_FORCE` / `GH_FORCE_TTY` は `NO_COLOR` より優先されて JSON に色が付くので外す）。
/// `timeout` までに終わらなければ強制終了し `timed_out` にする。pid は `live` と `current` に登録し、
/// 終了時の全停止と緊急 KILL の対象にする。取り消し済みなら起動しない
fn capture(program: &str, args: &[&str], cwd: &Path, timeout: Duration, gh: &GhHandles) -> Capture {
    if !cwd.is_dir() {
        // Unix では子の chdir が失敗して NotFound になり「gh が無い」と区別できないので、先に確かめる
        return Capture {
            spawn_error: Some(std::io::ErrorKind::NotADirectory),
            ..Capture::default()
        };
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("GH_FORCE_TTY")
        .env("GH_PAGER", "")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1");
    #[cfg(unix)]
    {
        // 既存のコマンドと同じ停止経路（プロセスグループへの kill）を使えるようにする
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // 起動と pid の登録はロックの下で行い、取り消しと行き違わないようにする
    let mut child = {
        let mut current = lock_pid(&gh.current);
        if gh.cancel.load(Ordering::Acquire) {
            return Capture::default();
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                return Capture {
                    spawn_error: Some(err.kind()),
                    ..Capture::default()
                };
            }
        };
        gh.live.insert(child.id());
        *current = Some(child.id());
        child
    };
    let pid = child.id();

    // stdout と stderr は別スレッドで同時に読む（片方だけ読むとパイプが詰まって止まる）
    let stdout = child.stdout.take().map(read_to_end_in_thread);
    let stderr = child.stderr.take().map(read_to_end_in_thread);

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            // 期限切れか、wait の失敗（まず起きない）。木ごと止め、最終手段で直接の子も止める。終了を回収して zombie にしない
            result => {
                timed_out = result.is_ok();
                let _ = force_kill(pid);
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    *lock_pid(&gh.current) = None;
    gh.live.remove(pid);
    Capture {
        status,
        stdout: collect_output(stdout),
        stderr: collect_output(stderr),
        spawn_error: None,
        timed_out,
    }
}

fn read_to_end_in_thread(mut stream: impl Read + Send + 'static) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        // 読み取りの失敗（強制終了でパイプが壊れたなど）は、そこまでの出力で返す
        let _ = stream.read_to_end(&mut buf);
        buf
    })
}

/// 読み取りスレッドの結果を回収する。プロセスは終わっているのでパイプはすぐ閉じるはずだが、
/// 孫がパイプを握っていても `READER_GRACE` を過ぎたら諦める（出力は空になる）
fn collect_output(reader: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    let Some(reader) = reader else {
        return Vec::new();
    };
    let deadline = Instant::now() + READER_GRACE;
    while Instant::now() < deadline && !reader.is_finished() {
        thread::sleep(POLL_INTERVAL);
    }
    if reader.is_finished() {
        reader.join().unwrap_or_default()
    } else {
        Vec::new()
    }
}

/// poison でも中身（pid）はそのまま使える。
fn lock_pid(current: &Mutex<Option<u32>>) -> MutexGuard<'_, Option<u32>> {
    current.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 外部コマンドを、出力を捨てて実行する（TUI 実行中に端末へ流さない）。
fn run_quietly(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("failed to run {program}: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

#[cfg(test)]
mod tests;
