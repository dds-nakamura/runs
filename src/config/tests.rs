use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\proj")
    } else {
        PathBuf::from("/proj")
    }
}

fn parse_ok(text: &str) -> Config {
    match parse(text, &root()) {
        Ok(config) => config,
        Err(err) => panic!("解釈できるはずが失敗した: {err:#}\n{text}"),
    }
}

fn parse_err(text: &str) -> String {
    match parse(text, &root()) {
        Ok(_) => panic!("エラーになるはずが解釈できた:\n{text}"),
        Err(err) => format!("{err:#}"),
    }
}

/// テストごとに一意な一時ディレクトリ。drop で消す。
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("runs-config-test-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("一時ディレクトリを作れる");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MINIMAL: &str = r#"
[[command]]
name = "test"
command = "cargo test"
"#;

#[test]
fn parses_minimal_config() {
    let config = parse_ok(MINIMAL);

    assert_eq!(config.root, root());
    assert_eq!(config.path, root().join(FILE_NAME));
    assert_eq!(config.commands.len(), 1);
    assert_eq!(config.commands[0].name, "test");
    assert_eq!(config.commands[0].command, "cargo test");
}

#[test]
fn default_shell_per_os() {
    let config = parse_ok(MINIMAL);

    if cfg!(windows) {
        assert_eq!(config.shell, ["cmd", "/S", "/C"]);
    } else {
        assert_eq!(config.shell, ["sh", "-c"]);
    }
}

#[test]
fn custom_shell() {
    let config = parse_ok(&format!(
        "shell = [\"pwsh\", \"-NoProfile\", \"-Command\"]\n{MINIMAL}"
    ));

    assert_eq!(config.shell, ["pwsh", "-NoProfile", "-Command"]);
}

#[test]
fn default_encoding_per_os() {
    let config = parse_ok(MINIMAL);

    if cfg!(windows) {
        assert_eq!(config.encoding, Some(encoding_rs::SHIFT_JIS));
    } else {
        assert_eq!(config.encoding, None);
    }
}

#[test]
fn custom_encoding_label() {
    let config = parse_ok(&format!("encoding = \"windows-1252\"\n{MINIMAL}"));
    assert_eq!(config.encoding, Some(encoding_rs::WINDOWS_1252));

    // WHATWG のラベルなので大文字小文字や別名も通る
    let config = parse_ok(&format!("encoding = \"Shift_JIS\"\n{MINIMAL}"));
    assert_eq!(config.encoding, Some(encoding_rs::SHIFT_JIS));
}

#[test]
fn utf8_encoding_means_no_fallback() {
    let config = parse_ok(&format!("encoding = \"utf-8\"\n{MINIMAL}"));

    assert_eq!(config.encoding, None);
}

#[test]
fn unknown_encoding_is_error() {
    let message = parse_err(&format!("encoding = \"klingon\"\n{MINIMAL}"));

    assert!(message.contains("encoding"), "{message}");
    assert!(message.contains("klingon"), "{message}");
}

#[test]
fn cwd_defaults_to_root() {
    let config = parse_ok(MINIMAL);

    assert_eq!(config.commands[0].cwd, root());
}

#[test]
fn relative_and_absolute_cwd() {
    let absolute = std::env::temp_dir();
    let config = parse_ok(&format!(
        r#"
[[command]]
name = "web"
command = "npm run dev"
cwd = "web"

[[command]]
name = "abs"
command = "ls"
cwd = {absolute:?}
"#
    ));

    assert_eq!(config.commands[0].cwd, root().join("web"));
    assert_eq!(config.commands[1].cwd, absolute);
}

#[test]
fn duplicate_name_is_error() {
    let message = parse_err(
        r#"
[[command]]
name = "test"
command = "a"

[[command]]
name = "test"
command = "b"
"#,
    );

    assert!(message.contains("test"), "{message}");
    assert!(message.contains("duplicate"), "{message}");
}

#[test]
fn missing_required_field_is_error_with_line() {
    let message = parse_err(
        r#"
[[command]]
name = "test"
"#,
    );

    assert!(message.contains("command"), "{message}");
    assert!(message.contains("line"), "{message}");
}

#[test]
fn syntax_error_is_error_with_line() {
    let message = parse_err("[[command]\nname = \"x\"\n");

    assert!(message.contains("line 1"), "{message}");
}

#[test]
fn empty_shell_is_error() {
    let message = parse_err(&format!("shell = []\n{MINIMAL}"));

    assert!(message.contains("shell"), "{message}");
}

#[test]
fn empty_name_is_error() {
    let message = parse_err(
        r#"
[[command]]
name = ""
command = "a"
"#,
    );

    assert!(message.contains("name"), "{message}");
}

#[test]
fn no_commands_is_error() {
    let message = parse_err("shell = [\"sh\", \"-c\"]\n");

    assert!(message.contains("[[command]]"), "{message}");
}

#[test]
fn preserves_order() {
    let config = parse_ok(
        r#"
[[command]]
name = "c"
command = "3"

[[command]]
name = "a"
command = "1"

[[command]]
name = "b"
command = "2"
"#,
    );

    let names: Vec<_> = config.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["c", "a", "b"]);
}

#[test]
fn load_searches_parent_directories() {
    let temp = TempDir::new("parents");
    let project = temp.path().join("project");
    let nested = project.join("src").join("deep");
    fs::create_dir_all(&nested).expect("入れ子のディレクトリを作れる");
    fs::write(project.join(FILE_NAME), MINIMAL).expect("設定ファイルを書ける");

    let config = load(&nested).expect("親ディレクトリの設定を見つける");

    assert_eq!(config.root, project);
    assert_eq!(config.path, project.join(FILE_NAME));
    assert_eq!(config.commands[0].cwd, project);
}

#[test]
fn load_file_reads_the_given_file() {
    let temp = TempDir::new("load-file");
    let path = temp.path().join("custom.toml");
    fs::write(&path, MINIMAL).expect("設定ファイルを書ける");

    let config = load_file(&path).expect("読める");

    assert_eq!(config.path, path);
    assert_eq!(config.root, temp.path());
    assert_eq!(config.commands[0].name, "test");
}

#[test]
fn load_file_reports_missing_file() {
    let temp = TempDir::new("load-file-missing");

    let message = format!(
        "{:#}",
        load_file(&temp.path().join("none.toml")).expect_err("無いファイルはエラー")
    );

    assert!(message.contains("none.toml"), "{message}");
}

#[test]
fn load_without_file_is_error_with_example() {
    let temp = TempDir::new("missing");
    // 親に runs.toml が無いことを前提にできないので、見つかった場合は検索の起点を変えて確かめる
    let Err(err) = load(temp.path()) else {
        eprintln!("注意: 一時ディレクトリの上位に runs.toml があるため、このケースは判定できない");
        return;
    };
    let message = format!("{err:#}");

    assert!(message.contains(FILE_NAME), "{message}");
    assert!(message.contains("[[command]]"), "{message}");
}

#[test]
fn load_reports_syntax_error_with_file_name() {
    let temp = TempDir::new("broken");
    fs::write(temp.path().join(FILE_NAME), "[[command]\n").expect("設定ファイルを書ける");

    let message = format!("{:#}", load(temp.path()).expect_err("壊れた設定はエラー"));

    assert!(message.contains(FILE_NAME), "{message}");
    assert!(message.contains("line 1"), "{message}");
}
