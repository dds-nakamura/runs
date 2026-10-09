//! 描画（状態 → 画面）。`TestBackend` にも描けるよう、実端末には触らない。

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use crate::app::{App, CommandState, CommandView, GhPanel, RightPane};
use crate::gh::{CheckSummary, GhStatus, PrSummary, RunResult, RunSummary};
use crate::timefmt;

/// 最下行のキーの案内。79 桁（80 桁の端末に収まる。タイトルはさらに幅があるときだけ右端に出す）。
/// 全部のキーは載せない（`End` など）。一覧は `--help` の Keys に書く
const HELP: &str =
    "Up/Dn move  Enter run  s stop  r reload  g gh  Tab pane  PgUp/Dn scroll  q quit";
/// status 区画のブランチ名の列の最大幅。
const BRANCH_MAX_WIDTH: usize = 20;
/// status 区画の結果の列の幅（`run..` / `FAIL`）。
const RESULT_WIDTH: usize = 6;
/// 状態の表記の最大幅（`exit 255`）。
const STATUS_WIDTH: usize = 8;
/// 選択中の印の幅（`> `）。
const MARK_WIDTH: usize = 2;
/// 一覧の幅の上限（画面幅に対する割合）。
const LIST_MAX_PERCENT: u32 = 40;
/// 幅が足りないときも名前に残す最低の桁数。
const MIN_NAME_WIDTH: usize = 6;

/// 画面の区画。`draw` と `tui::run`（出力欄の高さを `App` に渡す）の両方が使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panes {
    pub list: Rect,
    /// 出力欄の見出し（選択中のコマンドの名前・状態・時間）
    pub header: Rect,
    pub output: Rect,
    pub help: Rect,
}

/// 左に一覧、右に見出しと選択中の出力、最下行にキーの案内（または通知）。
///
/// 一覧の幅は「印 + 名前の最大幅 + 空白 + 状態 + 空白 + 時間」、ただし画面幅の 40% まで。
pub fn layout(area: Rect, app: &App) -> Panes {
    let [main, help] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    // u16 の掛け算は 65535 × 40 であふれるので u32 で計算する
    let cap = u16::try_from(u32::from(area.width) * LIST_MAX_PERCENT / 100).unwrap_or(u16::MAX);
    let wanted =
        u16::try_from(MARK_WIDTH + name_width(app) + 1 + STATUS_WIDTH + 1 + timefmt::WIDTH)
            .unwrap_or(u16::MAX);
    let [list, _gap, right] = Layout::horizontal([
        Constraint::Length(wanted.min(cap)),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(main);
    let [header, output] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(right);

    Panes {
        list,
        header,
        output,
        help,
    }
}

pub fn draw(frame: &mut Frame, app: &App) {
    let panes = layout(frame.area(), app);
    draw_list(frame, panes.list, app);
    match app.pane() {
        RightPane::Output => {
            draw_header(frame, panes.header, app);
            draw_output(frame, panes.output, app);
        }
        RightPane::Status => {
            draw_status_header(frame, panes.header, app);
            draw_status(frame, panes.output, app);
        }
    }
    draw_help(frame, panes.help, app);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    // 幅の上限で詰まったら、切るのは名前。状態と時間の列は残す（Paragraph の右端切りに任せない）。
    // ただし名前が全く読めなくなる極小幅では、名前を最低 MIN_NAME_WIDTH 桁出し、右の列が切れるのを許す
    let fixed = MARK_WIDTH + 1 + STATUS_WIDTH + 1 + timefmt::WIDTH;
    let full = name_width(app);
    let available = usize::from(area.width).saturating_sub(fixed);
    let name_width = if available >= full {
        full
    } else {
        available.max(full.min(MIN_NAME_WIDTH))
    };
    let now = app.now();
    let lines: Vec<Line> = app
        .commands()
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let mark = if index == app.selected() { "> " } else { "  " };
            // `{:<w$}` は文字数で埋めるので、全角を含む名前は表示幅で切って埋める
            let name = pad_to_width(&truncate_to_width(command.name(), name_width), name_width);
            Line::raw(format!(
                "{mark}{name} {:<STATUS_WIDTH$} {:<time_width$}",
                fit_status(&status_label(command)),
                time_label(command, now),
                time_width = timefmt::WIDTH,
            ))
        })
        .collect();
    // はみ出した分は Paragraph が切る（折り返さない）
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let Some(command) = app.selected_command() else {
        return;
    };
    let mut text = command.name().to_owned();
    if command.state() != CommandState::Idle {
        text.push_str("  ");
        text.push_str(&status_label(command));
    }
    if let Some(elapsed) = command.elapsed(app.now()) {
        text.push(' ');
        text.push_str(&timefmt::format_elapsed(elapsed));
    } else if let Some(took) = command.took() {
        text.push_str("  took ");
        text.push_str(&timefmt::format_elapsed(took));
    }
    frame.render_widget(Line::raw(text), area);
}

