---
name: intent
description: 課題（GitHub issue・口頭の要望）から intent.md（課題の意図）を作る。新しい機能・改善・不具合に着手するとき、「これを始めたい」「〇〇をやる」と言われたときに使う。
argument-hint: <#issue番号 | issue URL | 要望の説明>
---

# intent.md の作成（Plan ステージ）

「何を・なぜ」を機械が読める形で固定する。**解決策の設計はここではしない**（spec.md の仕事）。

## 手順

1. 課題の本文を読む
   - GitHub の issue（`#12` / URL）: `gh issue view <番号> --comments`（リポジトリ・gh が未準備ならユーザーに本文を貼ってもらう）
   - 口頭の要望: ユーザーの説明をそのまま正とし、課題は「なし」
2. 足りない情報をユーザーに質問する（誰が使うか／どの操作で困るか／成功の判定方法／対象 OS・端末／期限・制約）。
   推測で埋めない。分からないものは「未解決の問い」に残す
3. 作業ブランチと作業ディレクトリを決める
   - ブランチ: `<type>/<issue番号>-<slug>`（issue が無ければ `<type>/<slug>`。type は feat / fix / refactor / docs / chore）。
     未作成なら名前をユーザーに提案する（ブランチ作成は git 操作なのでユーザーの了承後）
   - 作業ディレクトリ: `.steering/<ブランチ名の type/ 以降>/`（例: `feat/12-key-bindings` → `.steering/12-key-bindings/`）
4. 下のテンプレートで `intent.md` を書く
5. ユーザーに確認を依頼し、承認されたら「次は /spec（小さな修正なら /plan）」と伝える。
   承認後、作業ブランチ上で `intent.md を追加 (#12)` としてコミットする

## テンプレート

```markdown
# Intent: <件名>

- 課題: <#issue番号・URL / なし（口頭の要望）>
- 作成者: <ユーザー名> / 状態: draft | approved
- 対象: <画面・コマンド・モジュール>

## 課題（Problem）
<誰が・どの操作で・何に困っているか。再現手順・頻度など事実があれば書く>

## 目指す結果（Proposed outcome）
<完了したら何ができるようになるか。判定できる形で>

## 影響する利用者・環境
<利用者像、対象 OS（Windows / Linux / macOS）、端末（Windows Terminal / conhost / iTerm2 / tmux など）>

## 制約
<互換性（CLI 引数・設定ファイル・キーバインド）、変えてはいけない挙動、性能、リリース時期>

## 未解決の問い
- [ ] <誰に確認するか>
```
