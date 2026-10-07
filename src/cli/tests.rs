use std::ffi::OsString;

use super::*;

fn parse_args(args: &[&str]) -> Result<Command, UsageError> {
    parse(args.iter().map(OsString::from))
}

fn error_message(args: &[&str]) -> String {
    match parse_args(args) {
        Ok(command) => panic!("エラーになるはずが {command:?} になった: {args:?}"),
        Err(err) => err.to_string(),
    }
}

#[test]
fn no_args_runs() {
    assert_eq!(parse_args(&[]), Ok(Command::Run));
}

#[test]
fn version_flags() {
    assert_eq!(parse_args(&["--version"]), Ok(Command::Version));
    assert_eq!(parse_args(&["-V"]), Ok(Command::Version));
}

#[test]
fn help_flags() {
    assert_eq!(parse_args(&["--help"]), Ok(Command::Help));
    assert_eq!(parse_args(&["-h"]), Ok(Command::Help));
}

#[test]
fn unknown_argument_is_error() {
    assert!(error_message(&["--bogus"]).contains("--bogus"));
    assert!(error_message(&["-v"]).contains("-v"));
    assert!(error_message(&["file.txt"]).contains("file.txt"));
}

#[test]
fn extra_argument_is_error() {
    // 先に有効なフラグがあっても、知らない引数が 1 つでもあればエラー
    assert!(error_message(&["--help", "extra"]).contains("extra"));
    assert!(error_message(&["--version", "--bogus"]).contains("--bogus"));
}

#[test]
fn first_flag_wins() {
    assert_eq!(parse_args(&["--version", "--help"]), Ok(Command::Version));
    assert_eq!(parse_args(&["-h", "-V"]), Ok(Command::Help));
}

#[test]
fn control_chars_in_argument_are_replaced() {
    // 引数は外部入力。ESC をそのまま端末に出すと画面を操作される
    let message = error_message(&["\u{1b}[2J\u{7}\u{9b}x"]);

    assert!(!message.chars().any(char::is_control), "{message:?}");
    assert!(message.contains("?[2J??x"), "{message:?}");
}

#[cfg(unix)]
#[test]
fn non_utf8_argument_is_error() {
    use std::os::unix::ffi::OsStringExt;

    let result = parse([OsString::from_vec(vec![b'-', 0xff])]);

    assert!(result.is_err());
}

#[cfg(windows)]
#[test]
fn invalid_utf16_argument_is_error() {
    use std::os::windows::ffi::OsStringExt;

    // 対になっていないサロゲート。UTF-8 に変換できない
    let result = parse([OsString::from_wide(&[u16::from(b'-'), 0xd800])]);

    assert!(result.is_err());
}

#[test]
fn version_text_is_name_and_version() {
    assert_eq!(
        version_text(),
        format!("runs {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_text_lists_options() {
    let help = help_text();

    assert!(help.starts_with(&version_text()), "{help}");
    assert!(help.contains("Usage: runs [OPTIONS]"), "{help}");
    assert!(help.contains("-h, --help"), "{help}");
    assert!(help.contains("-V, --version"), "{help}");
    assert!(help.is_ascii(), "{help}");
}

#[test]
fn help_text_explains_config_file() {
    let help = help_text();

    assert!(help.contains("runs.toml"), "{help}");
    assert!(help.contains("[[command]]"), "{help}");
}
