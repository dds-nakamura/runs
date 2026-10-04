// PreToolUse(Edit|Write|MultiEdit|NotebookEdit): 生成物・テストロック・シークレット混入を防ぐ。
import path from 'node:path';
import { readInput, relPath, stateDir, preToolUse, readJson } from './lib.mjs';

const input = readInput();
const ti = input.tool_input ?? {};
const filePath = ti.file_path ?? ti.notebook_path;
const rel = relPath(input, filePath);
if (!rel) process.exit(0);

// 書き込まれる内容（Write: content / Edit: new_string / MultiEdit: edits[].new_string）
const newText = [ti.content, ti.new_string, ti.new_source, ...(ti.edits ?? []).map((e) => e.new_string)]
  .filter((s) => typeof s === 'string')
  .join('\n');

// --- 生成物・ロックファイル ---------------------------------------------------------
if (/(^|\/)Cargo\.lock$/.test(rel)) {
  preToolUse('deny', 'Cargo.lock は cargo が管理する。Cargo.toml を直して cargo build / cargo update -p <crate> で更新すること。');
}
const GENERATED = [
  /^target\//,
  /^\.git\//,
];
if (GENERATED.some((re) => re.test(rel))) {
  preToolUse('deny', `${rel} は生成物のため直接編集しない。元のソースを直して cargo で再生成すること。`);
}

// --- テストロック（/fix-bug）: 修正中は再現テストを書き換えさせない ---------------------------
const lock = readJson(path.join(stateDir(input), 'test-lock.json'), null);
if (lock && Array.isArray(lock.files) && lock.files.includes(rel)) {
  preToolUse('deny', `${rel} はテストロック中（${lock.reason ?? '/fix-bug'}）。テストではなく実装を直すこと。` +
    'テスト自体が誤っていると判断した場合は作業を止め、根拠を添えてユーザーに報告する（解除はユーザーのみ）。');
}

// --- シークレット混入 ---------------------------------------------------------------
const isPersonalLocal = /(^|\/)settings\.local\.json$|\.local\.md$/.test(rel);
if (!isPersonalLocal && newText) {
  const SECRETS = [
    [/AKIA[0-9A-Z]{16}/, 'AWS アクセスキー'],
    [/-----BEGIN [A-Z ]*PRIVATE KEY-----/, '秘密鍵'],
    [/AIza[0-9A-Za-z_-]{35}/, 'Google API キー'],
    [/gh[pousr]_[A-Za-z0-9]{36,}/, 'GitHub トークン'],
    [/xox[abpr]-[0-9A-Za-z-]{10,}/, 'Slack トークン'],
    [/(api[_-]?key|secret|password|token)["']?\s*[:=]\s*["'][A-Za-z0-9+/_=-]{32,}["']/i, 'ハードコードされた資格情報'],
  ];
  for (const [re, label] of SECRETS) {
    if (re.test(newText)) {
      preToolUse('deny', `${label}らしき文字列を ${rel} に書き込もうとしている。資格情報は環境変数か OS のキーストアから読むこと。` +
        'テスト用のダミー値は example / dummy と分かる値にすること。');
    }
  }
}

// --- ハーネス自体（ガードレール）は人間が確認して変更する ---------------------------
if (/^\.claude\/(hooks\/|scripts\/|settings\.json$)|^REVIEW\.md$/.test(rel)) {
  preToolUse('ask', `${rel} はハーネスのガードレール設定。変更内容を確認してください。`);
}

process.exit(0);
