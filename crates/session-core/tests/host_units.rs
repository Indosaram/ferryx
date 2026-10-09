use fxsh::types::OperationKind;
use fxsh::Uuid;
use session_core::input::{InputLedger, WriteError, MAX_UNWRITTEN_EPOCHS, RETAINED_BYTES, VT_REPLY_LIMIT};
use session_core::operations::{request_hash, Decision, OperationTable, COMPLETED_TTL_MS, RESERVED_TTL_MS};
use session_core::replica::normalize_size;
use session_core::resize::{ResizeError, ResizeLease};

const A: [u8; 16] = [1; 16];
const B: [u8; 16] = [2; 16];

fn crc(b: &[u8]) -> u32 {
    fxsh::input_crc32c(b)
}

#[test]
fn ledger_accepts_suffix_rejects_divergence_and_gap() {
    let mut l = InputLedger::new();
    let e = l.acquire(A).unwrap().epoch;
    assert_eq!(l.write(A, e, 0, b"abc", crc(b"abc")).unwrap().accepted, 3);
    assert_eq!(l.write(A, e, 1, b"bcde", crc(b"bcde")).unwrap().accepted, 5);
    assert_eq!(l.write(A, e, 0, b"abcde", crc(b"abcde")).unwrap().accepted, 5);
    assert!(matches!(l.write(A, e, 9, b"x", crc(b"x")), Err((WriteError::Gap { accepted: 5 }, None))));
    assert!(matches!(l.write(A, e, 0, b"abc", 1), Err((WriteError::CorruptFrame, None))));
    match l.write(A, e, 2, b"Xd", crc(b"Xd")) {
        Err((WriteError::Diverged { .. }, Some((revoke, _)))) => assert_eq!(revoke.new_epoch, 0),
        other => panic!("{other:?}"),
    }
    assert!(matches!(l.write(A, e, 5, b"f", crc(b"f")), Err((WriteError::StaleLease { .. }, None))));
}

#[test]
fn ledger_queue_drains_in_order_and_counts_committed() {
    let mut l = InputLedger::new();
    let e = l.acquire(A).unwrap().epoch;
    l.write(A, e, 0, b"hello", crc(b"hello")).unwrap();
    assert!(l.push_vt_reply(b"\x1b[0n".to_vec()));
    let mut out = Vec::new();
    while let Some(h) = l.head().map(|h| h.to_vec()) {
        let n = h.len().min(2);
        out.extend_from_slice(&h[..n]);
        l.on_written(n);
    }
    assert_eq!(out, b"hello\x1b[0n");
    assert_eq!(l.epoch(e).unwrap().committed, 5);
}

