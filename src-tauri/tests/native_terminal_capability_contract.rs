use ferryx_lib::native_terminal::{ColorRgb, NativeTerminal, ScrollViewport, TerminalEngine};

fn first_row(term: &dyn TerminalEngine) -> String {
    term.render_snapshot()
        .expect("capture render snapshot")
        .row_text(0)
}

#[test]
fn scrollback_viewport_moves_to_top_and_returns_to_bottom() {
    let mut term = NativeTerminal::new(24, 5).expect("create terminal");
    let lines = (0..200)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    term.feed_str(&lines).expect("feed scrollback lines");

    let bottom_first = first_row(&term);
    assert!(bottom_first.starts_with("line-195"), "got {bottom_first:?}");
    assert!(term.scrollback_rows().expect("query scrollback rows") >= 195);
    assert_eq!(
        term.total_rows().expect("query total rows"),
        term.scrollbar().expect("query scrollbar").total as usize
    );

    term.scroll_viewport(ScrollViewport::Top)
        .expect("scroll to top");
    let top_first = first_row(&term);
    assert!(top_first.starts_with("line-000"), "got {top_first:?}");
    assert_ne!(top_first, bottom_first);

    term.scroll_viewport(ScrollViewport::Bottom)
        .expect("scroll to bottom");
    assert_eq!(first_row(&term), bottom_first);
}

#[test]
fn selection_helpers_install_and_copy_terminal_owned_text() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    term.feed_str("alpha contract-selection-needle omega\r\nsecond line")
        .expect("feed selectable text");

    term.select_all().expect("select all");
    let selected = term
        .selection_text()
        .expect("format selection")
        .expect("active selection");
    assert!(selected.contains("contract-selection-needle"));

    let range = term
        .selection_range()
        .expect("query selection range")
        .expect("active selection range");
    assert_eq!((range.0, range.1), (0, 0));

    term.select_word_at(6, 0)
        .expect("select word at viewport cell");
    assert_eq!(
        term.selection_text()
            .expect("format selected word")
            .expect("active word selection"),
        "contract-selection-needle"
    );

    term.select_line_at(1).expect("select viewport line");
    assert_eq!(
        term.selection_text()
            .expect("format selected line")
            .expect("active line selection"),
        "second line"
    );

    term.clear_selection().expect("clear selection");
    assert_eq!(
        term.selection_text().expect("query cleared selection"),
        None
    );
    assert_eq!(term.selection_range().expect("query cleared range"), None);
}

#[test]
fn paste_encoding_tracks_bracketed_paste_mode() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");

    let plain = term.encode_paste("hello").expect("encode plain paste");
    assert!(plain.windows(5).any(|window| window == b"hello"));
    assert_eq!(plain, b"hello");
    assert!(term.paste_is_safe("hello"));
    assert!(!term.paste_is_safe("hello\nworld"));

    term.feed_str("\x1b[?2004h")
        .expect("enable bracketed paste mode");
    let bracketed = term.encode_paste("hello").expect("encode bracketed paste");
    assert_eq!(bracketed, b"\x1b[200~hello\x1b[201~");

    // Multiline paste in bracketed paste mode preserves newlines and normalizes CRLF
    let multiline_bracketed = term
        .encode_paste("line 1\r\nline 2\nline 3")
        .expect("encode multiline bracketed paste");
    assert_eq!(
        multiline_bracketed,
        b"\x1b[200~line 1\nline 2\nline 3\x1b[201~"
    );

    // Explicit override forces bracketed mode even on an unbracketed terminal
    let plain_term = NativeTerminal::new(40, 6).expect("create plain terminal");
    let overridden = plain_term
        .encode_paste_with_bracketed_override("first\r\nsecond", Some(true))
        .expect("encode with override");
    assert_eq!(overridden, b"\x1b[200~first\nsecond\x1b[201~");

    // Plain term with multiline text also automatically brackets to prevent Enter splitting
    let multiline_plain = plain_term
        .encode_paste("first\r\nsecond")
        .expect("encode multiline on plain term");
    assert_eq!(multiline_plain, b"\x1b[200~first\nsecond\x1b[201~");
}

#[test]
fn grid_search_reports_screen_coordinates_and_absence() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    term.feed_str("zero\r\nprefix needle suffix\r\nlast")
        .expect("feed searchable text");

    assert_eq!(
        term.search_grid("needle", true)
            .expect("search exact needle"),
        vec![(1, 7, 12)]
    );
    assert_eq!(
        term.search_grid("NEEDLE", false)
            .expect("search case-insensitive needle"),
        vec![(1, 7, 12)]
    );
    assert!(term
        .search_grid("absent", true)
        .expect("search absent needle")
        .is_empty());

    let history = (0..20)
        .map(|index| {
            if index == 1 {
                "old scrollback-needle".to_string()
            } else {
                format!("history-{index:02}")
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    let mut history_term = NativeTerminal::new(40, 4).expect("create history terminal");
    history_term
        .feed_str(&history)
        .expect("feed searchable history");
    assert_eq!(
        history_term
            .search_grid("scrollback-needle", true)
            .expect("search retained scrollback"),
        vec![(1, 4, 20)]
    );
}

#[test]
fn configured_terminal_colors_and_palette_read_back() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    let foreground = ColorRgb {
        r: 11,
        g: 22,
        b: 33,
    };
    let background = ColorRgb {
        r: 44,
        g: 55,
        b: 66,
    };
    let cursor = ColorRgb {
        r: 77,
        g: 88,
        b: 99,
    };
    let mut palette = [ColorRgb::default(); 256];
    for (index, color) in palette.iter_mut().enumerate() {
        *color = ColorRgb {
            r: index as u8,
            g: (255 - index) as u8,
            b: (index / 2) as u8,
        };
    }

    term.set_default_foreground(foreground)
        .expect("set foreground through terminal option");
    term.set_default_background(background)
        .expect("set background through terminal option");
    term.set_default_cursor_color(cursor)
        .expect("set cursor through terminal option");
    term.set_palette(palette)
        .expect("set palette through terminal option");

    assert_eq!(
        term.default_foreground().expect("read foreground"),
        foreground
    );
    assert_eq!(
        term.default_background().expect("read background"),
        background
    );
    assert_eq!(term.default_cursor_color().expect("read cursor"), cursor);
    assert_eq!(term.palette().expect("read palette"), palette);
}

#[test]
fn mouse_tracking_query_reflects_vt_modes() {
    let mut term = NativeTerminal::new(40, 6).expect("create terminal");
    assert!(!term.mouse_tracking_enabled().expect("query initial mode"));
    term.feed_str("\x1b[?1000h").expect("enable mouse tracking");
    assert!(term.mouse_tracking_enabled().expect("query enabled mode"));
}
