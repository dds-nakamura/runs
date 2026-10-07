use std::time::Duration;

use super::*;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

#[test]
fn elapsed_seconds() {
    assert_eq!(format_elapsed(secs(0)), "0s");
    assert_eq!(format_elapsed(Duration::from_millis(1_999)), "1s");
    assert_eq!(format_elapsed(secs(59)), "59s");
}

#[test]
fn elapsed_minutes() {
    assert_eq!(format_elapsed(secs(60)), "1m 0s");
    assert_eq!(format_elapsed(secs(72)), "1m 12s");
    assert_eq!(format_elapsed(secs(3_599)), "59m 59s");
}

#[test]
fn elapsed_hours() {
    assert_eq!(format_elapsed(secs(3_600)), "1h 0m");
    assert_eq!(format_elapsed(secs(3_600 + 120 + 5)), "1h 2m");
    assert_eq!(format_elapsed(secs(86_399)), "23h 59m");
}

#[test]
fn elapsed_days() {
    assert_eq!(format_elapsed(secs(86_400)), "1d 0h");
    assert_eq!(format_elapsed(secs(90_000)), "1d 1h");
}

#[test]
fn ago_uses_largest_unit() {
    assert_eq!(format_ago(secs(0)), "0s ago");
    assert_eq!(format_ago(secs(5)), "5s ago");
    assert_eq!(format_ago(secs(59)), "59s ago");
    assert_eq!(format_ago(secs(180)), "3m ago");
    assert_eq!(format_ago(secs(3_599)), "59m ago");
    assert_eq!(format_ago(secs(7_200)), "2h ago");
    assert_eq!(format_ago(secs(86_400)), "1d ago");
    assert_eq!(format_ago(secs(99 * 86_400)), "99d ago");
}

#[test]
fn never_wider_than_seven_columns() {
    // 1 秒〜99 日の代表値（WIDTH は一覧の列幅）
    for n in [
        0,
        1,
        9,
        59,
        60,
        599,
        3_599,
        3_600,
        35_999,
        86_399,
        86_400,
        99 * 86_400,
    ] {
        assert!(format_elapsed(secs(n)).len() <= WIDTH, "{n}");
        assert!(format_ago(secs(n)).len() <= WIDTH, "{n}");
    }
}
