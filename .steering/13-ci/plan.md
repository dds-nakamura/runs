# Plan: CI を導入する（GitHub Actions: 3 OS マトリクス + MSRV 1.88 + cargo-deny）（spec.md 2026-10-08 より）

課題 #13。ブランチ `chore/13-ci`。2026-10-08 にプランモードで承認。

## Context

検証は手元の `verify.sh --all` だけで、PR マージ前の自動確認が無い。macOS / MSRV 1.88 / cargo-deny は一度も動かしていない。
GitHub Actions で 3 OS の stable + Linux の 1.88 + cargo-deny を回し、CLAUDE.md の未確定事項を埋める。
spec の判断: ブランチ保護のためにリポジトリを **public** にする（Free の private では不可）。公開前に LICENSE（`MIT OR Apache-2.0`）を置き、
CLAUDE.md / `.claude/README.md` の社内固有名を一般的な表現に直す。`src/` は変更しない。macOS でテストが落ちても #13 では直さず別課題。

## 変更するファイル

- `.github/workflows/ci.yml`（新規）: 下記
- `deny.toml`（新規）: 下記
- `.claude/scripts/verify.sh`（変更、Edit ツール。guard-edit の確認あり）: `cargo deny` 節に `elif [ -n "${CI:-}" ]; then fail "cargo deny（CI では必須…）"` を足す（spec の断片そのまま）
- `Cargo.toml`（変更）: `[package]` に `license = "MIT OR Apache-2.0"` を 1 行（`publish = false` の下）。依存・feature は変えない
- `LICENSE-MIT`（新規）: MIT 全文。`Copyright (c) 2026 dds-nakamura`
- `LICENSE-APACHE`（新規）: Apache-2.0 全文。`curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt` で取得し、そのまま置く（手打ちしない）
- `CLAUDE.md`（変更）:
  - L8: 旧表記（元のプロジェクト名・課題管理のキー・元のプラグイン名を含む「…とは独立した単独プロジェクト。…は適用しない。」）→「他のプロジェクトの規約・課題管理・スキル・エージェントは適用しない単独プロジェクト。」
  - L9: 「（private。課題は…」→「（public。課題は…」
  - L15 の CI 相当の行: 「+ `cargo deny check`（導入時））」→「+ `cargo deny check`）。CI（GitHub Actions）はこれを 3 OS で実行し、CI では cargo-deny が無いと失敗する」
  - L15 の直後に 1 行追加: 「CI の結果: PR の Checks または `gh pr checks`。落ちたジョブのログは `gh run view <run-id> --log-failed`。手元での再現は `verify.sh --all`（cargo-deny は `cargo install --locked cargo-deny`）、MSRV は `rustup toolchain install 1.88 && cargo +1.88 test --locked --workspace --all-features`」
  - Conventions の「main / master へ直接 push しない」→ 末尾に「。main はルールセットで保護し、CI の 4 ジョブ（verify × 3 OS、msrv）を必須ステータスチェックにしている」
  - 未確定事項 L95: 「[ ] macOS の確認手段（未定）。CI の 3 OS マトリクスは未導入」→「[x] macOS の確認手段 → GitHub Actions の `macos-latest`（`cargo test` のみ。実機・TUI の目視は無し）。CI は 3 OS（stable）+ Linux の MSRV 1.88 + cargo-deny（#13）」
  - L96: 「（`rust-version`。1.88 のツールチェーンでの実ビルドは未検証）」→「（`rust-version`。CI の `msrv` ジョブで 1.88 のビルドとテストを確認）」
  - L98: 「（private）」→「（public。#13 で公開）」
