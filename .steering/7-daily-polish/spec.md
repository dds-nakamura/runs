# Spec: 日常で使うための仕上げ（intent.md 2026-10-07 より）

## 要件

### 機能要件（受け入れ条件）

時刻と所要時間

- [ ] 一覧の各行に、状態の右に「時間」の列が出る。実行中は開始からの経過（`12s`、`1m 12s`、`1h 2m`）、終了・停止・失敗後は終わってからの経過（`3m ago`、`2h ago`、`1d ago`）。未実行は空
- [ ] 出力欄の 1 行目が見出しになり、選択中のコマンドの名前・状態・所要時間（終了後: `took 1m 12s`、実行中: `running 12s`）が出る。未実行は名前だけ
- [ ] 実行中のコマンドがあるとき、またはいずれかのコマンドが一度でも実行されていれば、表示は 1 秒ごとに更新される。
  一度も実行していなければ、入力か通知が無い限り描画しない（#5 の挙動のまま）
- [ ] 時刻・所要時間は `runs` を終了すると消える（保存しない）

設定の再読み込み

- [ ] `r` で `runs.toml`（起動時に見つけたファイル）を読み直し、一覧が新しい設定の順と内容になる
- [ ] 同じ名前のコマンドは、状態・出力・時刻・実行中のプロセスを引き継ぐ（`command` や `cwd` が変わっていても、実行中のプロセスはそのまま。次の実行から新しい内容）
- [ ] 設定から消えたコマンドが実行中なら停止する。終わっていれば単に消える
- [ ] 選択は同じ名前の行に移る。無ければ先頭
- [ ] `shell` の変更は次の実行から効く
- [ ] 読み直しに失敗したら（ファイルが無い・構文エラー・検証エラー）、一覧も実行中のプロセスも変えず、最下行に `reload failed: <理由の 1 行目>` が出る。次のキー入力で消える
- [ ] 読み直しに成功したら、最下行に `reloaded runs.toml (N commands)` が出る。次のキー入力で消える

停止して再実行

- [ ] 実行中のコマンドに `Enter` を押すと、停止を要求し、終了が届いた時点で出力を消して再実行する。確認は出さない
- [ ] 停止の猶予（2 秒）内に終わらなくても、強制終了の後に `Exited` が届いてから再実行する（並行して起動しない）
- [ ] 停止待ちの間に再度 `Enter` を押しても 1 回しか再実行しない。`s` を押すと再実行は取り消され、停止だけになる
- [ ] 前回の実行の出力が、遅れて届いても新しい実行の出力に混ざらない（実行ごとの ID で区別する）

変えないこと

- [ ] #1・#5 の受け入れ条件（終了・端末復元・停止・非 TTY・`--help`・キー）はそのまま。`r` と「実行中の `Enter`」だけが追加・変更

### 非機能要件

- 待機中に CPU を使わない: 時間表示が無い間はタイマーで描画しない。あるときも描画は 1 秒に 1 回まで
- 時間の計算は `std::time::Instant` の差。壁時計（`SystemTime`）は使わない（絶対時刻を出さないので要らない。時計の巻き戻しの影響も受けない）
- 一覧の幅は引き続き画面幅の 40% まで。時間の列（7 桁）が加わる分、狭い画面では名前が切れる
- 対象 OS・実機確認の範囲は #5 と同じ

## 設計

### 変更方針

#5 の構成を保つ。変更の中心は 3 つ。

1. **実行ごとの ID（`RunId`）を導入し、`Runner` と `RunnerEvent` のキーをコマンドの添字から `RunId` に変える**。
   再読み込みで一覧の順が変わっても、再実行で前回の出力が遅れて届いても、通知の宛先がずれない
2. **`App` が「現在時刻」を外から受け取る**（`set_now(Instant)`）。時刻を内部で取らないので、層 1 のテストで時間を自由に進められる
3. **`Effect` を `on_runner_event` からも返せるようにする**（停止後の再実行は `Exited` を受けた時点で `Start` を返す）

