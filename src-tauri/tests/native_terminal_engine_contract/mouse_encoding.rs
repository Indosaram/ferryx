use ferryx_lib::native_terminal::{
    KeyModifiers, MouseAction, MouseButton, MouseEvent, MousePosition, MouseRendererSize,
    NativeTerminal, NativeTerminalError, TerminalEngine,
};

#[test]
fn test_mouse_encode_rejects_nan_coordinate() {
    // Given: an initialized terminal engine instance and geometry context
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    let valid_size = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    // When: mouse position contains NaN
    let event_nan = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition {
            x: f32::NAN,
            y: 10.0,
        },
        modifiers: KeyModifiers::default(),
        size: Some(valid_size),
    };
    let res_nan = term.encode_mouse(&event_nan);

    // Then: rejected with typed InvalidValue
    assert!(
        matches!(res_nan, Err(NativeTerminalError::InvalidValue(_))),
        "NaN mouse coordinate must return InvalidValue"
    );
}

#[test]
fn test_mouse_encode_rejects_infinity_coordinate() {
    // Given: an initialized terminal engine instance and geometry context
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    let valid_size = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    // When: mouse position contains infinity
    let event_inf = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition {
            x: 10.0,
            y: f32::INFINITY,
        },
        modifiers: KeyModifiers::default(),
        size: Some(valid_size),
    };
    let res_inf = term.encode_mouse(&event_inf);

    // Then: rejected with typed InvalidValue
    assert!(
        matches!(res_inf, Err(NativeTerminalError::InvalidValue(_))),
        "Infinity mouse coordinate must return InvalidValue"
    );
}

#[test]
fn test_mouse_encode_rejects_zero_cell_width() {
    // Given: an initialized terminal engine instance and geometry context with zero cell_width
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    let zero_cell_width = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 0,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    // When: encoding mouse event with zero cell width
    let event_zero_w = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition { x: 10.0, y: 10.0 },
        modifiers: KeyModifiers::default(),
        size: Some(zero_cell_width),
    };
    let res_zero_w = term.encode_mouse(&event_zero_w);

    // Then: rejected with typed InvalidValue
    assert!(
        matches!(res_zero_w, Err(NativeTerminalError::InvalidValue(_))),
        "Zero cell_width must return InvalidValue"
    );
}

#[test]
fn test_mouse_encode_rejects_zero_cell_height() {
    // Given: an initialized terminal engine instance and geometry context with zero cell_height
    let term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    let zero_cell_height = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 0,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    // When: encoding mouse event with zero cell height
    let event_zero_h = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition { x: 10.0, y: 10.0 },
        modifiers: KeyModifiers::default(),
        size: Some(zero_cell_height),
    };
    let res_zero_h = term.encode_mouse(&event_zero_h);

    // Then: rejected with typed InvalidValue
    assert!(
        matches!(res_zero_h, Err(NativeTerminalError::InvalidValue(_))),
        "Zero cell_height must return InvalidValue"
    );
}

#[test]
fn test_mouse_encode_disabled_and_sgr_enabled_tracking() {
    let mut term: Box<dyn TerminalEngine> =
        Box::new(NativeTerminal::new(80, 24).expect("create terminal"));

    let mouse_size = MouseRendererSize {
        screen_width: 800,
        screen_height: 480,
        cell_width: 10,
        cell_height: 20,
        padding_top: 0,
        padding_bottom: 0,
        padding_right: 0,
        padding_left: 0,
    };

    let click_event = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        position: MousePosition { x: 25.0, y: 45.0 }, // Col 3, Row 3 in 1-based VT coords
        modifiers: KeyModifiers::default(),
        size: Some(mouse_size),
    };

    // 1. By default, mouse tracking is disabled in the terminal -> empty encoding
    let bytes_disabled = term
        .encode_mouse(&click_event)
        .expect("encode mouse disabled");
    assert!(
        bytes_disabled.is_empty(),
        "When mouse tracking is disabled, encode_mouse must produce empty output"
    );

    // 2. Enable SGR mouse tracking mode (Normal tracking 1000h + SGR format 1006h)
    term.feed_str("\x1b[?1000h\x1b[?1006h")
        .expect("enable SGR mouse mode");

    // 3. Encode same mouse click in SGR mode -> \x1b[<0;3;3M
    let bytes_sgr = term
        .encode_mouse(&click_event)
        .expect("encode mouse in SGR mode");
    assert!(
        !bytes_sgr.is_empty(),
        "When SGR mouse mode is enabled, mouse press must produce escape sequence"
    );
    let sgr_str = std::str::from_utf8(&bytes_sgr).expect("valid utf-8 escape sequence");
    assert!(
        sgr_str.starts_with("\x1b[<"),
        "SGR mouse sequence must start with \\x1b[<, got: {:?}",
        sgr_str
    );
    assert!(
        sgr_str.ends_with('M'),
        "SGR press sequence must terminate with 'M', got: {:?}",
        sgr_str
    );
}
