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

    // Detach releases the compositor surface but keeps the session so a backgrounded agent keeps
    // streaming; only close_session discards it.
    state.detach_session(session_id);
    let detached_snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query after detach");
    assert!(
        detached_snapshot.is_some(),
        "detach must keep the session alive for background streaming"
    );

    state.close_session(session_id);
    let closed_snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query after close");
    assert!(closed_snapshot.is_none());
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
            NativeTerminalEvent::AgentState(_) => {}
            NativeTerminalEvent::Scrollbar(_) => {}
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

    // Detach only releases the compositor surface. A backgrounded agent must keep reporting title
    // and bell, otherwise its tab would freeze on the last state observed before the tab switch.
    let detached_title_changed = title_rx.changed();
    state.detach_session(session_id);
    let _ = tx
        .send(DaemonStreamMessage::Output {
            session_id: Cow::Borrowed(session_id),
            sequence: 3,
            data: Cow::Borrowed(b"\x1b]2;detached-title\x07\x07"),
            metrics_read_unix_micros: None,
        })
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(5), detached_title_changed)
        .await
        .expect("detached session must still report title changes")
        .expect("title event sender remains connected");
    assert_eq!(
        title_rx.borrow().clone(),
        Some(NativeTerminalTitlePayload {
            session_id: session_id.to_string(),
            title: "detached-title".to_string(),
        })
    );

    // The same output carried a bell; drain that notification so the post-close watchers only
    // observe events produced after the session is gone.
    tokio::time::timeout(std::time::Duration::from_secs(5), bell_rx.changed())
        .await
        .expect("detached session must still report bell changes")
        .expect("bell event sender remains connected");

    // Closing the session tears down the pump, so no further output can produce events.
    let closed_title_changed = title_rx.changed();
    let closed_bell_changed = bell_rx.changed();
    state.close_session(session_id);
    let _ = tx
        .send(DaemonStreamMessage::Output {
            session_id: Cow::Borrowed(session_id),
            sequence: 4,
            data: Cow::Borrowed(b"\x1b]2;closed-title\x07\x07"),
            metrics_read_unix_micros: None,
        })
        .await;
    let closed_event = async {
        tokio::select! {
            result = closed_title_changed => ("title", result),
            result = closed_bell_changed => ("bell", result),
        }
    };
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), closed_event)
            .await
            .is_err(),
        "closed session must not emit title or bell events"
    );
}

#[tokio::test]
async fn native_terminal_daemon_pump_emits_scrollbar_only_for_scrollback_state_changes() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-native-scrollbar-events";
    let (scrollbar_tx, mut scrollbar_rx) = tokio::sync::mpsc::unbounded_channel();
    state.set_event_sink(Arc::new(move |event| {
        if let NativeTerminalEvent::Scrollbar(payload) = event {
            scrollbar_tx
                .send(payload)
                .expect("scrollbar receiver remains live");
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
        .expect("attach native scrollbar session");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before output");
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 1,
        data: Cow::Borrowed(b"one short line\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send short output");
    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("short output processed")
        .expect("session update remains live");
    assert!(
        scrollbar_rx.try_recv().is_err(),
        "output without scrollback must not emit a scrollbar event"
    );

    let history = (0..100)
        .map(|row| format!("row-{row:03}"))
        .collect::<Vec<_>>()
        .join("\r\n");
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 2,
        data: Cow::Owned(history.into_bytes()),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send scrollback output");
    let visible = tokio::time::timeout(std::time::Duration::from_secs(2), scrollbar_rx.recv())
        .await
        .expect("scrollback event arrives")
        .expect("scrollbar sender remains live");
    assert!(visible.total > visible.len);

    tx.send(DaemonStreamMessage::Gap {
        session_id: Cow::Borrowed(session_id),
        requested_after_sequence: 2,
        available_from_sequence: 3,
    })
    .await
    .expect("send replay gap");
    let hidden = tokio::time::timeout(std::time::Duration::from_secs(2), scrollbar_rx.recv())
        .await
        .expect("reset scrollbar event arrives")
        .expect("scrollbar sender remains live");
    assert_eq!(hidden.total, hidden.len);
    assert_eq!(hidden.offset, 0);
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

    // Detaching session A releases its surface without disturbing session B.
    state.detach_session(session_a);
    assert!(
        state.ensure_surface_attached(session_a).is_err(),
        "detached session must refuse further geometry work"
    );
    assert!(state.snapshot_for_session(session_a).unwrap().is_some());
    assert!(state.ensure_surface_attached(session_b).is_ok());
    assert!(state.snapshot_for_session(session_b).unwrap().is_some());

    // Closing discards each session entirely.
    state.close_session(session_a);
    assert!(state.snapshot_for_session(session_a).unwrap().is_none());
    state.close_session(session_b);
    assert!(state.snapshot_for_session(session_b).unwrap().is_none());
}

#[tokio::test]
async fn native_terminal_reattach_preserves_retained_output_without_replaying_history() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-retained-replay";
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let startup_block = b"unique-agent-startup-block\r\n";

    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: startup_block.to_vec(),
                messages: rx,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
        )
        .expect("attach initial daemon history");

    let mut updates = state
        .subscribe_session_update(session_id)
        .expect("subscribe before background output");
    state.detach_session(session_id);
    tx.send(DaemonStreamMessage::Output {
        session_id: Cow::Borrowed(session_id),
        sequence: 2,
        data: Cow::Borrowed(b"background-output\r\n"),
        metrics_read_unix_micros: None,
    })
    .await
    .expect("send output while pane is detached");
    tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed())
        .await
        .expect("background output processed")
        .expect("retained session update channel remains live");

    assert!(
        state
            .reattach_existing_session_with_bounds(
                session_id,
                Some(LogicalBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 480.0,
                    scale_factor: 1.0,
                }),
            )
            .expect("reattach retained session without daemon replay"),
        "existing background session must be resumed locally"
    );

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot lookup after reattach")
        .expect("retained session remains available");
    let text = snapshot
        .grid
        .iter()
        .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
        .collect::<String>();
    assert_eq!(
        text.matches("unique-agent-startup-block").count(),
        1,
        "reattaching a retained stream must not append daemon history again"
    );
    assert!(text.contains("background-output"));
}

