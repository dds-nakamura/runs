# runs（Rust ターミナルアプリ）

EVECLOUD とは独立した単独プロジェクト。EVECLOUD の規約・Backlog `THEMIS_GO`・evecloud-* のスキルやエージェントは適用しない。
リモート: GitHub `dds-nakamura/runs`（private。課題は GitHub Issues、PR は `gh`）。応答・ドキュメントは日本語。仕様の細部は未確定（末尾の「未確定事項」）。決まったらこのファイルを更新する。

## Commands

- 検証（これ1つ）: `bash .claude/scripts/verify.sh` → 最終行 `VERIFY OK` で成功
  - 中身: `cargo fmt --check`／`cargo clippy --workspace --all-targets --all-features -- -D warnings`／`cargo test --workspace --all-features`
  - CI 相当: `bash .claude/scripts/verify.sh --all`（`--locked` 付き + `cargo doc` 警告ゼロ + `cargo deny check`（導入時））
- 個別テスト: `cargo test <テスト名>`
- 実行: **対話 TUI を Bash ツールで起動しない**（TTY が無くハングする）。非対話なら `cargo run -- <引数>`。
  実機確認は PTY を持つ端末で行う（`/tui-test` の「層 4」）
- 疑似端末での確認（Linux / WSL）: `bash .claude/scripts/pty-check.sh`（キー入力・終了コード・終了後の端末モードを表示する。合否は出力を読んで判断）

## 開発フロー（成果物を順に `.steering/<ブランチ名の type/ 以降>/` にコミット）

1. `/intent <issue番号 | 要望>` → `intent.md`（課題の意図）
2. `/spec` → `spec.md`（要件＋設計。規約と `rust-safety` を制約として適用。懸念点を明示）
3. `/plan` → プランモードで `plan.md`（変更ファイル・作業順・リスク・証明方法）。**承認前にコードを書かない**
4. 実装 → `verify.sh` を回し続ける。計画から外れたら同じコミットで `plan.md` を更新。
   「実装中に分かったこと」への追記だけでなく、該当する節（plan の証明・リスク、spec の設計・受け入れ条件）も直す
5. 不具合修正は `/fix-bug`（失敗するテストを別ファイルに先に書き、テストをロックしてから直す）
6. `/pr` → reviewer サブエージェントのレビュー後に PR。指摘対応は `/pr-feedback`

小さな修正（1ファイル・自明）は 1〜3 を省略してよいが、検証は省略しない。

## Verifying your work

完了報告の前に必ず `bash .claude/scripts/verify.sh` を実行し、出力の要約（最後の10行程度）を貼る。
テストが落ちたらコードを直す。**テストを消す・`#[ignore]` にする・期待値を実装に合わせて書き換えるのは禁止**。
clippy 警告を `#[allow(...)]` で黙らせる場合は理由コメント必須。
実行できなかった確認（PTY での実機確認など）は「未検証」と明記し、推測で成功と書かない。

## Architecture（暫定）

- クレート名・バイナリ名は `runs`。単一のバイナリクレート（edition 2024、MSRV 1.88、`publish = false`、lib ターゲットなし）
- モジュール（実端末に触るのは `tui` と `main` だけ）
  - `src/main.rs`: 引数の解釈結果で分岐、エラーの表示、終了コード（0 = 正常、1 = 実行時エラー、2 = 引数の誤り）。`#![forbid(unsafe_code)]`
  - `src/cli.rs`: 引数の解釈（手書き。`Command`）、バージョンと使い方の文字列
  - `src/app.rs`: 状態 `App`、`Action`、キーバインド（`action_for`: イベント → Action）、更新（`App::apply`）
  - `src/ui.rs`: 描画（`draw(frame, app)`）。`TestBackend` に描ける
  - `src/tui.rs`: 端末ガード（`TerminalGuard`）とイベントループ（`run`）
- テストは実装と別ファイル: `src/<モジュール>/tests.rs`（層 1・2）、`tests/cli.rs`（層 3）。`tui` と `main` は層 3・4 で確認する
- 画面と CLI のメッセージは英語（ASCII）。出力は `writeln!` を使い、`println!` / `eprintln!` は使わない（閉じたパイプへ書くと panic する）
- 依存: `ratatui` 0.30（`default-features = false`、feature は `crossterm` / `layout-cache` / `underline-color`）、`anyhow` 1。
  crossterm（0.29）は直接依存にせず `ratatui::crossterm` を使う（ratatui とバージョンがずれるのを避ける。例外は `rust-safety` 8章）
- 方針: 状態（モデル）・更新（入力→状態）・描画を分け、状態と更新は端末なしでテストできるようにする
- 端末の初期化・復元は 1 か所（RAII ガード + panic hook）に閉じ込める。土台は `ratatui::try_init()` / `ratatui::try_restore()`。
  `try_init()` は途中で失敗しても元に戻さないので、`Err` のときも復元する（`rust-safety` 2章）

## Conventions

- ブランチ: `<type>/<issue番号>-<slug>`（例: `feat/12-key-bindings`。type: feat / fix / refactor / docs / chore。issue が無ければ `<type>/<slug>`）。main / master へ直接 push しない
- コミット: `<日本語で何をしたか> (#12)`（issue が無ければ要約のみ。行頭を `#` にしない。1コミット1論点）。
  作業ブランチ上なら区切りのよいところでコミットしてよい。main 上のコミットと push は確認が入る
- 依存クレートの追加は spec / plan で合意してから（`cargo add` は確認が入る）
- 非テストコードで `unwrap()` / `expect()` / `panic!` を使わない。`unsafe` は原則禁止（`rust-safety`）

## Things Claude gets wrong

- 対話 TUI を Bash ツールで `cargo run` してハングさせる
- panic・エラー時に端末を復元しない（raw mode / alternate screen / カーソル非表示のまま終了）
- `area.width - 2` のような `u16` の減算で、極小の端末サイズでアンダーフローさせる。`&s[..n]` で UTF-8 の文字境界を壊す
- Windows と Unix の差を片側だけで実装・確認する（crossterm の Windows ではキーの Press と Release が両方届く、パス区切り、改行）
- TUI 実行中に `println!` / `dbg!` で stdout に出して画面を崩す
- `Cargo.lock` を手で編集する／`cargo update` で無関係な依存まで上げる
- 実装中に計画が変わったとき、plan.md に差分を追記するだけで、古くなった節（証明のテスト名・リスク・spec の設計）を直さない

## 未確定事項（決まったら更新する）

- [x] 対象 OS・端末 → Windows 11（Windows Terminal）/ Linux / macOS の 3 OS すべてを一次対象とする。conhost は一次対象に含めない
- [x] Linux の確認手段 → WSL の Ubuntu 24.04（`cargo test` と `pty-check.sh`。`CARGO_TARGET_DIR=$HOME/.cache/runs-target` を指定し、`target/` を Windows と共有しない）
- [ ] macOS の確認手段（未定）。CI の 3 OS マトリクスは未導入
- [x] TUI ライブラリ → ratatui 0.30 + crossterm 0.29（`ratatui::crossterm` 経由）
- [x] エラー処理クレート → `anyhow` のみで開始。エラーの種類で分岐する必要が出たら `thiserror` の追加を spec で合意する
- [x] 課題管理とリモート → GitHub `dds-nakamura/runs`（private）
- [x] edition・MSRV → edition 2024 / MSRV 1.88（`rust-version`。1.88 のツールチェーンでの実ビルドは未検証）
- [ ] 配布方法（cargo install / バイナリ配布。最初のリリース前に決める）
