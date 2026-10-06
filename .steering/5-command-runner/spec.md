# Spec: 設定したコマンドを一覧から実行し、出力を見て、停止できるようにする（intent.md 2026-10-06 より）

## 要件

### 機能要件（受け入れ条件）

設定の読み込み

- [ ] カレントディレクトリから親へ向かって `runs.toml` を探し、最初に見つかったものを読む（Cargo が `Cargo.toml` を探すのと同じ）
- [ ] 見つからなければ、TUI を起動せずに stderr へ「探した範囲と、最小の `runs.toml` の例」を出し、終了コード 1 で終わる
- [ ] 構文エラー・必須項目の欠落・名前の重複があれば、TUI を起動せずに stderr へ「ファイル名と行番号を含むメッセージ」を出し、終了コード 1 で終わる
- [ ] コマンドは設定ファイルに書いた順に一覧される

一覧と実行

- [ ] 画面左にコマンド一覧（名前と状態）、右に選択中のコマンドの出力が出る。起動直後は先頭のコマンドが選択されている
- [ ] `↑` / `↓`（`k` / `j`）で選択を移動する。端で止まる（折り返さない）
- [ ] `Enter` で選択中のコマンドを実行する。状態が「実行中」になり、出力が右側に流れ始める
- [ ] 実行中のコマンドに `Enter` を押しても何も起きない
- [ ] 終了すると状態が「終了（コード N）」になる。シグナルで終わった（Unix）・停止された場合は「停止」になる
- [ ] 起動に失敗した（シェルが見つからない・作業ディレクトリが無い）場合は状態が「起動失敗」になり、理由が出力欄に出る
- [ ] 終了したコマンドに `Enter` を押すと再実行され、前回の出力は消える
- [ ] 複数のコマンドを同時に実行できる。右側には選択中のコマンドの出力だけが出る。選択を変えると表示が切り替わる
- [ ] 一覧の各行で、未実行・実行中・終了コード・停止・起動失敗が区別できる

出力

- [ ] stdout と stderr の両方を、届いた順に 1 つの流れとして表示する
- [ ] 出力が画面に収まらないときは末尾（最新）を表示する。`PageUp` / `PageDown` で遡れ、`End` で末尾に戻る。
  遡っている間に新しい出力が来ても表示位置は動かず、末尾に戻すと追従に戻る
- [ ] 保持する出力は 1 コマンドあたり直近 10,000 行。超えた分は古い行から捨てる
- [ ] 出力に含まれる ESC シーケンス（色・カーソル移動・OSC）と制御文字は、表示前に取り除くか `?` に置き換える。画面が崩れない
- [ ] UTF-8 でないバイト列は U+FFFD に置き換えて表示する（落ちない）
- [ ] 1 行が画面幅より長いときは末尾を切る（横スクロールはしない）

停止と終了

- [ ] `s` で選択中の実行中コマンドを停止する。シェル経由で起動した子プロセス（`npm run dev` が起こした node など）も残らない
- [ ] 停止は「穏やかに止める → 猶予（2 秒）→ 強制終了」の順。Windows は最初から強制終了（下記）
- [ ] `q` / Ctrl+C で終了すると、実行中のコマンドをすべて停止してから端末を復元して終了する。終了コードは 0
- [ ] 終了時の停止が猶予内に終わらないコマンドがあっても、強制終了して終わる（無限に待たない）

変えないこと

- [ ] `--version` / `--help` の出力、stdin / stdout が端末でないときの挙動（終了コード 1、端末に触らない）は #1 のまま
- [ ] キーの Press 以外は無視する。`q` は修飾キーなし、Ctrl+C は `c` / `C` + CONTROL（#1 のまま）

### 非機能要件

- 待機中に CPU を使わない: イベントの待ち合わせは `event::poll` の 50 ms 刻み。描画は入力・出力・終了のいずれかが届いたときだけ
- 大量の出力（1 秒に数千行）でも操作が固まらない: 1 回のループで溜まった出力をまとめて取り込み、描画は 1 回
- 端末サイズ: 幅・高さが 0 や 1 でも panic しない。一覧と出力の領域は `Layout` に任せる
- 色は使わない（状態は文字で表す）
- 一次対象は Windows 11（Windows Terminal）/ Linux / macOS。実機確認は Windows Terminal と WSL の Ubuntu 24.04。macOS は未検証

