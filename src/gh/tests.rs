//! `gh` の出力の解釈と失敗の分類を、固定の JSON と `Capture` で確かめる（`gh` は起動しない）。

use std::process::ExitStatus;
use std::time::Duration;

use super::*;

fn exit_status(code: i32) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
}

fn captured(code: i32, stdout: &str, stderr: &str) -> Capture {
    Capture {
        status: Some(exit_status(code)),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
        spawn_error: None,
        timed_out: false,
    }
}

const NOW: u64 = 1_791_443_636; // 2026-10-08T07:13:56Z

const TWO_PRS: &str = r#"[
  {"number": 15, "title": "gh status", "headRefName": "feat/15-gh-status",
   "statusCheckRollup": [
     {"__typename": "CheckRun", "name": "verify (ubuntu-latest)", "status": "COMPLETED", "conclusion": "SUCCESS"},
     {"__typename": "CheckRun", "name": "msrv", "status": "COMPLETED", "conclusion": "SUCCESS"}
   ]},
  {"number": 16, "title": "Fix foo", "headRefName": "fix/16-foo",
   "statusCheckRollup": [
     {"__typename": "CheckRun", "name": "verify (macos-latest)", "status": "IN_PROGRESS", "conclusion": ""},
     {"__typename": "CheckRun", "name": "msrv", "status": "COMPLETED", "conclusion": "SUCCESS"}
   ]}
]"#;

#[test]
fn parses_open_prs_with_check_summary() {
    let prs = parse_prs(TWO_PRS.as_bytes()).expect("読める");
    assert_eq!(
        prs,
        vec![
            PrSummary {
                number: 15,
                title: "gh status".into(),
                branch: "feat/15-gh-status".into(),
                checks: CheckSummary::Ok,
            },
            PrSummary {
                number: 16,
                title: "Fix foo".into(),
                branch: "fix/16-foo".into(),
                checks: CheckSummary::Running,
            },
        ]
    );
}

#[test]
fn failed_check_wins_over_running() {
    let json = r#"[{"number": 1, "title": "t", "headRefName": "b", "statusCheckRollup": [
        {"__typename": "CheckRun", "status": "IN_PROGRESS", "conclusion": null},
        {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "FAILURE"},
        {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS"}
    ]}]"#;
    let prs = parse_prs(json.as_bytes()).expect("読める");
    assert_eq!(prs[0].checks, CheckSummary::Failed);
}

#[test]
fn status_context_and_check_run_mixed() {
    let json = r#"[{"number": 1, "title": "t", "headRefName": "b", "statusCheckRollup": [
        {"__typename": "StatusContext", "context": "ci/external", "state": "SUCCESS"},
        {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "NEUTRAL"},
        {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SKIPPED"}
    ]}]"#;
    assert_eq!(
        parse_prs(json.as_bytes()).expect("読める")[0].checks,
        CheckSummary::Ok
    );

    let pending = r#"[{"number": 1, "title": "t", "headRefName": "b", "statusCheckRollup": [
        {"__typename": "StatusContext", "state": "PENDING"}
    ]}]"#;
    assert_eq!(
        parse_prs(pending.as_bytes()).expect("読める")[0].checks,
        CheckSummary::Running
    );

    let error = r#"[{"number": 1, "title": "t", "headRefName": "b", "statusCheckRollup": [
        {"__typename": "StatusContext", "state": "ERROR"}
    ]}]"#;
    assert_eq!(
        parse_prs(error.as_bytes()).expect("読める")[0].checks,
        CheckSummary::Failed
    );
}

#[test]
fn unknown_check_shape_is_not_mistaken_for_success() {
    // 種類も結論も無い要素は「未完了」。結論だけある要素はそれで判断する
    let json = r#"[{"number": 1, "title": "t", "headRefName": "b", "statusCheckRollup": [{"name": "x"}]},
                   {"number": 2, "title": "t", "headRefName": "b", "statusCheckRollup": [{"conclusion": "SUCCESS"}]}]"#;
    let prs = parse_prs(json.as_bytes()).expect("読める");
    assert_eq!(prs[0].checks, CheckSummary::Running);
    assert_eq!(prs[1].checks, CheckSummary::Ok);
}

