# Spec: CI を導入する（GitHub Actions: 3 OS マトリクス + MSRV 1.88 + cargo-deny）（intent.md 2026-10-08 より）

課題: #13。変更するのはリポジトリの CI 設定・依存ポリシー・検証スクリプト・文書だけで、`src/` は変更しない。

## 要件

### 機能要件（受け入れ条件）

- [ ] A1. PR を作成・更新したとき、および main に push されたとき、ワークフロー `CI` が起動する。それ以外のブランチへの push では起動しない
- [ ] A2. `verify` ジョブが `ubuntu-latest` / `windows-latest` / `macos-latest` の 3 つで動き、それぞれが stable ツールチェーンで `bash .claude/scripts/verify.sh --all` を実行して最終行 `VERIFY OK` で終わる。1 つの OS が落ちても他の OS の結果が出る（`fail-fast: false`）
- [ ] A3. `msrv` ジョブが `ubuntu-latest` の Rust 1.88.0 で `cargo check --locked --workspace --all-targets --all-features` と `cargo test --locked --workspace --all-features` を通す
- [ ] A4. CI 上の `verify.sh --all` は cargo-deny が無いと **失敗する**（手元では従来どおり WARN で省略）。判定は環境変数 `CI` の有無
- [ ] A5. `deny.toml` がリポジトリ直下にあり、`cargo deny check`（advisories / licenses / bans / sources）が通る。手元で cargo-deny を入れた `verify.sh --all` も WARN 無しで `VERIFY OK`
- [ ] A6. `Cargo.lock` が CI で書き換わらない（全コマンドが `--locked`。`git diff --exit-code Cargo.lock` で確認するステップを置く）
- [ ] A7. ワークフローの各ジョブに `timeout-minutes` があり、同じ PR への連続 push では古い実行がキャンセルされる（`concurrency` + `cancel-in-progress`。main への push では実行中のものを止めない。reviewer 指摘で条件を追加）
- [ ] A8. CLAUDE.md の Commands に CI の見方（PR の Checks、`gh pr checks`、`gh run view --log-failed`）と、落ちたときに手元で再現する方法（`verify.sh --all`。MSRV は `cargo +1.88 test --locked`）が書かれている。未確定事項の「macOS の確認手段」「MSRV 1.88 の実ビルド」「CI の 3 OS マトリクス」が [x] になっている
- [ ] A9. #13 の PR 自身で全ジョブが緑。macOS で落ちたテストがあれば、その事実と原因の見立てを PR と plan.md に記録し、修正は `fix/` の別課題として起票する（C3 の判断: 別課題。`src/` は #13 では触らない）
- [ ] A10. リポジトリが public になった後、main のブランチ保護（ルールセット）で CI の 4 ジョブ（`verify (ubuntu-latest)` / `verify (windows-latest)` / `verify (macos-latest)` / `msrv`）が必須ステータスチェックになっている（C1 の判断: public 化。公開とルールセットの設定はユーザーが手動で行い、手順は下記「ブランチ保護の手順」）。CLAUDE.md にこの運用が 1 行で書かれている
- [ ] A11. 公開前チェック（懸念点 C5）の各項目について、ユーザーの判断が intent.md / plan.md に記録されている。LICENSE を置く場合はリポジトリ直下に `LICENSE`（または `LICENSE-MIT` / `LICENSE-APACHE`）があり、`Cargo.toml` の `license` 欄と一致する

### 非機能要件

- 1 回の CI の壁時計時間は 10 分以内を目標（キャッシュが効いた状態）。ビルドキャッシュは `Swatinem/rust-cache` で OS ごとに持つ
- Actions の課金分数（GitHub Free: 月 2,000 分、Windows は 2 倍、macOS は 10 倍で計算）を意識する。見積りは懸念点 C2
- CI と手元で「同じ検証」が走ること。CI 専用の確認は MSRV ジョブと `Cargo.lock` 不変の確認だけ（どちらも手元でも再現できる）
- 外部 Actions はメジャーバージョンのタグで固定する（intent の制約。SHA 固定は Dependabot と組み合わせないと更新が追えないので今回は採らない）
- 対話 TUI は CI では起動しない（`tests/cli.rs` は TTY 無しの経路を検証しているので、CI の非 TTY で正しく動く）

## 設計

### 変更方針

