use ferryx_lib::native_terminal::{NativeTerminal, TerminalEngine};

#[test]
fn test_cursor_movement_sequences() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Move cursor to absolute position row 5, col 10 (1-indexed -> 0-indexed: x=9, y=4)
    term.feed_str("\x1b[5;10H").expect("feed CUP");
    assert_eq!(term.cursor_position().expect("cursor pos"), (9, 4));

    // Move cursor up 2 rows (CUU) -> y=2
    term.feed_str("\x1b[2A").expect("feed CUU");
    assert_eq!(term.cursor_position().expect("cursor pos"), (9, 2));

    // Move cursor back/left 4 cols (CUB) -> x=5
    term.feed_str("\x1b[4D").expect("feed CUB");
    assert_eq!(term.cursor_position().expect("cursor pos"), (5, 2));

    // Move cursor down 3 rows (CUD) -> y=5
    term.feed_str("\x1b[3B").expect("feed CUD");
    assert_eq!(term.cursor_position().expect("cursor pos"), (5, 5));

    // Move cursor forward/right 5 cols (CUF) -> x=10
    term.feed_str("\x1b[5C").expect("feed CUF");
    assert_eq!(term.cursor_position().expect("cursor pos"), (10, 5));
}

#[test]
fn test_alternate_screen_enter_exit_restoration() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Write text to primary screen
    term.feed_str("Primary Screen Data").expect("write primary");
    let snap_primary = term.render_snapshot().expect("snap primary");
    assert!(snap_primary.row_text(0).starts_with("Primary Screen Data"));

    // Enter alternate screen buffer (DECSET 1049) and home cursor
    term.feed_str("\x1b[?1049h\x1b[H")
        .expect("enter alt screen");
    term.feed_str("Alternate Screen Content")
        .expect("write alt");

    let snap_alt = term.render_snapshot().expect("snap alt");
    assert!(snap_alt.row_text(0).starts_with("Alternate Screen Content"));

    // Exit alternate screen buffer (DECRST 1049)
    term.feed_str("\x1b[?1049l").expect("exit alt screen");

    let snap_restored = term.render_snapshot().expect("snap restored");
    assert!(
        snap_restored.row_text(0).starts_with("Primary Screen Data"),
        "Primary screen contents must be fully restored after exiting alternate screen"
    );
}

#[test]
fn test_scrollback_after_enough_output() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Feed 50 lines (exceeding 24-row viewport)
    for i in 1..=50 {
        term.feed_str(&format!("Line {i:02}\r\n"))
            .expect("feed line");
    }

    let snapshot = term.render_snapshot().expect("snapshot after scrollback");
    // Visible viewport row 22 should show latest output
    assert!(snapshot.row_text(22).starts_with("Line 50"));
}

#[test]
fn test_resize_reflow_preservation() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(10, 5).expect("create 10x5 terminal"));
    // Write 20 characters on 10-wide terminal -> wraps to row 1
    term.feed_str("1234567890abcdefghij")
        .expect("feed wrap text");

    let snap_narrow = term.render_snapshot().expect("snap narrow");
    assert_eq!(snap_narrow.row_text(0), "1234567890");
    assert_eq!(snap_narrow.row_text(1), "abcdefghij");

    // Resize wider to 30 columns -> reflows into single row
    term.resize(30, 5, 10, 20).expect("resize 30x5");
    let snap_wide = term.render_snapshot().expect("snap wide");
    assert!(snap_wide.row_text(0).starts_with("1234567890abcdefghij"));
}
