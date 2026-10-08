# Intent: CI を導入する（GitHub Actions: 3 OS マトリクス + MSRV 1.88 + cargo-deny）

- 課題: #13 https://github.com/dds-nakamura/runs/issues/13
- 作成者: nakamura_kouji / 状態: approved
- 対象: リポジトリの CI 設定（`.github/workflows/`）、`deny.toml`、`.claude/scripts/verify.sh`、CLAUDE.md の未確定事項。アプリ本体のコード（`src/`）は変更しない

## 課題（Problem）

- 検証は開発者の手元の `bash .claude/scripts/verify.sh --all` だけで、PR のマージ前に自動で走る確認が無い。
  一度、verify が落ちるコミットを push してしまったことがある（d960151。次のコミットで修正）
- 手元の確認は Windows 11 と WSL（Ubuntu 24.04）に限られ、次が未検証のまま残っている（CLAUDE.md「未確定事項」）
  - macOS でのビルドとテスト（確認手段が無い。一次対象 OS なのに一度も動かしていない）
  - MSRV 1.88 での実ビルド（手元の stable は 1.95。`rust-version = "1.88"` が本当に守れているか分からない）
  - CI の 3 OS マトリクスは未導入
- `cargo deny`（依存のライセンス・脆弱性）は `verify.sh --all` で「未導入のため省略」の WARN が出るだけで、実際には一度も確認していない。
  バイナリ配布時に `encoding_rs` の BSD-3-Clause 表示が要ることは分かっているが、依存全体のライセンスの棚卸しはしていない

## 目指す結果（Proposed outcome）

1. PR の作成・更新と main への push で GitHub Actions が動き、次がすべて成功する
   - Windows / Linux / macOS の stable で `verify.sh --all` 相当: `--locked` の `cargo fmt --check` / `cargo clippy --workspace --all-targets --all-features -- -D warnings` / `cargo test --workspace --all-features` / `cargo doc`（`RUSTDOCFLAGS=-D warnings`）
   - Rust 1.88（MSRV）でのビルドとテスト（OS は 1 つでよい）
   - `cargo deny check`（ライセンス・advisory・重複やソースの確認）
2. `deny.toml` がリポジトリにあり、手元の `verify.sh --all` でも cargo-deny があれば WARN 無しで `VERIFY OK` になる
3. CI の通過が main へのマージの必須条件になっている（GitHub のブランチ保護。設定は利用者が手動で行い、手順は spec に書く）
4. CLAUDE.md の未確定事項「macOS の確認手段」「MSRV 1.88 の実ビルド」「CI の 3 OS マトリクス」が [x] になり、CI の使い方（どこで結果を見るか、落ちたときに手元で再現する方法）が Commands に 1〜2 行で書かれている

判定: #13 の PR 自身の CI が全ジョブ緑で、その後の PR でも同じワークフローが走ること

## 影響する利用者・環境

- 利用者: 開発者本人（PR を出す人 = マージする人）。アプリの利用者には影響しない
- 実行環境: GitHub Actions のホステッドランナー（`windows-latest` / `ubuntu-latest` / `macos-latest`）。private リポジトリなので Actions の無料枠（分数）を消費する。macOS は Linux の 10 倍の係数で分数を消費する
- 手元: Windows 11（stable 1.95）と WSL。手元のツールチェーンは変えない

## 制約

- アプリの挙動・CLI 引数・設定ファイル・キーバインドは変えない。`src/` の変更はしない
  （MSRV 1.88 で依存がビルドできない場合に `Cargo.lock` の依存を固定する、または MSRV を上げる判断が要る可能性はある。その場合は spec で明示する）
- 検証の内容は `verify.sh --all` と同じにし、CI だけで通る・手元だけで通る状態を作らない（手元で再現できることが条件）
- `Cargo.lock` は `--locked` で使う。CI が勝手に依存を更新しない
- `cargo install` でのツール導入は CI の時間を伸ばすので、キャッシュまたは事前ビルド済みバイナリを使う（手段は spec で決める）
- 対話 TUI は CI では起動しない（TTY が無い）。`pty-check.sh` は範囲外
- 外部の Actions（`actions/checkout` など）はメジャーバージョンまたはコミット SHA で固定する
- 他プロジェクトの CI 規約は適用しない（単独プロジェクト）
- （2026-10-08 追記）ブランチ保護は GitHub Free の private リポジトリでは使えないと分かった（spec C1）。リポジトリを **public にする** 前提で進める。公開の操作とルールセットの設定はユーザーが手動で行う。公開前の確認事項（author メール・社内固有名・LICENSE）は spec C5

## 範囲外

- `pty-check.sh` の CI 実行（別課題。flaky 対策の設計が要る）
- リリース（バイナリ配布）のワークフロー。配布方法は未確定事項のまま
- 自動マージ・自動リリースノート

## 未解決の問い

- [ ] MSRV 1.88 で現在の `Cargo.lock` の依存がビルドできるか（spec の前に手元の WSL で `rustup toolchain install 1.88` して確認できる。できなければ依存の固定か MSRV の引き上げを spec で判断する）
- [ ] cargo-deny のライセンス許可リストに何を入れるか（MIT / Apache-2.0 / BSD-3-Clause / Unicode-3.0 など。依存ツリーの棚卸しは spec で行う）
- [ ] MSRV のジョブを Linux だけにするか 3 OS にするか（分数の節約。spec で決める。既定案は Linux のみ）
- [x] ブランチ保護を有効にすると、管理者（本人）にも適用するかどうか → spec の「ブランチ保護の手順」で Bypass list を空（管理者にも適用）とした
- [x] public 化の前の判断（spec C5）→ 2026-10-08 にユーザーが決定: 今後のコミットだけ noreply メールにする（履歴は書き換えない）、CLAUDE.md の社内固有名は一般的な表現に書き換える、LICENSE は `MIT OR Apache-2.0`、`.steering` の作成者欄はそのまま
