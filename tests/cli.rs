//! バイナリを起動して、引数・終了コード・端末でない場所での挙動を確かめる。
//!
//! `output()` は stdout / stderr をパイプにするので、子プロセスから見ると端末ではない。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn runs(args: &[&str]) -> Output {
    runs_in(Path::new(env!("CARGO_MANIFEST_DIR")), args)
}

/// `dir` をカレントディレクトリにして起動する。
fn runs_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_runs"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("runs を起動できる")
}

/// テストごとに一意な一時ディレクトリ。drop で消す。
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("runs-cli-test-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("一時ディレクトリを作れる");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout は UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr は UTF-8")
}

#[test]
fn version_prints_to_stdout() {
    let output = runs(&["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("runs {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(stderr(&output), "");
}

#[test]
fn short_version_flag() {
    let output = runs(&["-V"]);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout(&output), stdout(&runs(&["--version"])));
}

#[test]
fn help_exits_zero() {
    for flag in ["--help", "-h"] {
        let output = runs(&[flag]);

        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert!(stdout(&output).contains("Usage: runs [OPTIONS]"), "{flag}");
        assert_eq!(stderr(&output), "", "{flag}");
    }
}

#[test]
fn unknown_argument_exits_2() {
    let output = runs(&["--bogus"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("unexpected argument '--bogus'"), "{stderr}");
    assert!(stderr.contains("Usage: runs [OPTIONS]"), "{stderr}");
}

#[test]
fn help_mentions_runs_toml() {
    let output = runs(&["--help"]);

    assert!(stdout(&output).contains("runs.toml"));
}

#[test]
fn no_config_exits_1_with_hint() {
    let temp = TempDir::new("no-config");
    // 一時ディレクトリの上位に runs.toml があると判定できないので、その場合は飛ばす
    if temp
        .0
        .ancestors()
        .any(|dir| dir.join("runs.toml").is_file())
    {
        eprintln!("注意: 一時ディレクトリの上位に runs.toml があるため、このケースは判定できない");
        return;
    }

    let output = runs_in(&temp.0, &[]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("runs.toml not found"), "{stderr}");
    assert!(stderr.contains("[[command]]"), "{stderr}");
}

#[test]
fn broken_config_exits_1_with_line_number() {
    let temp = TempDir::new("broken-config");
    fs::write(temp.0.join("runs.toml"), "[[command]\nname = \"x\"\n").expect("設定を書ける");

    let output = runs_in(&temp.0, &[]);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("runs.toml"), "{stderr}");
    assert!(stderr.contains("line 1"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
}

#[test]
fn no_args_without_tty_exits_1() {
    // リポジトリ直下には runs.toml があるので、設定は通り、端末でないことで止まる
    let output = runs(&[]);

    assert_eq!(output.status.code(), Some(1));
    // 端末に触っていないこと: alternate screen に入るエスケープシーケンスなどを出していない
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("must be a terminal"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
}
