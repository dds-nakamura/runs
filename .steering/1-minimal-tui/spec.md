# Spec: 起動して q / Ctrl+C で終了する最小 TUI を作る（intent.md 2026-10-05 より）

## 要件

### 機能要件（受け入れ条件）

起動と終了

- [ ] 端末上で引数なしで起動すると、alternate screen にプレースホルダー画面が出る
- [ ] `q`（修飾キーなし）を押すと終了し、終了コードは 0
- [ ] Ctrl+C を押すと終了し、終了コードは 0
- [ ] 上記以外のキー（Esc、`Q`、Alt+q など）では状態も画面も変わらない
- [ ] キーの Release・Repeat イベントでは状態が変わらない（Windows で `q` を離したときに二重に処理しない）
- [ ] リサイズすると、新しいサイズで画面が描き直される

端末の復元

- [ ] 正常終了後、raw mode と alternate screen が解除され、カーソルが表示され、起動前の画面内容が残っている
- [ ] 実行中のエラー（描画・イベント読み取りの失敗）で終わるときも同じ状態に戻り、その後で stderr にエラーメッセージが出る。終了コードは 1
- [ ] panic で終わるときも raw mode と alternate screen が解除され、panic メッセージが通常の画面で読める
- [ ] 端末の初期化が途中で失敗したときも、raw mode が残らない

端末でない場所での起動

- [ ] stdin または stdout が端末でないとき、引数なしで起動すると stderr にメッセージを出し、終了コード 1 で終わる
- [ ] このとき stdout には何も出さず、raw mode にも alternate screen にも入らない

CLI 引数

- [ ] `--version` / `-V` は `runs <バージョン>` を stdout に 1 行出し、終了コード 0 で終わる
- [ ] `--help` / `-h` は使い方を stdout に出し、終了コード 0 で終わる
- [ ] 上記 2 つは端末を初期化しない。stdout が端末でなくても動く（`runs --version | cat`）
- [ ] 知らない引数・余分な引数があると、stderr にメッセージと使い方を出し、終了コード 2 で終わる

### 非機能要件

- 端末サイズ: 幅・高さが 0 や 1 でも panic しない。入りきらない文字は切り捨てる
- 描画はイベントを受けたときだけ行う。待機中に CPU を使わない（イベントはブロッキングで読む）
- 色を使わない（`NO_COLOR` の扱いは、色を使う機能を作るときに決める）
- 画面と CLI のメッセージは ASCII のみ（懸念点 2）。全角・絵文字の表示幅はこの課題では扱わない
- 対象は Windows 11（Windows Terminal）/ Linux / macOS。実機確認は Windows Terminal と WSL の Ubuntu 24.04。macOS は未検証

## 設計

### 変更方針

既存のコードは `src/main.rs`（バージョンを表示するだけ）しか無いので、ここで構成を決める。
状態・更新・描画・端末・CLI をモジュールに分け、`main.rs` は組み立てと終了コードだけを持つ。
lib ターゲットは作らない（単一のバイナリクレートのまま）。

| ファイル | 役割 | 端末への依存 |
|---|---|---|
| `src/main.rs` | 引数の解釈結果で分岐、エラーの表示、終了コード | あり（stdout / stderr） |
| `src/cli.rs` | 引数の解釈（`Command` を返す）、バージョンと使い方の文字列 | なし |
| `src/app.rs` | 状態 `App`、`Action`、イベント→Action の対応、Action→状態の更新 | なし |
| `src/ui.rs` | `App` を `Frame` に描画する | なし（`TestBackend` で描画できる） |
| `src/tui.rs` | 端末の初期化と復元（`TerminalGuard`）、イベントループ | あり |

テストは各モジュールと別ファイルに置く: `src/cli/tests.rs`・`src/app/tests.rs`・`src/ui/tests.rs`・`tests/cli.rs`。
`src/tui.rs` と `src/main.rs` は実端末が要るので単体テストを書かず、層 3（CLI）と層 4（実機）で確認する。

### 型・状態遷移

```rust
// src/cli.rs
pub enum Command { Run, Version, Help }
pub struct UsageError { /* 利用者向けメッセージ */ }
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Command, UsageError>;
pub fn version_text() -> String;   // "runs 0.1.0"
pub fn help_text() -> String;

// src/app.rs
pub struct App { should_quit: bool }          // Default で should_quit = false
pub enum Action { Quit }
pub fn action_for(event: &Event) -> Option<Action>;   // キーバインドはここだけに書く
impl App {
    pub fn apply(&mut self, action: Action);
    pub fn should_quit(&self) -> bool;
}

// src/ui.rs
pub fn draw(frame: &mut Frame, app: &App);

// src/tui.rs
pub struct TerminalGuard { terminal: DefaultTerminal }
impl TerminalGuard { pub fn new() -> anyhow::Result<Self>; }
impl Drop for TerminalGuard { /* try_restore。失敗しても panic しない */ }
pub fn run(app: &mut App) -> anyhow::Result<()>;
```