fn draw_output(frame: &mut Frame, area: Rect, app: &App) {
    let Some(command) = app.selected_command() else {
        return;
    };
    let range = app.visible_range();
    let lines: Vec<Line> = command
        .output()
        .lines()
        .skip(range.start)
        .take(range.len())
        .map(Line::raw)
        .collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn draw_help(frame: &mut Frame, area: Rect, app: &App) {
    // 通知があればキーの案内の代わりに出す（次の操作で消える）
    let text = Line::raw(app.notice().unwrap_or(HELP));
    let title = Line::raw(app.title());
    // タイトルは、キーの案内の右に空白 2 桁を挟んで収まるときだけ出す（通知の有無で出たり消えたりしないよう、HELP の幅で判断）
    let needed = Line::raw(HELP).width() + 2 + title.width();
    if usize::from(area.width) >= needed {
        let title_width = u16::try_from(title.width()).unwrap_or(u16::MAX);
        let [left, title_area] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(title_width)]).areas(area);
        frame.render_widget(text, left);
        frame.render_widget(title, title_area);
    } else {
        frame.render_widget(text, area);
    }
}

/// status 区画の見出し: 取得の状態と、取得からの経過。
fn draw_status_header(frame: &mut Frame, area: Rect, app: &App) {
    let text = match app.gh_panel() {
        GhPanel::NotFetched => "gh status  (press g to fetch)".to_owned(),
        GhPanel::Fetching { since } => format!(
            "gh status  fetching... {}",
            timefmt::format_elapsed(app.now().saturating_duration_since(*since))
        ),
        GhPanel::Ready { at, .. } => format!(
            "gh status  fetched {}",
            timefmt::format_ago(app.now().saturating_duration_since(*at))
        ),
    };
    frame.render_widget(Line::raw(text), area);
}

/// status 区画の本文: open な PR と最近の CI 実行。収まらない行は最後の 1 行を `... N more` にして省く
fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let height = usize::from(area.height);
    if height == 0 {
        return;
    }
    let GhPanel::Ready { status, .. } = app.gh_panel() else {
        return;
    };
    let mut lines = status_lines(status, usize::from(area.width));
    if lines.len() > height {
        let hidden = lines.len() - (height - 1);
        lines.truncate(height - 1);
        lines.push(format!(" ... {hidden} more"));
    }
    let lines: Vec<Line> = lines.into_iter().map(Line::raw).collect();
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

fn status_lines(status: &GhStatus, width: usize) -> Vec<String> {
    let mut lines = vec!["Pull requests (open)".to_owned()];
    match &status.prs {
        Ok(prs) if prs.is_empty() => lines.push(" (none)".to_owned()),
        Ok(prs) => {
            let branch_width = branch_column_width(prs.iter().map(|pr| pr.branch.as_str()));
            lines.extend(prs.iter().map(|pr| pr_line(pr, branch_width, width)));
        }
        Err(err) => lines.push(format!(" error: {err}")),
    }
    lines.push("Recent CI runs".to_owned());
    match &status.runs {
        Ok(runs) if runs.is_empty() => lines.push(" (none)".to_owned()),
        Ok(runs) => {
            let branch_width = branch_column_width(runs.iter().map(|run| run.branch.as_str()));
            lines.extend(runs.iter().map(|run| run_line(run, branch_width, width)));
        }
        Err(err) => lines.push(format!(" error: {err}")),
    }
    lines
}

