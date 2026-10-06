# Plan: 設定したコマンドを一覧から実行し、出力を見て、停止できるようにする（spec.md 2026-10-06 より）

## Context

`runs` の方向は「自分のプロジェクトをターミナルから動かす操作盤（コマンドランナー）」に決まった。#1 で起動・終了・端末復元の土台
（`main` / `cli` / `app` / `ui` / `tui`）ができているが、アプリの機能はまだ無い。
issue #5 で最初の機能を作る: プロジェクト直下の `runs.toml` に登録したコマンドを一覧から選んで実行し、出力を見て、停止できる 1 画面。
子プロセスの起動・出力・停止の仕組みは、後の機能（`gh` 経由の PR / CI、エージェントの監視）の土台になる。

- ブランチ: `feat/5-command-runner`。2026-10-06 にユーザーが承認
- 依存の追加: `serde`（derive）と `toml` 1.1（`default-features = false`、`std` / `parse` / `serde`）。spec で合意済み
- 既存コードは #1 で書いた 5 モジュール（約 300 行）で、全部把握している。Explore / Plan エージェントは使っていない

## spec から詰めた点（同じコミットで spec.md も直す）

1. **出力欄の高さの渡し方**: spec は「`ui` から `App` に `set_output_height` で渡す」としていたが、`draw(&App)` を不変のままにするため、
   `ui::layout(area) -> Panes`（純粋関数）を `draw` と `tui` の両方が使い、`tui` が各ループで `app.set_output_height(panes.output.height)` を呼ぶ
2. **`runner` のテストはシェルを通さない**: `Runner::new(shell)` の `shell` に実行ファイルそのものを渡す
   （`shell = [runs の実行ファイル]`、`command = "--version"`）。`cmd /C` の引用符の癖を避け、OS に依存しない
3. **左ペインの幅の計算**: `u16` の掛け算は 65535 × 40 であふれるので、`u32` で `width * 40 / 100` を計算して `u16` に戻す
4. **`kill` / `taskkill` の出力は捨てる**（`Stdio::null()`）。TUI 実行中に端末へ流れると画面が崩れる

## 実装中に分かったこと（計画との差分。該当する節も直してある）

- **`runner` の単体テストで `CARGO_BIN_EXE_runs` は使えない**（結合テスト専用の環境変数）。代わりに OS 既定のシェル（`sh -c` / `cmd /C`）と、
  両方にある `echo` / `exit` を使う。本番と同じ経路を通るので、むしろ妥当。終わらないコマンドだけは `sleep` / `ping` を `shell` に直接渡す。spec.md も直した
- **`sanitize` の `\r` の規則を 1 段足した**: 行末の `\r` を 1 つ落としてから「最後の `\r` 以降を残す」。Windows の CRLF 出力で行が空になるのを防ぐ。
  読み取りスレッド側でも LF / CR を落とす（二重で安全側）
- **テスト専用のゲッターは `#[cfg(test)]`**（`App::scroll`、`OutputBuffer::dropped`、`Runner::is_running`）。本体で使われず dead_code になるため
- **`Started` 通知で状態を `Running` に合わせる**（`Run` で先に実行中にしているが、通知が正）
- **`stop_all_and_wait_stops_everything` が Windows で一度落ちた**: `ping` が起動直後に空行を出すため、`Started` 2 件の前に `Output` が割り込む。
  テスト側で `Output` を読み飛ばすようにした（実装の問題ではない）
- **実機確認（WSL）は疑似端末のスクリプトで行った**: 既存の `pty-check.sh` 10 ケースに加え、一時的な `runs.toml`（`sleep 300; echo done` / `echo … exit 3` / 存在しない `cwd`）で
  「`s` で停止 → `stopped`、`pgrep -f "sleep 300"` が 0」「実行したまま `q` → 終了コード 0、残存 0」「`exit 3` と stdout / stderr の表示」「`failed` と理由」を確認した。
  このスクリプトはリポジトリに入れていない（`pty-check.sh` への統合は別途）

## 変更するファイル

新規

