# Spec: gh 経由で PR と CI の状態を画面に表示する（intent.md 2026-10-09 より）

課題: #15。ブランチ `feat/15-gh-status`。

## 要件

### 機能要件（受け入れ条件）

- [ ] A1. `g` を押すと取得が始まり、右側の区画が「status 区画」に切り替わって `fetching...` と経過秒が出る。取得中もコマンド一覧の `↑↓ jk` / `Enter` / `s` / `r` は従来どおり効く（取得は別スレッド。UI は止まらない）
- [ ] A2. 取得が終わると status 区画に次が出る
  - `Pull requests (open)`: 1 行 1 PR。`#番号`、ブランチ名、チェックの要約（`ok` = すべて成功、`FAIL` = 1 つでも失敗、`run..` = 1 つでも実行中・待ち（失敗が無いとき）、`none` = チェック無し）、タイトル。幅に収まらない分は末尾を `~` で切る（既存の名前の切り詰めと同じ）
  - `Recent CI runs`: 1 行 1 実行。ブランチ名、結果（`ok` / `FAIL` / `run..` / その他は conclusion の小文字）、`3m ago` 形式の経過（取得時点基準）、タイトル
  - 上限は PR 10 件、実行 10 件（`gh ... --limit 10`）。区画に収まらない行は出さず、最後に `... N more` を出す（スクロールは無し）
  - どちらも 0 件なら `(none)`
- [ ] A3. status 区画のヘッダ行（既存のヘッダ 1 行）に `gh status  fetched 12s ago` のように取得からの経過が出て、1 秒ごとに更新される
- [ ] A4. `Tab` で右側の区画を output ↔ status に切り替える（取得はしない）。取得前に `Tab` で status に切り替えると `press g to fetch` が出る。output 区画に戻ればスクロール位置は維持される
- [ ] A5. 取得中に `g` を押しても二重に起動しない（最下行に `already fetching` を 1 回出す）。取得後の `g` は再取得
- [ ] A6. 失敗したときは status 区画の本文に理由が 1 行で出て、他の機能は影響を受けない
  - `gh` が PATH に無い → `gh not found in PATH`
  - 終了コード 4 → `gh is not logged in (run: gh auth login)`
  - stderr に `not a git repository` を含む → `not a git repository`
  - 30 秒で終わらない → `gh timed out (30s)`（プロセスは KILL する）
  - それ以外の非ゼロ終了 → `gh failed (exit N): <stderr の 1 行目>`
  - `runs.toml` のあるディレクトリが消えている → `gh failed: working directory does not exist`（実装時に追加）
  - JSON が読めない → `unexpected gh output`
  - 2 つのコマンド（PR / runs）の片方だけ失敗したときは、成功した側は表示し、失敗した側の見出しの下に理由を出す
- [ ] A7. 起動時・再読み込み時には `gh` を呼ばない。`g` を押すまでネットワークに触れない
- [ ] A8. 表示する文字列（タイトル・ブランチ名・stderr）は `output::sanitize` を通し、制御文字・ESC シーケンスが画面に出ない
- [ ] A9. 終了（`q` / Ctrl+C / シグナル）のとき、取得中の `gh` があれば既存の全停止と同じ経路で止める
- [ ] A10. 既存のキーと `runs.toml` の互換: 新しい設定キーは無し。既存のキーの意味は変わらない。`--help` に `g` と `Tab` が載る
- [ ] A11. `gh` が無い環境（CI の `cargo test`）でも全テストが通る。JSON の解釈・失敗の分類・経過時間の計算は固定入力の単体テスト、プロセス起動は `sh` / `cmd` で JSON を出す偽コマンドで確認する

### 非機能要件

- UI のブロックなし: 取得は別スレッド。`event::poll(50 ms)` のループは変えない
- 3 OS: `gh` の起動はシェルを経由せず `Command::new("gh")`（Windows では `gh.exe` に解決される）。Unix では既存コマンドと同じく `process_group(0)` を付け、停止経路（`kill -- -pid` / `taskkill /T /F`）を共有する
- 端末の復元・panic の方針は既存どおり（新しいスレッドは panic させない。`gh` の読み取り失敗はイベントで返す）
- 極小サイズ: status 区画は高さ 0 でも panic しない（既存の `does_not_panic_at_tiny_sizes` に status 区画のケースを足す）
- 幅: 行の組み立ては表示幅（`unicode-width` 相当の既存ヘルパー）で切る。日本語タイトルで崩れない
- 環境変数: `gh` には `NO_COLOR=1`、`GH_PAGER=`（空）、`GH_PROMPT_DISABLED=1`、`GH_NO_UPDATE_NOTIFIER=1` を付けて起動し、色・ページャー・対話・更新通知を抑える。
  利用者の環境の `CLICOLOR_FORCE` / `GH_FORCE_TTY` は `NO_COLOR` より優先されて `--json` の出力に色が付く（JSON が読めなくなる）ので外す（reviewer 指摘、2026-10-09）。stdin は null

