use bytes::Bytes;
use ferryx_lib::daemon::{DaemonAttachment, DaemonStreamMessage};
use ferryx_lib::native_terminal::surface_host::NativeTerminalSurfaceHostState;
use ferryx_lib::native_terminal::{ScrollViewport, TerminalEngine};
use std::borrow::Cow;

fn attachment(
    session_id: &str,
    history: &[u8],
    start_sequence: Option<u64>,
    end_sequence: Option<u64>,
) -> (
    tokio::sync::mpsc::Sender<DaemonStreamMessage<'static>>,
    DaemonAttachment,
) {
    attachment_at_epoch(session_id, history, start_sequence, end_sequence, 1)
}

fn attachment_at_epoch(
    session_id: &str,
    history: &[u8],
    start_sequence: Option<u64>,
    end_sequence: Option<u64>,
    epoch: u64,
) -> (
    tokio::sync::mpsc::Sender<DaemonStreamMessage<'static>>,
    DaemonAttachment,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    (
        tx,
        DaemonAttachment {
            session_id: session_id.to_string(),
            epoch,
            start_sequence,
            end_sequence,
            gap: None,
            history: Bytes::copy_from_slice(history),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages: rx,
            stream_task: tokio::spawn(std::future::pending()),
        },
    )
}

/// Counts matches across the full screen *including retained scrollback*; the visible viewport
/// alone cannot distinguish preserved history from history the attach silently dropped.
fn occurrences(state: &NativeTerminalSurfaceHostState, session_id: &str, needle: &str) -> usize {
    state
        .with_session_terminal(session_id, |terminal| terminal.search_grid(needle, true))
        .expect("resident terminal is reachable")
        .len()
}

#[tokio::test]
async fn resident_reattach_with_contiguous_delta_keeps_earlier_scrollback() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-contiguous-attach";

    let (_first_tx, first) = attachment(session_id, b"resident-origin-line\r\n", Some(1), Some(5));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // Sequences 6..=7 continue 5 exactly, so the resident grid stays authoritative through 5.
    let (_second_tx, second) = attachment(session_id, b"reconnect-delta-line\r\n", Some(6), Some(7));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach with contiguous delta");

    assert_eq!(
        occurrences(&state, session_id, "resident-origin-line"),
        1,
        "a contiguous delta must preserve resident scrollback"
    );
    assert_eq!(
        occurrences(&state, session_id, "reconnect-delta-line"),
        1,
        "a contiguous delta must be applied exactly once"
    );
}

#[tokio::test]
async fn resident_reattach_without_new_sequences_leaves_the_screen_untouched() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-no-new-sequences";

    let (_first_tx, first) = attachment(session_id, b"already-applied-line\r\n", Some(1), Some(4));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // The ring still ends at sequence 4: the resident grid has already applied every byte offered.
    let (_second_tx, second) = attachment(session_id, b"already-applied-line\r\n", Some(1), Some(4));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach with a fully-applied history");

    assert_eq!(
        occurrences(&state, session_id, "already-applied-line"),
        1,
        "re-offering applied sequences must not duplicate them"
    );
}

#[tokio::test]
async fn resident_reattach_with_overlapping_history_applies_each_sequence_once() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-overlap";

    let (_first_tx, first) = attachment(session_id, b"overlap-marker-line\r\n", Some(1), Some(5));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // 3..=7 straddles the applied boundary and the blob carries no per-sequence offsets, so the
    // only duplication-free reading is a rebuild from the supplied history.
    let (_second_tx, second) = attachment(
        session_id,
        b"overlap-marker-line\r\nfresh-tail-line\r\n",
        Some(3),
        Some(7),
    );
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach with an overlapping history");

    assert_eq!(
        occurrences(&state, session_id, "overlap-marker-line"),
        1,
        "an overlapping replay must not double-apply shared sequences"
    );
    assert_eq!(
        occurrences(&state, session_id, "fresh-tail-line"),
        1,
        "the unapplied tail of an overlapping history must still land"
    );
}

