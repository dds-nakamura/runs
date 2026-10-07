//! 経過時間の短い表記。一覧の列（幅 `WIDTH`）に収める。

use std::time::Duration;

/// 一覧の時間の列の幅（`59m 59s` / `59m ago`）。
pub const WIDTH: usize = 7;

const MINUTE: u64 = 60;
const HOUR: u64 = 60 * MINUTE;
const DAY: u64 = 24 * HOUR;

/// 開始からの経過・所要時間。`12s` / `1m 12s` / `1h 2m` / `1d 1h`
pub fn format_elapsed(duration: Duration) -> String {
    let s = duration.as_secs();
    if s < MINUTE {
        format!("{s}s")
    } else if s < HOUR {
        format!("{}m {}s", s / MINUTE, s % MINUTE)
    } else if s < DAY {
        format!("{}h {}m", s / HOUR, (s % HOUR) / MINUTE)
    } else {
        format!("{}d {}h", s / DAY, (s % DAY) / HOUR)
    }
}

/// 終わってからの経過。最上位の単位だけ。`5s ago` / `3m ago` / `2h ago` / `1d ago`
pub fn format_ago(duration: Duration) -> String {
    let s = duration.as_secs();
    if s < MINUTE {
        format!("{s}s ago")
    } else if s < HOUR {
        format!("{}m ago", s / MINUTE)
    } else if s < DAY {
        format!("{}h ago", s / HOUR)
    } else {
        format!("{}d ago", s / DAY)
    }
}

#[cfg(test)]
mod tests;