## 設計

### 変更方針

- 「状態 / 更新 / 描画」の分離と、「プロセスに触るのは `runner` だけ、`App` は時刻を `set_now` で受け取る」を守る
- `gh` の JSON 解釈と失敗の分類は新モジュール `gh` に置き、プロセスにもファイルにも触らない純粋関数にする（単体テストの対象）
- 取得の通知は `RunnerEvent` を増やさず、別の enum `GhEvent` と別チャネルで返す（`on_runner_event` の `RunId` 前提の 2 つの `match` と既存テストを壊さない。impact-analyzer の評価）
- 表示は右側の区画を output と status で切り替える（案 C）。コマンド一覧・ヘッダ・ヘルプの位置は変わらないので、既存の 80x24 スナップショットテストと `layout` の Rect テストは変わらない。status 区画の描画テストは新規に足す

### 型・状態遷移

```rust
// src/gh.rs（新規。プロセス・ファイル・端末に触らない）
pub struct PrSummary { pub number: u64, pub title: String, pub branch: String, pub checks: CheckSummary }
pub enum CheckSummary { Ok, Failed, Running, None }
pub struct RunSummary { pub branch: String, pub result: RunResult, pub age: Duration, pub title: String }
pub enum RunResult { Ok, Failed, Running, Other(String) }
pub enum GhError { NotFound, NotLoggedIn, NotARepository, TimedOut, Failed { code: Option<i32>, message: String }, BadJson }
pub struct GhStatus { pub prs: Result<Vec<PrSummary>, GhError>, pub runs: Result<Vec<RunSummary>, GhError> }

pub const PR_ARGS: &[&str]  = &["pr", "list", "--limit", "10", "--json", "number,title,headRefName,statusCheckRollup"];
pub const RUN_ARGS: &[&str] = &["run", "list", "--limit", "10", "--json", "headBranch,status,conclusion,displayTitle,createdAt"];

pub fn parse_prs(json: &[u8]) -> Result<Vec<PrSummary>, GhError>;
pub fn parse_runs(json: &[u8], now_unix: u64) -> Result<Vec<RunSummary>, GhError>;  // age = now - createdAt（負なら 0）
pub fn classify(capture: &runner::Capture) -> Result<&[u8], GhError>;              // 起動失敗・終了コード・stderr から GhError を決め、成功なら stdout
pub fn status_from(pr: &Capture, run: &Capture, now_unix: u64) -> GhStatus;        // App が使う入口
pub fn parse_iso8601_utc(s: &str) -> Option<u64>;                                   // "2026-10-08T07:13:56Z" → UNIX 秒。年は 1970〜9999、存在しない日付は None
// 実装時の変更: 入出力は &str ではなく &[u8]（Capture の stdout をそのまま渡す）。LIMIT は引数に埋める &str
// 作業ディレクトリが無いとき（spawn_error = NotADirectory）は `gh failed: working directory does not exist`（reviewer 指摘）
```

- チェックの要約（`statusCheckRollup` の各要素）: `__typename == "CheckRun"` は `status` が `COMPLETED` なら `conclusion`（`SUCCESS` / `NEUTRAL` / `SKIPPED` → 成功扱い、それ以外 → 失敗）、未完了なら実行中。`__typename == "StatusContext"` は `state`（`SUCCESS` → 成功、`PENDING` / `EXPECTED` → 実行中、それ以外 → 失敗）。要素が 0 なら `None`。1 つでも失敗 → `Failed`、失敗が無く実行中あり → `Running`、全部成功 → `Ok`
- `RunResult`: `status != "completed"` → `Running`、`conclusion` が `success` → `Ok`、`failure` / `timed_out` / `startup_failure` → `Failed`、その他（`cancelled` / `skipped` など）→ `Other(conclusion)`
- 失敗の分類（`classify`）: 起動失敗（`NotFound` 種別の io エラー）→ `NotFound`、終了コード 4 → `NotLoggedIn`、stderr に `not a git repository` → `NotARepository`、タイムアウト → `TimedOut`、その他の非ゼロ → `Failed { code, message: stderr の 1 行目を sanitize }`

