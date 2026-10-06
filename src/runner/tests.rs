//! 実プロセスで確かめる。OS 既定のシェル（`sh -c` / `cmd /C`）と、両方にある `echo` / `exit` を使う。
//! 終わらないコマンドだけはシェルを通さず、実行ファイルを `shell` に直接渡す。

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use super::*;
use crate::config::{CommandSpec, default_shell};

const TIMEOUT: Duration = Duration::from_secs(10);

fn cwd() -> PathBuf {
    std::env::current_dir().expect("カレントディレクトリを取れる")
}

fn spec(command: &str) -> CommandSpec {
    CommandSpec {
        name: "case".to_owned(),
        command: command.to_owned(),
        cwd: cwd(),
    }
}

/// 終わらないコマンド。OS ごとに別。
fn long_running() -> (Vec<String>, CommandSpec) {
    if cfg!(windows) {
        (
            vec!["ping".into(), "-n".into(), "30".into()],
            spec("127.0.0.1"),
        )
    } else {
        (vec!["sleep".into()], spec("30"))
    }
}

/// `Exited` か `SpawnFailed` が届くまでのイベントを集める。
fn collect_until_done(rx: &Receiver<RunnerEvent>) -> Vec<RunnerEvent> {
    let deadline = Instant::now() + TIMEOUT;
    let mut events = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let event = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("{TIMEOUT:?} 以内に終了しなかった: {events:?}"));
        let done = matches!(
            event,
            RunnerEvent::Exited { .. } | RunnerEvent::SpawnFailed { .. }
        );
        events.push(event);
        if done {
            return events;
        }
    }
}

fn output_lines(events: &[RunnerEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            RunnerEvent::Output { bytes, .. } => Some(String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        })
        .collect()
}

fn exit_code(events: &[RunnerEvent]) -> Option<i32> {
    match events.last() {
        Some(RunnerEvent::Exited { status, .. }) => status.code(),
        other => panic!("Exited で終わるはずが {other:?}"),
    }
}

#[test]
fn runs_command_and_reports_exit_zero() {
    let (mut runner, rx) = Runner::new(default_shell());

    runner.start(0, &spec("echo hello"));
    let events = collect_until_done(&rx);

    assert!(
        matches!(events.first(), Some(RunnerEvent::Started { id: 0 })),
        "{events:?}"
    );
    assert_eq!(exit_code(&events), Some(0));
    assert_eq!(output_lines(&events), ["hello"]);
}

#[test]
fn propagates_exit_code() {
    let (mut runner, rx) = Runner::new(default_shell());

    runner.start(0, &spec("exit 2"));
    let events = collect_until_done(&rx);

    assert_eq!(exit_code(&events), Some(2));
}

#[test]
fn captures_stdout_and_stderr() {
    let (mut runner, rx) = Runner::new(default_shell());

    runner.start(0, &spec("echo out && echo err 1>&2"));
    let events = collect_until_done(&rx);

    let lines = output_lines(&events);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines.iter().any(|l| l.trim() == "out"), "{lines:?}");
    assert!(lines.iter().any(|l| l.trim() == "err"), "{lines:?}");
}

#[test]
fn output_lines_have_no_line_ending() {
    let (mut runner, rx) = Runner::new(default_shell());

    runner.start(0, &spec("echo hello"));
    let events = collect_until_done(&rx);

    // LF も CRLF（Windows）も含まない
    for event in &events {
        if let RunnerEvent::Output { bytes, .. } = event {
            assert!(
                !bytes.ends_with(b"\n") && !bytes.ends_with(b"\r"),
                "{bytes:?}"
            );
        }
    }
}

#[test]
fn events_carry_the_command_id() {
    let (mut runner, rx) = Runner::new(default_shell());

    runner.start(7, &spec("echo hello"));
    let events = collect_until_done(&rx);

    assert!(
        events.iter().all(|e| match e {
            RunnerEvent::Started { id }
            | RunnerEvent::Output { id, .. }
            | RunnerEvent::Exited { id, .. }
            | RunnerEvent::SpawnFailed { id, .. } => *id == 7,
        }),
        "{events:?}"
    );
}

