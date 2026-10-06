//! `runs.toml` の探索・読み込み・検証。端末には触らない。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const FILE_NAME: &str = "runs.toml";

/// `--help` とエラーメッセージに出す最小の例。
pub const EXAMPLE: &str = "\
[[command]]
name = \"test\"
command = \"cargo test\"";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 設定ファイルのあるディレクトリ（プロジェクトルート）。
    pub root: PathBuf,
    /// コマンドを渡すシェル。最後の引数として `CommandSpec::command` を付ける。
    pub shell: Vec<String>,
    pub commands: Vec<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: String,
    pub command: String,
    /// `root` で解決済みの作業ディレクトリ。
    pub cwd: PathBuf,
}

/// ファイルの形そのまま。検証して `Config` に変換する。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    shell: Option<Vec<String>>,
    #[serde(default)]
    command: Vec<RawCommand>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    name: String,
    command: String,
    cwd: Option<PathBuf>,
}

/// `start_dir` から親へ向かって `runs.toml` を探し、読み込んで検証する。
pub fn load(start_dir: &Path) -> Result<Config> {
    let Some(path) = find(start_dir) else {
        bail!(
            "{FILE_NAME} not found in {} or any parent directory.\n\
             Create one like this:\n\n{EXAMPLE}",
            start_dir.display()
        );
    };
    let text =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    // 設定ファイルは必ずディレクトリの中にあるので parent は存在する
    let root = path.parent().unwrap_or(start_dir);
    parse(&text, root).with_context(|| format!("invalid {}", path.display()))
}

fn find(start_dir: &Path) -> Option<PathBuf> {
    start_dir
        .ancestors()
        .map(|dir| dir.join(FILE_NAME))
        .find(|candidate| candidate.is_file())
}

/// 設定ファイルの中身を解釈して検証する。`root` は相対パスの基準。
pub fn parse(text: &str, root: &Path) -> Result<Config> {
    // toml のエラーは行・列と該当箇所を含むので、そのまま見せる
    let raw: RawConfig = toml::from_str(text)?;

    let shell = match raw.shell {
        None => default_shell(),
        Some(shell) if shell.is_empty() => bail!("`shell` must not be empty"),
        Some(shell) => shell,
    };
    if raw.command.is_empty() {
        bail!("no commands defined. Add at least one [[command]]:\n\n{EXAMPLE}");
    }

    let mut names = HashSet::new();
    let mut commands = Vec::with_capacity(raw.command.len());
    for entry in raw.command {
        if entry.name.trim().is_empty() {
            bail!("`name` must not be empty (command: {:?})", entry.command);
        }
        if !names.insert(entry.name.clone()) {
            bail!("duplicate command name: {:?}", entry.name);
        }
        let cwd = match entry.cwd {
            // 相対パスは root 基準。絶対パスなら join はそのまま返す
            Some(cwd) => root.join(cwd),
            None => root.to_path_buf(),
        };
        commands.push(CommandSpec {
            name: entry.name,
            command: entry.command,
            cwd,
        });
    }

    Ok(Config {
        root: root.to_path_buf(),
        shell,
        commands,
    })
}

/// OS ごとの既定のシェル。
pub fn default_shell() -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".to_owned(), "/C".to_owned()]
    } else {
        vec!["sh".to_owned(), "-c".to_owned()]
    }
}

#[cfg(test)]
mod tests;