#[tokio::test]
async fn lagged_recovery_with_contiguous_delta_keeps_resident_history() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-lagged-contiguous";

    let (tx, initial) = attachment(session_id, b"pre-lag-resident-line\r\n", Some(1), Some(3));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
        .expect("initial resident attach");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");

    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Owned(session_id.to_string()),
        sequence: 4,
        data: Cow::Owned(b"streamed-line\r\n".to_vec()),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("pump accepts streamed output");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("streamed chunk signal arrives")
        .expect("pump publishes the streamed chunk");

    // available_from 5 continues last-applied 4: the subscriber lagged but missed no bytes.
    tx.send(DaemonStreamMessage::Lagged {
        session_id: Cow::Owned(session_id.to_string()),
        requested_after_sequence: 4,
        available_from_sequence: 5,
        start_sequence: Some(5),
        end_sequence: Some(6),
        history: Bytes::from_static(b"post-lag-delta-line\r\n"),
        segments: Vec::new(),
        replay_is_delta: Some(true),
    })
    .await
    .expect("pump accepts the lag recovery frame");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("lag recovery signal arrives")
        .expect("pump publishes the lag recovery");

    assert_eq!(
        occurrences(&state, session_id, "pre-lag-resident-line"),
        1,
        "a contiguous lag recovery must keep resident scrollback"
    );
    assert_eq!(
        occurrences(&state, session_id, "streamed-line"),
        1,
        "a contiguous lag recovery must keep streamed output"
    );
    assert_eq!(
        occurrences(&state, session_id, "post-lag-delta-line"),
        1,
        "the lag recovery delta must be applied exactly once"
    );
}

#[tokio::test]
async fn lagged_recovery_with_genuine_gap_rebuilds_from_supplied_history() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-lagged-gap";

    let (tx, initial) = attachment(session_id, b"unreachable-stale-line\r\n", Some(1), Some(2));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
        .expect("initial resident attach");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");

    // 3..=99 aged out of the ring: the resident grid is provably missing bytes nobody can replay.
    tx.send(DaemonStreamMessage::Lagged {
        session_id: Cow::Owned(session_id.to_string()),
        requested_after_sequence: 2,
        available_from_sequence: 100,
        start_sequence: Some(100),
        end_sequence: Some(120),
        history: Bytes::from_static(b"authoritative-recovery-line\r\n"),
        segments: Vec::new(),
        replay_is_delta: Some(false),
    })
    .await
    .expect("pump accepts the lag recovery frame");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("lag recovery signal arrives")
        .expect("pump publishes the lag recovery");

    assert_eq!(
        occurrences(&state, session_id, "authoritative-recovery-line"),
        1,
        "a genuine gap must apply the recovery history"
    );
    assert_eq!(
        occurrences(&state, session_id, "unreachable-stale-line"),
        0,
        "a genuine gap must not keep state the daemon can no longer vouch for"
    );
}

#[tokio::test]
async fn contiguous_delta_preserves_a_scrolled_back_viewport() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-viewport";

    let mut history = Vec::new();
    for line in 0..200 {
        history.extend_from_slice(format!("history-line-{line}\r\n").as_bytes());
    }
    let (_first_tx, first) = attachment(session_id, &history, Some(1), Some(5));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    state
        .with_session_terminal(session_id, |terminal| {
            terminal.scroll_viewport(ScrollViewport::Top)
        })
        .expect("park the resident viewport in scrollback");
    let parked = state
        .with_session_terminal(session_id, |terminal| terminal.scrollbar())
        .expect("read the parked scrollbar");
    assert!(
        parked.offset < parked.total.saturating_sub(parked.len),
        "test setup must park the viewport away from the bottom"
    );

    let (_second_tx, second) = attachment(session_id, b"delta-after-scroll\r\n", Some(6), Some(7));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach with contiguous delta");

    let after = state
        .with_session_terminal(session_id, |terminal| terminal.scrollbar())
        .expect("read the scrollbar after the delta");
    assert!(
        after.offset < after.total.saturating_sub(after.len),
        "a contiguous delta must not yank a scrolled-back viewport to the bottom: {after:?}"
    );
}

#[tokio::test]
async fn legacy_lagged_without_delta_marker_rebuilds_conservatively() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-legacy-marker";

    let (tx, initial) = attachment(session_id, b"legacy-resident-line\r\n", Some(1), Some(3));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
        .expect("initial resident attach");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");

    // An older daemon omits the field entirely. Its sequence numbers look contiguous, but nothing
    // on the wire proves it, so the only safe reading is the pre-existing rebuild.
    tx.send(DaemonStreamMessage::Lagged {
        session_id: Cow::Owned(session_id.to_string()),
        requested_after_sequence: 3,
        available_from_sequence: 4,
        start_sequence: Some(4),
        end_sequence: Some(5),
        history: Bytes::from_static(b"legacy-recovery-line\r\n"),
        segments: Vec::new(),
        replay_is_delta: None,
    })
    .await
    .expect("pump accepts the legacy lag frame");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("legacy lag signal arrives")
        .expect("pump publishes the legacy lag recovery");

    assert_eq!(
        occurrences(&state, session_id, "legacy-recovery-line"),
        1,
        "a legacy replay must still apply its history"
    );
    assert_eq!(
        occurrences(&state, session_id, "legacy-resident-line"),
        0,
        "an unproven replay must rebuild rather than assume contiguity"
    );
}

