use ferryx_lib::native_terminal::snapshot_codec::{
    decode_terminal_snapshot, enable_raw_continuation_tracking, encode_raw_terminal_snapshot,
    encode_terminal_snapshot, encode_terminal_snapshot_buf, query_raw_continuation_max_bytes,
    validate_snapshot_bounds, validate_snapshot_envelope, validate_snapshot_envelope_bounded,
    DecodedTerminal, IncrementalSnapshotDecoder, SnapshotCodecOptions,
    SnapshotProgressKind, DEFAULT_MAX_SNAPSHOT_WIRE_BYTES, KITTY_GRAPHICS_IN_SNAPSHOT_SUPPORTED,
    KITTY_VIRTUAL_PLACEHOLDER_CODEPOINT, MAX_CONTINUATION_CEILING, MIN_SNAPSHOT_WIRE_BYTES,
    SNAPSHOT_MAGIC, SNAPSHOT_VERSION_CURRENT,
};
use ferryx_lib::native_terminal::NativeTerminalError;

/// 1x1 RGBA Kitty image (pixel 00 00 00 ff), transmitted and displayed as image id 1.
const KITTY_RGBA_1X1: &[u8] = b"\x1b_Ga=T,f=32,s=1,v=1,i=1;AAAA/w==\x1b\\";
const KITTY_IMAGE_ID: u32 = 1;
/// Snapshot record tag for one Kitty image's pixels (snapshot/record.zig `kitty_image_payload`).
const KITTY_IMAGE_PAYLOAD_TAG: u16 = 9;

/// Returns the (start, end) byte range of the payload of the first record with `tag`.
fn find_record_payload(snapshot: &[u8], tag: u16) -> Option<(usize, usize)> {
    let mut offset = MIN_SNAPSHOT_WIRE_BYTES;
    while offset + 10 <= snapshot.len() {
        let record_tag = u16::from_le_bytes([snapshot[offset], snapshot[offset + 1]]);
        let len = u32::from_le_bytes(snapshot[offset + 2..offset + 6].try_into().ok()?) as usize;
        let start = offset + 10;
        let end = start.checked_add(len)?;
        if end > snapshot.len() {
            return None;
        }
        if record_tag == tag {
            return Some((start, end));
        }
        offset = end;
    }
    None
}

#[test]
fn test_envelope_validation_and_size_rejection() {
    let tiny = [0u8; 8];
    assert!(validate_snapshot_envelope(&tiny).is_err());

    let exact_short = [0u8; MIN_SNAPSHOT_WIRE_BYTES - 1];
    assert!(validate_snapshot_envelope(&exact_short).is_err());

    let mut bad_magic = [0u8; 10];
    bad_magic[0..8].copy_from_slice(b"BADMAGIC");
    bad_magic[8..10].copy_from_slice(&1u16.to_le_bytes());
    assert!(validate_snapshot_envelope(&bad_magic).is_err());

    let mut bad_version = [0u8; 10];
    bad_version[0..8].copy_from_slice(SNAPSHOT_MAGIC);
    bad_version[8..10].copy_from_slice(&99u16.to_le_bytes());
    assert!(validate_snapshot_envelope(&bad_version).is_err());

    let mut valid_envelope = [0u8; 10];
    valid_envelope[0..8].copy_from_slice(SNAPSHOT_MAGIC);
    valid_envelope[8..10].copy_from_slice(&SNAPSHOT_VERSION_CURRENT.to_le_bytes());
    let version = validate_snapshot_envelope(&valid_envelope).expect("valid envelope");
    assert_eq!(version, SNAPSHOT_VERSION_CURRENT);

    let ceiling_err = validate_snapshot_envelope_bounded(&valid_envelope, 9);
    match ceiling_err {
        Err(NativeTerminalError::LimitExceeded) => {}
        other => panic!("expected LimitExceeded, got {other:?}"),
    }
}

