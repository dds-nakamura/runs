// PreToolUse(Bash|PowerShell): シェルコマンドのガードレール。
// deny は理由と代わりの手段を Claude に返す。公開系（push）は確認、取り消せない公開（cargo publish）は人間のみ。
import { execSync } from 'node:child_process';
import { readInput, projectDir, preToolUse, PROTECTED_BRANCHES } from './lib.mjs';

const input = readInput();
const cmd = String(input.tool_input?.command ?? '');
if (!cmd) process.exit(0);

// git -C <dir> / cd <dir> && ... の場合はそのディレクトリのブランチで判定する
function targetDir() {
  const m = cmd.match(/\bgit\s+-C\s+("[^"]+"|'[^']+'|\S+)/) || cmd.match(/(?:^|&&|;)\s*cd\s+("[^"]+"|'[^']+'|\S+)/);
  return m ? m[1].replace(/^["']|["']$/g, '') : projectDir(input);
}

function currentBranch() {
  try {
    return execSync('git branch --show-current', { cwd: targetDir(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
  } catch {
    return '';
  }
}

// --- Git: 検証の迂回・履歴破壊・保護ブランチへの直接 push --------------------------
if (/--no-verify\b|commit\.gpgsign=false|--no-gpg-sign\b/.test(cmd)) {
  preToolUse('deny', 'フック・署名の迂回は禁止。失敗したフックの原因を直すこと。');
}
if (/\bgit\s+push\b/.test(cmd)) {
  if (/(\s-f\b|--force\b|--force-with-lease\b|\s\+\S+)/.test(cmd)) {
    preToolUse('deny', 'force push は禁止。必要な場合はユーザー自身が実行する。');
  }
  const explicit = PROTECTED_BRANCHES.find((b) => new RegExp(`(\\s|:)${b}(\\s|$)`).test(cmd));
  const branch = currentBranch();
  if (explicit || PROTECTED_BRANCHES.includes(branch)) {
    preToolUse('deny', `保護ブランチ（${explicit || branch}）への直接 push は禁止。作業ブランチを push し /pr でプルリクエストを出すこと。`);
  }
  preToolUse('ask', 'リモートへの push（外部に公開される操作）。ブランチと内容を確認してください。');
}
if (/\bgit\s+commit\b/.test(cmd)) {
  const branch = currentBranch();
  if (PROTECTED_BRANCHES.includes(branch)) {
    preToolUse('ask', `保護ブランチ ${branch} 上でのコミット。通常は作業ブランチ（<type>/<issue番号>-<slug>）を切ってからコミットする。`);
  }
}
if (/\bgit\s+(reset\s+--hard|clean\s+-[a-z]*f|checkout\s+--\s|restore\s+(--staged\s+)?\.)/.test(cmd)) {
  preToolUse('ask', '作業ツリーの変更を破棄する操作。対象を確認してください。');
}

// --- テストロック（/fix-bug）の解除は人間のみ ------------------------------------
const UNLOCK = /test-lock\.mjs["']?\s+off\b|\b(rm|del|erase|Remove-Item|mv|move|Move-Item|truncate|Clear-Content|Set-Content|Out-File)\b[^\n]*test-lock\.json|>\s*\S*test-lock\.json/;
if (UNLOCK.test(cmd)) {
  preToolUse('deny', 'テストロックの解除はユーザーが `! node .claude/scripts/test-lock.mjs off` で行う。' +
    'ロック中はテストではなく実装を直すこと。');
}

// --- Cargo: 取り消せない公開・リリース操作は人間のみ ------------------------------
if (/\bcargo\s+(publish|yank|owner)\b/.test(cmd)) {
  preToolUse('deny', 'crates.io への公開・yank・owner 変更は取り消せないためエージェントは実行しない。' +
    '手順とコマンドを準備してユーザーに実行を依頼すること。');
}

// --- 対話 TUI を Bash ツールで起動しない ------------------------------------------
// Bash ツールは TTY を持たないため、raw mode / alternate screen を使う TUI はハングするか端末状態を壊す。
// `cargo run -- --help` のように非対話の引数を渡す場合は通す。
if (/\bcargo\s+run\b/.test(cmd) && !/\bcargo\s+run\b[^\n|;&]*\s--\s+\S/.test(cmd)) {
  preToolUse('ask', '引数なしの `cargo run` は対話 TUI を起動し、Bash ツール（TTY なし）ではハングする可能性がある。' +
    '動作確認は描画テスト（/tui-test）か PTY を持つ端末（termio MCP のセッション等）で行うこと。' +
    '非対話の起動（--help 等）なら `cargo run -- <引数>` の形にする。');
}

process.exit(0);
