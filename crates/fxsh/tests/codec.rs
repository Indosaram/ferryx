use fxsh::frame::{FLAG_EVENT, FLAG_RESPONSE, HEADER_LEN, MAX_PAYLOAD};
use fxsh::messages::*;
use fxsh::types::*;
use fxsh::wire::Reader;
use fxsh::{decode_complete_frame, decode_frame, Bytes, Codec, Decoded, Digest, FatalFrameError, Frame, Message, PayloadError, Reason, Uuid};
use proptest::prelude::*;

fn uuid(b: u8) -> Uuid {
    Uuid([b; 16])
}

fn roundtrip(message: Message) {
    let frame = Frame { session_id: uuid(7), request_id: 42, flags: FLAG_RESPONSE, message };
    let bytes = frame.encode();
    match decode_complete_frame(&bytes).expect("header accepted") {
        Decoded::Frame { consumed, frame: back, .. } => {
            assert_eq!(consumed, bytes.len());
            assert_eq!(back, frame);
            assert_eq!(back.encode(), bytes, "re-encode must be byte identical");
        }
        other => panic!("frame rejected: {other:?}"),
    }
}

fn sample_row(cols: u16) -> RowData {
    let mut cells = vec![Cell::BLANK; cols as usize];
    cells[0] = Cell {
        codepoint: 'e' as u32,
        grapheme_extra: Some("\u{301}".into()),
        width: 1,
        fg: Color::Indexed(3),
        bg: Color::Rgb(1, 2, 3),
        underline_color: Color::Default,
        attrs: 0b101,
        hyperlink_id: 1,
    };
    RowData { wrapped: true, cells }
}

fn sample_sbody() -> SBody {
    SBody {
        cols: 4,
        rows: 2,
        screen: vec![sample_row(4), RowData { wrapped: false, cells: vec![Cell::BLANK; 4] }],
        cursor: Cursor { row: 1, col: 2, style: CursorStyle::Bar, blinking: true, visible: true },
        modes: Modes { flags: 1 | (1 << 9), mouse_mode: MouseMode::Button, mouse_encoding: MouseEncoding::Sgr },
        palette: Palette {
            default_fg: Color::Rgb(200, 200, 200),
            default_bg: Color::Default,
            cursor_color: Some(Color::Indexed(7)),
            overrides: vec![(1, (0x12, (0x34, 0x56))), (9, (1, (2, 3)))],
        },
        title: "fixture-title".into(),
        hyperlinks: vec![Hyperlink { id: 1, uri: "https://ex.com/1".into() }],
        scrollback: vec![(10, sample_row(4)), (11, RowData { wrapped: false, cells: vec![Cell::BLANK; 4] })],
        exit_info: None,
    }
}

fn op() -> OperationId {
    OperationId { host_instance_id: uuid(9), op_seq: 5 }
}

