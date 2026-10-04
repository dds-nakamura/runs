# terust（Rust ターミナルアプリ）

EVECLOUD とは独立した単独プロジェクト。EVECLOUD の規約・Backlog `THEMIS_GO`・evecloud-* のスキルやエージェントは適用しない。
リモート: GitHub `dds-nakamura/runs`（private。課題は GitHub Issues、PR は `gh`）。応答・ドキュメントは日本語。仕様の細部は未確定（末尾の「未確定事項」）。決まったらこのファイルを更新する。

## Commands

- 検証（これ1つ）: `bash .claude/scripts/verify.sh` → 最終行 `VERIFY OK` で成功
  - 中身: `cargo fmt --check`／`cargo clippy --workspace --all-targets --all-features -- -D warnings`／`cargo test --workspace --all-features`
  - CI 相当: `bash .claude/scripts/verify.sh --all`（`--locked` 付き + `cargo doc` 警告ゼロ + `cargo deny check`（導入時））
- 個別テスト: `cargo test <テスト名>`
- 実行: **対話 TUI を Bash ツールで起動しない**（TTY が無くハングする）。非対話なら `cargo run -- <引数>`。
  実機確認は PTY を持つ端末で行う（`/tui-test` の「層 4」）

## 開発フロー（成果物を順に `.steering/<ブランチ名の type/ 以降>/` にコミット）

1. `/intent <issue番号 | 要望>` → `intent.md`（課題の意図）
2. `/spec` → `spec.md`（要件＋設計。規約と `rust-safety` を制約として適用。懸念点を明示）
3. `/plan` → プランモードで `plan.md`（変更ファイル・作業順・リスク・証明方法）。**承認前にコードを書かない**
4. 実装 → `verify.sh` を回し続ける。計画から外れたら同じコミットで `plan.md` を更新
5. 不具合修正は `/fix-bug`（失敗するテストを別ファイルに先に書き、テストをロックしてから直す）
6. `/pr` → reviewer サブエージェントのレビュー後に PR。指摘対応は `/pr-feedback`

小さな修正（1ファイル・自明）は 1〜3 を省略してよいが、検証は省略しない。

## Verifying your work

完了報告の前に必ず `bash .claude/scripts/verify.sh` を実行し、出力の要約（最後の10行程度）を貼る。
テストが落ちたらコードを直す。**テストを消す・`#[ignore]` にする・期待値を実装に合わせて書き換えるのは禁止**。
clippy 警告を `#[allow(...)]` で黙らせる場合は理由コメント必須。
実行できなかった確認（PTY での実機確認など）は「未検証」と明記し、推測で成功と書かない。

## Architecture（暫定）

- 構成は cargo init 後に記入する
- 方針: 状態（モデル）・更新（入力→状態）・描画を分け、状態と更新は端末なしでテストできるようにする
- 端末の初期化・復元は 1 か所（RAII ガード + panic hook）に閉じ込める（`rust-safety` 2章）

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

## 未確定事項（決まったら更新する）

- [ ] 対象 OS・端末（Windows / Linux / macOS、Windows Terminal / conhost）
- [ ] TUI ライブラリ（候補: ratatui + crossterm）→ `/tui-test` を具体化
- [ ] エラー処理クレート（候補: anyhow / thiserror）→ `rust-safety` 1章を更新
- [x] 課題管理とリモート → GitHub `dds-nakamura/runs`（private）
- [ ] edition・MSRV、配布方法（cargo install / バイナリ配布）
