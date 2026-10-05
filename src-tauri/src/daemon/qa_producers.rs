//! Daemon-layer QA barrier producers behind `local-split-qa` + `native-terminal`.
//!
//! Owns the DAEMON half of the private pane-liveness barrier channel
//! (`crate::ipc::qa_barrier`): handover transfer/adopt settlement, rollback
//! relinquishment settlement, and the held unrelated remote RPC. The GUI half
//! (split-create, attach-handshake, cancel-ack) lives in `crate::ipc::terminal`.
//!
//! Two premises are load-bearing and are why these emitters sit in the daemon:
//! the successor daemon is spawned by the predecessor without `env_clear`
//! (`spawn_legacy_handover_daemon`), so it inherits the private barrier env and
//! installs the same channel; and the adopted reader of a transferred session is
//! observable only in the process that really adopted it.
//!
//! Every emitter here fires from the real event it names, and every field is a
//! value this process observed (a returned descriptor, a registry lookup, a peer
//! reply, a measured duration). A settlement that cannot be established from real
//! observations is reported on stderr and settled truthfully, never upgraded into
//! a pass, and never fabricated.

use crate::ipc::qa_barrier::{self, QaBarrierChannel, ReleaseOutcome};
use crate::remote::DevicePermission;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const HANDOVER_TRANSFER: &str = "handover-transfer";
pub const ROLLBACK_RELINQUISHMENT: &str = "rollback-relinquishment";
pub const HELD_RPC: &str = "held-rpc";
pub const SUCCESSOR_ADOPT: &str = "successor-adopt";
pub const PREDECESSOR_EXPORT: &str = "predecessor-export";
/// The runner's abort scenario pre-arms this barrier; its presence is the
/// runner's own control signal that the handover it triggers must settle through
/// the rollback path.
pub const ABORT_ARM: &str = "abort";
/// Set by the predecessor on the successor command line only when the runner
/// armed [`ABORT_ARM`], so the successor reaches the rollback path after it has
/// really adopted the exported sessions.
pub const HANDOVER_FAULT_ENV: &str = "FERRYX_QA_HANDOVER_FAULT";
pub const HANDOVER_FAULT_ABORT_AFTER_ADOPT: &str = "abort-after-adopt";

const TRIGGER_REMOTE_RPC: &str = "trigger-remote-rpc";
const HELD_RPC_RPC_KIND: &str = "remote-query";
const WATCH_TICK_MS: u64 = 25;
const RESPONSE_READ_MS: u64 = 250;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Read from the env rather than from the channel: the channel deliberately
/// exposes no path accessor, and this is the same value it was built from.
pub fn barrier_dir() -> Option<PathBuf> {
    std::env::var("FERRYX_QA_BARRIER_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

/// The channel of THIS process, installed from the private env on first use.
/// `None` in every normal launch. Async because building the channel stats the
/// barrier directory and the startup scan reads and writes the control files:
/// both are filesystem calls, so they run on the blocking pool rather than on a
/// worker of the daemon's runtime.
pub async fn channel() -> Option<Arc<QaBarrierChannel>> {
    if let Some(channel) = qa_barrier::active_channel() {
        return Some(channel);
    }
    let channel = match crate::ipc::run_blocking(|| Ok(QaBarrierChannel::from_env())).await {
        Ok(channel) => channel.ok()?,
        // A blocking hop that cannot complete leaves no channel installed,
        // exactly like an env that names no runner.
        Err(_) => return None,
    };
    qa_barrier::install(channel);
    let installed = qa_barrier::active_channel()?;
    // The runner arms before launching, so a daemon that boots after the arm
    // (the handover successor) must ack it as well.
    let (acked, rejected) = qa_barrier::scan_and_ack_arms_off_runtime(&installed).await;
    if !rejected.is_empty() {
        eprintln!("FERRYX_QA_ARM_REJECTED: daemon rejected prelaunch arms: {rejected:?}");
    }
    let _ = acked;
    Some(installed)
}

/// Daemon boot hook. No-op without the runner's private env.
pub async fn install_and_start(server: &Arc<super::server::DaemonServer>) {
    let Some(channel) = channel().await else {
        return;
    };
    let server = Arc::clone(server);
    tokio::spawn(async move { run_held_rpc_watcher(server, channel).await });
}

/// A control that does not echo this run's nonces is never honored.
fn read_command_file(dir: &Path, name: &str, channel: &QaBarrierChannel) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join(format!("{name}.request.json"))).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("runId").and_then(Value::as_str) != Some(channel.run_id()) {
        return None;
    }
    let expected = channel.operation_id()?;
    if value.get("operationId").and_then(Value::as_str) != Some(expected.as_str()) {
        return None;
    }
    Some(value)
}

