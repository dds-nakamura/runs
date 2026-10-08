# Spec: SIGTERM / SIGHUP・Ctrl+Break を受けたときも端末を復元し、子プロセスを止めてから終了する（intent.md 2026-10-08 より）

## 要件

### 機能要件（受け入れ条件）

- [ ] Unix で、実行中に SIGTERM を受けると、実行中のコマンドをすべて止め（#5 の全停止と同じ: TERM → 2 秒 → KILL）、端末を復元し、
  stderr に `runs: terminated by signal` を出して終了コード 1 で終わる。raw mode と alternate screen が残らない
- [ ] Unix で SIGHUP（端末を閉じる）でも同じ経路を通り、実行中のコマンドがすべて止まる
- [ ] Unix で SIGINT を `kill -INT` で受けたときも同じ（raw mode 中は Ctrl+C がキーとして届くので、この経路は `kill` だけ）
- [ ] Windows で Ctrl+Break を押すと、実行中のコマンドをすべて止め、端末を復元し、同じメッセージと終了コードで終わる
- [ ] シグナルを受けてから終了するまでの時間は、全停止の上限（2 秒 + 1 秒）に収まる。実行中のコマンドが無ければすぐ終わる
- [ ] `q` / Ctrl+C・エラー・panic の経路、`--version` / `--help`、端末でないときの挙動（終了コード 1、ハンドラを登録しない）は変えない

### 非機能要件

- ハンドラのスレッドからは端末にもプロセスにも触らない。フラグを立てるだけにして、主スレッドのイベントループが既存の終了経路を通る
- 待機中の CPU は変えない（フラグの確認は既存の 50 ms のループに乗せる）
- 対象は Windows 11（Windows Terminal）/ Linux / macOS。実機確認は Windows Terminal（Ctrl+Break、タブを閉じる）と WSL（`kill`、疑似端末）

## 設計

### 変更方針

`tui::run` の先頭（端末でないときの確認の後、端末ガードの前）で `ctrlc::set_handler` を登録し、ハンドラは `Arc<AtomicBool>` を立てるだけにする。
イベントループは毎回フラグを見て、立っていれば `Err(terminated by signal)` で抜ける。すると既存の経路（`stop_all_and_wait` → ガードの drop で復元 → `main` が stderr へ → 終了コード 1）が走る。

| ファイル | 変更 |
|---|---|
| `src/tui.rs` | `ctrlc::set_handler` の登録、ループでのフラグ確認、`TerminationRequested` のエラー |
| `src/main.rs` | 変更なし（`tui::run` の `Err` を `runs: <メッセージ>` で出して終了コード 1 にする既存の経路） |
| `Cargo.toml` | `ctrlc` 3.5（feature `termination`）。追加済み |

### 型・状態遷移

```rust
// src/tui.rs
/// シグナル・コンソールの制御イベントで立つ。ハンドラのスレッドはこれ以外に触らない
fn install_termination_flag() -> Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = Arc::clone(&flag);
    ctrlc::set_handler(move || handler_flag.store(true, Ordering::Release))
        .context("failed to install the signal handler")?;
    Ok(flag)
}

// event_loop の各周回の先頭
if terminated.load(Ordering::Acquire) {
    bail!("terminated by signal");
}
```

- `ctrlc::set_handler` はプロセスで 1 回しか呼べない（2 回目は `Err(MultipleHandlers)`）。`tui::run` は 1 回しか呼ばれないので問題ない
- `termination` feature で、Unix は SIGINT / SIGTERM / SIGHUP、Windows は CTRL_C / CTRL_BREAK / CTRL_CLOSE / CTRL_LOGOFF / CTRL_SHUTDOWN がハンドラの対象になる
- `event::poll(50 ms)` で待っている間にフラグが立っても、次の周回（最長 50 ms 後）で気づく

### ctrlc の挙動で設計が決まる点（ソースで確認）

- Unix（`ctrlc-3.5.2/src/platform/unix`）: `sigaction` でハンドラを登録し、シグナルが来るとパイプに書く。別スレッドがパイプを読んで利用者のクロージャを呼ぶ。
  シグナルの既定動作（プロセスの終了）は置き換わるので、プロセスは続き、主スレッドが片付けてから終わる
