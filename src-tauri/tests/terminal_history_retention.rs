//! Retention contract for bounded, protocol-safe full-history reconstruction.
//!
//! These cases pin the *hub-level* guarantees that a GUI restart relies on while the daemon
//! keeps running: a full replay (`after_sequence == None`) must reconstruct the retained
//! history from whole VT units, so image traffic can never evict older text and a snapshot
//! can never begin inside a graphics payload.

use ferryx_lib::terminal::TerminalOutputHub;

const OLDEST_TEXT: &[u8] = b"OLDEST-TEXT-MUST-SURVIVE";
const NEWEST_TEXT: &[u8] = b"NEWEST-TEXT-MUST-SURVIVE";
/// PTY reads are 8 KiB-ish, deliberately unaligned with any escape sequence boundary.
const PTY_READ: usize = 8191;
/// Kitty transmits base64 payload in <= 4096 byte chunks.
const KITTY_CHUNK: usize = 4096;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|w| *w == needle).count()
}

/// One Kitty RGB transmission split into `m=1` continuation chunks terminated by `m=0`.
/// The payload is synthetic base64 alphabet: nothing in this test decodes it, the point is
/// that the bytes are opaque and must never be cut apart by retention.
fn kitty_chunked_image(payload_bytes: usize, id: u32) -> Vec<u8> {
    let payload = vec![b'A'; payload_bytes];
    let mut out = Vec::with_capacity(payload_bytes + payload_bytes / KITTY_CHUNK * 64 + 64);
    let chunks: Vec<&[u8]> = payload.chunks(KITTY_CHUNK).collect();
    for (index, part) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        let header = if index == 0 {
            format!("a=T,f=24,s=64,v=64,i={id},c=12,r=6,C=1,q=2,m={more}")
        } else {
            format!("m={more}")
        };
        out.extend_from_slice(b"\x1b_G");
        out.extend_from_slice(header.as_bytes());
        out.push(b';');
        out.extend_from_slice(part);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn publish_stream(hub: &TerminalOutputHub, session: &str, stream: &[u8]) {
    for chunk in stream.chunks(PTY_READ) {
        hub.publish(session, chunk.to_vec())
            .expect("session accepts published chunk");
    }
}

fn full_history(hub: &TerminalOutputHub, session: &str) -> Vec<u8> {
    hub.subscribe_with_sequence(session, None)
        .expect("session attachment")
        .snapshot
        .history
}

/// Every APC/DCS/OSC string sequence in a replayable snapshot must be whole: an introducer
/// followed by its terminator, and never a terminator or raw payload with no introducer.
/// A snapshot that begins inside base64 fails here, which is exactly the truncation bug.
fn assert_only_whole_string_sequences(history: &[u8]) {
    let mut index = 0usize;
    let mut open: Option<usize> = None;
    while index < history.len() {
        if history[index] != 0x1b || index + 1 >= history.len() {
            index += 1;
            continue;
        }
        match history[index + 1] {
            b'_' | b'P' | b'^' | b'X' | b']' => {
                assert!(
                    open.is_none(),
                    "nested string introducer at {index}, previous opened at {:?}",
                    open
                );
                open = Some(index);
                index += 2;
            }
            b'\\' => {
                assert!(
                    open.is_some(),
                    "string terminator at byte {index} has no introducer: snapshot begins \
                     inside a graphics payload"
                );
                open = None;
                index += 2;
            }
            _ => index += 2,
        }
    }
    assert!(
        open.is_none(),
        "unterminated string sequence opened at {:?}",
        open
    );
}

#[test]
fn large_image_payload_does_not_evict_older_text_history() {
    // Given: a session that prints text, then an image far larger than the recovery ring.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("image-flood"));
    let mut stream = Vec::new();
    stream.extend_from_slice(OLDEST_TEXT);
    stream.extend_from_slice(b"\r\n");
    stream.extend_from_slice(&kitty_chunked_image(768 * 1024, 901));
    stream.extend_from_slice(b"\r\n");
    stream.extend_from_slice(NEWEST_TEXT);
    stream.extend_from_slice(b"\r\n");
    assert!(stream.len() > 512 * 1024, "image must outgrow the 512 KiB ring");

    // When: a freshly started GUI reconstructs the pane from the surviving daemon.
    publish_stream(&hub, "image-flood", &stream);
    let history = full_history(&hub, "image-flood");

    // Then: image bytes are charged to their own budget, so text history survives intact.
    assert!(
        contains(&history, OLDEST_TEXT),
        "oldest text was evicted by image payload ({} history bytes)",
        history.len()
    );
    assert!(contains(&history, NEWEST_TEXT), "newest text missing");
}

#[test]
fn full_replay_never_begins_inside_a_graphics_payload() {
    // Given: an image stream whose chunks straddle PTY read boundaries.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("image-boundary"));
    let image = kitty_chunked_image(768 * 1024, 902);
    let expected_chunks = count(&image, b"\x1b_G");
    let mut stream = Vec::new();
    stream.extend_from_slice(b"before\r\n");
    stream.extend_from_slice(&image);
    stream.extend_from_slice(b"after\r\n");

    // When: the full history is replayed into a fresh parser.
    publish_stream(&hub, "image-boundary", &stream);
    let history = full_history(&hub, "image-boundary");

    // Then: every graphics sequence in the snapshot is whole, and the retained image keeps
    // all of its transmission chunks (nothing is half-evicted inside the budget).
    assert_only_whole_string_sequences(&history);
    assert_eq!(
        count(&history, b"\x1b_G"),
        expected_chunks,
        "retained image lost transmission chunks"
    );
}

#[test]
fn image_budget_overflow_drops_whole_images_and_keeps_text() {
    // Given: far more image traffic than any single-session image budget may retain.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("image-budget"));
    let mut stream = Vec::new();
    stream.extend_from_slice(OLDEST_TEXT);
    stream.extend_from_slice(b"\r\n");
    for id in 0..24u32 {
        stream.extend_from_slice(&kitty_chunked_image(1024 * 1024, 1000 + id));
        stream.extend_from_slice(format!("\r\nIMAGE-{id:02}-DONE\r\n").as_bytes());
    }
    stream.extend_from_slice(NEWEST_TEXT);
    stream.extend_from_slice(b"\r\n");

    // When: the pane is reconstructed after the flood.
    publish_stream(&hub, "image-budget", &stream);
    let history = full_history(&hub, "image-budget");

    // Then: retention is bounded, evicted images leave no partial sequence behind, and the
    // text timeline is untouched by image eviction.
    assert!(
        history.len() < stream.len(),
        "history must stay bounded below the produced {} bytes",
        stream.len()
    );
    assert_only_whole_string_sequences(&history);
    assert!(contains(&history, OLDEST_TEXT), "oldest text evicted");
    assert!(contains(&history, NEWEST_TEXT), "newest text evicted");
    for id in 0..24u32 {
        assert!(
            contains(&history, format!("IMAGE-{id:02}-DONE").as_bytes()),
            "text marker for image {id} evicted by image budget"
        );
    }
}

#[test]
fn text_history_stays_bounded_and_keeps_the_newest_output() {
    // Given: sustained plain-text output well past any text budget.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("text-flood"));
    let mut stream = Vec::new();
    stream.extend_from_slice(OLDEST_TEXT);
    stream.extend_from_slice(b"\r\n");
    for line in 0..80_000u32 {
        stream.extend_from_slice(format!("line-{line:06}-payload\r\n").as_bytes());
    }
    stream.extend_from_slice(NEWEST_TEXT);
    stream.extend_from_slice(b"\r\n");

    // When: the pane is reconstructed.
    publish_stream(&hub, "text-flood", &stream);
    let history = full_history(&hub, "text-flood");

    // Then: the newest output always survives and retention is bounded.
    assert!(contains(&history, NEWEST_TEXT), "newest text evicted");
    assert!(
        history.len() < stream.len(),
        "text retention must stay bounded"
    );
}

#[test]
fn full_replay_segments_still_reconstruct_the_flat_history() {
    // Given: a session resized while an image is on screen.
    let hub = TerminalOutputHub::default();
    drop(hub.register_session("resize-image"));
    hub.record_initial_size("resize-image", 80, 24);
    publish_stream(&hub, "resize-image", OLDEST_TEXT);
    hub.record_resize("resize-image", 120, 30)
        .expect("resize recorded");
    publish_stream(&hub, "resize-image", &kitty_chunked_image(600 * 1024, 903));
    hub.record_resize("resize-image", 140, 50)
        .expect("resize recorded");
    publish_stream(&hub, "resize-image", NEWEST_TEXT);

    // When: the full history is taken with its resize segmentation.
    let snapshot = hub
        .subscribe_with_sequence("resize-image", None)
        .expect("session attachment")
        .snapshot;

    // Then: segments remain a lossless partition of the flat history, and the final size is
    // the last recorded one.
    let concatenated: Vec<u8> = snapshot
        .history_segments
        .iter()
        .flat_map(|segment| segment.bytes.clone())
        .collect();
    assert_eq!(concatenated, snapshot.history, "segments must partition history");
    let last = snapshot
        .history_segments
        .last()
        .expect("at least one segment");
    assert_eq!((last.cols, last.rows), (Some(140), Some(50)));
    assert!(contains(&snapshot.history, OLDEST_TEXT));
    assert!(contains(&snapshot.history, NEWEST_TEXT));
}
