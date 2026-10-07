//! 子プロセスの起動・出力の読み取り・停止。プロセスに触るのはこのモジュールだけ。
//!
//! 停止は外部コマンド（Unix: `kill`、Windows: `taskkill`）で木ごと行う。`libc` や Job Object は `unsafe` が要るため使わない。
//! 外部コマンドが使えないときの最終手段は `Child::kill`（直接の子だけ）。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::config::CommandSpec;

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

pub struct Runner {
    tx: Sender<RunnerEvent>,
    shell: Vec<String>,
    running: HashMap<RunId, RunningProcess>,
}

impl Runner {
    pub fn new(shell: Vec<String>) -> (Self, Receiver<RunnerEvent>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                tx,
                shell,
                running: HashMap::new(),
            },
            rx,
        )
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
