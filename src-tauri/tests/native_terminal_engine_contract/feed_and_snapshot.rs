use ferryx_lib::native_terminal::{CellWide, ColorRgb, NativeTerminal, TerminalEngine};

#[test]
fn test_feed_ansi_text_and_snapshot_grid_and_cursor() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Feed bold red (palette index 1) "Hello" followed by reset and plain " World"
    term.feed_str("\x1b[31;1mHello\x1b[0m World")
        .expect("feed ANSI text");

    let (cx, cy) = term.cursor_position().expect("query cursor position");
    assert_eq!(
        cx, 11,
        "Cursor X must advance by 11 cells for 'Hello World'"
    );
    assert_eq!(cy, 0, "Cursor Y must remain on row 0");

    let snapshot = term.render_snapshot().expect("take render snapshot");
    assert_eq!(snapshot.cols, 80);
    assert_eq!(snapshot.rows, 24);
    assert_eq!(snapshot.cursor.x, 11);
    assert_eq!(snapshot.cursor.y, 0);

    // Verify cell content and styles
    let row0 = &snapshot.grid[0];
    assert_eq!(row0[0].text, "H");
    assert!(row0[0].bold, "'H' must have bold style");
    assert_eq!(
        row0[0].fg,
        Some(ColorRgb {
            r: 204,
            g: 102,
            b: 102
        }),
        "ANSI 31m must map to resolved palette red foreground"
    );

    assert_eq!(row0[1].text, "e");
    assert_eq!(row0[2].text, "l");
    assert_eq!(row0[3].text, "l");
    assert_eq!(row0[4].text, "o");
    assert!(row0[4].bold);

    assert_eq!(row0[5].text, " ");
    assert!(!row0[5].bold, "Space after reset must not be bold");

    assert_eq!(row0[6].text, "W");
    assert!(!row0[6].bold);
    assert_eq!(row0[7].text, "o");
    assert_eq!(row0[8].text, "r");
    assert_eq!(row0[9].text, "l");
    assert_eq!(row0[10].text, "d");
    assert!(!row0[10].bold);

    // Row text convenience helper
    let line0_text = snapshot.row_text(0);
    assert!(
        line0_text.starts_with("Hello World"),
        "Row 0 rendered text must start with 'Hello World', got: {line0_text:?}"
    );
}

#[test]
fn test_feed_unicode_cjk_emoji_combining_mark_no_truncation() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Feed CJK (東京), Emoji (🦀🚀), and combining mark (e + \u{0301} = é)
    let unicode_input = "東京 🦀 e\u{0301}!";
    term.feed_str(unicode_input).expect("feed unicode text");

    let snapshot = term.render_snapshot().expect("take render snapshot");

    let row0 = &snapshot.grid[0];

    // CJK character 東 (wide char at col 0, spacer tail at col 1)
    assert_eq!(row0[0].text, "東");
    assert_eq!(row0[0].wide, CellWide::Wide);
    assert_eq!(row0[1].wide, CellWide::SpacerTail);

    // CJK character 京 (wide char at col 2, spacer tail at col 3)
    assert_eq!(row0[2].text, "京");
    assert_eq!(row0[2].wide, CellWide::Wide);
    assert_eq!(row0[3].wide, CellWide::SpacerTail);

    // Space at col 4
    assert_eq!(row0[4].text, " ");
    assert_eq!(row0[4].wide, CellWide::Narrow);

    // Emoji 🦀 (wide char at col 5, spacer tail at col 6)
    assert_eq!(row0[5].text, "🦀");
    assert_eq!(row0[5].wide, CellWide::Wide);
    assert_eq!(row0[6].wide, CellWide::SpacerTail);

    // Space at col 7
    assert_eq!(row0[7].text, " ");
    assert_eq!(row0[7].wide, CellWide::Narrow);

    // Combining mark at col 8: grapheme cluster "e\u{0301}"
    assert_eq!(
        row0[8].text, "e\u{0301}",
        "Combining mark must be preserved in grapheme cluster without truncation"
    );
    assert_eq!(row0[8].wide, CellWide::Narrow);

    // Exclamation mark at col 9
    assert_eq!(row0[9].text, "!");
    assert_eq!(row0[9].wide, CellWide::Narrow);

    // Cursor position should be at column 10
    let (cx, cy) = term.cursor_position().expect("query cursor position");
    assert_eq!(
        cx, 10,
        "Cursor X must be at column 10 after wide chars and graphemes"
    );
    assert_eq!(cy, 0);
}

#[test]
fn test_raw_pty_feed_allows_nul_bytes_and_remains_functional() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Feed raw PTY bytes with embedded NUL bytes.
    let raw_payload_with_nul = b"Hello\0World\0!\r\nSecond Line";
    term.feed(raw_payload_with_nul)
        .expect("feed with embedded NUL bytes must succeed");

    let snapshot = term
        .render_snapshot()
        .expect("take snapshot after NUL feed");
    assert!(
        snapshot.row_text(0).starts_with("HelloWorld!"),
        "Row 0 must contain 'HelloWorld!' with NUL bytes stripped/ignored by VT parser, got: {:?}",
        snapshot.row_text(0)
    );
    assert!(
        snapshot.row_text(1).starts_with("Second Line"),
        "Row 1 must contain 'Second Line', got: {:?}",
        snapshot.row_text(1)
    );

    // Terminal must remain functional for subsequent feeds and queries
    term.feed_str("\r\nThird Line")
        .expect("subsequent feed must succeed");
    let snapshot2 = term.render_snapshot().expect("take snapshot 2");
    assert!(snapshot2.row_text(2).starts_with("Third Line"));
}

#[test]
fn test_render_snapshot_copied_data_outlives_mutations() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));
    term.feed_str("Initial State").expect("feed text");

    let snapshot1 = term.render_snapshot().expect("take snapshot 1");
    assert!(snapshot1.row_text(0).starts_with("Initial State"));

    // Mutate terminal by overwriting row 0
    term.feed_str("\r\x1b[KMutated Text")
        .expect("overwrite row 0");

    let snapshot2 = term.render_snapshot().expect("take snapshot 2");
    assert!(snapshot2.row_text(0).starts_with("Mutated Text"));

    // Assert snapshot1 is completely unchanged and independent
    assert!(
        snapshot1.row_text(0).starts_with("Initial State"),
        "Previous snapshot must retain copied data after subsequent terminal mutations"
    );
}
