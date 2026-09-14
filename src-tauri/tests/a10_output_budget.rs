use ferryx_lib::terminal::output_hub::{TerminalOutputHub, machine_output::*};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn pending_output_stays_within_one_mib_when_consumer_is_held() {
    // Given a subscribed but held machine-path receiver.
    let hub = TerminalOutputHub::default();
    hub.register_session("held");
    let held = hub.subscribe_machine("held", None).unwrap().unwrap();
    // When unequal chunks exceed the byte ceiling without reaching the count cap.
    for size in [700_000, 400_000] {
        hub.publish("held", vec![b'x'; size]).unwrap();
    }
    // Then pending output remains bounded and termination is independently visible.
    assert!(held.receiver.pending_bytes() <= MACHINE_OUTPUT_BYTES);
    assert_eq!(*held.receiver.termination().borrow(), Some(MachineOutputError::Overflow));
}

#[tokio::test]
async fn charge_survives_receive_when_frame_is_in_flight() {
    // Given unequal sizes totaling the exact available payload budget.
    let hub = TerminalOutputHub::default();
    hub.register_session("held");
    let MachineAttachment { snapshot, mut receiver } = hub.subscribe_machine("held", None).unwrap().unwrap();
    drop(snapshot);
    let first_size = 333_333;
    let second_size = MACHINE_OUTPUT_BYTES - MACHINE_CONTROL_BYTES - 2 * MACHINE_FRAME_OVERHEAD - first_size;
    hub.publish("held", vec![1; first_size]).unwrap();
    hub.publish("held", vec![2; second_size]).unwrap();
    let before = receiver.pending_bytes();
    // When the first output is consumed but the write has not completed.
    let in_flight = receiver.recv().await.unwrap();
    // Then its charge remains until actual completion/drop.
    assert_eq!(before, MACHINE_OUTPUT_BYTES);
    assert_eq!(receiver.pending_bytes(), before);
    drop(in_flight);
    assert_eq!(receiver.pending_bytes(), before - first_size - MACHINE_FRAME_OVERHEAD);
    let second = receiver.recv().await.unwrap();
    drop(second);
    assert_eq!(receiver.pending_bytes(), MACHINE_CONTROL_BYTES);
}

#[tokio::test]
async fn overflow_signals_when_writer_holds_an_in_flight_frame() {
    // Given a frame filling every available byte and a separately subscribed signal.
    let hub = TerminalOutputHub::default();
    hub.register_session("held");
    let MachineAttachment { snapshot, mut receiver } = hub.subscribe_machine("held", None).unwrap().unwrap();
    drop(snapshot);
    hub.publish("held", vec![1; MACHINE_OUTPUT_BYTES - MACHINE_CONTROL_BYTES - MACHINE_FRAME_OVERHEAD]).unwrap();
    let frame = receiver.recv().await.unwrap();
    let mut signal = receiver.termination();
    // When one more payload byte is published while the writer cannot progress.
    hub.publish("held", vec![2]).unwrap();
    // Then the signal completes without requiring another receive or releasing the frame.
    timeout(Duration::from_secs(2), signal.wait_for(|state| state.is_some())).await.unwrap().unwrap();
    assert_eq!(*signal.borrow(), Some(MachineOutputError::Overflow));
    assert_eq!(receiver.pending_bytes(), MACHINE_OUTPUT_BYTES);
    drop(frame);
}

#[tokio::test]
async fn payload_is_released_when_subscription_is_dropped() {
    // Given a queued payload with no history or legacy receivers retaining it.
    let hub = TerminalOutputHub::new(0);
    hub.register_session("held");
    let held = hub.subscribe_machine("held", None).unwrap().unwrap();
    let chunk = hub.publish("held", vec![1; 4096]).unwrap();
    let weak = Arc::downgrade(&chunk.bytes);
    drop(chunk);
    assert!(weak.upgrade().is_some());
    // When the subscription is dropped without another publish.
    drop(held);
    // Then the queued payload is released immediately.
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn zero_byte_boundaries_are_bounded_when_consumer_is_held() {
    // Given an idle subscriber.
    let hub = TerminalOutputHub::default();
    hub.register_session("held");
    let held = hub.subscribe_machine("held", None).unwrap().unwrap();
    // When zero-payload replay controls exceed entry capacity.
    for _ in 0..1025 { hub.publish_gap("held").unwrap(); }
    // Then control entries cannot grow without bound.
    assert_eq!(*held.receiver.termination().borrow(), Some(MachineOutputError::Overflow));
    assert!(held.receiver.pending_bytes() <= MACHINE_OUTPUT_BYTES);
}

#[tokio::test]
async fn snapshot_precedes_live_output_when_replay_has_a_gap() {
    // Given evicted history and a cursor before it.
    let hub = TerminalOutputHub::new(4);
    hub.register_session("held");
    hub.publish("held", b"old!".to_vec());
    hub.publish("held", b"new!".to_vec());
    let MachineAttachment { snapshot, mut receiver } = hub.subscribe_machine("held", Some(0)).unwrap().unwrap();
    // When a publisher emits after the atomic attachment.
    let chunk = hub.publish("held", b"live".to_vec()).unwrap();
    // Then replay ends immediately before live output and Arc payload identity survives.
    assert_eq!(snapshot.value.history, b"new!");
    assert_eq!(snapshot.value.gap.unwrap().available_from_sequence, 2);
    let received = receiver.recv().await.unwrap();
    assert_eq!(received.value.sequence, snapshot.value.history_end_sequence.unwrap() + 1);
    assert!(Arc::ptr_eq(&chunk.bytes, &received.value.bytes));
}

#[test]
fn replay_is_rejected_when_history_exceeds_machine_budget() {
    // Given a hub with a legacy history capacity larger than the machine ceiling.
    let hub = TerminalOutputHub::new(2 * MACHINE_OUTPUT_BYTES);
    hub.register_session("held");
    hub.publish("held", vec![1; MACHINE_OUTPUT_BYTES]);
    // When attaching without a suffix cursor.
    let result = hub.subscribe_machine("held", None).unwrap();
    // Then admission rejects rather than allocating an over-budget machine replay.
    assert!(matches!(result, Err(MachineOutputError::Overflow)));
    assert!(hub.subscribe_machine("held", Some(1)).unwrap().is_ok());
}
