use super::workspace_service::DaemonWorkspaceService;
use crate::daemon::agent_state::AgentStateHub;
use crate::daemon::protocol::{
    AgentProviderSessionKey, DaemonRemoteEvent, DaemonResponse, DaemonSessionDetails,
    TerminalStartup,
};
use crate::session::{load_session_from_path, save_session_to_path};
use crate::terminal::{PtySessionState, TerminalService};
use crate::worktree::WorktreeIdentity;
use parking_lot::{Mutex, RwLock};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;

pub(crate) fn normalize_process_cwd(path: &Path) -> PathBuf {
    let Some(path_str) = path.to_str() else {
        return path.to_path_buf();
    };

    if let Some(rest) = path_str.strip_prefix(r"\\?\") {
        if rest.len() >= 4 && rest[..4].eq_ignore_ascii_case(r"UNC\") {
            return PathBuf::from(format!(r"\\{}", &rest[4..]));
        }

        let bytes = rest.as_bytes();
        if bytes.len() >= 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

#[path = "machine_owner.rs"]
mod machine_owner;
#[path = "session_metadata_events.rs"]
mod session_metadata_events;
#[path = "session_metadata_provider.rs"]
mod session_metadata_provider;
#[path = "session_metadata_forward.rs"]
mod session_metadata_forward;

const SPAWN_REQUEST_TTL: Duration = Duration::from_secs(30);

#[cfg(test)]
#[path = "session_service_machine_tests.rs"]
mod machine_tests;

pub(crate) struct MachineSpawn {
    pub request: crate::remote::machine_protocol::CreateSessionRequest,
    pub device: String,
    pub digest: String,
    pub target: crate::remote::machine_protocol::RemoteTerminalTarget,
    pub deadline: Instant,
    pub check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}
tokio::task_local! { pub(crate) static MACHINE_SPAWN: Arc<MachineSpawn>; }

#[derive(Clone)]
pub(super) struct SpawnCacheEntry {
    session_id: String,
    created_at: Instant,
    fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredSessionMeta {
    #[serde(default)]
    pub(super) machine_session: Option<crate::remote::machine_protocol::Session>,
    pub(super) client_request_id: String,
    pub(super) workspace_id: String,
    pub(super) worktree: Option<WorktreeIdentity>,
    pub(super) cwd: PathBuf,
    pub(super) provider_claim: Option<ProviderSessionClaimKey>,
    pub(super) spawn_fingerprint: SpawnRequestFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct SpawnRequestFingerprint {
    pub(super) workspace_id: String,
    pub(super) worktree: Option<WorktreeIdentity>,
    pub(super) cwd: Option<String>,
    pub(super) cols: u16,
    pub(super) rows: u16,
    pub(super) shell: Option<String>,
    pub(super) provider_claim: Option<ProviderSessionClaimKey>,
    pub(super) startup: Option<TerminalStartup>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct ProviderSessionClaimKey {
    agent_type: String,
    provider_key: AgentProviderSessionKey,
    provider_id: String,
    transcript_path: Option<String>,
}

impl ProviderSessionClaimKey {
    pub(super) fn from_startup(startup: Option<&TerminalStartup>) -> Option<Self> {
        let TerminalStartup::AgentResume {
            agent_type,
            provider_session,
        } = startup?
        else {
            return None;
        };
        let agent_type = agent_type.trim().to_ascii_lowercase();
        let transcript_path = if matches!(agent_type.as_str(), "pi" | "prime-agent") {
            provider_session
                .transcript_path
                .as_deref()
                .map(str::trim)
                .map(str::to_string)
        } else {
            None
        };
        Some(Self {
            agent_type,
            provider_key: provider_session.key,
            provider_id: provider_session.id.trim().to_string(),
            transcript_path,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SpawnError {
    #[error(
        "AgentSessionConflict: {agent_type} {provider_key:?} '{provider_id}' is already owned by session '{existing_session_id}'"
    )]
    AgentSessionConflict {
        agent_type: String,
        provider_key: AgentProviderSessionKey,
        provider_id: String,
        existing_session_id: String,
    },
    #[error("{0}")]
    InvalidAgentResume(String),
    #[error("{0}")]
    Other(String),
}

impl SpawnError {
    #[cfg(test)]
    pub(super) fn contains(&self, needle: &str) -> bool {
        self.to_string().contains(needle)
    }
}

impl From<String> for SpawnError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DurableRemoteSession {
    descriptor: crate::terminal::remote::RemoteSessionDescriptor,
    metadata: Option<StoredSessionMeta>,
}

/// Entries share disconnect state only with their own socket generation.
pub(crate) struct MachineController {
    pub device: String,
    pub generation: u64,
    pub cancelled: tokio::sync::watch::Sender<bool>,
    pub disconnected: Arc<Mutex<Option<tokio::time::Instant>>>,
}

impl From<&str> for MachineController {
    fn from(device: &str) -> Self {
        Self { device: device.into(), generation: 0, cancelled: tokio::sync::watch::channel(false).0,
            disconnected: Arc::new(Mutex::new(None)) }
    }
}

impl MachineController {
    pub(crate) fn reserved(&self) -> bool {
        self.reserved_at(tokio::time::Instant::now())
    }

    pub(crate) fn reserved_at(&self, now: tokio::time::Instant) -> bool {
        self.disconnected.lock().is_none_or(|at| now.duration_since(at) < Duration::from_secs(15))
    }
}

pub(crate) struct MachineSocketLease {
    pub generation: u64,
    pub cancelled: tokio::sync::watch::Receiver<bool>,
    disconnected: Arc<Mutex<Option<tokio::time::Instant>>>,
}

impl Drop for MachineSocketLease {
    fn drop(&mut self) { *self.disconnected.lock() = Some(tokio::time::Instant::now()); }
}

/// Headless session authority; owns metadata, claims and spawn idempotency.
/// Holds no server/gateway or AppHandle. Handover is weak to avoid a callback cycle.
pub struct DaemonSessionService {
    pub(crate) workspace_service: Arc<DaemonWorkspaceService>,
    pub(super) terminal_service: Arc<TerminalService>,
    pub(super) session_router: Arc<super::proxy::SessionRouter>,
    pub(super) handover_manager: Weak<super::handover::HandoverManager>,
    pub(super) remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
    pub(super) spawn_idempotency_cache: Arc<Mutex<HashMap<String, SpawnCacheEntry>>>,
    pub(super) spawn_lock: Arc<tokio::sync::Mutex<()>>,
    /// Sole authority shared by socket replacement, input, resize and HTTP close.
    pub(crate) machine_controllers: tokio::sync::Mutex<HashMap<String, MachineController>>,
    pub(super) machine_lifecycles: Arc<Mutex<HashMap<String, tokio::sync::watch::Receiver<bool>>>>,
    pub(super) remote_persistence_lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) remote_sessions_path: PathBuf,
    pub(super) ssh_store_path: PathBuf,
    pub(super) session_metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
    pub(super) provider_session_claims: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    pub(super) agent_states: Arc<AgentStateHub>,
}

impl DaemonSessionService {
    #[cfg(test)]
    pub(crate) fn journal_spawn_probe_handles(&self) -> (Arc<tokio::sync::Mutex<()>>, Arc<TerminalService>) {
        (self.spawn_lock.clone(), self.terminal_service.clone())
    }

    pub(crate) fn attach_machine_output(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<Result<crate::terminal::output_hub::machine_output::MachineAttachment,
        crate::terminal::output_hub::machine_output::MachineOutputError>> {
        self.terminal_service.output_hub().subscribe_machine(session_id, after_sequence)
    }

    pub(crate) async fn validate_machine_target(self: &Arc<Self>, target: &crate::remote::machine_protocol::RemoteTerminalTarget)
        -> Result<crate::remote::machine_protocol::Session, String> {
        let service = Arc::clone(self);
        let target = target.clone();
        tokio::task::spawn_blocking(move || service.validate_machine_target_blocking(&target))
            .await.map_err(|error| format!("Machine target validation task failed: {error}"))?
    }

    fn validate_machine_target_blocking(&self, target: &crate::remote::machine_protocol::RemoteTerminalTarget)
        -> Result<crate::remote::machine_protocol::Session, String> {
        // Metadata publication is not loss of authority. Wait off-runtime with
        // a bounded lock deadline; socket revocation/fencing remains cancellable.
        let deadline = Instant::now() + Duration::from_secs(10);
        let metadata = self.session_metadata.try_read_until(deadline).ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        let meta = metadata.get(&target.session_id).ok_or("SESSION_EXPIRED")?;
        let session = meta.machine_session.as_ref().ok_or("SESSION_NOT_FOUND")?;
        if session.target != *target { return Err("STALE_EPOCH".into()); }
        if meta.workspace_id != session.workspace_id
            || meta.worktree.as_ref().map(|w| (&w.ws_id, &w.slug)) != session.worktree.as_ref().map(|w| (&w.ws_id, &w.slug))
            || !self.workspace_service.catalog.try_lock_until(deadline).ok_or("MACHINE_SERVICE_UNAVAILABLE")?.as_ref().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?.workspaces.contains_key(&meta.workspace_id) {
            return Err("SESSION_OWNERSHIP_CHANGED".into());
        }
        let pty = self.terminal_service.get_session(&target.session_id).ok_or("SESSION_EXPIRED")?;
        if !matches!(pty.state(), PtySessionState::Starting | PtySessionState::Running) { return Err("SESSION_EXPIRED".into()); }
        let mut session = session.clone();
        (session.cols, session.rows) = pty.get_size();
        Ok(session)
    }

    pub(crate) fn acquire_machine_controller(controllers: &mut HashMap<String, MachineController>, id: &str, device: &str)
        -> Result<MachineSocketLease, String> {
        let generation = if let Some(previous) = controllers.get(id) {
            if previous.device != device && previous.reserved() { return Err("CONTROL_CONFLICT".into()); }
            previous.generation.checked_add(1).ok_or("CAPACITY_EXCEEDED")?
        } else { 1 };
        let (cancelled, receiver) = tokio::sync::watch::channel(false);
        let disconnected = Arc::new(Mutex::new(None));
        let entry = MachineController { device: device.into(), generation, cancelled, disconnected: disconnected.clone() };
        if let Some(previous) = controllers.insert(id.into(), entry) { previous.cancelled.send_replace(true); }
        Ok(MachineSocketLease { generation, cancelled: receiver, disconnected })
    }

    pub(crate) fn machine_only(&self, id: &str) -> bool {
        self.workspace_service.journal.owns_session(id)
    }

    pub(crate) fn machine_pty(&self, id: &str) -> Option<Arc<crate::terminal::PtySession>> {
        self.terminal_service.get_session(id)
    }

    pub(crate) async fn wait_machine_lifecycle(&self, id: &str) -> Result<(), String> {
        let receiver = self.machine_lifecycles.lock().get(id).cloned();
        if let Some(mut receiver) = receiver {
            tokio::time::timeout(Duration::from_secs(10), receiver.wait_for(|done| *done))
                .await.map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?.map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
        }
        Ok(())
    }

    pub(crate) async fn spawn_machine(
        &self, request: crate::remote::machine_protocol::CreateSessionRequest, device: String,
        digest: String, target: crate::remote::machine_protocol::RemoteTerminalTarget,
        deadline: Instant, check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<String, String> {
        use crate::remote::machine_protocol::Startup;
        let startup = match &request.startup {
            Startup::Shell => None,
            Startup::AgentResume { agent_type, provider_session } => {
                // Only OMO currently has an ID-based authoritative transcript/CWD resolver.
                // Other providers' argv support alone cannot prove transcript ownership.
                if agent_type != "omo" || provider_session.transcript_path.is_some() {
                    return Err("AGENT_RESUME_UNSUPPORTED".into());
                }
                Some(TerminalStartup::AgentResume { agent_type: agent_type.clone(), provider_session: provider_session.clone() })
            }
        };
        let worktree = request.worktree.as_ref().map(|w| WorktreeIdentity { ws_id: w.ws_id.clone(), slug: w.slug.clone() });
        let cache_key = serde_json::to_string(&("machine", &device, &request.request_id)).expect("request identity");
        let context = Arc::new(MachineSpawn { request: request.clone(), device: device.clone(), digest, target, deadline, check });
        let result = MACHINE_SPAWN.scope(context, self.handle_spawn(&cache_key, &request.workspace_id, worktree, None,
            request.cols, request.rows, None, startup, #[cfg(test)] None)).await;
        if result.is_err() {
            let workspaces = self.workspace_service.clone();
            let request_id = request.request_id.clone();
            let ambiguous = crate::ipc::run_blocking(move || {
                if workspaces.journal.reconcile(&device, &request_id).map_err(crate::ipc::IpcError::internal)?
                    .is_some_and(|r| matches!(r.operation, crate::remote::machine_protocol::Operation::Pending { .. })) {
                    workspaces.journal.mark_unknown(&device, &request_id).map_err(crate::ipc::IpcError::internal)?;
                    return Ok(true);
                }
                Ok(false)
            }).await.map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
            if ambiguous { return Err("OPERATION_OUTCOME_UNKNOWN".into()); }
        }
        result.map_err(|e| match e {
                SpawnError::AgentSessionConflict { .. } => "AGENT_SESSION_CONFLICT".into(),
                SpawnError::InvalidAgentResume(_) => "AGENT_RESUME_INVALID".into(),
                SpawnError::Other(code) => match code.as_str() {
                    "UNAUTHORIZED" | "TIMEOUT" | "PROJECT_NOT_FOUND" | "WORKTREE_NOT_FOUND" | "SESSION_NOT_FOUND" | "SESSION_EXPIRED" |
                    "PARENT_SESSION_MISMATCH" | "CAPACITY_EXCEEDED" | "MACHINE_SERVICE_UNAVAILABLE" |
                    "OPERATION_OUTCOME_UNKNOWN" | "REQUEST_CONFLICT" | "AGENT_RESUME_UNSUPPORTED" => code,
                    _ => "INVALID_PATH".into(),
                },
            })
    }

    pub(crate) fn machine_detail(&self, id: &str, epoch: crate::scoped_contracts::Epoch) -> Result<crate::remote::machine_protocol::SessionDetail, String> {
        use crate::remote::machine_protocol::SessionDetail;
        let mut record = self.workspace_service.journal.session(id)?.ok_or("SESSION_NOT_FOUND")?;
        if let Some(exit) = record.exit { record.session.running = false; return Ok(SessionDetail::Exited { session: record.session, exit }); }
        if record.session.target.daemon_epoch != epoch { return Ok(SessionDetail::Expired { target: record.session.target }); }
        let Some(pty) = self.terminal_service.get_session(id) else { return Ok(SessionDetail::Expired { target: record.session.target }); };
        if let PtySessionState::Exited { code } = pty.state() {
            record.session.running = false;
            return Ok(SessionDetail::Exited { session: record.session, exit: crate::remote::machine_protocol::ExitMetadata { code, signal: None } });
        }
        if matches!(pty.state(), PtySessionState::Failed { .. }) { return Ok(SessionDetail::Expired { target: record.session.target }); }
        (record.session.cols, record.session.rows) = pty.get_size();
        let (start, end) = self.terminal_service.output_hub().session_sequence_range(id).unwrap_or_default();
        record.session.start_sequence = crate::scoped_contracts::Epoch(start.unwrap_or(0));
        record.session.end_sequence = crate::scoped_contracts::Epoch(end.unwrap_or(0));
        Ok(SessionDetail::Running { session: record.session })
    }

    pub(crate) fn machine_sessions(&self, epoch: crate::scoped_contracts::Epoch) -> Result<crate::remote::machine_protocol::Sessions, String> {
        use crate::remote::machine_protocol::*;
        let mut sessions = Vec::new();
        for record in self.workspace_service.journal.sessions()? {
            let mut session = record.session;
            match self.machine_detail(&session.target.session_id, epoch)? {
                SessionDetail::Running { session: live } | SessionDetail::Exited { session: live, .. } => session = live,
                SessionDetail::Expired { .. } => session.running = false,
            }
            sessions.push(session);
        }
        Ok(Sessions { revision: self.workspace_service.journal.session_revision()?, completeness: Completeness::Complete, sessions, unavailable_workspace_ids: Vec::new() })
    }

    pub(crate) async fn close_machine(&self, device: &str, request: &str, digest: &str, id: &str,
        expected_epoch: crate::scoped_contracts::Epoch, owner_epoch: crate::scoped_contracts::Epoch,
        check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>) -> Result<(), String> {
        use crate::remote::{machine_operation_journal::Begin, machine_protocol::*};
        let _retirement = self.retain_machine_request()?;
        let _spawn = self.spawn_lock.lock().await;
        let controllers = self.machine_controllers.lock().await;
        check()?;
        let mut record = self.workspace_service.journal.session(id)?.ok_or("SESSION_NOT_FOUND")?;
        if record.session.target.daemon_epoch != expected_epoch { return Err("STALE_EPOCH".into()); }
        let controller = controllers.get(id).filter(|c| c.reserved()).map(|c| &c.device).unwrap_or(&record.creator_device);
        if controller != device { return Err("CONTROL_CONFLICT".into()); }
        if record.exit.is_none() && expected_epoch != owner_epoch { return Err("SESSION_EXPIRED".into()); }
        let services = self.workspace_service.clone();
        let device_owned = device.to_owned(); let request_owned = request.to_owned(); let digest = digest.to_owned();
        let resource = serde_json::to_string(&record.session.target).expect("target");
        let check_begin = check.clone();
        let begin = crate::ipc::run_blocking(move || {
            check_begin().map_err(crate::ipc::IpcError::internal)?;
            services.journal.begin(&device_owned, &request_owned, "closeSession", &digest, &resource).map_err(crate::ipc::IpcError::internal)
        }).await.map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        if let Begin::Existing(_) = begin { return Ok(()); }
        let pty = self.terminal_service.get_session(id);
        if record.exit.is_none() {
            if pty.is_none() { return Err("OPERATION_OUTCOME_UNKNOWN".into()); }
            check()?;
            self.terminal_service.close_machine_session(id, check.clone()).await.map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
            if !pty.as_ref().is_some_and(|p| p.is_reaped()) { return Err("OPERATION_OUTCOME_UNKNOWN".into()); }
            record.session.running = false;
            record.exit = Some(ExitMetadata { code: pty.and_then(|p| match p.state() { PtySessionState::Exited { code } => code, _ => None }), signal: None });
        }
        self.release_session_ownership(id);
        let services = self.workspace_service.clone(); let device = device.to_owned(); let request = request.to_owned();
        self.wait_machine_lifecycle(id).await?;
        crate::ipc::run_blocking(move || {
            services.journal.save_session(record).map_err(crate::ipc::IpcError::internal)?;
            services.journal.complete(&device, &request, 204, OperationOutcome::NoContent).map_err(crate::ipc::IpcError::internal)?;
            Ok(())
        }).await.map_err(|_| "OPERATION_OUTCOME_UNKNOWN".into())
    }

    pub(crate) fn retain_machine_request(&self) -> Result<Option<super::handover::RetirementGuard>, String> {
        self.handover_manager.upgrade().map(|manager| manager.retain_request(self.terminal_service.clone())).transpose()
    }

    pub(crate) fn router(&self) -> &super::proxy::SessionRouter {
        &self.session_router
    }

    pub(crate) fn project_session_metadata(
        &self,
        details: &mut crate::remote::backend::RemoteSessionDetails,
    ) {
        if let Some(meta) = self.session_metadata.read().get(&details.session_id) {
            details.workspace_id = Some(meta.workspace_id.clone());
            details.worktree_label = meta.worktree.as_ref().map(|worktree| worktree.slug.clone());
            details.worktree_path = Some(meta.cwd.clone());
        }
    }

    pub(super) async fn write_session_input(&self, id: &str, data: Vec<u8>) -> Result<(), String> {
        self.validate_session_ssh_target(id)
            .await
            .map_err(|e| e.to_string())?;
        let generation = self
            .terminal_service
            .remote()
            .details(id)
            .map(|d| d.generation)
            .unwrap_or(0);
        self.terminal_service
            .write_input_operation(id, generation, data)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn resize_session(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        self.validate_session_ssh_target(id)
            .await
            .map_err(|e| e.to_string())?;
        let generation = self
            .terminal_service
            .remote()
            .details(id)
            .map(|d| d.generation)
            .unwrap_or(0);
        self.terminal_service
            .resize_operation(id, generation, cols, rows)
            .map_err(|e| e.to_string())?
            .await
            .map_err(|e| e.to_string())
    }

    pub(super) async fn persist_remote_sessions_at(&self, path: PathBuf) -> Result<(), String> {
        let _guard = self.remote_persistence_lock.lock().await;
        let records: Vec<_> = self
            .terminal_service
            .remote()
            .list()
            .iter()
            .filter_map(|id| {
                self.terminal_service
                    .remote()
                    .details(id)
                    .map(|d| DurableRemoteSession {
                        descriptor: d.descriptor,
                        metadata: self.session_metadata.read().get(id).cloned(),
                    })
            })
            .collect();
        crate::ipc::run_blocking(move || {
            let mut session = crate::session::PersistedWorkspaceSession::default();
            session.version = 3;
            session.extra.insert(
                "remoteSessions".into(),
                serde_json::to_value(records)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?,
            );
            save_session_to_path(&path, &session)
        })
        .await
        .map_err(|e| e.to_string())
    }

    pub(super) async fn restore_remote_sessions_at(&self, path: PathBuf) -> Result<(), String> {
        let records: Vec<DurableRemoteSession> = crate::ipc::run_blocking(move || {
            let value = load_session_from_path(&path)?
                .and_then(|mut s| s.extra.remove("remoteSessions"))
                .unwrap_or_else(|| serde_json::json!([]));
            serde_json::from_value(value).map_err(|e| crate::ipc::IpcError::internal(e.to_string()))
        })
        .await
        .map_err(|e| e.to_string())?;
        for mut record in records {
            // A new daemon has no hub backlog. Replay retained remote output from zero;
            // Attach emits a reset boundary instead of treating old local sequences as cursors.
            record.descriptor.remote_cursor = crate::ssh::bridge::RemoteCursor(0);
            let id = record.descriptor.backend_session_id.clone();
            if self
                .session_router
                .find_legacy_peer_for_session(&id)
                .is_some()
            {
                continue;
            }
            self.session_router.register_workspace(
                &id,
                &record.descriptor.config.project_id,
                Some(self.ssh_store_path.clone()),
            );
            if let Some(meta) = record.metadata {
                self.session_metadata.write().insert(id.clone(), meta);
            }
            self.terminal_service
                .remote()
                .restore(record.descriptor)
                .map_err(|e| e.to_string())?;
            self.watch_remote_session(&id)?;
        }
        Ok(())
    }

    pub(super) fn watch_remote_session(&self, id: &str) -> Result<(), String> {
        let mut rx = self
            .terminal_service
            .remote()
            .subscribe(id)
            .map_err(|e| e.to_string())?;
        let tx = self.remote_event_tx.clone();
        let runtime = Arc::downgrade(self.terminal_service.remote());
        let metadata = self.session_metadata.clone();
        let path = self.remote_sessions_path.clone();
        let lock = self.remote_persistence_lock.clone();
        tokio::spawn(async move {
            loop {
                let details = rx.borrow_and_update().clone();
                {
                    let _guard = lock.lock().await;
                    let Some(runtime) = runtime.upgrade() else {
                        break;
                    };
                    let records: Vec<_> = runtime
                        .list()
                        .iter()
                        .filter_map(|id| {
                            runtime.details(id).map(|d| DurableRemoteSession {
                                descriptor: d.descriptor,
                                metadata: metadata.read().get(id).cloned(),
                            })
                        })
                        .collect();
                    drop(runtime);
                    let path = path.clone();
                    if let Err(error) = crate::ipc::run_blocking(move || {
                        let mut session = crate::session::PersistedWorkspaceSession::default();
                        session.version = 3;
                        session.extra.insert(
                            "remoteSessions".into(),
                            serde_json::to_value(records)
                                .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?,
                        );
                        save_session_to_path(&path, &session)
                    })
                    .await
                    {
                        tracing::error!(%error, "Remote session checkpoint persistence failed");
                    }
                }
                let _ = tx.send(DaemonRemoteEvent { event: "terminal_remote_status".into(), payload: serde_json::json!({"sessionId":details.descriptor.backend_session_id,"state":details.state,"generation":details.generation,"failure":details.failure,"replayGap":details.replay_gap}) });
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    fn remote_spawn_relative_path(repo_root: &str, root: &str) -> String {
        let norm_repo = repo_root.replace('\\', "/");
        let norm_root = root.replace('\\', "/");

        let repo_parts: Vec<&str> = norm_repo.split('/').filter(|s| !s.is_empty()).collect();
        let root_parts: Vec<&str> = norm_root.split('/').filter(|s| !s.is_empty()).collect();

        if root_parts.len() < repo_parts.len() {
            return String::new();
        }

        let is_windows = (norm_repo.len() >= 2 && norm_repo.as_bytes()[1] == b':')
            || norm_repo.starts_with("//")
            || (norm_root.len() >= 2 && norm_root.as_bytes()[1] == b':')
            || norm_root.starts_with("//");

        for (repo_part, root_part) in repo_parts.iter().zip(root_parts.iter()) {
            if is_windows {
                if repo_part.to_lowercase() != root_part.to_lowercase() {
                    return String::new();
                }
            } else if repo_part != root_part {
                return String::new();
            }
        }

        root_parts[repo_parts.len()..].join("/")
    }

    pub(super) async fn spawn_remote(
        &self,
        project: crate::ssh::projects::RemoteProject,
        host: crate::ssh::SshHost,
        request: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        fingerprint: SpawnRequestFingerprint,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        let environment = crate::ssh::runtime::detect(&host)
            .await
            .map_err(|e| e.to_string())?;
        if project
            .platform
            .unwrap_or(crate::ssh::runtime::RemotePlatform::Posix)
            != environment.platform
        {
            return Err(SpawnError::Other(
                "Remote platform changed; register the project again".into(),
            ));
        }
        let root = crate::ssh::worktree::resolve_remote_spawn_root(
            environment.platform,
            &project.repo_root,
            worktree.as_ref(),
            cwd.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        #[cfg(test)]
        let environment = {
            let mut environment = environment;
            if let Some(home) = &helper_home {
                environment.home = home.clone();
            }
            environment
        };
        let helper = crate::ssh::helper_setup::default_location(&host, &environment)
            .map_err(|e| e.to_string())?;
        let relative = Self::remote_spawn_relative_path(&project.repo_root, &root);
        let config = crate::terminal::remote::RemoteSessionConfig {
            host,
            environment,
            helper,
            project_id: project.workspace_id.clone(),
            project_path: project.repo_root,
            worktree: if relative.is_empty() {
                None
            } else {
                Some(relative)
            },
            agent_identity: None,
        };
        // Persist the immutable request before any potentially ambiguous remote spawn.
        use sha2::{Digest, Sha256};
        let request_key = format!("{:x}", Sha256::digest(request.as_bytes()));
        let request_path = self
            .remote_sessions_path
            .with_file_name(format!("remote-request-{request_key}.json"));
        let request_value = serde_json::json!({"clientRequestId":request,"config":config,"fingerprint":fingerprint});
        crate::ipc::run_blocking(move || {
            if let Some(previous) = load_session_from_path(&request_path)? {
                if previous.extra.get("request") != Some(&request_value) {
                    return Err(crate::ipc::IpcError::internal("clientRequestId was reused with a different remote spawn request"));
                }
                // Uncertain requests require recovery of their original target, not a new shell.
                return Err(crate::ipc::IpcError::internal("Remote spawn request is pending recovery; refusing to create a replacement target"));
            }
            let mut session = crate::session::PersistedWorkspaceSession::default(); session.version = 3;
            session.extra.insert("request".into(), request_value);
            save_session_to_path(&request_path, &session)
        }).await.map_err(|e| e.to_string())?;
        let descriptor = self
            .terminal_service
            .remote()
            .create(
                config,
                crate::ssh::bridge::SpawnParams {
                    cols: Some(cols),
                    rows: Some(rows),
                    ..Default::default()
                },
                request.into(),
            )
            .await
            .map_err(|e| e.to_string())?;
        let id = descriptor.backend_session_id.clone();
        self.session_metadata.write().insert(
            id.clone(),
            StoredSessionMeta {
                client_request_id: request.into(),
                machine_session: None,
                workspace_id: project.workspace_id.clone(),
                worktree,
                cwd: PathBuf::from(root),
                provider_claim: None,
                spawn_fingerprint: fingerprint,
            },
        );
        self.session_router.register_workspace(
            &id,
            &project.workspace_id,
            Some(self.ssh_store_path.clone()),
        );
        self.persist_remote_sessions_at(self.remote_sessions_path.clone())
            .await?;
        self.watch_remote_session(&id)?;
        Ok(id)
    }

    /// Revokes a workspace binding and terminates every live session the
    /// daemon owns for it, so remote clients cannot keep using already-spawned
    /// PTYs of a workspace the user removed. Idempotent: unregistering an
    /// unknown workspace (e.g. after a daemon restart) succeeds as a no-op.
    pub async fn handle_unregister_workspace(&self, workspace_id: &str) -> Result<(), String> {
        let _spawn_guard = Arc::clone(&self.spawn_lock).lock_owned().await;
        // SSH owns a separate inventory, but still needs the session cleanup
        // below. Never send remote identities through the local catalog gate.
        if !crate::ssh::projects::is_remote(workspace_id) {
            let workspaces = Arc::clone(&self.workspace_service);
            let workspace = workspace_id.to_string();
            crate::ipc::run_blocking(move || {
                workspaces.unregister(&workspace).map_err(crate::ipc::IpcError::internal)
            }).await.map_err(|error| error.to_string())?;
        }

        let owned_sessions: Vec<String> = self
            .session_metadata
            .read()
            .iter()
            .filter(|(_, meta)| meta.workspace_id == workspace_id)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in owned_sessions {
            if self.session_router.is_local_session(&session_id) {
                self.handle_close(&session_id)
                    .await
                    .map_err(|e| e.to_string())?;
            } else if let Some(peer) = self
                .session_router
                .find_legacy_peer_for_session(&session_id)
            {
                peer.close(&session_id).await.map_err(|message| {
                    format!("failed to close peer session '{session_id}': {message}")
                })?;
            }
            self.release_session_ownership(&session_id);
        }
        Ok(())
    }

    pub(super) async fn handle_spawn(
        &self,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        let machine = MACHINE_SPAWN.try_with(Arc::clone).ok();
        if let Some(machine) = &machine { (machine.check)()?; }
        if self
            .handover_manager
            .upgrade()
            .is_none_or(|manager| manager.is_draining())
        {
            return Err(SpawnError::Other(
                "Daemon is in draining mode and does not accept new sessions".into(),
            ));
        }

        if client_request_id.trim().is_empty() {
            return Err(SpawnError::Other("clientRequestId cannot be empty".into()));
        }

        let remote = match (
            crate::ssh::projects::is_remote(workspace_id),
            startup.as_ref(),
        ) {
            (true, Some(TerminalStartup::RemoteSsh { host_store_path })) => {
                if shell.is_some() {
                    return Err(SpawnError::Other(
                        "Local shell overrides are unsupported for SSH sessions".into(),
                    ));
                }
                if host_store_path != &self.ssh_store_path {
                    return Err(SpawnError::Other(
                        "SSH inventory path is not daemon-configured".into(),
                    ));
                }
                let path = self.ssh_store_path.clone();
                let id = workspace_id.to_string();
                Some(
                    crate::ipc::run_blocking(move || crate::ssh::projects::resolve(&path, &id))
                        .await
                        .map_err(|e| SpawnError::Other(e.to_string()))?,
                )
            }
            (true, _) | (false, Some(TerminalStartup::RemoteSsh { .. })) => {
                return Err(SpawnError::Other(
                    "SSH workspace requires stored SSH routing; local fallback is forbidden".into(),
                ));
            }
            (false, _) => None,
        };
        #[cfg(test)]
        if machine.is_some() {
            let probe = self.workspace_service.transaction_probe.read().clone();
            if let Some(probe) = probe { probe("sessionBeforeSpawnGate"); }
        }
        let _spawn_guard = if let Some(machine) = &machine {
            tokio::time::timeout_at(tokio::time::Instant::from_std(machine.deadline), Arc::clone(&self.spawn_lock).lock_owned())
                .await.map_err(|_| SpawnError::Other("TIMEOUT".into()))?
        } else { Arc::clone(&self.spawn_lock).lock_owned().await };

        let (_spawn_guard, previous) = if let Some(machine) = &machine {
            (machine.check)()?;
            let context = machine.clone();
            let workspaces = self.workspace_service.clone();
            crate::ipc::run_blocking(move || {
                // The worker owns the spawn gate until the journal wait really drains.
                let record = workspaces.journal.reconcile(&context.device, &context.request.request_id);
                Ok((_spawn_guard, record))
            }).await.map_err(|_| SpawnError::Other("MACHINE_SERVICE_UNAVAILABLE".into()))?
        } else { (_spawn_guard, Ok(None)) };
        let now = Instant::now();
        if let Some(machine) = &machine {
            (machine.check)()?;
            if let Some(record) = previous? {
                if record.digest != machine.digest || record.kind != "createSession" { return Err("REQUEST_CONFLICT".to_string().into()); }
                return match record.operation {
                    crate::remote::machine_protocol::Operation::Completed { outcome: crate::remote::machine_protocol::OperationOutcome::Session { session }, .. } => Ok(session.target.session_id),
                    _ => Err("OPERATION_OUTCOME_UNKNOWN".to_string().into()),
                };
            }
        }
        let provider_claim = ProviderSessionClaimKey::from_startup(startup.as_ref());
        let spawn_fingerprint = SpawnRequestFingerprint {
            workspace_id: workspace_id.to_string(),
            worktree: worktree.clone(),
            cwd: cwd.clone(),
            cols,
            rows,
            shell: shell.clone(),
            provider_claim: provider_claim.clone(),
            startup: startup.clone(),
        };
        self.prune_dead_spawn_ownership(now);
        {
            let mut cache = self.spawn_idempotency_cache.lock();
            if let Some(entry) = cache.get_mut(client_request_id) {
                if entry.fingerprint != spawn_fingerprint {
                    return Err(SpawnError::Other(format!(
                        "clientRequestId '{client_request_id}' was reused with a different spawn request"
                    )));
                }
                entry.created_at = now;
                return Ok(entry.session_id.clone());
            }
        }

        let existing_request = self
            .session_metadata
            .read()
            .iter()
            .find(|(session_id, meta)| {
                meta.client_request_id == client_request_id && self.session_is_live(session_id)
            })
            .map(|(session_id, meta)| (session_id.clone(), meta.spawn_fingerprint.clone()));
        if let Some((live_session_id, existing_fingerprint)) = existing_request {
            if existing_fingerprint != spawn_fingerprint {
                return Err(SpawnError::Other(format!(
                    "clientRequestId '{client_request_id}' was reused with a different spawn request"
                )));
            }
            self.spawn_idempotency_cache.lock().insert(
                client_request_id.to_string(),
                SpawnCacheEntry {
                    session_id: live_session_id.clone(),
                    created_at: now,
                    fingerprint: spawn_fingerprint.clone(),
                },
            );
            return Ok(live_session_id);
        }

        if let Some((project, host)) = remote {
            return self
                .spawn_remote(
                    project,
                    host,
                    client_request_id,
                    worktree,
                    cwd,
                    cols,
                    rows,
                    spawn_fingerprint,
                    #[cfg(test)]
                    helper_home,
                )
                .await;
        }
        if let Some(machine) = &machine { (machine.check)()?; }
        let resume_startup = startup.clone();
        let machine_resume = machine.is_some();
        let resume_cwd = crate::ipc::run_blocking(move || {
            if machine_resume { return Ok(None); }
            crate::terminal::resume_cwd::resolve_agent_resume_cwd(resume_startup.as_ref()).map_err(
                |error| {
                    crate::ipc::IpcError::new(
                        crate::ipc::IpcErrorCode::AgentResumeInvalid,
                        error.to_string(),
                    )
                },
            )
        })
        .await
        .map_err(|error| SpawnError::InvalidAgentResume(error.to_string()))?;
        let workspace_service = Arc::clone(&self.workspace_service);
        let terminal_service = Arc::clone(&self.terminal_service);
        let claims = Arc::clone(&self.provider_session_claims);
        let workspace_id_owned = workspace_id.to_string();
        let spawn_worktree = worktree.clone();
        let spawn_startup = startup.clone();
        let spawn_claim = provider_claim.clone();
        let session_router = Arc::clone(&self.session_router);
        let spawn_idempotency_cache = Arc::clone(&self.spawn_idempotency_cache);
        let session_metadata = Arc::clone(&self.session_metadata);
        let provider_session_claims = Arc::clone(&self.provider_session_claims);
        let agent_states = Arc::clone(&self.agent_states);
        let handover_manager = self.handover_manager.clone();
        let machine_lifecycles = self.machine_lifecycles.clone();
        let client_request_id = client_request_id.to_string();
        crate::ipc::run_blocking(move || {
            // Cancellation cannot release admission before PTY ownership is published.
            let _spawn_guard = _spawn_guard;
            let workspace_gate = workspace_service.worktree_gate(&workspace_id_owned);
            let deadline = machine.as_ref().map(|m| m.deadline);
            let _workspace_gate = match deadline {
                Some(deadline) => workspace_gate.try_lock_until(deadline).ok_or_else(|| crate::ipc::IpcError::internal("TIMEOUT"))?,
                None => workspace_gate.lock(),
            };
            let _gate = match deadline {
                Some(deadline) => workspace_service.mutation_gate.try_lock_until(deadline).ok_or_else(|| crate::ipc::IpcError::internal("TIMEOUT"))?,
                None => workspace_service.mutation_gate.lock(),
            };
            let result = (|| -> Result<_, SpawnError> {
                if let Some(machine) = &machine { (machine.check)()?; }
                // Machine roots stay out of the shared mirror registry.
                let (mgr, default_cwd) = if machine.is_some() {
                    let manager = workspace_service.worktree_manager(&workspace_id_owned, false)?;
                    let private_registry = crate::worktree::WorkspaceRegistry::new();
                    private_registry.publish(workspace_id_owned.clone(), manager);
                    private_registry.resolve_terminal_target(&workspace_id_owned, spawn_worktree.as_ref())
                } else { workspace_service.registry.resolve_terminal_target(&workspace_id_owned, spawn_worktree.as_ref()) }
                    .map_err(|e| {
                        if machine.is_some() && matches!(e, crate::worktree::WorktreeError::WorktreeIdentityNotFound { .. }) {
                            SpawnError::Other("WORKTREE_NOT_FOUND".into())
                        } else { SpawnError::Other(e.to_string()) }
                    })?;

                let cwd = if let Some(machine) = &machine {
                    if let Some(parent) = &machine.request.inherit_from_session_id {
                        let record = workspace_service.journal.session(parent)?.ok_or("SESSION_NOT_FOUND".to_string())?;
                        if record.session.target.daemon_epoch != machine.target.daemon_epoch
                            || record.session.target.machine_id != machine.target.machine_id
                            || record.session.workspace_id != workspace_id_owned
                            || record.session.worktree != machine.request.worktree {
                            return Err("PARENT_SESSION_MISMATCH".to_string().into());
                        }
                        let parent = terminal_service.get_session(parent).ok_or("SESSION_EXPIRED".to_string())?;
                        if !matches!(parent.state(), PtySessionState::Running) { return Err("SESSION_EXPIRED".to_string().into()); }
                        let pid = parent.pid().ok_or("SESSION_EXPIRED".to_string())?;
                        Some(crate::ipc::terminal::process_cwd(pid).ok_or("CWD_UNAVAILABLE".to_string())?.to_str().ok_or("INVALID_PATH".to_string())?.to_owned())
                    } else {
                        machine.request.cwd_relative.as_ref().map(|relative| {
                            let rel_norm = relative.replace('\\', "/");
                            if let Some(ref w) = spawn_worktree {
                                let expected_wt_rel = format!(".orca-worktrees/wt-{}", w.slug);
                                if rel_norm == expected_wt_rel {
                                    default_cwd.to_string_lossy().into_owned()
                                } else if let Some(sub) = rel_norm.strip_prefix(&format!("{}/", expected_wt_rel)) {
                                    default_cwd.join(sub).to_string_lossy().into_owned()
                                } else if default_cwd.to_string_lossy().replace('\\', "/").ends_with(&rel_norm) {
                                    default_cwd.to_string_lossy().into_owned()
                                } else {
                                    default_cwd.join(relative).to_string_lossy().into_owned()
                                }
                            } else if default_cwd.to_string_lossy().replace('\\', "/").ends_with(&rel_norm) {
                                default_cwd.to_string_lossy().into_owned()
                            } else {
                                default_cwd.join(relative).to_string_lossy().into_owned()
                            }
                        })
                    }
                } else { cwd };
                let resume_cwd = if machine.is_some() {
                    if let Some(TerminalStartup::AgentResume { agent_type, provider_session }) = &spawn_startup {
                        let plan = crate::terminal::shell::resolve_agent_resume_plan(agent_type, provider_session)
                            .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        if crate::ipc::agents::resolve_binary(&plan.program, &crate::ipc::agents::search_paths()).is_none() {
                            return Err("AGENT_RESUME_UNSUPPORTED".to_string().into());
                        }
                    }
                    crate::terminal::resume_cwd::resolve_agent_resume_cwd(spawn_startup.as_ref())
                        .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?
                } else { resume_cwd };
                if machine.is_some() {
                    if let Some(resume) = &resume_cwd {
                        let requested = cwd.as_ref().map(PathBuf::from).unwrap_or_else(|| default_cwd.clone());
                        let requested = fs::canonicalize(requested).map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        let resume = fs::canonicalize(resume).map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        if resume != requested { return Err(SpawnError::InvalidAgentResume("Provider CWD does not match the selected target".into())); }
                    }
                }
                let cwd = resume_cwd
                    .map(|path| path.to_string_lossy().into_owned())
                    .or(cwd);
                let resolved_cwd = if let Some(ref custom_cwd_str) = cwd {
                    let custom_path = PathBuf::from(custom_cwd_str);
                    if !custom_path.exists() {
                        return Err(SpawnError::Other(format!(
                            "CWD does not exist: {custom_cwd_str}"
                        )));
                    }
                    if !custom_path.is_dir() {
                        return Err(SpawnError::Other(format!(
                            "CWD is not a directory: {custom_cwd_str}"
                        )));
                    }
                    let canonical = fs::canonicalize(&custom_path).map_err(|e| {
                        SpawnError::Other(format!("Cannot canonicalize CWD {custom_cwd_str}: {e}"))
                    })?;
                    let allowed = mgr.canonical_allowed_path(&canonical).map_err(|e| {
                        SpawnError::Other(format!(
                            "CWD '{custom_cwd_str}' is outside workspace: {e}"
                        ))
                    })?;
                    if allowed != default_cwd && !allowed.starts_with(&default_cwd) {
                        return Err(SpawnError::Other(format!(
                    "CWD '{custom_cwd_str}' is outside the resolved workspace/worktree root '{}'",
                    default_cwd.display()
                )));
                    }
                    allowed
                } else {
                    default_cwd
                };

                let mut cmd = match crate::terminal::shell::resolve_startup_command(
                    shell.as_deref(),
                    spawn_startup.as_ref(),
                ) {
                    Ok(cmd) => cmd,
                    Err(err) => return Err(SpawnError::InvalidAgentResume(err.to_string())),
                };
                if let Some(claim) = spawn_claim.as_ref() {
                    if let Some(existing_session_id) = claims.lock().get(claim) {
                        return Err(SpawnError::AgentSessionConflict {
                            agent_type: claim.agent_type.clone(),
                            provider_key: claim.provider_key,
                            provider_id: claim.provider_id.clone(),
                            existing_session_id: existing_session_id.clone(),
                        });
                    }
                }
                cmd.env("PROMPT_EOL_MARK", "");
                cmd.cwd(normalize_process_cwd(&resolved_cwd));

                let raw_id = if let Some(machine) = &machine {
                    (machine.check)()?;
                    if terminal_service.list_sessions().iter().filter(|id| workspace_service.journal.owns_session(id)).count() >= 64 {
                        return Err("CAPACITY_EXCEEDED".to_string().into());
                    }
                    match workspace_service.journal.begin(&machine.device, &machine.request.request_id, "createSession", &machine.digest, &serde_json::to_string(&machine.target).expect("target"))? {
                        crate::remote::machine_operation_journal::Begin::New => {},
                        crate::remote::machine_operation_journal::Begin::Existing(_) => return Err("OPERATION_OUTCOME_UNKNOWN".to_string().into()),
                    }
                    machine.target.session_id.clone()
                } else { uuid::Uuid::new_v4().to_string() };
                #[cfg(test)]
                if machine.is_some() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe { probe("sessionIntent"); }
                }
                let (session_id, mut lifecycle_rx) = terminal_service
                    .spawn_in_worktree_with_id(raw_id, cmd, cols, rows, &mgr, &resolved_cwd)
                    .map_err(|e| SpawnError::Other(e.to_string()))?;
                #[cfg(test)]
                if machine.is_some() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe { probe("sessionSpawned"); }
                }
                let (start_sequence, end_sequence) = terminal_service.output_hub().session_sequence_range(&session_id).unwrap_or_default();
                let durable = machine.as_ref().map(|machine| crate::remote::machine_operation_journal::MachineSession {
                    creator_device: machine.device.clone(),
                    session: crate::remote::machine_protocol::Session {
                        target: machine.target.clone(), workspace_id: workspace_id_owned.clone(),
                        worktree: machine.request.worktree.clone(), cwd: resolved_cwd.to_string_lossy().into_owned(),
                        cols, rows, running: true, title: None,
                        agent_type: match &machine.request.startup { crate::remote::machine_protocol::Startup::Shell => None, crate::remote::machine_protocol::Startup::AgentResume { agent_type, .. } => Some(agent_type.clone()) },
                        provider_session: match &machine.request.startup { crate::remote::machine_protocol::Startup::Shell => None, crate::remote::machine_protocol::Startup::AgentResume { provider_session, .. } => Some(provider_session.clone()) },
                        start_sequence: crate::scoped_contracts::Epoch(start_sequence.unwrap_or(0)), end_sequence: crate::scoped_contracts::Epoch(end_sequence.unwrap_or(0)),
                    }, exit: None,
                });
                // Store idempotency entry and session metadata before releasing the request lock.
                let ssh_store = match startup.as_ref() {
                    Some(TerminalStartup::RemoteSsh { host_store_path }) => {
                        Some(host_store_path.clone())
                    }
                    _ => None,
                };
                session_router.register_workspace(&session_id, &workspace_id_owned, ssh_store);
                spawn_idempotency_cache.lock().insert(
                    client_request_id.to_string(),
                    SpawnCacheEntry {
                        session_id: session_id.clone(),
                        created_at: now,
                        fingerprint: spawn_fingerprint.clone(),
                    },
                );
                session_metadata.write().insert(
                    session_id.clone(),
                    StoredSessionMeta {
                        client_request_id: client_request_id.to_string(),
                        machine_session: durable.as_ref().map(|r| r.session.clone()),
                        workspace_id: workspace_id_owned.clone(),
                        worktree,
                        cwd: resolved_cwd,
                        provider_claim: provider_claim.clone(),
                        spawn_fingerprint,
                    },
                );
                if let Some(claim) = provider_claim {
                    provider_session_claims
                        .lock()
                        .insert(claim, session_id.clone());
                }

                let cleanup_session_id = session_id.clone();
                let cleanup_router = Arc::clone(&session_router);
                let cleanup_cache = Arc::clone(&spawn_idempotency_cache);
                let cleanup_metadata = Arc::clone(&session_metadata);
                let cleanup_claims = Arc::clone(&provider_session_claims);
                let cleanup_agent_states = Arc::clone(&agent_states);
                let handover_manager = handover_manager.clone();
                let terminal_service = Arc::clone(&terminal_service);
                let exited_pty = terminal_service.get_session(&session_id);
                let lifecycle_workspaces = workspace_service.clone();
                let durable_exit = durable.clone();
                let metadata_target = durable.as_ref().map(|record| record.session.target.clone());
                let lifecycle_done = if machine.is_some() {
                    let (done, receiver) = tokio::sync::watch::channel(false);
                    machine_lifecycles.lock().insert(session_id.clone(), receiver);
                    Some(done)
                } else { None };
                // Persist before starting exit publication, so a fast exit cannot be overwritten
                // by the initial running record. Even on failure, install lifecycle cleanup.
                let persisted = match (&machine, durable) {
                    (Some(machine), Some(record)) => {
                        let result = workspace_service.journal.commit_spawn(&machine.device, &machine.request.request_id, record.clone());
                        if result.is_ok() {
                            workspace_service.machine_events.publish("sessionStarted", Some(&record.session.workspace_id), Some(&record.session.target.session_id), serde_json::json!(record.session));
                        }
                        result
                    },
                    _ => Ok(()),
                };
                #[cfg(test)]
                if machine.is_some() && persisted.is_ok() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe { probe("sessionCommitted"); }
                }
                let metadata_task = if persisted.is_ok() {
                    metadata_target.map(|target| session_metadata_events::MetadataOwner {
                        workspaces: workspace_service.clone(), terminals: terminal_service.clone(), metadata: session_metadata.clone(),
                    }.subscribe(target)).transpose()
                } else { Ok(None) };
                tokio::spawn(async move {
                    loop {
                        match lifecycle_rx.recv().await {
                            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    match metadata_task {
                        Ok(Some(task)) => if let Err(error) = task.await { tracing::warn!(%error, "Machine metadata task failed"); },
                        Ok(None) => {},
                        Err(error) => tracing::warn!(%error, "Machine metadata subscription failed"),
                    }
                    if let Some(mut record) = durable_exit {
                        record.session.running = false;
                        record.exit = Some(crate::remote::machine_protocol::ExitMetadata {
                            code: exited_pty.as_ref().and_then(|p| match p.state() { PtySessionState::Exited { code } => code, _ => None }), signal: None,
                        });
                        if let Err(error) = crate::ipc::run_blocking(move || {
                            lifecycle_workspaces.journal.save_session(record.clone()).map_err(crate::ipc::IpcError::internal)?;
                            let record = lifecycle_workspaces.journal.session(&record.session.target.session_id).map_err(crate::ipc::IpcError::internal)?.ok_or_else(|| crate::ipc::IpcError::internal("SESSION_NOT_FOUND"))?;
                            lifecycle_workspaces.machine_events.publish("sessionExited", Some(&record.session.workspace_id), Some(&record.session.target.session_id), serde_json::json!({"session":record.session,"exit":record.exit}));
                            Ok(())
                        }).await {
                            tracing::error!(%error, "Machine exit persistence failed");
                        }
                    }
                    cleanup_router.remove_workspace(&cleanup_session_id);
                    cleanup_agent_states.remove(&cleanup_session_id);
                    cleanup_cache
                        .lock()
                        .retain(|_, entry| entry.session_id != cleanup_session_id);
                    if let Some(meta) = cleanup_metadata.write().remove(&cleanup_session_id) {
                        if let Some(claim) = meta.provider_claim {
                            cleanup_claims
                                .lock()
                                .retain(|key, owner| key != &claim || owner != &cleanup_session_id);
                        }
                    }
                    if let Some(manager) = handover_manager.upgrade() {
                        manager.check_retirement_if_empty(&terminal_service);
                    }
                    if let Some(done) = lifecycle_done {
                        done.send_replace(true);
                        machine_lifecycles.lock().remove(&cleanup_session_id);
                    }
                });

                persisted?;
                Ok(session_id)
            })();
            Ok(result)
        })
        .await
        .map_err(|error| SpawnError::Other(error.to_string()))?
    }

    pub(super) async fn validate_session_ssh_target(
        &self,
        session_id: &str,
    ) -> Result<(), crate::ipc::IpcError> {
        let meta = self.session_metadata.read().get(session_id).cloned();
        if let Some(meta) = meta {
            if crate::ssh::projects::is_remote(&meta.workspace_id) {
                let path = self.ssh_store_path.clone();
                crate::ipc::run_blocking(move || {
                    crate::ssh::projects::resolve(&path, &meta.workspace_id)
                })
                .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn handle_close(
        &self,
        session_id: &str,
    ) -> Result<(), crate::terminal::PtyError> {
        let remote = self.terminal_service.remote().contains(session_id);
        self.terminal_service.close_session(session_id).await?;
        self.release_session_ownership(session_id);
        self.agent_states.remove(session_id);
        if remote {
            self.persist_remote_sessions_at(self.remote_sessions_path.clone())
                .await
                .map_err(crate::terminal::PtyError::Other)?;
        }
        Ok(())
    }

    pub(super) async fn handle_hibernate(
        &self,
        session_id: &str,
    ) -> Result<(), crate::terminal::PtyError> {
        self.terminal_service.hibernate_session(session_id).await?;
        self.release_session_ownership(session_id);
        self.agent_states.remove(session_id);
        Ok(())
    }

    pub(super) fn session_is_live(&self, session_id: &str) -> bool {
        if self.terminal_service.remote().contains(session_id) {
            return true;
        }
        self.terminal_service
            .get_session(session_id)
            .is_some_and(|session| {
                matches!(
                    session.state(),
                    PtySessionState::Starting | PtySessionState::Running
                )
            })
    }

    pub(super) fn release_session_ownership(&self, session_id: &str) {
        self.session_router.remove_workspace(session_id);
        self.spawn_idempotency_cache
            .lock()
            .retain(|_, entry| entry.session_id != session_id);
        if let Some(meta) = self.session_metadata.write().remove(session_id) {
            if let Some(claim) = meta.provider_claim {
                self.provider_session_claims
                    .lock()
                    .retain(|key, owner| key != &claim || owner != session_id);
            }
        }
    }

    pub(super) fn prune_dead_spawn_ownership(&self, now: Instant) {
        let dead_sessions: Vec<String> = self
            .session_metadata
            .read()
            .keys()
            .filter(|session_id| !self.session_is_live(session_id))
            .cloned()
            .collect();
        for session_id in dead_sessions {
            self.release_session_ownership(&session_id);
        }
        self.spawn_idempotency_cache.lock().retain(|_, entry| {
            now.duration_since(entry.created_at) <= SPAWN_REQUEST_TTL
                && self.session_is_live(&entry.session_id)
        });
        self.provider_session_claims
            .lock()
            .retain(|_, session_id| self.session_is_live(session_id));
    }

    #[cfg(test)]
    pub(super) fn expire_spawn_request_for_test(&self, client_request_id: &str) {
        if let Some(entry) = self
            .spawn_idempotency_cache
            .lock()
            .get_mut(client_request_id)
        {
            entry.created_at = Instant::now() - SPAWN_REQUEST_TTL - Duration::from_secs(1);
        }
    }

    #[cfg(test)]
    pub(super) fn spawn_cache_len_for_test(&self) -> usize {
        self.spawn_idempotency_cache.lock().len()
    }

    #[cfg(test)]
    pub(super) fn provider_claim_len_for_test(&self) -> usize {
        self.provider_session_claims.lock().len()
    }

    #[cfg(test)]
    pub(super) fn reserve_provider_claim_for_test(
        &self,
        client_request_id: &str,
        session_id: &str,
        startup: &TerminalStartup,
    ) -> Result<String, SpawnError> {
        let claim = ProviderSessionClaimKey::from_startup(Some(startup))
            .expect("agent resume startup has a provider claim");
        let mut claims = self.provider_session_claims.lock();
        if let Some(existing_session_id) = claims.get(&claim) {
            return Err(SpawnError::AgentSessionConflict {
                agent_type: claim.agent_type,
                provider_key: claim.provider_key,
                provider_id: claim.provider_id,
                existing_session_id: existing_session_id.clone(),
            });
        }
        claims.insert(claim.clone(), session_id.to_string());
        self.session_metadata.write().insert(
            session_id.to_string(),
            StoredSessionMeta {
                client_request_id: client_request_id.to_string(),
                machine_session: None,
                workspace_id: "test".to_string(),
                worktree: None,
                cwd: PathBuf::from("/test"),
                provider_claim: Some(claim),
                spawn_fingerprint: SpawnRequestFingerprint {
                    workspace_id: "test".to_string(),
                    worktree: None,
                    cwd: None,
                    cols: 80,
                    rows: 24,
                    shell: None,
                    provider_claim: ProviderSessionClaimKey::from_startup(Some(startup)),
                    startup: Some(startup.clone()),
                },
            },
        );
        Ok(session_id.to_string())
    }

    pub(super) fn handle_describe_session(&self, session_id: &str) -> DaemonResponse {
        if let Some(details) = self.terminal_service.remote().details(session_id) {
            let d = details.descriptor;
            let meta = self.session_metadata.read().get(session_id).cloned();
            let (start_sequence, end_sequence) = self
                .terminal_service
                .output_hub()
                .session_sequence_range(session_id)
                .unwrap_or((None, None));
            return DaemonResponse::DescribeSessionOk {
                session: DaemonSessionDetails {
                    session_id: session_id.into(),
                    workspace_id: Some(d.config.project_id),
                    worktree: meta.as_ref().and_then(|m| m.worktree.clone()),
                    cwd: Some(
                        meta.map(|m| m.cwd.to_string_lossy().into_owned())
                            .unwrap_or(d.config.project_path),
                    ),
                    cols: d.cols,
                    rows: d.rows,
                    running: details.state
                        == crate::terminal::remote::RemoteConnectionState::Connected,
                    start_sequence,
                    end_sequence,
                },
            };
        }
        if super::super::terminal::paired_runtime::Runtime::owns(session_id) {
            if self.terminal_service.paired().contains(session_id) {
                let (start_sequence, end_sequence) = self
                    .terminal_service
                    .output_hub()
                    .session_sequence_range(session_id)
                    .unwrap_or((None, None));
                return DaemonResponse::DescribeSessionOk {
                    session: DaemonSessionDetails {
                        session_id: session_id.into(),
                        workspace_id: None,
                        worktree: None,
                        cwd: None,
                        cols: 80,
                        rows: 24,
                        running: true,
                        start_sequence,
                        end_sequence,
                    },
                };
            }
        }
        let Some(pty_session) = self.terminal_service.get_session(session_id) else {
            return DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
            };
        };

        let (cols, rows) = pty_session.get_size();
        let running = matches!(
            pty_session.state(),
            PtySessionState::Starting | PtySessionState::Running
        );
        let (start_sequence, end_sequence) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(session_id)
            .unwrap_or((None, None));

        let meta = self.session_metadata.read().get(session_id).cloned();
        let (workspace_id, worktree, cwd) = match meta {
            Some(m) => (
                Some(m.workspace_id),
                m.worktree,
                Some(m.cwd.to_string_lossy().to_string()),
            ),
            None => (
                None,
                None,
                pty_session
                    .worktree_path()
                    .map(|p| p.to_string_lossy().to_string()),
            ),
        };

        DaemonResponse::DescribeSessionOk {
            session: DaemonSessionDetails {
                session_id: session_id.to_string(),
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                running,
                start_sequence,
                end_sequence,
            },
        }
    }
}
