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

// --- 文字コード（#8） --------------------------------------------------------------

/// `127.0.0.1 からの応答` の「からの応答」を CP932（Shift_JIS）で表したバイト列。
const KARANO_OUTOU_CP932: &[u8] = &[0x82, 0xa9, 0x82, 0xe7, 0x82, 0xcc, 0x89, 0x9e, 0x93, 0x9a];

#[test]
fn cp932_line_is_decoded_with_fallback() {
    // Windows の ping などは CP932 で出力する。UTF-8 として不正なら既定の文字コードで読み直す
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw(&[b"127.0.0.1 ".as_slice(), KARANO_OUTOU_CP932].concat());

    assert_eq!(
        buffer.lines().collect::<Vec<_>>(),
        vec!["127.0.0.1 からの応答"]
    );
}

#[test]
fn utf8_line_is_kept_even_with_fallback() {
    // UTF-8 の出力（cargo など）は読み直さない。CP932 として読めてしまう列でも UTF-8 を優先する
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw("日本語 🦀 é".as_bytes());

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["日本語 🦀 é"]);
}

#[test]
fn mixed_lines_are_decoded_per_line() {
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw("ビルド完了".as_bytes());
    buffer.push_raw(KARANO_OUTOU_CP932);

    assert_eq!(
        buffer.lines().collect::<Vec<_>>(),
        vec!["ビルド完了", "からの応答"]
    );
}

#[test]
fn without_fallback_invalid_bytes_become_replacement_char() {
    let mut buffer = OutputBuffer::with_fallback(10, None);

    buffer.push_raw(KARANO_OUTOU_CP932);

    let line = buffer.lines().next().expect("1 行ある");
    assert!(line.contains('\u{fffd}'), "{line:?}");
    assert!(!line.contains("からの応答"), "{line:?}");
}

#[test]
fn cp932_bytes_that_happen_to_be_valid_utf8_are_not_redecoded() {
    // 既知の制約: 半角カナ「ﾃｱ」（C3 B1）だけの行は UTF-8 の「ñ」として正しいので読み直さない（行単位の判定）
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw(&[0xc3, 0xb1]);

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["ñ"]);
}

#[test]
fn control_chars_produced_by_fallback_decoding_are_sanitized() {
    // Shift_JIS の 0x80 は U+0080（C1 制御文字）になる。読み直した後も `?` に置き換える
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw(&[0x80, 0x82, 0xa9]);

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["?か"]);
}

#[test]
fn decode_returns_borrowed_for_valid_utf8() {
    // UTF-8 の行はコピーしない（大量の出力で無駄にしない）
    let decoded = decode(b"plain ascii", Some(encoding_rs::SHIFT_JIS));

    assert!(matches!(decoded, std::borrow::Cow::Borrowed(_)));
    assert_eq!(decoded, "plain ascii");
}

#[test]
fn fallback_decoding_keeps_sanitizing() {
    // 読み直した行にも制御文字の除去がかかる（ESC [ 31 m を CP932 の文字列の前に置く）
    let mut buffer = OutputBuffer::with_fallback(10, Some(encoding_rs::SHIFT_JIS));

    buffer.push_raw(&[b"\x1b[31m".as_slice(), KARANO_OUTOU_CP932].concat());

    assert_eq!(buffer.lines().collect::<Vec<_>>(), vec!["からの応答"]);
}
