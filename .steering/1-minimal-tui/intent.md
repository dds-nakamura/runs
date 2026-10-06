# Intent: 起動して q / Ctrl+C で終了する最小 TUI を作る（端末復元ガード込み）

- 課題: #1 https://github.com/dds-nakamura/runs/issues/1
- 作成者: nakamura_kouji / 状態: approved
- 対象: `runs` バイナリ全体（起動・画面表示・終了・端末の初期化と復元・CLI 引数）

## 課題（Problem）

`cargo init` と依存（ratatui 0.30 / anyhow）の追加までは済んでいるが、TUI のコードが無い。
`src/main.rs` はバージョンを表示して終了するだけで、次の 3 つが決まっていないため、機能を載せ始められない。

- モジュール構成（状態・更新・描画をどう分けるか）
- 端末の初期化と復元をどこで行うか
- テストをどこに置き、どの層で何を証明するか

## 目指す結果（Proposed outcome）

アプリの機能は載せず、以後の機能が乗る土台だけを作る。完了は次で判定する。

- 引数なしで起動すると、プレースホルダーの画面（アプリ名・バージョン・終了キーの案内）が出る
- `q` と Ctrl+C で終了し、終了後に端末が元に戻る（raw mode・alternate screen・カーソル）
- panic・エラーで終わるときも端末が元に戻り、メッセージが読める
- Windows でキーの Release イベントによる二重入力が起きない
- 極小の端末サイズ（幅 1・高さ 1 など）とリサイズで panic しない
- 端末でない場所（パイプ・リダイレクト）で起動すると、メッセージを stderr に出して非ゼロで終了する。端末の状態は変えない
- `--version` はバージョンを、`--help` は使い方を stdout に出して 0 で終了する（端末の初期化はしない）
- 状態更新と描画を端末なしでテストでき、`bash .claude/scripts/verify.sh` が通る
- CLAUDE.md の Architecture と `/tui-test` に、決まったモジュール構成とテスト用ヘルパーが書かれている

## 影響する利用者・環境

- 利用者: 現時点では開発者本人（アプリの機能が無いため）
- 一次対象: Windows 11（Windows Terminal）/ Linux / macOS
- この課題で実機確認する範囲: Windows Terminal と WSL の Ubuntu 24.04
- macOS は確認手段が無いので未検証とし、報告に明記する

## 制約

- Esc は終了キーにしない（後でモーダルなどを作るときに意味が競合するため）
- 非テストコードで `unwrap()` / `expect()` / `panic!` を使わない。`unsafe` は使わない（`rust-safety`）
- 依存クレートの追加は spec で合意してから行う
- 互換性の制約は無い（既存の CLI 引数・設定ファイル・キーバインドが無い）
- 期限は無い

## 未解決の問い

- [ ] `--version` / `--help` の引数パーサを手書きにするか、クレート（clap など）を入れるか（spec で案を出し、ユーザーが決める）
- [x] WSL の Ubuntu の Rust ツールチェーン → 2026-10-05 に `rustup default stable` で設定済み（rustc 1.99.0。Windows 側は 1.95.0。C コンパイラもあり）
- [ ] macOS の確認手段（実機か CI か）。CLAUDE.md の未確定事項として残っている（ユーザーが決める）