```rust
// src/runner.rs に追加
pub struct Capture { pub status: Option<ExitStatus>, pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub spawn_error: Option<io::ErrorKind>, pub timed_out: bool }
pub enum GhEvent { Fetched { pr: Capture, run: Capture, now_unix: u64 } }
impl Runner {
    /// `gh` を 2 回（PR / runs）順に実行し、両方の結果を 1 つの GhEvent で送る。別スレッド。stdin null、stdout/stderr piped、
    /// 環境変数（上記）付き、Unix は process_group(0)。pid を LivePids に登録し、終了時の全停止と緊急 KILL の対象に入れる。
    /// 各コマンドは FETCH_TIMEOUT（30 秒）で KILL する。program は既定 "gh"（テストでは sh / cmd に差し替える）
    pub fn fetch_gh(&mut self, program: &str, cwd: &Path, tx: Sender<GhEvent>);
}
```

- `fetch_gh` の中身は汎用の `capture(program, args, cwd, timeout, &GhHandles) -> Capture`（`try_wait` を 50 ms で回し、タイムアウトか `wait` の失敗で `force_kill` + `child.kill` + `wait`。stdout / stderr は別スレッドで `read_to_end`。起動前に `cwd.is_dir()` を確かめる）。テストは `capture` を `sh -c 'echo ...; exit N'` / `cmd /S /C "..."` で確認する
- `Runner` が持つ gh の子は `running: HashMap<RunId, _>` には載せない（`RunId` はコマンド用）。取得スレッドと共有するのは `GhHandles { live, current: Arc<Mutex<Option<u32>>>（動いている gh の pid）, cancel: Arc<AtomicBool> }` と `gh_busy: Arc<AtomicBool>`（取得中。結果を送る直前に下ろす）。
  終了時（`stop_all_and_wait` / `Drop`）は `cancel_gh`: `current` のロックの下で `cancel` を立て、pid があれば `force_kill`。取得スレッドは各コマンドの前後で `cancel` を見て、立っていれば残りを起動せず結果も送らない（`fetch_sequence`）。`capture` の起動と pid の登録も同じロックの下で行い、取り消しと行き違わない
  （当初案の「`gh_child` を 1 つ持つ」では 2 本目のコマンドが終了後に起動して残る、と reviewer に指摘されて変更。2026-10-09）

```rust
// src/app.rs に追加
pub enum Action { …, FetchGh, TogglePane }
pub enum Effect { …, FetchGh }                 // cwd は tui が config.root を渡す。App は root を持たない
pub enum RightPane { Output, Status }
pub enum GhPanel { NotFetched, Fetching { since: Instant }, Ready { status: GhStatus, at: Instant }, }
// App のフィールド: pane: RightPane, gh: GhPanel
impl App {
    pub fn on_gh_event(&mut self, event: GhEvent);   // Capture → gh::classify / parse_* → GhPanel::Ready
    pub fn pane(&self) -> RightPane; pub fn gh_panel(&self) -> &GhPanel;
}
```

- 状態遷移: `NotFetched --g--> Fetching --GhEvent--> Ready --g--> Fetching …`。`Fetching` 中の `g` は無視して `set_notice("already fetching")`。`apply(FetchGh)` は `Fetching` に遷移し `pane = Status` にして `[Effect::FetchGh]` を返す。`TogglePane` は `pane` を反転するだけ
- `needs_tick()`: `Fetching` または `Ready`（`fetched Ns ago` を更新するため）なら true を足す。ただし `pane == Output` のときは status の経過は見えないので、`pane == Status` のときだけ
- `age`（CI 実行の経過）は取得時点の `now_unix`（tui が `SystemTime::now()` から作る）で計算して `Duration` にし、以後は固定。ヘッダの `fetched Ns ago` は `Instant`（`set_now`）で動く。`App` は `SystemTime::now()` も `Instant::now()` も呼ばない

### 画面・キーバインド

- 区画: `Panes { list, header, output, help }` は変えない。`draw` が `app.pane()` で右側の本文を `draw_output` / `draw_status` に振り分ける。`layout` は不変（`output.height` は status 区画の高さにも使う）
- status 区画の例（80x24、右側 55 桁）:

