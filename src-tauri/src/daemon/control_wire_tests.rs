//! Wire-level tests for the control-v1 channel and its effect on the data protocol (todo 15).
//! A child of server.rs (through control_server.rs), so it drives a real DaemonServer.

use super::*;
use crate::daemon::control::{ControlFailure, ControlReply, ControlRequest};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Sends one data-protocol request over handle_client and returns the one-line response.
async fn data_exchange(server: &Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut json = serde_json::to_string(&request).unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.unwrap();
    let mut line = String::new();
    BufReader::new(read_half).read_line(&mut line).await.unwrap();
    drop(write_half);
    serving.abort();
    serde_json::from_str(line.trim()).unwrap()
}

/// Sends CommitHandover and returns None when the predecessor closed the connection without a
/// reply (the injected reply-loss faults).
async fn commit_exchange(server: &Arc<DaemonServer>) -> Option<DaemonResponse> {
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut json = serde_json::to_string(&DaemonRequest::CommitHandover { legacy_socket_path: None }).unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.unwrap();
    let mut line = String::new();
    let read = BufReader::new(read_half).read_line(&mut line).await.unwrap();
    drop(write_half);
    serving.await.unwrap();
    (read > 0).then(|| serde_json::from_str(line.trim()).unwrap())
}

fn prepared_v5_predecessor(root: &std::path::Path) -> (Arc<DaemonServer>, std::path::PathBuf) {
    let server = Arc::new(DaemonServer::new_with_paths(
        Some(root.join("config")),
        Some(root.join("auth")),
    ));
    let legacy = root.join("legacy.sock");
    server.handover_manager.force_prepared_for_test(legacy.clone());
    server.handover_manager.set_retirement_action(|| {});
    (server, legacy)
}

/// CommitHandoverOk dropped after the predecessor recorded Retired: the candidate reads the
/// decision, is the owner, and serves; the predecessor accepts nothing more.
#[tokio::test]
async fn commit_ok_dropped_after_record_resolves_to_the_candidate_as_owner() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy) = prepared_v5_predecessor(root.path());
    server
        .handover_manager
        .inject_commit_fault(Some(crate::daemon::handover::CommitFault::DropReplyAfterRecord));
    assert!(commit_exchange(&server).await.is_none(), "the reply must be lost");
    let decision = crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap();
    assert_eq!(decision, Some(crate::daemon::handover_transaction::HandoverState::Retired));
    assert_eq!(
        crate::daemon::handover::resolve_commit(false, decision),
        crate::daemon::handover::CommitResolution::Owner
    );
    assert_eq!(server.handover_manager.status(), crate::daemon::handover::HandoverStatus::Retired);
    assert!(!server.handover_manager.accept_gate_open());
}

/// CommitHandoverOk dropped before the predecessor recorded anything: no decision exists, the
/// candidate aborts, and the predecessor keeps serving.
#[tokio::test]
async fn commit_ok_dropped_before_record_makes_the_candidate_abort() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy) = prepared_v5_predecessor(root.path());
    server
        .handover_manager
        .inject_commit_fault(Some(crate::daemon::handover::CommitFault::DropReplyBeforeRecord));
    assert!(commit_exchange(&server).await.is_none(), "the reply must be lost");
    let decision = crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap();
    assert_eq!(decision, None);
    assert_eq!(
        crate::daemon::handover::resolve_commit(false, decision),
        crate::daemon::handover::CommitResolution::Abort
    );
    server.handover_manager.inject_commit_fault(None);
    assert!(matches!(data_exchange(&server, DaemonRequest::AbortHandover).await, DaemonResponse::AbortHandoverOk));
    assert_eq!(server.handover_manager.status(), crate::daemon::handover::HandoverStatus::Active);
    assert!(server.handover_manager.accept_gate_open());
    assert!(matches!(data_exchange(&server, DaemonRequest::Ping).await, DaemonResponse::Pong));
}

