//! `gh` の出力（`--json`）の解釈と、失敗の分類。プロセス・ファイル・端末には触らない（起動は `runner::fetch_gh`）。
//!
//! 表示する文字列（タイトル・ブランチ名・エラーの本文）は外部入力なので、ここで `output::sanitize` を通して 1 行目だけにする。

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::output;
use crate::runner::{Capture, FETCH_TIMEOUT};

/// 1 回の取得で見せる件数の上限（PR / CI 実行それぞれ）。
pub const LIMIT: &str = "10";

/// open な PR の一覧。チェックの結果は `statusCheckRollup` にまとめて入る
pub const PR_ARGS: &[&str] = &[
    "pr",
    "list",
    "--limit",
    LIMIT,
    "--json",
    "number,title,headRefName,statusCheckRollup",
];

/// 最近の CI 実行の一覧（main への push を含む）。
pub const RUN_ARGS: &[&str] = &[
    "run",
    "list",
    "--limit",
    LIMIT,
    "--json",
    "headBranch,status,conclusion,displayTitle,createdAt",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub branch: String,
    pub checks: CheckSummary,
}

/// PR のチェック全体の要約。1 つでも失敗なら `Failed`、失敗が無く未完了があれば `Running`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckSummary {
    Ok,
    Failed,
    Running,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSummary {
    pub branch: String,
    pub result: RunResult,
    /// 取得時点から見た、実行が作られてからの経過
    pub age: Duration,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunResult {
    Ok,
    Failed,
    Running,
    /// `cancelled` / `skipped` など。表示は小文字のまま
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhError {
    /// PATH に `gh` が無い
    NotFound,
    /// 終了コード 4、または stderr が `gh auth login` を促している（未ログインのほか、トークンの失効も含む）
    NotLoggedIn,
    NotARepository,
    TimedOut,
    Failed {
        code: Option<i32>,
        message: String,
    },
    BadJson,
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("gh not found in PATH"),
            Self::NotLoggedIn => f.write_str("gh is not logged in (run: gh auth login)"),
            Self::NotARepository => f.write_str("not a git repository"),
            Self::TimedOut => write!(f, "gh timed out ({}s)", FETCH_TIMEOUT.as_secs()),
            Self::Failed {
                code: Some(code),
                message,
            } => write!(f, "gh failed (exit {code}): {message}"),
            Self::Failed {
                code: None,
                message,
            } => write!(f, "gh failed: {message}"),
            Self::BadJson => f.write_str("unexpected gh output"),
        }
    }
}

/// 1 回の取得の結果。PR と CI 実行は別々に失敗しうる
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhStatus {
    pub prs: Result<Vec<PrSummary>, GhError>,
    pub runs: Result<Vec<RunSummary>, GhError>,
}

/// 2 つの `Capture` から表示用の結果を組み立てる。
pub fn status_from(pr: &Capture, run: &Capture, now_unix: u64) -> GhStatus {
    GhStatus {
        prs: classify(pr).and_then(parse_prs),
        runs: classify(run).and_then(|json| parse_runs(json, now_unix)),
    }
}

/// 起動の失敗・終了コード・stderr から失敗の種類を決める。成功なら stdout を返す
pub fn classify(capture: &Capture) -> Result<&[u8], GhError> {
    if let Some(kind) = capture.spawn_error {
        return Err(match kind {
            std::io::ErrorKind::NotFound => GhError::NotFound,
            // runner が起動前に確かめる（runs.toml のあるディレクトリが消えた）
            std::io::ErrorKind::NotADirectory => GhError::Failed {
                code: None,
                message: "working directory does not exist".to_owned(),
            },
            other => GhError::Failed {
                code: None,
                message: format!("failed to start gh: {other}"),
            },
        });
    }
    if capture.timed_out {
        return Err(GhError::TimedOut);
    }
    let Some(status) = capture.status else {
        return Err(GhError::Failed {
            code: None,
            message: "no exit status".to_owned(),
        });
    };
    if status.success() {
        return Ok(&capture.stdout);
    }
    let stderr = String::from_utf8_lossy(&capture.stderr);
    // 未ログインは終了コード 4。版で変わっても stderr の案内文で拾う
    if status.code() == Some(4) || stderr.contains("gh auth login") {
        return Err(GhError::NotLoggedIn);
    }
    if stderr.contains("not a git repository") {
        return Err(GhError::NotARepository);
    }
    let first_line = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no error output");
    Err(GhError::Failed {
        code: status.code(),
        message: output::sanitize(first_line),
    })
}