## 設計

### 変更方針

#1 の構成を保ち、モジュールを 2 つ足す。端末に触るのは引き続き `tui` と `main` だけ。
子プロセスに触るのは `runner` だけ。`app` は入力とランナーからの通知を受けて状態を更新するだけで、端末にもプロセスにも触らない。

| ファイル | 役割 | 端末 / プロセスへの依存 |
|---|---|---|
| `src/main.rs`（変更） | 引数の解釈 → 設定の読み込み → `tui::run`。設定エラーは stderr へ出して終了コード 1 | 端末 |
| `src/cli.rs`（変更） | `--help` の文言に設定ファイルの説明を足す | なし |
| `src/config.rs`（新規） | `runs.toml` の探索・読み込み・検証。`Config` / `CommandSpec` | ファイル |
| `src/app.rs`（変更） | 状態 `App`（コマンドごとの状態・出力・選択・スクロール）、`Action`、`action_for`、ランナーからの通知の反映 | なし |
| `src/output.rs`（新規） | 出力行の保持（上限付き）と、制御文字・ESC シーケンスの除去 | なし |
| `src/runner.rs`（新規） | 子プロセスの起動・出力の読み取りスレッド・終了待ち・停止。通知はチャネルで `tui` へ | プロセス |
| `src/ui.rs`（変更） | 一覧と出力の 2 ペインを描く | なし |
| `src/tui.rs`（変更） | イベントループを「`event::poll` + チャネルの `try_recv`」に変え、`Action::Run` / `Stop` を `runner` に渡す。終了時に全停止 | 端末 |

### 設定ファイル

ファイル名は `runs.toml`。カレントディレクトリから親へ向かって探す。見つかったファイルのあるディレクトリを「プロジェクトルート」と呼ぶ。

```toml
# 省略可。コマンドを渡すシェル。既定は Unix が ["sh", "-c"]、Windows が ["cmd", "/S", "/C"]
shell = ["pwsh", "-NoProfile", "-Command"]

[[command]]
name = "test"                 # 必須。一覧に出す名前。重複不可
command = "cargo test"        # 必須。shell の最後の引数として渡す（パイプやリダイレクトが使える）
cwd = "."                     # 省略可。プロジェクトルートからの相対パス（絶対パスも可）。既定は "."

[[command]]
name = "dev server"
command = "npm run dev"
cwd = "web"
```

- `command` は利用者自身が書く文字列で、シェルの最後の引数としてそのまま渡す。利用者の入力を文字列に連結することはない
- Windows でシェルが `cmd`（`cmd.exe`・フルパスも含む）のときは、コマンド全体を `"` で包んで `CommandExt::raw_arg` で渡す。
  理由は 2 つ: std は MSVC の規則で `"` を `\"` にエスケープするが `cmd` はそれを解釈しない（通常の `arg` では `echo "a b"` が `\"a b\"` になる）。
  `cmd /C` は引用符が 3 つ以上あると先頭と末尾の `"` を外すので、`"C:\Program Files\x.exe" "a b"` のようなコマンドが壊れる。
  既定の `/S` は「先頭と末尾の `"` だけを外す」指定で、包んだ `"` がそこで外れて中身がそのまま届く。
  `pwsh` など他のシェルは `CommandLineToArgvW` の規則で読むので通常の `arg` でよい
  （`rust-safety` 6 章が禁じるのは「入力の連結」。設定ファイルは利用者が自分の環境のために書く信頼できる入力とする。懸念点 1）
- `shell` は配列で、`Command::new(shell[0]).args(&shell[1..]).arg(command)` として使う。空配列はエラー
- 環境変数の指定（`env`）はこの課題では入れない。必要になったら追加する
- 構文エラーは `toml` クレートのエラー（行・列を含む）をそのまま出す。検証エラー（重複・空の `shell`・空の `name`）は自前のメッセージ

### 型・状態遷移

