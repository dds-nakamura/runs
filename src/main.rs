#![forbid(unsafe_code)]

mod app;
mod cli;
mod tui;
mod ui;

use std::io::{self, Write};
use std::process::ExitCode;

use crate::app::App;
use crate::cli::Command;

/// 実行時エラー（端末でない・初期化失敗・I/O）。
const EXIT_FAILURE: u8 = 1;
/// 引数の誤り。
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    match cli::parse(std::env::args_os().skip(1)) {
        Ok(Command::Run) => run(),
        Ok(Command::Version) => print(&cli::version_text()),
        Ok(Command::Help) => print(&cli::help_text()),
        Err(err) => {
            report(&format!("{err}\n\n{}", cli::help_text()));
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run() -> ExitCode {
    let mut app = App::new(cli::version_text());
    // tui::run が返った時点で端末は復元済みなので、ここで出すメッセージは通常の画面に残る
    match tui::run(&mut app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&format!("{err:#}"));
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// stdout に出す。`println!` は使わない（相手が先に閉じたパイプへ書くと panic する）。
fn print(text: &str) -> ExitCode {
    match writeln!(io::stdout().lock(), "{text}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILURE),
    }
}

/// stderr にエラーを出す。`eprintln!` は使わない（`print` と同じ理由）。
fn report(message: &str) {
    // stderr に書けないときは伝える先が無い。失敗は終了コードで示す
    let _ = writeln!(io::stderr().lock(), "runs: {message}");
}
