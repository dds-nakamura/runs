//! バイナリを起動して、引数・終了コード・端末でない場所での挙動を確かめる。
//!
//! `output()` は stdout / stderr をパイプにするので、子プロセスから見ると端末ではない。

use std::process::{Command, Output, Stdio};

fn runs(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_runs"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("runs を起動できる")
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
fn no_args_without_tty_exits_1() {
    let output = runs(&[]);

    assert_eq!(output.status.code(), Some(1));
    // 端末に触っていないこと: alternate screen に入るエスケープシーケンスなどを出していない
    assert_eq!(stdout(&output), "");
    let stderr = stderr(&output);
    assert!(stderr.contains("must be a terminal"), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr:?}");
}