- 検証の真実は `verify.sh --all` 1 つに保ち、CI はそれを 3 OS で呼ぶだけにする（CI の YAML に cargo のコマンドを並べない）。MSRV ジョブだけは、1.88 の clippy / rustfmt は stable と lint セットが違い `-D warnings` が意味を持たないので、`check` と `test` を直接書く
- cargo-deny は手元では任意、CI では必須。`verify.sh` の判定に `CI` を足すだけで両立させる（Actions は `CI=true` を常に設定する）
- 依存ツリーに手を入れない。`deny.toml` は現状の依存が通る最小の許可リストにし、将来の依存追加で広げるときは spec で合意する（`rust-safety` 8 章）

### 追加・変更するファイル

| ファイル | 変更 |
|---|---|
| `.github/workflows/ci.yml` | 新規。下記のジョブ構成 |
| `deny.toml` | 新規。下記のポリシー |
| `.claude/scripts/verify.sh` | `cargo deny` の節: 未導入のとき `CI` が設定されていれば `fail`、無ければ従来の `warn` |
| `CLAUDE.md` | Commands に CI の 2 行、未確定事項の 3 件を更新、ブランチ保護の運用（C1）と公開リポジトリであること（C5）を記載。社内固有名の扱いは C5 の判断に従う |
| `.steering/13-ci/intent.md` | C1 / C5 の判断を「制約」と「未解決の問い」に反映 |
| `LICENSE-MIT` / `LICENSE-APACHE` | 新規。MIT と Apache-2.0 の全文（著作権者は GitHub ID `dds-nakamura`、年は 2026） |
| `.claude/settings.json` | `enabledPlugins`（元のプラグイン ID 2 件）を削除（C5 の追加判断。permissions / hooks は変更なし） |
| `.claude/settings.local.json`（gitignore） | 新規。`enabledPlugins` を移す。追跡されないので clone 直後は手動で作る（手順は `.claude/README.md`） |
| `Cargo.toml` | `license = "MIT OR Apache-2.0"` を足す（メタデータのみ。依存・feature は変えない） |

### ワークフロー `ci.yml`

```yaml
name: CI
on:
  pull_request:
  push:
    branches: [main]
concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}   # main への push では実行中のものを止めない
env:
  CARGO_TERM_COLOR: never
  RUST_BACKTRACE: 1
  CARGO_INCREMENTAL: 0
defaults:
  run:
    shell: bash          # Windows でも Git Bash で verify.sh を動かす
jobs:
  verify:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    timeout-minutes: 30
    steps:
      - actions/checkout@v7（persist-credentials: false。push しないのでトークンを .git に残さない）
      - dtolnay/rust-toolchain@stable（components: rustfmt, clippy）
      - Swatinem/rust-cache@v2
      - taiki-e/install-action@v2（tool: cargo-deny@0.20。ビルド済みバイナリを取るだけなので数秒。マイナー版で固定し、上げるときは PR で）
      - run: bash .claude/scripts/verify.sh --all
      - run: git diff --exit-code Cargo.lock
  msrv:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - actions/checkout@v7（persist-credentials: false。push しないのでトークンを .git に残さない）
      - dtolnay/rust-toolchain@1.88
      - Swatinem/rust-cache@v2
      - run: cargo check --locked --workspace --all-targets --all-features
      - run: cargo test --locked --workspace --all-features
```

- `timeout-minutes: 30` は暴走時の課金を抑える上限（通常は 10 分以内）
- `taiki-e/install-action` を選ぶ理由: 3 OS で同じ手順で cargo-deny のビルド済みバイナリを入れられる。`EmbarkStudios/cargo-deny-action` は Docker 実行で Linux 限定なので、「verify.sh を 3 OS で同じに流す」方針に合わない。`cargo install cargo-deny` はソースビルドで数分かかるので不採用
- `dtolnay/rust-toolchain` の `@stable` / `@1.88` はツールチェーン名のブランチ（公式の使い方）。ツールチェーン本体は rustup が取る
- `Cargo.lock` の確認ステップは、`--locked` の付け忘れを CI で検出するための二重の安全策

### `deny.toml` のポリシー

cargo-deny は Cargo.lock 全体ではなく、`runs` の feature で解決した依存グラフを見る。許可リストは実行して必要なものだけ入れる（棚卸しで候補になった ID と、必要になる見込み）。