fn every_message() -> Vec<Message> {
    let body = sample_sbody();
    vec![
        Message::Hello(Hello { major: 1, minor: 0, capabilities: fxsh::CAP_V1_REQUIRED, client_kind: ClientKind::PolicyDaemon, client_instance_id: uuid(1) }),
        Message::HelloAck(HelloAck { major: 1, minor: 0, capabilities: 31, host_instance_id: uuid(2), supervisor: Supervisor::Systemd, pid: 4242, process_start_time: 123456789, host_version: "0.1.0".into() }),
        Message::Spawn(Spawn { operation_id: op(), cols: 200, rows: 60, program: "/bin/zsh".into(), args: vec!["-l".into()], env: vec![("TERM".into(), "xterm-256color".into())], cwd: "/home/u".into() }),
        Message::SpawnResult(SpawnResult { operation_id: op(), session_id: uuid(3), session_incarnation: uuid(4), pid: 99, created: true, created_table_revision: 17 }),
        Message::AcquireLease(AcquireLease { client_instance_id: uuid(5), lease_request_id: 1, scope: Scope::Input }),
        Message::LeaseGranted(LeaseGranted { client_instance_id: uuid(5), lease_request_id: 1, scope: Scope::Input, epoch: 3, prior: Some(PriorEpoch { epoch: 2, final_accepted: 10, committed: 9 }), resize_applied: None }),
        Message::LeaseGranted(LeaseGranted { client_instance_id: uuid(5), lease_request_id: 2, scope: Scope::Resize, epoch: 8, prior: None, resize_applied: Some(ResizeApplied { epoch: 7, seq: 4, cols: 80, rows: 24 }) }),
        Message::ReleaseLease(ReleaseLease { client_instance_id: uuid(5), lease_request_id: 3, scope: Scope::Resize, epoch: 8 }),
        Message::LeaseReleased(LeaseReleased { client_instance_id: uuid(5), lease_request_id: 3, scope: Scope::Resize, epoch: 8, released: true }),
        Message::LeaseRevoked(LeaseRevoked { holder_instance_id: uuid(5), scope: Scope::Input, revoked_epoch: 3, new_epoch: 0 }),
        Message::Reclaim(Reclaim { client_instance_id: uuid(5), lease_request_id: 4, scope: Scope::Input, epoch: 3 }),
        Message::ReclaimResult(ReclaimResult { client_instance_id: uuid(5), lease_request_id: 4, scope: Scope::Input, requested_epoch: 3, ok: true, current_epoch: 3, input: Some(InputProgress { accepted: 10, committed: 9 }), input_final: None, resize: None }),
        Message::ReclaimResult(ReclaimResult { client_instance_id: uuid(5), lease_request_id: 5, scope: Scope::Input, requested_epoch: 3, ok: false, current_epoch: 4, input: None, input_final: Some(InputFinal { final_accepted: 10, committed: 10, pending_dropped: 0 }), resize: None }),
        Message::ReclaimResult(ReclaimResult { client_instance_id: uuid(5), lease_request_id: 6, scope: Scope::Resize, requested_epoch: 8, ok: false, current_epoch: 0, input: None, input_final: None, resize: Some(ResizeApplied { epoch: 8, seq: 2, cols: 1, rows: 1 }) }),
        Message::WriteInput(WriteInput { client_instance_id: uuid(5), epoch: 3, start: 0, bytes: Bytes(b"ls\r".to_vec()), crc32c: fxsh::input_crc32c(b"ls\r") }),
        Message::InputAck(InputAck { client_instance_id: uuid(5), epoch: 3, accepted: 3, committed: 3 }),
        Message::GetEpochState(GetEpochState { client_instance_id: uuid(5), epoch: 2 }),
        Message::EpochState(EpochState { epoch: 2, final_accepted: 10, committed: 9, pending_dropped: 1, fenced: true }),
        Message::Resize(Resize { resize_epoch: 8, resize_seq: 9, cols: 1024, rows: 512 }),
        Message::ResizeAck(ResizeAck { epoch: 8, seq: 9, cols: 1024, rows: 512 }),
        Message::Subscribe(Subscribe { subscriber_id: uuid(6), attach_seq: 2, client_known_revision: Some(77), client_known_incarnation: None }),
        Message::SubscribeAck(SubscribeAck { attach_seq: 2, subscription_id: 1, session_incarnation: uuid(4), revision: 0, snapshot_follows: true }),
        Message::SnapshotFrame(SnapshotFrame { subscription_id: 1, session_incarnation: uuid(4), snapshot_id: 1, payload: SnapshotPayload::Full(Body { revision: 77, state_digest: state_digest(&body), next_ui_event_id: 5, ui_event_gap: false, state: body.clone() }) }),
        Message::SnapshotFrame(SnapshotFrame { subscription_id: 1, session_incarnation: uuid(4), snapshot_id: 2, payload: SnapshotPayload::Chunk { index: 0, total: 2, total_len: 10, bytes: Bytes(vec![1, 2, 3, 4, 5]) } }),
        Message::Delta(Delta {
            subscription_id: 1, base_revision: 77, new_revision: 78, size: Some((4, 2)), cursor: None, modes: None, palette: None, title: Some("t".into()),
            dirty_rows: vec![(0, sample_row(4))], hyperlinks_added: vec![Hyperlink { id: 2, uri: "u".into() }], scrollback_appended: vec![(12, sample_row(4))],
            scrollback_evicted_before: Some(10), exit_info: Some(ExitInfo { exit_code: Some(0), posix_signal: None }),
            ui_events: vec![
                UiEvent { event_id: 5, kind: UiEventKind::ClipboardWrite { target: ClipboardTarget::Primary, text: "copy".into() } },
                UiEvent { event_id: 6, kind: UiEventKind::Notification { title: "".into(), body: "done".into() } },
                UiEvent { event_id: 7, kind: UiEventKind::Bell },
            ],
        }),
        Message::Resync(Resync { subscription_id: 1 }),
        Message::Unsubscribe(Unsubscribe { subscription_id: 1 }),
        Message::GetSessionState,
        Message::SessionState(SessionState { session_incarnation: uuid(4), state_revision: 78, state_digest: Digest([9; 16]), input_epoch: 3, input_accepted: 3, input_committed: 3, resize_epoch: 8, resize_seq: 9, cols: 4, rows: 2, child_running: false, exit_info: Some(ExitInfo { exit_code: None, posix_signal: Some(9) }), pending_dropped_by_epoch: vec![(2, 1)], vt_replies_dropped: 0, creation_operation_id: op() }),
        Message::ListSessions(ListSessions { request_token: (uuid(8), 11) }),
        Message::SessionList(SessionList { request_token: (uuid(8), 11), owner_instance_id: uuid(2), table_revision: 18, complete: true, sessions: vec![SessionEntry { session_id: uuid(3), session_incarnation: uuid(4), created_table_revision: 17, creation_operation_id: op(), child_running: true, exit_info: None }] }),
        Message::Kill(Kill { operation_id: op(), signal: Signal::Terminate }),
        Message::KillResult(KillResult { operation_id: op(), delivered: false }),
        Message::ChildExited(ChildExited { session_incarnation: uuid(4), exit_code: Some(1), posix_signal: None, table_revision: 19, pending_dropped_by_epoch: vec![] }),
        Message::Error(ErrorDetail::StaleLease { scope: Scope::Input, current_epoch: 4, epoch_final_accepted: 10 }.to_frame("stale")),
        Message::ReserveOperation(ReserveOperation { kind: OperationKind::Kill }),
        Message::OperationReserved(OperationReserved { operation_id: op() }),
        Message::LeaseVacated(LeaseVacated { holder_instance_id: uuid(5), scope: Scope::Resize, epoch: 8 }),
    ]
}

