#!/usr/bin/env bash
# 疑似端末（util-linux の script コマンド）で runs を起動し、キー入力・終了コード・
# 出力されたエスケープシーケンス・終了後の端末モードを表示する（/tui-test の層 4 の一部。Linux 用）。
#
#   bash .claude/scripts/pty-check.sh [バイナリ] [ケース名の正規表現]
#
# Windows からは WSL で実行する（target/ を Windows と共有しないこと）:
#   wsl.exe -e bash -lc 'cd /mnt/c/<リポジトリ> && export CARGO_TARGET_DIR=$HOME/.cache/runs-target \
#     && cargo build --locked && bash .claude/scripts/pty-check.sh'
#
# 合否は判定しない。出力を読んで次を確かめる:
#   - 起動時に ESC[?1049h（alternate screen に入る）、終了時に ESC[?1049l が 1 回だけ出て、その後に ESC[?25h（カーソル表示）
#   - EXIT= が期待する終了コード
#   - 終了後のモードが "isig icanon echo"（raw mode が残っていると -isig -icanon -echo になる）
# 端末エミュレータ上での見た目は確認できない。実端末での目視は別に行う。
set -u

BIN="${1:-${CARGO_TARGET_DIR:-target}/debug/runs}"
FILTER="${2:-.}"
# ケースの中で一時ディレクトリへ cd するので、相対パスのままだと起動に失敗する
BIN=$(realpath "$BIN" 2>/dev/null || echo "$BIN")

for tool in script timeout stty; do
  command -v "$tool" >/dev/null 2>&1 || { echo "NG: $tool が見つかりません"; exit 2; }
done
[ -x "$BIN" ] || { echo "NG: バイナリがありません: $BIN（先に cargo build する）"; exit 2; }

# バイナリのパスは環境変数で渡す。下のシェル断片は単一引用符で書き、疑似端末の中の bash に "$BIN" を展開させる
# （パスを文字列に埋め込むと、空白や引用符を含むパスで壊れる）
export BIN

MODES='stty -a | tr ";" "\n" | grep -oE -- "-?(icanon|isig|echo)\b" | tr "\n" " "; echo'
RUN='stty cols 80 rows 24; "$BIN"; echo "EXIT=$?"; '"$MODES"

# run_case <名前> <キー入力を出すシェル断片> <疑似端末の中で実行するシェル断片> [timeout 秒]
run_case() {
  local name="$1" keys="$2" body="$3" limit="${4:-20}"
  [[ "$name" =~ $FILTER ]] || return 0
  local f
  f=$(mktemp)
  printf '%s\n' "$body" > "$f"
  echo "=== $name"
  (sleep 1.5; eval "$keys"; sleep 1) | timeout "$limit" script -qec "bash \"$f\"" /dev/null | cat -v
  echo "(script の終了コード: ${PIPESTATUS[1]})"
  rm -f "$f"
}

run_case "q で終了（EXIT=0）" "printf q" "$RUN"
run_case "Ctrl+C で終了（EXIT=0）" "printf '\003'" "$RUN"
run_case "Esc と Q では終了せず、その後の q で終了（EXIT=0）" \
  "printf '\033'; sleep 0.7; printf Q; sleep 0.7; printf q" "$RUN"
run_case "Esc と Q だけでは終了しない（timeout で script の終了コードが 124 になるのが正しい）" \
  "printf '\033'; sleep 0.7; printf Q; sleep 6" "$RUN" 5
run_case "極小サイズ 1x1 で起動して q（EXIT=0）" "printf q" \
  'stty cols 1 rows 1; "$BIN"; echo "EXIT=$?"; stty cols 80 rows 24; '"$MODES"
run_case "サイズ 0x0 で起動して q（EXIT=0）" "printf q" \
  'stty cols 0 rows 0; "$BIN"; echo "EXIT=$?"; stty cols 80 rows 24; '"$MODES"
run_case "実行中に 80x24 から 30x4 へリサイズして q（描き直しが出る。EXIT=0）" "sleep 2; printf q" \
  'stty cols 80 rows 24; (sleep 1.5; stty cols 30 rows 4 < /dev/tty) & "$BIN"; echo "EXIT=$?"; '"$MODES"
run_case "stdout がパイプ（メッセージが出て EXIT=1。エスケープシーケンスは出ない）" "true" \
  '"$BIN" | cat; echo "EXIT=${PIPESTATUS[0]}"; '"$MODES"
run_case "stdin が /dev/null（メッセージが出て EXIT=1）" "true" \
  '"$BIN" < /dev/null; echo "EXIT=$?"; '"$MODES"
run_case "--version をパイプへ（EXIT=0）" "true" \
  '"$BIN" --version | cat; echo "EXIT=${PIPESTATUS[0]}"; '"$MODES"