/// Decision write failure over the wire: the commit is answered with an error, the gate
/// reopens, the predecessor serves new requests, and the candidate's abort is accepted.
#[tokio::test]
async fn decision_write_failure_is_answered_and_the_predecessor_keeps_serving() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy) = prepared_v5_predecessor(root.path());
    server
        .handover_manager
        .inject_commit_fault(Some(crate::daemon::handover::CommitFault::DecisionWrite));
    let reply = commit_exchange(&server).await.expect("an answer");
    assert!(!matches!(reply, DaemonResponse::CommitHandoverOk), "{reply:?}");
    assert_eq!(crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap(), None);
    assert!(matches!(data_exchange(&server, DaemonRequest::Ping).await, DaemonResponse::Pong));
    assert!(matches!(data_exchange(&server, DaemonRequest::AbortHandover).await, DaemonResponse::AbortHandoverOk));
}

#[tokio::test]
async fn control_retiring_spawn_answers_typed_daemon_retiring_code() {
    let server = Arc::new(DaemonServer::new());
    server
        .handover_manager
        .control_retire_when_idle(&server.terminal_service);
    let response = data_exchange(
        &server,
        DaemonRequest::Spawn {
            client_request_id: "retiring-spawn".into(),
            workspace_id: "ws-retiring".into(),
            worktree: None,
            cwd: None,
            cols: 80,
            rows: 24,
            shell: None,
            startup: None,
            local_split: None,
            session_id: None,
        },
    )
    .await;
    match response {
        DaemonResponse::Error { code, .. } => {
            assert_eq!(code.as_deref(), Some(crate::daemon::control::CODE_DAEMON_RETIRING));
        }
        other => panic!("expected a typed DAEMON_RETIRING error, got {other:?}"),
    }
}

/// In-process control fixture: a DaemonServer whose retirement task writes its marker into a
/// temp runtime dir and whose exit is a oneshot instead of process::exit. The oneshot carries
/// the live session count seen at the moment of exit.
struct ControlFixture {
    server: Arc<DaemonServer>,
    runtime: tempfile::TempDir,
    exited: tokio::sync::oneshot::Receiver<u32>,
}

impl ControlFixture {
    fn new() -> Self {
        let server = Arc::new(DaemonServer::new());
        let runtime = tempfile::tempdir().expect("runtime dir");
        let (tx, exited) = tokio::sync::oneshot::channel();
        let tx = std::sync::Mutex::new(Some(tx));
        let terminals = Arc::clone(&server.terminal_service);
        server.handover_manager.set_retirement_action(move || {
            if let Some(tx) = tx.lock().unwrap().take() {
                let _ = tx.send(crate::daemon::handover::live_session_count(&terminals));
            }
        });
        server.spawn_control_retirement_task_in(runtime.path().to_path_buf());
        Self { server, runtime, exited }
    }

    fn marker(&self) -> PathBuf {
        self.runtime
            .path()
            .join(format!("retired-{}.json", std::process::id()))
    }

    /// Opens one control connection, sends request and returns the reply.
    async fn control(&self, request: ControlRequest, token: Option<&'static str>) -> ControlReply {
        let (client, server_side) = tokio::io::duplex(64 * 1024);
        let server = Arc::clone(&self.server);
        tokio::spawn(async move { server.handle_control_connection(server_side, token).await });
        let (read_half, mut write_half) = tokio::io::split(client);
        let mut frame = control::encode_request(&request);
        if let (Some(token), serde_json::Value::Object(map)) = (token, &mut frame) {
            map.insert(control::CONTROL_TOKEN_FIELD.into(), token.into());
        }
        write_half
            .write_all(control::to_line(&frame).as_bytes())
            .await
            .unwrap();
        let mut line = String::new();
        BufReader::new(read_half).read_line(&mut line).await.unwrap();
        control::decode_reply(request.op(), &line).expect("control reply")
    }

    async fn await_exit(&mut self) -> u32 {
        tokio::time::timeout(std::time::Duration::from_secs(10), &mut self.exited)
            .await
            .expect("retirement ran before the deadline")
            .expect("retirement action sent the live count")
    }
}

/// RequestUpgrade with no generation key, or a staged executable outside the daemon runtime
/// root, is refused before anything is prepared: no handover request, no job start, and the
/// daemon stays Active (todo 8 check (a)).
#[cfg(unix)]
#[tokio::test]
async fn request_upgrade_rejects_unstaged_executables_and_keeps_serving() {
    let fixture = ControlFixture::new();
    let outside = tempfile::tempdir().unwrap();
    let exe = outside.path().join("2026.1013.2-0a1b2c3d").join("ferryx");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"x").unwrap();
    for generation_key in [None, Some("2026.1013.2-0a1b2c3d".to_string())] {
        let reply = fixture
            .control(
                ControlRequest::RequestUpgrade {
                    staged_executable: exe.to_string_lossy().into_owned(),
                    generation_key,
                },
                None,
            )
            .await;
        assert_eq!(
            reply,
            ControlReply::Failed(ControlFailure::code(control::CODE_BAD_REQUEST))
        );
    }
    assert_eq!(
        fixture.server.handover_manager.status(),
        crate::daemon::handover::HandoverStatus::Active,
        "a rejected upgrade never prepares a handover"
    );
}

