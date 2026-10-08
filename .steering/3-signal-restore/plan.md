# Plan: SIGTERM / SIGHUP・Ctrl+Break を受けたときも端末を復元し、子プロセスを止めてから終了する（spec.md 2026-10-08 より）

## Context

`runs` は `q` / Ctrl+C・エラー・panic では端末を復元し実行中のコマンドを止めるが、SIGTERM / SIGHUP（Unix）と Ctrl+Break（Windows）では
復元処理が走らずに終わり、raw mode と alternate screen が残り、開発サーバーなどが取り残される（issue #3）。
`ctrlc` クレート（追加済み）でこれらを受け、ハンドラはフラグを立てるだけにして、主スレッドのイベントループが既存の終了経路を通るようにする。
Windows でタブ・ウィンドウを閉じる操作は `ctrlc` の実装上間に合わないので範囲外（spec の懸念点 1、ユーザー決定）。

- ブランチ: `feat/3-signal-restore`。intent.md・spec.md・`ctrlc` の追加はコミット済み。2026-10-08 にユーザーが承認
- 既存コードは全部把握している（変更は `src/tui.rs` 1 ファイル + スクリプトと文書）。Explore / Plan エージェントは使っていない

## 変更するファイル

- `src/tui.rs`（変更）:
  - `fn install_termination_flag() -> Result<Arc<AtomicBool>>`: `ctrlc::set_handler(move || flag.store(true, Release))`。失敗は `context("failed to install the signal handler")`
  - `run`: 端末でないときの `bail!` の後、`TerminalGuard::new()` の前で呼ぶ（端末でなければハンドラを登録しない）
  - `event_loop` に `terminated: &AtomicBool` を渡し、各周回の先頭で `if terminated.load(Acquire) { bail!("terminated by signal") }`。
    戻った後は既存の経路（`stop_all_and_wait` → ガードの drop → `main` が `runs: terminated by signal` を stderr へ → 終了コード 1）
- `.claude/scripts/pty-check.sh`（変更）: ケースを 3 つ追加
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
- WSL の `bash .claude/scripts/pty-check.sh`: 新しい 3 ケース（SIGTERM / SIGHUP / 実行中の停止）が上の期待どおり。既存 10 ケースが変わらない
- Windows Terminal（ユーザー）: `ping -t` 実行中の Ctrl+Break で、ping が残らない・端末が戻る・`runs: terminated by signal` が出る・`$LASTEXITCODE` が 1
- 未検証として報告: macOS、タブを閉じる操作（範囲外。現状を記録する）、MSRV 1.88、cargo-deny

## 並行可能な作業

なし。

## 捨てた案

- **ハンドラの中で `try_restore` と全停止を行う**: ハンドラは別スレッドで、主スレッドが描画中だと出力が混ざる。`Runner` も主スレッドが持っている。フラグだけにする
- **`signal-hook`（Unix のみ）**: Windows の Ctrl+Break を扱えない。ユーザーが `ctrlc` を選んだ
- **`windows-sys` で `SetConsoleCtrlHandler` を直接登録し、タブを閉じる操作にも対応する**: `unsafe` が要る。範囲外（ユーザー決定）
- **終了コードを 128 + シグナル番号にする**: Windows にシグナル番号が無い。1 に統一（ユーザー決定）