#[tokio::test]
async fn native_terminal_recovery_attach_replaces_stale_terminal_history() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-recovery-history";
    let (_old_tx, old_rx) = tokio::sync::mpsc::channel(16);

    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: b"stale-native-history\r\n".to_vec(),
                messages: old_rx,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
        )
        .expect("attach stale native terminal state");

    let (_fresh_tx, fresh_rx) = tokio::sync::mpsc::channel(16);
    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(2),
                gap: None,
                history: b"authoritative-daemon-history\r\n".to_vec(),
                messages: fresh_rx,
                stream_task: tokio::spawn(std::future::pending()),
            },
            None,
        )
        .expect("recover from a fresh daemon attachment");

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot after recovery")
        .expect("recovered session remains available");
    let text = snapshot
        .grid
        .iter()
        .flat_map(|row| row.iter().map(|cell| cell.text.as_str()))
        .collect::<String>();
    assert!(text.contains("authoritative-daemon-history"));
    assert!(
        !text.contains("stale-native-history"),
        "a full daemon recovery must replace, not append to, stale native history"
    );
}

/// Rapid tab switching lets a pane's `ResizeObserver` callback reach the backend after the pane
/// already detached. Such a late geometry update must be refused as a benign detached state rather
/// than rebuilding a compositor surface for a pane nobody can see.
#[tokio::test]
async fn native_terminal_geometry_update_after_detach_is_refused_until_reattach() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-bounds-race";
    let (_tx, rx) = tokio::sync::mpsc::channel(16);

    state
        .attach_daemon_attachment::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: b"prompt> ".to_vec(),
                messages: rx,
                stream_task: tokio::spawn(async {}),
            },
            None,
        )
        .expect("attach session");

    assert!(
        state.ensure_surface_attached(session_id).is_ok(),
        "an attached pane must accept geometry updates"
    );

    state.detach_session(session_id);
    assert_eq!(
        state.ensure_surface_attached(session_id),
        Err(NativeTerminalError::SessionDetached(session_id.to_string())),
        "a geometry update racing detach must report the detached state, not a hard failure"
    );

    // An unknown session is the same benign case: the pane is simply gone.
    assert_eq!(
        state.ensure_surface_attached("term-never-existed"),
        Err(NativeTerminalError::SessionDetached(
            "term-never-existed".to_string()
        ))
    );

    // Re-attaching the same session restores geometry handling.
    let (_tx2, rx2) = tokio::sync::mpsc::channel(16);
    state
        .attach_daemon_attachment_with_bounds::<tauri::Wry>(
            session_id,
            DaemonAttachment {
                session_id: session_id.to_string(),
                epoch: 1,
                start_sequence: Some(1),
                end_sequence: Some(1),
                gap: None,
                history: Vec::new(),
                messages: rx2,
                stream_task: tokio::spawn(async {}),
            },
            None,
            Some(LogicalBounds {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
                scale_factor: 2.0,
            }),
        )
        .expect("re-attach session");
    assert!(
        state.ensure_surface_attached(session_id).is_ok(),
        "re-attaching must restore geometry handling"
    );
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

    // Detach drains any pending render but keeps the coordinator with the surviving session.
    state.detach_session(session_id);
    assert!(state.session_render_coordinator(session_id).is_some());
    assert!(!state.is_session_render_pending(session_id));

    // Close discards the session and with it the coordinator.
    state.close_session(session_id);
    assert!(state.session_render_coordinator(session_id).is_none());
    assert!(!state.schedule_session_render(session_id));
    assert!(!state.is_session_render_pending(session_id));
}