```rust
// src/config.rs
pub struct Config { pub root: PathBuf, pub shell: Vec<String>, pub commands: Vec<CommandSpec> }
pub struct CommandSpec { pub name: String, pub command: String, pub cwd: PathBuf }   // cwd は root で解決済みの絶対パス
pub fn load(start_dir: &Path) -> anyhow::Result<Config>;     // 探索 → 読み込み → 検証。見つからなければエラー（例を含む）
pub fn parse(text: &str, root: &Path) -> anyhow::Result<Config>;   // テストはこちらを使う
pub const FILE_NAME: &str = "runs.toml";

// src/output.rs
pub struct OutputBuffer { lines: VecDeque<String>, dropped: usize }  // 上限 10,000 行
impl OutputBuffer { pub fn push_raw(&mut self, bytes: &[u8]); pub fn lines(&self) -> impl Iterator<Item = &str>; pub fn len(&self) -> usize; pub fn clear(&mut self); }
pub fn sanitize(text: &str) -> String;   // ESC シーケンス（CSI / OSC / 単独 ESC + 1 文字）を除去し、残る制御文字を '?' に置き換える。タブは空白 4 つ

// src/runner.rs
pub type CommandId = usize;                      // Config.commands の添字
pub enum RunnerEvent {
    Started { id: CommandId },
    Output  { id: CommandId, bytes: Vec<u8> },   // 行単位。stdout と stderr を区別しない
    Exited  { id: CommandId, status: ExitStatus },
    SpawnFailed { id: CommandId, message: String },
}
pub struct Runner { tx: Sender<RunnerEvent>, running: HashMap<CommandId, RunningProcess>, shell: Vec<String> }
impl Runner {
    pub fn new(shell: Vec<String>) -> (Self, Receiver<RunnerEvent>);
    pub fn start(&mut self, id: CommandId, spec: &CommandSpec);   // 起動失敗は SpawnFailed として通知（Err を返さない）
    pub fn stop(&mut self, id: CommandId);                       // 穏やかな停止を開始する。実際の終了は Exited で届く
    pub fn stop_all_and_wait(&mut self, grace: Duration);        // 終了時用。全部に stop → 猶予 → 強制終了 → wait
    pub fn is_running(&self, id: CommandId) -> bool;
}

// src/app.rs
pub enum CommandState { Idle, Running, Exited { code: Option<i32> }, Stopped, SpawnFailed }
pub struct CommandView { pub name: String, pub state: CommandState, pub output: OutputBuffer }
pub struct App { title: String, commands: Vec<CommandView>, selected: usize, scroll: Scroll, should_quit: bool }
pub enum Scroll { Follow, At(usize) }           // At(n) は先頭からの行オフセット
pub enum Action { Quit, SelectPrev, SelectNext, Run, Stop, PageUp, PageDown, ScrollToEnd }
pub enum Effect { Start(CommandId), Stop(CommandId) }     // app が tui に頼むこと。app 自身はプロセスに触らない
impl App {
    pub fn new(title: impl Into<String>, config: &Config) -> Self;
    pub fn apply(&mut self, action: Action) -> Option<Effect>;
    pub fn on_runner_event(&mut self, event: RunnerEvent);
    pub fn selected(&self) -> usize; pub fn commands(&self) -> &[CommandView]; pub fn scroll(&self) -> Scroll; ...
}
```

状態遷移（コマンド 1 件）:

```
Idle ──Run──▶ Running ──Exited(code)──▶ Exited{code}
                 │  ──Exited(signal)/Stop 後──▶ Stopped
                 └──SpawnFailed──▶ SpawnFailed
Exited / Stopped / SpawnFailed ──Run──▶ Running（出力を消して再実行）
Running ──Run──▶ 変化なし
```

- `apply(Run)`: 選択中が `Running` なら `None`。それ以外なら出力を消し、状態を `Running` にして `Some(Effect::Start(id))`
  （楽観的に `Running` にする。失敗すれば `SpawnFailed` が届いて戻る）
