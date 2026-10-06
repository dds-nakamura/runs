# Plan: 起動して q / Ctrl+C で終了する最小 TUI を作る（spec.md 2026-10-05 より）

## Context

`runs` は `cargo init` と依存追加（ratatui 0.30 / anyhow）まで済んでいるが、TUI のコードが無い（`src/main.rs` は 5 行でバージョンを表示するだけ）。
issue #1 で、以後の機能が乗る土台（モジュール構成・端末の初期化と復元・テストの型）を作る。アプリの機能は載せない。

- ブランチ: `feat/1-minimal-tui`
- 2026-10-05 にユーザーが承認
- 依存クレートは追加しない

## spec からの変更（この計画で 2 点だけ詰めた。同じコミットで spec.md も直す）

1. **Ctrl+C の判定をゆるめる**: spec は「`c` かつ修飾キーが CONTROL のみ」。これを「修飾キーに CONTROL を含み、文字が `c` または `C`」にする。
   理由: Caps Lock が有効だと `q` は `Q` になって効かない。Ctrl+C まで効かないと終了できなくなる。`q` は spec どおり厳密なままにする。
2. **画面のタイトルを `App` に持たせる**: `App::new(title)` で受け取り、`main` が `cli::version_text()` を渡す。
   理由: 描画テストの期待値がクレートのバージョンに依存しないようにする（テストでは固定の文字列を渡す）。
   あわせて、幅が足りないときの挙動を「行頭から表示して末尾を切る」に確定する（spec は「ratatui の既定に任せる」）。

## 実装中に分かったこと（計画との差分）

- spec の「カーソルは非表示にしない」は誤りだった。`Terminal::draw` はカーソル位置を指定しないと毎回カーソルを隠す。
  戻すのは `Terminal` の drop なので、ガードが `Terminal` を持つ今の設計のままで復元される。spec.md を直した。コードの変更は無い
- `TerminalGuard` は `tui::run` だけが使うので非公開にした
- 層 1 に `ctrl_c_release_is_ignored` と `version_text_is_name_and_version` を足した
- **panic 時に端末を二重に復元していた**。ratatui の panic hook が復元した後、巻き戻しでガードの drop がもう一度 `try_restore()` を呼び、
  alternate screen を出るシーケンス（`ESC[?1049l`）が panic メッセージの後にも出ていた。xterm 系ではこのシーケンスがカーソル位置も復元するので、
  メッセージが次のプロンプトで上書きされるおそれがある。`std::thread::panicking()` のときはガードの drop で復元しないようにした。
  WSL の疑似端末で、直す前は 2 回、直した後は 1 回だけ出ることを確認した
- **termio のセッションが無かった**ので、実機確認は次のように分けた
  - WSL: `script` コマンドの疑似端末で自動確認した（キー入力・終了コード・出力されたエスケープシーケンス・終了後の `stty` のモード）。
    端末エミュレータ上での見た目は確認していない
  - Windows Terminal: 2026-10-05 にユーザーが手動で確認し、6 項目すべて期待どおりだった
    （2 行の表示／Esc・Shift+Q で変化なし／リサイズと極小サイズ／`q` で終了して端末が戻り終了コード 0／
    Ctrl+C で終了（Caps Lock 中も）／パイプ時にメッセージと終了コード 1）。panic 時の復元は Windows では確認していない
- WSL での確認を再現できるよう、使ったスクリプトを `.claude/scripts/pty-check.sh` として追加した（計画に無かったファイル）。
  あわせて `rust-safety` 2 章に二重復元の注意を、`.claude/README.md` にスクリプトの説明を足した

- reviewer の指摘（Important 1・Nit 8）への対応
  - spec.md: Windows の Ctrl+Break / ウィンドウを閉じる操作でも復元されないことを懸念点 3 に追記（#3 の対象）。
    巻き戻し中は復元しない例外、非端末の片側ずつのケースを `pty-check.sh` で確かめること、mintty、`try_restore()` の途中失敗を追記
  - `src/cli/tests.rs`: `invalid_utf16_argument_is_error`（Windows のみ）を追加
  - `pty-check.sh`: バイナリのパスを文字列に埋め込まず、環境変数で渡すようにした
  - 対応しなかったもの: `try_restore()` が raw mode の解除に失敗すると alternate screen を出ない点（spec.md の懸念点 9）

## 変更するファイル

- `src/main.rs`（変更）: `mod` 宣言、`fn main() -> ExitCode`。`cli::parse` の結果で分岐し、出力と終了コード（0 / 1 / 2）を決める。
  出力は `writeln!` を使う（`println!` / `eprintln!` は使わない）。`#![forbid(unsafe_code)]` は残す
