use std::time::{Duration, Instant};

use ferryx_rd_app::{CursorState, FrameAssembler, MediaAssemblyError};
use ferryx_rd_proto::{CursorUpdate, FrameChunk, FrameHeader, MAX_CHUNKS_PER_FRAME, MAX_FRAME_BYTES};

#[test]
fn video_chunks_reassemble_out_of_order_and_from_orphans() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    assert_eq!(
        assembler
            .push_chunk(
                FrameChunk {
                    frame_id: 9,
                    chunk_index: 1,
                    data: b"world".to_vec(),
                },
                now,
            )
            .unwrap(),
        None
    );
    let header = FrameHeader {
        frame_id: 9,
        width: 640,
        height: 360,
        is_key_frame: true,
        total_chunks: 2,
        total_size: 10,
    };
    assert_eq!(assembler.push_header(header, 123, now).unwrap(), None);
    let frame = assembler
        .push_chunk(
            FrameChunk {
                frame_id: 9,
                chunk_index: 0,
                data: b"hello".to_vec(),
            },
            now,
        )
        .unwrap()
        .unwrap();
    assert_eq!(frame.data, b"helloworld");
    assert_eq!(frame.timestamp_ms, 123);
}

#[test]
fn video_reassembly_enforces_protocol_caps() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    let too_many = FrameHeader {
        frame_id: 1,
        width: 1,
        height: 1,
        is_key_frame: false,
        total_chunks: MAX_CHUNKS_PER_FRAME + 1,
        total_size: 1,
    };
    assert_eq!(
        assembler.push_header(too_many, 0, now),
        Err(MediaAssemblyError::InvalidHeader)
    );
    let too_large = FrameHeader {
        frame_id: 2,
        width: 1,
        height: 1,
        is_key_frame: false,
        total_chunks: 1,
        total_size: MAX_FRAME_BYTES + 1,
    };
    assert_eq!(
        assembler.push_header(too_large, 0, now),
        Err(MediaAssemblyError::InvalidHeader)
    );
}

#[test]
fn cursor_updates_are_clamped_for_overlay_use() {
    let mut cursor = CursorState::default();
    cursor.update(CursorUpdate {
        x: -0.2,
        y: 1.4,
        cursor_type: 3,
    });
    assert_eq!(cursor.x, 0.0);
    assert_eq!(cursor.y, 1.0);
    assert_eq!(cursor.cursor_type, 3);
}

fn orphan_chunk(frame_id: u32, chunk_index: u16, data: &[u8]) -> FrameChunk {
    FrameChunk {
        frame_id,
        chunk_index,
        data: data.to_vec(),
    }
}

fn orphan_header(frame_id: u32) -> FrameHeader {
    FrameHeader {
        frame_id,
        width: 1,
        height: 1,
        is_key_frame: false,
        total_chunks: 1,
        total_size: 3,
    }
}

#[test]
fn expired_orphans_release_reorder_capacity() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    for frame_id in 1..=16 {
        assembler
            .push_chunk(orphan_chunk(frame_id, 0, b"old"), now)
            .unwrap();
    }

    let later = now + Duration::from_secs(1) + Duration::from_nanos(1);
    for frame_id in 17..=32 {
        assembler
            .push_chunk(orphan_chunk(frame_id, 0, b"new"), later)
            .unwrap();
    }

    for frame_id in 17..=32 {
        let frame = assembler
            .push_header(orphan_header(frame_id), 123, later)
            .unwrap();
        assert_eq!(
            frame.as_ref().map(|frame| frame.data.as_slice()),
            Some(b"new".as_slice())
        );
        assert_eq!(frame.unwrap().timestamp_ms, 123);
    }
}

#[test]
fn orphan_survival_matches_assembly_timeout_boundary() {
    let now = Instant::now();
    for (arrival, survives) in [
        (now - Duration::from_nanos(1), true),
        (now + Duration::from_secs(1) - Duration::from_nanos(1), true),
        (now + Duration::from_secs(1), false),
        (
            now + Duration::from_secs(1) + Duration::from_nanos(1),
            false,
        ),
    ] {
        let mut assembler = FrameAssembler::default();
        assembler
            .push_chunk(orphan_chunk(1, 0, b"old"), now)
            .unwrap();

        let frame = assembler.push_header(orphan_header(1), 0, arrival).unwrap();

        assert_eq!(
            frame.map(|frame| frame.data),
            survives.then(|| b"old".to_vec())
        );
    }
}

#[test]
fn newest_orphan_survives_expiry_of_older_records() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    for frame_id in 1..16 {
        assembler
            .push_chunk(orphan_chunk(frame_id, 0, b"old"), now)
            .unwrap();
    }
    let newer = now + Duration::from_millis(500);
    assembler
        .push_chunk(orphan_chunk(16, 0, b"new"), newer)
        .unwrap();
    assembler
        .push_chunk(orphan_chunk(16, 0, b"dup"), newer)
        .unwrap();

    let boundary = now + Duration::from_secs(1);
    assembler
        .push_chunk(orphan_chunk(17, 0, b"end"), boundary)
        .unwrap();
    let newest = assembler
        .push_header(orphan_header(17), 0, boundary)
        .unwrap();
    let survivor = assembler
        .push_header(orphan_header(16), 0, boundary)
        .unwrap();

    assert_eq!(newest.map(|frame| frame.data), Some(b"end".to_vec()));
    assert_eq!(survivor.map(|frame| frame.data), Some(b"new".to_vec()));
}

#[test]
fn later_orphan_chunks_do_not_refresh_expiry() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    assembler
        .push_chunk(orphan_chunk(1, 1, b"end"), now)
        .unwrap();
    let later = now + Duration::from_millis(999);
    assembler
        .push_chunk(orphan_chunk(1, 1, b"dup"), later)
        .unwrap();
    assembler
        .push_chunk(orphan_chunk(1, 0, b"new"), later)
        .unwrap();
    let mut header = orphan_header(1);
    header.total_chunks = 2;
    header.total_size = 6;

    let frame = assembler
        .push_header(header, 0, now + Duration::from_secs(1))
        .unwrap();

    assert_eq!(frame, None);
}

#[test]
fn clear_discards_orphans_and_preserves_header_first_recovery() {
    let now = Instant::now();
    let mut assembler = FrameAssembler::default();
    for frame_id in 1..=16 {
        assembler
            .push_chunk(orphan_chunk(frame_id, 0, b"old"), now)
            .unwrap();
    }

    assembler.clear();
    assert_eq!(
        assembler.push_header(orphan_header(1), 456, now).unwrap(),
        None
    );
    let frame = assembler
        .push_chunk(orphan_chunk(1, 0, b"new"), now)
        .unwrap()
        .unwrap();

    assert_eq!(frame.data, b"new");
    assert_eq!(frame.timestamp_ms, 456);
    assert_eq!(assembler.completed_frames(), 1);
}