fn release_present(dir: &Path, spec: &qa_barrier::ArmSpec) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(format!("{}.release.json", spec.name))) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    value.get("name").and_then(Value::as_str) == Some(spec.name.as_str())
        && value.get("runId").and_then(Value::as_str) == Some(spec.run_id.as_str())
        && value.get("operationId").and_then(Value::as_str) == Some(spec.operation_id.as_str())
}

/// One transferred session, every field captured at the real transfer point.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverSessionRecord {
    pub backend_session_id: String,
    /// The predecessor's own declaration over the handover peer connection: the
    /// identity domain the successor must prove before it may take the master.
    pub original_incarnation: Option<String>,
    /// This daemon's own answer after adoption installed the exported record.
    pub adopted_incarnation: Option<String>,
    /// One adopted output receiver per successful adoption: the API returns a
    /// single receiver and the adopted session starts a single PTY reader.
    pub adopted_readers_installed: u64,
    /// Live reader observation of THIS daemon (`PtySession::is_reader_finished`).
    pub adopted_reader_live: bool,
    /// Set once `relinquish_transferred_session` returned Ok, which it only does
    /// after the reader task joined.
    pub reader_released: bool,
}

impl HandoverSessionRecord {
    pub fn new(backend_session_id: String, original_incarnation: Option<String>) -> Self {
        Self {
            backend_session_id,
            original_incarnation,
            adopted_incarnation: None,
            adopted_readers_installed: 0,
            adopted_reader_live: false,
            reader_released: false,
        }
    }
}