/// ` #15   feat/15-gh-status ok     タイトル`。タイトルは残りの幅に収まるよう切る
fn pr_line(pr: &PrSummary, branch_width: usize, width: usize) -> String {
    let branch = pad_to_width(&truncate_to_width(&pr.branch, branch_width), branch_width);
    let prefix = format!(
        " #{:<4} {branch} {:<RESULT_WIDTH$} ",
        pr.number,
        check_label(pr.checks)
    );
    with_title(prefix, &pr.title, width)
}

/// ` main   ok     3m ago  タイトル`。
fn run_line(run: &RunSummary, branch_width: usize, width: usize) -> String {
    let branch = pad_to_width(&truncate_to_width(&run.branch, branch_width), branch_width);
    let prefix = format!(
        " {branch} {:<RESULT_WIDTH$} {:<time_width$} ",
        truncate_to_width(result_label(&run.result), RESULT_WIDTH),
        timefmt::format_ago(run.age),
        time_width = timefmt::WIDTH,
    );
    with_title(prefix, &run.title, width)
}

fn with_title(prefix: String, title: &str, width: usize) -> String {
    let remaining = width.saturating_sub(Line::raw(prefix.as_str()).width());
    format!("{prefix}{}", truncate_to_width(title, remaining))
}

/// ブランチ名の列の幅: 最長のブランチ名、ただし上限あり。
fn branch_column_width<'a>(branches: impl Iterator<Item = &'a str>) -> usize {
    branches
        .map(|b| Line::raw(b).width())
        .max()
        .unwrap_or(0)
        .min(BRANCH_MAX_WIDTH)
}

fn check_label(checks: CheckSummary) -> &'static str {
    match checks {
        CheckSummary::Ok => "ok",
        CheckSummary::Failed => "FAIL",
        CheckSummary::Running => "run..",
        CheckSummary::None => "none",
    }
}

fn result_label(result: &RunResult) -> &str {
    match result {
        RunResult::Ok => "ok",
        RunResult::Failed => "FAIL",
        RunResult::Running => "run..",
        RunResult::Other(other) => other,
    }
}

/// 表示幅が `width` に収まるまで末尾の文字を落とす（全角の途中で切らない）。切ったときは末尾を `~` にして、
/// 先頭が同じ名前（`frontend-dev-server` と `frontend-dev-client`）が同じに見えないようにする。
fn truncate_to_width(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = Line::raw(c.to_string()).width();
        if used + w > width.saturating_sub(1) {
            break;
        }
        used += w;
        out.push(c);
    }
    if width > 0 {
        out.push('~');
    }
    out
}

/// 一覧の状態の列に収まらない表記（Windows の大きな終了コードなど）は `exit ?` にする。全文は見出し行に出る
fn fit_status(label: &str) -> String {
    if label.len() > STATUS_WIDTH {
        "exit ?".to_owned()
    } else {
        label.to_owned()
    }
}

/// 表示幅が `width` になるまで右に空白を足す。
fn pad_to_width(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(Line::raw(text).width());
    format!("{text}{}", " ".repeat(padding))
}

fn name_width(app: &App) -> usize {
    app.commands()
        .iter()
        .map(|c| Line::raw(c.name()).width())
        .max()
        .unwrap_or(0)
}

fn status_label(command: &CommandView) -> String {
    match command.state() {
        CommandState::Idle => "idle".to_owned(),
        CommandState::Running => "running".to_owned(),
        CommandState::Exited { code: Some(code) } => format!("exit {code}"),
        CommandState::Exited { code: None } => "exit ?".to_owned(),
        CommandState::Stopped => "stopped".to_owned(),
        CommandState::SpawnFailed => "failed".to_owned(),
    }
}

/// 一覧の時間の列。実行中は経過、終わっていれば何分前。未実行は空
fn time_label(command: &CommandView, now: std::time::Instant) -> String {
    if let Some(elapsed) = command.elapsed(now) {
        timefmt::format_elapsed(elapsed)
    } else if let Some(ago) = command.ago(now) {
        timefmt::format_ago(ago)
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests;
