# Intent: Windows で CP932 で出力するコマンドの出力が文字化けする

- 課題: #8 https://github.com/dds-nakamura/runs/issues/8
- 作成者: nakamura_kouji / 状態: approved
- 対象: 子プロセスの出力の取り込み（`output::OutputBuffer::push_raw`）、設定ファイル

## 課題（Problem）

日本語環境の Windows では、`ping`・`dir`・`ipconfig`・`cmd` の内蔵コマンドなど標準コマンドの出力がコードページ CP932（Shift_JIS）で届く。
`runs` は子プロセスの出力を UTF-8 として読む（`String::from_utf8_lossy`）ので、日本語部分が `�` に化ける。
#7 の実機確認（2026-10-07）で `ping -n 4 127.0.0.1` の「からの応答」が化けた。

#5 の spec の懸念点 4 で「UTF-8 固定。CP932 の文字化けは許容」と決めていたが、`ping` のように日常的に使うコマンドで起きるので直す。
Rust / Go / Node / Python / cargo の出力は UTF-8 なので、これらは化けていない。

## 目指す結果（Proposed outcome）

- 日本語環境の Windows で `ping -n 4 127.0.0.1` の出力が化けずに表示される
- UTF-8 で出力するコマンド（`cargo test` など）の表示は変わらない
- UTF-8 と CP932 が行ごとに混ざる出力（`cmd` の `&&` で UTF-8 のツールと `dir` をつなぐ）でも、それぞれの行が読める
- 日本語以外の環境（西欧の Windows は CP1252 など）でも、設定で文字コードを指定すれば読める
- Unix（Linux / macOS）の挙動は変わらない（UTF-8 のみ。不正なバイト列は U+FFFD）

## 影響する利用者・環境

- 利用者: 開発者本人（日本語環境の Windows 11 と WSL）
- 一次対象: Windows 11 / Linux / macOS。実機確認は Windows Terminal（文字化けの再現と修正の確認）と WSL（挙動が変わらないこと）

## 制約

- `unsafe` を使わない（Windows API の `GetACP` / `GetConsoleOutputCP` は `windows-sys` と `unsafe` が要るので使わない）
- 子プロセスの出力は信頼できない入力（`rust-safety` 3 章）。デコードの前後で制御文字の除去（`sanitize`）は維持する
- 依存クレートの追加は合意してから（候補: `encoding_rs`。MIT / Apache-2.0、`unsafe` の API を使わずに済む）
- #5・#7 の挙動（停止・再読み込み・時刻など）は変えない

## 範囲外

- 子プロセスへの入力（stdin）の文字コード
- 設定ファイル自体の文字コード（TOML は UTF-8 のみ）
- 1 行の中で UTF-8 と CP932 が混ざるケース（行単位で判定する）

## 未解決の問い

- [x] 文字コードの決め方 → (A) UTF-8 として不正な行だけ、既定の文字コード（Windows は Shift_JIS、Unix は無し）で読み直す。`runs.toml` の `encoding` で上書き可（2026-10-07 ユーザー決定）
- [x] `encoding_rs` の追加 → 追加する（2026-10-07 ユーザー決定）