```
gh status  fetched 12s ago                              ← ヘッダ行（既存の header の位置）
Pull requests (open)
 #15 feat/15-gh-status  run..  gh 経由で PR と CI の状態を~
 #16 fix/16-foo         ok     Fix foo
Recent CI runs
 main                ok     3m ago   開発用 runs.toml に手動確認用~
 chore/dev-commands  ok     10m ago  開発用 runs.toml に手動確認用~
 ... 3 more
```

- ヘッダ行: `gh status  fetching... 3s` / `gh status  fetched 12s ago` / `gh status  (press g to fetch)`
- 失敗時の本文: 見出しの下に ` error: gh not found in PATH` のように 1 行
- キー: `g` = 取得して status 区画へ、`Tab` = output ↔ status。どちらも修飾なし。既存キーとの衝突なし（`g` / `Tab` は未使用。棚卸し済み）
- HELP（最下行）: 現行は 81 桁でタイトルと重なっている。`Up/Dn move  Enter run  s stop  r reload  g gh  Tab pane  PgUp/Dn scroll  q quit`（79 桁。実装時に当初案の 78 桁が実際は 85 桁あったので詰め直した。`End` は載せない）に詰める。80 桁ではまだタイトル（10 桁）と重なるので、右端のタイトルは幅が足りないときは出さない（`help` の幅 ≥ HELP の幅 + 2 + タイトル幅 のときだけ出す。通知の有無では変えない）。`help_row()` テストヘルパーは `HELP` 定数を参照しているので追随する。タイトル非表示の条件はテストを足す
- `--help`（`cli.rs`）のキー一覧に `g` / `Tab` を足す

### 設定・CLI

- `runs.toml` のキーは増やさない。`gh` の実行ディレクトリは `config.root`（`runs.toml` のあるディレクトリ）。`tui::handle_effects` が `Effect::FetchGh` を受けたとき `runner.fetch_gh("gh", &config.root, gh_tx.clone())` を呼ぶ
- `tui::event_loop` の引数に `gh_rx: Receiver<GhEvent>` が増える（呼び出しは `run` の 1 か所）。受信ループは runner のループの隣に `while let Ok(e) = gh_rx.try_recv() { app.on_gh_event(e); dirty = true; }`

### 依存クレート（追加する場合）

| クレート | 用途 | ライセンス | 代替案・追加しない場合 |
|---|---|---|---|
| `serde_json` 1（`default-features = false`, `features = ["std"]`） | `gh --json` の解釈（`serde` の derive で構造体に読む） | MIT OR Apache-2.0 | 自前の JSON パーサ（壊れやすく不採用）、`gh --jq` で整形してから行単位で読む（JSON 解釈を gh 側に寄せ、タブ区切りの解釈が要る。テストしにくいので不採用）。追加で増える依存は本体のみの見込み（`itoa` / `memchr` / `ryu` は lock に既にある。新しい版は `zmij` を使う可能性があり、その場合 1 つ増える）。`deny.toml` の許可リストで通る見込み（MIT / Apache-2.0）。`cargo add` 後に `cargo deny check` で確認 |

`chrono` / `time` は追加しない（ISO 8601 UTC の 1 形式だけなので自前の換算で足りる。`time` は lock に既にあるが直接依存にはしない）。

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| CLAUDE.md「外部サービスは API を直接呼ばず CLI 経由」 | `gh` のみ。`--json` で受ける |
| CLAUDE.md Architecture「プロセスに触るのは runner だけ」「App は Instant::now() を呼ばない」 | 起動は `Runner::fetch_gh`。`App` は `GhEvent` の `now_unix` と `set_now` の `Instant` だけを使う |
| CLAUDE.md「状態・更新・描画を分け、端末なしでテスト」 | `gh` モジュールは純粋関数。`App` のテストで `Action → Effect` と `GhEvent → GhPanel` を確認。描画は `TestBackend` |
| rust-safety 1 章（unwrap / panic 禁止） | JSON の欠損は `Option` / `serde(default)` で受ける。スレッド内でも `?` と `GhError` で返す |
| rust-safety 2 章（端末復元） | 新しい経路で `process::exit` は使わない。`gh` の子は `LivePids` に入れ、緊急終了の KILL 対象にする |
| rust-safety 3 章（外部入力の表示） | タイトル・ブランチ・stderr は `sanitize`。改行は 1 行目のみ採用 |
| rust-safety 4 章（文字列と座標） | 幅は表示幅で計算し、既存の切り詰め関数を使う。`u16` の減算は `saturating_sub` |
| rust-safety 6 章（外部コマンド） | `Command::new("gh").args(PR_ARGS)`。シェル文字列に連結しない。`cwd` は `PathBuf` |
| rust-safety 8 章（依存） | `serde_json` を最小 feature で追加。`deny.toml` の許可リストで検証 |
| CLAUDE.md「Windows と Unix の差を片側だけで実装しない」 | `capture` の停止は既存の `terminate` / `force_kill`（両 OS 実装済み）を使う。Windows の実機確認はユーザー、Linux は WSL |
| CLAUDE.md「テストは実装と別ファイル」 | `src/gh/tests.rs`（新規）、`src/runner/tests.rs` / `src/app/tests.rs` / `src/ui/tests.rs` に追加 |