#[cfg(unix)]
fn spawn_cat(server: &DaemonServer) -> (String, tokio::sync::mpsc::Receiver<Vec<u8>>) {
    server
        .terminal_service
        .pty_manager()
        .spawn(portable_pty::CommandBuilder::new("/bin/cat"), 80, 24)
        .expect("spawn a live pty session")
}

fn spawn_request(id: &str) -> DaemonRequest {
    DaemonRequest::Spawn {
        client_request_id: id.into(),
        workspace_id: "ws-retiring".into(),
        worktree: None,
        cwd: None,
        cols: 80,
        rows: 24,
        shell: None,
        startup: None,
        local_split: None,
        session_id: None,
    }
}

fn assert_daemon_retiring(response: DaemonResponse) {
    match response {
        DaemonResponse::Error { code, .. } => {
            assert_eq!(code.as_deref(), Some(crate::daemon::control::CODE_DAEMON_RETIRING));
        }
        other => panic!("expected a typed DAEMON_RETIRING error, got {other:?}"),
    }
}

/// A Close whose own handler is parked after its response flush, i.e. after the session is gone
/// but before its RetirementGuard drops: no exit while parked, the client already has CloseOk,
/// and the daemon exits only after the handler is released.
#[cfg(unix)]
#[tokio::test]
async fn control_close_parked_after_session_removal_defers_exit_until_its_guard_drops() {
    let mut fixture = ControlFixture::new();
    let park = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    fixture.server.set_close_park_for_test(Arc::clone(&park));
    let (session_id, _output) = spawn_cat(&fixture.server);
    assert_eq!(
        fixture.control(ControlRequest::RetireWhenIdle, None).await,
        ControlReply::Live { live: 1 }
    );

    // Keep the handler task alive while it is parked (data_exchange would abort it).
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(&fixture.server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    let mut reader = BufReader::new(read_half);
    let mut json = serde_json::to_string(&DaemonRequest::Close { session_id }).unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), park.0.notified())
        .await
        .expect("the Close handler parked after its flush");

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let response: DaemonResponse = serde_json::from_str(line.trim()).unwrap();
    assert!(matches!(response, DaemonResponse::CloseOk), "{response:?}");
    assert_eq!(crate::daemon::handover::live_session_count(&fixture.server.terminal_service), 0);
    assert!(
        !fixture.server.handover_manager.retiring_signalled(),
        "no exit while a handler is parked"
    );
    assert!(fixture.exited.try_recv().is_err(), "no exit while a handler is parked");

    park.1.notify_one();
    assert_eq!(fixture.await_exit().await, 0);
    assert!(fixture.marker().exists());
    drop(write_half);
    serving.abort();
}

/// RetireWhenIdle on an idle daemon: the client receives { ok: true } and only then the daemon
/// writes its marker and exits.
#[tokio::test]
async fn control_retire_when_idle_on_idle_daemon_answers_before_exit() {
    let mut fixture = ControlFixture::new();
    let reply = fixture.control(ControlRequest::RetireWhenIdle, None).await;
    assert_eq!(reply, ControlReply::Live { live: 0 });
    assert_eq!(fixture.await_exit().await, 0);
    assert!(fixture.marker().exists());
}

/// RetireWhenIdle with one live session: spawns are refused with DAEMON_RETIRING at once, the
/// daemon does not exit while the session lives, then writes its marker and exits after close.
#[cfg(unix)]
#[tokio::test]
async fn control_retire_when_idle_with_live_session_exits_after_it_closes() {
    let mut fixture = ControlFixture::new();
    let (session_id, _output) = spawn_cat(&fixture.server);
    assert_eq!(
        fixture.control(ControlRequest::RetireWhenIdle, None).await,
        ControlReply::Live { live: 1 }
    );
    assert_daemon_retiring(data_exchange(&fixture.server, spawn_request("while-retiring")).await);
    assert!(
        !fixture.server.handover_manager.retiring_signalled(),
        "no exit signal while a session is live"
    );
    assert!(fixture.exited.try_recv().is_err(), "no exit while a session is live");
    assert!(!fixture.marker().exists());
    let response = data_exchange(&fixture.server, DaemonRequest::Close { session_id }).await;
    assert!(matches!(response, DaemonResponse::CloseOk), "{response:?}");
    assert_eq!(fixture.await_exit().await, 0);
    assert!(fixture.marker().exists());
}

