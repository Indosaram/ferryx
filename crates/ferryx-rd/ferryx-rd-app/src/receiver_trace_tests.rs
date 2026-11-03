#[test]
fn receiver_trace_attributes_invalid_assembly() {
    let dir = tempfile::tempdir().unwrap();
    let trace = crate::ReceiverTrace::at_path(dir.path().join("trace.json"));
    let mut frames = crate::FrameAssembler::default();
    frames.set_receiver_trace(trace.clone());
    let now = Instant::now();
    assert!(frames
        .push_header(
            ferryx_rd_proto::FrameHeader {
                frame_id: 55,
                width: 1,
                height: 1,
                is_key_frame: false,
                total_chunks: 0,
                total_size: 1
            },
            0,
            now
        )
        .is_err());
    assert!(trace
        .snapshot()
        .unwrap()
        .unwrap()
        .records
        .iter()
        .any(|r| r.event == crate::ReceiverTraceEvent::AssemblyInvalid && r.frame == Some(55)));
}

#[test]
fn receiver_trace_attributes_assembly_timeout_capacity_and_completion() {
    let dir = tempfile::tempdir().unwrap();
    let trace = crate::ReceiverTrace::at_path(dir.path().join("trace.json"));
    let mut frames = crate::FrameAssembler::default();
    frames.set_receiver_trace(trace.clone());
    let now = Instant::now();
    for frame_id in 0..17 {
        frames
            .push_header(
                ferryx_rd_proto::FrameHeader {
                    frame_id,
                    width: 1,
                    height: 1,
                    is_key_frame: false,
                    total_chunks: 1,
                    total_size: 1,
                },
                0,
                now,
            )
            .unwrap();
    }
    frames
        .push_chunk(
            ferryx_rd_proto::FrameChunk {
                frame_id: 16,
                chunk_index: 0,
                data: vec![1],
            },
            now,
        )
        .unwrap();
    frames
        .push_chunk(
            ferryx_rd_proto::FrameChunk {
                frame_id: 99,
                chunk_index: 0,
                data: vec![1],
            },
            now + Duration::from_secs(1),
        )
        .unwrap();
    let records = trace.snapshot().unwrap().unwrap().records;
    assert!(records
        .iter()
        .any(|r| r.event == crate::ReceiverTraceEvent::AssemblyCapacity && r.frame == Some(0)));
    assert!(records
        .iter()
        .any(|r| r.event == crate::ReceiverTraceEvent::AssemblyComplete && r.frame == Some(16)));
    assert_eq!(
        records
            .iter()
            .filter(|r| r.event == crate::ReceiverTraceEvent::AssemblyTimeout)
            .count(),
        15
    );
}

#[test]
fn receiver_trace_attributes_gap_deadline_eviction_and_late() {
    let dir = tempfile::tempdir().unwrap();
    let trace = crate::ReceiverTrace::at_path(dir.path().join("trace.json"));
    let now = Instant::now();
    let mut stats = ReceiverStats::default();
    stats.set_trace(trace.clone());
    stats.observe_datagram(10, 100, now);
    stats.observe_datagram(12, 100, now);
    stats.observe_datagram(11, 100, now + Duration::from_millis(101));
    let snapshot = stats.snapshot(now + Duration::from_millis(101));
    assert_eq!(
        (
            snapshot.loss_expected_packets,
            snapshot.loss_missing_packets
        ),
        (3, 1)
    );
    let records = trace.snapshot().unwrap().unwrap().records;
    assert!(records
        .iter()
        .any(|r| r.event == crate::ReceiverTraceEvent::GapGrace
            && r.sequence == Some(11)
            && r.count == 1));
    assert!(records.iter().any(
        |r| r.event == crate::ReceiverTraceEvent::LateOutsidePending && r.sequence == Some(11)
    ));
    stats.observe_datagram(10_000, 100, now + Duration::from_millis(102));
    assert!(trace
        .snapshot()
        .unwrap()
        .unwrap()
        .records
        .iter()
        .any(|r| r.event == crate::ReceiverTraceEvent::GapEvicted));
}