## 懸念点（要判断）

**判断結果（2026-10-09、ユーザー）**: C1 は (C) 右側の区画の切り替え、C2 は各 10 件・スクロール無し、C3 は `serde_json` を追加、C4 は #15 に含める。C5 / C6 は確認済み。

### C1. 表示場所は「右側の区画を output ↔ status で切り替える」（判断者: ユーザー）

intent の未解決の問い。候補は (A) 常時表示の区画を足す、(B) 全画面の別ビュー、(C) 右側の区画だけ切り替え。
- (A) は常に見えるが出力欄が狭くなり、80x24 のスナップショットテスト 6 件と `layout` テスト 2 件の期待値が変わる
- (B) は既存画面に影響しないが、status を見ている間はコマンド一覧も出力も見えない（intent 2 と相性が悪い）
- (C) は一覧・ヘッダ・ヘルプの位置が変わらず、既存テストも変わらない。status を見ながらコマンドの選択・実行・停止ができ、`Tab` で出力に戻れる

推奨は (C)。

### C2. 件数上限 10 件・スクロール無し（判断者: ユーザー）

80x24 では status 区画は 22 行。見出し 2 行 + PR 10 + runs 10 = 22 でぎりぎり。収まらない分は `... N more` で省略し、v1 ではスクロールしない。利用者は本人で open な PR は数件のため十分と考える。スクロールが要れば別課題。

### C3. `serde_json` の追加（判断者: ユーザー）

上の表のとおり。`cargo add` は確認が入る。

### C4. HELP 行とタイトルの重なり（判断者: ユーザー。既存の不具合の扱い）

現行の HELP は 81 桁で、80 桁の端末では既に末尾がタイトルに隠れている（棚卸しで判明。#15 以前からの不具合）。本件でキーが 2 つ増えるので、文言を 79 桁に詰め（`End` を省く）、幅が足りないときはタイトルを出さない。この修正を #15 に含める（別課題にすると `g` / `Tab` が HELP に載らない）。

### C5. `gh` のタイムアウト 30 秒と、終了時の扱い（判断者: ユーザー。確認のみ）

ネットワーク待ちで `gh` が返らないとき、30 秒で KILL して `gh timed out (30s)` を出す。`q` で終了するときは既存の全停止（TERM → 2 秒 → KILL）に混ぜるので、最長 3 秒待つ。

### C6. 経過時間に壁時計（`SystemTime`）を使う（判断者: ユーザー。確認のみ）

CI 実行の `createdAt` は UTC の壁時計なので、`Instant` では差が取れない。tui が `SystemTime::now()` を UNIX 秒にして `GhEvent` に載せ、`App` はそれで `age` を計算する（`App` が `SystemTime::now()` を呼ばない形）。端末の時計が狂っていると `age` が負になるので、負なら 0 にする。

## intent の未解決の問い

- 表示場所 → **回答（案）**: C1 の (C)
- 取得に使うキー → **回答**: `g`（取得して status へ）と `Tab`（切り替え）
- 件数上限 → **回答**: 各 10 件、スクロール無し（C2）
- `gh` を runner にどう載せるか → **回答**: `Runner::fetch_gh` + 汎用の `capture`。`RunId` には載せず `gh_child` を 1 つ持つ。通知は別チャネルの `GhEvent`
- `gh` 不在の CI でテストを通す分け方 → **回答**: 解釈・分類・暦換算は `gh` モジュールの純粋関数を固定 JSON で、起動は `capture` を `sh` / `cmd` の偽コマンドで、`App` は `GhEvent` を直接渡して
