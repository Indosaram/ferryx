use ferryx_vt::{HostTerminal, DEFAULT_SCROLLBACK_LINES};
use fxsh::types::{Color, UiEventKind};
use proptest::prelude::*;
use session_core::replica::{diff, Replica};

fn text_of(r: &Replica, row: usize) -> String {
    r.screen[row].cells.iter().map(|c| char::from_u32(c.codepoint).filter(|_| c.codepoint != 0).unwrap_or(' ')).collect::<String>().trim_end().to_owned()
}

#[test]
fn printable_output_lands_on_screen_with_styles_and_links() {
    let mut t = HostTerminal::new(40, 6, 100);
    t.feed(b"plain \x1b[1;31mred\x1b[0m \x1b]8;;https://ex.com/a\x1b\\link\x1b]8;;\x1b\\");
    let r = &t.replica;
    assert_eq!(text_of(r, 0), "plain red link");
    let red = &r.screen[0].cells[6];
    assert_eq!((red.codepoint, red.attrs & 1, red.fg), ('r' as u32, 1, Color::Indexed(1)));
    let link = r.screen[0].cells[10].hyperlink_id;
    assert_ne!(link, 0);
    assert_eq!(r.hyperlinks.get(&link).map(String::as_str), Some("https://ex.com/a"));
    assert_eq!((r.cursor.row, r.cursor.col), (0, 14));
}

#[test]
fn scrolled_rows_get_increasing_line_ids_and_eviction_keeps_order() {
    let mut t = HostTerminal::new(20, 3, 100);
    for i in 0..10 {
        t.feed(format!("line {i}\r\n").as_bytes());
    }
    let ids: Vec<u64> = t.replica.scrollback.iter().map(|(id, _)| *id).collect();
    assert!(ids.windows(2).all(|w| w[1] == w[0] + 1), "{ids:?}");
    let texts: Vec<String> = t.replica.scrollback.iter().map(|(_, r)| r.cells.iter().map(|c| char::from_u32(c.codepoint).filter(|_| c.codepoint != 0).unwrap_or(' ')).collect::<String>().trim_end().to_owned()).collect();
    assert_eq!(texts.last().map(String::as_str), Some("line 7"));
}

#[test]
fn eviction_from_the_front_keeps_line_ids_and_content_aligned() {
    let mut t = HostTerminal::new(80, 24, DEFAULT_SCROLLBACK_LINES);
    let mut client = t.replica.clone();
    let mut rev = 0;
    let mut next = 0;
    while next < 40_000 {
        let prev = t.replica.clone();
        let mut chunk = String::new();
        for _ in 0..200 {
            chunk.push_str(&format!("line {next}\r\n"));
            next += 1;
        }
        let adv = t.feed(chunk.as_bytes());
        assert!(!adv.rewritten, "appending output is never a rewrite (up to line {next})");
        let d = diff(&prev, &t.replica, 1, rev, rev + 1).expect("append-only");
        client.apply_delta(&d);
        rev += 1;
    }
    let sb = &t.replica.scrollback;
    assert_eq!(sb.len(), t.engine.scrollback_rows());
    assert!(sb.len() < 20_000, "the engine must have evicted rows: {}", sb.len());
    assert!(sb.front().is_some_and(|(id, _)| *id > 10_000), "line ids of evicted rows are never reused");
    assert!(sb.iter().zip(sb.iter().skip(1)).all(|((a, _), (b, _))| *b == *a + 1));
    let last: String = sb.back().unwrap().1.cells.iter().map(|c| char::from_u32(c.codepoint).filter(|_| c.codepoint != 0).unwrap_or(' ')).collect();
    assert_eq!(last.trim_end(), format!("line {}", next - 24));
    assert_eq!(fxsh::state_digest(&client.to_body()), fxsh::state_digest(&t.replica.to_body()));
}

#[test]
fn rewrites_are_reported() {
    let cases: [(&str, &[u8], Option<(u16, u16)>); 4] = [
        ("ED3", b"\x1b[3J", None),
        ("RIS", b"\x1bc", None),
        ("alt screen", b"\x1b[?1049h", None),
        ("resize cols", b"", Some((30, 6))),
    ];
    for (name, seq, size) in cases {
        let mut t = HostTerminal::new(40, 6, 100);
        for i in 0..20 {
            t.feed(format!("row {i}\r\n").as_bytes());
        }
        let adv = match size {
            Some((c, r)) => t.resize(c, r),
            None => t.feed(seq),
        };
        assert!(adv.rewritten, "{name} must be reported as a scrollback rewrite");
    }
    let mut t = HostTerminal::new(40, 6, 100);
    for i in 0..20 {
        t.feed(format!("row {i}\r\n").as_bytes());
    }
    assert!(!t.feed(b"more\r\n").rewritten, "plain scrolling is not a rewrite");
}

