use ferryx_lib::native_terminal::{NativeTerminal, NativeTerminalError, TerminalEngine};

#[test]
fn test_terminal_creation_and_default_dimensions_via_trait() {
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("Failed to create native terminal 80x24"));
    assert_eq!(
        term.dimensions().expect("query dimensions"),
        (80, 24),
        "Terminal dimensions must match initial parameters"
    );
    assert_eq!(term.cols().expect("query cols"), 80);
    assert_eq!(term.rows().expect("query rows"), 24);

    let (cx, cy) = term.cursor_position().expect("query cursor position");
    assert_eq!(cx, 0, "Initial cursor column must be 0");
    assert_eq!(cy, 0, "Initial cursor row must be 0");

    let cursor = term.cursor_state().expect("query cursor state");
    assert_eq!(cursor.x, 0);
    assert_eq!(cursor.y, 0);
    assert!(cursor.visible, "Cursor should be visible by default");

    // Negative dimension checks
    let err_zero_cols = NativeTerminal::new(0, 24);
    assert!(
        matches!(
            err_zero_cols,
            Err(NativeTerminalError::InvalidDimensions(0, 24))
        ),
        "Creating terminal with 0 cols must fail with InvalidDimensions"
    );

    let err_zero_rows = NativeTerminal::new(80, 0);
    assert!(
        matches!(
            err_zero_rows,
            Err(NativeTerminalError::InvalidDimensions(80, 0))
        ),
        "Creating terminal with 0 rows must fail with InvalidDimensions"
    );
}

#[test]
fn test_terminal_resize() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));
    term.feed_str("Line 1\r\nLine 2").expect("feed text");

    let (cols_orig, rows_orig) = term.dimensions().expect("query dimensions");
    assert_eq!((cols_orig, rows_orig), (80, 24));

    // Resize to 120 cols x 40 rows with cell pixel dimensions
    term.resize(120, 40, 10, 20).expect("resize terminal");

    let (cols_new, rows_new) = term.dimensions().expect("query new dimensions");
    assert_eq!((cols_new, rows_new), (120, 40));
    assert_eq!(term.cols().expect("query cols"), 120);
    assert_eq!(term.rows().expect("query rows"), 40);

    let snapshot = term.render_snapshot().expect("snapshot after resize");
    assert_eq!(snapshot.cols, 120);
    assert_eq!(snapshot.rows, 40);
    assert_eq!(snapshot.grid.len(), 40);
    assert_eq!(snapshot.grid[0].len(), 120);

    assert!(snapshot.row_text(0).starts_with("Line 1"));
    assert!(snapshot.row_text(1).starts_with("Line 2"));

    // Negative resize check: zero dimensions
    let err_zero_cols = term.resize(0, 40, 10, 20);
    assert!(
        matches!(
            err_zero_cols,
            Err(NativeTerminalError::InvalidDimensions(0, 40))
        ),
        "Resize with 0 cols must fail with InvalidDimensions"
    );

    let err_zero_rows = term.resize(120, 0, 10, 20);
    assert!(
        matches!(
            err_zero_rows,
            Err(NativeTerminalError::InvalidDimensions(120, 0))
        ),
        "Resize with 0 rows must fail with InvalidDimensions"
    );
}

#[test]
fn test_title_update_and_query() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // Initially title is empty
    let initial_title = term.title().expect("query initial title");
    assert_eq!(initial_title, "", "Initial title must be empty");
    assert!(!term.take_title_changed());

    // Feed OSC 0 title sequence
    term.feed_str("\x1b]0;Orca Phase 1\x07")
        .expect("feed OSC 0 title");
    assert!(
        term.take_title_changed(),
        "Title changed event must be flagged after OSC 0 sequence"
    );
    assert!(!term.take_title_changed(), "Subsequent check must be false");
    let title1 = term.title().expect("query updated title");
    assert_eq!(
        title1, "Orca Phase 1",
        "Title must update after OSC 0 sequence"
    );

    // Feed OSC 2 title sequence with ST terminator (\x1b\\)
    term.feed_str("\x1b]2;Ferryx Engine\x1b\\")
        .expect("feed OSC 2 title");
    assert!(term.take_title_changed());
    let title2 = term.title().expect("query updated title");
    assert_eq!(
        title2, "Ferryx Engine",
        "Title must update after OSC 2 sequence"
    );

    // Set title via direct API
    term.set_title("Direct Title Update")
        .expect("set title via API");
    let title3 = term.title().expect("query updated title");
    assert_eq!(
        title3, "Direct Title Update",
        "Title must match directly set value"
    );
}

#[test]
fn test_bell_event_observation_and_counter() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));
    assert_eq!(term.bell_count(), 0, "Initial bell count must be zero");

    // Feed BEL character (\x07)
    term.feed_str("Command\x07").expect("feed BEL");
    assert_eq!(term.bell_count(), 1, "Bell count must increment on BEL");

    // Feed multiple BELs
    term.feed(b"\x07\x07\x07").expect("feed 3 BELs");
    assert_eq!(term.bell_count(), 4, "Bell count must accumulate");

    // Take bell count resets counter to zero
    let taken = term.take_bell_count();
    assert_eq!(taken, 4);
    assert_eq!(term.bell_count(), 0);
}

#[test]
fn test_high_iteration_create_feed_snapshot_drop_lifecycle() {
    // 50 iterations of complete terminal lifecycle via TerminalEngine trait
    for i in 0..50 {
        let mut term: Box<dyn TerminalEngine> = Box::new(
            NativeTerminal::new(80, 24)
                .unwrap_or_else(|e| panic!("iteration {i} creation failed: {e}")),
        );
        term.feed_str(&format!("Iteration {i} text\r\n"))
            .unwrap_or_else(|e| panic!("iteration {i} feed failed: {e}"));

        let snapshot = term
            .render_snapshot()
            .unwrap_or_else(|e| panic!("iteration {i} snapshot failed: {e}"));
        assert!(snapshot
            .row_text(0)
            .starts_with(&format!("Iteration {i} text")));

        term.resize(100, 30, 10, 20)
            .unwrap_or_else(|e| panic!("iteration {i} resize failed: {e}"));

        // Dropped at end of loop iteration
    }
}