#[test]
fn test_pre_ffi_dimension_and_count_bounding() {
    let options = SnapshotCodecOptions {
        max_cols: 200,
        max_rows: 100,
        max_screens: 2,
        ..Default::default()
    };

    let mut malformed_hdr = Vec::new();
    malformed_hdr.extend_from_slice(SNAPSHOT_MAGIC);
    malformed_hdr.extend_from_slice(&SNAPSHOT_VERSION_CURRENT.to_le_bytes());

    malformed_hdr.extend_from_slice(&1u16.to_le_bytes());
    malformed_hdr.extend_from_slice(&100u32.to_le_bytes());
    malformed_hdr.extend_from_slice(&0u32.to_le_bytes());

    malformed_hdr.extend_from_slice(&65535u16.to_le_bytes());
    malformed_hdr.extend_from_slice(&24u16.to_le_bytes());

    let bound_err = validate_snapshot_bounds(&malformed_hdr, &options);
    match bound_err {
        Err(NativeTerminalError::LimitExceeded) => {}
        other => panic!("expected LimitExceeded for oversized cols, got {other:?}"),
    }

    let decode_err = decode_terminal_snapshot(&malformed_hdr, options);
    assert!(decode_err.is_err());
}

#[test]
fn absent_scrollback_policies_do_not_exceed_decode_limits() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(b"snapshot policy\r\n");
    let snapshot = term.encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES).expect("snapshot");
    let options = SnapshotCodecOptions {
        max_scrollback_rows: 0,
        ..Default::default()
    };
    assert!(validate_snapshot_bounds(&snapshot, &options).is_ok());

    let mut explicit = snapshot.clone();
    explicit[115..123].copy_from_slice(&1_u64.to_le_bytes());
    assert!(matches!(validate_snapshot_bounds(&explicit, &options), Err(NativeTerminalError::LimitExceeded)));
}

#[test]
fn test_continuation_tracking_configuration() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    assert_eq!(term.cols().unwrap(), 80);
    assert_eq!(term.rows().unwrap(), 24);

    term.enable_continuation_tracking(8192)
        .expect("enable continuation tracking");
    let configured = term
        .continuation_max_bytes()
        .expect("query continuation bytes");
    assert_eq!(configured, 8192);

    term.enable_continuation_tracking(MAX_CONTINUATION_CEILING + 1024)
        .expect("clamped ceiling");
    let clamped = term
        .continuation_max_bytes()
        .expect("query clamped continuation");
    assert_eq!(clamped, MAX_CONTINUATION_CEILING);

    let null_raw_track = unsafe { enable_raw_continuation_tracking(std::ptr::null_mut(), 1024) };
    assert!(null_raw_track.is_err());
    let null_raw_query = unsafe { query_raw_continuation_max_bytes(std::ptr::null_mut()) };
    assert!(null_raw_query.is_err());
}

#[test]
fn test_incomplete_utf8_continuation_preservation() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.enable_continuation_tracking(4096)
        .expect("enable continuation");

    let prefix = b"Status: ";
    term.vt_write(prefix);

    let incomplete_utf8 = [0xE2, 0x9C];
    term.vt_write(&incomplete_utf8);

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode snapshot with incomplete utf8");
    assert!(snapshot.len() >= MIN_SNAPSHOT_WIRE_BYTES);
    assert_eq!(&snapshot[0..8], SNAPSHOT_MAGIC);

    let mut decoded =
        decode_terminal_snapshot(&snapshot, SnapshotCodecOptions::default())
            .expect("decode snapshot");
    assert_eq!(decoded.cols().unwrap(), 80);
    assert_eq!(decoded.rows().unwrap(), 24);

    let remaining_utf8 = [0x93];
    decoded.vt_write(&remaining_utf8);
    decoded.vt_write(b" Done\r\n");

    let final_pos = decoded.cursor_position().expect("cursor position");
    assert!(final_pos.1 >= 1);
}

