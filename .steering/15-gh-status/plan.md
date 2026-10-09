# Plan: gh 経由で PR と CI の状態を画面に表示する（spec.md 2026-10-09 より）

課題 #15。ブランチ `feat/15-gh-status`。2026-10-09 にプランモードで承認。

## Context

PR やマージ後の CI の結果を見るのに runs を離れて `gh` を叩いている。`g` キーで `gh pr list` / `gh run list` を非同期に実行し、
右側の区画を output ↔ status（`Tab`）で切り替えて表示する。spec で決めたこと: 右区画の切り替え（案 C）、各 10 件・スクロール無し、
`serde_json` を追加、HELP 行の既存不具合（81 桁でタイトルと重なる）も直す。`gh` の失敗（未導入・未ログイン・非リポジトリ・タイムアウト）は理由を表示する。
方針「プロセスに触るのは runner だけ」「App は `Instant::now()` を呼ばない」「状態・更新・描画を分ける」を守る。

## 変更するファイル

- `Cargo.toml`（変更）: `serde_json = { version = "1", default-features = false, features = ["std"] }`（spec C3 で合意済み。`cargo add` は確認が入る）
- `src/gh.rs`（新規）+ `src/gh/tests.rs`（新規）: 純粋関数。プロセス・ファイル・端末に触らない
  - 型: `PrSummary { number: u64, title: String, branch: String, checks: CheckSummary }`、`CheckSummary { Ok, Failed, Running, None }`、
    `RunSummary { branch: String, result: RunResult, age: Duration, title: String }`、`RunResult { Ok, Failed, Running, Other(String) }`、
    `GhError { NotFound, NotLoggedIn, NotARepository, TimedOut, Failed { code: Option<i32>, message: String }, BadJson }`（`Display` で spec A6 の英文）、
    `GhStatus { prs: Result<Vec<PrSummary>, GhError>, runs: Result<Vec<RunSummary>, GhError> }`
  - 定数 `PR_ARGS` / `RUN_ARGS`（spec のとおり。`--limit 10`）、`LIMIT: usize = 10`
  - `fn classify(capture: &runner::Capture) -> Result<&[u8], GhError>`（spawn_error NotFound → NotFound、timed_out → TimedOut、code 4 → NotLoggedIn、stderr に `not a git repository` → NotARepository、非ゼロ → Failed{code, stderr 1 行目を sanitize}、成功 → stdout）
  - `fn parse_prs(json: &[u8]) -> Result<Vec<PrSummary>, GhError>`（`serde_json::from_slice` → `#[derive(Deserialize)]` の生の構造体（`#[serde(default)]` で欠損に耐える）→ 要約。タイトル・ブランチは `output::sanitize` して 1 行目のみ）
  - `fn parse_runs(json: &[u8], now_unix: u64) -> Result<Vec<RunSummary>, GhError>`（`age = now_unix.saturating_sub(created)`。`createdAt` が読めなければ age 0）
  - `fn summarize_checks(rollup: &[RawCheck]) -> CheckSummary`（spec の規則。`CheckRun` は status/conclusion、`StatusContext` は state）
  - `fn parse_iso8601_utc(s: &str) -> Option<u64>`（`YYYY-MM-DDTHH:MM:SSZ` のみ。days-from-civil で UNIX 秒。範囲外は None）