#[test]
fn pr_without_checks_is_none_and_missing_keys_are_empty() {
    let json = r#"[{"number": 7}]"#;
    let prs = parse_prs(json.as_bytes()).expect("読める");
    assert_eq!(
        prs,
        vec![PrSummary {
            number: 7,
            title: String::new(),
            branch: String::new(),
            checks: CheckSummary::None,
        }]
    );
}

#[test]
fn empty_list_is_ok() {
    assert_eq!(parse_prs(b"[]").expect("読める"), Vec::new());
    assert_eq!(parse_runs(b"[]", NOW).expect("読める"), Vec::new());
}

#[test]
fn broken_json_is_bad_json() {
    assert_eq!(parse_prs(b"[{").unwrap_err(), GhError::BadJson);
    assert_eq!(parse_runs(b"not json", NOW).unwrap_err(), GhError::BadJson);
    assert_eq!(parse_prs(b"{}").unwrap_err(), GhError::BadJson);
}

#[test]
fn titles_are_sanitized_to_one_line() {
    let json = "[{\"number\": 1, \"title\": \"bad \\u001b[31mtitle\\nsecond\", \"headRefName\": \"br\\u0007anch\"}]";
    let prs = parse_prs(json.as_bytes()).expect("読める");
    // ESC シーケンスは除かれ、他の制御文字は `?` で可視化される（output::sanitize の既存の規則）
    assert_eq!(prs[0].title, "bad title");
    assert_eq!(prs[0].branch, "br?anch");
}

#[test]
fn parses_runs_with_result_and_age() {
    let json = r#"[
      {"headBranch": "main", "status": "completed", "conclusion": "success",
       "displayTitle": "CI を導入する (#14)", "createdAt": "2026-10-08T07:10:56Z"},
      {"headBranch": "feat/x", "status": "in_progress", "conclusion": "",
       "displayTitle": "wip", "createdAt": "2026-10-08T07:13:56Z"},
      {"headBranch": "feat/y", "status": "completed", "conclusion": "failure",
       "displayTitle": "broken", "createdAt": "2026-10-08T06:13:56Z"},
      {"headBranch": "feat/z", "status": "completed", "conclusion": "cancelled",
       "displayTitle": "stopped", "createdAt": "2026-10-08T07:13:56Z"}
    ]"#;
    let runs = parse_runs(json.as_bytes(), NOW).expect("読める");
    assert_eq!(
        runs,
        vec![
            RunSummary {
                branch: "main".into(),
                result: RunResult::Ok,
                age: Duration::from_secs(180),
                title: "CI を導入する (#14)".into(),
            },
            RunSummary {
                branch: "feat/x".into(),
                result: RunResult::Running,
                age: Duration::ZERO,
                title: "wip".into(),
            },
            RunSummary {
                branch: "feat/y".into(),
                result: RunResult::Failed,
                age: Duration::from_secs(3600),
                title: "broken".into(),
            },
            RunSummary {
                branch: "feat/z".into(),
                result: RunResult::Other("cancelled".into()),
                age: Duration::ZERO,
                title: "stopped".into(),
            },
        ]
    );
}

#[test]
fn age_is_zero_when_created_in_future_or_unreadable() {
    let json = r#"[{"headBranch": "main", "status": "completed", "conclusion": "success", "createdAt": "2026-10-08T07:20:00Z"},
                   {"headBranch": "main", "status": "completed", "conclusion": "success", "createdAt": "yesterday"},
                   {"headBranch": "main", "status": "completed", "conclusion": "success"}]"#;
    let runs = parse_runs(json.as_bytes(), NOW).expect("読める");
    assert!(runs.iter().all(|r| r.age == Duration::ZERO), "{runs:?}");
}

#[test]
fn iso8601_to_unix_seconds() {
    assert_eq!(
        parse_iso8601_utc("2026-10-08T07:13:56Z"),
        Some(1_791_443_636)
    );
    assert_eq!(parse_iso8601_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_iso8601_utc("2000-02-29T00:00:00Z"), Some(951_782_400));
    assert_eq!(
        parse_iso8601_utc("2024-02-29T12:34:56Z"),
        Some(1_709_210_096)
    );
    assert_eq!(parse_iso8601_utc("1999-12-31T23:59:59Z"), Some(946_684_799));
    assert_eq!(
        parse_iso8601_utc("2100-03-01T00:00:00Z"),
        Some(4_107_542_400)
    );
    // 小数秒は捨てる
    assert_eq!(
        parse_iso8601_utc("2026-10-08T07:13:56.123Z"),
        Some(1_791_443_636)
    );
}

