//! 子プロセスの起動・出力の読み取り・停止。プロセスに触るのはこのモジュールだけ。
//!
//! 停止は外部コマンド（Unix: `kill`、Windows: `taskkill`）で行う。`libc` や Job Object は `unsafe` が要るため使わない。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::config::CommandSpec;

/// `Config::commands` の添字。
pub type CommandId = usize;

#[derive(Debug)]
pub enum RunnerEvent {
    Started {
        id: CommandId,
    },
    /// 1 行分。末尾の改行（LF / CRLF）は含まない。stdout と stderr は区別しない
    Output {
        id: CommandId,
        bytes: Vec<u8>,
    },
    Exited {
        id: CommandId,
        status: ExitStatus,
    },
    SpawnFailed {
        id: CommandId,
        message: String,
    },
}

/// 穏やかな停止から強制終了までの猶予（Unix）。
const STOP_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

struct RunningProcess {
    pid: u32,
    /// 終了待ちスレッドが `wait` を終えたら true。pid の再利用で無関係なプロセスを止めないための目印
    finished: Arc<AtomicBool>,
    waiter: JoinHandle<()>,
}

pub struct Runner {
    tx: Sender<RunnerEvent>,
    shell: Vec<String>,
    running: HashMap<CommandId, RunningProcess>,
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
    pub fn start(&mut self, id: CommandId, spec: &CommandSpec) {
        self.reap();
        if self.running.contains_key(&id) {
            return;
        }
        match self.spawn(spec) {
            Ok(child) => {
                self.send(RunnerEvent::Started { id });
                let process = self.watch(id, child);
                self.running.insert(id, process);
            }
            Err(message) => self.send(RunnerEvent::SpawnFailed { id, message }),
        }
    }

    /// 穏やかな停止を要求する。実際の終了は `Exited` で届く。
    /// Unix は TERM を送り、猶予の後も生きていれば KILL。Windows は最初から強制終了
    pub fn stop(&mut self, id: CommandId) {
        self.reap();
        let Some(process) = self.running.get(&id) else {
            return;
        };
        terminate(process.pid);
        if cfg!(unix) {
            let pid = process.pid;
            let finished = Arc::clone(&process.finished);
            thread::spawn(move || {
                thread::sleep(STOP_GRACE);
                if !finished.load(Ordering::Acquire) {
                    force_kill(pid);
                }
            });
        }
    }

    /// 全部に停止を送り、猶予の後に残りを強制終了して、終わるまで待つ（TUI の終了時用）
    pub fn stop_all_and_wait(&mut self, grace: Duration) {
        self.reap();
        for process in self.running.values() {
            terminate(process.pid);
        }
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline && self.running.values().any(|p| !p.is_finished()) {
            thread::sleep(POLL_INTERVAL);
        }
        for process in self.running.values() {
            if !process.is_finished() {
                force_kill(process.pid);
            }
        }
        for (_, process) in self.running.drain() {
            // 終了待ちスレッドは wait が返れば終わる。panic していても復元することは無いので結果は見ない
            let _ = process.waiter.join();
        }
    }

    /// テスト用。本体は `start` / `stop` が内部で判定する
    #[cfg(test)]
    pub fn is_running(&self, id: CommandId) -> bool {
        self.running.get(&id).is_some_and(|p| !p.is_finished())
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
            .arg(&spec.command)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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
    fn watch(&self, id: CommandId, mut child: Child) -> RunningProcess {
        let pid = child.id();
        if let Some(stdout) = child.stdout.take() {
            spawn_reader(id, stdout, self.tx.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_reader(id, stderr, self.tx.clone());
        }
        let finished = Arc::new(AtomicBool::new(false));
        let waiter = {
            let tx = self.tx.clone();
            let finished = Arc::clone(&finished);
            thread::spawn(move || {
                let result = child.wait();
                finished.store(true, Ordering::Release);
                let event = match result {
                    Ok(status) => RunnerEvent::Exited { id, status },
                    // wait の失敗（ECHILD など）はまず起きない。状態としては「失敗」で見せる
                    Err(err) => RunnerEvent::SpawnFailed {
                        id,
                        message: format!("failed to wait for the process: {err}"),
                    },
                };
                // 受信側が先に終わっていたら伝える先が無いだけ
                let _ = tx.send(event);
            })
        };
        RunningProcess {
            pid,
            finished,
            waiter,
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

impl RunningProcess {
    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// 行ごとに `Output` を送る。パイプが閉じるか受信側が終わるまで。
fn spawn_reader(id: CommandId, stream: impl Read + Send + 'static, tx: Sender<RunnerEvent>) {
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
                    id,
                    bytes: line.clone(),
                })
                .is_err()
            {
                return;
            }
        }
    });
}

/// 穏やかに止める。Unix はプロセスグループへ TERM、Windows は木ごと強制終了（穏やかな手段が無い）。
fn terminate(pid: u32) {
    #[cfg(unix)]
    run_quietly("kill", &["-s", "TERM", "--", &format!("-{pid}")]);
    #[cfg(windows)]
    run_quietly("taskkill", &["/T", "/F", "/PID", &pid.to_string()]);
}

/// 強制終了する。
fn force_kill(pid: u32) {
    #[cfg(unix)]
    run_quietly("kill", &["-s", "KILL", "--", &format!("-{pid}")]);
    #[cfg(windows)]
    run_quietly("taskkill", &["/T", "/F", "/PID", &pid.to_string()]);
}

/// 外部コマンドを、出力を捨てて実行する（TUI 実行中に端末へ流さない）。失敗しても伝える先が無いので無視する。
fn run_quietly(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests;
