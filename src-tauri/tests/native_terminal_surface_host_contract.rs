use ferryx_lib::daemon::{DaemonAttachment, DaemonStreamMessage};
use ferryx_lib::native_terminal::composition::{
    CellMetrics, CompositorTargetKind, LogicalBounds, PhysicalBounds, PlatformCompositorDescriptor,
};
use ferryx_lib::native_terminal::surface_host::{
    snapshot_for_layout, NativeTerminalBellPayload, NativeTerminalBoundsRequest,
    NativeTerminalEvent, NativeTerminalSurfaceHostState, NativeTerminalTitlePayload,
    RenderScheduleCoordinator,
};
use ferryx_lib::native_terminal::{NativeTerminalError, NativeTerminalInput};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

#[test]
fn native_surface_request_preserves_session_and_nonzero_viewport() {
    let request = NativeTerminalBoundsRequest {
        session_id: "term-main".into(),
        bounds: LogicalBounds {
            x: 48.0,
            y: 96.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 2.0,
        },
    };

    let layout = request
        .layout(CellMetrics {
            width_px: 10,
            height_px: 20,
        })
        .expect("a valid surface request must produce a physical viewport");

    assert_eq!(request.session_id, "term-main");
    assert_eq!(
        layout.physical_bounds,
        PhysicalBounds {
            x: 96,
            y: 192,
            width: 1600,
            height: 960,
        }
    );
    assert_eq!((layout.cols, layout.rows), (160, 48));

    let (snapshot, _) = snapshot_for_layout(layout);
    assert_eq!((snapshot.cols, snapshot.rows), (160, 48));
    assert_eq!(snapshot.grid.len(), 48);
    assert!(snapshot.grid.iter().all(|row| row.len() == 160));
}

#[test]
fn native_surface_request_rejects_missing_session_id() {
    let request = NativeTerminalBoundsRequest {
        session_id: "   ".into(),
        bounds: LogicalBounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 480.0,
            scale_factor: 1.0,
        },
    };

    assert!(matches!(
        request.layout(CellMetrics {
            width_px: 10,
            height_px: 20,
        }),
        Err(NativeTerminalError::InvalidValue(_))
    ));
}

#[tokio::test]
async fn native_session_renders_bytes_from_daemon_attachment_history_and_live_stream() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-daemon-bridge-contract";

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let stream_task = tokio::spawn(async {});

    let attachment = DaemonAttachment {
        session_id: session_id.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history: b"prompt> daemon replay history\r\n".to_vec(),
        messages: rx,
        stream_task,
    };

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach daemon session to native surface host state");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("session update receiver");

    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 2,
        data: Cow::Borrowed(b"live daemon stream output\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send live message to stream");

    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("output processed signal within bounded timeout")
        .expect("native terminal update sender remains connected");

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query")
        .expect("session snapshot must exist for attached session");

    let text: String = snapshot
        .grid
        .iter()
        .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
        .collect();

    assert!(
        text.contains("daemon replay history"),
        "expected snapshot grid to contain replay history, got: {text}"
    );
    assert!(
        text.contains("live daemon stream output"),
        "expected snapshot grid to contain live stream output, got: {text}"
    );
}

#[tokio::test]
async fn native_session_handles_replay_gaps_and_session_cleanup_deterministically() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-gap-and-cleanup-test";

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let stream_task = tokio::spawn(std::future::pending());

    let attachment = DaemonAttachment {
        session_id: session_id.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history: b"initial output before gap\r\n".to_vec(),
        messages: rx,
        stream_task,
    };

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("attach daemon session");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("session update receiver");

    // Send a replay gap message followed by fresh output
    tx.send(DaemonStreamMessage::Lagged {
        session_id: Cow::Borrowed(session_id),
        requested_after_sequence: 1,
        available_from_sequence: 5,
        start_sequence: Some(5),
        end_sequence: Some(5),
        history: Cow::Borrowed(b"recovered fresh stream\r\n"),
    })
    .await
    .expect("send lagged recovery message");

    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("gap processed signal within bounded timeout")
        .expect("native terminal update sender remains connected");

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query")
        .expect("snapshot exists");

    let text: String = snapshot
        .grid
        .iter()
        .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
        .collect();

    assert!(
        text.contains("recovered fresh stream"),
        "expected snapshot to contain recovered stream output after gap, got: {text}"
    );

    // Test input encoding through the session
    let encoded = state
        .encode_input(
            session_id,
            &NativeTerminalInput::Text {
                text: "ls -la\n".into(),
            },
        )
        .expect("encode text input");
    assert_eq!(encoded, b"ls -la\n");

    // Detach and verify session cleanup
    state.detach_session(session_id);
    let detached_snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query after detach");
    assert!(detached_snapshot.is_none());
}