- `src/runner.rs`（変更）+ `src/runner/tests.rs`（追加）
  - `pub struct Capture { pub status: Option<ExitStatus>, pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub spawn_error: Option<io::ErrorKind>, pub timed_out: bool }`
  - `pub enum GhEvent { Fetched { pr: Capture, run: Capture, now_unix: u64 } }`
  - `pub const FETCH_TIMEOUT: Duration = 30s`
  - `fn capture(program: &str, args: &[&str], cwd: &Path, timeout: Duration, live: &LivePids) -> Capture`: `Command::new(program).args(args).current_dir(cwd)`、stdin null、stdout/stderr piped、
    `env("NO_COLOR","1").env("GH_PAGER","").env("GH_PROMPT_DISABLED","1").env("GH_NO_UPDATE_NOTIFIER","1")`、Unix は `process_group(0)`。
    spawn 失敗は `spawn_error = Some(err.kind())`。stdout / stderr は各 1 スレッドで `read_to_end`。本体は `try_wait` を `POLL_INTERVAL` で回し、`timeout` を過ぎたら `force_kill(pid)` + `child.kill()` して `timed_out = true`。pid は `live` に insert / remove
  - `pub fn fetch_gh(&mut self, program: &str, cwd: &Path, tx: Sender<GhEvent>)`: 取得中（`gh_fetch: Option<JoinHandle<()>>` が未完了）なら何もしない。スレッドを起こし、`capture(PR_ARGS)` → `capture(RUN_ARGS)` の順に実行し、
    `now_unix = SystemTime::now().duration_since(UNIX_EPOCH)` を付けて `GhEvent::Fetched` を送る（`SystemTime` は runner 側で取る。App は受け取るだけ）
  - 終了時: `stop_all_and_wait` の冒頭で、gh の pid（`gh_pid: Arc<Mutex<Option<u32>>>` をスレッドと共有）があれば `force_kill` する。`Drop` も同様。`LivePids` に入れているので緊急 KILL の対象にもなる
  - テスト（`sh -c` / `cmd /S /C` で偽コマンド。`config::default_shell()` を流用）: 標準出力を全部返す／終了コードが取れる／stderr が取れる／存在しないプログラムは `spawn_error = NotFound`／`sleep 30` 相当を 1 秒の timeout で `timed_out = true` かつプロセスが残らない／`fetch_gh` を `program = "nonexistent-gh"` で呼ぶと `GhEvent::Fetched` が届き両方 `spawn_error` が NotFound
- `src/app.rs`（変更）+ `src/app/tests.rs`（追加）
  - `Action::{FetchGh, TogglePane}`、`Effect::FetchGh`、`pub enum RightPane { Output, Status }`、`pub enum GhPanel { NotFetched, Fetching { since: Instant }, Ready { status: gh::GhStatus, at: Instant } }`
  - `App` に `pane: RightPane`、`gh: GhPanel`。`action_for`: `Char('g') => FetchGh`、`Tab => TogglePane`
  - `apply(FetchGh)`: `Fetching` 中なら `set_notice("already fetching")`（`apply` 冒頭の `notice = None` の後に設定）で `[]`。それ以外は `gh = Fetching { since: now }`、`pane = Status`、`[Effect::FetchGh]`。`apply(TogglePane)`: `pane` を反転、`[]`
  - `pub fn on_gh_event(&mut self, event: GhEvent)`: `classify` → `parse_*` で `GhStatus` を作り `Ready { status, at: now }`
  - `needs_tick()`: 既存の条件に `|| (pane == Status && !matches!(gh, NotFetched))` を足す
  - getter: `pane()`, `gh_panel()`
  - テスト: `g` → `Effect::FetchGh` と `pane == Status`／取得中の `g` は空で notice／`Tab` の反転／`on_gh_event` で `Ready`（固定 JSON の Capture を組み立てる）／`needs_tick` が status 表示中だけ true／`r`（Reload）で `GhPanel` が消えない
- `src/ui.rs`（変更）+ `src/ui/tests.rs`（変更・追加）
  - `HELP` を `"Up/Dn select  Enter run  s stop  r reload  g gh  Tab pane  PgUp/Dn/End scroll  q quit"`（78 桁）に
  - `draw_help`: `title_width + 2 + HELP の幅 <= area.width` のときだけタイトルを出す（足りなければ `Fill(1)` のみ）
  - `draw`: `app.pane()` で右側を `draw_header` / `draw_output`（Output）か `draw_status_header` / `draw_status`（Status）に振り分け
  - `draw_status_header`: `gh status  (press g to fetch)` / `gh status  fetching... Ns`（`format_elapsed`）/ `gh status  fetched Ns ago`（`format_ago`）
  - `draw_status`: 行を組み立てて `Paragraph`。`Pull requests (open)` → 各 PR ` #N branch  ok     title~`（番号 5 桁右寄せ、ブランチは最大幅 20 に `truncate_to_width`、要約 6 桁、残りをタイトル）、`Recent CI runs` → ` branch  ok  3m ago  title~`。
    片方が `Err` なら見出しの下に ` error: <GhError の Display>`。0 件は ` (none)`。行数が `area.height` を超えたら最後の行を ` ... N more` に置き換える（`height == 0` なら何も描かない）
  - テスト: `help_row()` ヘルパーをタイトル表示の規則に合わせる（80 桁では HELP 78 + 2 > 70 なのでタイトル無し。既存 6 件はヘルパー経由なので追随。90 桁でタイトルが出るテストを 1 件足す）。
    status 区画のスナップショット（80x24）: 取得前（`Tab` 後の `press g to fetch`）、取得中（`fetching... 3s`）、PR 2 件 + runs 2 件、PR 側だけ `error: gh is not logged in (run: gh auth login)`、11 件で `... 1 more`、日本語タイトルの切り詰め。極小サイズ（`does_not_panic_at_tiny_sizes` に `pane = Status` の分岐を足す）