#[test]
fn iso8601_rejects_other_shapes() {
    for text in [
        "",
        "2026-10-08",
        "2026-10-08T07:13:56",
        "2026-10-08T07:13:56+09:00",
        "2026-13-01T00:00:00Z",
        "2026-10-32T00:00:00Z",
        "2026-10-08T24:00:00Z",
        "1969-12-31T23:59:59Z",
        "abcd-ef-ghTij:kl:mnZ",
    ] {
        assert_eq!(parse_iso8601_utc(text), None, "{text:?}");
    }
}

#[test]
fn classify_success_returns_stdout() {
    let capture = captured(0, "[]", "");
    assert_eq!(classify(&capture), Ok(&b"[]"[..]));
}

#[test]
fn classify_not_found_spawn_error() {
    let capture = Capture {
        spawn_error: Some(std::io::ErrorKind::NotFound),
        ..Capture::default()
    };
    assert_eq!(classify(&capture), Err(GhError::NotFound));
    assert_eq!(GhError::NotFound.to_string(), "gh not found in PATH");

    let denied = Capture {
        spawn_error: Some(std::io::ErrorKind::PermissionDenied),
        ..Capture::default()
    };
    assert!(matches!(
        classify(&denied),
        Err(GhError::Failed { code: None, .. })
    ));
}

#[test]
fn classify_exit_4_is_not_logged_in() {
    let capture = captured(
        4,
        "",
        "To get started with GitHub CLI, please run:  gh auth login\n",
    );
    assert_eq!(classify(&capture), Err(GhError::NotLoggedIn));
    // 終了コードが変わっても案内文で拾う
    let by_message = captured(1, "", "please run gh auth login first");
    assert_eq!(classify(&by_message), Err(GhError::NotLoggedIn));
    assert_eq!(
        GhError::NotLoggedIn.to_string(),
        "gh is not logged in (run: gh auth login)"
    );
}

#[test]
fn classify_not_a_git_repository() {
    let capture = captured(
        1,
        "",
        "failed to run git: fatal: not a git repository (or any of the parent directories): .git\n",
    );
    assert_eq!(classify(&capture), Err(GhError::NotARepository));
}

#[test]
fn classify_timed_out() {
    let capture = Capture {
        timed_out: true,
        ..Capture::default()
    };
    assert_eq!(classify(&capture), Err(GhError::TimedOut));
    assert_eq!(GhError::TimedOut.to_string(), "gh timed out (30s)");
}

#[test]
fn classify_other_failure_keeps_first_stderr_line_sanitized() {
    let capture = captured(
        1,
        "",
        "\n  \x1b[31mHTTP 502\x1b[0m: bad gateway\nsecond line\n",
    );
    let err = classify(&capture).unwrap_err();
    assert_eq!(
        err,
        GhError::Failed {
            code: Some(1),
            message: "HTTP 502: bad gateway".into(),
        }
    );
    assert_eq!(err.to_string(), "gh failed (exit 1): HTTP 502: bad gateway");

    let silent = captured(2, "", "");
    assert_eq!(
        classify(&silent).unwrap_err().to_string(),
        "gh failed (exit 2): no error output"
    );
}

#[test]
fn classify_without_status_is_failure() {
    let capture = Capture::default();
    assert!(matches!(
        classify(&capture),
        Err(GhError::Failed { code: None, .. })
    ));
}

#[test]
fn status_from_keeps_each_side_separate() {
    let pr = captured(4, "", "");
    let run = captured(
        0,
        r#"[{"headBranch": "main", "status": "completed", "conclusion": "success"}]"#,
        "",
    );
    let status = status_from(&pr, &run, NOW);
    assert_eq!(status.prs, Err(GhError::NotLoggedIn));
    assert_eq!(status.runs.as_ref().map(Vec::len), Ok(1));
}
