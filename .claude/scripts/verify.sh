#!/usr/bin/env bash
# 検証をまとめて行う（フィードバックループ用の単一コマンド）。
#
#   bash .claude/scripts/verify.sh          fmt / clippy / test（通常はこれ）
#   bash .claude/scripts/verify.sh --all    CI 相当（上記を --locked で実行 + doc 警告ゼロ + cargo deny（導入時））
#
# 成功時のみ最終行に "VERIFY OK" を出し、.claude/state/verified-at を更新する
# （Stop フックはこの時刻と最終編集時刻を比べて検証漏れを検出する）。
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
STATE_DIR="$ROOT/.claude/state"
mkdir -p "$STATE_DIR"

MODE="default"
for arg in "$@"; do
  case "$arg" in
    --all) MODE="all" ;;
  esac
done
FAIL=0
WARN=0

section() { echo ""; echo "=== $* ==="; }
fail() { echo "NG: $*"; FAIL=1; }
warn() { echo "WARN: $*"; WARN=1; }

# --------------------------------------------------
# 前提
# --------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
  echo "NG: cargo が見つかりません。rustup で stable ツールチェーンを導入してください。"
  echo "VERIFY FAILED (toolchain)"
  exit 2
fi
if [ ! -f Cargo.toml ]; then
  echo "NG: Cargo.toml がありません（cargo init 前）。"
  echo "VERIFY FAILED (no crate)"
  exit 2
fi

export CARGO_TERM_COLOR=never
LOCKED=""
[ "$MODE" = "all" ] && LOCKED="--locked"

section "cargo fmt --check"
cargo fmt --all -- --check || fail "cargo fmt（cargo fmt --all で整形する）"

section "cargo clippy"
cargo clippy $LOCKED --workspace --all-targets --all-features -- -D warnings || fail "cargo clippy"

section "cargo test"
cargo test $LOCKED --workspace --all-features || fail "cargo test"

if [ "$MODE" = "all" ]; then
  section "cargo doc"
  RUSTDOCFLAGS="-D warnings" cargo doc $LOCKED --workspace --all-features --no-deps || fail "cargo doc"

  section "cargo deny"
  if cargo deny --version >/dev/null 2>&1; then
    cargo deny check || fail "cargo deny"
  else
    warn "cargo-deny 未導入のため依存（ライセンス・脆弱性）の確認を省略（cargo install --locked cargo-deny）"
  fi
fi

echo ""
if [ $FAIL -ne 0 ]; then
  echo "VERIFY FAILED"
  exit 1
fi
date +%s > "$STATE_DIR/verified-at"
[ $WARN -ne 0 ] && echo "（WARN あり。上記を確認すること）"
echo "VERIFY OK"