| ファイル | 変更 |
|---|---|
| `src/runner.rs` | `CommandId` → `RunId`（`u64`）。`start(run, spec)` / `stop(run)`。`set_shell(Vec<String>)` を追加。それ以外は変えない |
| `src/app.rs` | `CommandView` に `spec`（名前・コマンド・cwd）、`run: Option<RunId>`、`started_at` / `finished_at`、`restart_pending` を追加。`App` に `now`、`next_run`、`notice` を追加。`Action::Reload`、`Effect::Start { run, spec }` / `Stop(run)` / `Reload`。`replace_config(&Config) -> Vec<Effect>` |
| `src/ui.rs` | 一覧に時間の列、出力欄の見出し行（`Panes.header`。右ペインを `[Length(1), Fill(1)]` で分ける）、最下行の通知。`layout().output` は見出しを除いた矩形 |
| `src/tui.rs` | 毎秒の描画判定、`Effect::Reload` で `config::load_file(path)` → `app.replace_config` → 返った `Effect` を処理、`Effect::Start` に `spec` を使う |
| `src/config.rs` | `Config` に `path: PathBuf`（読んだファイル）を追加。`parse` は `root.join(FILE_NAME)` を入れ、`load_file(&Path) -> Result<Config>` が実際のパスで上書きする |
| `src/output.rs` | 変更なし |
| `src/main.rs` | `tui::run(app, config)` の引数はそのまま（`shell` と `path` の初期値に使う） |
| `src/timefmt.rs`（新規） | `format_elapsed(Duration) -> String`（`12s` / `1m 12s` / `1h 2m` / `3d 4h`）、`format_ago(Duration) -> String`（`5s ago` / `3m ago` / `2h ago` / `1d ago`） |

### 型・状態遷移

```rust
// src/runner.rs
pub type RunId = u64;                                   // 実行ごとに一意。App が採番する
pub enum RunnerEvent { Started { run }, Output { run, bytes }, Exited { run, status }, SpawnFailed { run, message }, StopFailed { run, message } }
impl Runner { pub fn set_shell(&mut self, shell: Vec<String>); /* start / stop / stop_all_and_wait は run をキーに */ }

// src/app.rs
pub struct CommandView {
    spec: CommandSpec,                 // 再読み込みで置き換わる。Effect::Start に渡す
    state: CommandState,
    output: OutputBuffer,
    run: Option<RunId>,                // 現在または直前の実行。これと違う run の通知は無視する
    started_at: Option<Instant>,
    finished_at: Option<Instant>,
    stop_requested: bool,
    restart_pending: bool,             // 停止後に再実行する
}
pub enum Action { Quit, SelectPrev, SelectNext, Run, Stop, PageUp, PageDown, ScrollToEnd, Reload }
pub enum Effect { Start { run: RunId, spec: CommandSpec }, Stop(RunId), Reload }
impl App {
    pub fn set_now(&mut self, now: Instant);                       // tui が各ループで呼ぶ
    pub fn apply(&mut self, action: Action) -> Vec<Effect>;        // Reload は Effect::Reload を返すだけ
    pub fn on_runner_event(&mut self, event: RunnerEvent) -> Vec<Effect>;   // 再実行の Start を返しうる
    pub fn replace_config(&mut self, config: &Config) -> Vec<Effect>;       // 消えた実行中のコマンドの Stop を返す
    pub fn set_notice(&mut self, text: impl Into<String>);         // 最下行の通知（sanitize した 1 行目だけ保持）。apply の先頭で消える
    pub fn notice(&self) -> Option<&str>;
    pub fn needs_tick(&self) -> bool;                              // 時間表示があるか（実行中、または finished_at がある）
}
impl CommandView {
    pub fn elapsed(&self, now: Instant) -> Option<Duration>;       // 実行中: now - started_at
    pub fn ago(&self, now: Instant) -> Option<Duration>;           // 終了後: now - finished_at
    pub fn took(&self) -> Option<Duration>;                        // 終了後: finished_at - started_at
}
```

状態遷移の追加分:

```
Running ──Enter──▶ Running（stop_requested, restart_pending）──Exited──▶ Running（新しい run。出力を消す。Effect::Start）
Running（restart_pending）──s──▶ Running（restart_pending = false。停止のみ）
Running（restart_pending）──Enter──▶ 変化なし
Running（restart_pending）──StopFailed──▶ restart_pending は保持（プロセスは動いたまま）
```

`Exited` を受けたとき `restart_pending` なら: `finished_at` を記録せず、`run` を新しい値にし、出力を消し、`started_at = now`、状態 `Running` のまま、`Effect::Start` を返す。

