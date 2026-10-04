---
name: pr-feedback
description: プルリクエストについた未対応のレビューコメントを集めて対応し、検証して push する。「PRの指摘を直して」「レビューコメント対応」と言われたときに使う。
argument-hint: [PR番号（省略時は現在のブランチの PR）]
---

# PR 指摘対応ループ

## 手順

1. 現在のブランチの PR を特定する（PR 番号指定があればそれ）
   - `gh pr view [番号] --json number,url,reviews,comments`、行コメントは `gh api repos/{owner}/{repo}/pulls/<番号>/comments`
2. 未対応のコメント（対応済み返信・resolve のないもの）を一覧化してユーザーに示す
3. 指摘ごとに: 妥当か判断 → 妥当なら修正、不同意なら根拠を用意（勝手に無視しない）
4. `bash .claude/scripts/verify.sh` → `reviewer` サブエージェントで修正差分を再確認
5. コミット（`レビュー指摘を直す: <要点> (#12)`）して push（確認あり）
6. 各指摘への対応内容を返信する（`gh pr comment` / 行コメントへの返信は `gh api` の replies エンドポイント）。**投稿前に文面をユーザーに確認する**
7. 同じ種類の指摘が 2 回目なら、CLAUDE.md の「Things Claude gets wrong」への追記案を提示する

マージ・承認はしない。人の承認待ちの状態まで持っていくのがゴール。