#[tokio::test]
async fn resize_allocated_sequence_hole_is_not_mistaken_for_a_gap() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-resize-hole";

    let (tx, initial) = attachment(session_id, b"pre-resize-line\r\n", Some(1), Some(4));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
        .expect("initial resident attach");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");

    // record_resize allocates sequence 5 without publishing a chunk, so the next real output
    // starts at 6. The hole is structural, not data loss, and the producer says so.
    tx.send(DaemonStreamMessage::Lagged {
        session_id: Cow::Owned(session_id.to_string()),
        requested_after_sequence: 4,
        available_from_sequence: 6,
        start_sequence: Some(6),
        end_sequence: Some(7),
        history: Bytes::from_static(b"post-resize-line\r\n"),
        segments: Vec::new(),
        replay_is_delta: Some(true),
    })
    .await
    .expect("pump accepts the lag frame spanning a resize hole");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("resize-hole lag signal arrives")
        .expect("pump publishes the resize-hole recovery");

    assert_eq!(
        occurrences(&state, session_id, "pre-resize-line"),
        1,
        "a resize-allocated sequence hole must not trigger a destructive rebuild"
    );
    assert_eq!(
        occurrences(&state, session_id, "post-resize-line"),
        1,
        "output after a resize hole must be applied exactly once"
    );
}

#[tokio::test]
async fn attach_from_a_new_daemon_epoch_rebuilds_even_when_sequences_look_lower() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-epoch";

    let (_first_tx, first) =
        attachment_at_epoch(session_id, b"old-epoch-line\r\n", Some(1), Some(900), 1);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // A restarted daemon numbers from scratch, so sequence 1 under epoch 2 is not covered by the
    // 900 the grid applied under epoch 1; the two counters are unrelated.
    let (_second_tx, second) =
        attachment_at_epoch(session_id, b"new-epoch-line\r\n", Some(1), Some(2), 2);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach under a new daemon epoch");

    assert_eq!(
        occurrences(&state, session_id, "new-epoch-line"),
        1,
        "a new epoch's history must be applied"
    );
    assert_eq!(
        occurrences(&state, session_id, "old-epoch-line"),
        0,
        "sequences from a previous epoch must not suppress the new epoch's replay"
    );
    assert_eq!(
        state.session_last_sequence(session_id),
        Some(2),
        "the cursor must adopt the new epoch's sequence, not keep the higher stale one"
    );
}

#[tokio::test]
async fn stale_duplicate_end_sequence_never_rewinds_the_replay_cursor() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-stale-end";

    let (_first_tx, first) = attachment(session_id, b"cursor-origin-line\r\n", Some(1), Some(10));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // A snapshot that raced the pump reports an older end. Replaying it would duplicate applied
    // bytes, and adopting its cursor would duplicate them again on the following attach.
    let (_second_tx, second) = attachment(session_id, b"cursor-origin-line\r\n", Some(1), Some(6));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach with a stale end sequence");

    assert_eq!(
        occurrences(&state, session_id, "cursor-origin-line"),
        1,
        "a stale replay must not duplicate already-applied output"
    );
    assert_eq!(
        state.session_last_sequence(session_id),
        Some(10),
        "a stale end sequence must never rewind the replay cursor"
    );
}

#[tokio::test]
async fn output_at_or_below_the_watermark_is_discarded_instead_of_replayed() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-watermark";

    let (tx, initial) = attachment(session_id, b"watermark-line\r\n", Some(1), Some(8));
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
        .expect("initial resident attach");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");

    // Sequence 5 is already covered by the replay that ended at 8. Feeding it would duplicate
    // output and adopting it would rewind the cursor, re-opening the same window on the next
    // attach. Sequence 9 is genuinely new and is the barrier proving 5 was processed and dropped.
    for (sequence, payload) in [
        (5u64, b"stale-duplicate-line\r\n".as_slice()),
        (9, b"fresh-after-watermark\r\n".as_slice()),
    ] {
        tx.send(DaemonStreamMessage::Output {
            session_id: Cow::Owned(session_id.to_string()),
            sequence,
            data: Cow::Owned(payload.to_vec()),
            metrics_read_unix_micros: None,
        })
        .await
        .expect("pump accepts output");
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            updates.changed().await.expect("pump alive");
            if state.session_last_sequence(session_id) == Some(9) {
                break;
            }
        }
    })
    .await
    .expect("the post-watermark chunk is processed");

    assert_eq!(
        occurrences(&state, session_id, "stale-duplicate-line"),
        0,
        "output at or below the watermark must be discarded, not fed"
    );
    assert_eq!(
        occurrences(&state, session_id, "fresh-after-watermark"),
        1,
        "output past the watermark must still be applied"
    );
    assert_eq!(
        state.session_last_sequence(session_id),
        Some(9),
        "a discarded chunk must not rewind the cursor"
    );
}