#[test]
fn missing_shell_is_spawn_failed() {
    let (mut runner, rx) = Runner::new(vec!["runs-test-no-such-program-xyz".to_owned()]);

    runner.start(0, &spec("echo hello"));
    let events = collect_until_done(&rx);

    match events.last() {
        Some(RunnerEvent::SpawnFailed { id: 0, message }) => {
            assert!(
                message.contains("runs-test-no-such-program-xyz"),
                "{message}"
            );
        }
        other => panic!("SpawnFailed のはずが {other:?}"),
    }
    assert!(!runner.is_running(0));
}

#[test]
fn missing_cwd_is_spawn_failed() {
    let (mut runner, rx) = Runner::new(default_shell());
    let mut spec = spec("echo hello");
    spec.cwd = cwd().join("runs-test-no-such-dir-xyz");

    runner.start(0, &spec);
    let events = collect_until_done(&rx);

    match events.last() {
        Some(RunnerEvent::SpawnFailed { message, .. }) => {
            assert!(message.contains("working directory"), "{message}");
            assert!(message.contains("runs-test-no-such-dir-xyz"), "{message}");
        }
        other => panic!("SpawnFailed のはずが {other:?}"),
    }
}

#[test]
fn stop_terminates_long_running_command() {
    let (shell, spec) = long_running();
    let (mut runner, rx) = Runner::new(shell);

    runner.start(0, &spec);
    match rx.recv_timeout(TIMEOUT) {
        Ok(RunnerEvent::Started { id: 0 }) => {}
        other => panic!("Started のはずが {other:?}"),
    }
    assert!(runner.is_running(0));

    let started = Instant::now();
    runner.stop(0);
    let events = collect_until_done(&rx);

    assert!(
        matches!(events.last(), Some(RunnerEvent::Exited { id: 0, .. })),
        "{events:?}"
    );
    // 猶予（2 秒）+ 強制終了より十分前に終わる
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert!(!runner.is_running(0));
}

#[test]
fn stop_all_and_wait_stops_everything() {
    let (shell, spec) = long_running();
    let (mut runner, rx) = Runner::new(shell);

    runner.start(0, &spec);
    runner.start(1, &spec);
    // ping は起動直後に空行を出すので、Output を飛ばして Started を 2 つ待つ
    let mut started = 0;
    while started < 2 {
        match rx.recv_timeout(TIMEOUT) {
            Ok(RunnerEvent::Started { .. }) => started += 1,
            Ok(RunnerEvent::Output { .. }) => {}
            other => panic!("Started のはずが {other:?}"),
        }
    }

    runner.stop_all_and_wait(Duration::from_secs(2));

    assert!(!runner.is_running(0));
    assert!(!runner.is_running(1));
    let mut exited: Vec<_> = rx
        .try_iter()
        .filter_map(|e| match e {
            RunnerEvent::Exited { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    exited.sort_unstable();
    assert_eq!(exited, [0, 1]);
}

#[test]
fn stop_all_and_wait_returns_immediately_without_processes() {
    let (mut runner, _rx) = Runner::new(default_shell());

    let started = Instant::now();
    runner.stop_all_and_wait(Duration::from_secs(2));

    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn is_running_tracks_lifecycle() {
    let (mut runner, rx) = Runner::new(default_shell());
    assert!(!runner.is_running(0));

    runner.start(0, &spec("echo hello"));
    let _ = collect_until_done(&rx);

    assert!(!runner.is_running(0));
}

#[test]
fn start_while_running_is_ignored() {
    let (shell, spec) = long_running();
    let (mut runner, rx) = Runner::new(shell);

    runner.start(0, &spec);
    runner.start(0, &spec);
    runner.stop_all_and_wait(Duration::from_secs(2));

    let started = rx
        .try_iter()
        .filter(|e| matches!(e, RunnerEvent::Started { .. }))
        .count();
    assert_eq!(started, 1);
}