- `src/cli.rs`（新規）: `Command { Run, Version, Help }`、`UsageError`、`parse`、`version_text`、`help_text`。
  全引数を見て、知らない引数が 1 つでもあれば `UsageError`。無ければ最初のフラグに従う。
  エラーメッセージに入れる引数は `to_string_lossy` の後、制御文字（`char::is_control`）を `?` に置き換える
- `src/cli/tests.rs`（新規）: 引数の解釈と文言
- `src/app.rs`（新規）: `App { title, should_quit }`、`Action { Quit }`、`action_for(&Event) -> Option<Action>`、`App::apply`、`App::should_quit`、`App::title`
- `src/app/tests.rs`（新規）: キー入力 → Action → 状態
- `src/ui.rs`（新規）: `draw(frame, app)`。タイトルと `q / Ctrl+C: quit` の 2 行を中央に置く。
  縦は `Layout::vertical([Length(1), Length(1)]).flex(Flex::Center)`、各行の横は `Layout::horizontal([Length(行の幅)]).flex(Flex::Center)`。
  行の幅は `Line::width()` を `u16::try_from(..).unwrap_or(u16::MAX)` で変換する。`split` の結果は添字でなく `zip` で回す。`u16` の引き算は書かない
- `src/ui/tests.rs`（新規）: `TestBackend` での描画
- `src/tui.rs`（新規）: `TerminalGuard`（`new` は `ratatui::try_init()`、`Err` なら `try_restore()` を呼んでから返す。`Drop` は `try_restore()`、失敗は `writeln!(stderr)` で伝え、その失敗は無視）と
  `run(app)`（stdin / stdout の `IsTerminal` を確認 → ガード作成 → 「描画 → `event::read()` → `action_for` → `apply`」のループ）
- `tests/cli.rs`（新規）: `env!("CARGO_BIN_EXE_runs")` を `Command` で実行。stdin は `Stdio::null()` を明示する
- `CLAUDE.md`（変更）: Architecture にモジュール構成を書く
- `.claude/skills/tui-test/SKILL.md`（変更）: 「ヘルパーとモジュール構成はまだ無い」を実際の配置と書き方に置き換える
- `.steering/1-minimal-tui/plan.md`（新規）、`.steering/1-minimal-tui/spec.md`（変更: 上の 2 点）

`Cargo.toml` / `Cargo.lock` は変更しない。

## 作業順

各モジュールで「テストと、コンパイルが通るだけの空の実装」を先に書き、テストが期待どおりの理由で失敗することを確認してから実装する。

1. plan.md を保存し、spec.md の 2 点を直してコミット
2. `app`: テスト → 失敗確認 → 実装（`cargo test app::` で確認）
3. `cli`: テスト → 失敗確認 → 実装（`cargo test cli::`）
4. `ui`: テスト → 失敗確認 → 実装（`cargo test ui::`）
5. `tui` と `main`、`tests/cli.rs`: CLI テスト → 失敗確認 → 実装（`cargo test --test cli`）
6. `bash .claude/scripts/verify.sh --all` が通ったら、実装を 1 コミットにまとめる。WSL でも fmt / clippy / test を実行する
   （モジュールごとに分けてコミットすると、`main.rs` から呼ばれる前のモジュールが dead_code 警告で clippy に落ちるため）
7. 実機確認（下の「証明」の層 4）
8. CLAUDE.md と `/tui-test` を更新してコミット
9. verifier エージェントで「証明」の項目を別コンテキストで実行 → `/pr`

## リスク

- **一番危険なのは `src/tui.rs`**。単体テストが書けず、誤ると利用者のシェルが壊れる。確認は層 3（非 TTY）と層 4（実機）とレビューに頼る
  - ガードの drop より先にエラーを表示すると、メッセージが alternate screen に消える。`tui::run` が返ってから `main` で表示する
  - `try_init()` の `Err` 経路と実行中の I/O エラーは実機で再現できない（未検証として報告する）
- **Windows と Unix の差**: Press / Release の扱いは層 1 のテストで固定する。UTF-8 に変換できない引数のテストは OS ごとに別（`#[cfg(unix)]` と `#[cfg(windows)]`）なので、Windows と WSL の両方で実行する
- **極小サイズ**: 0x0・1x1 を層 2 のテストで確認する。レイアウトの余りが奇数のときの丸めは期待値にしない（80x24 と 10x2 は余りが偶数か 0）
- **WSL でのビルド**: リポジトリは `/mnt/c` 上にある。`target/` を Windows と共有しないよう、WSL では `CARGO_TARGET_DIR=$HOME/.cache/runs-target` を指定する
- **clippy `-D warnings`**: `main.rs` から呼ばれる前のモジュールは dead_code 警告になる。作業順 2〜5 の途中は `cargo test` だけで確認し、`verify.sh` は全体がつながった後に通す
- 既存のキーバインド・設定・CLI 引数は無いので、互換性の問題は無い