- `apply(Stop)`: 選択中が `Running` なら `Some(Effect::Stop(id))`。状態は `Exited` / `Stopped` の通知で変える（停止要求中も `Running` のまま）
- `on_runner_event(Exited)`: `status.code()` が `Some(n)` なら `Exited{code: Some(n)}`。`None`（シグナル）なら `Stopped`。
  自分が停止を要求していた場合は、コードの有無によらず `Stopped`（`stop_requested` フラグを `CommandView` に持つ）
- `apply(PageUp/PageDown)`: `Follow` から遡ると `At(表示中の先頭行 − ページ分)`。`ScrollToEnd` で `Follow` に戻る。
  ページの高さは描画側しか知らないので、`ui::layout(area) -> Panes`（純粋関数。`draw` も同じものを使う）で求めた出力欄の高さを、
  `tui` が各ループで `App::set_output_height` に渡す（`draw(&App)` は不変のまま）

### イベントループ（`tui::run`）

```
Runner::new(config.shell) → (runner, rx)
loop {
    draw
    if event::poll(50ms)? { 入力を読み、action_for → apply → Effect があれば runner.start / stop }
    while let Ok(ev) = rx.try_recv() { app.on_runner_event(ev) }   // 溜まった分をまとめて取り込む
    if app.should_quit() { break }
}
runner.stop_all_and_wait(2 秒)   // ガードの drop（端末の復元）より前。TUI 実行中なので stdout には出さない
```

- 描画は「入力かランナーの通知があったとき」だけにする（`poll` がタイムアウトで戻り、`try_recv` も空なら描かない）
- 入力は `event::poll` → `event::read` で主スレッドから読む。入力用のスレッドは作らない（#1 の構造を保つ）

### 子プロセスの起動・出力・停止（`runner`）

起動:

- `Command::new(&shell[0]).args(&shell[1..]).arg(&spec.command).current_dir(&spec.cwd)`
- `stdin(Stdio::null())`、`stdout(Stdio::piped())`、`stderr(Stdio::piped())`
- Unix: `CommandExt::process_group(0)` で新しいプロセスグループにする（停止でグループごと止めるため）
- Windows: `CommandExt::creation_flags(CREATE_NO_WINDOW)` は使わない（コンソールアプリなので新しいウィンドウは開かない）
- `spawn()` の `Err` は `SpawnFailed { message }` として送る
- stdout / stderr それぞれに読み取りスレッドを立て、`BufRead::read_until(b'\n')` で行ごとに `Output { bytes }` を送る（末尾の改行は除く）。
  EOF で終わる。送信に失敗したら（受信側が終わっている）スレッドを終える
- 終了待ちのスレッドを 1 つ立て、`child.wait()` の結果を `Exited { status }` で送る。`Child` はこのスレッドが所有する。
  停止のために `Runner` 側では `pid` と（Unix では）プロセスグループ ID を持つ

停止（`rust-safety` 5 章の `unsafe` 禁止のため、`libc` の直接呼び出しは使わない。懸念点 2）:

- Unix: `kill -s TERM -- -<pgid>` を `Command` で実行（引数は分けて渡す）。2 秒後に、シェルが終わっていても `kill -s KILL -- -<pgid>` を送る
  （シェルだけが TERM で終わり、TERM を無視する孫が残ることがある。グループの誰かが生きている間、その pgid は再利用されない。全員が先に終わっていた場合は、2 秒の間に同じ pid が別のグループの先頭に再利用される確率がごく低いながら残る）
- Windows: `taskkill /T /F /PID <pid>` を `Command` で実行（プロセスツリーごと強制終了。穏やかな停止は無い）
- `kill` / `taskkill` の stdout / stderr は `Stdio::null()` で捨てる（TUI 実行中に端末へ流れると画面が崩れる）
- `kill` / `taskkill` が失敗したら（実行ファイルが無い、非ゼロ終了）、最終手段として `Child::kill`（直接の子だけ）を呼び、
  `RunnerEvent::StopFailed` で理由を出力欄に出す。状態は `Running` のまま
- 終了待ちはブロックする `wait` でなく `try_wait` のポーリング（50 ms）にして、`Runner` 側が `Child::kill` を使えるようにする
- `stop_all_and_wait(grace)`: 全部に停止を送り、`grace` の間終了を待ち、残ったものに強制終了（Unix はグループへ無条件に KILL）と `Child::kill` を送り、
  さらに 1 秒待つ。それでも終わらないものは諦めて戻る（端末の復元を待たせない。無限に待たない）

