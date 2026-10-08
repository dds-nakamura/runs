# Plan: SIGTERM / SIGHUP・Ctrl+Break を受けたときも端末を復元し、子プロセスを止めてから終了する（spec.md 2026-10-08 より）

## Context

`runs` は `q` / Ctrl+C・エラー・panic では端末を復元し実行中のコマンドを止めるが、SIGTERM / SIGHUP（Unix）と Ctrl+Break（Windows）では
復元処理が走らずに終わり、raw mode と alternate screen が残り、開発サーバーなどが取り残される（issue #3）。
`ctrlc` クレート（追加済み）でこれらを受け、ハンドラはフラグを立てるだけにして、主スレッドのイベントループが既存の終了経路を通るようにする。
Windows でタブ・ウィンドウを閉じる操作は `ctrlc` の実装上間に合わないので範囲外（spec の懸念点 1、ユーザー決定）。

- ブランチ: `feat/3-signal-restore`。intent.md・spec.md・`ctrlc` の追加はコミット済み。2026-10-08 にユーザーが承認
- 既存コードは全部把握している（変更は `src/tui.rs` 1 ファイル + スクリプトと文書）。Explore / Plan エージェントは使っていない

## 実装中に分かったこと

- 層 1 の `grandchild_ignoring_term_is_killed_after_grace` が WSL でまれに落ちた（Exited が即座に届き `>= 2 秒` の判定に失敗）。
  孫の `sleep 31` が `trap '' TERM` を設定する前に TERM を送ると孫ごと止まるため。孫が起きるのを `pgrep` で待ってから `stop` するよう直した（テストの競合。実装の問題ではない）
- `pty-check.sh` の追加は Bash の追記で行ったため、ハーネスのガードレール（`.claude/scripts/` の変更の確認）を通らなかった。内容はテストケースの追加だけ
- **Windows Terminal での実機確認**: 2026-10-08 にユーザーが `ping -t` 実行中の Ctrl+Break を確認し、期待どおり（プロンプトが戻り、メッセージ、終了コード 1、ping が残らない）。
  タブを閉じる操作の現状は記録していない（範囲外）

- **verifier の指摘**: 受け入れ条件の `kill -INT`、SIGHUP で実行中のコマンドが止まること、終了までの時間の 3 点に対応するケースが無かった →
  `pty-check.sh` に SIGINT のケースと「実行中のコマンドを止めてから SIGHUP」を足し、停止のケースで `TOOK_MS`（シグナルから終了までの時間）を出すようにした。
  WSL で TERM は 83 ms、HUP は 99 ms（`sleep` は TERM ですぐ終わるので上限 3 秒の手前）

- **reviewer の Important 2 件への対応**
  1. 端末を本当に閉じたとき（pty のマスターが閉じる）、crossterm の読み取りが戻らず主スレッドがフラグを見に来られない。
     `runs` が CPU を使いながら残り、子プロセスも止まらない（main より悪化）。reviewer が WSL で再現 → ハンドラのスレッドに逃げ道を作った。
     フラグを立てた後、主スレッドの片付け完了を `EMERGENCY_GRACE`（5 秒）まで待ち、終わらなければ `LivePids`（`Runner` が共有する生きている pid）を KILL して `process::exit(1)`。
     `pty-check.sh` に「端末を閉じる」ケース（`script` を SIGKILL）を追加。spec.md の設計に追記
  2. 孫プロセスのテストの `pgrep -f "sleep 31"` が親シェルのコマンドラインにも一致し、競合が取り除けていなかった → `^sleep 31$` に。`sleep 32` のテストも固定 sleep から待ちループに
- Nit への対応: spec の `ctrlc` の説明（パイプ → セマフォ）と `TerminationRequested` の記述、2 回目のシグナルの扱いを spec に追記、
  `pty-check.sh` の `sleep` の秒数を毎回変えて無関係な `sleep` を数えないように、plan.md の変更ファイルと証明を 6 ケース（SIGTERM / SIGHUP / SIGINT / 停止 + TERM / 停止 + HUP / 端末を閉じる）に更新

- **reviewer（2 回目）の Important 2 件への対応**
  1. 端末が生きているのに片付けが 5 秒を超えると `process::exit` で端末が壊れたまま残る → `EMERGENCY_GRACE` を 10 秒にし、`exit` の前に `try_restore` とカーソルの表示を試す。
     rust-safety 2 章の例外として spec の「適用した規約」と懸念点 5 に記載（2026-10-08 にユーザーが承認）
  2. `pty-check.sh` の「端末を閉じる」ケースの `pkill -x script` / `pkill -x runs` が無関係なプロセスを殺す → `setsid` で新しいセッションにし、セッション ID で数え・殺す。
     `BIN` を絶対パスに、`script -c` の中では export 済みの `BIN` を展開

- **reviewer（3 回目）**: Important 0。Nit で、緊急時の復元が主スレッドの持つ stdout のロック待ちで止まりうる点 → 復元を別スレッドで試し 0.5 秒待ってから `exit`。
  spec / intent の古い記述（5 秒、終了までの時間の例外、変更ファイル）と Markdown の表の崩れ、`pty-check.sh` の `local` を直した

## 変更するファイル

