// テストロックの操作（/fix-bug 用 CLI）。
//   node .claude/scripts/test-lock.mjs on <test file...> [--reason <issue番号>]  ロック（エージェント可）
//   node .claude/scripts/test-lock.mjs status                                  状態表示
//   node .claude/scripts/test-lock.mjs off                                     解除（ユーザーが `!` で実行）
import fs from 'node:fs';
import path from 'node:path';

const root = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const dir = path.join(root, '.claude', 'state');
const file = path.join(dir, 'test-lock.json');
const [cmd, ...rest] = process.argv.slice(2);

if (cmd === 'on') {
  const ri = rest.indexOf('--reason');
  const reason = ri >= 0 ? rest.splice(ri, 2)[1] : '/fix-bug';
  const files = rest.map((f) => path.relative(root, path.resolve(root, f)).split(path.sep).join('/'));
  if (!files.length) { console.error('ロック対象のテストファイルを指定してください'); process.exit(1); }
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(file, JSON.stringify({ reason, files, lockedAt: new Date().toISOString() }, null, 2));
  console.log(`テストロック: ${files.join(', ')}`);
} else if (cmd === 'off') {
  fs.rmSync(file, { force: true });
  console.log('テストロックを解除しました');
} else {
  console.log(fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : 'ロックなし');
}