出力の取り込み（`app` 側）: `Output { bytes }` を `String::from_utf8_lossy` → `output::sanitize` → `OutputBuffer::push`

### 画面・キーバインド

```
┌ runs 0.1.0 ──────────────────────────────────────────────────────────────┐
│ > test        running   │ running 24 tests                                │
│   dev server  exit 0    │ test cli::tests::help_flags ... ok              │
│   verify      idle      │ ...                                             │
│                         │                                                 │
│ ↑↓ select  Enter run  s stop  PgUp/PgDn scroll  q quit                    │
└───────────────────────────────────────────────────────────────────────────┘
```

- 左ペインの幅は 名前の最大幅 + 状態の幅、ただし画面幅の 40% まで（`u32` で計算する。`u16` の掛け算はあふれる）。右ペインが残り。最下行にキーの案内。枠線なし
- 状態の表記: `idle` / `running` / `exit N` / `stopped` / `failed`
- 出力欄: `Scroll::Follow` なら末尾の `height` 行、`At(n)` なら n 行目から `height` 行。長い行は末尾を切る（`Paragraph` の既定）

| キー | 動作 | 備考 |
|---|---|---|
| `↑` / `k`、`↓` / `j` | 選択の移動 | 端で止まる |
| `Enter` | 選択中を実行（再実行） | 実行中なら無視 |
| `s` | 選択中を停止 | 実行中でなければ無視 |
| `PageUp` / `PageDown` | 出力を遡る / 進める | 末尾まで進めると追従に戻る |
| `End` | 末尾に戻る（追従） | |
| `q`、Ctrl+C | 全停止して終了 | #1 のまま |

### 設定・CLI

- 引数は変えない。`--help` の末尾に「`runs.toml` をカレントディレクトリから親へ探す」旨と最小の例を足す
- 終了コード: 0 = 正常、1 = 実行時エラー（設定が無い・壊れている・端末でない・初期化失敗）、2 = 引数の誤り

### 依存クレート（追加）

| クレート | 用途 | ライセンス | 代替案・追加しない場合 |
|---|---|---|---|
| `serde` 1.0（feature: `derive`） | `runs.toml` を構造体に読む | MIT / Apache-2.0 | 手書きの TOML パーサは現実的でない |
| `toml` 1.1（`default-features = false`、feature: `std`, `parse`, `serde`） | TOML の解析。MSRV 1.85 | MIT / Apache-2.0 | `toml_edit` は編集向けで過剰。JSON は intent で不採用 |

追加しないもの: `libc` / `nix`（`unsafe` 禁止。停止は `kill` / `taskkill` で行う）、`strip-ansi-escapes`（除去は 40 行程度で書ける。将来、色を表示するなら `ansi-to-tui` を検討）、`encoding_rs`（懸念点 4）

### テスト（`/tui-test` の層）