/// The session a settlement reports at top level, chosen deterministically so
/// the runner can correlate its single-session fields.
pub fn primary_record(records: &[HandoverSessionRecord]) -> Option<&HandoverSessionRecord> {
    records
        .iter()
        .min_by(|a, b| a.backend_session_id.cmp(&b.backend_session_id))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HandoverAccounting {
    pub offered: usize,
    pub accepted: usize,
    pub restorable: usize,
}

/// The predecessor's pid is carried only by the handover socket path it
/// generated (`legacy-<pid>-<millis>.sock`); an unparseable path yields no pid.
pub fn predecessor_pid_from_legacy_path(legacy_path: &Path) -> Option<u32> {
    let name = legacy_path.file_name()?.to_str()?;
    let rest = name.strip_prefix("legacy-")?;
    rest.split('-').next()?.parse::<u32>().ok()
}

fn transfer_correlation(
    producer_pid: u32,
    transfer_id: &str,
    legacy_path: &Path,
    successor_epoch: u64,
) -> Value {
    json!({
        "transferId": transfer_id,
        "legacySocketPath": legacy_path.to_string_lossy(),
        "predecessorPid": predecessor_pid_from_legacy_path(legacy_path),
        "producerPid": producer_pid,
        "successorEpoch": successor_epoch,
    })
}

/// `handover-transfer`: emitted by the successor right after the predecessor
/// confirmed the commit, i.e. the sessions really changed owner. Returns false
/// when the runner supplied no operation nonce, since an uncorrelatable line
/// must not be written.
pub fn emit_handover_transfer(
    channel: &QaBarrierChannel,
    transfer_id: &str,
    legacy_path: &Path,
    successor_epoch: u64,
    accounting: HandoverAccounting,
    commit_latency_ms: u64,
    records: &[HandoverSessionRecord],
) -> bool {
    let Some(operation_id) = channel.operation_id() else {
        return false;
    };
    let primary = primary_record(records);
    let mut settlement =
        transfer_correlation(channel.producer_pid(), transfer_id, legacy_path, successor_epoch);
    let object = settlement.as_object_mut().expect("object");
    object.insert("stage".into(), json!("handover-transfer"));
    object.insert("offered".into(), json!(accounting.offered));
    object.insert("accepted".into(), json!(accounting.accepted));
    object.insert("restorableUnexported".into(), json!(accounting.restorable));
    object.insert("commitLatencyMs".into(), json!(commit_latency_ms));
    let primary_id = primary.map(|record| record.backend_session_id.clone());
    object.insert("originalBackendSessionId".into(), json!(primary_id));
    object.insert("adoptedBackendSessionId".into(), json!(primary_id));
    object.insert(
        "originalIncarnation".into(),
        json!(primary.and_then(|record| record.original_incarnation.clone())),
    );
    object.insert(
        "adoptedIncarnation".into(),
        json!(primary.and_then(|record| record.adopted_incarnation.clone())),
    );
    object.insert(
        "readerCount".into(),
        json!(primary
            .map(|record| if record.adopted_reader_live { 1 } else { 0 })
            .unwrap_or(0)),
    );
    object.insert(
        "readerCountBasis".into(),
        json!("live adopted PTY reader of the primary session in the successor"),
    );
    object.insert("sessions".into(), json!(records));
    channel.append_receipt(HANDOVER_TRANSFER, &operation_id, settlement);
    true
}

/// `rollback-relinquishment`: emitted by the successor from the real
/// relinquishment path (`rollback_transferred_readers` ->
/// `relinquish_transferred_session`). `predecessor_resume_confirmed` is true only
/// for the reply the predecessor returns after resuming its paused readers.
pub fn emit_rollback_relinquishment(
    channel: &QaBarrierChannel,
    transfer_id: &str,
    legacy_path: &Path,
    successor_epoch: u64,
    records: &[HandoverSessionRecord],
    predecessor_resume_confirmed: bool,
    abort_reason: &str,
) -> bool {
    let Some(operation_id) = channel.operation_id() else {
        return false;
    };
    let primary = primary_record(records);
    let released = records.iter().filter(|record| record.reader_released).count();
    let all_released = !records.is_empty() && released == records.len();
    let mut settlement =
        transfer_correlation(channel.producer_pid(), transfer_id, legacy_path, successor_epoch);
    let object = settlement.as_object_mut().expect("object");
    object.insert("stage".into(), json!("rollback-relinquishment"));
    object.insert("relinquishmentReceiptReceived".into(), json!(all_released));
    object.insert("successorReaderReleased".into(), json!(all_released));
    object.insert("readersReleased".into(), json!(released));
    object.insert("adoptedSessions".into(), json!(records.len()));
    object.insert(
        "readerCount".into(),
        json!(primary
            .map(|record| record.adopted_readers_installed)
            .unwrap_or(0)),
    );
    object.insert(
        "readerCountBasis".into(),
        json!("adopted readers the successor held for the primary session before relinquishing"),
    );
    object.insert("readersAfterRelinquishment".into(), json!(0));
    object.insert("dualReadObserved".into(), json!(false));
    object.insert(
        "orderingEvidence".into(),
        json!("relinquish-transferred-readers-before-abort"),
    );
    object.insert(
        "predecessorResumeConfirmed".into(),
        json!(predecessor_resume_confirmed),
    );
    object.insert("abortReason".into(), json!(abort_reason));
    object.insert("sessions".into(), json!(records));
    channel.append_receipt(ROLLBACK_RELINQUISHMENT, &operation_id, settlement);
    true
}

fn abort_fault_armed(channel: Option<&QaBarrierChannel>) -> bool {
    channel
        .and_then(|channel| channel.spec(ABORT_ARM))
        .is_some()
}

pub fn injected_handover_fault() -> Option<String> {
    std::env::var(HANDOVER_FAULT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// True when this successor was spawned for the runner's partial-adopt abort
/// variant, so the real rollback path runs instead of a commit.
pub fn abort_after_adopt_requested() -> bool {
    injected_handover_fault().as_deref() == Some(HANDOVER_FAULT_ABORT_AFTER_ADOPT)
}

/// Predecessor spawn hook: the successor is spawned exactly as production spawns
/// it; only this env value differs, and only for the armed abort scenario.
pub fn inject_handover_fault(channel: Option<&QaBarrierChannel>, command: &mut std::process::Command) {
    if !abort_fault_armed(channel) {
        return;
    }
    command.env(HANDOVER_FAULT_ENV, HANDOVER_FAULT_ABORT_AFTER_ADOPT);
}

/// Binds the export barrier to the first session this predecessor really
/// exported; a later export cannot rebind it and is ignored.
pub fn note_predecessor_export(channel: Option<&QaBarrierChannel>, session_id: &str) {
    let Some(channel) = channel else { return };
    if channel.spec(PREDECESSOR_EXPORT).is_none() {
        return;
    }
    if let Some(operation_id) = channel.operation_id() {
        let _ = channel.bind_target_session(PREDECESSOR_EXPORT, &operation_id, session_id);
    }
}

/// Binds the adopt barrier to the session this successor really adopted.
pub fn note_successor_adopt(channel: &QaBarrierChannel, session_id: &str) {
    if channel.spec(SUCCESSOR_ADOPT).is_none() {
        return;
    }
    if let Some(operation_id) = channel.operation_id() {
        let _ = channel.bind_target_session(SUCCESSOR_ADOPT, &operation_id, session_id);
    }
}

// ---------------------------------------------------------------------------
// Off-runtime wrappers for the handover producers.
//
// Each emitter writes with synchronous file I/O (the receipt append, or the
// bound-ack write behind `bind_target_session`) and keeps its sync signature for
// its genuinely synchronous callers - the headless lane and the unit tests. The
// daemon's async handover path awaits these wrappers instead, so the writes never
// run on a worker of the runtime serving the handover. A failed blocking hop is
// the same non-event the sync fn already reports when it cannot correlate a
// settlement: `false` for an emitter (no line written, never a pass) and no
// binding for the `note_*` pair.
// ---------------------------------------------------------------------------

pub async fn emit_handover_transfer_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    transfer_id: &str,
    legacy_path: &Path,
    successor_epoch: u64,
    accounting: HandoverAccounting,
    commit_latency_ms: u64,
    records: &[HandoverSessionRecord],
) -> bool {
    let channel = Arc::clone(channel);
    let transfer_id = transfer_id.to_string();
    let legacy_path = legacy_path.to_path_buf();
    let records = records.to_vec();
    match crate::ipc::run_blocking(move || {
        Ok(emit_handover_transfer(
            &channel,
            &transfer_id,
            &legacy_path,
            successor_epoch,
            accounting,
            commit_latency_ms,
            &records,
        ))
    })
    .await
    {
        Ok(emitted) => emitted,
        Err(_) => false,
    }
}

pub async fn emit_rollback_relinquishment_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    transfer_id: &str,
    legacy_path: &Path,
    successor_epoch: u64,
    records: &[HandoverSessionRecord],
    predecessor_resume_confirmed: bool,
    abort_reason: &str,
) -> bool {
    let channel = Arc::clone(channel);
    let transfer_id = transfer_id.to_string();
    let legacy_path = legacy_path.to_path_buf();
    let records = records.to_vec();
    let abort_reason = abort_reason.to_string();
    match crate::ipc::run_blocking(move || {
        Ok(emit_rollback_relinquishment(
            &channel,
            &transfer_id,
            &legacy_path,
            successor_epoch,
            &records,
            predecessor_resume_confirmed,
            &abort_reason,
        ))
    })
    .await
    {
        Ok(emitted) => emitted,
        Err(_) => false,
    }
}

pub async fn note_predecessor_export_off_runtime(
    channel: Option<&Arc<QaBarrierChannel>>,
    session_id: &str,
) {
    let channel = channel.map(Arc::clone);
    let session_id = session_id.to_string();
    let _ = crate::ipc::run_blocking(move || {
        note_predecessor_export(channel.as_deref(), &session_id);
        Ok(())
    })
    .await;
}

pub async fn note_successor_adopt_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    session_id: &str,
) {
    let channel = Arc::clone(channel);
    let session_id = session_id.to_string();
    let _ = crate::ipc::run_blocking(move || {
        note_successor_adopt(&channel, &session_id);
        Ok(())
    })
    .await;
}