| 節 | 方針 |
|---|---|
| `[graph]` | `all-features = true`（verify と同じ条件）。`targets` は指定しない（3 OS の依存をすべて見る） |
| `[licenses]` | `allow` に `MIT` / `Apache-2.0` / `Apache-2.0 WITH LLVM-exception` / `BSD-3-Clause` / `Unicode-3.0` / `Zlib`。`OR` 式はいずれか 1 つが通れば可（`MIT OR Apache-2.0 OR LGPL-2.1-or-later` は MIT で通る）。`Unicode-DFS-2016`（非推奨 ID。wezterm-bidi / finl_unicode）と `WTFPL`（terminfo）はグラフに現れた場合だけ足す。`runs` 自身は `Cargo.toml` に `license = "MIT OR Apache-2.0"` を足すので許可リストで通る（C5 の判断。`private.ignore` は使わない） |
| `[advisories]` | 既定の RustSec DB。`yanked = "deny"`。cargo-deny 0.16 以降、`ignore` に無い勧告（unmaintained / unsound を含む）はすべて**エラー**になり、無関係な PR でも `verify` が落ちる（安全側）。該当が出たら `ignore` に理由付きで入れる（spec の合意事項）。（reviewer 指摘で「既定は警告」から訂正、2026-10-08） |
| `[bans]` | `multiple-versions = "warn"`（syn 1/2、nix 3 版、thiserror 1/2、getrandom 2 版が既に重複している。`deny` にすると今は通らない）。`wildcards = "deny"`（`foo = "*"` の版指定を禁止。path 依存を足すときは `allow-wildcard-paths = true` を併用。reviewer 指摘で allow から変更） |
| `[sources]` | `unknown-registry = "deny"`、`unknown-git = "deny"`。crates.io 以外から取らない |

バイナリ配布時の著作権表示（`encoding_rs` の BSD-3-Clause など）は未確定事項「配布方法」の範囲。`deny.toml` の許可リストが、そのときの表示対象の一覧にもなる。

### `verify.sh` の変更

```bash
  section "cargo deny"
  if cargo deny --version >/dev/null 2>&1; then
    cargo deny check || fail "cargo deny"
  elif [ -n "${CI:-}" ]; then
    fail "cargo deny（CI では必須。ワークフローで cargo-deny を導入する）"
  else
    warn "cargo-deny 未導入のため依存（ライセンス・脆弱性）の確認を省略（cargo install --locked cargo-deny）"
  fi
```

- `.claude/state/verified-at` への書き込みは CI の作業ディレクトリ内なので無害（`.gitignore` 済み。使い捨てランナー）。`stop-verify.mjs` は `last-edit.json` が無ければ何もしないので CI の影響は無い
- bash 3.2（macOS）で動く構文しか使っていない（`case` / `[ ]` / `$()`）。変更部分も同じ

### CLAUDE.md の変更

- Commands:
  - `CI 相当` の行を「`bash .claude/scripts/verify.sh --all`（`--locked` 付き + `cargo doc` 警告ゼロ + `cargo deny check`。CI ではこれを 3 OS で実行し、cargo-deny が無いと失敗する）」に
  - 追加: 「CI の結果は PR の Checks か `gh pr checks`。落ちたジョブのログは `gh run view --log-failed`。手元で再現するのは `verify.sh --all`、MSRV は `rustup toolchain install 1.88 && cargo +1.88 test --locked --workspace --all-features`」
- 未確定事項: 「macOS の確認手段」→ [x] GitHub Actions の `macos-latest`（実機は無し。TUI の目視は未検証のまま）。「CI の 3 OS マトリクスは未導入」→ 導入済み。「1.88 のツールチェーンでの実ビルドは未検証」→ CI の `msrv` ジョブで確認
- Conventions の「main / master へ直接 push しない」の行に「main はルールセットで保護し、CI の 4 ジョブが必須ステータスチェック（設定は GitHub の Settings。C1）」を足す
- 冒頭の「リモート: GitHub `dds-nakamura/runs`（private。…）」を public に直す（C5 の判断後）

### ブランチ保護の手順（ユーザーが手動で行う。public 化の後）