| 層 | ファイル | ケース |
|---|---|---|
| 1 | `src/config/tests.rs` | 最小の設定／`shell` と `cwd` の省略と指定／`cwd` の相対・絶対／名前の重複→エラー／必須項目の欠落→エラー（行番号を含む）／空の `shell`→エラー／順序の保持／親ディレクトリの探索（一時ディレクトリ） |
| 1 | `src/output/tests.rs` | CSI（色・カーソル移動）の除去／OSC（タイトル変更、BEL と ST の両方の終端）の除去／単独 ESC／C0・C1 制御文字→`?`／タブ→空白／不正 UTF-8→U+FFFD／上限 10,000 行で古い行から捨てる／`clear` |
| 1 | `src/app/tests.rs` | 選択の移動と端／`Run` → `Running` + `Effect::Start`／実行中の `Run` は無効／`Stop` は実行中だけ `Effect::Stop`／`Exited(code)`→`Exited`／停止要求後の `Exited`→`Stopped`／`SpawnFailed`／再実行で出力が消える／出力は選択中のものだけ／`PageUp` で `At`、`End` で `Follow`、遡り中は追従しない／`q` と Ctrl+C は #1 のまま |
| 1 | `src/runner/tests.rs` | 実プロセスで確かめる。OS 既定のシェル（`sh -c` / `cmd /S /C`）と、両方にある `echo` / `exit` を使う（`CARGO_BIN_EXE_runs` は結合テスト専用で単体テストでは使えない）。終わらないコマンドだけは `sleep` / `ping` を `shell` に直接渡す: 起動→出力→`Exited(0)`／終了コード 2 の伝播／存在しないシェル→`SpawnFailed`／存在しない `cwd`→`SpawnFailed`／長く動くコマンド（Unix: `sleep 30`、Windows: `ping -n 30 127.0.0.1`）を `stop` → `Exited` が届く／`stop_all_and_wait` で全部止まる。各ケースに受信のタイムアウト（10 秒）を付ける |
| 2 | `src/ui/tests.rs` | 固定の `App` 状態（3 コマンド、各状態）を 80x24 で描いて比較／出力が多いときの末尾表示／`At(n)` の表示／1x1・0x0 で panic しない／長い名前で左ペインが 40% に収まる |
| 3 | `tests/cli.rs` | `runs.toml` が無いディレクトリで起動（非 TTY）→ 終了コード 1 と「runs.toml」を含むメッセージ／壊れた `runs.toml` → 終了コード 1 と行番号／`--help` に `runs.toml` が出る／既存 5 ケースは変えない |
| 4 | 実機 | Windows Terminal と WSL: リポジトリ直下の `runs.toml`（`verify` / `test` / `clippy` を登録）で起動→`test` を実行→出力が流れる→`PageUp` で遡る→`End`→終わらないコマンド（`sleep 300` / `ping -t`）を実行したまま別のコマンドを実行→`s` で停止→子プロセスが残っていない（`ps` / `tasklist`）→`q` で全停止して終了→端末が戻る。`pty-check.sh` は既存ケースをそのまま使う（リポジトリ直下に `runs.toml` があるので起動できる） |

リポジトリ直下に `runs.toml` を追加してコミットする（このプロジェクト自身の `verify` / `test` / `clippy` を登録。ドッグフーディングと層 3・4 の前提）。

### ドキュメント

- README は無いので作らない（配布は未定）。`--help` と CLAUDE.md の Architecture に `runs.toml` の説明を書く
- CLAUDE.md の Architecture にモジュール表を更新。`/tui-test` に `runner` のテスト（実プロセス・タイムアウト）の書き方を足す

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| CLAUDE.md Architecture | 状態（`app`）・更新・描画（`ui`）を分ける。プロセスは `runner`、端末は `tui` に閉じ込める。`app` は `Effect` を返すだけでプロセスに触らない |
| CLAUDE.md Conventions | 非テストコードで `unwrap` / `expect` / `panic!` を使わない。依存は本 spec の表で合意 |
| rust-safety 1 章 | 設定エラーは `anyhow` の文脈付きで `main` が stderr へ。ランナーの失敗は `SpawnFailed` として状態に反映し、落とさない |
| rust-safety 2 章 | 終了時の全停止はガードの drop より前に行い、2 秒 + 強制終了で必ず終わる。端末の復元は #1 のまま |
| rust-safety 3 章 | コマンドの出力は信頼できない入力として `sanitize` を通す。設定ファイルの名前・コマンド文字列も表示前に制御文字を除く |
| rust-safety 4 章 | ペイン幅は `Layout` の `Percentage` / `Min` / `Length` で決め、`u16` の引き算を書かない。行は `Paragraph` に切らせる |
| rust-safety 5 章 | `unsafe` を使わない。そのため `libc` / Job Object を使わず `kill` / `taskkill` コマンドで停止する |
| rust-safety 6 章 | 外部コマンドは引数を分けて渡す。利用者の入力をシェル文字列に連結しない（`command` は利用者が書いた文字列そのもの）。パスは `Path` で扱い、`cwd` は `root.join()` で解決 |
| rust-safety 7 章 | TUI 実行中は stdout / stderr に出さない。子プロセスの出力はパイプで受ける（端末に直接流させない） |
| rust-safety 8 章 | `toml` は `default-features = false`。`serde` は `derive` のみ |
| /fix-bug のテストロック | テストは `src/<モジュール>/tests.rs` と `tests/` に置く |