#[test]
fn test_incomplete_csi_continuation_preservation() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.enable_continuation_tracking(4096)
        .expect("enable continuation");

    let incomplete_csi = b"\x1b[38;2;120;200;";
    term.vt_write(incomplete_csi);

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode snapshot with incomplete csi");

    let mut decoded =
        decode_terminal_snapshot(&snapshot, SnapshotCodecOptions::default())
            .expect("decode snapshot");

    let csi_suffix = b"255mColoredText\x1b[0m";
    decoded.vt_write(csi_suffix);

    let cursor = decoded.cursor_position().expect("cursor position");
    assert!(cursor.0 >= 11);
}

#[test]
fn test_primary_alternate_screen_and_saved_state() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(b"Main Screen Line 1\r\n");

    term.vt_write(b"\x1b[?1049h");
    assert!(term.is_alternate_screen().expect("query alt screen"));

    term.vt_write(b"Alt Screen Content\r\n");
    term.vt_write(b"\x1b[6;15H");
    term.vt_write(b"\x1b7");
    term.vt_write(b"\x1b[10;25H");

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode alt screen snapshot");

    let alt_incremental = IncrementalSnapshotDecoder::start(&snapshot, SnapshotCodecOptions::default())
        .expect("start incremental decoder with alt screen");
    assert!(
        alt_incremental.history_rows_alternate().expect("history alternate query").is_some(),
        "declared alternate screen must provide alternate history rows"
    );

    let mut decoded =
        decode_terminal_snapshot(&snapshot, SnapshotCodecOptions::default())
            .expect("decode alt screen snapshot");
    assert!(decoded.is_alternate_screen().expect("decoded alt screen"));

    decoded.vt_write(b"\x1b8");
    let restored_pos = decoded.cursor_position().expect("cursor restored");
    assert_eq!(restored_pos, (14, 5));

    decoded.vt_write(b"\x1b[?1049l");
    assert!(!decoded.is_alternate_screen().expect("back to primary"));
}

#[test]
fn test_malformed_crc_and_truncated_records() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(b"Hello Superlogical VT World!\r\n");

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode snapshot");
    assert!(snapshot.len() > 32);

    let truncated = &snapshot[0..snapshot.len() / 2];
    assert!(decode_terminal_snapshot(truncated, SnapshotCodecOptions::default()).is_err());

    let mut corrupted = snapshot.clone();
    let corrupt_index = corrupted.len() - 8;
    corrupted[corrupt_index] ^= 0xFF;
    assert!(decode_terminal_snapshot(&corrupted, SnapshotCodecOptions::default()).is_err());

    let mut kitty_term = DecodedTerminal::new(80, 24).expect("new kitty terminal");
    kitty_term.vt_write(KITTY_RGBA_1X1);
    let kitty_snapshot = kitty_term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode kitty snapshot");
    assert!(decode_terminal_snapshot(&kitty_snapshot, SnapshotCodecOptions::default()).is_ok());
    let (start, end) = find_record_payload(&kitty_snapshot, KITTY_IMAGE_PAYLOAD_TAG)
        .expect("kitty snapshot carries an image payload record");
    assert!(end > start, "kitty image payload record is not empty");
    let mut kitty_corrupted = kitty_snapshot.clone();
    kitty_corrupted[end - 1] ^= 0xFF;
    assert!(
        decode_terminal_snapshot(&kitty_corrupted, SnapshotCodecOptions::default()).is_err(),
        "a corrupted byte inside the Kitty record must fail its CRC"
    );
}

#[test]
fn test_wire_limits_and_controlled_encode() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(b"Bounded encoding test\r\n");

    let encode_err = encode_terminal_snapshot(&term, 16);
    match encode_err {
        Err(NativeTerminalError::LimitExceeded) => {}
        other => panic!("expected LimitExceeded, got {other:?}"),
    }

    let mut small_buf = [0u8; 16];
    let buf_err = encode_terminal_snapshot_buf(&term, &mut small_buf);
    assert!(buf_err.is_err());

    let null_raw_err = unsafe { encode_raw_terminal_snapshot(std::ptr::null_mut(), 1024) };
    assert!(null_raw_err.is_err());

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("valid snapshot");

    let decode_opts = SnapshotCodecOptions {
        max_wire_bytes: snapshot.len() - 1,
        ..Default::default()
    };
    let decode_limit_err = decode_terminal_snapshot(&snapshot, decode_opts);
    match decode_limit_err {
        Err(NativeTerminalError::LimitExceeded) => {}
        other => panic!("expected LimitExceeded on decode, got {:?}", other.err()),
    }

    let high_continuation_opts = SnapshotCodecOptions {
        max_continuation_bytes: MAX_CONTINUATION_CEILING + 1,
        ..Default::default()
    };
    let ceil_err = decode_terminal_snapshot(&snapshot, high_continuation_opts);
    match ceil_err {
        Err(NativeTerminalError::LimitExceeded) => {}
        other => panic!("expected LimitExceeded on continuation ceiling, got {:?}", other.err()),
    }
}

