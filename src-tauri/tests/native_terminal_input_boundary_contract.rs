use std::borrow::Cow;

use ferryx_lib::daemon::{DaemonAttachment, DaemonStreamMessage};
use ferryx_lib::ipc::native_terminal::{
    copy_attached_native_selection, encode_attached_native_input, encode_attached_native_mouse,
    encode_attached_native_paste, scroll_attached_native_terminal,
    scrollbar_for_attached_native_terminal, search_attached_native_terminal,
    select_attached_native_terminal, NativeTerminalScrollBehavior, NativeTerminalSelectMode,
};
use ferryx_lib::native_terminal::surface_host::NativeTerminalSurfaceHostState;
use ferryx_lib::native_terminal::{
    KeyModifiers, MouseAction, MouseButton, MouseEvent, MousePosition, NativeTerminal,
    NativeTerminalError, NativeTerminalInput, ScrollViewport, TerminalEngine,
};

fn create_attachment(
    session_id: &str,
) -> (
    tokio::sync::mpsc::Sender<ferryx_lib::daemon::DaemonStreamMessage<'static>>,
    DaemonAttachment,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let stream_task = tokio::spawn(std::future::pending());
    let attachment = DaemonAttachment {
        session_id: session_id.to_string(),
        epoch: 1,
        start_sequence: None,
        end_sequence: None,
        gap: None,
        history: Vec::new(),
        history_segments: Vec::new(),
        pty_cols: None,
        pty_rows: None,
        messages: rx,
        stream_task,
    };
    (tx, attachment)
}