#[test]
fn every_command_roundtrips_byte_identically() {
    let msgs = every_message();
    let mut seen: Vec<u16> = msgs.iter().map(Message::command).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, (0x01u16..=0x22).collect::<Vec<_>>(), "every command id 0x01..=0x22 is covered");
    for m in msgs {
        roundtrip(m);
    }
}

#[test]
fn header_golden_bytes() {
    let frame = Frame { session_id: Uuid([0xAB; 16]), request_id: 0x0102_0304_0506_0708, flags: FLAG_EVENT, message: Message::Resync(Resync { subscription_id: 0x1122 }) };
    let bytes = frame.encode();
    let mut expected = vec![0x46, 0x58, 0x53, 0x48, 0, 1, 0, 0, 0, 0x16, 0, 4];
    expected.extend_from_slice(&[0xAB; 16]);
    expected.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    expected.extend_from_slice(&[0, 0, 0, 8]);
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0x11, 0x22]);
    assert_eq!(bytes.len(), HEADER_LEN + 8);
    assert_eq!(bytes, expected);
}

#[test]
fn payload_golden_bytes_for_opt_list_str_and_color() {
    let mut w = Vec::new();
    Some(5u64).enc(&mut w);
    None::<u64>.enc(&mut w);
    vec![("a".to_string(), "bc".to_string())].enc(&mut w);
    Color::Rgb(1, 2, 3).enc(&mut w);
    Color::Indexed(9).enc(&mut w);
    Color::Default.enc(&mut w);
    assert_eq!(
        w,
        vec![1, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 1, 0, 0, 0, 1, b'a', 0, 0, 0, 2, b'b', b'c', 2, 1, 2, 3, 1, 9, 0]
    );
}

