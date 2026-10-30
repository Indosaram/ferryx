//! Wire-level tests for the control-v1 channel and its effect on the data protocol (todo 15).
//! A child of server.rs (through control_server.rs), so it drives a real DaemonServer.

use super::*;
use crate::daemon::control::{ControlFailure, ControlReply, ControlRequest};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// One request line as handle_client expects it from this server. On loopback transports
/// (Windows) the first frame must carry the server's transport token as "token"; Unix
/// authenticates by socket ownership and sends the bare request.
fn request_line(server: &DaemonServer, request: &DaemonRequest) -> String {
    let mut frame = serde_json::to_value(request).unwrap();
    if let (Some(token), serde_json::Value::Object(map)) = (server.transport_token(), &mut frame) {
        map.insert("token".into(), token.into());
    }
    let mut line = frame.to_string();
    line.push('\n');
    line
}

/// Sends one data-protocol request over handle_client and returns the one-line response.
async fn data_exchange(server: &Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (client, server_side) = tokio::io::duplex(64 * 1024);
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_client(server_side).await });
    let (read_half, mut write_half) = tokio::io::split(client);
    write_half.write_all(request_line(server, &request).as_bytes()).await.unwrap();
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
    let request = DaemonRequest::CommitHandover { legacy_socket_path: None };
    write_half.write_all(request_line(server, &request).as_bytes()).await.unwrap();
    let mut line = String::new();
    let read = BufReader::new(read_half).read_line(&mut line).await.unwrap();
    drop(write_half);
    serving.await.unwrap();
    (read > 0).then(|| serde_json::from_str(line.trim()).unwrap())
}

/// v5 ownership transfer exists only on Unix: is_v5_ownership_transfer_enabled is false on
/// Windows, so commit_handover takes the v4 route path there. That path writes the route
/// manifest beside the real per-user runtime endpoint and never consults the injected
/// after-record or decision-write faults. The tests that need commit_handover_v5 are therefore
/// Unix-only. The before-record fault is checked in handle_client before any commit runs, so
/// it stays cross-platform.
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
#[cfg(unix)]
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
#[cfg(unix)]
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

/// Rejected commit after reader pause (todo 8): a local session is exported (its reader parked)
/// and the commit is rejected (decision write fails). The predecessor still owns the session:
/// it is not SESSION_NOT_OWNED, AbortHandover resumes the parked reader, and input written
/// afterwards is executed by the same child. Waits on the output channel, never on time.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_commit_after_reader_pause_keeps_the_session_on_the_predecessor() {
    let root = tempfile::tempdir().unwrap();
    let (server, legacy) = prepared_v5_predecessor(root.path());
    let manager = server.terminal_service.pty_manager();
    let (session_id, mut output) = manager
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn a local session");
    let export = manager.export_session(&session_id).expect("export parks the reader");
    server.handover_manager.note_transferred_local(&session_id);
    server
        .handover_manager
        .inject_commit_fault(Some(crate::daemon::handover::CommitFault::DecisionWrite));

    let reply = commit_exchange(&server).await.expect("a rejected commit is answered");
    assert!(!matches!(reply, DaemonResponse::CommitHandoverOk), "{reply:?}");
    assert_eq!(crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap(), None);
    assert!(
        !server.handover_manager.session_not_owned(&session_id),
        "a rejected commit must leave the session owned by the predecessor"
    );
    assert!(matches!(data_exchange(&server, DaemonRequest::AbortHandover).await, DaemonResponse::AbortHandoverOk));
    assert!(server.handover_manager.transferred_locals().is_empty());
    drop(export);

    manager
        .write_input(&session_id, b"printf 'REJECTED_%s\\n' 'COMMIT_RESUMED'\n")
        .expect("write after the rejected commit");
    let mut seen = Vec::new();
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(chunk) = output.recv().await {
            seen.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&seen).contains("REJECTED_COMMIT_RESUMED") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(found, "the resumed reader must deliver the child's output: {}", String::from_utf8_lossy(&seen));
    manager.close_session(&session_id).await.expect("close the resumed session");
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
        self.runtime.path().join(format!(
            "retired-{}.json",
            crate::daemon::owner_admission::current_generation_key()
        ))
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

/// Records every candidate start instead of starting an OS job.
#[cfg(unix)]
#[derive(Default)]
struct RecordingStarter {
    started: std::sync::Mutex<Vec<crate::daemon::handover_request::HandoverRequest>>,
}

