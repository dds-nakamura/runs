# Plan: 日常で使うための仕上げ（spec.md 2026-10-07 より）

## Context

#5 でコマンドの実行・出力・停止ができるようになった `runs` を、毎日使える状態にする（issue #7）。
一覧に「いつ動かしてどれくらいかかったか」を出し、`r` で `runs.toml` を読み直せるようにし、実行中の `Enter` で停止して再実行できるようにする。
土台の変更は 3 点: 実行ごとの `RunId`（添字をやめる）、`App` が現在時刻を外から受け取る、`on_runner_event` も `Effect` を返す。

- ブランチ: `feat/7-daily-polish`。2026-10-07 にユーザーが承認
- 依存クレートは追加しない
- 既存コードは #5 までに書いた 8 モジュール。Explore / Plan エージェントは使っていない（全部把握している）

## spec から詰めた点（同じコミットで spec.md も直す）

1. **`Config.path` の決め方**: `parse(text, root)` の署名は変えず、`parse` が `path = root.join(FILE_NAME)` を入れる。`load_file(path)` は実際に読んだパスで上書きする。
   テストで `Config` を直接組み立てている箇所（`app` / `ui` のテスト）は `path` を足す
2. **`Panes` に `header` を追加**: 右ペインを `[Length(1), Fill(1)]` で見出しと出力に分ける。`layout().output` は出力だけの矩形（`App::set_output_height` に渡す値はこれ）
3. **通知は `apply` の先頭で消す**: どの `Action` でも、処理の前に `notice = None`。`set_notice` は `output::sanitize` を通し、1 行目だけを保持する
4. **`stop_all_and_wait` は変えない**: `RunId` をキーにするだけ。再実行中（`restart_pending`）に `q` を押した場合は、停止だけ行われて再実行は起きない（終了するので当然）

## 実装中に分かったこと（計画との差分。該当する節も直してある）

- **`App::new` に `now: Instant` を足した**（初期値も外から受け取る。`main` と `tui` だけが `Instant::now()` を呼ぶ）
- **`set_now` は描画の直前だけでは足りない**: 疑似端末で `sleep 3` の `took` が `2s` になった。開始・終了時刻の記録に、`event::poll` で待った分だけ古い時刻を使っていたため。
  入力の処理前と通知の取り込み前にも `set_now(Instant::now())` を呼ぶようにした
- **`CommandView::spec` / `run` / `restart_pending` のゲッターは `#[cfg(test)]`**（描画では使わない）
- **通知の文言は単数形に対応**（`1 command` / `2 commands`）
- **疑似端末の確認スクリプト**（scratchpad の `pty-polish.sh`。リポジトリには入れていない）で、残存プロセスは `pgrep -f "^sleep 300$"` で数える。
  `pgrep -x sleep` だとスクリプト自身の `sleep 1` を数えてしまう

- **verifier の指摘**: 証明に書いたテスト名 4 件が実装時に統合されて存在しなかった（`parse_sets_path_under_root` → `parses_minimal_config` に含む、
  `shows_time_column` と `header_shows_elapsed_while_running` → `renders_list_and_output_at_80x24` に含む、`output_height_excludes_header` → `layout_reserves_rows_for_header_and_help`）。
  証明の節を実態に直した。層 4 の「`r` 直後の残存」「pid の変化」も疑似端末では見ていなかったので、証明の節を直した

- **Windows Terminal での実機確認**: 2026-10-07 にユーザーが手動で 7 項目を確認し、すべて期待どおりだった
  （経過と `took`／実行中のまま `r` で再読み込みと通知／壊れた設定で `reload failed`／`Enter` で停止して再実行、`tasklist` の `ping` が 1 行／
  設定から消して `r` で停止、`tasklist` に残らない／待機中の CPU が 0% 近く／`q` で終了）。
  途中で 2 点判明: `timeout /t 3` は stdin が無いと即終了する（#5 の懸念点 6 のとおり。確認手順を `ping -n 4` に変更）、
  `ping` の日本語出力が CP932 で化ける（#5 の懸念点 4 で許容したが、日常のコマンドで起きるので #8 として起票）

## 変更するファイル

新規

