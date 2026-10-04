// SessionStart: ブランチ・作業ディレクトリ・ツールチェーンの状態をコンテキストに入れる。
import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { readInput, projectDir, PROTECTED_BRANCHES } from './lib.mjs';

const input = readInput();
const root = projectDir(input);
const run = (c) => {
  try { return execSync(c, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim(); } catch { return ''; }
};

const lines = [];

if (!run('git rev-parse --is-inside-work-tree')) {
  lines.push('git 未初期化です（git init 前）。ブランチ・差分に依存する手順（/pr、reviewer）は使えません。');
} else {
  const branch = run('git branch --show-current');
  lines.push(`現在のブランチ: ${branch || '(detached)'}`);

  // feat/TERUST-12-key-bindings → .steering/TERUST-12-key-bindings/
  const slug = branch.includes('/') ? branch.split('/').slice(1).join('-') : '';
  if (slug) {
    const d = `.steering/${slug}`;
    if (fs.existsSync(path.join(root, d))) {
      const have = ['intent.md', 'spec.md', 'plan.md'].filter((f) => fs.existsSync(path.join(root, d, f)));
      lines.push(`作業ディレクトリ: ${d}（あり: ${have.join(', ') || 'なし'}）`);
    } else {
      lines.push(`作業ディレクトリ: ${d} は未作成（/intent で開始）`);
    }
  } else if (PROTECTED_BRANCHES.includes(branch)) {
    lines.push('保護ブランチ上です。コード変更の前に作業ブランチ（<type>/<issue番号>-<slug>）を作成すること。');
  }
}

if (!run('cargo -V')) {
  lines.push('警告: cargo が見つかりません。rustup で stable ツールチェーンを導入してください。');
} else if (!fs.existsSync(path.join(root, 'Cargo.toml'))) {
  lines.push('Cargo.toml がありません（cargo init 前）。verify.sh は初期化後に使えます。');
}

process.stdout.write(JSON.stringify({
  hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: lines.join('\n') },
}));
