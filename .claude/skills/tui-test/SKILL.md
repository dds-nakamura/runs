---
name: tui-test
description: ターミナル UI のテスト（状態更新の単体テスト・描画スナップショット・CLI 結合テスト・PTY での実機確認）を作成・実行する。画面・キー操作の確認、UI 変更の検証、画面不具合の再現テストを書くときに使う。
argument-hint: <対象画面・キー操作・機能>
---

# ターミナル UI のテスト

> 採用: ratatui 0.30。crossterm 0.29 は `ratatui::crossterm` 経由で使う（直接依存にしない）。
> `insta`・`assert_cmd` は未導入（導入は spec で合意してから）。
> モジュール構成は CLAUDE.md の Architecture。既存のテスト（下の「既存のヘルパー」）の流儀に合わせる。

## 層と使い分け（下の層ほど速く安定。できるだけ下で証明する）

| 層 | 何を証明するか | 書き方 |
|---|---|---|
| 1. 状態更新 | キー入力 → 状態の変化 | `ratatui::crossterm::event::KeyEvent`（または Action）を更新関数に渡し、状態を assert。端末不要 |
| 2. 描画 | 状態 → 画面の見た目 | `Terminal::new(TestBackend::new(80, 24))` に `draw` し、`terminal.backend().assert_buffer_lines([...])` で比較 |
| 3. CLI | 引数・終了コード・非 TTY 時の挙動 | `tests/*.rs` で `std::process::Command::new(env!("CARGO_BIN_EXE_runs"))` を実行 |
| 4. 実機（PTY） | 実端末でのキー入力・リサイズ・終了時の端末復元 | 自動テストではない。PTY を持つ端末で手動確認（下記） |

配置: 単体テスト（層 1・2）は実装と別ファイルの `src/<モジュール>/tests.rs`（`#[cfg(test)] mod tests;`）、層 3 は `tests/*.rs`。
`/fix-bug` がテストファイル単位でロックするため、実装と同じファイルに `mod tests { ... }` を書かない。

層 1 の定番ケース:

- キーは `KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)` で作る（`kind` は Press になる）
- Windows では Press と Release が両方届く。更新関数は `KeyEventKind::Press` 以外を無視し、
  `KeyEvent::new_with_kind(.., KeyEventKind::Release)` を渡しても状態が変わらないケースを 1 つ書く
  （キーボード拡張の `REPORT_EVENT_TYPES` を有効にすると長押しが `Repeat` で届く。有効にするときは Repeat の扱いを見直す）
- Ctrl+C（`KeyCode::Char('c')` + `KeyModifiers::CONTROL`）で終了状態になること

層 2 の補足: `TestBackend` は `Display` を実装している。`insta` 導入後は `assert_snapshot!(terminal.backend())` に置き換える。

既存のヘルパー（新しいテストはこれを使うか、同じ形で足す）:

| ファイル | ヘルパー | 用途 |
|---|---|---|
| `src/app/tests.rs` | `press(code, modifiers)` / `key(code, modifiers, kind)` | キーイベントを作る |
| `src/app/tests.rs` | `feed(&mut app, &event)` | イベントループと同じ手順（`action_for` → `apply`）で 1 件処理する |
| `src/ui/tests.rs` | `render(width, height)` | 固定タイトルの `App` を `TestBackend` に描き、`Terminal` を返す |
| `src/cli/tests.rs` | `parse_args(&[..])` / `error_message(&[..])` | 引数を解釈する／エラーメッセージを取り出す |
| `tests/cli.rs` | `runs(&[..])` / `stdout(&output)` / `stderr(&output)` | バイナリを端末なし（stdin は null、出力はパイプ）で実行する |

- 描画の期待値は、レイアウトの余りが偶数になるサイズで書く（80x24 など）。余りが奇数のときの丸めを期待値にしない
- 描画テストにクレートのバージョンなど変わる値を入れない（`App::new` に固定の文字列を渡す）
- `tests/cli.rs` から内部の関数は呼べない（lib ターゲットが無い）。内部のロジックは `src/<モジュール>/tests.rs` で確かめる

## 手順

1. 近い既存テストを読み、配置・ヘルパー・命名の流儀に合わせる
2. 対象フローのハッピーパス → 異常系（不正入力・空データ・権限なしファイル）→ 境界の順にケースを挙げ、ユーザーに確認する
   - 境界の定番: 端末サイズ（80x24 と極小 例 10x3、幅 1・高さ 1）、全角・絵文字・結合文字、長い行、空リスト、末尾での上下移動
3. 実装方針
   - 描画テストは端末サイズを固定する（`TestBackend::new(80, 24)`）。サイズ違いは別ケースにする
   - 時刻・乱数・環境変数・ファイルシステムに依存させない（注入する）
   - `thread::sleep` で待たない。非同期処理は完了をチャネルで受けるか、状態を直接進める
   - テスト間で端末・グローバル状態を共有しない
4. 実行: `cargo test <テスト名>`。失敗したら原因を調べてから直す（期待値を実装に合わせて書き換えない）
5. 報告: ファイル、ケース数（正常/異常/境界）、実行結果の出力、カバーしていない観点（特に層 4 で未確認のもの）

## 層 4: PTY での実機確認

Bash ツールは TTY を持たないので、**Bash で対話 TUI を起動しない**（ハング・端末破壊の原因）。

- termio MCP が使える場合: 既存セッション（`list_sessions`）に `send_input` で `cargo run` とキー入力を送り、
  `wait_for_output` / `read_output_since` で画面を確認する。終了後にプロンプトが正常に戻るか（raw mode・カーソル）も確認する
- 使えない場合: 確認手順（起動コマンド・押すキー・期待する画面）を書いてユーザーに依頼し、結果を待つ。自分で「確認済み」と書かない
- 一次対象は Windows 11（Windows Terminal）/ Linux / macOS。このうち変更が影響する OS を intent.md の「影響する利用者・環境」から決める
  - Windows: Windows Terminal で確認する（conhost は一次対象外）
  - Linux: WSL の Ubuntu 24.04（Rust 導入済み）。`CARGO_TARGET_DIR=$HOME/.cache/runs-target` を指定してビルドする
    - `bash .claude/scripts/pty-check.sh` が疑似端末（`script` コマンド）で起動し、キー入力・終了コード・
      エスケープシーケンス・終了後の端末モードを表示する。Bash ツールから実行できる（timeout 付きなのでハングしない）
    - これは実端末での目視の代わりにはならない。報告では「疑似端末で確認」と「端末エミュレータで目視」を分けて書く
    - panic 時の復元は、イベントループに `panic!` を一時的に入れたビルドを別の `CARGO_TARGET_DIR` に作って同じスクリプトで確かめ、差分を捨てる
  - macOS: 確認手段が未定。確認できていない OS は報告に「未検証」と明記する
