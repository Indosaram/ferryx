use super::*;
use crate::terminal::output_hub::TerminalOutputHub;

#[tokio::test]
async fn two_consumers_isolation_stalled_viewer_overflows_while_controller_progresses() {
    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "test-two-consumers-hub";
    hub.register_session(session_id);

    let mut controller = hub.subscribe_machine(session_id, None).unwrap().unwrap();
    let mut viewer = hub.subscribe_machine(session_id, None).unwrap().unwrap();

    let session = hub.sessions.read().get(session_id).cloned().unwrap();
    assert_eq!(session.read().machine_senders.len(), 2);

    let chunk_payload = vec![0x55; 64 * 1024];
    let mut controller_received = 0usize;

    for _ in 0..20 {
        hub.publish(session_id, chunk_payload.clone()).unwrap();
        if let Ok(charged) = controller.receiver.recv().await {
            controller_received += charged.value.bytes.len();
        }
    }

    assert!(controller_received > 0, "Controller must receive published chunks");

    let viewer_res = viewer.receiver.recv().await;
    assert_eq!(viewer_res.unwrap_err(), MachineOutputError::Overflow);

    hub.publish(session_id, vec![0xaa; 100]).unwrap();
    assert_eq!(
        session.read().machine_senders.len(),
        1,
        "Hub must retain only the healthy controller"
    );

    let post_chunk = controller.receiver.recv().await.unwrap();
    assert_eq!(post_chunk.value.bytes.as_ref(), &[0xaa; 100]);

    let viewer_budget = viewer.receiver.budget.clone();
    drop(viewer);
    assert_eq!(
        viewer_budget.available_permits(),
        MACHINE_OUTPUT_BYTES,
        "All permits must be reclaimed upon viewer drop"
    );

    let last_seq = post_chunk.value.sequence;
    let mut reconnected = hub.subscribe_machine(session_id, Some(last_seq)).unwrap().unwrap();

    hub.publish(session_id, b"reconnected".to_vec()).unwrap();
    let ctrl_msg = controller.receiver.recv().await.unwrap();
    let view_msg = reconnected.receiver.recv().await.unwrap();
    assert_eq!(ctrl_msg.value.bytes.as_ref(), b"reconnected");
    assert_eq!(view_msg.value.bytes.as_ref(), b"reconnected");
}

#[tokio::test]
async fn stalled_consumer_drop_releases_all_permits() {
    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "test-permit-cleanup";
    hub.register_session(session_id);

    let mut attachment = hub.subscribe_machine(session_id, None).unwrap().unwrap();
    let budget = attachment.receiver.budget.clone();

    drop(attachment.snapshot);
    assert_eq!(
        budget.available_permits(),
        MACHINE_OUTPUT_BYTES - MACHINE_CONTROL_BYTES
    );

    for _ in 0..25 {
        hub.publish(session_id, vec![0xbb; 64 * 1024]);
    }

    let err = attachment.receiver.recv().await;
    assert_eq!(err.unwrap_err(), MachineOutputError::Overflow);

    drop(attachment.receiver);
    assert_eq!(
        budget.available_permits(),
        MACHINE_OUTPUT_BYTES,
        "Zero permit leakage after stalled receiver drop"
    );
}

#[tokio::test]
async fn queue_entry_cap_overflow_triggers_when_exceeding_max_entries() {
    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "test-queue-entry-cap";
    hub.register_session(session_id);

    let mut attachment = hub.subscribe_machine(session_id, None).unwrap().unwrap();

    for _ in 0..(MAX_ENTRIES + 10) {
        hub.publish(session_id, vec![0x01]);
    }

    let err = attachment.receiver.recv().await;
    assert_eq!(
        err.unwrap_err(),
        MachineOutputError::Overflow,
        "Entry saturation beyond MAX_ENTRIES must trigger Overflow"
    );
}

#[tokio::test]
async fn reconnect_with_sequence_cursor_resynchronizes() {
    let hub = TerminalOutputHub::new(10 * 1024 * 1024);
    let session_id = "test-reconnect-seq";
    hub.register_session(session_id);

    let chunk1 = hub.publish(session_id, b"chunk1".to_vec()).unwrap();
    let chunk2 = hub.publish(session_id, b"chunk2".to_vec()).unwrap();

    let attachment = hub.subscribe_machine(session_id, Some(chunk1.sequence)).unwrap().unwrap();
    assert_eq!(attachment.snapshot.value.history_start_sequence, Some(chunk2.sequence));
    assert_eq!(attachment.snapshot.value.history, b"chunk2");
    assert!(attachment.snapshot.value.gap.is_none());
}

#[test]
fn replay_exceeding_budget_fails_closed_at_admission() {
    let hub = TerminalOutputHub::new(2 * MACHINE_OUTPUT_BYTES);
    let session_id = "test-overbudget-replay";
    hub.register_session(session_id);
    hub.publish(session_id, vec![0x77; MACHINE_OUTPUT_BYTES]);

    let result = hub.subscribe_machine(session_id, None).unwrap();
    assert_eq!(result.unwrap_err(), MachineOutputError::Overflow);

    let suffix_result = hub.subscribe_machine(session_id, Some(1)).unwrap();
    assert!(suffix_result.is_ok());
}