#[test]
fn canonical_encoding_is_deterministic_and_digest_changes_with_state() {
    let a = sample_sbody();
    assert_eq!(canonical_encode(&a), canonical_encode(&a.clone()));
    assert_eq!(state_digest(&a), state_digest(&a.clone()));
    let mut b = a.clone();
    b.screen[1].cells[3].codepoint = 'x' as u32;
    assert_ne!(state_digest(&a), state_digest(&b));
}

fn payload_of(m: &Message) -> Vec<u8> {
    let mut w = Vec::new();
    m.encode_payload(&mut w);
    w
}

fn reject(command: u16, payload: &[u8]) -> Reason {
    match Message::decode_payload(command, payload) {
        Err(PayloadError::Violation(r)) => r,
        other => panic!("expected violation, got {other:?}"),
    }
}

#[test]
fn malformed_primitives_are_protocol_violations() {
    let ack = payload_of(&Message::LeaseReleased(LeaseReleased { client_instance_id: uuid(1), lease_request_id: 1, scope: Scope::Input, epoch: 1, released: true }));
    let mut bad_bool = ack.clone();
    *bad_bool.last_mut().unwrap() = 2;
    assert_eq!(reject(0x08, &bad_bool), Reason::BadBool);

    let mut bad_enum = ack.clone();
    bad_enum[16 + 8] = 9;
    assert_eq!(reject(0x08, &bad_enum), Reason::BadEnum);

    assert_eq!(reject(0x08, &ack[..ack.len() - 1]), Reason::Truncated);

    let mut bad_utf8 = Vec::new();
    1u16.enc(&mut bad_utf8);
    bad_utf8.extend_from_slice(&[0, 0, 0, 2, 0xC3, 0x28]);
    bad_utf8.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(reject(0x1F, &bad_utf8), Reason::BadUtf8);

    let mut bad_opt_tag = payload_of(&Message::Subscribe(Subscribe { subscriber_id: uuid(1), attach_seq: 1, client_known_revision: None, client_known_incarnation: None }));
    bad_opt_tag[24] = 7;
    assert_eq!(reject(0x12, &bad_opt_tag), Reason::BadTag);
}

#[test]
fn present_iff_conditions_are_enforced() {
    let base = LeaseGranted { client_instance_id: uuid(1), lease_request_id: 1, scope: Scope::Resize, epoch: 1, prior: None, resize_applied: None };
    assert_eq!(reject(0x06, &payload_of(&Message::LeaseGranted(base.clone()))), Reason::OptCondition);
    let with_prior = LeaseGranted { prior: Some(PriorEpoch { epoch: 0, final_accepted: 0, committed: 0 }), resize_applied: Some(ResizeApplied { epoch: 1, seq: 0, cols: 1, rows: 1 }), ..base };
    assert_eq!(reject(0x06, &payload_of(&Message::LeaseGranted(with_prior))), Reason::OptCondition);

    let rr = ReclaimResult { client_instance_id: uuid(1), lease_request_id: 1, scope: Scope::Input, requested_epoch: 3, ok: true, current_epoch: 3, input: None, input_final: None, resize: None };
    assert_eq!(reject(0x0B, &payload_of(&Message::ReclaimResult(rr.clone()))), Reason::OptCondition);
    let wrong_epoch = ReclaimResult { current_epoch: 4, input: Some(InputProgress { accepted: 0, committed: 0 }), ..rr };
    assert_eq!(reject(0x0B, &payload_of(&Message::ReclaimResult(wrong_epoch))), Reason::BadShape);

    let mut st = match every_message().into_iter().find(|m| m.command() == 0x19).unwrap() { Message::SessionState(s) => s, _ => unreachable!() };
    st.child_running = true;
    assert_eq!(reject(0x19, &payload_of(&Message::SessionState(st))), Reason::OptCondition);
}

