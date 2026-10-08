# runs の Claude Code ハーネス

既存の開発フロー用プラグイン（[The AI-Native SDLC Playbook](https://claude.com/blog/the-ai-native-sdlc-playbook) に基づく）を、
この Rust ターミナルアプリ向けに作り直したものです。プラグインではなくプロジェクトの `.claude/` に直接置いているため、
`/sdlc-setup` のような導入手順は不要です（clone して Claude Code を起動すれば有効になります）。

各段階の成果物（`intent.md` → `spec.md` → `plan.md` → 差分・テスト → PR・レビュー所見）を `.steering/<作業ディレクトリ>/` にコミットし、次の段階がそれを読みます。
人は各成果物と PR の承認を担い、機械的な確認（整形・静的解析・テスト・禁止操作）はフックと検証スクリプトが担います。

## 前提

| ツール | 用途 |
|---|---|
| Rust stable（rustfmt・clippy 込み） | ビルド・検証 |
| Node.js | フックの実行 |
| Git Bash（Windows） | `verify.sh` の実行 |
| GitHub CLI（`gh`） | issue の参照・PR 作成（`winget install GitHub.cli` → `gh auth login`） |
| cargo-deny（任意。CI では必須） | `verify.sh --all` での依存のライセンス・脆弱性確認（`cargo install --locked cargo-deny`） |

元のプラグインを使っている環境では、`.claude/settings.local.json`（gitignore 済み）の `enabledPlugins` でそのプラグインを `false` にしてください
（フックの二重実行と、別プロジェクト前提のスキル・警告を避けるため。プラグイン ID は公開リポジトリに置かないので追跡ファイルには書いていません）。

## 元のプラグインからの主な変更

| 項目 | 元のプラグイン | runs |
|---|---|---|
| 配布 | プラグイン + `/sdlc-setup` | プロジェクトの `.claude/` に直接配置 |
| 課題・リモート | Backlog / Backlog Git 固定 | GitHub Issues / GitHub（`gh` CLI） |
| 検証 | gofmt / go vet / go test（ビルドタグ別）/ test-catalog | cargo fmt / clippy `-D warnings` / test（`--all` で `--locked`・doc・deny） |
| 編集後の整形 | gofmt | rustfmt（Cargo.toml の edition を渡す） |
| 保護ブランチ | master / release / development | main / master |
| コマンドガード | go mod tidy・本番 AWS ゲート | `cargo publish` 等の拒否、引数なし `cargo run`（対話 TUI）の確認 |
| 編集ガード | dist・go.sum・go.mod の replace | `target/`・`Cargo.lock` |
| 方針スキル | security（API・認証・ログ） | rust-safety（panic・端末復元・エスケープ注入・座標・unsafe・依存） |
| E2E | e2e-test（Playwright） | tui-test（状態・描画スナップショット・CLI・PTY 実機確認） |
| /fix-bug | テストファイルをロック | 同じ。ただしロックのためテストを実装と別ファイル（`src/foo/tests.rs` / `tests/`）に置く |

## 使い方（課題 1 件を進める手順）

1. 作業ブランチを切る: `git switch -c feat/<issue番号>-<slug>`（issue が無ければ `feat/<slug>`）
2. `/intent <#issue番号 | 要望>` → intent.md を承認
3. `/spec` → 懸念点を判断して承認（小さな修正なら省略可）
4. `/plan` → プランモードで計画を詰めて承認。**承認までコードは書かない**
5. 実装・検証。`bash .claude/scripts/verify.sh` を繰り返す。検証せずに完了報告しようとすると Stop フックが一度差し止める。
   最後に verifier エージェントが plan.md の証明項目を別コンテキストで実行する
6. 不具合は `/intent` の後に `/fix-bug`（再現テスト → 失敗確認 → ロック → 実装修正 → 人が `! node .claude/scripts/test-lock.mjs off` で解除）
7. `/pr` → `/pr-feedback`。承認とマージは人が行う

同じ指摘を 2 回受けたら CLAUDE.md の「Things Claude gets wrong」に追記します（reviewer と `/pr-feedback` が追記案を出します）。

## 構成

| パス | 内容 |
|---|---|
| `CLAUDE.md`（ルート） | コマンド・開発フロー・検証・規約・Claude がよく間違えること・未確定事項 |
| `.github/workflows/ci.yml`（ルート） | CI。3 OS の stable で `verify.sh --all`、Linux の Rust 1.88（MSRV）で `cargo check` / `cargo test` |
| `REVIEW.md`（ルート） | レビュー方針（Bugs / Security / Compliance の 3 パス、Important の定義、Nit 上限） |
| `settings.json` | permissions・hooks |
| `settings.local.json`（gitignore） | 個人環境の設定（元のプラグインの無効化など） |
| `scripts/verify.sh` | 検証コマンド（成功時のみ `VERIFY OK` と `state/verified-at` の更新） |
| `scripts/test-lock.mjs` | テストロックの操作 |
| `scripts/pty-check.sh` | 疑似端末（Linux / WSL の `script` コマンド）で起動し、キー入力・終了コード・終了後の端末モードを表示する |
| `hooks/` | 下記のフック |
| `agents/` | verifier / reviewer / impact-analyzer |
| `skills/` | intent / spec / plan / fix-bug / tui-test / rust-safety / pr / pr-feedback |

## Hooks

| フック | 内容 |
|---|---|
| guard-bash（PreToolUse） | force push・保護ブランチへの push・`--no-verify`・テストロック解除・`cargo publish/yank/owner` を拒否。push・破壊的 git 操作・引数なし `cargo run` は確認 |
| guard-edit（PreToolUse） | `target/`・`Cargo.lock` の編集、ロック中テストの編集、シークレットの書き込みを拒否。`.claude/hooks/`・`.claude/scripts/`・`.claude/settings.json`・`REVIEW.md` の変更は確認 |
| post-edit（PostToolUse） | `.rs` に rustfmt を適用し、構文エラーを即時フィードバック。`.rs`・`Cargo.toml` の編集時刻を記録 |
| stop-verify（Stop） | コード編集後に verify.sh が成功していなければ、完了を 1 回だけ差し止める |
| session-start（SessionStart） | ブランチ・作業ディレクトリ・cargo / Cargo.toml / git の有無をコンテキストに入れる |

フックは文字列の一致で判定するため、説明文に禁止コマンドの文字列を含む bash コマンド（heredoc など）も止まることがあります。
その場合はファイルの作成に Write / Edit ツールを使ってください。