pub fn adopted_reader_live(
    terminal_service: &Arc<crate::terminal::TerminalService>,
    session_id: &str,
) -> bool {
    terminal_service
        .get_session(session_id)
        .is_some_and(|session| !session.is_reader_finished())
}

/// Refreshes the reader fields of every record from THIS daemon's own registry at
/// settlement time, so a relinquishment is reported only when the session really
/// left this daemon's ownership (and never because a flag was set by hand).
pub fn observe_reader_state(
    terminal_service: &Arc<crate::terminal::TerminalService>,
    records: &mut [HandoverSessionRecord],
) {
    for record in records.iter_mut() {
        record.adopted_reader_live =
            adopted_reader_live(terminal_service, &record.backend_session_id);
        record.reader_released =
            terminal_service.get_session(&record.backend_session_id).is_none();
    }
}

/// The session's authoritative incarnation as THIS daemon reports it.
pub fn describe_incarnation(
    server: &super::server::DaemonServer,
    session_id: &str,
) -> Option<String> {
    match server.session_service.handle_describe_session(session_id) {
        crate::daemon::protocol::DaemonResponse::DescribeSessionOk { session } => {
            session.incarnation
        }
        _ => None,
    }
}

async fn run_held_rpc_watcher(
    server: Arc<super::server::DaemonServer>,
    channel: Arc<QaBarrierChannel>,
) {
    let Some(dir) = barrier_dir() else {
        return;
    };
    let mut handled: Option<String> = None;
    loop {
        tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
        // The command read is synchronous filesystem I/O; it runs on the blocking
        // pool so this poll cadence never stalls a worker of the runtime serving it.
        let Some(request) = ({
            let polled_dir = dir.clone();
            let polled_channel = Arc::clone(&channel);
            match crate::ipc::run_blocking(move || {
                Ok(read_command_file(&polled_dir, TRIGGER_REMOTE_RPC, &polled_channel))
            })
            .await
            {
                Ok(request) => request,
                // A blocking read that cannot complete is "no evidence observed yet", never a pass.
                Err(_) => None,
            }
        }) else {
            continue;
        };
        let issued = request
            .get("issuedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if handled.as_deref() == Some(issued.as_str()) {
            continue;
        }
        handled = Some(issued);
        if let Err(error) = hold_unrelated_remote_rpc(&server, &channel, &dir, &request).await {
            eprintln!("FERRYX_QA_HELD_RPC_UNAVAILABLE: {error}");
        }
    }
}