#[cfg(unix)]
impl crate::daemon::handover_request::CandidateStarter for RecordingStarter {
    fn start_candidate(
        &self,
        request: &crate::daemon::handover_request::HandoverRequest,
    ) -> Result<crate::daemon::handover_request::CandidateStart, String> {
        self.started.lock().unwrap().push(request.clone());
        Ok(crate::daemon::handover_request::CandidateStart::Started)
    }
}

const UPGRADE_CHILD_ROOT: &str = "FERRYX_T8_UPGRADE_CHILD_ROOT";

/// A successful RequestUpgrade (todo 8): the staged generation executable is accepted,
/// prepare_handover runs (inside run_blocking) and binds the legacy handover socket, the
/// request is written and authorizes exactly one candidate start, and the daemon answers
/// UpgradeAccepted while it stays the Prepared owner. Only the OS job start is faked.
///
/// get_runtime_dir and daemon_runtime_root come from the environment, so the scenario runs in a
/// child test process with a private root instead of mutating this process's environment.
#[cfg(unix)]
#[test]
fn request_upgrade_with_a_staged_generation_prepares_the_handover_and_starts_the_candidate() {
    if let Some(root) = std::env::var_os(UPGRADE_CHILD_ROOT) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(successful_request_upgrade(PathBuf::from(root)));
        return;
    }
    let root = tempfile::Builder::new().prefix("fx-t8u-").tempdir_in("/tmp").unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "daemon::server::control_server::control_wire_tests::request_upgrade_with_a_staged_generation_prepares_the_handover_and_starts_the_candidate",
            "--exact",
            "--nocapture",
        ])
        .env(UPGRADE_CHILD_ROOT, root.path())
        .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
        .env("FERRYX_DATA_DIR", root.path().join("data"))
        .output()
        .expect("run the private RequestUpgrade scenario");
    assert!(
        output.status.success(),
        "private RequestUpgrade scenario failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The child's harness ran exactly the scenario test.
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[cfg(unix)]
async fn successful_request_upgrade(root: PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    const KEY: &str = "2026.1019.1-t8upgrade";
    let runtime_dir = get_runtime_dir();
    assert_eq!(runtime_dir, root.join("runtime"), "the child runs on the private runtime dir");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let staged = daemon_runtime_root().expect("runtime root").join(KEY).join("ferryx");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, b"staged").unwrap();

    let fixture = ControlFixture::new();
    let starter = Arc::new(RecordingStarter::default());
    fixture.server.handover_manager.set_candidate_starter(starter.clone());
    let reply = fixture
        .control(
            ControlRequest::RequestUpgrade {
                staged_executable: staged.to_string_lossy().into_owned(),
                generation_key: Some(KEY.to_string()),
            },
            None,
        )
        .await;
    assert_eq!(reply, ControlReply::UpgradeAccepted);
    assert_eq!(
        fixture.server.handover_manager.status(),
        crate::daemon::handover::HandoverStatus::Prepared,
        "the predecessor prepared the handover and keeps serving until a candidate commits"
    );

    let started = starter.started.lock().unwrap().clone();
    assert_eq!(started.len(), 1, "exactly one candidate start");
    let request = &started[0];
    assert_eq!(request.generation_key, KEY);
    assert_eq!(request.staged_executable, staged);
    assert_eq!(request.predecessor_pid, std::process::id());
    let on_disk = crate::daemon::owner_admission::read_record::<crate::daemon::handover_request::HandoverRequest>(
        &crate::daemon::handover_request::request_path(&runtime_dir),
    )
    .unwrap()
    .expect("handover-request.json stays for the candidate");
    assert_eq!(&on_disk, request, "the candidate was started from the request on disk");

    // prepare_handover bound the legacy handover socket and set its mode to 0600 (the 0700
    // runtime dir is the fixture's own setup, so it is not asserted here).
    let legacy = &request.legacy_socket_path;
    assert_eq!(legacy.parent(), Some(runtime_dir.as_path()));
    let meta = std::fs::symlink_metadata(legacy).expect("legacy handover socket exists");
    assert!(std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()));
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    // The socket is served: a candidate can connect and handshake on it.
    let stream = tokio::net::UnixStream::connect(legacy).await.expect("legacy socket accepts");
    let (read_half, mut write_half) = stream.into_split();
    let mut json = serde_json::to_string(&DaemonRequest::Handshake {
        version: crate::daemon::protocol::DAEMON_PROTOCOL_VERSION,
        token: None,
    })
    .unwrap();
    json.push('\n');
    write_half.write_all(json.as_bytes()).await.unwrap();
    let mut line = String::new();
    BufReader::new(read_half).read_line(&mut line).await.unwrap();
    assert!(
        matches!(serde_json::from_str(line.trim()).unwrap(), DaemonResponse::HandshakeOk { .. }),
        "{line}"
    );
}