- `.claude/README.md`（変更）:
  - L3-4: 「<元のプラグイン名> プラグイン（…）を、<元のプロジェクト名> から独立した Rust ターミナルアプリ向けに作り直したものです。」→「既存の開発フロー用プラグイン（[The AI-Native SDLC Playbook](…) に基づく）を、この Rust ターミナルアプリ向けに作り直したものです。」（reviewer の Nit で「社内の」→「既存の」）
  - L18: 「cargo-deny（任意）」→「cargo-deny（任意。CI では必須）」
  - L20: 「<元のプラグイン 2 件> は…（フックの二重実行と <元のプロジェクト名> 前提のスキル・警告を避けるため）」→「元のプラグインを使っている環境では、`.claude/settings.local.json`（gitignore 済み）の `enabledPlugins` でそのプラグインを `false` にしてください（…）」（当初は settings.json を指す文だったが、プラグイン ID を local へ移したのに合わせて変更）
  - L22 見出し「<元のプラグイン名> からの主な変更」→「元のプラグインからの主な変更」、L24 の列名 `<元のプラグイン名>` → `元のプラグイン`
  - L27: 「Backlog `<プロジェクトキー>` / Backlog Git 固定」→「Backlog / Backlog Git 固定」
  - L33: 「<元のプラグイン名>-security（API・認証・ログ）」→「security（API・認証・ログ）」
  - 「構成」表の末尾付近に行を追加: `| .github/workflows/ci.yml（ルート） | CI。3 OS の stable で verify.sh --all、Linux の 1.88 で check / test |`
  - `.claude/settings.json` の `enabledPlugins`（元のプラグイン ID 2 件）は当初「機能に必要なので変えない」としたが、reviewer 指摘を受けてユーザーが「gitignore 済みの `.claude/settings.local.json` へ移す」と判断。settings.json からはブロックごと削除した（permissions / hooks は変更なし）
- `.claude/settings.json`（変更。reviewer 対応で追加）: `enabledPlugins` ブロックを削除
- `.claude/settings.local.json`（新規、gitignore。reviewer 対応で追加）: `enabledPlugins` の移設先
- `.steering/13-ci/plan.md`（新規）: この計画。`.steering/13-ci/spec.md`: 実装中にずれたら該当節を直す

### `.github/workflows/ci.yml`

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]
permissions:
  contents: read
concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  # PR への連続 push は古い実行を止める。main への push は実行中のものを止めない（reviewer 対応）
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
env:
  CARGO_TERM_COLOR: never
  CARGO_INCREMENTAL: 0
  RUST_BACKTRACE: 1
defaults:
  run:
    shell: bash