#[test]
fn device_queries_produce_replies_and_osc_produces_ui_events() {
    let mut t = HostTerminal::new(40, 6, 100);
    let a = t.feed(b"\x1b[5n\x1b[6n");
    assert_eq!(a.vt_replies.concat(), b"\x1b[0n\x1b[1;1R".to_vec());
    let b = t.feed(b"\x07\x1b]52;c;aGVsbG8=\x07\x1b]777;notify;T;B\x07");
    let kinds: Vec<UiEventKind> = b.ui_events.iter().map(|e| e.kind.clone()).collect();
    assert!(kinds.contains(&UiEventKind::Bell));
    assert!(kinds.iter().any(|k| matches!(k, UiEventKind::ClipboardWrite { text, .. } if text == "hello")));
    assert!(kinds.iter().any(|k| matches!(k, UiEventKind::Notification { title, body } if title == "T" && body == "B")));
    let ids: Vec<u64> = b.ui_events.iter().map(|e| e.event_id).collect();
    assert_eq!(ids, (1..=ids.len() as u64).collect::<Vec<_>>());
}

#[test]
fn title_modes_and_palette_are_extracted() {
    let mut t = HostTerminal::new(40, 6, 100);
    t.feed(b"\x1b]0;my title\x07\x1b[?2004h\x1b[?1h\x1b[?1006h\x1b[?1002h\x1b]4;1;rgb:12/34/56\x1b\\");
    let r = &t.replica;
    assert_eq!(r.title, "my title");
    assert_ne!(r.modes.flags & (1 << 3), 0, "bracketed paste");
    assert_ne!(r.modes.flags & (1 << 1), 0, "app cursor keys");
    assert_eq!(r.modes.mouse_mode, fxsh::types::MouseMode::Button);
    assert_eq!(r.modes.mouse_encoding, fxsh::types::MouseEncoding::Sgr);
    assert_eq!(r.palette.overrides, vec![(1, (0x12, (0x34, 0x56)))]);
}

fn chunk() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => "[a-z ]{0,30}".prop_map(|s| s.into_bytes()),
        3 => Just(b"\r\n".to_vec()),
        1 => (0u8..8).prop_map(|c| format!("\x1b[3{c}m").into_bytes()),
        1 => Just(b"\x1b[0m".to_vec()),
        1 => (1u8..6, 1u8..30).prop_map(|(r, c)| format!("\x1b[{r};{c}H").into_bytes()),
        1 => Just(b"\x1b[2J".to_vec()),
        1 => Just(b"\x1b[K".to_vec()),
        1 => Just("e\u{301} \u{4e2d}\u{1f469}\u{200d}\u{1f4bb}".as_bytes().to_vec()),
        1 => (0u8..4).prop_map(|i| format!("\x1b]8;;https://e/{i}\x1b\\L{i}\x1b]8;;\x1b\\").into_bytes()),
        1 => Just(b"\x1b[3J".to_vec()),
        1 => Just(b"\x1b[?1049h".to_vec()),
        1 => Just(b"\x1b[?1049l".to_vec()),
        1 => (0u8..3).prop_map(|t| format!("\x1b]2;t{t}\x07").into_bytes()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(300), .. ProptestConfig::default() })]

    #[test]
    fn deltas_replicate_the_host_state(steps in prop::collection::vec((chunk(), prop::option::weighted(0.05, (20u16..60, 3u16..10))), 1..60)) {
        let mut t = HostTerminal::new(40, 6, 30);
        let mut client = t.replica.clone();
        let mut rev = 0;
        for (bytes, resize) in steps {
            let prev = t.replica.clone();
            let adv = match resize {
                Some((c, r)) => t.resize(c, r),
                None => t.feed(&bytes),
            };
            if adv.rewritten {
                client = t.replica.clone();
            } else {
                let d = diff(&prev, &t.replica, 1, rev, rev + 1).expect("an Appended step never rewrites scrollback");
                client.apply_delta(&d);
            }
            rev += 1;
            prop_assert_eq!(fxsh::state_digest(&client.to_body()), fxsh::state_digest(&t.replica.to_body()));
            let body = t.replica.to_body();
            let mut w = Vec::new();
            fxsh::Codec::enc(&body, &mut w);
            let back = <fxsh::types::SBody as fxsh::Codec>::dec(&mut fxsh::wire::Reader::new(&w));
            prop_assert!(back.is_ok(), "extracted S must satisfy the wire schema: {:?}", back.err());
        }
    }
}