- `src/timefmt.rs` + `src/timefmt/tests.rs`: `format_elapsed(Duration) -> String`（`0s` / `59s` / `1m 0s` / `59m 59s` / `1h 0m` / `1d 1h`）、
  `format_ago(Duration) -> String`（`5s ago` / `3m ago` / `2h ago` / `1d ago`）。整数の割り算だけ。どちらも最大 7 桁（`format_elapsed` は `59m 59s` の 7 桁、`format_ago` は `59m ago` の 7 桁。日は `999d 23h` のように超えうるが、そこまで動かしていれば一覧が切れても構わない。`WIDTH: usize = 7` を公開）

変更

- `src/runner.rs` + `src/runner/tests.rs`: `pub type CommandId` → `pub type RunId = u64`。`RunnerEvent` のフィールド名 `id` → `run`。`start(run, spec)` / `stop(run)`。
  `set_shell(&mut self, shell: Vec<String>)` を追加（次の `start` から効く）。`#[cfg(test)] is_running(run)` はそのまま。テストは名前の置き換え＋`set_shell_applies_to_next_start`
- `src/config.rs` + `src/config/tests.rs`: `Config.path: PathBuf` を追加。`load(start_dir)` は探索だけにして `load_file(&Path)`（読み込み + `parse` + `path` の上書き）を切り出す。
  テスト `parse_sets_path_under_root`、`load_file_reads_the_given_file`、`load_searches_parent_directories` で `path` も確認
- `src/app.rs` + `src/app/tests.rs`:
  - `CommandView`: `name: String` を `spec: CommandSpec`（`name` は `spec.name` を `sanitize` した表示用の `display_name` として別に持つ）に。`run: Option<RunId>`、`started_at` / `finished_at: Option<Instant>`、`restart_pending: bool` を追加。
    `elapsed(now)` / `ago(now)` / `took()` / `spec()` / `run()` / `restart_pending()` を公開
  - `App`: `now: Instant`、`next_run: RunId`（1 から）、`notice: Option<String>` を追加。`set_now`、`set_notice`、`notice`、`needs_tick` を追加
  - `Action::Reload`（`r`）、`Effect` を `Start { run, spec }` / `Stop(run)` / `Reload` に。`apply` と `on_runner_event` は `Vec<Effect>` を返す
  - `apply(Run)`: `Idle` / 終了後 → 新しい `run` を採番し、`started_at = now`、出力を消し、`Running`、`Start`。`Running` かつ `!restart_pending` → `stop_requested = restart_pending = true`、`Stop(run)`。`Running` かつ `restart_pending` → 何もしない
  - `apply(Stop)`: `restart_pending = false` にしてから従来どおり
  - `on_runner_event`: `run` が `view.run` と一致するコマンドだけを対象にする（一致しなければ無視）。`Exited` で `restart_pending` なら新しい `run` で再開して `Start` を返す。
    それ以外の `Exited` / `SpawnFailed` は `finished_at = now`。`StopFailed` は従来どおり（`restart_pending` も保持）
  - `replace_config(&Config) -> Vec<Effect>`: 名前で引き継ぎ、消えた `Running` に `Stop(run)`、選択の追従、`scroll = Follow`
  - 既存テストは `Effect::Start(0)` → `Effect::Start { run: 1, .. }` のように `RunId` と `Vec` に合わせて直す（期待する挙動は変えない）
- `src/ui.rs` + `src/ui/tests.rs`: 一覧の行を `印 2 + 名前 + 1 + 状態 8 + 1 + 時間 7`（時間は `timefmt`、未実行は空白 7）。`Panes.header` に `<名前>  <状態>[ <経過>]` / `<名前>  <状態>  took <所要>` / `<名前>`。
  最下行は `app.notice()` があればそれ、無ければキーの案内 `Up/Down select  Enter run/restart  s stop  r reload  PgUp/PgDn/End scroll  q quit`（タイトルは右端のまま）。
  既存テストの期待値は新しい列・見出し行に合わせて更新（見た目の仕様変更）。`render` ヘルパーは `app.set_now(固定の Instant)` を呼ぶ
- `src/tui.rs`: `run(app, config)` で `path = config.path.clone()`、`Runner::new(config.shell.clone())`。ループ: `app.set_now(Instant::now())` → 描画判定に `app.needs_tick() && last_draw.elapsed() >= 1 s` を追加 →
  `apply` / `on_runner_event` の `Vec<Effect>` を `handle_effects` で処理（`Start { run, spec }` → `runner.start(run, &spec)`、`Stop(run)` → `runner.stop(run)`、
  `Reload` → `config::load_file(&path)` の成否で `replace_config` + `set_shell` + 通知、または失敗の通知）