#[tokio::test]
async fn production_input_boundary_rejects_detached_session_without_resurrecting_native_state() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-input-owner";
    let (_tx, attachment) = create_attachment(session_id);

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach native daemon stream state");

    let text_input = NativeTerminalInput::Text {
        text: Cow::Borrowed("echo attached\n").into_owned(),
    };
    assert_eq!(
        encode_attached_native_input(&state, session_id, &text_input)
            .expect("attached input encodes"),
        b"echo attached\n"
    );

    let key_input: NativeTerminalInput = serde_json::from_str(
        r#"{
        "keyEvent": {
            "key": "Enter",
            "action": "Press",
            "modifiers": {
                "shift": false,
                "ctrl": false,
                "alt": false,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }
    }"#,
    )
    .expect("deserialize enter key event");

    let encoded_key = encode_attached_native_input(&state, session_id, &key_input)
        .expect("attached key input encodes");
    assert!(!encoded_key.is_empty(), "enter key must encode to bytes");

    state.detach_session(session_id);

    // Text input to detached session must fail and not recreate state
    assert!(matches!(
        encode_attached_native_input(&state, session_id, &text_input),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state.ensure_surface_attached(session_id).is_err(),
        "rejected production text input must not re-attach the detached surface"
    );

    // Key event to detached session must also fail and not recreate state
    assert!(matches!(
        encode_attached_native_input(&state, session_id, &key_input),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state.ensure_surface_attached(session_id).is_err(),
        "rejected production key input must not re-attach the detached surface"
    );
}

#[tokio::test]
async fn production_input_boundary_rejects_unattached_session_without_creating_native_state() {
    let state = NativeTerminalSurfaceHostState::default();
    let unattached_id = "never-attached-session";

    let text_input = NativeTerminalInput::Text {
        text: "hello".to_string(),
    };
    assert!(matches!(
        encode_attached_native_input(&state, unattached_id, &text_input),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state
            .snapshot_for_session(unattached_id)
            .expect("snapshot lookup for unattached session")
            .is_none(),
        "unattached session must not lazily construct native state"
    );

    let key_input: NativeTerminalInput = serde_json::from_str(
        r#"{
        "keyEvent": {
            "key": "a",
            "action": "Press",
            "modifiers": {
                "shift": false,
                "ctrl": false,
                "alt": false,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }
    }"#,
    )
    .expect("deserialize key event");

    assert!(matches!(
        encode_attached_native_input(&state, unattached_id, &key_input),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state
            .snapshot_for_session(unattached_id)
            .expect("snapshot lookup after key input on unattached session")
            .is_none(),
        "unattached session must not lazily construct native state on key event"
    );
}

#[tokio::test]
async fn production_input_boundary_rejects_invalid_session_id() {
    let state = NativeTerminalSurfaceHostState::default();
    let text_input = NativeTerminalInput::Text {
        text: "hello".to_string(),
    };

    assert!(matches!(
        encode_attached_native_input(&state, "", &text_input),
        Err(NativeTerminalError::InvalidValue(_))
    ));
    assert!(matches!(
        encode_attached_native_input(&state, "   ", &text_input),
        Err(NativeTerminalError::InvalidValue(_))
    ));
}

#[tokio::test]
async fn production_paste_boundary_rejects_detached_and_unattached_session() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-paste-session";
    let (_tx, attachment) = create_attachment(session_id);

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach daemon stream state");

    let pasted = encode_attached_native_paste(&state, session_id, "hello paste")
        .expect("attached paste encodes");
    assert_eq!(pasted, b"hello paste");

    state.detach_session(session_id);

    // Paste to detached session must fail and not recreate state
    assert!(matches!(
        encode_attached_native_paste(&state, session_id, "after detach"),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state.ensure_surface_attached(session_id).is_err(),
        "rejected paste must not re-attach the detached surface"
    );

    // Paste to unattached session must fail
    assert!(matches!(
        encode_attached_native_paste(&state, "unattached-session", "test"),
        Err(NativeTerminalError::NoValue)
    ));
}

#[tokio::test]
async fn production_mouse_boundary_rejects_detached_and_unattached_session() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-mouse-session";
    let (_tx, attachment) = create_attachment(session_id);

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach daemon stream state");

    let mouse_event = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition { x: 10.0, y: 20.0 },
        modifiers: KeyModifiers::default(),
        size: None,
    };

    let encoded = encode_attached_native_mouse(&state, session_id, &mouse_event)
        .expect("attached mouse encodes");
    // Initial mode without mouse tracking returns empty bytes (untracked)
    assert!(encoded.is_empty());

    state.detach_session(session_id);

    // Mouse event to detached session must fail and not recreate state
    assert!(matches!(
        encode_attached_native_mouse(&state, session_id, &mouse_event),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(
        state.ensure_surface_attached(session_id).is_err(),
        "rejected mouse must not re-attach the detached surface"
    );

    // Mouse to unattached session must fail
    assert!(matches!(
        encode_attached_native_mouse(&state, "unattached-session", &mouse_event),
        Err(NativeTerminalError::NoValue)
    ));
}

#[test]
fn paste_encoding_honors_bracketed_paste_mode_at_boundary() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");

    // Plain paste when bracketed-paste is off
    let plain = term.encode_paste("echo foo").expect("encode plain paste");
    assert_eq!(plain, b"echo foo");

    // Enable bracketed paste mode (DEC mode 2004)
    term.feed_str("\x1b[?2004h")
        .expect("enable bracketed paste mode");
    let bracketed = term
        .encode_paste("echo foo")
        .expect("encode bracketed paste");
    assert_eq!(bracketed, b"\x1b[200~echo foo\x1b[201~");
}

#[test]
fn search_boundary_returns_match_coordinates_and_handles_absent() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    term.feed_str("hello search world\r\nsecond search line\r\n")
        .expect("feed text for search");

    let matches = term
        .search_grid("search", true)
        .expect("search grid for 'search'");
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0], (0, 6, 11));
    assert_eq!(matches[1], (1, 7, 12));

    let absent = term
        .search_grid("nonexistent", true)
        .expect("search for absent needle");
    assert!(absent.is_empty());
}

