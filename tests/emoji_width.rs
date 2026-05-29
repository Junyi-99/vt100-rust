// Regression tests for emoji-presentation width.
//
// A text-presentation base codepoint followed by U+FE0F (VARIATION
// SELECTOR-16) requests *emoji presentation*, which real terminals, tmux,
// and `unicode-width`'s string-level `width()` all render as two columns.
// vt100 historically decided a cell's wide flag from the base char alone
// (width 1), so the VS16 sequence was stored as a single narrow cell. That
// left every column after the emoji shifted by one, producing on-screen
// residue. These tests pin the corrected, two-column layout.

fn cells(parser: &vt100::Parser, n: u16) -> Vec<(String, bool, bool)> {
    let screen = parser.screen();
    (0..n)
        .map(|col| {
            let c = screen.cell(0, col).unwrap();
            (
                c.contents().to_string(),
                c.is_wide(),
                c.is_wide_continuation(),
            )
        })
        .collect()
}

#[test]
fn vs16_emoji_occupies_two_columns() {
    let mut parser = vt100::Parser::new(1, 20, 0);
    // "A" + ❤️ (U+2764 U+FE0F) + "B"
    parser.process("A\u{2764}\u{FE0F}B".as_bytes());

    let c = cells(&parser, 5);
    assert_eq!(c[0], ("A".into(), false, false), "col0 = A");
    assert_eq!(
        c[1],
        ("\u{2764}\u{FE0F}".into(), true, false),
        "col1 = ❤️ and must be WIDE"
    );
    assert_eq!(
        c[2],
        (String::new(), false, true),
        "col2 must be a wide continuation"
    );
    assert_eq!(c[3], ("B".into(), false, false), "col3 = B (not shifted)");

    // cursor lands after B: A(1) + emoji(2) + B(1) = col 4
    assert_eq!(parser.screen().cursor_position(), (0, 4));
}

#[test]
fn warning_sign_vs16_occupies_two_columns() {
    let mut parser = vt100::Parser::new(1, 20, 0);
    // ⚠️ = U+26A0 U+FE0F
    parser.process("\u{26A0}\u{FE0F}X".as_bytes());
    let c = cells(&parser, 3);
    assert!(c[0].1, "⚠️ must be wide");
    assert!(c[1].2, "continuation after ⚠️");
    assert_eq!(c[2].0, "X");
}

#[test]
fn heart_without_vs16_stays_single_width() {
    // No regression: a bare U+2764 (text presentation) is genuinely 1 column.
    let mut parser = vt100::Parser::new(1, 20, 0);
    parser.process("A\u{2764}B".as_bytes());
    let c = cells(&parser, 3);
    assert_eq!(
        c[1],
        ("\u{2764}".into(), false, false),
        "bare heart is narrow"
    );
    assert_eq!(c[2], ("B".into(), false, false), "B immediately follows");
}

#[test]
fn default_emoji_presentation_unaffected() {
    // 😀 (U+1F600) is already wide by base-char width; behaviour unchanged.
    let mut parser = vt100::Parser::new(1, 20, 0);
    parser.process("\u{1F600}Y".as_bytes());
    let c = cells(&parser, 3);
    assert!(c[0].1, "grinning face is wide");
    assert!(c[1].2, "continuation");
    assert_eq!(c[2].0, "Y");
}