- Windows（`src/platform/windows/mod.rs`）: `SetConsoleCtrlHandler` のコールバックはセマフォを上げて **すぐ TRUE を返す**。利用者のクロージャは別スレッドが後で呼ぶ。
  Ctrl+Break はコールバックが返ってもプロセスが続くので、主スレッドが片付けられる。
  **タブ・ウィンドウを閉じる操作（CTRL_CLOSE_EVENT）は、コールバックが返った時点で OS がプロセスを終了させる**ので、片付けは間に合わない（懸念点 1）

### 画面・キーバインド

変更なし。

### 設定・CLI

変更なし。終了コードは 1、メッセージは `runs: terminated by signal`。

### 依存クレート（追加済み）

| クレート | 用途 | ライセンス | 代替案 |
|---|---|---|---|
| `ctrlc` 3.5.2（`termination`） | SIGINT / SIGTERM / SIGHUP と Windows の制御イベントを安全な API で受ける。MSRV 1.69 | MIT / Apache-2.0 | `signal-hook`（Unix のみ）。`windows-sys` の直接利用は `unsafe` が要る |

推移的依存: `nix` 0.31.3（Unix）、`windows-sys`（Windows。既に ratatui 経由で入っている）、`objc2` 0.6.5 / `objc2-encode` 4.1.0 / `block2` 0.6.2 / `dispatch2` 0.3.1（macOS のみ）。いずれも MIT / Apache-2.0

### テスト（`/tui-test` の層）

| 層 | 内容 |
|---|---|
| 1〜2 | 追加なし（`app` / `ui` は変わらない） |
| 3 | `tests/cli.rs` の既存 9 件が変わらないこと（端末でないときはハンドラを登録しない） |
| 4（WSL、`pty-check.sh` に追加） | (a) 起動して 2 秒後に `kill -s TERM`: `EXIT=1`、stderr に `terminated by signal`、終了後のモードが `isig icanon echo`、`ESC[?1049l` が 1 回。(b) `kill -s HUP` で同じ。(c) 一時的な `runs.toml` で `sleep 300` を実行中に `kill -s TERM`: `pgrep -f "^sleep 300$"` が 0 |
| 4（Windows Terminal、ユーザー） | `ping -t` を実行中に Ctrl+Break → `tasklist` に ping が残らず、端末が戻り、メッセージが出る。タブを閉じる → ping が残るかどうかを記録する（懸念点 1） |

### ドキュメント

- CLAUDE.md の Architecture: `tui` の説明に「シグナルはフラグを立てるだけ（`ctrlc`）」、依存の一覧に `ctrlc`
- `.claude/scripts/pty-check.sh` にシグナルのケースを追加（上の (a)〜(c)）

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| rust-safety 2 章 | 復元は既存の 1 か所（ガードの drop）。ハンドラから復元しない。復元の失敗で panic しない |
| rust-safety 5 章 | `unsafe` を使わない（`ctrlc` が内部で持つ） |
| rust-safety 7 章 | メッセージは端末の復元後に `main` が stderr へ |
| rust-safety 8 章 | 依存は intent で合意。feature は `termination` だけ |
| CLAUDE.md Architecture | 端末に触るのは `tui` と `main` だけのまま |

## 懸念点（1 は 2026-10-08 にユーザーが判断済み）

1. **Windows でタブ・ウィンドウを閉じる操作は片付けが間に合わない** → 決定: 範囲外。`ctrlc` のコールバックがすぐ返るため、OS がプロセスを終了させる。
   実行中のコマンド（`ping -t` など）が残る。対応するには `SetConsoleCtrlHandler` を直接登録してコールバックの中で片付ける必要があり、`windows-sys` と `unsafe` が要る。
   この課題では範囲外とし、intent の「目指す結果」から外す（Ctrl+Break は対応する）
2. **ハンドラの登録が失敗した場合**（まれ。OS のエラー）は `tui::run` をエラーで終える。シグナルを捕まえられないまま動くより、起動しない方を選ぶ
3. **SIGHUP で端末が無くなった後の復元は、stdout への書き込みが失敗する**。失敗は捨てる（既存の `Drop` の方針）
4. **macOS 向けの推移的依存（`objc2` 系）が `Cargo.lock` に入る**。macOS でのビルドは未検証のまま
5. **impact-analyzer は使っていない**。変更は `tui.rs` 1 ファイル

## intent の未解決の問い

- Windows でタブを閉じたときに全停止が間に合わない場合 → 間に合わない（`ctrlc` の実装上、必ず）。範囲外にする（懸念点 1）
