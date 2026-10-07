//! コマンドライン引数の解釈。端末には触らない。

use std::ffi::OsString;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Run,
    Version,
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UsageError {
    message: String,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// プログラム名を除いた引数を解釈する。
///
/// 知らない引数が 1 つでもあればエラー。無ければ最初に書かれたフラグに従う。
pub fn parse<I>(args: I) -> Result<Command, UsageError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut command = None;
    for arg in args {
        let parsed = match arg.to_str() {
            Some("-h" | "--help") => Command::Help,
            Some("-V" | "--version") => Command::Version,
            _ => {
                return Err(UsageError {
                    message: format!("unexpected argument '{}'", sanitize(&arg.to_string_lossy())),
                });
            }
        };
        command.get_or_insert(parsed);
    }
    Ok(command.unwrap_or(Command::Run))
}

pub fn version_text() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

pub fn help_text() -> String {
    format!(
        "{version}\n\
         \n\
         Usage: {name} [OPTIONS]\n\
         \n\
         Options:\n  \
           -h, --help     Print help\n  \
           -V, --version  Print version\n\
         \n\
         Commands are read from {file} in the current directory or a parent:\n\
         \n\
         {example}
\n         
\n         {optional}",
        version = version_text(),
        name = env!("CARGO_PKG_NAME"),
        file = crate::config::FILE_NAME,
        example = crate::config::EXAMPLE,
        optional = crate::config::OPTIONAL_KEYS,
    )
}

/// 引数は外部入力なので、制御文字（ESC など）をそのまま端末に出さない。
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests;