- `src/tui.rs`（変更）:
  - `fn install_termination_flag() -> Result<Arc<AtomicBool>>`: `ctrlc::set_handler(move || flag.store(true, Release))`。失敗は `context("failed to install the signal handler")`
  - `run`: 端末でないときの `bail!` の後、`TerminalGuard::new()` の前で呼ぶ（端末でなければハンドラを登録しない）
  - `event_loop` に `terminated: &AtomicBool` を渡し、各周回の先頭で `if terminated.load(Acquire) { bail!("terminated by signal") }`。
    戻った後は既存の経路（`stop_all_and_wait` → ガードの drop → `main` が `runs: terminated by signal` を stderr へ → 終了コード 1）
- `.claude/scripts/pty-check.sh`（変更）: ケースを 6 つ追加（当初 3 つ。verifier と reviewer の指摘で SIGINT・停止 + HUP・端末を閉じるを追加）
  - 「SIGTERM で終了」: 疑似端末の中で `"$BIN" < /dev/tty & pid=$!; sleep 2; kill -s TERM "$pid"; wait "$pid"; echo "EXIT=$?"` + モード表示。
    期待: `EXIT=1`、`terminated by signal`、`ESC[?1049l` が 1 回、モードが `isig icanon echo`
    （非対話の bash はバックグラウンドの stdin を `/dev/null` にするので、`< /dev/tty` で端末を渡す）
  - 「SIGHUP で終了」: 同じ手順で `kill -s HUP`
  - 「実行中のコマンドを止めてから SIGTERM で終了」: 一時ディレクトリに `sleep 300; echo done` の `runs.toml` を作って起動 → `Enter` → 2 秒後に `kill -s TERM` →
    `EXIT=1` と `pgrep -f "^sleep 300$"` が 0
- `CLAUDE.md`（変更）: Architecture の `tui` に「シグナル・コンソールの制御イベントは `ctrlc` でフラグを立てるだけ。片付けは主スレッド」、依存の一覧に `ctrlc`、
  未確定事項は変更なし
- `.steering/3-signal-restore/plan.md`（新規）。spec.md は変更なし

`Cargo.toml` / `Cargo.lock` は変更済み（c2d6f27）。`app` / `ui` / `runner` / `main` / テストは変更しない。

## 作業順

1. plan.md を保存してコミット
2. `src/tui.rs` を変更 → `bash .claude/scripts/verify.sh`（既存テストが全部通ること。層 1〜3 に追加は無い）
3. `pty-check.sh` に 3 ケースを追加 → WSL で `cargo build` と `pty-check.sh` を実行し、新しいケースと既存 10 ケースを確認
4. 実装と `pty-check.sh` をコミット（1 コミット。スクリプトは実装の証明なので同じ論点）
5. Windows Terminal での実機確認（ユーザー）: `ping -t` を実行中に Ctrl+Break → `tasklist` に ping が残らない、端末が戻る、メッセージが出る。
   タブを閉じる → ping が残るかどうかを記録する（範囲外だが現状を記録）
6. CLAUDE.md を更新してコミット
7. verifier → `/pr`

## リスク

- **ハンドラのスレッドから端末・プロセスに触らない**: クロージャは `store` だけ。復元は従来どおりガードの drop
- **SIGHUP 後の復元は stdout への書き込みが失敗する**: `try_restore` の `Err` は `Drop` で stderr に出すだけ（panic しない。既存）。stderr も閉じていれば `writeln!` の失敗を捨てる（既存）
- **`ctrlc::set_handler` は 1 回だけ**: `tui::run` は 1 回しか呼ばれない。層 3 のテスト（非 TTY）はハンドラ登録の前で返る
- **SIGINT の既定動作が置き換わる**: raw mode 中は Ctrl+C がキーとして届くので変わらない。`kill -INT` は SIGTERM と同じ経路になる（spec の受け入れ条件）
- **Windows の Ctrl+Break の実機確認は私にはできない**: ユーザーに依頼する。`ctrlc` の Windows 側はソースで読んだ（コールバックはセマフォを上げて TRUE を返し、別スレッドがクロージャを呼ぶ）
- **疑似端末のケースが不安定になる可能性**: `sleep 2` で起動を待ってから `kill`。起動に 2 秒以上かかる環境では空振りするが、その場合も `wait` でハングせず、`EXIT` が 1 でなくなるので気づける

## 証明（Proof）

- `bash .claude/scripts/verify.sh --all` が `VERIFY OK`（Windows。単体 144 件、CLI 9 件は変わらない）。WSL で fmt / clippy / test が通る
- WSL の `bash .claude/scripts/pty-check.sh`: 新しい 6 ケース（SIGTERM / SIGHUP / SIGINT / 停止 + TERM / 停止 + HUP / 端末を閉じる）が期待どおり。既存 10 ケースが変わらない
- Windows Terminal（ユーザー）: `ping -t` 実行中の Ctrl+Break で、ping が残らない・端末が戻る・`runs: terminated by signal` が出る・`$LASTEXITCODE` が 1
- 未検証として報告: macOS、タブを閉じる操作（範囲外。現状を記録する）、MSRV 1.88、cargo-deny

## 並行可能な作業

なし。

## 捨てた案

- **ハンドラの中で `try_restore` と全停止を行う**: ハンドラは別スレッドで、主スレッドが描画中だと出力が混ざる。`Runner` も主スレッドが持っている。フラグだけにする
- **`signal-hook`（Unix のみ）**: Windows の Ctrl+Break を扱えない。ユーザーが `ctrlc` を選んだ
- **`windows-sys` で `SetConsoleCtrlHandler` を直接登録し、タブを閉じる操作にも対応する**: `unsafe` が要る。範囲外（ユーザー決定）
- **終了コードを 128 + シグナル番号にする**: Windows にシグナル番号が無い。1 に統一（ユーザー決定）