状態遷移は 1 つだけ: `Running`（`should_quit = false`）→ `Action::Quit` → `should_quit = true` → ループを抜ける。

`action_for` の規則:

- `Event::Key` で `kind == KeyEventKind::Press` のものだけを見る
- `KeyCode::Char('q')` かつ修飾キーなし → `Quit`
- `KeyCode::Char('c')` かつ修飾キーが `CONTROL` のみ → `Quit`
- それ以外（`Event::Resize` を含む）→ `None`。リサイズはループが次の描画で反映する

イベントループ（`tui::run`）:

1. stdin と stdout が端末か確認する（`std::io::IsTerminal`）。違えばエラーを返す。端末には触らない
2. `TerminalGuard::new()` で初期化する
3. `should_quit` になるまで「描画 → `event::read()` → `action_for` → `apply`」を繰り返す
4. 関数を抜けるとガードの drop で復元する。エラーは `?` で `main` に返す

端末の初期化と復元（`rust-safety` 2 章）:

- 初期化は `ratatui::try_init()`。panic hook の設定、raw mode、alternate screen、`Terminal` の作成をまとめて行う
- `try_init()` が `Err` のときは、その場で `ratatui::try_restore()` を呼んでからエラーを返す。
  復元側のエラーは捨てる（元のエラーを優先して返すため。理由をコメントに書く）
- 復元は `Drop` で `ratatui::try_restore()` を呼ぶ。失敗したら `writeln!(io::stderr(), ..)` で伝え、その書き込みの失敗は無視する。
  `ratatui::restore()` と `eprintln!` は使わない（stderr への書き込みに失敗すると panic するため）
- カーソルは非表示にしない。マウスキャプチャも有効にしない。追加の復元処理は要らない
- `main` はガードが drop された後でエラーメッセージを出す。順序を保証するため、`tui::run` の戻り値を `main` で受けてから表示する

`main`:

- `fn main() -> ExitCode`。`cli::parse(std::env::args_os().skip(1))` の結果で分岐する
- 出力は `writeln!(io::stdout().lock(), ..)` / `writeln!(io::stderr().lock(), ..)`。`println!` / `eprintln!` は使わない
  （パイプの相手が先に閉じると `println!` は panic する）。書き込みの失敗は終了コード 1 にする
- エラーは `anyhow` の原因の連鎖を 1 行で出す（`{:#}`）

### 画面・キーバインド

画面は中央寄せの 2 行だけ。枠・色は付けない。

```
                               runs 0.1.0
                          q / Ctrl+C: quit
```

- `Layout` と `Paragraph`（中央寄せ）で配置する。`u16` の引き算を自分で書かない
- 高さが足りなければ上の行から表示し、幅が足りなければ切り捨てる（ratatui の既定の挙動に任せる）

| キー | 動作 | 衝突 |
|---|---|---|
| `q` | 終了 | なし（初めてのキーバインド） |
| Ctrl+C | 終了 | なし |

### 設定・CLI

```
runs 0.1.0

Usage: runs [OPTIONS]

Options:
  -h, --help     Print help
  -V, --version  Print version
```

- 引数は手書きで解釈する（懸念点 1）。`--help` と `--version` が両方あれば、先に書かれた方に従う
- UTF-8 でない引数も「知らない引数」として扱う（`OsString` のまま受け、表示は `to_string_lossy`）
- 終了コード: 0 = 正常、1 = 実行時エラー（端末でない・初期化失敗・I/O）、2 = 引数の誤り
- 設定ファイル・環境変数は追加しない

### 依存クレート

追加しない。テストも標準ライブラリと ratatui の `TestBackend` だけで書く（`insta`・`assert_cmd` は入れない）。

### テスト（`/tui-test` の層）

| 層 | ファイル | ケース |
|---|---|---|
| 1 | `src/app/tests.rs` | `q` で終了／Ctrl+C で終了／`q` の Release・Repeat は無視／Esc・`Q`・Alt+q・Ctrl+q・修飾なしの `c` は無視／Resize は無視／初期状態は終了でない |
| 1 | `src/cli/tests.rs` | 引数なし→Run／`--version`・`-V`／`--help`・`-h`／知らない引数→エラー／余分な引数→エラー／両方指定は先勝ち／UTF-8 でない引数→エラー（Unix のみ） |
| 2 | `src/ui/tests.rs` | 80x24 の画面全体を期待値と比較／10x3／1x1／0x0 で panic しない |
| 3 | `tests/cli.rs` | `--version` の stdout と終了コード 0／`--help` の終了コード 0／知らない引数で終了コード 2 と stderr／引数なし（テストでは stdin・stdout がパイプ）で終了コード 1・stderr にメッセージ・stdout が空 |
| 4 | 実機 | Windows Terminal と WSL Ubuntu: 起動→画面→`q`／Ctrl+C／リサイズ／極小サイズ／終了後のプロンプトとカーソル／panic 時の復元（下記） |