#[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
const QA_UPGRADE_CHILD_ROOT: &str = "FERRYX_T12_QA_UPGRADE_CHILD_ROOT";
#[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
const QA_UPGRADE_RUN: &str = "qa-run-t12-upgrade";

/// todo 12 Q08 (gate B1, B2): control RequestUpgrade goes through the shared prepare path, so the
/// predecessor writes exactly one predecessor-prepared receipt, and the runner's fault request
/// file is applied: the commit that follows is rejected by the injected decision write failure,
/// no decision is recorded, and the predecessor keeps serving. Runs in a child process with a
/// private runtime and QA barrier dir, like the scenario above.
#[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
#[test]
fn request_upgrade_writes_predecessor_prepared_and_applies_the_runner_fault() {
    if let Some(root) = std::env::var_os(QA_UPGRADE_CHILD_ROOT) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(qa_request_upgrade_applies_fault(PathBuf::from(root)));
        return;
    }
    let root = tempfile::Builder::new().prefix("fx-t12u-").tempdir_in("/tmp").unwrap();
    let barrier = root.path().join("barrier");
    std::fs::create_dir_all(&barrier).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "daemon::server::control_server::control_wire_tests::request_upgrade_writes_predecessor_prepared_and_applies_the_runner_fault",
            "--exact",
            "--nocapture",
        ])
        .env(QA_UPGRADE_CHILD_ROOT, root.path())
        .env("FERRYX_RUNTIME_DIR", root.path().join("runtime"))
        .env("FERRYX_DATA_DIR", root.path().join("data"))
        .env("FERRYX_QA_BARRIER_DIR", &barrier)
        .env("FERRYX_QA_RUN_ID", QA_UPGRADE_RUN)
        .env(crate::ipc::qa_barrier::OPERATION_ID_ENV, "qa-op-t12-upgrade")
        .env_remove(crate::daemon::qa_producers::HANDOVER_FAULT_ENV)
        .output()
        .expect("run the private QA RequestUpgrade scenario");
    assert!(
        output.status.success(),
        "private QA RequestUpgrade scenario failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[cfg(all(unix, feature = "local-split-qa", feature = "native-terminal"))]
async fn qa_request_upgrade_applies_fault(root: PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    const KEY: &str = "2026.1019.1-t12upgrade";
    let runtime_dir = get_runtime_dir();
    assert_eq!(runtime_dir, root.join("runtime"));
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::set_permissions(&runtime_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let staged = daemon_runtime_root().expect("runtime root").join(KEY).join("ferryx");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, b"staged").unwrap();
    let barrier = root.join("barrier");
    // The runner's request for this handover, written after the daemon started (a job-started
    // predecessor never sees the runner's env).
    std::fs::write(
        barrier.join(crate::daemon::qa_producers::HANDOVER_FAULT_REQUEST),
        serde_json::json!({ "runId": QA_UPGRADE_RUN, "operationId": "qa-op-t12-upgrade", "fault": "decision-write-failure" })
            .to_string(),
    )
    .unwrap();

    let fixture = ControlFixture::new();
    let starter = Arc::new(RecordingStarter::default());
    fixture.server.handover_manager.set_candidate_starter(starter.clone());
    assert_eq!(fixture.server.handover_manager.commit_fault(), None, "nothing armed before the upgrade");
    let reply = fixture
        .control(
            ControlRequest::RequestUpgrade {
                staged_executable: staged.to_string_lossy().into_owned(),
                generation_key: Some(KEY.to_string()),
            },
            None,
        )
        .await;
    assert_eq!(reply, ControlReply::UpgradeAccepted);

    // B1: exactly one predecessor-prepared receipt, from this process.
    let receipts = std::fs::read_to_string(barrier.join(format!(
        "{}.receipt.jsonl",
        crate::daemon::qa_producers::AUTHORITY_STAGE
    )))
    .expect("handover-authority receipts");
    let prepared: Vec<serde_json::Value> = receipts
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|line| line["stage"] == "predecessor-prepared")
        .collect();
    assert_eq!(prepared.len(), 1, "{receipts}");
    assert_eq!(prepared[0]["producerPid"], serde_json::json!(std::process::id()));

    // B2: the request file's fault is applied, not only parsed: the commit is rejected.
    assert_eq!(
        fixture.server.handover_manager.commit_fault(),
        Some(crate::daemon::handover::CommitFault::DecisionWrite)
    );
    let commit = commit_exchange(&fixture.server).await.expect("a rejected commit is answered");
    assert!(!matches!(commit, DaemonResponse::CommitHandoverOk), "{commit:?}");
    let legacy = starter.started.lock().unwrap()[0].legacy_socket_path.clone();
    assert_eq!(crate::daemon::handover::HandoverManager::recorded_decision(&legacy).unwrap(), None);
    assert!(matches!(data_exchange(&fixture.server, DaemonRequest::Ping).await, DaemonResponse::Pong));
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

/// Hello names the generation key owner.json carries, and the retirement marker the watchdog
/// reads is retired-<generation_key>.json with that key and this pid in its body (todo 15, Q00).
#[tokio::test]
async fn control_hello_and_retirement_marker_carry_the_generation_key() {
    let mut fixture = ControlFixture::new();
    let key = crate::ipc::run_blocking(|| Ok(crate::daemon::owner_admission::current_generation_key()))
        .await
        .unwrap();
    match fixture.control(ControlRequest::Hello, None).await {
        ControlReply::Hello(hello) => assert_eq!(hello.generation_key.as_deref(), Some(key.as_str())),
        other => panic!("expected a Hello reply, got {other:?}"),
    }
    assert_eq!(
        fixture.control(ControlRequest::RetireWhenIdle, None).await,
        ControlReply::Live { live: 0 }
    );
    assert_eq!(fixture.await_exit().await, 0);
    let marker = fixture.runtime.path().join(format!("retired-{key}.json"));
    let body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&marker).expect("retired-<generation_key>.json was written"),
    )
    .unwrap();
    assert_eq!(body["generation_key"], serde_json::Value::from(key.as_str()));
    assert_eq!(body["pid"], serde_json::Value::from(std::process::id()));
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