#[tokio::test]
async fn native_terminal_attach_with_target_geometry_replays_rprompt_at_target_column_width() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-rprompt-pre-size-contract";

    let (_tx, rx) = tokio::sync::mpsc::channel(16);
    let stream_task = tokio::spawn(async {});

    // Model ZLE/RPROMPT replay with right-margin-relative cursor move:
    // "~/orca $ ", save cursor (\x1b[s), clamp to right edge (\x1b[999C),
    // move left by prompt width (\x1b[9D), write marker ("[RPROMPT]"), restore cursor (\x1b[u).
    let history = b"~/orca $ \x1b[s\x1b[999C\x1b[9D[RPROMPT]\x1b[u".to_vec();

    let attachment = DaemonAttachment {
        session_id: session_id.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history,
        messages: rx,
        stream_task,
    };

    let cell_metrics =
        ferryx_lib::native_terminal::renderer::font_manager::derived_cell_metrics_for_scale(1.0);
    let target_cols: u16 = 130;
    let target_rows: u16 = 24;
    let target_bounds = LogicalBounds {
        x: 0.0,
        y: 0.0,
        width: (target_cols as u32 * cell_metrics.width_px) as f64,
        height: (target_rows as u32 * cell_metrics.height_px) as f64,
        scale_factor: 1.0,
    };

    state
        .attach_daemon_attachment_with_bounds::<tauri::Wry>(
            session_id,
            attachment,
            None,
            Some(target_bounds),
        )
        .expect("attach daemon session with target bounds");

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query")
        .expect("session snapshot must exist");

    assert_eq!(snapshot.cols, target_cols);
    assert_eq!(snapshot.rows, target_rows);

    let row0_str: String = snapshot.grid[0]
        .iter()
        .map(|c| {
            if c.text.is_empty() {
                ' '
            } else {
                c.text.chars().next().unwrap_or(' ')
            }
        })
        .collect();

    // With 130-column pre-sizing before replay, [RPROMPT] is placed at the 130-column edge
    assert!(
        row0_str.len() == 130 && row0_str[120..130].contains("[RPROMPT]"),
        "expected [RPROMPT] at 130-column right margin (cols 120..130), got len {} row: '{row0_str}'",
        row0_str.len()
    );
    assert!(
        !row0_str[70..80].contains("[RPROMPT]"),
        "expected [RPROMPT] NOT to be stranded at 80-column boundary (cols 70..80), got: '{row0_str}'"
    );
}

#[tokio::test]
async fn native_terminal_legacy_attach_without_geometry_replays_at_default_80_columns() {
    let state = NativeTerminalSurfaceHostState::default();
    let session_id = "term-legacy-attach-contract";

    let (_tx, rx) = tokio::sync::mpsc::channel(16);
    let stream_task = tokio::spawn(async {});

    let history = b"~/orca $ \x1b[s\x1b[999C\x1b[9D[RPROMPT]\x1b[u".to_vec();

    let attachment = DaemonAttachment {
        session_id: session_id.to_string(),
        epoch: 1,
        start_sequence: Some(1),
        end_sequence: Some(1),
        gap: None,
        history,
        messages: rx,
        stream_task,
    };

    state
        .attach_daemon_attachment::<tauri::Wry>(session_id, attachment, None)
        .expect("legacy attach daemon session");

    let snapshot = state
        .snapshot_for_session(session_id)
        .expect("snapshot query")
        .expect("session snapshot must exist");

    assert_eq!(snapshot.cols, 80);
    assert_eq!(snapshot.rows, 24);

    let row0_str: String = snapshot.grid[0]
        .iter()
        .map(|c| {
            if c.text.is_empty() {
                ' '
            } else {
                c.text.chars().next().unwrap_or(' ')
            }
        })
        .collect();
    assert!(
        row0_str.len() == 80 && row0_str[70..80].contains("[RPROMPT]"),
        "legacy attach without bounds must replay at 80-column margin, got len {} row: '{row0_str}'",
        row0_str.len()
    );
}
