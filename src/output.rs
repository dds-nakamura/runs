//! コマンドの出力の保持（上限付き）と、表示前の無害化。端末にもプロセスにも触らない。

use std::collections::VecDeque;

/// 1 コマンドあたりに保持する行数。
pub const DEFAULT_LIMIT: usize = 10_000;

pub struct OutputBuffer {
    lines: VecDeque<String>,
    limit: usize,
    dropped: usize,
}

impl OutputBuffer {
    pub fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            limit,
            dropped: 0,
        }
    }

    /// 子プロセスから届いた 1 行（改行を含まない生のバイト列）を無害化して追加する。
    pub fn push_raw(&mut self, bytes: &[u8]) {
        self.lines
            .push_back(sanitize(&String::from_utf8_lossy(bytes)));
        while self.lines.len() > self.limit {
            self.lines.pop_front();
            self.dropped += 1;
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 上限を超えて捨てた行数（テスト用）。
    #[cfg(test)]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.dropped = 0;
    }
}

/// 信頼できない文字列から、端末を操作しうるものを取り除く。
///
/// - ESC シーケンス（CSI `ESC [ … 最終バイト`、OSC `ESC ] … BEL | ESC \`、その他の `ESC` + 1 文字）は捨てる
/// - 行末の `\r` は CRLF の名残として落とす。残る `\r` は上書きとみなし、最後の `\r` より後だけを残す
///   （プログレスバーは最終状態だけになる）
/// - タブは空白 4 つ、残る制御文字（C0・DEL・C1）は `?` にする
pub fn sanitize(text: &str) -> String {
    let text = text.strip_suffix('\r').unwrap_or(text);
    let text = text.rsplit('\r').next().unwrap_or(text);

    let mut out = String::with_capacity(text.len());
    let mut state = State::Normal;
    for c in text.chars() {
        state = match state {
            State::Normal => match c {
                '\u{1b}' => State::Escape,
                '\t' => {
                    out.push_str("    ");
                    State::Normal
                }
                _ if c.is_control() => {
                    out.push('?');
                    State::Normal
                }
                _ => {
                    out.push(c);
                    State::Normal
                }
            },
            State::Escape => match c {
                '[' => State::Csi,
                ']' => State::Osc,
                _ => State::Normal,
            },
            // パラメータ（0x30..=0x3F）と中間バイト（0x20..=0x2F）を読み飛ばし、最終バイトで終わる
            State::Csi => match c {
                '\u{40}'..='\u{7e}' => State::Normal,
                _ => State::Csi,
            },
            State::Osc => match c {
                '\u{7}' => State::Normal,
                '\u{1b}' => State::OscEscape,
                _ => State::Osc,
            },
            // OSC の中の ESC は、次が `\` なら終端（ST）。そうでなければ別のシーケンスの始まりとして読み直す
            State::OscEscape => match c {
                '\\' => State::Normal,
                '[' => State::Csi,
                ']' => State::Osc,
                _ => State::Normal,
            },
        };
    }
    out
}

#[derive(Clone, Copy)]
enum State {
    Normal,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

#[cfg(test)]
mod tests;