`replace_config(config)`:

1. 新しい `commands` を、名前で旧 `CommandView` を探して引き継ぐ（`spec` は新しいもので置き換え、それ以外のフィールドはそのまま）。無ければ `Idle` で新規
2. 旧のうち名前が無くなったもので `state == Running` → `Effect::Stop(run)` を集める（`run` が `Some` のもの）
3. `selected` を同じ名前の新しい位置に。無ければ 0。`scroll` は `Follow`
4. 通知は `tui` 側で `set_notice`（成功・失敗の文言は `tui` が作る。`App` はファイルを知らない）

`tui` 側の `Effect::Reload`:

```
match config::load_file(&path) {
    Ok(new) => { let effects = app.replace_config(&new); runner.set_shell(new.shell.clone()); 処理(effects); app.set_notice("reloaded runs.toml (N commands)") }
    Err(err) => app.set_notice(format!("reload failed: {}", 先頭行(sanitize(err))))
}
```

### 毎秒の更新（`tui::run`）

既存のループは `event::poll(50 ms)` で回っている。描画判定に「`app.needs_tick()` かつ前回の描画から 1 秒以上」を加える。
`set_now(Instant::now())` は毎ループ、描画の直前に呼ぶ。待機中（時間表示なし）は従来どおり入力か通知があるときだけ描く。

### 画面

```
  build     idle                 │ test  running 12s
> test      running  12s         │ running 24 tests
  serve     exit 0   3m ago      │ test cli::tests::help_flags ... ok
                                 │ ...
Up/Down select  Enter run/restart  s stop  r reload  PgUp/PgDn/End scroll  q quit        runs 0.1.0
```

- 一覧の列: 印 2 + 名前 + 1 + 状態 8 + 1 + 時間 7。上限は画面幅の 40%（#5 のまま）。80 桁なら名前は 13 桁まで
- 出力欄の 1 行目は見出し: `<名前>  <状態>[ <経過>]` または `<名前>  <状態>  took <所要>`。2 行目から出力。`layout` の `output` は見出しを除いた矩形を返す
- 最下行: 通知があればキーの案内の代わりに通知を出す（タイトルは右端のまま）。無ければキーの案内（`Enter run/restart` と `r reload` を追加）
- 時間の表記（`timefmt`）: 経過は `Ns`（< 60 s）/ `Nm Ns`（< 1 h）/ `Nh Nm`（< 1 d）/ `Nd Nh`。`ago` は最上位の単位だけ + ` ago`（`5s ago` / `3m ago` / `2h ago` / `1d ago`）。幅は最大 7

| キー | 動作 | 変更 |
|---|---|---|
| `Enter` | 未実行・終了後: 実行。実行中: 停止して再実行 | 変更（実行中は無視 → 再実行） |
| `r` | `runs.toml` を読み直す | 追加 |
| 他 | #5 のまま | |

### 設定・CLI

- `runs.toml` の形式は変えない。`--help` の文言に `r` の説明は不要（キーの案内に出る）
- 終了コード・非 TTY の扱いは変えない

### 依存クレート

追加しない（`std::time::Instant` と自前の整形で足りる。`humantime` 等は入れない）

### テスト（`/tui-test` の層）

| 層 | ファイル | ケース |
|---|---|---|
| 1 | `src/timefmt/tests.rs` | `0s` / `59s` / `1m 0s` / `59m 59s` / `1h 0m` / `25h`→`1d 1h`／`ago` の各単位／最大幅 7 |
| 1 | `src/app/tests.rs` | 既存は `RunId` と `Vec<Effect>` に合わせて直す（期待する挙動は変えない）＋`set_now` で時間を進めて `elapsed` / `ago` / `took`／`needs_tick` の真偽（未実行→偽、実行中→真、終了後→真）／実行中の `Run` → `Stop` と `restart_pending`／`Exited` 後に `Start` が返り出力が消え `run` が変わる／停止待ち中の `Run` は 1 回だけ／`s` で取り消し／古い `run` の `Output` は無視／`replace_config` の引き継ぎ・消えた実行中の `Stop`・選択の追従・順序／`notice` が次の Action で消える／`Reload` が `Effect::Reload` を返す |
| 1 | `src/runner/tests.rs` | 既存を `RunId` に合わせる＋`set_shell` が次の `start` から効く |
| 1 | `src/config/tests.rs` | `path` が入る／`load_file` |
| 2 | `src/ui/tests.rs` | 既存の期待値を新しい列・見出し行に合わせて更新（見た目の仕様変更なので期待値を変える）＋時間の列の表示／見出し行の 3 パターン／通知の表示／40% の上限で時間の列が切れずに名前が切れる |
| 3 | `tests/cli.rs` | 変更なし（全件そのまま通ること） |
| 4 | 実機 | Windows Terminal と WSL: `test` を実行して経過が毎秒進み、終了後に `3s ago` と `took` が出る／`runs.toml` を編集して `r` → 一覧が変わり通知が出る／壊して `r` → `reload failed` が出て一覧はそのまま／サーバーを動かしたまま `r` → 動き続ける／サーバーに `Enter` → 停止して再実行（`running` のまま出力が入れ替わる）／設定から消して `r` → 停止して消える／`pty-check.sh` の既存ケース |