#[test]
fn native_terminal_host_composition_target_must_be_platform_child_view_not_root_window() {
    let descriptor = PlatformCompositorDescriptor::active_for_platform();

    #[cfg(target_os = "macos")]
    {
        // Regression: When host targets the root WebviewWindow, WKWebView occludes the Metal layer.
        // The active platform target must be a dedicated layer-backed pointer-transparent child view.
        assert_eq!(
            descriptor.target_kind,
            CompositorTargetKind::NativeChildView,
            "Native terminal host target must be a dedicated platform child compositor view above WKWebView, not the occluded root WebviewWindow"
        );
        assert!(
            descriptor.pointer_transparent,
            "Native terminal child view must be pointer-transparent for web focus/input routing"
        );
        assert!(
            descriptor.layer_backed,
            "Native terminal child view must be layer-backed"
        );
        assert!(
            descriptor.validate_desktop_composition().is_ok(),
            "Active platform compositor descriptor must pass desktop composition validation"
        );
    }

    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(
            descriptor.target_kind,
            CompositorTargetKind::UnsupportedFallback,
            "Non-macOS platforms must fall back explicitly until platform compositor targets are implemented"
        );
        assert!(
            descriptor.validate_desktop_composition().is_err(),
            "Non-macOS fallback targets must reject desktop native composition"
        );
    }
}

#[tokio::test]
async fn native_terminal_host_retains_independent_geometry_and_live_content_for_split_sessions() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_a = "term-split-layout-a";
    let session_b = "term-split-layout-b";
    let (tx_a, rx_a) = tokio::sync::mpsc::channel(16);
    let (tx_b, rx_b) = tokio::sync::mpsc::channel(16);

    for (session_id, rx) in [(session_a, rx_a), (session_b, rx_b)] {
        state
            .attach_daemon_attachment::<tauri::Wry>(
                session_id,
                DaemonAttachment {
                    session_id: session_id.to_string(),
                    epoch: 1,
                    start_sequence: None,
                    end_sequence: None,
                    gap: None,
                    history: Vec::new(),
                    messages: rx,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
            )
            .expect("attach split-pane session");
    }

    let metrics = CellMetrics {
        width_px: 10,
        height_px: 20,
    };
    let bounds_a = LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 480.0,
        scale_factor: 1.0,
    };
    let bounds_b = LogicalBounds {
        x: 800.0,
        y: 0.0,
        width: 400.0,
        height: 360.0,
        scale_factor: 1.0,
    };
    let layout_a = state
        .prepare_session_layout(
            NativeTerminalBoundsRequest {
                session_id: session_a.to_string(),
                bounds: bounds_a,
            },
            metrics,
        )
        .expect("prepare session A layout");
    let layout_b = state
        .prepare_session_layout(
            NativeTerminalBoundsRequest {
                session_id: session_b.to_string(),
                bounds: bounds_b,
            },
            metrics,
        )
        .expect("prepare session B layout");

    assert_eq!(state.session_layout(session_a), Some(layout_a));
    assert_eq!(state.session_layout(session_b), Some(layout_b));
    assert_eq!(state.session_logical_bounds(session_a), Some(bounds_a));
    assert_eq!(state.session_logical_bounds(session_b), Some(bounds_b));
    assert_ne!(layout_a, layout_b);

    let mut updates_a = state.subscribe_session_update(session_a).unwrap();
    let mut updates_b = state.subscribe_session_update(session_b).unwrap();
    tx_a.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_a),
        sequence: 1,
        data: Cow::Borrowed(b"content unique to pane alpha\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .unwrap();
    tx_b.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_b),
        sequence: 1,
        data: Cow::Borrowed(b"content unique to pane beta\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .unwrap();

    for updates in [&mut updates_a, &mut updates_b] {
        tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
            .await
            .expect("split-pane output processed within bounded timeout")
            .expect("session update sender remains connected");
    }

    let snapshot_text = |session_id: &str| {
        state
            .snapshot_for_session(session_id)
            .unwrap()
            .unwrap()
            .grid
            .iter()
            .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
            .collect::<String>()
    };
    assert!(snapshot_text(session_a).contains("content unique to pane alpha"));
    assert!(snapshot_text(session_b).contains("content unique to pane beta"));
    assert_eq!(state.session_layout(session_a), Some(layout_a));
    assert_eq!(state.session_layout(session_b), Some(layout_b));
}