#[tokio::test]
async fn an_overlapping_replay_preserves_the_older_marker_semantics() {
    // `resident-only-marker` appears ONLY in the resident history, never in the replay payload, so
    // its disappearance is positive evidence that the overlapping replay rebuilt the grid rather
    // than being fed on top of it. `shared-overlap-line` appears in both, so its count of exactly
    // one is what rules out double-application. Asserting only the shared line would be satisfied
    // by either outcome and would prove nothing.
    for marker in [None, Some(false), Some(true)] {
        let state = NativeTerminalSurfaceHostState::default();
        let session_id = "native-recovery-overlap-marker";

        let (tx, initial) = attachment(
            session_id,
            b"resident-only-marker\r\nshared-overlap-line\r\n",
            Some(1),
            Some(5),
        );
        state
            .attach_daemon_attachment::<tauri::Wry>(session_id, initial, None)
            .expect("initial resident attach");
        assert_eq!(
            occurrences(&state, session_id, "resident-only-marker"),
            1,
            "marker {marker:?}: setup must place the resident-only marker on the grid"
        );

        let mut updates = state
            .subscribe_session_update(session_id)
            .expect("subscribe before driving the pump");

        tx.send(DaemonStreamMessage::Lagged {
            session_id: Cow::Owned(session_id.to_string()),
            requested_after_sequence: 2,
            available_from_sequence: 3,
            start_sequence: Some(3),
            end_sequence: Some(7),
            history: Bytes::from_static(b"shared-overlap-line\r\noverlap-tail-line\r\n"),
            segments: Vec::new(),
            replay_is_delta: marker,
        })
        .await
        .expect("pump accepts the overlapping recovery");
        tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
            .await
            .expect("overlap recovery signal arrives")
            .expect("pump publishes the overlap recovery");

        assert_eq!(
            occurrences(&state, session_id, "resident-only-marker"),
            0,
            "marker {marker:?}: a straddling replay must rebuild, discarding resident-only content"
        );
        assert_eq!(
            occurrences(&state, session_id, "shared-overlap-line"),
            1,
            "marker {marker:?}: an overlapping replay must not double-apply shared sequences"
        );
        assert_eq!(
            occurrences(&state, session_id, "overlap-tail-line"),
            1,
            "marker {marker:?}: the unapplied tail must land exactly once"
        );
        assert_eq!(
            state.session_last_sequence(session_id),
            Some(7),
            "marker {marker:?}: the cursor must advance to the replay's end"
        );
    }
}

#[tokio::test]
async fn a_new_epoch_without_an_end_sequence_clears_the_stale_cursor() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "native-recovery-empty-epoch";

    let (_first_tx, first) =
        attachment_at_epoch(session_id, b"old-epoch-body\r\n", Some(1), Some(900), 1);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, first, None)
        .expect("initial resident attach");

    // A restarted daemon with an empty ring reports no sequences at all. Preserving 900 from the
    // retired epoch would fence out the new epoch's first 900 chunks as "already applied".
    let (tx, second) = attachment_at_epoch(session_id, b"", None, None, 2);
    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, second, None)
        .expect("reattach under an empty new epoch");

    assert_eq!(
        state.session_last_sequence(session_id),
        None,
        "a new epoch with no sequences must clear the stale cursor"
    );

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before driving the pump");
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Owned(session_id.to_string()),
        sequence: 1,
        data: Cow::Owned(b"new-epoch-first-output\r\n".to_vec()),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("pump accepts the new epoch's first chunk");
    tokio::time::timeout(std::time::Duration::from_secs(5), updates.changed())
        .await
        .expect("new epoch output signal arrives")
        .expect("pump publishes the new epoch output");

    assert_eq!(
        occurrences(&state, session_id, "new-epoch-first-output"),
        1,
        "the new epoch's low sequences must not be discarded as already applied"
    );
}