#[test]
fn ordering_and_shape_rules_are_enforced() {
    let mut s = sample_sbody();
    s.palette.overrides = vec![(9, (0, (0, 0))), (1, (0, (0, 0)))];
    let mut w = Vec::new();
    s.enc(&mut w);
    assert_eq!(SBody::dec(&mut Reader::new(&w)), Err(Reason::BadOrder));

    let mut s = sample_sbody();
    s.scrollback[1].1.cells.pop();
    let mut w = Vec::new();
    s.enc(&mut w);
    assert_eq!(SBody::dec(&mut Reader::new(&w)), Err(Reason::BadShape));

    let mut s = sample_sbody();
    s.screen[0].cells[1].attrs = 1 << 11;
    let mut w = Vec::new();
    s.enc(&mut w);
    assert_eq!(SBody::dec(&mut Reader::new(&w)), Err(Reason::BadShape));

    let chunk = SnapshotFrame { subscription_id: 1, session_incarnation: uuid(1), snapshot_id: 1, payload: SnapshotPayload::Chunk { index: 2, total: 2, total_len: 4, bytes: Bytes(vec![0; 4]) } };
    assert_eq!(reject(0x14, &payload_of(&Message::SnapshotFrame(chunk))), Reason::ChunkInconsistent);
}

#[test]
fn every_error_code_detail_roundtrips() {
    let all = vec![
        ErrorDetail::StaleLease { scope: Scope::Resize, current_epoch: 0, epoch_final_accepted: 0 },
        ErrorDetail::InputGap { accepted: 5 },
        ErrorDetail::InputDiverged { epoch: 1, accepted: 2, committed: 3 },
        ErrorDetail::InputUnverifiable { epoch: 1, accepted: 2, committed: 3, retained_start: 1 },
        ErrorDetail::InputBackpressure { accepted: 9 },
        ErrorDetail::CorruptFrame,
        ErrorDetail::StaleResize { applied_epoch: 1, applied_seq: 2, cols: 3, rows: 4 },
        ErrorDetail::SessionNotFound,
        ErrorDetail::OperationConflict(op()),
        ErrorDetail::OperationExpired(op()),
        ErrorDetail::OperationUnknown(op()),
        ErrorDetail::SlowConsumer { subscription_id: 4 },
        ErrorDetail::VersionUnsupported { host_major: 1, host_minor: 0 },
        ErrorDetail::UnsupportedCapability { capability_bit: 4 },
        ErrorDetail::UnknownCommand { command: 0x99 },
        ErrorDetail::LimitExceeded { limit_kind: 8, limit: 64 },
        ErrorDetail::ProtocolViolation(Reason::ChunkInconsistent),
        ErrorDetail::StaleViewport,
        ErrorDetail::StaleSubscribe { current_attach_seq: 3 },
        ErrorDetail::EpochUnknown { epoch: 2 },
        ErrorDetail::InstanceRetired,
        ErrorDetail::OwnerUnreachable,
        ErrorDetail::SpawnFailed,
    ];
    let codes: Vec<u16> = all.iter().map(ErrorDetail::code).collect();
    assert_eq!(codes, (1u16..=23).collect::<Vec<_>>());
    for d in all {
        let frame = d.to_frame("m");
        assert_eq!(ErrorDetail::decode_detail(frame.code, &frame.detail.0), Ok(d.clone()));
        roundtrip(Message::Error(frame));
    }
}

