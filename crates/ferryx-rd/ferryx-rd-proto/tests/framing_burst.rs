use ferryx_rd_proto::{TcpFrameEvent, TcpFrameReader, TcpFrameWriter, MAX_TCP_FRAME_SIZE};

#[test]
fn reader_has_no_production_counter_storage() {
    // Integration tests link the library without cfg(test).
    assert_eq!(
        std::mem::size_of::<TcpFrameReader>(),
        std::mem::size_of::<Vec<u8>>()
    );
}

#[test]
fn test_coalesced_burst_exact_bytes_order_and_subsequent_completion() {
    let mut reader = TcpFrameReader::new();

    let mut coalesced = Vec::with_capacity(1024 * 64 + 7);
    let mut expected_payloads = Vec::with_capacity(1025);

    for i in 0..1024 {
        let payload = vec![(i % 251) as u8; 60];
        let encoded = TcpFrameWriter::encode(&payload).unwrap();
        coalesced.extend_from_slice(&encoded);
        expected_payloads.push(payload);
    }

    // Frame 1025: length 16 (0x10), payload 16 bytes.
    let tail_payload = vec![0x77; 16];
    let tail_encoded = TcpFrameWriter::encode(&tail_payload).unwrap();
    expected_payloads.push(tail_payload);

    // Split tail: 7 bytes in first push, 13 bytes in second push.
    coalesced.extend_from_slice(&tail_encoded[..7]);

    let first_events = reader.push(&coalesced);
    assert_eq!(
        first_events,
        expected_payloads[..1024]
            .iter()
            .cloned()
            .map(TcpFrameEvent::Frame)
            .collect::<Vec<_>>()
    );
    assert_eq!(reader.buffered_len(), 7);

    // Push the remaining 13 bytes of the 1025th frame.
    let second_events = reader.push(&tail_encoded[7..]);
    assert_eq!(second_events.len(), 1);
    assert_eq!(reader.buffered_len(), 0);

    // Verify 1025th frame payload.
    match &second_events[0] {
        TcpFrameEvent::Frame(payload) => {
            assert_eq!(payload, &expected_payloads[1024]);
        }
        TcpFrameEvent::DroppedInvalidLength(len) => {
            panic!("unexpected drop {len}");
        }
    }
}

#[test]
fn test_malformed_tail_discarding_semantics_unchanged() {
    for invalid_len in [0_u32, (MAX_TCP_FRAME_SIZE as u32) + 1] {
        let mut reader = TcpFrameReader::new();

        let mut coalesced = Vec::with_capacity(1024 * 64 + 32);
        for i in 0..1024 {
            let payload = vec![(i % 251) as u8; 60];
            coalesced.extend_from_slice(&TcpFrameWriter::encode(&payload).unwrap());
        }

        // Malformed tail: invalid length header followed by trailing garbage.
        coalesced.extend_from_slice(&invalid_len.to_le_bytes());
        coalesced.extend_from_slice(b"trailing garbage to be discarded");

        let events = reader.push(&coalesced);
        assert_eq!(events.len(), 1025);

        // First 1024 events are valid frames in exact order.
        for (i, event) in events[..1024].iter().enumerate() {
            match event {
                TcpFrameEvent::Frame(payload) => {
                    assert_eq!(payload, &vec![(i % 251) as u8; 60]);
                }
                TcpFrameEvent::DroppedInvalidLength(len) => {
                    panic!("unexpected drop {len} at index {i}");
                }
            }
        }

        // 1025th event is DroppedInvalidLength.
        assert_eq!(
            events[1024],
            TcpFrameEvent::DroppedInvalidLength(invalid_len)
        );

        // Entire buffer must be cleared (0 buffered bytes).
        assert_eq!(reader.buffered_len(), 0);

        // Verify resynchronization on subsequent push.
        let resync_payload = b"resynchronized_frame".to_vec();
        let resync_events = reader.push(&TcpFrameWriter::encode(&resync_payload).unwrap());
        assert_eq!(resync_events, vec![TcpFrameEvent::Frame(resync_payload)]);
        assert_eq!(reader.buffered_len(), 0);
    }
}