- `src/main.rs`: 変更なし（`tui::run(&mut app, &config)` のまま）
- `CLAUDE.md`: Architecture に `timefmt`、`RunId`、`set_now`、`r` のキー。`.claude/skills/tui-test/SKILL.md`: `set_now` で時間を進めるテスト、`Vec<Effect>` の assert の書き方
- `.steering/7-daily-polish/plan.md`（新規）、`spec.md`（上の「詰めた点」を反映）

`Cargo.toml` / `Cargo.lock` / `runs.toml` / `tests/cli.rs` は変更しない。

## 作業順

`RunId` の導入でコンパイルが通らない期間が長くならないよう、下から順に差し替え、各段階で `cargo test <モジュール>::` を通す。`verify.sh` は全体がつながった 7 の後。

1. plan.md を保存、spec.md の 4 点を直してコミット
2. `timefmt`: テスト → 実装（`main.rs` に `mod timefmt;`）
3. `config`: `path` と `load_file`。テスト → 実装。`app` / `ui` のテストの `Config` リテラルに `path` を足してコンパイルを通す
4. `runner`: `RunId` と `set_shell`。テストの置き換え → 実装（Windows と WSL の両方で `cargo test runner::`）
5. `app`: 新しい型・`Vec<Effect>`・時刻・再実行・再読み込み・通知。既存テストの修正 → 新しいテスト → 実装
6. `ui`: 時間の列・見出し・通知。テストの期待値の更新 → 新しいテスト → 実装
7. `tui`: ループと `handle_effects`。`cargo build` と `tests/cli.rs` が通ること
8. `bash .claude/scripts/verify.sh --all`（Windows）と WSL の fmt / clippy / test。通ったら実装をコミット（`timefmt + config` / `runner` / `app + ui` / `tui` の 4 コミットを目安に、どの区切りでも `verify.sh` が通るなら分ける。通らない区切りなら 1 コミット）
9. 実機確認（下の層 4）。見つかった不具合は別コミット
10. CLAUDE.md と `/tui-test` を更新してコミット
11. verifier → `/pr`

## リスク

- **一番危険なのは「停止して再実行」と再読み込みの組み合わせ**: 再実行待ち（`restart_pending`）のコマンドが再読み込みで消えた場合は `Stop` だけ送り、再実行しない（`replace_config` で旧 `CommandView` を捨てるので `Exited` は無視される）。
  再実行待ちの間に名前が引き継がれた場合は、新しい `spec` で再実行される（意図どおり）。層 1 で両方テストする
- **`RunId` の取り違え**: `Runner` のキーと `App` の `run` が一致しないと通知が届かない。`App` だけが採番し、`Effect::Start { run }` で `Runner` に渡す。層 1 で古い `run` の通知が無視されることを確認
- **毎秒の描画が止まらない / 止まりすぎる**: `needs_tick` は「`Running` がある、または `finished_at` がある」。未実行だけなら偽。層 1 でテスト。CPU は実機で `top` / タスクマネージャーを目視
- **`Instant` を直接取らない**: `app.rs` に `Instant::now()` を書かない（テストで時間を進められなくなる）。`tui` だけが呼ぶ
- **ui の期待値の更新**: 見た目の仕様変更なので期待値を変えるが、`row()` ヘルパーで列幅を固定して手で空白を数えない。時間の列は固定の `Instant` と `set_now` で決定的にする
- **40% の上限**: 列が増えた分、`list_width_is_capped_at_40_percent` の期待値（16 → 変わらず 16。上限側で決まる）と `long_names_and_lines_are_truncated` を確認
- **再読み込みの失敗メッセージ**: toml のエラーは複数行で設定ファイルを引用する。1 行目だけを `sanitize` して出す（rust-safety 3 章）
- **Windows と Unix の差**: `runner` の変更はキーの型だけなので cfg の差は増えない。両方で `cargo test runner::`

## 証明（Proof）

自動