- `src/tui.rs`（変更）: `Runner::new` はそのまま。`run` で `let (gh_tx, gh_rx) = mpsc::channel::<GhEvent>()` を作り、`event_loop` の引数に `config.root` と `gh_tx` / `gh_rx` を足す。
  `handle_effects` に `Effect::FetchGh => runner.fetch_gh("gh", root, gh_tx.clone())`。受信ループの隣に `while let Ok(e) = gh_rx.try_recv() { app.on_gh_event(e); dirty = true; }`
- `src/cli.rs`（変更）: `help_text` に `Keys:` 節を足す（`Up/Down, j/k select / Enter run (restart if running) / s stop / r reload runs.toml / g fetch PR and CI status with gh / Tab switch output and status / PgUp/PgDn/End scroll / q, Ctrl+C quit`）。`tests/cli.rs` の help 系テストは「含む」判定なので影響なし（確認する）
- `src/main.rs`（変更）: `mod gh;` を足す
- `CLAUDE.md`（変更）: キーの行に `g` / `Tab`、Architecture に `src/gh.rs` と `runner::capture` / `GhEvent` の 1 行、依存に `serde_json`
- `.steering/15-gh-status/plan.md`（新規）、spec.md（ずれたら該当節を直す）

## 作業順

1. `gh` モジュールの型と関数のシグネチャだけ書き、`src/gh/tests.rs` を先に書く（PR 2 件の固定 JSON、`StatusContext` 混在、`IN_PROGRESS`、失敗 1 件、空配列、壊れた JSON、ISO 8601 の境界（2026-10-08T07:13:56Z = 1791443636、うるう年）、`classify` の 6 分岐）→ 失敗を確認 → 実装
   - `cargo add serde_json --no-default-features --features std`（確認が入る）→ `cargo deny check` と `Cargo.lock` の差分（増える crate の確認）
2. `runner::capture` と `fetch_gh`。テストを先に書く（上記）。Windows / Unix 両方で動く偽コマンド（`echo` / `exit` / `ping -n` と `sleep`）
3. `App`: `Action` / `Effect` / `RightPane` / `GhPanel` / `on_gh_event` / `needs_tick`。テスト先行
4. `ui`: HELP と `draw_help` の規則 → `help_row` を直して既存テストが通ることを確認 → status 区画の描画とスナップショット
5. `tui` の配線、`cli` の Keys 節、`main` の `mod gh`、CLAUDE.md
6. 各段階で `bash .claude/scripts/verify.sh`。段階ごとにコミット（gh モジュール＋依存 / runner / app / ui / tui・cli・文書）
7. 手元の実機確認（ユーザーに依頼、Windows Terminal。手順は短く番号付きで別メッセージ）: `g` で取得 → status 表示 → `Tab` で戻る → PR のある状態（この PR 自身）で `ok` / `run..` が出る → `gh` を PATH から外した PowerShell から起動して `gh not found in PATH`。WSL では `pty-check.sh` は変更不要（キー追加のケースは無くても可）
8. verifier → `/pr`（reviewer）

## リスク