# --- シグナル（#3）。非対話の bash はバックグラウンドの stdin を /dev/null にするので、< /dev/tty で端末を渡す ---
SIGNAL_BODY='stty cols 80 rows 24; "$BIN" < /dev/tty & pid=$!; sleep 2; kill -s SIGNAME "$pid"; wait "$pid"; echo "EXIT=$?"; '"$MODES"
run_case "SIGTERM で終了（EXIT=1、terminated by signal、モードが戻る）" "true" "${SIGNAL_BODY//SIGNAME/TERM}"
run_case "SIGHUP で終了（EXIT=1、モードが戻る）" "true" "${SIGNAL_BODY//SIGNAME/HUP}"
run_case "SIGINT（kill -INT）で終了（EXIT=1、モードが戻る）" "true" "${SIGNAL_BODY//SIGNAME/INT}"
# 長い sleep を実行中にシグナル → 全停止してから終了。SLEEPS=0 が正しく、TOOK_MS は全停止の上限（3000 ms）に収まる。
# sleep の秒数は毎回変えて（1000 + RANDOM）、無関係な sleep を数えないようにする
STOP_BODY='work=$(mktemp -d); n=$((1000 + RANDOM)); printf "[[command]]\nname = \"sleeper\"\ncommand = \"sleep $n; echo done\"\n" > "$work/runs.toml"; cd "$work"; stty cols 80 rows 24; "$BIN" < /dev/tty & pid=$!; sleep 3; start=$(date +%s%N); kill -s SIGNAME "$pid"; wait "$pid"; echo "EXIT=$?"; echo "TOOK_MS=$(( ($(date +%s%N) - start) / 1000000 ))"; '"$MODES"'; echo "SLEEPS=$(pgrep -fc "^sleep $n$" || true)"; cd /; rm -rf "$work"'
run_case "実行中のコマンドを止めてから SIGTERM で終了（EXIT=1、SLEEPS=0、TOOK_MS が 3000 以下）" "sleep 0.5; printf '\r'" "${STOP_BODY//SIGNAME/TERM}" 30
run_case "実行中のコマンドを止めてから SIGHUP で終了（EXIT=1、SLEEPS=0）" "sleep 0.5; printf '\r'" "${STOP_BODY//SIGNAME/HUP}" 30

# 端末を本当に閉じる（script を SIGKILL して pty のマスターを閉じる）。`kill -s HUP` では端末が生きているので、この経路は別に確かめる。
# crossterm の読み取りが戻らず主スレッドが片付けられないため、ハンドラのスレッドが EMERGENCY_GRACE（10 秒）の後に子プロセスを KILL して終わる。
# 起動したものだけを対象にするため、setsid で新しいセッションにして、そのセッション ID で数え・殺す（別の端末の runs や script を巻き込まない）。
# 期待: 12 秒後に RUNS=0（runs が残っていない）、SLEEPS=0
run_hangup_case() {
  local name="端末を閉じる（pty のマスターを閉じる。12 秒後に RUNS=0、SLEEPS=0）"
  [[ "$name" =~ $FILTER ]] || return 0
  command -v setsid >/dev/null 2>&1 || { echo "=== $name"; echo "SKIP: setsid が無い"; return 0; }
  echo "=== $name"
  local work n sid
  work=$(mktemp -d); n=$((1000 + RANDOM))
  printf '[[command]]\nname = "sleeper"\ncommand = "sleep %s; echo done"\n' "$n" > "$work/runs.toml"
  export WORK_DIR="$work"
  # & の子はセッションリーダーではないので setsid は fork せずに exec し、$! がそのままセッション ID になる
  setsid bash -c 'cd "$WORK_DIR" && (sleep 1.5; printf "\r"; sleep 30) | script -qec '"'"'stty cols 80 rows 24; exec "$BIN"'"'"' /dev/null > /dev/null' &
  sid=$!
  sleep 3
  echo "before: RUNS=$(count_runs) SLEEPS=$(pgrep -fc "^sleep $n$" || true)"
  pkill -KILL -s "$sid" -x script
  sleep 12
  echo "after: RUNS=$(count_runs) SLEEPS=$(pgrep -fc "^sleep $n$" || true)"
  local p
  for p in $(pgrep -x runs); do
    [ "$(readlink "/proc/$p/cwd" 2>/dev/null)" = "$work" ] && kill -KILL "$p" 2>/dev/null
  done
  pkill -KILL -s "$sid" 2>/dev/null; pkill -f "^sleep $n$" 2>/dev/null
  rm -rf "$work"
}
# script は子のために別のセッションを作るので、runs はカレントディレクトリ（一時ディレクトリ $work）で見分ける
count_runs() {
  local p c=0
  for p in $(pgrep -x runs); do
    [ "$(readlink "/proc/$p/cwd" 2>/dev/null)" = "$work" ] && c=$((c + 1))
  done
  echo "$c"
}
run_hangup_case
