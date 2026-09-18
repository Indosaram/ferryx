//! Resource contracts, not a renderer/UDS end-to-end claim.
use ferryx_lib::{remote::{machine_protocol::RemoteTerminalTarget, terminal_wire::{encode_frame, decode_frame, Metadata, MAX_FRAME_BYTES}}, scoped_contracts::Epoch, terminal::{output_hub::TerminalOutputHub, paired_daemon::{Descriptor, Proxy}}};
use std::sync::Arc;

#[tokio::test]
async fn a23_slow_consumer_is_bounded_and_drop_removes_host_qualified_entries() {
    let hub = Arc::new(TerminalOutputHub::new(32));
    let descriptor = Descriptor { host_id: "a".into(), generation: Epoch(1), target: RemoteTerminalTarget { machine_id: "machine".into(), daemon_epoch: Epoch(1), session_id: "same".into() }, after_sequence: None };
    let a = Proxy::new(descriptor.clone(), hub.clone()).unwrap();
    let mut other = descriptor.clone();
    other.host_id = "b".into();
    let b = Proxy::new(other, hub.clone()).unwrap();
    assert_ne!(a.id(), b.id());
    assert!(Proxy::new(descriptor.clone(), hub.clone()).is_err());
    let mut slow = hub.subscribe_with_sequence(a.id(), None).unwrap().receiver;
    for _ in 0..4096 { hub.publish(a.id(), vec![b'x'; 16]); }
    assert!(matches!(slow.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))));
    let snapshot = hub.subscribe_with_sequence(a.id(), None).unwrap().snapshot;
    assert_eq!(snapshot.history.len(), 32);
    assert!(hub.subscribe_with_sequence(b.id(), None).unwrap().snapshot.history.is_empty());
    let ids = [a.id().to_owned(), b.id().to_owned()];
    drop(a);
    drop(b);
    // R4-N3: an unconfirmed exit releases the transport claim but retains the
    // host-qualified entry so the next proxy can adopt the retained history.
    let other_b = Descriptor { host_id: "b".into(), generation: Epoch(1), target: RemoteTerminalTarget { machine_id: "machine".into(), daemon_epoch: Epoch(1), session_id: "same".into() }, after_sequence: None };
    for id in &ids {
        assert!(!hub.transport_owner(id));
        assert!(hub.has_session(id));
    }
    assert!(Proxy::new(descriptor, hub.clone()).is_ok());
    assert!(Proxy::new(other_b, hub.clone()).is_ok());
    // Explicitly confirmed exits clear the entries and synchronously close the
    // retained broadcast queues.
    for id in &ids { hub.remove_session(id); }
    // Drain a finite retained broadcast queue; closure is synchronous, not time-based.
    while slow.try_recv().is_ok() {}
    assert!(matches!(slow.try_recv(), Err(tokio::sync::broadcast::error::TryRecvError::Closed)));
}

#[test]
fn a23_wire_limit_rejects_oversized_frames() {
    let metadata = Metadata::Output { sequence: 1, gap: None };
    let frame = encode_frame(metadata.clone(), b"output", false).unwrap();
    assert!(frame.len() <= MAX_FRAME_BYTES);
    assert_eq!(decode_frame(&frame).unwrap().terminal_bytes, b"output");
    assert!(encode_frame(metadata, &vec![0; MAX_FRAME_BYTES + 1], false).is_err());
    assert!(decode_frame(&vec![0; MAX_FRAME_BYTES + 1]).is_err());
}
