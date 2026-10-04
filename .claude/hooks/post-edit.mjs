// PostToolUse(Edit|Write|MultiEdit): 編集記録（Stop フック用）と rustfmt。
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { readInput, relPath, stateDir, projectDir } from './lib.mjs';

const input = readInput();
const filePath = input.tool_input?.file_path;
const rel = relPath(input, filePath);
if (!rel || !/\.rs$|(^|\/)Cargo\.toml$/.test(rel) || rel.startsWith('.claude/')) process.exit(0);

const root = projectDir(input);
fs.writeFileSync(path.join(stateDir(input), 'last-edit.json'),
  JSON.stringify({ session_id: input.session_id, ts: Math.floor(Date.now() / 1000), file: rel }));

if (!rel.endsWith('.rs')) process.exit(0);

// rustfmt 単体は Cargo.toml の edition を読まない（既定は 2015）ため、最寄りの Cargo.toml から拾って渡す
function edition() {
  let dir = path.dirname(path.join(root, rel));
  while (true) {
    try {
      const m = fs.readFileSync(path.join(dir, 'Cargo.toml'), 'utf8').match(/^\s*edition\s*=\s*"(\d{4})"/m);
      if (m) return m[1];
    } catch { /* Cargo.toml なし */ }
    if (path.resolve(dir) === path.resolve(root)) return '2024';
    const parent = path.dirname(dir);
    if (parent === dir) return '2024';
    dir = parent;
  }
}

const r = spawnSync('rustfmt', ['--edition', edition(), rel], { cwd: root, encoding: 'utf8' });
if (r.error) process.exit(0); // rustfmt 未導入の環境では何もしない
if (r.status !== 0) {
  // 構文エラー等。exit 2 で Claude に即時フィードバックする
  process.stderr.write(`rustfmt が失敗しました（構文エラーの可能性）:\n${r.stderr}`);
  process.exit(2);
}
process.exit(0);