#[test]
fn test_incremental_decoder_and_next_page() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    for i in 0..60 {
        let line = format!("Scrollback row {i}\r\n");
        term.vt_write(line.as_bytes());
    }

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode snapshot with history");

    let mut incremental =
        IncrementalSnapshotDecoder::start(&snapshot, SnapshotCodecOptions::default())
            .expect("start incremental decoder");
    assert_eq!(incremental.terminal().cols().unwrap(), 80);
    assert_eq!(incremental.terminal().rows().unwrap(), 24);

    let offset = incremental.source_offset().expect("source offset");
    assert!(offset > 0);

    let _ = incremental.history_rows_primary().expect("history primary");
    assert_eq!(
        incremental.history_rows_alternate().expect("history alternate"),
        None,
        "snapshot without alternate screen returns None for alternate history rows"
    );

    while incremental.next_page().expect("next page") {}

    let mut restored = incremental.finish();
    restored.vt_write(b"Appended after full replay\r\n");
}

#[test]
fn test_incremental_decoder_early_drop_safety() {
    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(b"Early drop test\r\n");
    let snapshot = term.encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES).expect("encode");

    let incremental =
        IncrementalSnapshotDecoder::start(&snapshot, SnapshotCodecOptions::default())
            .expect("start incremental decoder");
    drop(incremental);
}

#[test]
fn kitty_image_round_trips_through_snapshot() {
    const { assert!(KITTY_GRAPHICS_IN_SNAPSHOT_SUPPORTED) };
    assert_eq!(KITTY_VIRTUAL_PLACEHOLDER_CODEPOINT, 0x10EEEE);

    let mut term = DecodedTerminal::new(80, 24).expect("new terminal");
    term.vt_write(KITTY_RGBA_1X1);
    assert_eq!(
        term.kitty_image_pixels(KITTY_IMAGE_ID).expect("source image query"),
        Some(vec![0, 0, 0, 255]),
        "source terminal stores the transmitted image"
    );

    let snapshot = term
        .encode(DEFAULT_MAX_SNAPSHOT_WIRE_BYTES)
        .expect("encode kitty snapshot");

    let mut incremental =
        IncrementalSnapshotDecoder::start(&snapshot, SnapshotCodecOptions::default())
            .expect("start incremental decoder");
    assert_eq!(incremental.kitty_images_primary().expect("kitty images primary"), 1);

    let mut image_records = 0;
    while incremental.next_page().expect("next record") {
        match incremental.progress_kind().expect("progress kind") {
            SnapshotProgressKind::Image => {
                image_records += 1;
                assert_eq!(incremental.progress_rows().expect("rows on image"), None);
                assert_eq!(incremental.progress_remaining().expect("remaining on image"), None);
            }
            SnapshotProgressKind::Page => {
                assert!(incremental.progress_rows().expect("rows on page").is_some());
                assert!(incremental.progress_remaining().expect("remaining on page").is_some());
            }
        }
    }
    assert_eq!(image_records, 1, "exactly one Kitty image payload record");

    let decoded = incremental.finish();
    assert_eq!(
        decoded.kitty_image_pixels(KITTY_IMAGE_ID).expect("decoded image query"),
        Some(vec![0, 0, 0, 255]),
        "decoded terminal holds the Kitty image with its pixels"
    );
}
