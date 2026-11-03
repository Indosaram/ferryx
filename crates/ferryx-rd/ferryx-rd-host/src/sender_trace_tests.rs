use super::*;

#[test]
fn sender_failed_sink_records_attempt_error_and_nonce_progression() {
    // Given the actual encryption/send seam and an isolated trace sink.
    let directory = tempfile::tempdir().unwrap();
    let trace = host_trace::Trace::for_test(directory.path().join("host.json"));
    let mut sender = UdpSender {
        trace: Some(Arc::clone(&trace)),
        ..Default::default()
    };
    let mut cipher =
        DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
    let mut receiver =
        DatagramCipher::derive(&[0x31; 32], &[0x72; 16], Direction::HostToClient).unwrap();
    let mut emitted = Vec::new();
    // When only attempted packet 2 fails, without rollback or retry.
    for sequence in 1..=3 {
        let result = sender.send_selected_packet(
            &mut cipher,
            PacketType::FrameChunk,
            &[7, 0, 0, 0, 0, 0],
            |bytes| {
                if sequence == 2 {
                    return Err(io::Error::from_raw_os_error(105));
                }
                emitted.push(bytes.to_vec());
                Ok(bytes.len())
            },
        );
        assert_eq!(result.is_err(), sequence == 2);
    }
    // Then authenticated emissions have one sequence gap and distinct nonces.
    let sequences: Vec<_> = emitted
        .iter()
        .map(|packet| receiver.open_datagram(packet).unwrap().0.sequence)
        .collect();
    assert_eq!(sequences, [1, 3]);
    assert_ne!(
        &emitted[0][PacketHeader::SIZE..PacketHeader::SIZE + 12],
        &emitted[1][PacketHeader::SIZE..PacketHeader::SIZE + 12]
    );
    let records = trace.records.lock().unwrap();
    let actions: Vec<_> = records
        .records
        .iter()
        .map(|r| (r.sequence, r.event))
        .collect();
    assert_eq!(actions, [(1, 1), (1, 2), (2, 1), (2, 3), (3, 1), (3, 2)]);
    assert_eq!(records.records[3].value, 105);
    assert!(records
        .records
        .iter()
        .all(|r| r.frame == 7 && r.size == emitted[0].len()));
    drop(records);
    trace.dump().unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("host.json")).unwrap()).unwrap();
    assert_eq!(json["records"].as_array().unwrap().len(), 6);
}

#[test]
fn sender_trace_retains_first_fixed_records_and_overflow() {
    let mut records = host_trace::Records::default();
    for sequence in 0..65_539 {
        records.push(host_trace::Record {
            sequence,
            event: 1,
            kind: 2,
            size: 123,
            frame: 7,
            ..Default::default()
        });
    }
    assert_eq!(records.records.len(), 65_536);
    assert_eq!(records.overflow, 3);
    assert_eq!(records.records[65_535].sequence, 65_535);
    assert_eq!(records.records[0].frame, 7);
}

#[test]
fn windows_stage_attribution_events_record_and_serialize() {
    let directory = tempfile::tempdir().unwrap();
    let trace = host_trace::Trace::for_test(directory.path().join("attribution.json"));

    // Stage attribution events across pipeline:
    // 21/22: cursor send start/end
    // 23/24: video output send start/end
    // 25/26: MFT sample alloc/copy start/end
    // 27/28: ProcessInput start/end
    // 33/34: ProcessOutput start/end
    // 35/36: bitrate SetValue start/end
    // 37/38: bitrate Recreate start/end
    // 39: selected MFT identity & backend
    // 40: total encode wall-time
    let events: [(u8, u64, u64, usize, u8, bool); 17] = [
        (21, 100, 0, 0, 0, false),            // cursor send start
        (22, 100, 45, 0, 0, false),           // cursor send end: 45us
        (25, 100, 0, 9_216_000, 0, false),    // sample alloc/copy start
        (26, 100, 1200, 9_216_000, 0, false), // sample alloc/copy end: 1200us
        (27, 100, 0, 0, 0, true),             // ProcessInput start (keyframe)
        (28, 100, 340, 0, 0, false),          // ProcessInput end: 340us
        (33, 100, 0, 0, 0, false),            // ProcessOutput start
        (34, 100, 850, 42_000, 0, true),      // ProcessOutput end: 850us, 42KB, keyframe
        (40, 100, 2390, 0, 0, false),         // total encode wall-time: 2390us
        (23, 100, 0, 42_000, 0, true),        // video output send start
        (24, 100, 60, 0, 0, false),           // video output send end: 60us
        (35, 0, 5_000_000, 0, 0, false),      // bitrate SetValue start: 5Mbps
        (36, 0, 5_000_000, 150, 0, false),    // bitrate SetValue end: 150us
        (37, 0, 6_000_000, 0, 0, false),      // bitrate Recreate start: 6Mbps
        (38, 0, 6_000_000, 45_000, 0, false), // bitrate Recreate end: 45ms
        (39, 0, 0x6ca5_0344, 1, 1, false),    // selected MFT: backend=1 (SW), codec=1 (HEVC), clsid
        (20, 100, 123_456, 0, 0, true),       // send_frame start
    ];

    for (event, frame, value, size, kind, keyframe) in events {
        trace.record(host_trace::Record {
            event,
            frame,
            value,
            size,
            kind,
            keyframe,
            ..Default::default()
        });
    }

    let records = trace.records.lock().unwrap();
    assert_eq!(records.records.len(), 17);
    assert_eq!(records.overflow, 0);
    assert_eq!(records.records[0].event, 21);
    assert_eq!(records.records[1].event, 22);
    assert_eq!(records.records[1].value, 45);
    assert_eq!(records.records[8].event, 40);
    assert_eq!(records.records[8].value, 2390);
    assert_eq!(records.records[15].event, 39);
    assert_eq!(records.records[15].kind, 1);
    assert_eq!(records.records[15].size, 1);
    assert_eq!(records.records[15].value, 0x6ca5_0344);
    drop(records);

    trace.dump().unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.path().join("attribution.json")).unwrap())
            .unwrap();
    let array = json["records"].as_array().unwrap();
    assert_eq!(array.len(), 17);
    assert_eq!(array[0]["event"], 21);
    assert_eq!(array[1]["event"], 22);
    assert_eq!(array[1]["value"], 45);
    assert_eq!(array[8]["event"], 40);
    assert_eq!(array[8]["value"], 2390);
    assert_eq!(array[15]["event"], 39);
    assert_eq!(array[15]["kind"], 1);
    assert_eq!(array[15]["size"], 1);
}
