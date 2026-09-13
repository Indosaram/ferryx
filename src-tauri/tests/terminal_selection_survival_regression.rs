use ferryx_lib::native_terminal::{
    MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize, NativeTerminal,
    TerminalEngine,
};

/// Regression test protecting active terminal text selection from being dropped or overwritten
/// during line text inspection (`line_text_at` / `cmd_native_terminal_line_at`).
///
/// Historical Regression:
/// Prior to the implementation of non-mutating line text extraction in libghostty-vt FFI,
/// querying line text (for Cmd+click link/file path resolution or hover detection) relied on
/// selecting the line via terminal line selection (`select_line_at`), which invoked
/// `install_selection` on the terminal handle. This interaction dropped and replaced the user's
/// active text selection with the full line text of whatever row was clicked or hovered over.
///
/// The fix in `src-tauri/src/native_terminal/selection.rs::line_text_at` introduced stack-isolated
/// selection querying using `ghostty_terminal_select_line` without calling `install_selection` on the
/// terminal handle, formatting the temporary selection directly via `format_selection(handle, Some(&selection), true)`.
///
/// This regression test proves that:
/// 1. An active word selection survives subsequent `line_text_at` queries on different rows as well as
///    the same row, retaining both its exact string content and selection range.
/// 2. An active pointer drag selection survives subsequent `line_text_at` queries across multiple rows,
///    retaining both its exact dragged text content and viewport selection bounds.
#[test]
fn test_terminal_text_selection_survives_line_inspection_regression() {
    let mut terminal = NativeTerminal::new(80, 24).expect("create native terminal");

    terminal
        .feed(b"first line has user-selected target text to copy\r\n")
        .expect("write row 0 text");
    terminal
        .feed(b"second line contains https://ferryx.dev/docs link\r\n")
        .expect("write row 1 text");
    terminal
        .feed(b"third line contains src/components/App.tsx:42:10 error\r\n")
        .expect("write row 2 text");

    // --- Phase 1: Word selection survival across line text inspection ---

    // Establish active word selection on row 0: select "target" (col 29)
    // "first line has user-selected target text to copy"
    //  012345678901234567890123456789
    // 'target' starts at col 29
    terminal
        .select_word_at(29, 0)
        .expect("select word 'target' on row 0");

    let initial_word_text = terminal
        .selection_text()
        .expect("query active word selection text")
        .expect("word selection must be present");
    assert_eq!(initial_word_text, "target");

    let initial_word_range = terminal
        .selection_range()
        .expect("query active word selection range")
        .expect("word selection range must be present");

    // Interaction that historically dropped selection:
    // User Cmd+clicks or hovers to inspect lines for links or file paths.
    // Query row 1 (URL)
    let line1 = terminal
        .line_text_at(21, 1)
        .expect("read line 1 for link extraction");
    assert!(
        line1.contains("https://ferryx.dev/docs"),
        "line 1 must contain the expected URL"
    );

    // Query row 2 (file path)
    let line2 = terminal
        .line_text_at(20, 2)
        .expect("read line 2 for file path extraction");
    assert!(
        line2.contains("src/components/App.tsx:42:10"),
        "line 2 must contain the expected file path"
    );

    // Query row 0 (the same row where active selection resides)
    let line0 = terminal
        .line_text_at(0, 0)
        .expect("read row 0 text");
    assert!(
        line0.contains("first line has user-selected target"),
        "line 0 must contain row 0 content"
    );

    // Assert: active word selection survived all line inspections
    let surviving_word_text = terminal
        .selection_text()
        .expect("query selection text after line inspection")
        .expect("word selection must still be present");
    assert_eq!(
        surviving_word_text, "target",
        "active word selection must survive line inspection without being dropped or replaced"
    );

    let surviving_word_range = terminal
        .selection_range()
        .expect("query selection range after line inspection")
        .expect("word selection range must still be present");
    assert_eq!(
        surviving_word_range, initial_word_range,
        "active word selection range must remain identical after line inspection"
    );

    // --- Phase 2: Pointer drag selection survival across line text inspection ---

    // Drag select "contains https://ferryx.dev" on row 1
    // row 1: "second line contains https://ferryx.dev/docs link"
    // 'contains' starts at col 12, 'https://ferryx.dev' ends around col 38
    let size = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };
    let event = |action, x, y| MouseEvent {
        action,
        button: (action == MouseAction::Press).then_some(MouseButton::Left),
        position: MousePosition { x, y },
        modifiers: Default::default(),
        size: Some(size),
        timestamp_ns: None,
    };

    // Press at col 12 (x = 125.0, y = 30.0 for row 1: y in [20.0, 40.0))
    terminal
        .handle_mouse_gesture(&event(MouseAction::Press, 125.0, 30.0))
        .expect("start drag selection on row 1");
    // Drag to col 39 (x = 395.0, y = 30.0)
    terminal
        .handle_mouse_gesture(&event(MouseAction::Motion, 395.0, 30.0))
        .expect("extend drag selection on row 1");
    // Release at col 39
    terminal
        .handle_mouse_gesture(&event(MouseAction::Release, 395.0, 30.0))
        .expect("finish drag selection on row 1");

    let initial_drag_text = terminal
        .selection_text()
        .expect("query active drag selection text")
        .expect("drag selection must be present");
    assert!(
        initial_drag_text.contains("contains https://ferryx.dev"),
        "drag selection text must contain dragged range, got: {:?}",
        initial_drag_text
    );

    let initial_drag_range = terminal
        .selection_range()
        .expect("query active drag selection range")
        .expect("drag selection range must be present");

    // Interact again: query row 2 and row 0
    let inspected_row2 = terminal
        .line_text_at(10, 2)
        .expect("read line 2 while drag selection active");
    assert!(inspected_row2.contains("src/components/App.tsx:42:10"));

    let inspected_row0 = terminal
        .line_text_at(5, 0)
        .expect("read line 0 while drag selection active");
    assert!(inspected_row0.contains("first line has"));

    // Assert: active drag selection survived line inspections
    let surviving_drag_text = terminal
        .selection_text()
        .expect("query selection text after second line inspection")
        .expect("drag selection must still be present");
    assert_eq!(
        surviving_drag_text, initial_drag_text,
        "active drag selection must survive line inspection without being dropped or replaced"
    );

    let surviving_drag_range = terminal
        .selection_range()
        .expect("query selection range after second line inspection")
        .expect("drag selection range must still be present");
    assert_eq!(
        surviving_drag_range, initial_drag_range,
        "active drag selection range must remain identical after line inspection"
    );
}