- `src/output.rs` + `src/output/tests.rs`: `OutputBuffer`（`VecDeque<String>`、上限 10,000）、`sanitize(&str) -> String`。
  `sanitize` は小さな状態機械: `ESC [` … 最終バイト（0x40–0x7E）までを捨てる／`ESC ]` … `BEL` または `ESC \` までを捨てる／
  それ以外の `ESC` + 1 文字を捨てる／`\t` は空白 4 つ／`\r` は「最後の `\r` 以降だけ残す」／残る制御文字（`char::is_control`）は `?`
- `src/config.rs` + `src/config/tests.rs`: `FILE_NAME`、`Config`、`CommandSpec`、`load(start_dir)`（親へ探索）、`parse(text, root)`（検証込み）。
  serde 用の中間構造体 `RawConfig { shell: Option<Vec<String>>, command: Vec<RawCommand> }` を `Config` に変換する。
  エラーメッセージ: 見つからない → 探した範囲と最小の例／構文 → `toml` のエラーをそのまま（行・列を含む）／検証 → 項目名入りの自前メッセージ
- `src/runner.rs` + `src/runner/tests.rs`: `CommandId`、`RunnerEvent`、`Runner`（`new` / `start` / `stop` / `stop_all_and_wait` / `is_running`）。
  `start`: `Command` 組み立て → `spawn` → 失敗は `SpawnFailed` → stdout / stderr の読み取りスレッド（`read_until(b'\n')`）→ 終了待ちスレッド（`child.wait()`）。
  `RunningProcess { pid: u32, stop_requested: bool }`。Unix は `CommandExt::process_group(0)` を付け、pgid = pid として `kill -s TERM -- -<pid>`。
  `stop` は停止要求を記録して `kill` / `taskkill` を `Command` で実行（出力は `Stdio::null()`）。
  `stop_all_and_wait(grace)`: 全部に `stop` → `grace` の間 `rx` を見ずに `Exited` を待てないので、終了待ちスレッドの `JoinHandle` を `Runner` が持ち、
  `grace` まで `is_finished()` をポーリング（50 ms）→ 残りに Unix は `kill -s KILL`、Windows は再度 `taskkill /T /F` → `join`
- `runs.toml`（リポジトリ直下）: `verify`（`bash .claude/scripts/verify.sh`）、`test`（`cargo test`）、`clippy`、`fmt-check` を登録。ドッグフーディングと層 3・4 の前提

変更

- `src/app.rs` + `src/app/tests.rs`: `CommandState`、`CommandView`、`Scroll`、`Effect`。`App::new(title, &Config)`、`apply(Action) -> Option<Effect>`、
  `on_runner_event(RunnerEvent)`、`set_output_height(u16)`、参照用のゲッター。`action_for` に `↑↓ j k Enter s PageUp PageDown End` を追加。
  既存のテスト（`q` / Ctrl+C / Release / Repeat / Resize）は `App::new` の引数が変わる分だけ直し、期待値は変えない
- `src/ui.rs` + `src/ui/tests.rs`: `layout(area) -> Panes { list, output, help }`、`draw(frame, &App)`。左: 名前と状態、選択行は `>`。右: `Scroll` に従う行の範囲。最下行: キーの案内
- `src/tui.rs`: `run(app, config)`。`Runner::new(config.shell.clone())`、`event::poll(50 ms)` + `try_recv` のループ、`Effect` → `runner.start / stop`、
  終了時 `runner.stop_all_and_wait(2 秒)`（ガードの drop より前）。`TerminalGuard` は変えない
- `src/main.rs`: `Command::Run` のとき `config::load(&current_dir)` → 失敗は `report` + 終了コード 1 → `App::new(cli::version_text(), &config)` → `tui::run`
- `src/cli.rs` + `src/cli/tests.rs`: `help_text` に `runs.toml` の説明と最小の例を追加。テスト `help_text_lists_options` に `runs.toml` を含む確認を足す
- `tests/cli.rs`: 一時ディレクトリ（`std::env::temp_dir()` 配下に一意な名前。後始末する）を `current_dir` にして、設定なし → 1、壊れた設定 → 1 と行番号、`--help` に `runs.toml`
- `Cargo.toml` / `Cargo.lock`: `cargo add serde --features derive` と `cargo add toml --no-default-features --features std,parse,serde`（確認が入る）
- `CLAUDE.md`: Architecture のモジュール表に `config` / `output` / `runner` と `runs.toml` を追加。Commands に「`runs` の起動には `runs.toml` が要る」
- `.claude/skills/tui-test/SKILL.md`: `runner` のテスト（実プロセス・タイムアウト・`shell` に実行ファイルを渡す）と、新しいヘルパーを追記
- `.steering/5-command-runner/spec.md`: 上の「spec から詰めた点」を反映
- `.steering/5-command-runner/plan.md`: この計画。実装中に外れたら同じコミットで直す（該当する節も）

## 作業順

各モジュールで「テストと、コンパイルが通るだけの空の実装」を先に書き、期待どおりの理由で失敗することを確認してから実装する。
途中のモジュールは `main.rs` から呼ばれるまで dead_code 警告が出るので、2〜7 の間は `cargo test <モジュール>::` で確認し、`verify.sh` は 8 の後に通す。

1. plan.md を保存、spec.md の 4 点を直してコミット。`cargo add` で依存を追加してコミット（依存だけで 1 コミット）
2. `output`: テスト → 実装
3. `config`: テスト → 実装
4. `runner`: テスト → 実装（Windows で先に動かし、WSL でも `cargo test runner::` を通す）
5. `app`: テスト → 実装（既存テストの修正を含む）
6. `ui`: テスト → 実装
7. `tui` / `main` / `cli`、`tests/cli.rs`、`runs.toml`
8. `bash .claude/scripts/verify.sh --all`（Windows）と WSL の fmt / clippy / test。通ったら実装を 1 コミットにまとめる
   （大きければ `output + config` / `runner` / `app + ui` / `tui + main + cli + tests` の 4 コミットに分ける。どの区切りでも `verify.sh` が通ること）
9. 実機確認（下の層 4）。見つかった不具合は別コミット
10. CLAUDE.md と `/tui-test` を更新してコミット
11. verifier エージェントで「証明」を別コンテキストで実行 → `/pr`

## リスク

- **一番危険なのは `runner` の停止と、終了時の全停止**。子プロセスが残ると利用者のポートやファイルを掴んだままになる。
  Unix はプロセスグループ、Windows は `taskkill /T` で木ごと止める。層 1（実プロセス）と層 4（`ps` / `tasklist` で残っていないこと）で確かめる
- **スレッドとチャネル**: 読み取りスレッドが `tx.send` に失敗する（受信側が先に終わった）ときは静かに終える。終了待ちスレッドの `JoinHandle` は `Runner` が持ち、
  `stop_all_and_wait` で `join` する。読み取りスレッドはパイプが閉じれば終わるので `join` しない
- **TUI 実行中の端末出力**: `kill` / `taskkill` の stdout / stderr を捨てる。子プロセスの stdin は `null`、stdout / stderr はパイプ。
  子が `/dev/tty` に直接書く場合（まれ）は画面が崩れるが、この課題では扱わない
- **Windows と Unix の差**: 停止の実装が `cfg` で分かれる。両方でテストを走らせる（WSL）。`process_group` は Unix 専用の trait なので `cfg(unix)` の中で使う
- **大量の出力**: 1 ループで `try_recv` を空になるまで回す。`OutputBuffer` は `VecDeque` で先頭の削除が O(1)。描画は選択中の出力の表示範囲だけを `Paragraph` に渡す
  （10,000 行全部を `Text` にしない）
- **極小サイズ**: `layout` が 0 幅・0 高さの矩形を返しても `draw` が panic しないことを層 2 で確かめる。左ペイン幅の計算は `u32`
- **既存の挙動を変えない**: `--version` / `--help` / 非 TTY の層 3 テスト 5 件と `app` の既存テストはそのまま通す。`pty-check.sh` も既存ケースのまま
  （リポジトリ直下に `runs.toml` を置くので起動できる）
- **テストの一時ディレクトリ**: `tests/cli.rs` と `config` の探索テストは `temp_dir()` 配下に一意なディレクトリを作り、終わりに消す。テスト間で共有しない
- **`cmd /C` の引用符**: 利用者が書く `command` に引用符が含まれると `cmd` の解釈が直感と違うことがある。この課題では文書化のみ（spec の懸念点 1）

## 証明（Proof）

自動

- `bash .claude/scripts/verify.sh --all` が `VERIFY OK`（Windows）。WSL の Ubuntu で fmt / clippy（`-D warnings`）/ test が通る
- 層 1 `src/output/tests.rs`（13 件）: `strips_csi_sequences`／`strips_osc_with_bel_and_st`／`strips_lone_escape`／`replaces_control_chars`／`expands_tab`／
  `keeps_text_after_last_carriage_return`／`trailing_carriage_return_is_line_ending_not_overwrite`／`leaves_unicode_untouched`／`lossy_utf8_becomes_replacement_char`／
  `push_raw_sanitizes`／`drops_oldest_lines_over_limit`／`clear_empties_buffer`／`zero_limit_keeps_nothing`
- 層 1 `src/config/tests.rs`（15 件）: `parses_minimal_config`／`default_shell_per_os`／`custom_shell`／`cwd_defaults_to_root`／`relative_and_absolute_cwd`／
  `duplicate_name_is_error`／`missing_required_field_is_error_with_line`／`syntax_error_is_error_with_line`／`empty_shell_is_error`／`empty_name_is_error`／
  `no_commands_is_error`／`preserves_order`／`load_searches_parent_directories`／`load_without_file_is_error_with_example`／`load_reports_syntax_error_with_file_name`
- 層 1 `src/runner/tests.rs`（12 件。実プロセス、受信は 10 秒でタイムアウト）: `runs_command_and_reports_exit_zero`／`propagates_exit_code`／`captures_stdout_and_stderr`／
  `output_lines_have_no_line_ending`／`events_carry_the_command_id`／`missing_shell_is_spawn_failed`／`missing_cwd_is_spawn_failed`／`stop_terminates_long_running_command`／
  `stop_all_and_wait_stops_everything`／`stop_all_and_wait_returns_immediately_without_processes`／`is_running_tracks_lifecycle`／`start_while_running_is_ignored`
- 層 1 `src/app/tests.rs`（30 件）: #1 の 9 件はそのまま＋`keys_map_to_actions`／`lists_commands_in_config_order`／`select_moves_and_stops_at_ends`／
  `run_marks_running_and_requests_start`／`run_on_running_is_noop`／`stop_requests_only_when_running`／`exited_sets_code`／`exited_after_stop_request_is_stopped`／
  `signal_exit_is_stopped`（Unix のみ）／`spawn_failed_sets_state_and_message`／`rerun_clears_output`／`output_goes_to_its_command`／`events_for_unknown_id_are_ignored`／
  `output_is_sanitized`／`follows_tail_by_default`／`page_up_leaves_follow`／`page_down_returns_to_follow_at_the_end`／`end_returns_to_follow`／
  `new_output_does_not_move_while_scrolled`／`page_up_with_few_lines_stays_at_top`／`selecting_another_command_resets_scroll`／`zero_height_does_not_panic`
- 層 1 `src/cli/tests.rs`: 既存＋`help_text_explains_config_file`
- 層 2 `src/ui/tests.rs`（7 件）: `renders_list_and_output_at_80x24`（3 コマンド、画面全体を比較）／`shows_offset_when_scrolled`／`shows_all_states`／
  `list_width_is_capped_at_40_percent`／`layout_reserves_one_row_for_help`／`does_not_panic_at_tiny_sizes`（0x0・1x1・1x5・5x1・0x5・5x0）／`long_names_and_lines_are_truncated`
- 層 3 `tests/cli.rs`（8 件）: 既存 5 件＋`help_mentions_runs_toml`／`no_config_exits_1_with_hint`／`broken_config_exits_1_with_line_number`

実機（層 4。Windows Terminal はユーザー、WSL は私が疑似端末で。termio のセッションがあれば私が両方）

- リポジトリ直下で起動 → 一覧に `verify` / `test` / `clippy` / `fmt-check` → `test` を `Enter` → 出力が流れ、終わると `exit 0` → `PageUp` で遡る → `End` で末尾
- 終わらないコマンド（一時的に `runs.toml` に `sleep 300` / `ping -t 127.0.0.1` を足す。コミットしない）を実行したまま `test` を実行 → 両方 `running` → `s` で停止 → `stopped` →
  `ps -ef | grep sleep` / `tasklist | findstr ping` に残っていない
- 終わらないコマンドを実行したまま `q` → 2 秒以内に終了し、プロセスが残っていない。端末が戻る
- `runs.toml` の無いディレクトリで `runs` → メッセージと終了コード 1。`runs.toml` を壊す → 行番号付きのメッセージ
- `bash .claude/scripts/pty-check.sh` の既存 10 ケースが #1 と同じ結果
- 大量の出力: `cargo build -vv` を登録して実行し、操作が固まらない（数値の目標は置かない）

未検証として報告するもの

- macOS
- `cmd` 以外のシェル（`pwsh`）での動作
- CP932 で出力するコマンドの文字化け（spec の懸念点 4）
- MSRV 1.88 のツールチェーンでのビルド

## 並行可能な作業

`output` / `config` / `runner` は互いに独立だが、1 人（1 セッション）で順に進める。worktree で分けるほどの量ではない。

## 捨てた案

- **入力スレッド + 単一チャネル**（`event::read` を別スレッドで回し、入力も `RunnerEvent` と同じチャネルに流す）: 待機中のポーリングが無くなるが、
  終了時に `event::read` で止まったスレッドが残る。#1 の「主スレッドだけが端末に触る」構造を保てる `event::poll(50 ms)` にした
- **`libc` / `nix` / Job Object で停止**: `unsafe` 禁止（`rust-safety` 5 章）。`nix` は安全だが依存が大きい。spec で `kill` / `taskkill` に決定
- **`draw(&mut App)` で出力欄の高さを記録**: 描画が状態を変えると層 2 のテストで `&mut` が要り、描画と更新の分離が崩れる。`ui::layout` を純粋関数にした
- **`strip-ansi-escapes` クレート**: 40 行で書ける。将来、色を表示するなら `ansi-to-tui` に置き換える
- **設定を見つけられなくても TUI を起動して空の一覧を出す**: 何が悪いかを TUI の中で伝える UI が要る。stderr に出して終了コード 1 の方が #1 の非 TTY の扱いと揃う
- **出力を `Text` に全部積んで `Paragraph::scroll`**: 10,000 行を毎フレーム組み立てることになる。表示範囲の行だけ渡す
