---
name: pr
description: 変更をレビューしてプルリクエストを作成・更新する。「PRを出して」「プルリクエスト作って」と言われたときに使う。
argument-hint: [base ブランチ（既定: main）]
---

# プルリクエスト作成（Deploy ステージ）

リモートは GitHub。操作は `gh` CLI で行う（未導入なら `winget install GitHub.cli`、未認証なら `! gh auth login` をユーザーに依頼）。

## 手順

1. **状態確認**: `git branch --show-current`、`git status`、`git log origin/<base>..HEAD --oneline`
   - 保護ブランチ（main / master）上なら中止し、作業ブランチの作成を提案する
   - 未コミットの変更があればユーザーに確認する
2. **検証**: `bash .claude/scripts/verify.sh --all` を実行（VERIFY OK でなければ PR を出さない）
3. **セルフレビュー**: `reviewer` サブエージェントを起動。Important があれば直して 2〜3 を繰り返す。
   直さない判断をした指摘は、理由を PR 本文の「レビュー所見」に書く
4. **push**: `git push -u origin <branch>`（フックで確認が入る）
   - origin 未設定（`git remote get-url origin` が失敗）なら push せず、GitHub リポジトリの作成と `git remote add origin <URL>` をユーザーに依頼する
5. **重複確認**: `gh pr list --head <branch>`。既存 PR があれば本文の更新（`gh pr edit`）を提案する
6. **作成**: 本文を `.steering/<作業ディレクトリ>/pr.local.md` に書き、`gh pr create --base <base> --title "<タイトル>" --body-file <本文ファイル>`（確認が入る）
   - base は引数、なければ `main`
   - タイトル: `<日本語の要約>`（issue があれば末尾に ` (#12)`）
   - issue があれば本文の「関連」に `Closes #12` を書く（マージで自動クローズされる）
7. 作成した PR の URL・base←branch・issue番号を報告する

## 本文テンプレート

```markdown
## 概要
<1〜3文>

## 変更内容
- <変更点>

## 関連
- 課題: Closes #<番号> / なし
- 計画: .steering/<ディレクトリ>/plan.md（計画との差分があれば記載）

## 検証（証跡）
<verify.sh --all の最終出力、実行したテストとその結果、PTY での確認結果（OS・端末）。未検証の項目は未検証と明記>

## レビュー所見
<reviewer の Important 件数と対応。対応しなかった指摘と理由>
```