#[tokio::test]
async fn native_terminal_daemon_pump_pushes_title_and_bell_and_detach_stops_events() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-native-events";
    let (title_tx, mut title_rx) = tokio::sync::watch::channel(None::<NativeTerminalTitlePayload>);
    let (bell_tx, mut bell_rx) = tokio::sync::watch::channel(None::<NativeTerminalBellPayload>);
    let observed_events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = Arc::clone(&observed_events);
    state.set_event_sink(Arc::new(move |event| {
        sink_events.lock().unwrap().push(event.clone());
        match event {
            NativeTerminalEvent::Title(payload) => {
                title_tx.send_replace(Some(payload));
            }
            NativeTerminalEvent::Bell(payload) => {
                bell_tx.send_replace(Some(payload));
            }
        }
    }));

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: None,
                end_sequence: None,
                gap: None,
                history: Vec::new(),
                messages: rx,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
        )
        .expect("attach native event session");

    let title_changed = title_rx.changed();
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 1,
        data: Cow::Borrowed(b"\x1b]2;my-title\x07"),
        metrics_read_unix_micros: None,
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), title_changed)
        .await
        .expect("title event within bounded timeout")
        .expect("title event sender remains connected");
    assert_eq!(
        title_rx.borrow().clone(),
        Some(NativeTerminalTitlePayload {
            session_id: session_id.to_string(),
            title: "my-title".to_string(),
        })
    );
    let bell_changed = bell_rx.changed();
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 2,
        data: Cow::Borrowed(b"\x07"),
        metrics_read_unix_micros: None,
    })
    .await
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), bell_changed)
        .await
        .expect("bell event within bounded timeout")
        .expect("bell event sender remains connected");
    assert_eq!(
        bell_rx.borrow().clone(),
        Some(NativeTerminalBellPayload {
            session_id: session_id.to_string(),
            count: 1,
        })
    );
    assert_eq!(
        observed_events.lock().unwrap().as_slice(),
        &[
            NativeTerminalEvent::Title(NativeTerminalTitlePayload {
                session_id: session_id.to_string(),
                title: "my-title".to_string(),
            }),
            NativeTerminalEvent::Bell(NativeTerminalBellPayload {
                session_id: session_id.to_string(),
                count: 1,
            }),
        ],
        "title and explicit bell input must each surface exactly once"
    );

    let detached_title_changed = title_rx.changed();
    let detached_bell_changed = bell_rx.changed();
    state.detach_session(session_id);
    let _ = tx
        .send(DaemonStreamMessage::Output {
            session_id: Cow::Borrowed(session_id),
            sequence: 3,
            data: Cow::Borrowed(b"\x1b]2;detached-title\x07\x07"),
            metrics_read_unix_micros: None,
        })
        .await;
    let detached_event = async {
        tokio::select! {
            result = detached_title_changed => ("title", result),
            result = detached_bell_changed => ("bell", result),
        }
    };
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), detached_event)
            .await
            .is_err(),
        "detached session must not emit title or bell events"
    );
    assert_eq!(observed_events.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn native_terminal_active_session_detach_resets_host_layout_and_preserves_isolation() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_a = "term-pane-a";
    let session_b = "term-pane-b";

    let (_tx_a, rx_a) = tokio::sync::mpsc::channel(16);
    let (_tx_b, rx_b) = tokio::sync::mpsc::channel(16);

    let attachment_a = DaemonAttachment {
        session_id: session_a.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history: b"prompt-a> ".to_vec(),
        messages: rx_a,
        stream_task: tokio::spawn(async {}),
    };
    let attachment_b = DaemonAttachment {
        session_id: session_b.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history: b"prompt-b> ".to_vec(),
        messages: rx_b,
        stream_task: tokio::spawn(async {}),
    };

    state
        .attach_daemon_attachment::<tauri::Wry>(session_a, attachment_a, None)
        .expect("attach session A");
    state
        .attach_daemon_attachment::<tauri::Wry>(session_b, attachment_b, None)
        .expect("attach session B");

    // Both sessions exist and are queryable
    assert!(state.snapshot_for_session(session_a).unwrap().is_some());
    assert!(state.snapshot_for_session(session_b).unwrap().is_some());

    // Detaching session A leaves session B intact
    state.detach_session(session_a);
    assert!(state.snapshot_for_session(session_a).unwrap().is_none());
    assert!(state.snapshot_for_session(session_b).unwrap().is_some());

    // Detaching session B cleanly empties sessions
    state.detach_session(session_b);
    assert!(state.snapshot_for_session(session_b).unwrap().is_none());
}