/// `gh` が返す JSON の要素。欠けているキーや `null` は空として読む（`gh` の版や状態で出ないことがある）
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawPr {
    number: u64,
    title: Option<String>,
    head_ref_name: Option<String>,
    status_check_rollup: Option<Vec<RawCheck>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawCheck {
    #[serde(rename = "__typename")]
    typename: Option<String>,
    /// `CheckRun`: `COMPLETED` / `IN_PROGRESS` / `QUEUED` など
    status: Option<String>,
    /// `CheckRun`: `SUCCESS` / `FAILURE` / `NEUTRAL` / `SKIPPED` など（未完了は空か null）
    conclusion: Option<String>,
    /// `StatusContext`: `SUCCESS` / `PENDING` / `EXPECTED` / `FAILURE` / `ERROR`
    state: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct RawRun {
    head_branch: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    display_title: Option<String>,
    created_at: Option<String>,
}

pub fn parse_prs(json: &[u8]) -> Result<Vec<PrSummary>, GhError> {
    let raw: Vec<RawPr> = serde_json::from_slice(json).map_err(|_| GhError::BadJson)?;
    Ok(raw
        .into_iter()
        .map(|pr| PrSummary {
            number: pr.number,
            title: display_text(pr.title.as_deref()),
            branch: display_text(pr.head_ref_name.as_deref()),
            checks: summarize_checks(pr.status_check_rollup.as_deref().unwrap_or(&[])),
        })
        .collect())
}

pub fn parse_runs(json: &[u8], now_unix: u64) -> Result<Vec<RunSummary>, GhError> {
    let raw: Vec<RawRun> = serde_json::from_slice(json).map_err(|_| GhError::BadJson)?;
    Ok(raw
        .into_iter()
        .map(|run| {
            let created = run
                .created_at
                .as_deref()
                .and_then(parse_iso8601_utc)
                .unwrap_or(now_unix);
            RunSummary {
                branch: display_text(run.head_branch.as_deref()),
                result: run_result(run.status.as_deref(), run.conclusion.as_deref()),
                // 端末の時計が遅れていて「未来」の実行があっても 0 にする
                age: Duration::from_secs(now_unix.saturating_sub(created)),
                title: display_text(run.display_title.as_deref()),
            }
        })
        .collect())
}

/// チェック 1 つの結果。種類が分からない要素は、成功と誤認しないよう「未完了」にする
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    Ok,
    Failed,
    Running,
}

fn check_state(check: &RawCheck) -> CheckState {
    let conclusion = check.conclusion.as_deref().unwrap_or("");
    let state = check.state.as_deref().unwrap_or("");
    match check.typename.as_deref() {
        Some("CheckRun") => {
            if check.status.as_deref() == Some("COMPLETED") {
                conclusion_state(conclusion)
            } else {
                CheckState::Running
            }
        }
        Some("StatusContext") => context_state(state),
        _ if !conclusion.is_empty() => conclusion_state(conclusion),
        _ if !state.is_empty() => context_state(state),
        _ => CheckState::Running,
    }
}

fn conclusion_state(conclusion: &str) -> CheckState {
    match conclusion {
        "SUCCESS" | "NEUTRAL" | "SKIPPED" => CheckState::Ok,
        "" => CheckState::Running,
        _ => CheckState::Failed,
    }
}

fn context_state(state: &str) -> CheckState {
    match state {
        "SUCCESS" => CheckState::Ok,
        "PENDING" | "EXPECTED" | "" => CheckState::Running,
        _ => CheckState::Failed,
    }
}

fn summarize_checks(checks: &[RawCheck]) -> CheckSummary {
    if checks.is_empty() {
        return CheckSummary::None;
    }
    let states: Vec<CheckState> = checks.iter().map(check_state).collect();
    if states.contains(&CheckState::Failed) {
        CheckSummary::Failed
    } else if states.contains(&CheckState::Running) {
        CheckSummary::Running
    } else {
        CheckSummary::Ok
    }
}

fn run_result(status: Option<&str>, conclusion: Option<&str>) -> RunResult {
    if status != Some("completed") {
        return RunResult::Running;
    }
    match conclusion.unwrap_or("") {
        "success" => RunResult::Ok,
        "failure" | "timed_out" | "startup_failure" => RunResult::Failed,
        "" => RunResult::Other("unknown".to_owned()),
        other => RunResult::Other(output::sanitize(other)),
    }
}

/// 外部入力の文字列を表示用に: 制御文字を除き、1 行目だけ。
fn display_text(text: Option<&str>) -> String {
    let first_line = text.unwrap_or("").lines().next().unwrap_or("");
    output::sanitize(first_line)
}

/// `YYYY-MM-DDTHH:MM:SSZ`（小数秒があってもよい）を UNIX 秒にする。それ以外の形や 1970 年より前は `None`
pub fn parse_iso8601_utc(text: &str) -> Option<u64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date_parts = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (
        date_parts.next()??,
        date_parts.next()??,
        date_parts.next()??,
    );
    // 小数秒は捨てる
    let time = time.split('.').next()?;
    let mut time_parts = time.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (hour, minute, second) = (
        time_parts.next()??,
        time_parts.next()??,
        time_parts.next()??,
    );
    // 外部入力なので範囲を絞る（年に上限が無いと日数の計算があふれる。存在しない日付も通さない）
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&second)
    {
        return None;
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds).ok()
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

/// 1970-01-01 からの日数（グレゴリオ暦。Howard Hinnant の days_from_civil）。年は 1970〜9999 に絞ってから呼ぶ
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests;
