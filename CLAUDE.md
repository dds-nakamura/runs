# runs（Rust ターミナルアプリ）

自分のプロジェクトをターミナルから動かす操作盤（コマンドランナー）。設定に登録したコマンド（ビルド・テスト・開発サーバーなど）を
TUI から選んで実行し、出力をその場で見て、実行中のものを停止できることが芯（#5）。子プロセスの起動・出力・停止の仕組みを土台に、
`gh` などの CLI 経由の PR / CI / 課題の確認、AI エージェントの実行状況、複数リポジトリの状態を後から載せる。
外部サービスは API を直接呼ばず、既存の CLI（`gh` など）経由で使う。利用者は当面は開発者本人。

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
- モジュール（実端末に触るのは `tui` と `main` だけ。子プロセスに触るのは `runner` だけ）
  - `src/main.rs`: 引数の解釈 → 設定の読み込み → `tui::run`。エラーの表示、終了コード（0 = 正常、1 = 実行時エラー、2 = 引数の誤り）。`#![forbid(unsafe_code)]`
  - `src/cli.rs`: 引数の解釈（手書き。`Command`）、バージョンと使い方の文字列
  - `src/config.rs`: `runs.toml` の探索（カレントから親へ）・読み込み・検証（`Config` / `CommandSpec`）。無い・壊れていれば TUI を起動せず終了コード 1
  - `src/app.rs`: 状態 `App`（コマンドごとの状態・出力・選択・スクロール・開始 / 終了時刻）、`Action`、キーバインド（`action_for`）、
    更新（`App::apply` / `on_runner_event` → `Vec<Effect>`、`replace_config`）。プロセスにもファイルにも触らず、`Effect::Start / Stop / Reload` で `tui` に頼む。
    現在時刻は `set_now` で外から受け取る（中で `Instant::now()` を呼ばない。テストで時間を進めるため）
  - `src/output.rs`: 出力行の保持（上限 10,000 行）、文字コード（`decode`: UTF-8 で読めない行だけ設定の `encoding` で読み直す。Windows の既定は Shift_JIS）、無害化（`sanitize`: ESC シーケンス・制御文字の除去）
  - `src/runner.rs`: 子プロセスの起動（シェル経由）・出力の読み取りスレッド・停止（Unix: プロセスグループへ `kill`、Windows: `taskkill /T /F`）・終了時の全停止。
    通知はチャネル。宛先は実行ごとの `RunId`（`App` が採番。コマンドの添字ではないので、再読み込みや再実行でずれない）
  - `src/timefmt.rs`: 経過時間の短い表記（`12s` / `1m 12s` / `3m ago`。幅 7）
  - `src/ui.rs`: 描画（`draw(frame, app)`）と区画（`layout(area, app)`。`tui` が出力欄の高さを `App` に渡すのにも使う）。`TestBackend` に描ける
  - `src/tui.rs`: 端末ガード（`TerminalGuard`）とイベントループ（`run`: `event::poll(50 ms)` + チャネルの `try_recv`。時間の表示があるときは 1 秒ごとに描き直す）。
    `Effect` の実行（プロセスは `runner`、`runs.toml` の再読み込みは `config::load_file`）。
    SIGTERM / SIGHUP（Unix）と Ctrl+Break（Windows）は `ctrlc` のハンドラがフラグを立てるだけで、主スレッドが全停止と復元をしてから終了コード 1 で終わる
    （Windows でタブを閉じる操作は間に合わない。既知の制限）
- 設定ファイルは `runs.toml`（`[[command]]` の `name` / `command` / `cwd`、トップレベルの `shell` / `encoding`）。書き方は `runs --help`。リポジトリ直下のものは `runs` 自身の開発用
- キー: `↑↓` / `jk` 選択、`Enter` 実行（実行中なら停止して再実行）、`s` 停止、`r` 再読み込み、`PageUp` / `PageDown` / `End` スクロール、`q` / Ctrl+C 終了
- テストは実装と別ファイル: `src/<モジュール>/tests.rs`（層 1・2）、`tests/cli.rs`（層 3）。`tui` と `main` は層 3・4 で確認する。
  テスト専用のゲッターは `#[cfg(test)]` を付ける（本体で使われないと dead_code で clippy に落ちる）
- 画面と CLI のメッセージは英語（ASCII）。出力は `writeln!` を使い、`println!` / `eprintln!` は使わない（閉じたパイプへ書くと panic する）
- 依存: `ratatui` 0.30（`default-features = false`、feature は `crossterm` / `layout-cache` / `underline-color`）、`anyhow` 1、`serde` 1（derive）、`toml` 1.1（`default-features = false`）、`encoding_rs` 0.8、`ctrlc` 3.5（`termination`）。
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
- `cmd` に渡す文字列を std の `arg` で渡す（MSVC 流の `\"` エスケープを `cmd` は解釈しない）。`raw_arg` で全体を `"` に包み、`/S /C` と組み合わせる（`runner::push_command_arg`）
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
- [ ] 配布方法（cargo install / バイナリ配布。最初のリリース前に決める。`encoding_rs` の WHATWG データが BSD-3-Clause なので、バイナリを配るときは著作権表示を同梱する）
