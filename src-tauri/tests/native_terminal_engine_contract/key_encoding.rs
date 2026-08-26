use ferryx_lib::native_terminal::{
    KeyAction, KeyCode, KeyEvent, KeyModifiers, NativeTerminal, NativeTerminalError, TerminalEngine,
};

#[test]
fn test_key_encode_plain_and_mode_sensitive_arrow_keys() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // 1. Plain character input
    let key_a = KeyEvent::new(KeyCode::Character('a'), KeyAction::Press);
    let bytes_a = term.encode_key(&key_a).expect("encode 'a'");
    assert_eq!(bytes_a, b"a");

    // 2. Enter key
    let key_enter = KeyEvent::new(KeyCode::Enter, KeyAction::Press);
    let bytes_enter = term.encode_key(&key_enter).expect("encode Enter");
    assert_eq!(bytes_enter, b"\r");

    // 3. Arrow Up key in default ANSI cursor mode: \x1b[A
    let key_up = KeyEvent::new(KeyCode::ArrowUp, KeyAction::Press);
    let bytes_up_normal = term.encode_key(&key_up).expect("encode ArrowUp normal");
    assert_eq!(bytes_up_normal, b"\x1b[A");

    // 4. Switch terminal to Application Cursor Keys mode (DECCKM: \x1b[?1h)
    term.feed_str("\x1b[?1h").expect("feed DECCKM mode enable");

    // 5. Arrow Up key in Application Cursor Keys mode: \x1bOA
    let bytes_up_app = term
        .encode_key(&key_up)
        .expect("encode ArrowUp application mode");
    assert_eq!(
        bytes_up_app, b"\x1bOA",
        "In DECCKM application mode, Arrow Up must encode to \\x1bOA"
    );

    // 6. Reset DECCKM (\x1b[?1l) returns Arrow Up to \x1b[A
    term.feed_str("\x1b[?1l").expect("feed DECCKM mode disable");
    let bytes_up_restored = term.encode_key(&key_up).expect("encode ArrowUp restored");
    assert_eq!(bytes_up_restored, b"\x1b[A");

    // 7. Function keys F1..F4 (SS3 / ESC O sequences) and F5, F12 (CSI ~ sequences)
    let key_f1 = KeyEvent::new(KeyCode::F1, KeyAction::Press);
    let bytes_f1 = term.encode_key(&key_f1).expect("encode F1");
    assert_eq!(bytes_f1, b"\x1bOP", "F1 must encode to \\x1bOP");

    let key_f2 = KeyEvent::new(KeyCode::F2, KeyAction::Press);
    let bytes_f2 = term.encode_key(&key_f2).expect("encode F2");
    assert_eq!(bytes_f2, b"\x1bOQ", "F2 must encode to \\x1bOQ");

    let key_f5 = KeyEvent::new(KeyCode::F5, KeyAction::Press);
    let bytes_f5 = term.encode_key(&key_f5).expect("encode F5");
    assert_eq!(bytes_f5, b"\x1b[15~", "F5 must encode to \\x1b[15~");

    let key_f12 = KeyEvent::new(KeyCode::F12, KeyAction::Press);
    let bytes_f12 = term.encode_key(&key_f12).expect("encode F12");
    assert_eq!(bytes_f12, b"\x1b[24~", "F12 must encode to \\x1b[24~");
}

#[test]
fn test_key_encode_rejects_c0_control_utf8_payload() {
    // Given: an initialized terminal engine instance
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // When: encoding a KeyEvent whose UTF-8 text contains C0 control character (e.g. \x01 SOH)
    let key_with_c0 = KeyEvent {
        key: KeyCode::Character('a'),
        action: KeyAction::Press,
        modifiers: KeyModifiers::default(),
        utf8: Some("\x01".to_string()),
    };
    let res_c0 = term.encode_key(&key_with_c0);

    // Then: the operation is rejected before foreign encoding with a typed InvalidValue error
    assert!(
        matches!(res_c0, Err(NativeTerminalError::InvalidValue(_))),
        "C0 control characters in KeyEvent utf8 payload must return InvalidValue"
    );
}

#[test]
fn test_key_encode_rejects_del_utf8_payload() {
    // Given: an initialized terminal engine instance
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // When: encoding a KeyEvent whose UTF-8 text contains DEL (\x7f)
    let key_with_del = KeyEvent {
        key: KeyCode::Backspace,
        action: KeyAction::Press,
        modifiers: KeyModifiers::default(),
        utf8: Some("\x7f".to_string()),
    };
    let res_del = term.encode_key(&key_with_del);

    // Then: DEL is rejected with a typed InvalidValue error
    assert!(
        matches!(res_del, Err(NativeTerminalError::InvalidValue(_))),
        "DEL in KeyEvent utf8 payload must return InvalidValue"
    );
}

#[test]
fn test_key_encode_rejects_pua_utf8_payload() {
    // Given: an initialized terminal engine instance
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    // When: encoding a KeyEvent whose UTF-8 text contains macOS Private Use Area codepoint (U+F700)
    let key_with_pua = KeyEvent {
        key: KeyCode::F1,
        action: KeyAction::Press,
        modifiers: KeyModifiers::default(),
        utf8: Some("\u{F700}".to_string()),
    };
    let res_pua = term.encode_key(&key_with_pua);

    // Then: PUA codepoints are rejected with a typed InvalidValue error
    assert!(
        matches!(res_pua, Err(NativeTerminalError::InvalidValue(_))),
        "PUA codepoints in KeyEvent utf8 payload must return InvalidValue"
    );
}
