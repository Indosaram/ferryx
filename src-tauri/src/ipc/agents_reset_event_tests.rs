#![cfg(unix)]

use crate::daemon::client::{DaemonAttachment, DaemonClient};
use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonStreamMessage, DAEMON_PROTOCOL_VERSION,
};
use crate::ipc::{cmd_agent_state_reset, run_blocking, IpcErrorCode};
use crate::native_terminal::surface_host::{
    NativeTerminalSurfaceHostState, NATIVE_TERMINAL_AGENT_STATE_EVENT,
};
use std::{sync::Arc, time::Duration};
use tauri::{Listener, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn reset_scenario(succeeds: bool) {
    // Given: a local wire fixture and a real native session reporting Working.
    let directory = run_blocking(|| {
        Ok(tempfile::Builder::new()
            .prefix("reset-")
            .tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
            .expect("local fixture"))
    })
    .await
    .expect("fixture worker");
    let socket = directory.path().join("d.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("local socket");
    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let app = tauri::test::mock_builder()
        .manage(client)
        .manage(NativeTerminalSurfaceHostState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let host = app.state::<NativeTerminalSurfaceHostState>();
    let (events_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    app.listen(NATIVE_TERMINAL_AGENT_STATE_EVENT, move |event| {
        events_tx
            .send(
                serde_json::from_str::<serde_json::Value>(event.payload())
                    .expect("native event JSON"),
            )
            .expect("observer alive");
    });
    let (messages_tx, messages) = tokio::sync::mpsc::channel(4);
    host.attach_daemon_attachment(
        "reset-session",
        DaemonAttachment {
            session_id: "reset-session".into(),
            epoch: 1,
            start_sequence: None,
            end_sequence: None,
            gap: None,
            history: Vec::new(),
            history_segments: Vec::new(),
            pty_cols: None,
            pty_rows: None,
            remote_generation: None,
            messages,
            stream_task: tokio::spawn(std::future::pending()),
        },
        Some(app.handle().clone()),
    )
    .expect("native attachment");
    let report = |state: &'static str, is_snapshot| DaemonStreamMessage::AgentState {
        session_id: "reset-session".into(),
        state: state.into(),
        agent: Some("omo".into()),
        provider_session: None,
        is_snapshot,
        origin: crate::daemon::protocol::AgentStateOrigin::Agent,
    };
    messages_tx
        .send(report("working", false))
        .await
        .expect("seed working");
    let initial = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("seed event deadline")
        .expect("seed event");
    assert_eq!(initial["state"], "working");

    let (requested_tx, requested) = tokio::sync::oneshot::channel();
    let (reply_tx, reply) = tokio::sync::oneshot::channel();
    let server = async move {
        let (stream, _) = listener.accept().await.expect("accept fixture client");
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();
        let handshake: DaemonRequest = serde_json::from_str(
            &lines
                .next_line()
                .await
                .expect("read handshake")
                .expect("handshake"),
        )
        .expect("parse handshake");
        assert!(matches!(
            handshake,
            DaemonRequest::Handshake {
                version: DAEMON_PROTOCOL_VERSION
            }
        ));
        let handshake = DaemonResponse::HandshakeOk {
            version: DAEMON_PROTOCOL_VERSION,
            pid: 0,
            epoch: 1,
            binary_path: None,
            binary_mtime_ms: None,
            daemon_version: Some(env!("CARGO_PKG_VERSION").into()),
        };
        writer
            .write_all(
                format!(
                    "{}\n",
                    serde_json::to_string(&handshake).expect("encode handshake")
                )
                .as_bytes(),
            )
            .await
            .expect("send handshake");
        let request: DaemonRequest =
            serde_json::from_str(&lines.next_line().await.expect("read reset").expect("reset"))
                .expect("parse reset");
        assert!(
            matches!(request, DaemonRequest::ResetAgentState { session_id } if session_id == "reset-session")
        );
        requested_tx.send(()).expect("request observer alive");
        reply.await.expect("controlled reply release");
        let response = if succeeds {
            DaemonResponse::ResetAgentStateOk
        } else {
            DaemonResponse::Error {
                message: "fixture rejection".into(),
            }
        };
        writer
            .write_all(
                format!(
                    "{}\n",
                    serde_json::to_string(&response).expect("encode response")
                )
                .as_bytes(),
            )
            .await
            .expect("send reset response");
    };

    // When: invoke the actual command, withholding the daemon response until observed.
    let command = cmd_agent_state_reset(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        "reset-session".into(),
    );
    let observer = async {
        requested.await.expect("reset request observed");
        let premature = events.try_recv().ok();
        reply_tx.send(()).expect("release daemon response");
        premature
    };
    let (result, (), premature) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(command, server, observer)
    })
    .await
    .expect("command deadline");
    let emitted_before_reply = premature.is_some();
    let mut observed = premature.into_iter().collect::<Vec<_>>();
    while let Ok(event) = events.try_recv() {
        observed.push(event);
    }
    let reset_events = observed.clone();

    // Then: replaying Working exposes retained-state changes through real deduplication.
    // The snapshot is a guaranteed event barrier after that replay, not a timed wait.
    messages_tx
        .send(report("working", false))
        .await
        .expect("replay prior activity");
    messages_tx
        .send(report("blocked", true))
        .await
        .expect("snapshot barrier");
    let replay = tokio::time::timeout(Duration::from_secs(5), async {
        let mut replay = Vec::new();
        loop {
            let event = events.recv().await.expect("native observer alive");
            let barrier = event["isSnapshot"] == true;
            replay.push(event);
            if barrier {
                break;
            }
        }
        replay
    })
    .await
    .expect("snapshot barrier deadline");
    host.teardown();
    drop(messages_tx);
    run_blocking(move || {
        directory.close().expect("remove local fixture");
        Ok(())
    })
    .await
    .expect("cleanup worker");

    if succeeds {
        result.expect("successful reset");
        assert_eq!(
            reset_events.len(),
            1,
            "success emits exactly one native reset"
        );
        assert_eq!(reset_events[0]["state"], "idle");
        assert_eq!(reset_events[0]["ruleId"], "manual-reset");
        assert_eq!(
            replay.len(),
            2,
            "success retained Idle so Working transitions again"
        );
        assert_eq!(replay[0]["state"], "working");
    } else {
        assert_eq!(
            result.expect_err("daemon rejection").code,
            IpcErrorCode::InternalError
        );
        assert!(
            reset_events.is_empty(),
            "rejection must emit no native reset event: {reset_events:?}"
        );
        assert_eq!(
            replay.len(),
            1,
            "rejection must retain Working and suppress its duplicate"
        );
    }
    assert!(
        !emitted_before_reply,
        "native reset must await daemon success"
    );
}

#[tokio::test]
async fn rejection_preserves_native_activity_and_emits_no_idle() {
    reset_scenario(false).await;
}

#[tokio::test]
async fn success_resets_native_activity_and_emits_idle() {
    reset_scenario(true).await;
}