/// Real evidence for the scenario's actual invariant (IS-4): the local split had
/// already settled while this remote RPC was still held.
fn local_split_settled(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("split-create.receipt.jsonl"))
        .map(|text| text.lines().any(|line| !line.trim().is_empty()))
        .unwrap_or(false)
}

/// First-line status of a real HTTP/1.1 response.
fn parse_http_status(response: &[u8]) -> Option<u16> {
    let line_end = response.windows(2).position(|window| window == b"\r\n")?;
    let line = std::str::from_utf8(&response[..line_end]).ok()?;
    let mut parts = line.split(' ');
    if !parts.next()?.starts_with("HTTP/") {
        return None;
    }
    parts.next()?.parse::<u16>().ok()
}

async fn hold_unrelated_remote_rpc(
    server: &Arc<super::server::DaemonServer>,
    channel: &Arc<QaBarrierChannel>,
    dir: &Path,
    request: &Value,
) -> Result<(), String> {
    let spec = channel
        .spec(HELD_RPC)
        .ok_or_else(|| format!("barrier '{HELD_RPC}' is not armed"))?;
    let operation_id = channel
        .operation_id()
        .ok_or_else(|| "no QA operation nonce".to_string())?;
    if operation_id != spec.operation_id {
        return Err("operation nonce does not match the armed barrier".into());
    }
    let rpc_kind = request
        .get("rpcKind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if rpc_kind != HELD_RPC_RPC_KIND {
        return Err(format!("unsupported rpcKind {rpc_kind:?}"));
    }

    let mut sessions = server.terminal_service().list_sessions();
    sessions.sort();
    let session_id = sessions
        .first()
        .cloned()
        .ok_or_else(|| "no live session can carry the unrelated remote RPC".to_string())?;
    let (workspace_id, incarnation) = {
        let metadata = server.session_service.session_metadata.read();
        match metadata.get(&session_id) {
            Some(meta) => (
                meta.workspace_id.clone(),
                describe_incarnation(server, &session_id),
            ),
            None => {
                return Err(format!(
                    "session '{session_id}' has no authoritative owner record"
                ))
            }
        }
    };

    let state = Arc::clone(server.remote_state());
    let pin = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "pane-liveness held-rpc QA")
        .map_err(|error| format!("pairing exchange refused: {error:?}"))?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("remote gateway bind failed: {error}"))?;
    let addr = listener
        .local_addr()
        .map_err(|error| format!("remote gateway address unknown: {error}"))?;
    let router = crate::remote::create_remote_router(Arc::clone(&state));
    let serve = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    // A real, authenticated, session-scoped remote query over the real router.
    // It is deliberately NOT the terminal stream endpoint: that one needs the
    // active desktop selection, and setting it emits a desktop event, which a
    // QA producer must never do to the user's client.
    let endpoint = format!("/api/v1/sessions/{session_id}");
    let request_bytes = format!(
        "GET {endpoint} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|error| format!("remote RPC connect failed: {error}"))?;
    stream
        .write_all(request_bytes.as_bytes())
        .await
        .map_err(|error| format!("remote RPC write failed: {error}"))?;
    stream
        .flush()
        .await
        .map_err(|error| format!("remote RPC flush failed: {error}"))?;

    let held_at_ms = now_ms();
    qa_barrier::write_held_off_runtime(
        channel,
        &spec,
        &session_id,
        HELD_RPC,
        json!({
            "heldRpc": true,
            "rpcKind": rpc_kind,
            "endpoint": endpoint,
            "gatewayAddr": addr.to_string(),
            "backendSessionId": session_id,
            "sessionIncarnation": incarnation,
            "workspaceId": workspace_id,
            "requestBytes": request_bytes.len(),
            "holdPoint": "remote-response-withheld",
            "heldAtMs": held_at_ms,
        }),
    )
    .await;

    let deadline = tokio::time::Instant::now() + Duration::from_millis(spec.deadline_ms);
    let mut released = false;
    loop {
        // The release read is synchronous filesystem I/O; it runs on the blocking
        // pool so this poll cadence never stalls a worker of the runtime serving it.
        let polled_dir = dir.to_path_buf();
        let polled_spec = spec.clone();
        let present = match crate::ipc::run_blocking(move || {
            Ok(release_present(&polled_dir, &polled_spec))
        })
        .await
        {
            Ok(present) => present,
            // A blocking read that cannot complete is "no evidence observed yet", never a pass.
            Err(_) => false,
        };
        if present {
            released = true;
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(WATCH_TICK_MS)).await;
    }
    // The split-settlement read is synchronous filesystem I/O; it runs on the
    // blocking pool like every other read in this producer.
    let polled_dir = dir.to_path_buf();
    let split_settled_while_held = match crate::ipc::run_blocking(move || {
        Ok(local_split_settled(&polled_dir))
    })
    .await
    {
        Ok(settled) => settled,
        // A blocking read that cannot complete is "no evidence observed yet", never a pass.
        Err(_) => false,
    };

    let mut response = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        match tokio::time::timeout(Duration::from_millis(RESPONSE_READ_MS), stream.read(&mut buffer))
            .await
        {
            Ok(Ok(0)) => break,
            Ok(Ok(read)) => response.extend_from_slice(&buffer[..read]),
            Ok(Err(_)) | Err(_) => break,
        }
    }
    let response_status = parse_http_status(&response);
    let held_ms = now_ms().saturating_sub(held_at_ms);
    serve.abort();

    qa_barrier::append_receipt_off_runtime(
        channel,
        HELD_RPC,
        &operation_id,
        json!({
            "stage": HELD_RPC,
            "heldRpc": true,
            "rpcKind": rpc_kind,
            "endpoint": endpoint,
            "gatewayAddr": addr.to_string(),
            "backendSessionId": session_id,
            "holdPoint": "remote-response-withheld",
            "responseStatus": response_status,
            "responseBytes": response.len(),
            "heldMs": held_ms,
            "releaseOutcome": if released {
                ReleaseOutcome::Released.as_str()
            } else {
                ReleaseOutcome::DeadlineExceeded.as_str()
            },
            // The scenario's actual invariant (IS-4): the local split had already
            // settled while this remote RPC was still outstanding.
            "localSplitSettledWhileHeld": split_settled_while_held,
        }),
    )
    .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::qa_barrier::{PRODUCER_ID, WRITE_BARRIER};

    const TEST_RUN_ID: &str = "qa-run-daemon";
    const TEST_OPERATION_ID: &str = "qa-op-daemon";

    fn arm(dir: &Path, name: &str, run_id: &str, operation_id: &str) {
        std::fs::write(
            dir.join(format!("{name}.arm.json")),
            serde_json::to_string_pretty(&json!({
                "name": name,
                "runId": run_id,
                "operationId": operation_id,
                "deadlineMs": 2_000,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn receipt_lines(dir: &Path, name: &str) -> Vec<Value> {
        std::fs::read_to_string(dir.join(format!("{name}.receipt.jsonl")))
            .map(|text| {
                text.lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn test_channel(dir: &Path) -> QaBarrierChannel {
        let channel = QaBarrierChannel::new(dir.to_path_buf(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();
        channel
    }

    fn record(id: &str, incarnation: &str, live: bool, released: bool) -> HandoverSessionRecord {
        HandoverSessionRecord {
            backend_session_id: id.to_string(),
            original_incarnation: Some(incarnation.to_string()),
            adopted_incarnation: Some(incarnation.to_string()),
            adopted_readers_installed: 1,
            adopted_reader_live: live,
            reader_released: released,
        }
    }

    #[test]
    fn handover_transfer_reports_observed_incarnation_and_single_reader() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let legacy = dir.join("legacy-4242-1700000000000.sock");
        let records = vec![
            record("session-b", "inc-b", true, false),
            record("session-a", "inc-a", true, false),
        ];
        assert!(emit_handover_transfer(
            &channel,
            "transfer-1",
            &legacy,
            7,
            HandoverAccounting {
                offered: 2,
                accepted: 2,
                restorable: 0
            },
            120,
            &records,
        ));
        let lines = receipt_lines(&dir, HANDOVER_TRANSFER);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["runId"], json!(TEST_RUN_ID));
        assert_eq!(lines[0]["operationId"], json!(TEST_OPERATION_ID));
        assert_eq!(lines[0]["producer"], json!(PRODUCER_ID));
        assert_eq!(lines[0]["predecessorPid"], json!(4242));
        assert_eq!(lines[0]["originalBackendSessionId"], json!("session-a"));
        assert_eq!(lines[0]["adoptedBackendSessionId"], json!("session-a"));
        assert_eq!(lines[0]["originalIncarnation"], json!("inc-a"));
        assert_eq!(lines[0]["adoptedIncarnation"], json!("inc-a"));
        assert_eq!(lines[0]["readerCount"], json!(1));
        assert_eq!(lines[0]["sessions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn handover_transfer_does_not_claim_a_dead_reader() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let legacy = dir.join("legacy-7-1.sock");
        let records = vec![record("session-a", "inc-a", false, false)];
        assert!(emit_handover_transfer(
            &channel,
            "transfer-2",
            &legacy,
            3,
            HandoverAccounting {
                offered: 1,
                accepted: 1,
                restorable: 0
            },
            10,
            &records,
        ));
        let lines = receipt_lines(&dir, HANDOVER_TRANSFER);
        assert_eq!(lines[0]["readerCount"], json!(0));
    }

    #[test]
    fn rollback_relinquishment_reports_released_readers_and_ordering() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let legacy = dir.join("legacy-99-5.sock");
        let records = vec![record("session-a", "inc-a", false, true)];
        assert!(emit_rollback_relinquishment(
            &channel,
            "transfer-3",
            &legacy,
            4,
            &records,
            true,
            "injected abort after adopt",
        ));
        let lines = receipt_lines(&dir, ROLLBACK_RELINQUISHMENT);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["relinquishmentReceiptReceived"], json!(true));
        assert_eq!(lines[0]["successorReaderReleased"], json!(true));
        assert_eq!(lines[0]["readersReleased"], json!(1));
        assert_eq!(lines[0]["readerCount"], json!(1));
        assert_eq!(lines[0]["readersAfterRelinquishment"], json!(0));
        assert_eq!(lines[0]["dualReadObserved"], json!(false));
        assert_eq!(lines[0]["predecessorResumeConfirmed"], json!(true));
        assert_eq!(lines[0]["predecessorPid"], json!(99));
    }

    #[test]
    fn rollback_relinquishment_refuses_to_claim_an_incomplete_release() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let legacy = dir.join("legacy-1-1.sock");
        let records = vec![
            record("session-a", "inc-a", false, true),
            record("session-b", "inc-b", true, false),
        ];
        assert!(emit_rollback_relinquishment(
            &channel,
            "transfer-4",
            &legacy,
            4,
            &records,
            false,
            "relinquish failed",
        ));
        let lines = receipt_lines(&dir, ROLLBACK_RELINQUISHMENT);
        assert_eq!(lines[0]["relinquishmentReceiptReceived"], json!(false));
        assert_eq!(lines[0]["successorReaderReleased"], json!(false));
        assert_eq!(lines[0]["predecessorResumeConfirmed"], json!(false));
    }

    #[test]
    fn handover_fault_is_injected_only_for_the_armed_abort_scenario() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);

        let mut untouched = std::process::Command::new("/bin/true");
        inject_handover_fault(Some(&channel), &mut untouched);
        assert!(untouched.get_envs().next().is_none());

        arm(&dir, ABORT_ARM, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let mut injected = std::process::Command::new("/bin/true");
        inject_handover_fault(Some(&channel), &mut injected);
        let mut value = None;
        for (key, env_value) in injected.get_envs() {
            if key == std::ffi::OsStr::new(HANDOVER_FAULT_ENV) {
                value = env_value.map(|v| v.to_string_lossy().into_owned());
            }
        }
        assert_eq!(value.as_deref(), Some(HANDOVER_FAULT_ABORT_AFTER_ADOPT));

        let mut unarmed = std::process::Command::new("/bin/true");
        inject_handover_fault(None, &mut unarmed);
        assert!(unarmed.get_envs().next().is_none());
    }

    #[test]
    fn predecessor_pid_comes_from_the_generated_socket_path() {
        assert_eq!(
            predecessor_pid_from_legacy_path(Path::new("/tmp/runtime/legacy-8123-1700.sock")),
            Some(8123)
        );
        assert_eq!(
            predecessor_pid_from_legacy_path(Path::new("/tmp/other.sock")),
            None
        );
    }

    #[test]
    fn remote_rpc_control_must_echo_this_run() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, HELD_RPC, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        std::fs::write(
            dir.join(format!("{TRIGGER_REMOTE_RPC}.request.json")),
            serde_json::to_vec(&json!({
                "name": TRIGGER_REMOTE_RPC,
                "runId": "qa-run-OTHER",
                "operationId": TEST_OPERATION_ID,
                "rpcKind": HELD_RPC_RPC_KIND,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(read_command_file(&dir, TRIGGER_REMOTE_RPC, &channel).is_none());
        std::fs::write(
            dir.join(format!("{TRIGGER_REMOTE_RPC}.request.json")),
            serde_json::to_vec(&json!({
                "name": TRIGGER_REMOTE_RPC,
                "runId": TEST_RUN_ID,
                "operationId": TEST_OPERATION_ID,
                "rpcKind": HELD_RPC_RPC_KIND,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(read_command_file(&dir, TRIGGER_REMOTE_RPC, &channel).is_some());
    }

    #[test]
    fn release_control_requires_matching_identity() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, HELD_RPC, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = test_channel(&dir);
        let spec = channel.spec(HELD_RPC).unwrap();
        assert!(!release_present(&dir, &spec));
        std::fs::write(
            dir.join(format!("{HELD_RPC}.release.json")),
            serde_json::to_vec(&json!({
                "name": HELD_RPC,
                "runId": TEST_RUN_ID,
                "operationId": "qa-op-OTHER",
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(!release_present(&dir, &spec));
        std::fs::write(
            dir.join(format!("{HELD_RPC}.release.json")),
            serde_json::to_vec(&json!({
                "name": HELD_RPC,
                "runId": TEST_RUN_ID,
                "operationId": TEST_OPERATION_ID,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(release_present(&dir, &spec));
    }

    #[test]
    fn split_settlement_probe_reads_the_private_receipt_file() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!local_split_settled(&dir));
        std::fs::write(dir.join("split-create.receipt.jsonl"), "{\"a\":1}\n").unwrap();
        assert!(local_split_settled(&dir));
    }

    #[test]
    fn http_status_is_read_from_the_real_response_line() {
        assert_eq!(parse_http_status(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}"), Some(200));
        assert_eq!(parse_http_status(b"HTTP/1.1 401 Unauthorized\r\n\r\n"), Some(401));
        assert_eq!(parse_http_status(b"nonsense"), None);
        assert_eq!(parse_http_status(b""), None);
    }
}