- `bash .claude/scripts/verify.sh --all` が `VERIFY OK`（Windows）。WSL で fmt / clippy（`-D warnings`）/ test が通る
- 層 1 `src/timefmt/tests.rs`: `elapsed_seconds`／`elapsed_minutes`／`elapsed_hours`／`elapsed_days`／`ago_uses_largest_unit`／`never_wider_than_seven_columns`（1 秒〜99 日の代表値）
- 層 1 `src/config/tests.rs`: 既存 15 件＋`load_file_reads_the_given_file`／`load_file_reports_missing_file`。`parses_minimal_config` と `load_searches_parent_directories` で `path` も確認
- 層 1 `src/runner/tests.rs`: 既存を `RunId` に置き換えて全件＋`set_shell_applies_to_next_start`
- 層 1 `src/app/tests.rs`: 既存を `RunId` / `Vec<Effect>` に合わせて全件＋`run_allocates_increasing_run_ids`／`events_for_stale_run_are_ignored`／
  `elapsed_while_running`／`ago_and_took_after_exit`／`needs_tick_only_after_first_run`／`enter_on_running_requests_stop_and_restart`／
  `restart_starts_new_run_after_exit`／`second_enter_while_restarting_is_noop`／`stop_cancels_pending_restart`／`stop_failed_keeps_pending_restart`／
  `reload_action_returns_reload_effect`／`replace_config_keeps_same_names`／`replace_config_stops_removed_running`／`replace_config_moves_selection`／
  `replace_config_drops_pending_restart_of_removed`／`notice_is_cleared_by_next_action`／`notice_is_sanitized_to_one_line`
- 層 2 `src/ui/tests.rs`: 既存を新しい列・見出しに更新して全件（`renders_list_and_output_at_80x24` が時間の列の経過・ago と実行中の見出しを含む）＋
  `header_shows_took_after_exit`／`header_shows_name_only_when_idle`／`notice_replaces_help_line`／`layout_reserves_rows_for_header_and_help`（出力欄が見出しを除く）
- 層 3 `tests/cli.rs`: 変更なしで全 9 件が通る

実機（層 4。WSL は私が疑似端末で、Windows Terminal はユーザー）

- 一時的な `runs.toml`（`sleep 300; echo done` / `echo hello` / `sleep 3`）で:
  - `sleep 3` を実行 → 経過が `1s` `2s` `3s` と進み、終了後に見出しに `took 3s`、一覧に `Ns ago` が進む
  - `sleep 300` を実行したまま `r` → 一覧で `running` のまま。通知 `reloaded runs.toml (2 commands)`
  - `runs.toml` に 1 件足して `r` → 一覧に `added` が出る。壊して `r` → `reload failed:` が出る
  - `sleep 300` に `Enter` → 経過が 0 から数え直す（停止して再実行）
  - `sleep 300` を設定から消して `r` → 一覧から消える。通知 `reloaded runs.toml (1 command)`
  - 各ケースの終了後に `pgrep -f "^sleep 300$"` が 0（残存なし）。「`r` の直後にプロセスが残っていること」「再実行で pid が変わること」は
    疑似端末では直接見ず、層 1（`replace_config_keeps_same_names` が `run` を保つ、`restart_starts_new_run_after_exit` が新しい `run` になる）と
    Windows Terminal の手動確認（`tasklist` の行数）で確かめる
  - `bash .claude/scripts/pty-check.sh` の既存 10 ケースが #5 と同じ結果
- Windows Terminal: 上と同じ手順を `ping -t 127.0.0.1` と `timeout /t 3` で。待機中に CPU が 0% 近くであること（タスクマネージャー）

未検証として報告するもの

- macOS、MSRV 1.88、cargo-deny

## 並行可能な作業

なし。`RunId` の変更が `runner` → `app` → `tui` に連鎖するので 1 つの作業ツリーで順に進める。

## 捨てた案

- **`SystemTime` で絶対時刻も持つ**: intent で絶対時刻は出さないと決めた。`Instant` だけなら時計の巻き戻しの影響も受けない
- **別スレッドの 1 秒タイマー**: `event::poll(50 ms)` のループに「前回の描画から 1 秒」の判定を足すだけで足りる。スレッドを増やさない
- **再実行を `Runner::restart` で行う**（`Runner` が `Exited` を見て起動し直す）: `Runner` は `rx` を見ないので実装が複雑になる。`App` が `Exited` で `Start` を返す方が状態遷移が 1 か所にまとまる
- **再読み込みを `App` 内でファイルを読んで行う**: `App` にファイル I/O を入れない（端末なしでテストできる構造を保つ）。`Effect::Reload` で `tui` に頼む
- **`CommandId` を残して `RunId` を別に足す**: 2 つの ID を持ち回ると取り違えやすい。`Runner` のキーは `RunId` だけにし、`App` は `run` からコマンドを探す（コマンド数は少ない）
- **通知を別の行に出す**: 画面を 1 行消費する。最下行のキー案内と排他にすれば十分（次のキーで消える）
