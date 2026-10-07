//! `runs.toml` の探索・読み込み・検証。端末には触らない。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use encoding_rs::Encoding;
use serde::Deserialize;

pub const FILE_NAME: &str = "runs.toml";

/// `--help` とエラーメッセージに出す最小の例。
pub const EXAMPLE: &str = "\
[[command]]
name = \"test\"
command = \"cargo test\"";

/// `--help` に出す、省略できる項目の説明。
pub const OPTIONAL_KEYS: &str = "\
Optional top-level keys:
  shell = [\"pwsh\", \"-NoProfile\", \"-Command\"]   # default: sh -c (Unix), cmd /S /C (Windows)
  encoding = \"shift_jis\"                       # for output that is not UTF-8 (default: shift_jis on Windows, none elsewhere;
                                               #  UTF-16 and ISO-2022-JP are not supported)
Optional per-command key: cwd = \"web\"           # relative to runs.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 読んだ設定ファイル（再読み込みに使う）。
    pub path: PathBuf,
    /// 設定ファイルのあるディレクトリ（プロジェクトルート）。
    pub root: PathBuf,
    /// コマンドを渡すシェル。最後の引数として `CommandSpec::command` を付ける。
    pub shell: Vec<String>,
    /// UTF-8 として読めない出力行を読み直す文字コード。`None` なら読み直さない
    pub encoding: Option<&'static Encoding>,
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
    /// 文字コードのラベル（`shift_jis`、`windows-1252` など）。`utf-8` なら読み直さない
    encoding: Option<String>,
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
    load_file(&path)
}

/// 指定したファイルを読み込んで検証する（再読み込みでも使う）。
pub fn load_file(path: &Path) -> Result<Config> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    // 設定ファイルは必ずディレクトリの中にあるので parent は存在する
    let root = path.parent().unwrap_or(path);
    let mut config = parse(&text, root).with_context(|| format!("invalid {}", path.display()))?;
    config.path = path.to_path_buf();
    Ok(config)
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
    let encoding = match raw.encoding {
        None => default_encoding(),
        Some(label) => parse_encoding(&label)?,
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
        path: root.join(FILE_NAME),
        root: root.to_path_buf(),
        shell,
        encoding,
        commands,
    })
}

/// `encoding` の値を文字コードにする。`utf-8` は「読み直さない」。
///
/// 出力は `\n` で行に切ってから読み直すので、UTF-16（`\n` が 2 バイト）と ISO-2022-JP（7 ビットなので
/// UTF-8 として正しく、読み直しが起きない）は扱えない。replacement 系のラベルも拒む
fn parse_encoding(label: &str) -> Result<Option<&'static Encoding>> {
    // ラベルは外部入力。メッセージには Debug 書式（制御文字をエスケープ）で出す
    let Some(encoding) = Encoding::for_label_no_replacement(label.trim().as_bytes()) else {
        bail!(
            "unknown `encoding`: {:?} (examples: \"shift_jis\", \"windows-1252\", \"utf-8\")",
            label
        );
    };
    if encoding.output_encoding() != encoding || encoding == encoding_rs::ISO_2022_JP {
        bail!(
            "`encoding` {:?} cannot be used for line-based output (UTF-16 and ISO-2022-JP are not supported)",
            label
        );
    }
    Ok((encoding != encoding_rs::UTF_8).then_some(encoding))
}

/// OS ごとの既定の文字コード。日本語環境の Windows の標準コマンドは CP932（Shift_JIS）で出力する。
/// Unix は UTF-8 だけ（読み直さない）
pub fn default_encoding() -> Option<&'static Encoding> {
    if cfg!(windows) {
        Some(encoding_rs::SHIFT_JIS)
    } else {
        None
    }
}

/// OS ごとの既定のシェル。
pub fn default_shell() -> Vec<String> {
    if cfg!(windows) {
        // /S: 先頭と末尾の `"` だけを外す（runner がコマンド全体を `"` で包んで渡す）
        vec!["cmd".to_owned(), "/S".to_owned(), "/C".to_owned()]
    } else {
        vec!["sh".to_owned(), "-c".to_owned()]
    }
}

#[cfg(test)]
mod tests;