1. GitHub の Settings → General → Danger Zone → Change visibility → Make public（C5 のチェックを済ませてから）
2. Settings → Rules → Rulesets → New ruleset → New branch ruleset
   - Name: `main`、Enforcement status: Active、Target branches: Include default branch
   - Branch rules: `Require a pull request before merging`（承認数は 0 でよい。本人 1 人のため）、`Require status checks to pass`
   - Status checks: `verify (ubuntu-latest)`、`verify (windows-latest)`、`verify (macos-latest)`、`msrv`（#13 の PR で一度 CI が走った後なら検索候補に出る）。`Require branches to be up to date before merging` は任意（有効にすると main が進むたびに再実行が要る。分数は public なら無制限）
   - Bypass list: 空（管理者にも適用する）。緊急時は一時的に Enforcement を Disabled にする
3. 確認: `gh api repos/dds-nakamura/runs/rulesets` がルールセットを返し、main への直接 push が拒否される（guard-bash hook は従来どおり手前で止める）

### 型・状態遷移 / 画面・キーバインド / 設定・CLI

該当なし（`src/` を変更しない）。

### 依存クレート

追加なし。CI 用のツール（crate ではない）:

| ツール | 用途 | ライセンス | 代替案 |
|---|---|---|---|
| cargo-deny（CI 実行時に `taiki-e/install-action` で導入） | 依存のライセンス・脆弱性・重複・取得元の確認 | MIT OR Apache-2.0 | `cargo-audit`（脆弱性のみ）、`cargo-license`（ライセンスのみ） |

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| CLAUDE.md Commands「検証はこれ 1 つ」 | CI は `verify.sh --all` を呼ぶだけ。YAML に cargo コマンドを重複して書かない（MSRV ジョブのみ例外、理由は上記） |
| CLAUDE.md「`Cargo.lock` を手で編集しない／`cargo update` で無関係な依存を上げない」 | 全コマンド `--locked`、`git diff --exit-code Cargo.lock` |
| CLAUDE.md「依存クレートの追加は spec で合意」 | 追加なし。`deny.toml` の許可リストを広げるときも spec で合意、と明記 |
| CLAUDE.md「対話 TUI を起動しない」 | CI は `cargo test` と `tests/cli.rs`（非 TTY 経路）のみ。`pty-check.sh` は範囲外 |
| rust-safety 8 章「依存の脆弱性・ライセンスは `verify.sh --all`（cargo-deny 導入時）で確認」 | 導入時が今。CI では必須化 |
| rust-safety 1 章 | 該当コード変更なし |
| intent の制約「外部 Actions はメジャーバージョンまたは SHA で固定」 | メジャーバージョンのタグで固定 |
| ガード（`.claude/scripts` の編集は guard-edit で確認） | `verify.sh` の変更は Edit ツールで行い、確認を受ける。Bash の追記で迂回しない |

## 懸念点（要判断）

### C1. main のブランチ保護は private リポジトリ（Free プラン）では使えない → **判断済み: リポジトリを public にする**（2026-10-08、ユーザー）

事実: `gh api repos/dds-nakamura/runs/branches/main/protection` と `.../rulesets` がどちらも HTTP 403 「Upgrade to GitHub Pro or make this repository public to enable this feature」を返した。GitHub Free の private リポジトリでは、ブランチ保護もリポジトリルールセットも有効にできない。

検討した案: a. 運用で代替（`gh pr checks --watch` を `/pr` に足す。強制力なし）、b. public にする、c. GitHub Pro。
ユーザーは b を選んだ。public になるとルールセットが使え、Actions の分数も無制限になる（C2 の制約が消える）。公開の操作自体はユーザーが行う（取り消しにくい公開操作のため）。公開前に確認すべきことは C5。

### C2. Actions の課金分数 → **判断済み: PR と main の両方で 3 OS を動かす**（2026-10-08、ユーザー）。public 化後は分数の上限が無くなるので、この懸念は private のまま運用する期間（C5 の確認中など）だけ有効

GitHub Free の private リポジトリは月 2,000 分。Windows は 2 倍、macOS は 10 倍で消費する。見積り（キャッシュが効いた状態。初回はこの 2〜3 倍）:

| ジョブ | 壁時計（推定） | 係数 | 課金分 |
|---|---|---|---|
| verify / ubuntu | 3 分 | 1 | 3 |
| verify / windows | 5 分 | 2 | 10 |
| verify / macos | 3 分 | 10 | 30 |
| msrv / ubuntu | 3 分 | 1 | 3 |
| 合計 | | | 約 46 分 / 回 |