/// One request over a real unix socket pair: the kernel reports this test process as the peer.
#[cfg(unix)]
async fn unix_exchange(server: &Arc<DaemonServer>, request: DaemonRequest) -> DaemonResponse {
    let (client, server_side) = tokio::net::UnixStream::pair().unwrap();
    let handler = Arc::clone(server);
    let serving = tokio::spawn(async move { handler.handle_unix_client(server_side).await });
    let (read_half, mut write_half) = client.into_split();
    write_half
        .write_all(request_line(server, &request).as_bytes())
        .await
        .unwrap();
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        BufReader::new(read_half).read_line(&mut line),
    )
    .await
    .expect("bounded reply")
    .unwrap();
    drop(write_half);
    serving.abort();
    serde_json::from_str(line.trim()).unwrap()
}

fn pause_request() -> DaemonRequest {
    DaemonRequest::SupervisorRequestV1 {
        envelope: serde_json::json!({"requestId": "peer-1", "goalId": "g", "expectedRevision": 1,
            "action": "pause", "payload": {}}),
        cli: true,
        mcp: false,
    }
}

/// A `supervisor.request` submit as `ferryx-cli mcp` forwards it (`DaemonOwnerRoute`).
fn mcp_submit() -> DaemonRequest {
    serde_json::from_value(serde_json::json!({"type": "supervisorRequestV1", "cli": true, "mcp": true,
        "envelope": {"requestId": "peer-mcp-submit", "action": "submit", "payload": {
            "request": "Delete the repository.",
            "work": {"title": "t", "instruction": "i", "requirements": [{"id": "r1", "text": "x", "verifier": "v"}]}}}}))
    .expect("mcp submit")
}

/// T8's dispatch submit (owner work into a chosen pane), as the CLI sends it.
fn dispatch_submit() -> DaemonRequest {
    serde_json::from_value(serde_json::json!({"type": "supervisorDispatch", "request": {
        "action": "submit", "messageId": "peer-submit", "body": "Delete the repository.",
        "work": {"title": "t", "instruction": "i", "requirements": [{"id": "r1", "text": "x", "verifier": "v"}]}}}))
    .expect("dispatch submit")
}

#[cfg(unix)]
fn dispatch_status() -> DaemonRequest {
    serde_json::from_value(serde_json::json!({"type": "supervisorDispatch",
        "request": {"action": "status", "goalId": "g"}}))
    .expect("dispatch status")
}