/// RetireWhenIdle while a spawn is parked: the released spawn's session counts as live, and the
/// daemon exits only after that one session closes.
#[cfg(unix)]
#[tokio::test]
async fn control_retire_when_idle_parked_spawn_session_keeps_daemon_alive_until_closed() {
    let mut fixture = ControlFixture::new();
    let manager = Arc::clone(&fixture.server.handover_manager);
    let parked_spawn = manager.retain_spawn_owner().expect("parked spawn");
    assert_eq!(
        fixture.control(ControlRequest::RetireWhenIdle, None).await,
        ControlReply::Live { live: 0 }
    );
    // The parked spawn is released and creates its session.
    let (session_id, _output) = spawn_cat(&fixture.server);
    drop(parked_spawn);
    assert!(!manager.retiring_signalled(), "the released spawn's session is live");
    assert!(fixture.exited.try_recv().is_err());
    let response = data_exchange(&fixture.server, DaemonRequest::Close { session_id }).await;
    assert!(matches!(response, DaemonResponse::CloseOk), "{response:?}");
    assert_eq!(fixture.await_exit().await, 0);
}

/// Shutdown enters Retiring before counting: the next spawn gets DAEMON_RETIRING, and the
/// sessions the exit ends are exactly the confirmed count.
#[cfg(unix)]
#[tokio::test]
async fn control_shutdown_ends_exactly_the_confirmed_sessions() {
    let mut fixture = ControlFixture::new();
    let (_session_id, _output) = spawn_cat(&fixture.server);
    let reply = fixture
        .control(ControlRequest::Shutdown { confirm_live_sessions: 1 }, None)
        .await;
    assert_eq!(reply, ControlReply::Live { live: 1 });
    assert_daemon_retiring(data_exchange(&fixture.server, spawn_request("after-shutdown")).await);
    assert_eq!(fixture.await_exit().await, 1, "sessions ended == confirmed count");
    assert!(fixture.marker().exists());
}

/// Shutdown with a wrong count: COUNT_CHANGED, no exit and no marker.
#[tokio::test]
async fn control_shutdown_wrong_count_leaves_no_marker_and_no_exit() {
    let mut fixture = ControlFixture::new();
    let reply = fixture
        .control(ControlRequest::Shutdown { confirm_live_sessions: 5 }, None)
        .await;
    assert_eq!(
        reply,
        ControlReply::Failed(ControlFailure {
            code: control::CODE_COUNT_CHANGED.to_string(),
            live: Some(0),
        })
    );
    assert!(!fixture.server.handover_manager.retiring_signalled());
    assert!(fixture.exited.try_recv().is_err());
    assert!(!fixture.marker().exists());
    assert_eq!(
        fixture.server.handover_manager.status(),
        crate::daemon::handover::HandoverStatus::Active
    );
}

/// Windows loopback rule, exercised on the shared handler: a wrong first-frame token closes the
/// connection without a reply.
#[tokio::test]
async fn windows_wrong_token_connection_is_closed_without_reply() {
    let fixture = ControlFixture::new();
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let server = Arc::clone(&fixture.server);
    let handler = tokio::spawn(async move {
        server
            .handle_control_connection(server_side, Some("expected-token"))
            .await
    });
    let (read_half, mut write_half) = tokio::io::split(client);
    write_half
        .write_all(b"{\"control\":1,\"op\":\"Hello\",\"token\":\"wrong-token!!\"}\n")
        .await
        .unwrap();
    let mut line = String::new();
    let read = BufReader::new(read_half).read_line(&mut line).await.unwrap();
    assert_eq!(read, 0, "connection closed without a reply: {line:?}");
    tokio::time::timeout(std::time::Duration::from_secs(5), handler)
        .await
        .expect("handler returned")
        .unwrap();
}