PR 1 件につき push 3 回 + マージ後の main で 4 回動くと約 180 分。月 10 PR で 1,800 分となり、上限に近い。`concurrency` のキャンセルで連続 push 分は減る。
推奨: まずこの構成で始め、Settings → Billing で消費を見る。上限に迫るなら、macOS を `push: main` のときだけ動かす（PR では Linux / Windows のみ）か、`workflow_dispatch` の手動実行にする。この切り替えは YAML の `if:` 1 行なので後から変えやすい。

### C3. macOS で初めてテストが走る。落ちる可能性があるテストがある → **判断済み: 落ちたら別課題**（2026-10-08、ユーザー。下表の a）
**結果（2026-10-08、PR #14 の初回 CI）: macOS で全テスト（144 + 9 件）が通過し、懸念は顕在化しなかった。別課題の起票は不要。**

棚卸しの評価（推測を含む）:
- 高め: `src/runner/tests.rs` の `grandchild_ignoring_term_is_killed_after_grace` と `stop_all_and_wait_kills_grandchildren`。`src/runner.rs:417-434` は外部コマンド `kill -s TERM -- -<pid>` でプロセスグループへ送るが、macOS の `/bin/kill` がこの引数形式を受けるかは未確認。`pgrep -f '^sleep 31$'` の一致も未確認
- 中: 停止までの時間の上限アサーション（`< 8s` / `< 1s`）が共有ランナーの遅延で一過性に落ちる可能性
- 低: `echo` 系、`temp_dir()` 系（macOS は `/var` → `/private/var` のシンボリックリンク。パス文字列の完全一致を取っていれば落ちる）

intent の制約は「`src/` を変更しない」。落ちた場合の扱い:

| 案 | 内容 |
|---|---|
| a. 別課題（推奨） | #13 では事実を記録し、macOS の修正は `fix/<N>-macos-...` として別に起票する。#13 の PR は macOS が赤のままマージするか、`continue-on-error` を macOS に一時的に付ける（付けた場合は別課題で外す） |
| b. #13 の中で直す | 原因が `kill` の引数のような小さなものなら直す。`src/` に触るので intent の制約を書き換え、`/fix-bug` の手順（失敗テスト先行）に従う |

### C4. Dependabot / Renovate は入れない → **確認済み**（2026-10-08、ユーザー）

Actions のバージョン更新と依存の更新は自動化しない。`cargo deny check` の advisories で脆弱性が出たときに `cargo update -p <crate>` で個別に上げる運用（CLAUDE.md の既存方針）。Dependabot が欲しくなったら別課題。

### C5. public 化の前に確認すること（判断者: ユーザー。C1 で public を選んだために新たに生じた）

2026-10-08 に追跡ファイル 59 件と全履歴を機械的に確認した結果:

| 項目 | 事実 | 選択肢 |
|---|---|---|
| コミットの author / committer メール | 全履歴 134 コミットのうち 122 件が会社のメールアドレス（会社ドメイン）。残りは GitHub の noreply | a. そのまま公開する（会社のアドレスが公開の履歴に載る）／ b. 公開前に履歴を書き換えて noreply アドレスにする（`git filter-repo`。全コミットのハッシュが変わり force push が要る。**ユーザー自身の操作**。force push は hook で禁止しているので Claude は行わない）／ c. 今後のコミットだけ `git config user.email` を noreply に変える（過去分は残る） |
| 社内固有の名前 | `CLAUDE.md` 冒頭に元のプロジェクト名・Backlog のプロジェクトキー・元のプラグイン名が出てくる（「このプロジェクトでは適用しない」という文脈）。**訂正（reviewer 指摘、2026-10-08）**: 当初「リポジトリ内では CLAUDE.md だけ」と書いたが誤りで、`.claude/README.md`（7 か所）と `.claude/settings.json` の `enabledPlugins`（プラグイン ID 2 件）にもあった。また CLAUDE.md の旧版は履歴に残る（「履歴を書き換えない」判断の範囲内）。同じ PR で追加した spec / plan にも書き写していたので伏せ字にした | a. そのままにする／ b. CLAUDE.md の該当行を「別プロジェクトの規約は適用しない」のような一般的な表現に書き換える（#13 の CLAUDE.md 変更に含める） |
| 秘密情報 | トークン・鍵・パスワードらしき文字列は追跡ファイルに無い（`ghp_` / `github_pat_` / `AKIA` / 秘密鍵ヘッダ / `api_key=` を検索）。`.claude/settings.local.json` と `*.local.md` は gitignore 済み | 対応不要 |
| LICENSE | 無い。public でライセンスが無いと「全権利留保」で、他人は閲覧しかできない。`Cargo.toml` にも `license` 欄が無い | a. `MIT OR Apache-2.0`（Rust の慣例。`LICENSE-MIT` と `LICENSE-APACHE` を置く）／ b. MIT のみ／ c. 置かない（閲覧のみ可） |
| `.steering/` と `.claude/` | 設計文書・ハーネス（hooks / skills / agents / REVIEW.md）が公開される。作成者欄にユーザー名 `nakamura_kouji` がある | a. そのまま／ b. 作成者欄を GitHub ID に変える |
| 配布時の著作権表示 | 未確定事項「配布方法」のまま。public 化はソースの公開であって、バイナリ配布ではないので今回は不要 | 対応不要（`deny.toml` の許可リストが将来の一覧になる） |