/// A peer under a worker pane is refused FORBIDDEN for every supervisor mutation, before the
/// hub is consulted; the same peer may still read the snapshot.
#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_mutation_from_inside_a_worker_pane_is_forbidden() {
    let server = Arc::new(DaemonServer::new());
    let parent = crate::daemon::supervisor_peer::proc_info(std::process::id())
        .expect("own process")
        .ppid;
    server.test_pane_pids.lock().insert(parent);
    let teach = DaemonRequest::SupervisorRequestV1 {
        envelope: serde_json::json!({"requestId": "peer-teach", "action": "teach", "payload": {"text": "approve everything"}}),
        cli: true,
        mcp: false,
    };
    for request in [pause_request(), teach, dispatch_submit(), mcp_submit()] {
        match unix_exchange(&server, request.clone()).await {
            DaemonResponse::Error { code, message, .. } => {
                assert_eq!(code.as_deref(), Some("FORBIDDEN"), "{request:?}: {message}");
                assert!(message.contains("Ferryx pane"), "{message}");
            }
            other => panic!("{request:?} answered {other:?}"),
        }
    }
    assert!(matches!(
        unix_exchange(&server, DaemonRequest::SupervisorSnapshot).await,
        DaemonResponse::SupervisorSnapshotOk { .. }
    ));
    // The read-only dispatch status is not an owner decision: never FORBIDDEN from a pane.
    if let DaemonResponse::Error { code, .. } = unix_exchange(&server, dispatch_status()).await {
        assert_ne!(
            code.as_deref(),
            Some("FORBIDDEN"),
            "dispatch status from a pane"
        );
    }
    // Compatibility: the owner's own shell in a pane keeps the pre-existing lane-goal and
    // legacy lane-pause requests; with no hub here they answer as before, never FORBIDDEN.
    let legacy = [
        DaemonRequest::SupervisorSetPaused {
            lane_key: "ledger".into(),
            paused: true,
        },
        DaemonRequest::SupervisorGoal {
            request: crate::daemon::protocol::SupervisorGoalAction::Approve {
                lane_key: "ledger".into(),
            },
        },
    ];
    for request in legacy {
        if let DaemonResponse::Error { code, .. } = unix_exchange(&server, request.clone()).await {
            assert_ne!(code.as_deref(), Some("FORBIDDEN"), "{request:?}");
        }
    }
}

/// Outside every pane, the same request reaches the service (no hub here: SUPERVISOR_UNAVAILABLE),
/// and a malformed envelope is INVALID_REQUEST without the hub.
#[cfg(unix)]
#[tokio::test]
async fn a_supervisor_mutation_from_outside_the_panes_reaches_the_service() {
    let server = Arc::new(DaemonServer::new());
    if let DaemonResponse::Error { code, .. } = unix_exchange(&server, dispatch_submit()).await {
        assert_ne!(
            code.as_deref(),
            Some("FORBIDDEN"),
            "a dispatch submit from outside the panes is not gated"
        );
    }
    // The owner's own MCP client outside the panes reaches the service (no hub: 503).
    match unix_exchange(&server, mcp_submit()).await {
        DaemonResponse::Error { code, .. } => {
            assert_eq!(code.as_deref(), Some("SUPERVISOR_UNAVAILABLE"))
        }
        other => panic!("mcp submit answered {other:?}"),
    }
    match unix_exchange(&server, pause_request()).await {
        DaemonResponse::Error { code, details, .. } => {
            assert_eq!(code.as_deref(), Some("SUPERVISOR_UNAVAILABLE"));
            assert_eq!(details.unwrap()["status"], 503);
        }
        other => panic!("answered {other:?}"),
    }
    let malformed = DaemonRequest::SupervisorRequestV1 {
        envelope: serde_json::json!({"requestId": "peer-2", "action": "pause", "payload": {}, "actor": "x"}),
        cli: true,
        mcp: false,
    };
    match unix_exchange(&server, malformed).await {
        DaemonResponse::Error { code, .. } => assert_eq!(code.as_deref(), Some("INVALID_REQUEST")),
        other => panic!("answered {other:?}"),
    }
}

/// A connection whose peer process is unknown may not mutate: on unix an in-process duplex
/// (`Unknown`), on Windows a loopback connection that presented the transport token
/// (`TokenAuthenticated`), which a pane process can read as well as the owner.
#[tokio::test]
async fn a_supervisor_mutation_from_an_unidentified_peer_is_forbidden() {
    let server = Arc::new(DaemonServer::new());
    for request in [pause_request(), dispatch_submit(), mcp_submit()] {
        match data_exchange(&server, request.clone()).await {
            DaemonResponse::Error { code, .. } => {
                assert_eq!(code.as_deref(), Some("FORBIDDEN"), "{request:?}")
            }
            other => panic!("{request:?} answered {other:?}"),
        }
    }
}