fn header_bytes(magic: u32, flags: u16, command: u16, payload_len: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&magic.to_be_bytes());
    b.extend_from_slice(&[0, 1, 0, 0]);
    b.extend_from_slice(&command.to_be_bytes());
    b.extend_from_slice(&flags.to_be_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&[0; 8]);
    b.extend_from_slice(&payload_len.to_be_bytes());
    b
}

#[test]
fn fatal_header_errors_and_unknown_command() {
    assert_eq!(decode_frame(&header_bytes(0xDEAD_BEEF, 0, 0x16, 0)), Err(FatalFrameError::BadMagic));
    assert_eq!(decode_frame(&header_bytes(fxsh::frame::MAGIC, 1 << 3, 0x16, 0)), Err(FatalFrameError::UndefinedFlags(8)));
    assert_eq!(decode_frame(&header_bytes(fxsh::frame::MAGIC, 0, 0x16, MAX_PAYLOAD + 1)), Err(FatalFrameError::PayloadTooLarge(MAX_PAYLOAD + 1)));
    assert_eq!(decode_frame(&header_bytes(fxsh::frame::MAGIC, 0, 0x16, 8)), Ok(Decoded::NeedMore));
    assert_eq!(decode_complete_frame(&header_bytes(fxsh::frame::MAGIC, 0, 0x16, 8)), Err(FatalFrameError::Truncated));
    match decode_frame(&header_bytes(fxsh::frame::MAGIC, 0, 0x7777, 0)) {
        Ok(Decoded::Rejected { error: PayloadError::UnknownCommand(0x7777), consumed: 40, .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn trailing_bytes_from_newer_minor_are_ignored() {
    let mut p = payload_of(&Message::Resync(Resync { subscription_id: 3 }));
    p.extend_from_slice(&[0xFF, 0xEE]);
    assert_eq!(Message::decode_payload(0x16, &p), Ok(Message::Resync(Resync { subscription_id: 3 })));
}

#[test]
fn streaming_decode_handles_back_to_back_frames() {
    let a = Frame { session_id: uuid(1), request_id: 1, flags: 0, message: Message::Resync(Resync { subscription_id: 1 }) }.encode();
    let b = Frame { session_id: uuid(1), request_id: 2, flags: 0, message: Message::GetSessionState }.encode();
    let mut buf = a.clone();
    buf.extend_from_slice(&b);
    let Decoded::Frame { consumed, .. } = decode_frame(&buf).unwrap() else { panic!() };
    assert_eq!(consumed, a.len());
    let Decoded::Frame { frame, .. } = decode_frame(&buf[consumed..]).unwrap() else { panic!() };
    assert_eq!(frame.message, Message::GetSessionState);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 20_000, .. ProptestConfig::default() })]

    #[test]
    fn arbitrary_payload_bytes_never_panic(command in 0u16..0x30, payload in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = Message::decode_payload(command, &payload);
    }

    #[test]
    fn arbitrary_stream_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let _ = decode_frame(&bytes);
    }

    #[test]
    fn mutated_valid_frames_never_panic(idx in 0usize..37, pos in any::<usize>(), val in any::<u8>()) {
        let msgs = every_message();
        let m = &msgs[idx % msgs.len()];
        let mut bytes = Frame { session_id: uuid(1), request_id: 1, flags: 0, message: m.clone() }.encode();
        let p = pos % bytes.len();
        bytes[p] = val;
        if let Ok(Decoded::Frame { frame, .. }) = decode_frame(&bytes) {
            let again = frame.encode();
            let reparsed = matches!(decode_complete_frame(&again), Ok(Decoded::Frame { .. }));
            prop_assert!(reparsed);
        }
    }
}