## 証明（Proof）

自動

- `bash .claude/scripts/verify.sh --all` が `VERIFY OK`（Windows）
- WSL の Ubuntu で `cargo fmt --check`・`cargo clippy --all-targets -- -D warnings`・`cargo test` が通る
- 層 1 `src/app/tests.rs`: `initial_state_is_running`／`q_press_quits`／`ctrl_c_press_quits`／`ctrl_shift_c_press_quits`／`q_release_is_ignored`／`q_repeat_is_ignored`／`ctrl_c_release_is_ignored`／
  `other_keys_are_ignored`（Esc・`Q`・Alt+q・Ctrl+q・修飾なしの `c`）／`resize_is_ignored`
- 層 1 `src/cli/tests.rs`: `no_args_runs`／`version_flags`／`help_flags`／`unknown_argument_is_error`／`extra_argument_is_error`／`first_flag_wins`／
  `control_chars_in_argument_are_replaced`／`help_text_lists_options`／`version_text_is_name_and_version`／`non_utf8_argument_is_error`（Unix のみ）／`invalid_utf16_argument_is_error`（Windows のみ）
- 層 2 `src/ui/tests.rs`（タイトルは固定の `runs 1.2.3`）: `renders_centered_at_80x24`（画面全体を比較）／`truncates_at_10x2`／`renders_first_cell_at_1x1`／`does_not_panic_at_0x0`
- 層 3 `tests/cli.rs`: `version_prints_to_stdout`（`runs <バージョン>`、終了コード 0）／`short_version_flag`／`help_exits_zero`／
  `unknown_argument_exits_2`（stderr にメッセージ）／`no_args_without_tty_exits_1`（stderr にメッセージ、stdout が空）

実機（層 4。termio MCP の既存セッションで私が実施。セッションが無ければ手順を渡してユーザーに依頼する）

- 対象: Windows Terminal（PowerShell）と WSL の Ubuntu 24.04
- 起動 → 2 行の画面が中央に出る → `q` で終了 → プロンプトが戻り、入力がエコーされ、起動前の画面内容が残っている
- 同じ手順を Ctrl+C で
- Esc・`Q` では終了しない
- リサイズで描き直される。極小サイズにしても落ちない
- `runs | cat` と `runs < /dev/null`（PowerShell では相当のリダイレクト）で、メッセージが出て終了コード 1、端末が壊れない
- panic 時の復元: イベントループに `panic!` を一時的に入れてビルドし、端末が戻って panic メッセージが読めることを確認する。確認後に差分を捨て、コミットしない

未検証として報告するもの

- macOS
- `try_init()` の `Err` 経路、実行中の I/O エラーの経路
- MSRV 1.88 のツールチェーンでのビルド

## 並行可能な作業

なし。`app`・`cli`・`ui` は互いに独立だが、それぞれ小さく、`main.rs` の `mod` 宣言で衝突するので 1 つの作業ツリーで順に進める。

## 捨てた案

- **`try_init()` を使わず、raw mode と alternate screen を自前で 1 段ずつ有効にして、有効にした分だけ戻す**: 失敗時の復元が正確になるが、panic hook も自前で書くことになる。
  マージ済みの `rust-safety` 2 章の方針（`try_init()` を土台にし、`Err` のときも `try_restore()`）に従う
- **lib + bin に分ける**: `tests/` から内部の関数を呼べるようになるが、いま必要なテストは `src/<モジュール>/tests.rs` で書ける。CLAUDE.md の「単一のバイナリクレート」を保つ
- **`insta` / `assert_cmd` を入れる**: 画面は 2 行、CLI は 5 ケースなので、`assert_buffer_lines` と `std::process::Command` で足りる
- **タイムアウト付きのポーリング（`event::poll`）でループする**: 定期的に更新する表示が無いので、ブロッキングの `event::read()` にする
- **`Paragraph` の中央寄せに任せる**: 幅が足りないときにどこが切られるかが配置の実装に依存する。`Layout` で矩形を決めて左寄せで描き、「行頭から表示」を確定させる
- **デバッグ用の環境変数で panic を起こす仕掛けを入れる**: 非テストコードに `panic!` を置かない規約に反するので、実機確認のときだけ一時的に入れる