#[test]
fn backpressure_and_vt_reply_limits() {
    let mut l = InputLedger::new();
    let e = l.acquire(A).unwrap().epoch;
    let chunk = vec![b'x'; 64 * 1024];
    let mut off = 0;
    loop {
        match l.write(A, e, off, &chunk, crc(&chunk)) {
            Ok(a) => off = a.accepted,
            Err((WriteError::Backpressure { accepted }, None)) => {
                assert_eq!(accepted, off);
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(off, 1024 * 1024);
    let mut n = 0;
    while l.push_vt_reply(vec![0; 1024]) {
        n += 1;
    }
    assert_eq!(n, VT_REPLY_LIMIT / 1024);
    assert_eq!(l.vt_replies_dropped, 1);
}

#[test]
fn handover_fences_previous_epoch_and_reports_prior() {
    let mut l = InputLedger::new();
    let e1 = l.acquire(A).unwrap().epoch;
    l.write(A, e1, 0, b"abc", crc(b"abc")).unwrap();
    l.on_written(1);
    let g = l.acquire(B).unwrap();
    assert_eq!(g.prior, Some((e1, 3, 1)));
    assert_eq!(g.revoked.unwrap().holder, A);
    assert!(l.epoch(e1).unwrap().fenced);
    assert!(l.reclaim(A, e1).unwrap().is_err());
    l.on_written(2);
    assert_eq!(l.epoch_state(e1), Some((3, 3, 0, true)));
    let (released, vacated) = l.release(B, g.epoch);
    assert!(released);
    assert_eq!(vacated, vec![A]);
}

#[test]
fn dead_pty_drops_pending_per_epoch() {
    let mut l = InputLedger::new();
    let e = l.acquire(A).unwrap().epoch;
    l.write(A, e, 0, b"abcdef", crc(b"abcdef")).unwrap();
    l.on_written(2);
    l.on_dead();
    assert_eq!(l.pending_dropped_by_epoch(), vec![(e, 4)]);
    assert!(l.head().is_none());
}

#[test]
fn unwritten_epoch_cap() {
    let mut l = InputLedger::new();
    for _ in 0..MAX_UNWRITTEN_EPOCHS {
        let e = l.acquire(A).unwrap().epoch;
        l.write(A, e, 0, b"x", crc(b"x")).unwrap();
    }
    assert!(l.acquire(A).is_err());
    l.on_written(1);
    assert!(l.acquire(A).is_ok());
}

#[test]
fn resize_lease_orders_and_fences() {
    let mut r = ResizeLease::new(80, 24);
    let (e1, applied, _) = r.acquire(A);
    assert_eq!(applied.seq, 0);
    assert_eq!(r.resize(A, e1, 1, 2000, 0).unwrap(), session_core::resize::Applied { epoch: e1, seq: 1, cols: 1024, rows: 1 });
    assert!(matches!(r.resize(A, e1, 1, 10, 10), Err(ResizeError::StaleResize(_))));
    let (e2, _, revoked) = r.acquire(B);
    assert_eq!(revoked, Some((A, e1)));
    assert!(matches!(r.resize(A, e1, 2, 10, 10), Err(ResizeError::StaleLease { current_epoch }) if current_epoch == e2));
    assert_eq!(normalize_size(0, 9999), (1, 512));
}

#[test]
fn operation_table_idempotency() {
    let host = Uuid([7; 16]);
    let mut t = OperationTable::new(host);
    let id = t.reserve(OperationKind::Kill, 0);
    let h1 = request_hash(0x1C, &[1; 16], b"payload");
    let h2 = request_hash(0x1C, &[2; 16], b"payload");
    assert_ne!(h1, h2, "hash includes the target session");
    assert_eq!(t.decide(&id, OperationKind::Kill, h1), Decision::Execute);
    t.complete(&id, h1, b"result".to_vec(), None, 10);
    assert_eq!(t.decide(&id, OperationKind::Kill, h1), Decision::Replay(b"result".to_vec()));
    assert_eq!(t.decide(&id, OperationKind::Kill, h2), Decision::Conflict);
    assert_eq!(t.decide(&id, OperationKind::Spawn, h1), Decision::Conflict);
    let foreign = fxsh::types::OperationId { host_instance_id: Uuid([8; 16]), op_seq: id.op_seq };
    assert_eq!(t.decide(&foreign, OperationKind::Kill, h1), Decision::Unknown);
    let future = fxsh::types::OperationId { host_instance_id: host, op_seq: id.op_seq + 5 };
    assert_eq!(t.decide(&future, OperationKind::Kill, h1), Decision::Unknown);
    t.evict(10 + COMPLETED_TTL_MS, |_| None);
    assert_eq!(t.decide(&id, OperationKind::Kill, h1), Decision::Expired);

    let s = t.reserve(OperationKind::Spawn, 0);
    let hs = request_hash(0x03, &[0; 16], b"spawn");
    t.complete(&s, hs, b"ok".to_vec(), Some([3; 16]), 0);
    t.evict(10 * COMPLETED_TTL_MS, |_| None);
    assert_eq!(t.decide(&s, OperationKind::Spawn, hs), Decision::Replay(b"ok".to_vec()), "live session keeps its spawn result");
    t.evict(10 * COMPLETED_TTL_MS, |_| Some(0));
    assert_eq!(t.decide(&s, OperationKind::Spawn, hs), Decision::Expired);

    let r = t.reserve(OperationKind::Spawn, 0);
    t.evict(RESERVED_TTL_MS, |_| None);
    assert_eq!(t.decide(&r, OperationKind::Spawn, hs), Decision::Expired);
}

#[test]
fn retransmit_below_the_retained_window_is_unverifiable() {
    let mut l = InputLedger::new();
    let e = l.acquire(A).unwrap().epoch;
    let chunk = vec![b'q'; 64 * 1024];
    let mut off = 0u64;
    while off < (RETAINED_BYTES + chunk.len()) as u64 {
        off = l.write(A, e, off, &chunk, crc(&chunk)).unwrap().accepted;
        while let Some(n) = l.head().map(|h| h.len()) {
            l.on_written(n);
        }
    }
    match l.write(A, e, 0, &chunk, crc(&chunk)) {
        Err((WriteError::Unverifiable { retained_start, accepted, .. }, Some((revoke, _)))) => {
            assert_eq!(retained_start, accepted - RETAINED_BYTES as u64);
            assert_eq!((revoke.holder, revoke.new_epoch), (A, 0));
        }
        other => panic!("{other:?}"),
    }
    assert!(l.epoch(e).unwrap().fenced);
}