### ドキュメント

- CLAUDE.md の Architecture: `timefmt` を追加、`runner` のキーが `RunId` になったこと、`App` が時刻を外から受け取ること
- `/tui-test`: `set_now` で時間を進めるテストの書き方

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| CLAUDE.md Architecture | 状態・更新・描画の分離を保つ。`App` は時刻もファイルも外から受け取る（`set_now`、`replace_config`）。プロセスは `runner` だけ |
| CLAUDE.md Things Claude gets wrong | 計画が変わったら plan / spec の該当節も直す |
| rust-safety 1 章 | 再読み込みの失敗は `anyhow` の文脈付きで通知に出し、落とさない |
| rust-safety 3 章 | 通知に出す理由（toml のエラーに設定ファイルの引用が含まれる）は `sanitize` を通す |
| rust-safety 4 章 | 時間の列は固定幅 7。`Instant` の差は `saturating_duration_since`。`u16` の引き算を書かない |
| rust-safety 7 章 | 通知は画面内。stdout / stderr に出さない |
| rust-safety 8 章 | 依存を追加しない |
| /fix-bug のテストロック | テストは別ファイル |

## 懸念点（1〜2 は 2026-10-07 にユーザーが判断済み）

1. **時間の表示場所** → 決定: 案 A。 一覧に 1 列（実行中は経過、終了後は何分前）+ 出力欄の見出しに所要時間。
   案 B: 一覧に経過・何分前・所要時間の 3 列（幅 +15。80 桁では名前が 5 桁しか残らない）。案 C: 一覧は変えず見出し行だけ（一覧を見ても分からない）
2. **毎秒の更新は「時間表示があるとき」に広げた**。intent は「実行中のコマンドがあるときだけ」としたが、それだと `5s ago` が次の入力まで古いまま残る。
   1 秒に 1 回の描画は `event::poll` の 50 ms の待ち合わせに比べて無視できる。→ 決定: 広げる。intent.md の文言も直した
3. **再実行は `Exited` を待つので、停止に時間がかかるコマンドでは最大 2 秒 + α 遅れる**。その間は `running` のまま（表示上の区別は無い）。
   見出し行に `restarting...` を出す案もあるが、この課題では入れない
4. **再読み込みで `command` / `cwd` が変わった実行中のコマンドは、古い内容のまま動き続ける**。意図どおり（intent の決定）だが、一覧の表示からは区別できない
5. **`RunId` の導入で `runner` と `app` のテストの大半に手が入る**（添字 → `RunId`）。期待する挙動は変えないが、差分は大きい
6. **出力欄が見出し 1 行分狭くなる**。高さ 1 の端末では出力が見えない（極小サイズで panic しないことは引き続きテストする）
7. **impact-analyzer は使っていない**。#5 の 8 モジュールは全部把握している

## intent の未解決の問い

- 時刻・所要時間の表示場所 → 回答済み（案 A。懸念点 1）
- 毎秒の更新の仕組み → `event::poll` のループで前回描画から 1 秒経過を見る。時間表示があるときだけ（懸念点 2）
- 再読み込みのエラーの表示位置 → 最下行（キーの案内の代わり）。次のキーで消える
- 停止が猶予内に終わらないときの再実行 → `Exited` が届くまで待つ。並行して起動しない