jobs:
  verify:
    name: verify (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-deny@0.20   # マイナー版で固定（reviewer 対応）
      - run: bash .claude/scripts/verify.sh --all
      - run: git diff --exit-code -- Cargo.lock
  msrv:
    name: msrv
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@1.88
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --locked --workspace --all-targets --all-features
      - run: cargo test --locked --workspace --all-features
      - run: git diff --exit-code -- Cargo.lock
```

ジョブ名は `verify (ubuntu-latest)` 等に固定し、ルールセットの必須チェック名と一致させる。

### `deny.toml`

```toml
# 依存のライセンス・脆弱性・重複・取得元の確認（cargo deny check）。verify.sh --all と CI で実行する。
# 許可リストを広げるときは spec で合意する（rust-safety 8 章）。
[graph]
all-features = true

[advisories]
yanked = "deny"
ignore = []

[licenses]
allow = [
  "MIT",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-3-Clause",
  "Unicode-3.0",
  "Zlib",
]
confidence-threshold = 0.8

[bans]
multiple-versions = "warn"
# `foo = "*"` のような版指定を禁止する。path 依存を足すときは allow-wildcard-paths = true を併用する
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

手元で `cargo deny check` を回し、必要になった ID（`Unicode-DFS-2016` / `WTFPL` の見込み）だけ `allow` に足す。古い書式の警告（`version = 2` 等）が出たら従う。

## 作業順

1. 手元に cargo-deny を導入: `cargo install --locked cargo-deny`（ユーザー承認済み。約 5 分）
2. `deny.toml` と `Cargo.toml` の `license` を書き、`cargo deny check` を回して `allow` を確定する（ライセンス式のパース結果も見る。`MIT/Apache-2.0` 形式が通るか）
3. `verify.sh` の `cargo deny` 節を Edit で変更 → `bash .claude/scripts/verify.sh --all` が WARN 無しで `VERIFY OK`、`CI=1 PATH=<cargo-deny 無し>` で実行すると `VERIFY FAILED` になることを確認（PATH 操作が難しければ一時的に `cargo-deny` を rename せず、`CI=1` + 存在しないコマンド名に差し替えたコピーで確認）
4. `LICENSE-MIT` / `LICENSE-APACHE` を置く
5. `ci.yml` を書く。`python -I -c "import yaml"` は無いかもしれないので、文法は push 後の Actions で確認（最初の 1 回は失敗しても即直せる）
6. CLAUDE.md と `.claude/README.md` を直す
7. コミット（論点ごと: deny.toml + Cargo.toml + verify.sh ／ LICENSE ／ ci.yml ／ 文書）。各コミット前に `verify.sh` を通す
8. push → `gh pr create`（`Closes #13`）→ `gh pr checks --watch` で 4 ジョブを見る。落ちたら `gh run view --log-failed` で読み、直して push（CI の往復）。
   macOS で `src/` 起因のテスト失敗が出たら: 事実を plan.md「実装中に分かったこと」と PR 本文に書き、`fix/` の issue を起票し、macOS だけ `continue-on-error: true` を一時的に付ける（issue 番号をコメントで残す）
9. verifier サブエージェント → `/pr`（reviewer）。
10. **ユーザーの手順（CI が緑になった後、マージ前）**: Settings → Danger Zone → Make public → Settings → Rules → Rulesets で spec「ブランチ保護の手順」のとおり設定（必須チェック: `verify (ubuntu-latest)` / `verify (windows-latest)` / `verify (macos-latest)` / `msrv`）→ PR をマージ。
    私が `gh api repos/dds-nakamura/runs/rulesets` で設定を確認し、記憶ファイルの「private」を更新する

## リスク

- **MSRV 1.88 で依存がビルドできない**: 宣言上は 1.88 以下だが未宣言のクレートが多い。落ちたら、原因クレートを特定して spec の「懸念点」に追記し、`cargo update -p <crate> --precise <版>` で下げるか MSRV を上げるかをユーザーに聞く（`Cargo.lock` の手編集はしない）
- **macOS のテスト失敗**（`kill -s TERM -- -<pid>`、`pgrep -f`、時間アサーション）: 別課題。手順 8 のとおり
- **Windows ランナーの Git Bash**: `defaults.run.shell: bash` で `verify.sh` を動かす。`cargo` が PATH に無い場合は `dtolnay/rust-toolchain` が設定するので問題ないはず。`taiki-e/install-action` の Windows 対応は公式に明記あり
- **cargo-deny の設定書式**: バージョンにより非推奨キーのエラーがある。手元（最新版）と CI（install-action の最新版）は同じ版になる見込み
- **ライセンス式のパース**: `MIT/Apache-2.0`（スラッシュ）、`Apache-2.0 / MIT` を cargo-deny が lax モードで読むか。読めなければ `[licenses.clarify]` で該当クレートに式を与える（手順 2 で判明）
- **CI の分数**: public 化までは private の分数を消費する。CI の往復は多くて 5 回程度の見込み（約 250 分）。`concurrency` のキャンセルあり
- **`git diff --exit-code Cargo.lock`**: `--locked` なので変わらないはず。変わったら `--locked` 漏れか、ランナーの cargo がロックファイルの版を書き換えたかを見る
- **文書の書き換えでハーネスが壊れる**: `.claude/README.md` の変更は説明文のみ。`settings.json` は触らない
- **公開のタイミング**: 公開前に PR 内容（LICENSE、固有名の除去）をユーザーが確認する。公開操作は私は行わない

## 証明（Proof）

- `bash .claude/scripts/verify.sh --all` が手元（Windows、cargo-deny 導入後）で WARN 無しの `VERIFY OK`
- `CI=1` かつ cargo-deny が見つからない状態の `verify.sh --all` が `VERIFY FAILED`（手順 3）
- `cargo deny check` 単体が成功。`deny.toml` の `allow` に無いライセンスのクレートが無い
- `cargo metadata --locked --format-version 1 | python -I -c "..."` で `runs` の `license` が `MIT OR Apache-2.0`
- PR の CI: `gh pr checks` で `verify (ubuntu-latest)` / `verify (windows-latest)` / `verify (macos-latest)` / `msrv` が pass（macOS が別課題行きの場合は、その事実と issue 番号を記録）
- `gh run view <id> --log` の verify ジョブに `VERIFY OK` が出ている（3 OS）
- `git diff --exit-code Cargo.lock` のステップが pass（3 OS + msrv）
- 追跡ファイル全体（`.steering/` を含む）に元のプロジェクト名・元のプラグイン名・課題管理のプロジェクトキー・会社のメールドメインが残っていない（`git grep -i` で 0 件。`.claude/settings.json` の扱いは C5 の追加判断に従う。reviewer 指摘で範囲を CLAUDE.md / README からリポジトリ全体に広げた）
- 公開後（ユーザー操作後）: `gh api repos/dds-nakamura/runs/rulesets` がルールセットを返す。`gh api repos/dds-nakamura/runs --jq .visibility` が `public`

## 実装中に分かったこと

- cargo-deny 0.20.2（手元、2026-10-08）で `deny.toml` は書いたとおりで通った。`Unicode-DFS-2016` / `WTFPL` は `runs` の依存グラフに現れず、許可リストへの追加は不要。古い書式の警告も無し
- `cargo deny check` の警告は `hashbrown`（2 版）と `syn`（2 版）の重複のみ（`multiple-versions = "warn"` のとおり）。advisories / bans / licenses / sources すべて ok
- `CI=1` で cargo-deny が無い状態の `verify.sh --all` は `NG: cargo deny（CI では必須…）` → `VERIFY FAILED` になった（手順 3 の確認。cargo-deny のインストール中に実施）
- `ci.yml` の YAML 文法は手元に pyyaml が無く確認できなかった。push 後の Actions で確認した（初回で 4 ジョブとも成功）
- PR #14 の初回 CI（run 37737000249、2026-10-08）: `verify (ubuntu-latest)` 1m09s / `verify (windows-latest)` 2m33s / `verify (macos-latest)` 52s / `msrv` 1m06s、すべて pass。3 OS のログに `VERIFY OK`・`advisories ok, bans ok, licenses ok, sources ok`・テスト 144 + 9 件。stable は rustc 1.99.0、cargo-deny は 0.20.2（手元と同じ版）。`msrv` は rustc 1.88.0 で check と test が通過（MSRV 1.88 の実ビルドが初めて確認できた）
- リスク「macOS のテスト失敗」は顕在化しなかった。`kill -s TERM -- -<pid>` / `pgrep -f` を使う孫プロセス停止のテストも macOS で通った。別課題の起票は不要
- `Cargo.lock` の確認ステップは 4 ジョブとも pass（`--locked` の漏れ無し）
- reviewer 1 回目（Important 1 / Minor 4 / Nit 5）への対応:
  - Important: spec / plan に書き写していた固有名と会社ドメインを伏せ字にし、spec C5 の事実欄を訂正。`settings.json` のプラグイン ID は `settings.local.json` へ移動（ユーザー判断）。CLAUDE.md の Conventions に 1 行追加
  - Minor: spec の advisories の記述を「エラーになる」に訂正。`ci.yml` で `cargo-deny@0.20` に固定、`concurrency` のグループに `github.workflow` を足し `cancel-in-progress` を PR のときだけに。LICENSE の著作権者は個人名義のまま（ユーザー判断）
  - Nit: `verify.sh` の先頭コメントと `CI` 判定のコメント、`persist-credentials: false`、`wildcards = "deny"`（手元の `cargo deny check` で ok）、README の「社内の」→「既存の」
  - 変更ファイルが plan の表から増えた: `.claude/settings.json`（`enabledPlugins` の削除）、`.claude/settings.local.json`（新規、gitignore）
- reviewer 2 回目（Important 1 / Minor 1 / Nit 2）への対応:
  - Important: 1 回目の対応で ci.yml / deny.toml / settings.json を変えたのに、spec / plan の設計節（ci.yml と deny.toml の断片、ファイル表、settings.json の記述）が旧値のままだった。実物に合わせて直し、旧値を grep して残りが無いことを確認。CLAUDE.md の「Things Claude gets wrong」に「旧値を spec / plan に grep する」を追記（同種の失敗 2 回目）
  - Minor: 途中コミットに固有名が残る → スカッシュマージで main に入れない（ユーザー判断。spec C5）
  - Nit: ci.yml の `concurrency` のコメントを正確に（待機中は 1 件まで）、README に `settings.local.json` の書き方の例を 1 行

## 並行可能な作業

無し（CI の往復が直列）。

## 捨てた案

- `EmbarkStudios/cargo-deny-action`: Docker 実行で Linux 限定。「verify.sh を 3 OS で同じに流す」方針に合わない
- `cargo install cargo-deny` を CI で毎回: ソースビルドで数分かかる。`taiki-e/install-action` のビルド済みバイナリを採用
- MSRV ジョブで clippy / fmt も実行: 1.88 の lint セットが stable と違い `-D warnings` が安定しない。`check` / `test` のみ
- Actions の SHA 固定: Dependabot を入れないので更新が追えない。メジャーバージョンのタグで固定
- 手元の WSL で 1.88 を事前確認: CI の最初の 1 回で分かるので省く
- 履歴の author メール書き換え（`git filter-repo` + force push）: ユーザーが「今後のコミットだけ noreply」を選択
- `/pr` スキルに `gh pr checks --watch` を足す: public 化でルールセットが使えるので不要