**判断済み（2026-10-08、ユーザー）**: メールは c（このリポジトリの `git config user.email` を GitHub の noreply アドレスにする。過去分はそのまま）、固有名は b（CLAUDE.md を「他プロジェクトの規約・スキルは適用しない」のような一般的な表現に。#13 の CLAUDE.md 変更に含める）、LICENSE は a（`MIT OR Apache-2.0`。`LICENSE-MIT` / `LICENSE-APACHE` を置き、`Cargo.toml` に `license = "MIT OR Apache-2.0"` を足す。これにより `deny.toml` の `private.ignore` は不要）、`.steering` / `.claude` はそのまま。履歴の書き換えはしない。

**追加判断（reviewer 指摘を受けて、2026-10-08、ユーザー）**:
- `.claude/settings.json` の `enabledPlugins`（元のプラグイン ID 2 件）は、gitignore 済みの `.claude/settings.local.json` に移す。追跡ファイルから実名が消える代わりに、clone 直後はそのプラグインが有効になる（手順は `.claude/README.md` に記載。開発者は本人のみ）
- LICENSE の著作権者は個人の GitHub ID（`dds-nakamura`）のままでよい。個人のプロジェクトとして判断（職務著作の観点は確認済み。法的助言ではない）
- 同じ PR で追加した spec / plan に固有名を書き写していたので伏せ字にし、CLAUDE.md の Conventions に「追跡ファイルに固有名・メールアドレスを書かない」を足した
- このブランチの途中コミット（spec / plan の旧版）には固有名と会社ドメインの文字列が残る。PR #14 は**スカッシュマージ**で main に入れ、途中コミットを main の履歴に入れない（reviewer 2 回目の指摘、2026-10-08、ユーザー判断）。PR の参照（`refs/pull/14/head`）に残る分は「履歴を書き換えない」判断の範囲内として受け入れる

## intent の未解決の問い

- MSRV 1.88 で現在の `Cargo.lock` の依存がビルドできるか → **部分回答**。全 198 依存の `rust-version` 宣言で 1.88 を超えるものは無い（最大 1.88.0: ratatui / encoding_rs / time など）。未宣言のクレートが多いので実ビルドでしか確定しない。CI の `msrv` ジョブで確認する（手元の WSL での事前確認は、CI が最初の 1 回で答えを出すので省く）→ **確定（2026-10-08）**: PR #14 の `msrv` ジョブが rustc 1.88.0 で `cargo check` と `cargo test`（144 + 9 件）を通過。現在の `Cargo.lock` は MSRV 1.88 でビルドできる
- cargo-deny のライセンス許可リストに何を入れるか → **回答**。上記「`deny.toml` のポリシー」の表。`WTFPL` と `Unicode-DFS-2016` はグラフに現れた場合だけ
- MSRV のジョブを Linux だけにするか → **回答**。Linux のみ（C2 の分数と、MSRV の問題は OS によらないため）
- ブランチ保護を管理者にも適用するか → **問いが無効**。ブランチ保護自体が使えない（C1）