- **`Capture` の読み取りスレッドとタイムアウト**: stdout / stderr を別スレッドで読まないとパイプが詰まる。タイムアウトで KILL した後、読み取りスレッドは EOF で終わる（孫が無いので待たない）。`join` は `READER_GRACE` と同じ考えで上限付き
- **`gh` の終了コード**: 未ログインは 4（手元で確認済み）。他の版で変わる可能性 → stderr の `gh auth login` の文字列も見る二段構え
- **Windows の `gh.exe` 解決**: `Command::new("gh")` は PATH の `gh.exe` を見つける（std の挙動）。`.cmd` / `.bat` ラッパーは解決しないが一般的な導入では無い。実機確認で確かめる
- **`statusCheckRollup` の形**: `CheckRun` と `StatusContext` の 2 種類。`__typename` が無い要素は `conclusion` / `state` のどちらかがあれば解釈し、無ければ `Running` 扱い（安全側: 「成功」と誤認しない）
- **タイトルの表示幅**: 日本語タイトルは `truncate_to_width` で全角の途中を切らない（既存関数）。`Line::width` は ratatui の unicode-width 相当
- **HELP の変更で既存テストが変わる**: `help_row` ヘルパーの規則変更で 6 件が追随。期待値の直書きは無い（棚卸し済み）。`notice_replaces_help_line` は notice 側なので影響を確認
- **`needs_tick` の増加**: status 表示中は毎秒描画。Output に戻せば従来どおり
- **`Reload` との関係**: `replace_config` は `gh` / `pane` に触らない（取得結果は残る）。`config.root` は再読み込みで変わらない
- **終了時**: `stop_all_and_wait` で gh を `force_kill` → 最長でも既存の猶予内。`GhEvent` の受信側が消えていても `send` の失敗は捨てる
- **`SystemTime` が UNIX_EPOCH より前**: `unwrap_or_default()` で 0 にし、age は `saturating_sub`

## 証明（Proof）

- `bash .claude/scripts/verify.sh --all` が `VERIFY OK`（`cargo deny check` で `serde_json` の追加分も ok）
- 単体テスト（名前は実装時に確定。以下は予定）
  - `gh::tests`: `parses_open_prs_with_check_summary`, `status_context_and_check_run_mixed`, `in_progress_check_is_running`, `failed_check_wins_over_running`, `empty_list_is_ok`, `broken_json_is_bad_json`, `classify_exit_4_is_not_logged_in`, `classify_not_found_spawn_error`, `classify_not_a_git_repository`, `classify_timed_out`, `iso8601_to_unix_seconds`, `age_is_zero_when_created_in_future`, `titles_are_sanitized`
  - `runner::tests`: `capture_collects_stdout_stderr_and_status`, `capture_reports_not_found`, `capture_times_out_and_kills`, `fetch_gh_sends_one_event_for_both_commands`, `fetch_gh_ignores_second_call_while_running`
  - `app::tests`: `g_starts_fetch_and_shows_status_pane`, `g_while_fetching_sets_notice`, `tab_toggles_pane`, `gh_event_makes_panel_ready`, `needs_tick_while_status_pane_is_shown`, `reload_keeps_gh_panel`
  - `ui::tests`: `status_pane_before_fetch`, `status_pane_while_fetching`, `status_pane_renders_prs_and_runs_at_80x24`, `status_pane_shows_error_for_failed_side`, `status_pane_truncates_with_more_line`, `help_hides_title_when_narrow`, `help_shows_title_when_wide`、`does_not_panic_at_tiny_sizes` の拡張
- `tests/cli.rs`: `help_exits_zero` が通り、`--help` の出力に `Keys:` と `g` / `Tab` が含まれる（`help_mentions_keys` を追加）
- 実機確認（ユーザー、Windows Terminal。未検証なら明記）: 手順 7 の 4 項目。WSL で `cargo test` が通ること（`sh -c` の偽コマンド）
- CI（PR の 4 ジョブ）が緑。macOS の `capture` テストも通ること

## 並行可能な作業

`gh` モジュール（手順 1）と `runner::capture`（手順 2）は独立。`App` / `ui` は両方のシグネチャが決まれば並行できるが、1 人で順にやる想定。

## 捨てた案

- `RunnerEvent` に gh の variant を足す: `on_runner_event` の `RunId` 前提の 2 つの `match` と既存テストに波及。別 enum `GhEvent` + 別チャネルに
- `gh` を既存の「コマンド」として `Runner::start` で起動（出力欄に流す）: シェル経由になり、JSON を行単位で受けて組み立て直す必要がある。構造化表示ができない
- `gh --jq` で整形して読む: JSON 解釈が gh 側に寄り、テストしにくい（spec C3 で不採用）
- `chrono` / `time` の追加: UTC の 1 形式だけなので自前の換算で足りる
- 常時表示の区画 / 全画面ビュー: spec C1 で不採用
- `wait_with_output` でタイムアウト無し: ネットワーク待ちで戻らないと終了が遅れる。`try_wait` ポーリング + KILL に
