# Intent: SIGTERM / SIGHUP・Ctrl+Break を受けたときも端末を復元し、子プロセスを止めてから終了する

- 課題: #3 https://github.com/dds-nakamura/runs/issues/3
- 作成者: nakamura_kouji / 状態: approved
- 対象: `tui`（イベントループ・端末ガード）、`runner`（終了時の全停止）

## 課題（Problem）

`runs` は `q` / Ctrl+C・エラー・panic で終わるときは端末を復元し、実行中のコマンドを止める。
しかし次の経路では復元処理が走らずにプロセスが終わる。

- Unix: SIGTERM（`kill`）、SIGHUP（端末エミュレータを閉じる、SSH の切断）
- Windows: Ctrl+Break、タブやウィンドウを閉じる操作（CTRL_CLOSE_EVENT）

起きること: 同じ端末でシェルを使い続けると raw mode と alternate screen が残る（Unix で `kill` された場合）。
どちらの OS でも、実行中のコマンド（開発サーバーなど）が止まらずに残り、ポートやファイルを掴んだままになる。
#5 でコマンドの実行ができるようになったので、取り残しの実害が大きくなった。

## 目指す結果（Proposed outcome）

- Unix で実行中に `kill <pid>`（SIGTERM）すると、実行中のコマンドをすべて止め、端末を復元してから終了する。端末は正常に使える
- Unix で端末を閉じる（SIGHUP）と、実行中のコマンドがすべて止まる（端末はもう無いので復元は問わない）
- Windows で Ctrl+Break を押すと、実行中のコマンドをすべて止め、端末を復元してから終了する
- シグナルで終わったときは stderr に `runs: terminated by signal` のようなメッセージを出し、終了コードは 1（通常の終了と区別できる）
- `q` / Ctrl+C・エラー・panic の経路の挙動は変えない

## 影響する利用者・環境

- 利用者: 開発者本人
- 一次対象: Windows 11（Windows Terminal）/ Linux / macOS。実機確認は Windows Terminal（Ctrl+Break、タブを閉じる）と WSL（`kill`、疑似端末の `timeout -s TERM`）

## 制約

- `unsafe` を使わない。シグナルやコンソールの制御イベントの登録は、安全な API を持つクレートに任せる（候補: `ctrlc`。MIT / Apache-2.0、MSRV 1.69。Unix は `nix`、Windows は `windows-sys` に依存する）
- 復元と全停止はイベントループの中（主スレッド）で行う。シグナルのハンドラは「終了してほしい」と伝えるだけにする（ハンドラのスレッドから端末やプロセスに触らない）
- 終了までの時間は、実行中のコマンドの停止の猶予（2 秒 + 1 秒）に収める（例外: 端末が本当に閉じて主スレッドが戻らない場合は最長およそ 10 秒。spec の懸念点 5）
- #1・#5・#7・#8 の挙動は変えない

## 範囲外

- SIGTSTP / SIGCONT（Ctrl+Z での中断と再開）
- Windows のコンソール以外からの終了（タスクマネージャーの「タスクの終了」= TerminateProcess は捕まえられない）
- SIGKILL（捕まえられない）
- Windows でタブ・ウィンドウを閉じる操作（CTRL_CLOSE_EVENT）: `ctrlc` のコールバックがすぐ返るため OS がプロセスを終了させ、片付けが間に合わない（spec で判明。2026-10-08 に範囲外と決定）

## 未解決の問い

- [x] `ctrlc` クレートの追加 → 追加する（2026-10-08 ユーザー決定）
- [x] シグナルで終わったときの終了コード → 1（実行時エラーと同じ。stderr のメッセージで区別。2026-10-08 ユーザー決定）
- [x] Windows でタブを閉じたときの扱い → 片付けは間に合わないので範囲外（spec の懸念点 1）