panic 時の復元の確認: 製品コードに panic を起こす仕掛けは入れない。確認のときだけイベントループに `panic!` を一時的に入れてビルドし、
実機で復元とメッセージを確認した後、その差分を捨てる（コミットしない）。

### ドキュメント

- `CLAUDE.md` の Architecture に上のモジュール表を反映する
- `.claude/skills/tui-test/SKILL.md` の「ヘルパーとモジュール構成はまだ無い」を、実際の配置とテストの書き方に置き換える

## 適用した規約・方針

| 規約・方針 | 適用内容 |
|---|---|
| CLAUDE.md Architecture | 状態（`app`）・更新（`app`）・描画（`ui`）を分け、端末なしでテストする。端末の初期化と復元は `tui::TerminalGuard` の 1 か所 |
| CLAUDE.md Conventions | 非テストコードで `unwrap` / `expect` / `panic!` を使わない。依存を追加しない |
| CLAUDE.md Things Claude gets wrong | Press 以外のキーイベントを無視。`u16` の引き算をしない。TUI 実行中に stdout / stderr へ出さない |
| rust-safety 1 章 | `anyhow::Result` と `.context(..)`。メッセージは端末の復元後に stderr へ。`println!` / `eprintln!` を使わない |
| rust-safety 2 章 | `try_init()` / `try_restore()` を土台にし、`Err` のときも復元。RAII ガード。Ctrl+C を終了キーとして扱う。復元の失敗で panic しない |
| rust-safety 3 章 | 知らない引数をエラーメッセージに出すときは制御文字を `?` に置き換える（引数は外部入力） |
| rust-safety 4 章 | 配置は `Layout` に任せる。文字列をバイト位置で切らない |
| rust-safety 5 章 | `#![forbid(unsafe_code)]` を維持 |
| rust-safety 8 章 | 依存を追加しない。crossterm は `ratatui::crossterm` を使う |
| /fix-bug のテストロック | テストを実装と別ファイルに置く |

## 懸念点（1〜3 は 2026-10-05 にユーザーが判断済み）

1. **引数の解釈を手書きにする** → 決定: 手書き。オプションが 2 つだけなので、手書き（30 行程度）で依存を増やさない。
   サブコマンドや値を取るオプションが増えたら clap に切り替える。
2. **画面と CLI のメッセージを英語（ASCII）にする** → 決定: 英語。CLAUDE.md が決めているのは応答とドキュメントの言語だけで、アプリの表示言語は決まっていなかった。
   土台の段階で全角の表示幅の問題を持ち込まない。日本語にするときは、表示幅のテストケースを足す。
3. **SIGTERM / SIGHUP・端末を閉じたときは復元されない** → 決定: この課題には入れず、#3 で扱う。
   Unix で `kill` されると raw mode のままシェルに戻ることがある。対応にはシグナル処理のクレート（signal-hook など）が要る。
4. **panic 時の復元は自動テストにできない**。ratatui の panic hook に依存し、証明は実機確認（一時的な `panic!`）だけになる。
   自動化には PTY を扱うクレートが要る。この課題では入れない。
5. **`try_init()` が最初の段階（raw mode の有効化）で失敗した場合**、`try_restore()` が alternate screen に入っていない端末へ離脱のシーケンスを書く。
   端末によってはカーソル位置が動く可能性がある（表示上の影響のみ。未確認）。
   stdin / stdout が端末であることを先に確認するので、この経路に入ることはまれと考える。
6. **`try_init()` の `Err` 経路と実行中の I/O エラーの経路は、実機で再現する手段が無い**。コードレビューでの確認にとどまる（未検証として報告する）。
7. **macOS は未検証のまま完了とする**（intent で合意済み）。
8. **impact-analyzer は使っていない**。既存コードが 5 行の `src/main.rs` だけで、直接読めば足りるため。

## intent の未解決の問い

- 引数パーサを手書きにするか、クレートを入れるか → 回答済み（手書き。懸念点 1）
- WSL の Ubuntu の Rust ツールチェーン → 回答済み（2026-10-05 に設定。rustc 1.99.0）
- macOS の確認手段 → 持ち越し（CLAUDE.md の未確定事項。この課題では未検証とする）