## 懸念点（1〜2 は 2026-10-06 にユーザーが判断済み）

1. **コマンドはシェル経由で実行する** → 決定: シェル経由。Windows の既定は `cmd /S /C`。パイプ・リダイレクト・環境変数の展開が使え、利用者が普段打つ文字列をそのまま書ける。
   設定ファイルは利用者自身が書くので、シェルに渡すこと自体は問題にしない。代替は `args = ["cargo", "test"]` の配列（安全だがパイプが書けない）。
   Windows の既定を `cmd /S /C` にしたが、普段 PowerShell なら `shell = ["pwsh", "-NoProfile", "-Command"]` を設定で指定する（既定を PowerShell にする案もある）
2. **停止に外部コマンド（`kill` / `taskkill`）を使う** → 決定: 外部コマンド。`unsafe` 禁止のため `libc::killpg` や Windows の Job Object を使わない。
   `kill` は POSIX 標準、`taskkill` は Windows 標準なので、無い環境は想定しない（`Command::new("kill")` はシェルの組み込みではなく実行ファイルを探すので、
   procps の無いコンテナでは失敗しうる）。失敗したら `Child::kill` で直接の子だけ止め、`StopFailed` で出力欄に理由を出す。
   安全なラッパー（`nix` クレート）を入れる案もあるが、依存が大きい
3. **Windows の停止は最初から強制終了**。Windows には SIGTERM に相当する穏やかな停止の仕組みが無い（Ctrl+C イベントの送信は同じコンソールのプロセスにしか効かない）。
   停止されたコマンドが後始末（一時ファイルの削除など）をできない。利用者が自分で使う範囲では許容する
4. **Windows のコンソールプログラムの出力の文字コード**。`cmd` の内蔵コマンド（`dir` など）はコードページ（日本語環境では CP932）で出力するので、
   UTF-8 として読むと文字化けする。Rust・Go・Node・Python の出力は UTF-8 なので、この課題では UTF-8 固定とし、CP932 は文字化けを許容する。
   必要になれば `encoding_rs` を検討する
5. **出力は行単位で受けるので、改行の無い進捗表示（`\r` で上書きするプログレスバー）は 1 行に溜まる**。`\r` は `sanitize` で `?` にせず、
   「行末の `\r` を 1 つ落としてから、最後の `\r` 以降だけを残す」扱いにする（CRLF の行は空にならず、プログレスバーは最終状態だけ出る）。完全な再現はしない
6. **子プロセスが stdin を読もうとすると即座に EOF になる**（`Stdio::null()`）。対話的なコマンドは動かない。この道具の用途（ビルド・テスト・サーバー）では問題にしない
7. **`stop_all_and_wait` の猶予 2 秒 + 強制終了で、終了が最大 2 秒ほど遅れる**。実行中のコマンドが無ければ即座に終わる
8. **`toml` 1.1 の MSRV は 1.85**、`serde` は 1.56。プロジェクトの MSRV 1.88 と矛盾しない。ただし 1.88 のツールチェーンでの実ビルドは引き続き未検証
9. **impact-analyzer は使っていない**。既存コードが #1 で書いた 5 モジュールで、全部読んで設計した
10. **パフォーマンスの上限は決めていない**。1 秒に数万行の出力（`cargo build -vv` など）でも固まらないことは実機で確かめるが、数値の目標は置かない

## intent の未解決の問い

- 設定ファイルの名前と項目 → `runs.toml`。`[[command]]` の `name` / `command` / `cwd`、トップレベルの `shell`（本 spec「設定ファイル」）
- シェル経由か引数分離か → 回答済み（シェル経由。懸念点 1）
- 停止の強さ → Unix は TERM → 2 秒 → KILL（プロセスグループ）、Windows は `taskkill /T /F`（懸念点 2・3）
- 出力の保持量とスクロール → 10,000 行、`PageUp` / `PageDown` / `End`
- 設定の共有 → 回答済み（自分専用。OS ごとの切り替えは入れない）
