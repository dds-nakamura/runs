---
name: rust-safety
description: runs の安全性・堅牢性の方針（エラー処理と panic、端末状態の復元、外部入力の表示、文字列と座標の扱い、unsafe、外部コマンド、ログ出力、依存クレート）を適用する。これらに触れるコードを新規作成・変更・レビュー・設計するとき（spec 作成時を含む）は必ず使う。
---

# 安全性・堅牢性チェックリスト

各項目は「違反するとどうなるか」とセットで覚える。満たせない場合は spec.md の「懸念点」に書く。

## 1. エラー処理と panic

- 非テストコードで `unwrap()` / `expect()` / `panic!` / `unreachable!` / 範囲外になりうる添字 `v[i]` を使わない。
  `?` で伝播し、呼び出し側で利用者向けメッセージにする。不変条件で本当に起きない場合のみ `expect("<なぜ起きないか>")` 可
- エラー型は `anyhow`（`anyhow::Result`）。`?` で伝播するときは `.context("<何をしていて失敗したか>")` を付ける。
  エラーの種類で呼び出し側が分岐する必要が出たら、`thiserror` の追加を spec で合意する（それまで独自のエラー型は作らない）
- 利用者向けのエラーメッセージは、端末を復元した後に stderr へ出し、非ゼロの終了コードで終わる
- エラーを握りつぶさない（`let _ = ...` / `.ok()` で捨てる場合は理由コメント）

## 2. 端末状態の復元（TUI 固有・最重要）

raw mode・alternate screen・カーソル非表示・マウスキャプチャを有効にしたまま終了すると、利用者のシェルが壊れる。

- 土台は `ratatui::try_init()`（panic hook の設定・raw mode・alternate screen）と `ratatui::try_restore()`。
  `ratatui::init()` は初期化に失敗すると panic するので使わない（非テストコードで panic させない方針のため）
- **`try_init()` は途中で失敗しても元に戻さない**（raw mode を有効にした後、alternate screen への切り替えや
  サイズ取得で `Err` になると raw mode が残る）。`try_init()` が `Err` のときも必ず `try_restore()` を呼ぶ
  （ガードを `try_init()` の前に作るか、`Err` の分岐で復元してから返す）
- ratatui の panic hook と `try_restore()` が戻すのは raw mode と alternate screen だけ。
  - カーソル: 非表示にしたカーソルは `Terminal` の drop で戻る。`Terminal` が drop されない経路
    （`std::process::exit`、`panic = "abort"`、別スレッドが保持）では戻らないので、その経路を作らない
  - マウスキャプチャなどを追加で有効にしたら、その復元はガードと panic hook の両方に自分で入れる。
    自前の panic hook は `try_init()` より前に設定する（ratatui の hook が先に端末を復元してから呼ばれる）
- ratatui の panic hook はどのスレッドの panic でも端末を復元する。ワーカースレッドが panic すると、
  メインの TUI が動いたまま raw mode と alternate screen だけ解除される。ワーカーの panic は終了として扱う
- `try_init()` は呼ぶたびに panic hook を積み増す。外部エディタの起動などで端末を一時的に手放すときは、
  `try_init()` を呼び直さずに raw mode と alternate screen だけを切り替える
- 有効化と復元を RAII ガード（`Drop` で復元）にまとめ、早期 return・`?` でも必ず復元されるようにする
- panic hook を設定し、**panic メッセージを出す前に**端末を復元する（復元しないとメッセージが alternate screen に消える）
- Ctrl+C は raw mode ではシグナルにならずキー入力として届く。終了キーとして明示的に扱う
- 復元処理自体のエラーで panic しない

## 3. 外部入力の表示（端末エスケープ注入）

ファイル内容・ファイル名・外部コマンドの出力・ネットワークからの文字列には ESC などの制御文字が含まれうる。
そのまま端末に出すと、画面の改ざん・ウィンドウタイトル変更・OSC 52 によるクリップボード書き換えなどが起きる。

- 信頼できない文字列は、描画前に制御文字（`\x00-\x1f`、`\x7f`、C1 `\u{80}-\u{9f}`）を除去または可視化（`^[` 等）する
- 改行・タブは描画側で明示的に扱う

## 4. 文字列と座標

- `&s[..n]` のバイト添字で切らない（UTF-8 の文字境界でなければ panic）。`char_indices` / `chars().take()` を使う
- 表示幅は文字数ではない（全角・絵文字は幅 2、結合文字は幅 0）。幅計算は `unicode-width` 等で行う
- 端末座標は `u16`。`area.width - 2` のような減算は極小サイズでアンダーフローする（debug は panic、release は巨大値）。
  `saturating_sub` / `checked_sub` を使う。リサイズ直後のサイズ 0 も想定する

## 5. unsafe

- 原則禁止。クレートルートに `#![forbid(unsafe_code)]` を置く（`src/main.rs` に設定済み。外さない）
- 必要な場合は spec で合意し、`// SAFETY:` コメントで前提を書き、最小のモジュールに閉じ込める

## 6. 外部コマンド・ファイル

- 外部コマンドは `std::process::Command::new(prog).args([...])` で引数を分けて渡す。利用者の入力を `sh -c` / `cmd /C` の文字列に連結しない
- パスは `Path` / `PathBuf` で扱い、文字列連結で組み立てない（Windows の区切り・ドライブレター・UNC）
- 設定・データの保存先は OS の規約に従う（`%APPDATA%`、`$XDG_CONFIG_HOME`、`~/Library/Application Support`）

## 7. ログ・秘密情報

- TUI 実行中は stdout / stderr に `println!` / `eprintln!` / `dbg!` を出さない（画面が崩れる）。ログはファイルへ出す
- トークン・パスワード・個人情報をログ・画面・エラーメッセージに出さない

## 8. 依存クレート

- 追加は spec の「依存クレート」表で合意してから（用途・ライセンス・代替案）。`cargo add` は確認が入る
- feature は必要なものだけ有効にする（`default-features = false` を検討）。
  `ratatui` は `default-features = false` で入れている。`macros`・`widget-calendar` などが必要になったら feature を足す
- crossterm は直接依存に追加しない。`ratatui::crossterm` を使う（ratatui が使う版とずれると型が合わなくなる）。
  例外: `event-stream`（async）など crossterm 側の feature は ratatui 経由では有効にできない。
  必要になったら spec で合意し、ratatui と同じ版（0.29）に固定して直接依存に足す
- `cargo update` で無関係な依存まで上げない（`cargo update -p <crate>`）
- 依存の脆弱性・ライセンスは `bash .claude/scripts/verify.sh --all`（cargo-deny 導入時）で確認する
