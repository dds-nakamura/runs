// Stop: このセッションでコードを編集したのに verify.sh が成功していなければ、一度だけ完了を差し止める。
import path from 'node:path';
import fs from 'node:fs';
import { readInput, stateDir, readJson } from './lib.mjs';

const input = readInput();
if (input.stop_hook_active) process.exit(0); // 無限ループ防止（2回目は通す）

const dir = stateDir(input);
const last = readJson(path.join(dir, 'last-edit.json'), null);
if (!last || last.session_id !== input.session_id) process.exit(0);

let verifiedAt = 0;
try {
  verifiedAt = Number(fs.readFileSync(path.join(dir, 'verified-at'), 'utf8').trim()) || 0;
} catch { /* 未検証 */ }

if (verifiedAt < last.ts) {
  process.stdout.write(JSON.stringify({
    decision: 'block',
    reason: `コード変更（最終: ${last.file}）の後に verify が成功していません。` +
      '`bash .claude/scripts/verify.sh` を実行し、失敗ならコードを直してから、出力の要約を添えて報告してください。' +
      '環境要因で実行できない場合は、その理由と「未検証」であることを明記して報告してください。',
  }));
}
process.exit(0);