#[test]
fn scroll_boundary_maps_each_behavior_variant_to_engine() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    let lines = (0..50)
        .map(|i| format!("row-{i:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    term.feed_str(&lines).expect("feed lines");

    // Top
    let top_variant = NativeTerminalScrollBehavior::Top;
    assert_eq!(top_variant.to_scroll_viewport(), ScrollViewport::Top);
    term.scroll_viewport(top_variant.to_scroll_viewport())
        .expect("scroll to top");
    assert!(term
        .render_snapshot()
        .expect("snapshot")
        .row_text(0)
        .starts_with("row-000"));

    // Bottom
    let bottom_variant = NativeTerminalScrollBehavior::Bottom;
    assert_eq!(bottom_variant.to_scroll_viewport(), ScrollViewport::Bottom);
    term.scroll_viewport(bottom_variant.to_scroll_viewport())
        .expect("scroll to bottom");
    assert!(term
        .render_snapshot()
        .expect("snapshot")
        .row_text(0)
        .starts_with("row-044"));

    // Delta
    let delta_variant = NativeTerminalScrollBehavior::Delta { rows: -10 };
    assert_eq!(
        delta_variant.to_scroll_viewport(),
        ScrollViewport::Delta(-10)
    );
    term.scroll_viewport(delta_variant.to_scroll_viewport())
        .expect("scroll delta");
    assert!(term
        .render_snapshot()
        .expect("snapshot after upward delta")
        .row_text(0)
        .starts_with("row-034"));

    // Row
    let row_variant = NativeTerminalScrollBehavior::Row { offset: 5 };
    assert_eq!(row_variant.to_scroll_viewport(), ScrollViewport::Row(5));
    term.scroll_viewport(row_variant.to_scroll_viewport())
        .expect("scroll row");
    assert!(term
        .render_snapshot()
        .expect("snapshot after row scroll")
        .row_text(0)
        .starts_with("row-005"));
}

#[tokio::test]
async fn attached_scrollbar_boundary_tracks_native_viewport_position() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-scrollbar-boundary";
    let (tx, attachment) = create_attachment(session_id);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach native session");
    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe to native output");

    let history = (0..100)
        .map(|row| format!("row-{row:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 1,
        data: Cow::Owned(history.into_bytes()),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send scrollback output");
    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("output update arrives")
        .expect("update sender stays open");

    let bottom = scrollbar_for_attached_native_terminal(&state, session_id)
        .expect("read scrollbar at live prompt");
    assert!(bottom.total > bottom.len, "fixture must create scrollback");
    assert_eq!(bottom.offset + bottom.len, bottom.total);

    scroll_attached_native_terminal(&state, session_id, ScrollViewport::Top)
        .expect("scroll to top");
    let top =
        scrollbar_for_attached_native_terminal(&state, session_id).expect("read scrollbar at top");
    assert_eq!(top.total, bottom.total);
    assert_eq!(top.len, bottom.len);
    assert_eq!(top.offset, 0);
}

#[test]
fn scroll_behavior_serde_wire_contract_matches_frontend_payloads() {
    // Given: valid frontend JSON payloads for scroll behavior
    let delta_json = r#"{"type":"delta","rows":3}"#;
    let delta_neg_json = r#"{"type":"delta","rows":-3}"#;
    let top_json = r#"{"type":"top"}"#;
    let bottom_json = r#"{"type":"bottom"}"#;
    let row_json = r#"{"type":"row","offset":5}"#;

    // When: deserializing from JSON wire representation
    let delta: NativeTerminalScrollBehavior =
        serde_json::from_str(delta_json).expect("deserialize delta");
    let delta_neg: NativeTerminalScrollBehavior =
        serde_json::from_str(delta_neg_json).expect("deserialize negative delta");
    let top: NativeTerminalScrollBehavior =
        serde_json::from_str(top_json).expect("deserialize top");
    let bottom: NativeTerminalScrollBehavior =
        serde_json::from_str(bottom_json).expect("deserialize bottom");
    let row: NativeTerminalScrollBehavior =
        serde_json::from_str(row_json).expect("deserialize row");

    // Then: correct variants and viewport mapping
    assert_eq!(delta, NativeTerminalScrollBehavior::Delta { rows: 3 });
    assert_eq!(delta.to_scroll_viewport(), ScrollViewport::Delta(3));

    assert_eq!(delta_neg, NativeTerminalScrollBehavior::Delta { rows: -3 });
    assert_eq!(delta_neg.to_scroll_viewport(), ScrollViewport::Delta(-3));

    assert_eq!(top, NativeTerminalScrollBehavior::Top);
    assert_eq!(top.to_scroll_viewport(), ScrollViewport::Top);

    assert_eq!(bottom, NativeTerminalScrollBehavior::Bottom);
    assert_eq!(bottom.to_scroll_viewport(), ScrollViewport::Bottom);

    assert_eq!(row, NativeTerminalScrollBehavior::Row { offset: 5 });
    assert_eq!(row.to_scroll_viewport(), ScrollViewport::Row(5));

    // Regressive externally-tagged format {"Delta":{"rows":3}} without "type" field must fail deserialization
    let regressive_delta_json = r#"{"Delta":{"rows":3}}"#;
    assert!(
        serde_json::from_str::<NativeTerminalScrollBehavior>(regressive_delta_json).is_err(),
        "Externally tagged JSON object without 'type' discriminator must fail deserialization"
    );
}

#[tokio::test]
async fn selection_and_search_boundaries_reject_detached_and_unattached_sessions() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-select-search-session";
    let (tx, attachment) = create_attachment(session_id);

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach daemon stream state");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe session update");

    // Feed known bytes into the attached session via live DaemonStreamMessage::Output
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 1,
        data: Cow::Borrowed(b"alpha selection-needle-xyz omega\r\nsecond line\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send output message to daemon stream");

    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("output processed within bounded timeout")
        .expect("update sender remains connected");

    // Selection on attached session selects actual session content
    let select_all_receipt =
        select_attached_native_terminal(&state, session_id, &NativeTerminalSelectMode::All)
            .expect("attached select-all succeeds");
    let selected_text = select_all_receipt
        .text
        .expect("select all should produce selection text on non-empty terminal");
    assert!(
        selected_text.contains("selection-needle-xyz"),
        "selection must contain session's live terminal content"
    );

    let copy_text = copy_attached_native_selection(&state, session_id)
        .expect("attached copy selection succeeds");
    assert!(
        copy_text.contains("selection-needle-xyz"),
        "copied text must match active selection in session's terminal"
    );

    // Word selection at column 6 row 0
    let select_word_receipt = select_attached_native_terminal(
        &state,
        session_id,
        &NativeTerminalSelectMode::Word { col: 6, row: 0 },
    )
    .expect("word selection succeeds");
    assert_eq!(
        select_word_receipt.text.as_deref(),
        Some("selection-needle-xyz")
    );

    // Line selection at row 1
    let select_line_receipt = select_attached_native_terminal(
        &state,
        session_id,
        &NativeTerminalSelectMode::Line { row: 1 },
    )
    .expect("line selection succeeds");
    assert_eq!(
        select_line_receipt.text.as_deref().map(str::trim),
        Some("second line")
    );

    // Search on attached session finds match coordinates
    let search_receipt =
        search_attached_native_terminal(&state, session_id, "selection-needle-xyz", true)
            .expect("attached search succeeds");
    assert_eq!(search_receipt.total_matches, 1);
    assert_eq!(search_receipt.matches[0].row, 0);
    assert_eq!(search_receipt.matches[0].start_col, 6);
    assert_eq!(search_receipt.matches[0].end_col, 25);

    let absent_search = search_attached_native_terminal(&state, session_id, "nonexistent", true)
        .expect("absent search succeeds");
    assert_eq!(absent_search.total_matches, 0);
    assert!(absent_search.matches.is_empty());

    let scroll_res = scroll_attached_native_terminal(
        &state,
        session_id,
        NativeTerminalScrollBehavior::Top.to_scroll_viewport(),
    );
    assert!(scroll_res.is_ok());

    state.detach_session(session_id);

    // After detach, select / copy / search / scroll must be rejected
    assert!(matches!(
        select_attached_native_terminal(&state, session_id, &NativeTerminalSelectMode::All),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(matches!(
        copy_attached_native_selection(&state, session_id),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(matches!(
        search_attached_native_terminal(&state, session_id, "foo", true),
        Err(NativeTerminalError::NoValue)
    ));
    assert!(matches!(
        scroll_attached_native_terminal(
            &state,
            session_id,
            NativeTerminalScrollBehavior::Top.to_scroll_viewport()
        ),
        Err(NativeTerminalError::NoValue)
    ));
}

#[test]
fn native_terminal_commands_are_registered_in_tauri_generate_handler() {
    let lib_rs_source = include_str!("../src/lib.rs");
    let generate_handler_block = lib_rs_source
        .split("tauri::generate_handler![")
        .nth(1)
        .expect("must contain generate_handler! macro invocation")
        .split(']')
        .next()
        .expect("generate_handler! closing bracket");

    let required_commands = [
        "cmd_native_terminal_attach",
        "cmd_native_terminal_detach",
        "cmd_native_terminal_set_bounds",
        "cmd_native_terminal_set_focus",
        "cmd_native_terminal_send_input",
        "cmd_native_terminal_scroll",
        "cmd_native_terminal_scrollbar",
        "cmd_native_terminal_set_scrollbar_overlay",
        "cmd_native_terminal_set_attention_frame",
        "cmd_native_terminal_select",
        "cmd_native_terminal_copy_selection",
        "cmd_native_terminal_paste",
        "cmd_native_terminal_clipboard_content",
        "cmd_native_terminal_mouse",
        "cmd_native_terminal_search",
    ];

    for cmd in required_commands {
        assert!(
            generate_handler_block.contains(cmd),
            "Command '{cmd}' must be present in tauri::generate_handler![...] in src-tauri/src/lib.rs"
        );
    }
}

#[tokio::test]
async fn option_as_alt_disabled_strips_the_alt_modifier_from_encoded_keys() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-option-as-alt";
    let (_tx, attachment) = create_attachment(session_id);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach native daemon stream state");

    let alt_b: NativeTerminalInput = serde_json::from_str(
        r#"{
        "keyEvent": {
            "key": "b",
            "action": "Press",
            "modifiers": {
                "shift": false,
                "ctrl": false,
                "alt": true,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }
    }"#,
    )
    .expect("deserialize alt key event");

    ferryx_lib::terminal::set_terminal_preference_overrides(
        ferryx_lib::terminal::TerminalPreferenceOverrides {
            macos_option_as_alt: Some(true),
            ..Default::default()
        },
    );
    let with_alt =
        encode_attached_native_input(&state, session_id, &alt_b).expect("encode with alt enabled");

    ferryx_lib::terminal::set_terminal_preference_overrides(
        ferryx_lib::terminal::TerminalPreferenceOverrides {
            macos_option_as_alt: Some(false),
            ..Default::default()
        },
    );
    let without_alt =
        encode_attached_native_input(&state, session_id, &alt_b).expect("encode with alt disabled");

    ferryx_lib::terminal::set_terminal_preference_overrides(Default::default());

    if cfg!(target_os = "macos") {
        assert_eq!(
            with_alt, b"\x1bb",
            "option-as-alt enabled must emit the ESC-prefixed Meta sequence"
        );
        assert_eq!(
            without_alt, b"b",
            "option-as-alt disabled must not inject an ESC prefix"
        );
    } else {
        assert_eq!(with_alt, without_alt);
    }
}

#[tokio::test]
async fn production_input_boundary_encodes_ctrl_v_image_paste_shortcut_to_pty_byte_0x16() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-c2-ctrl-v";
    let (_tx, attachment) = create_attachment(session_id);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach native daemon stream state");

    // Exact IPC payload produced by NativeTerminalPane on DOM image paste
    let ctrl_v_press: NativeTerminalInput = serde_json::from_str(
        r#"{
        "keyEvent": {
            "key": "v",
            "action": "Press",
            "modifiers": {
                "shift": false,
                "ctrl": true,
                "alt": false,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }
    }"#,
    )
    .expect("deserialize ctrl+v key event");

    let encoded_press = encode_attached_native_input(&state, session_id, &ctrl_v_press)
        .expect("encode attached ctrl+v input");

    // 0x16 is ASCII SYN (decimal 22 / Ctrl+V) expected by terminal CLI applications like Claude Code
    assert_eq!(
        encoded_press,
        vec![0x16],
        "Ctrl+V press must encode to exactly one 0x16 byte ready for PTY delivery"
    );

    let ctrl_v_release: NativeTerminalInput = serde_json::from_str(
        r#"{
        "keyEvent": {
            "key": "v",
            "action": "Release",
            "modifiers": {
                "shift": false,
                "ctrl": true,
                "alt": false,
                "superKey": false,
                "capsLock": false,
                "numLock": false
            },
            "utf8": null
        }
    }"#,
    )
    .expect("deserialize ctrl+v release event");

    let encoded_release = encode_attached_native_input(&state, session_id, &ctrl_v_release)
        .expect("encode attached ctrl+v release");

    assert!(
        encoded_release.is_empty(),
        "key release must not emit duplicate bytes to the PTY"
    );
}