#[test]
fn native_terminal_render_coordinator_coalesces_rapid_bursts_and_rearms_after_consumption() {
    let coordinator = RenderScheduleCoordinator::new();

    assert!(
        !coordinator.is_render_pending(),
        "newly initialized coordinator must not have a pending render"
    );
    assert!(
        !coordinator.consume_render(),
        "consuming an idle coordinator must return false"
    );

    // Initial render request should succeed and mark pending
    assert!(
        coordinator.schedule_render(),
        "initial render request must successfully transition from idle to scheduled"
    );
    assert!(
        coordinator.is_render_pending(),
        "coordinator must reflect a pending render"
    );

    // Rapid burst of subsequent output events must all coalesce (return false)
    for _ in 0..50 {
        assert!(
            !coordinator.schedule_render(),
            "burst render request while render is pending must coalesce without scheduling additional work"
        );
    }
    assert!(
        coordinator.is_render_pending(),
        "coordinator must remain in pending state during burst"
    );

    // Consuming pending work resets state
    assert!(
        coordinator.consume_render(),
        "consuming pending render must return true"
    );
    assert!(
        !coordinator.is_render_pending(),
        "coordinator must return to idle state after consuming pending render"
    );

    // Second render becomes schedulable after pending work is consumed
    assert!(
        coordinator.schedule_render(),
        "a second render must become schedulable after pending render is consumed"
    );
    assert!(
        coordinator.is_render_pending(),
        "coordinator must be pending again after second schedule"
    );
    assert!(
        coordinator.consume_render(),
        "final consumption must succeed"
    );
}

#[tokio::test]
async fn native_terminal_surface_host_state_coalescing_seam_tracks_session_lifecycle() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-coalesce-lifecycle";

    let (_tx, rx) = tokio::sync::mpsc::channel(16);
    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: None,
                end_sequence: None,
                gap: None,
                history: Vec::new(),
                messages: rx,
                stream_task: tokio::spawn(async {}),
            },
            None,
        )
        .expect("attach session");

    let coordinator = state
        .session_render_coordinator(session_id)
        .expect("session coordinator must exist");

    assert!(!coordinator.is_render_pending());
    assert!(state.schedule_session_render(session_id));
    assert!(state.is_session_render_pending(session_id));

    // Rapid burst via state API is coalesced
    for _ in 0..20 {
        assert!(!state.schedule_session_render(session_id));
    }
    assert!(state.is_session_render_pending(session_id));

    // Consumed -> re-arm
    assert!(state.consume_session_render(session_id));
    assert!(!state.is_session_render_pending(session_id));
    assert!(state.schedule_session_render(session_id));
    assert!(state.consume_session_render(session_id));

    // Detach clears session
    state.detach_session(session_id);
    assert!(state.session_render_coordinator(session_id).is_none());
    assert!(!state.schedule_session_render(session_id));
    assert!(!state.is_session_render_pending(session_id));
}
