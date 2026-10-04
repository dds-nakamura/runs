// フック共通処理。各フックは stdin の JSON を読み、判定結果を stdout の JSON で返す。
import fs from 'node:fs';
import path from 'node:path';

export function readInput() {
  try {
    return JSON.parse(fs.readFileSync(0, 'utf8') || '{}');
  } catch {
    return {};
  }
}

export function projectDir(input) {
  return process.env.CLAUDE_PROJECT_DIR || input.cwd || process.cwd();
}

export function stateDir(input) {
  const dir = path.join(projectDir(input), '.claude', 'state');
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

// プロジェクトルートからの相対パス（区切りは / に統一）。プロジェクト外なら null。
export function relPath(input, filePath) {
  if (!filePath) return null;
  const rel = path.relative(projectDir(input), path.resolve(projectDir(input), filePath));
  if (rel.startsWith('..') || path.isAbsolute(rel)) return null;
  return rel.split(path.sep).join('/');
}

export function preToolUse(decision, reason) {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'PreToolUse',
      permissionDecision: decision,
      permissionDecisionReason: reason,
    },
  }));
  process.exit(0);
}

export function readJson(file, fallback) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return fallback;
  }
}

export const PROTECTED_BRANCHES = ['main', 'master'];
