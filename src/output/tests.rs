use super::*;

#[test]
fn strips_csi_sequences() {
    // 色と カーソル移動。最終バイト（0x40..=0x7E）までを捨てる
    assert_eq!(sanitize("\u{1b}[31mred\u{1b}[0m plain"), "red plain");
    assert_eq!(sanitize("a\u{1b}[2J\u{1b}[1;1Hb"), "ab");
    assert_eq!(sanitize("\u{1b}[?25l\u{1b}[38;5;196mx"), "x");
}

#[test]
fn strips_osc_with_bel_and_st() {
    // ウィンドウタイトルの変更。終端は BEL か ESC \
    assert_eq!(sanitize("\u{1b}]0;evil title\u{7}text"), "text");
    assert_eq!(sanitize("\u{1b}]52;c;base64\u{1b}\\text"), "text");
}

#[test]
fn strips_lone_escape() {
    // ESC + 1 文字（例: ESC 7 でカーソル保存）
    assert_eq!(sanitize("a\u{1b}7b"), "ab");
    // 末尾の ESC だけ
    assert_eq!(sanitize("a\u{1b}"), "a");
}

#[test]
fn strips_escape_with_intermediate_bytes() {
    // 文字セットの指定（tput sgr0 などが出す）。`B` を残さない
    assert_eq!(sanitize("a\u{1b}(Bb"), "ab");
    assert_eq!(sanitize("a\u{1b}#8b"), "ab");
}

#[test]
fn strips_dcs_apc_pm_sos_strings() {
    assert_eq!(sanitize("a\u{1b}Pq#0;2;0;0;0#0!30~\u{1b}\\b"), "ab");
    assert_eq!(sanitize("a\u{1b}_payload\u{7}b"), "ab");
    assert_eq!(sanitize("a\u{1b}^x\u{1b}\\b\u{1b}Xy\u{1b}\\c"), "abc");
}

#[test]
fn replaces_control_chars() {
    // C0・DEL・C1 は '?' にする
    assert_eq!(sanitize("a\u{1}b\u{7f}c\u{9b}d"), "a?b?c?d");
    // 改行は行の区切りとして別に扱うので、ここでも '?' になる
    assert_eq!(sanitize("a\nb"), "a?b");
}

#[test]
fn expands_tab() {
    assert_eq!(sanitize("a\tb"), "a    b");
}

#[test]
fn keeps_text_after_last_carriage_return() {
    // プログレスバーのような \r による上書きは最終状態だけ残す
    assert_eq!(sanitize("10%\r20%\r100%"), "100%");
    assert_eq!(sanitize("10%\r20%\r100%\r"), "100%");
}

#[test]
fn trailing_carriage_return_is_line_ending_not_overwrite() {
    // Windows のプログラムは CRLF で出力する。行末の \r で行を空にしない
    assert_eq!(sanitize("done\r"), "done");
}

#[test]
fn leaves_unicode_untouched() {
    assert_eq!(sanitize("日本語 🦀 é"), "日本語 🦀 é");
}

#[test]
fn lossy_utf8_becomes_replacement_char() {
    let mut buffer = OutputBuffer::new(10);

    buffer.push_raw(&[b'a', 0xff, b'b']);

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["a\u{fffd}b"]);
}

#[test]
fn push_raw_sanitizes() {
    let mut buffer = OutputBuffer::new(10);

    buffer.push_raw(b"\x1b[31mred\x1b[0m");

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["red"]);
}

#[test]
fn drops_oldest_lines_over_limit() {
    let mut buffer = OutputBuffer::new(3);

    for i in 1..=5 {
        buffer.push_raw(i.to_string().as_bytes());
    }

    assert_eq!(buffer.len(), 3);
    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["3", "4", "5"]);
    assert_eq!(buffer.dropped(), 2);
}

#[test]
fn clear_empties_buffer() {
    let mut buffer = OutputBuffer::new(3);
    buffer.push_raw(b"a");
    buffer.push_raw(b"b");

    buffer.clear();

    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.dropped(), 0);
    assert!(buffer.lines().next().is_none());
}

#[test]
fn zero_limit_keeps_nothing() {
    let mut buffer = OutputBuffer::new(0);

    buffer.push_raw(b"a");

    assert_eq!(buffer.len(), 0);
    assert_eq!(buffer.dropped(), 1);
}
