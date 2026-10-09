use super::workspace_service::DaemonWorkspaceService;
use crate::daemon::agent_state::AgentStateHub;
use crate::daemon::protocol::{
    AgentProviderSessionKey, DaemonRemoteEvent, DaemonResponse, DaemonSessionDetails,
    TerminalStartup, LocalSplitEnvelope, SplitOperationResult, SplitUnknownReason,
    SplitOwnership, SplitNoChild, LOCAL_SPLIT_VALIDITY_MS,
};
use crate::session::{load_session_from_path, save_session_to_path};
use crate::terminal::{PtySessionState, TerminalService};
use crate::worktree::WorktreeIdentity;
use parking_lot::{Mutex, RwLock};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Weak, atomic::{AtomicBool, Ordering}},
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
#[path = "session_metadata_forward.rs"]
mod session_metadata_forward;
#[path = "session_metadata_provider.rs"]
mod session_metadata_provider;

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

pub(super) struct SpawnOperation {
    fingerprint: Option<SpawnRequestFingerprint>,
    machine_digest: Option<String>,
    envelope: Option<LocalSplitEnvelope>,
    deadline: Option<Instant>,
    cancelled: Arc<AtomicBool>,
    cancellation: Arc<tokio::sync::Notify>,
    result: tokio::sync::watch::Sender<Option<Result<String, SpawnError>>>,
    state: SplitOperationResult,
    child: Option<String>,
    machine_target: Option<String>,
    cleanup_started: bool,
    changed: tokio::sync::watch::Sender<u64>,
    workspace_guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

struct SpawnReservation {
    id: String,
    claim: Option<ProviderSessionClaimKey>,
    live: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    pending: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    capacity: Arc<Mutex<HashSet<String>>>,
    retain_capacity: bool,
    published: bool,
}

impl SpawnReservation {
    fn reserve(id: String, claim: Option<ProviderSessionClaimKey>, machine: bool,
        live: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
        pending: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
        capacity: Arc<Mutex<HashSet<String>>>, limit: usize) -> Result<Self, SpawnError> {
        let reservation = Self { id, claim, live: live.clone(), pending, capacity, retain_capacity: false, published: false };
        if let Some(claim) = &reservation.claim {
            let mut claims = live.lock();
            if let Some(existing) = claims.get(claim) {
                return Err(SpawnError::AgentSessionConflict { agent_type: claim.agent_type.clone(),
                    provider_key: claim.provider_key, provider_id: claim.provider_id.clone(), existing_session_id: existing.clone() });
            }
            reservation.pending.lock().insert(claim.clone(), reservation.id.clone());
            claims.insert(claim.clone(), reservation.id.clone());
        }
        if machine {
            let mut capacity = reservation.capacity.lock();
            if capacity.len() >= limit {
                drop(capacity);
                return Err("CAPACITY_EXCEEDED".into());
            }
            capacity.insert(reservation.id.clone());
        }
        Ok(reservation)
    }
}

impl Drop for SpawnReservation {
    fn drop(&mut self) {
        if let Some(claim) = &self.claim {
            let mut live = self.live.lock();
            if !self.published && live.get(claim) == Some(&self.id) { live.remove(claim); }
            let mut pending = self.pending.lock();
            if !self.published && pending.get(claim) == Some(&self.id) { pending.remove(claim); }
        }
        if !self.retain_capacity { self.capacity.lock().remove(&self.id); }
    }
}

pub(super) struct AdmissionClock {
    logical_ms: u64,
    measured: Instant,
}

impl Default for AdmissionClock {
    fn default() -> Self {
        Self { logical_ms: 0, measured: Instant::now() }
    }
}

impl AdmissionClock {
    fn now(&mut self) -> u64 {
        let observed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_millis().min(u64::MAX as u128) as u64;
        let elapsed = self.measured.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.logical_ms = self.logical_ms.saturating_add(elapsed).max(observed);
        self.measured += Duration::from_millis(elapsed);
        self.logical_ms
    }
}

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

/// Serves a stored session cwd only when it can be a real local absolute path. A probe that captured
/// command output planted values like `cwd|rtd info error: No such file or directory` on sessions,
/// and serving such a value back made panes inherit a cwd they cannot spawn a shell from, so it is
/// replaced by the worktree path.
fn serveable_local_cwd(stored: &std::path::Path, worktree_cwd: Option<String>) -> Option<String> {
    if crate::ipc::terminal::is_plausible_absolute_cwd(stored) {
        Some(stored.to_string_lossy().to_string())
    } else {
        worktree_cwd
    }
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
    /// GUI-minted session id; part of the fingerprint so a retry of the same
    /// clientRequestId with a different id is a conflict, not a silent reuse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) requested_session_id: Option<String>,
}

/// Holds a GUI-minted session id while its spawn is in flight so two different
/// requests cannot both pass the conflict check before the PTY is registered.
struct RequestedSessionIdClaim {
    id: String,
    claims: Arc<Mutex<HashSet<String>>>,
}

impl Drop for RequestedSessionIdClaim {
    fn drop(&mut self) {
        self.claims.lock().remove(&self.id);
    }
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

#[derive(Clone, Debug, thiserror::Error)]
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
    Structured(crate::ipc::IpcError),
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

impl From<&str> for SpawnError {
    fn from(value: &str) -> Self { Self::Other(value.to_owned()) }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DurableRemoteSession {
    descriptor: crate::terminal::remote::RemoteSessionDescriptor,
    metadata: Option<StoredSessionMeta>,
}

/// Blocking cross-process guard for durable remote snapshot read-modify-write
/// cycles. During a drain chain two live daemons share one durable file; the
/// in-process tokio mutex cannot serialize across processes.
struct RemoteSnapshotFileLock(std::fs::File);

impl RemoteSnapshotFileLock {
    fn acquire(path: &std::path::Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;

            // SAFETY:
            // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
            // Invariant: `file.as_raw_fd()` returns a valid open file descriptor
            // borrowed from `file`, which remains open and valid for the duration of
            // the `libc::flock` call. The blocking variant waits instead of failing
            // because snapshot cycles are short and contention is rare.
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if ret != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::HANDLE;
            use windows_sys::Win32::Storage::FileSystem::{
                LockFileEx, LOCKFILE_EXCLUSIVE_LOCK,
            };
            use windows_sys::Win32::System::IO::OVERLAPPED;

            let handle = file.as_raw_handle() as HANDLE;
            // SAFETY:
            // Category: Uninitialized Memory.
            // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero
            // bit pattern is valid memory representing zero offset and null hEvent.
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

            // SAFETY:
            // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
            // Invariant: `handle` is a valid open Win32 file handle owned by `file`,
            // which remains open and valid for the duration of the LockFileEx call.
            // Without LOCKFILE_FAIL_IMMEDIATELY the call blocks until the range is free.
            let ret = unsafe {
                LockFileEx(
                    handle,
                    LOCKFILE_EXCLUSIVE_LOCK,
                    0,
                    1,
                    0,
                    &mut overlapped,
                )
            };
            if ret == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(Self(file))
    }
}

#[cfg(unix)]
impl Drop for RemoteSnapshotFileLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid File Descriptor.
        // Invariant: `self.0.as_raw_fd()` is a valid open file descriptor owned by
        // `self.0`, which has not been closed yet; clearing the flock before the
        // descriptor is closed by the `File` drop.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(windows)]
impl Drop for RemoteSnapshotFileLock {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let handle = self.0.as_raw_handle() as HANDLE;
        // SAFETY:
        // Category: Uninitialized Memory.
        // Invariant: `OVERLAPPED` is a C-compatible repr(C) struct whose all-zero bit
        // pattern is valid memory representing zero offset and null hEvent.
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };

        // SAFETY:
        // Category: Foreign Function Interface (FFI) / Invalid Handle Dereference.
        // Invariant: `handle` is a valid open Win32 file handle owned by `self.0`;
        // the unlock range (0, 1 byte) exactly matches the locked range.
        unsafe {
            UnlockFileEx(handle, 0, 1, 0, &mut overlapped);
        }
    }
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
        Self {
            device: device.into(),
            generation: 0,
            cancelled: tokio::sync::watch::channel(false).0,
            disconnected: Arc::new(Mutex::new(None)),
        }
    }
}

impl MachineController {
    pub(crate) fn reserved(&self) -> bool {
        self.reserved_at(tokio::time::Instant::now())
    }

    pub(crate) fn reserved_at(&self, now: tokio::time::Instant) -> bool {
        self.disconnected
            .lock()
            .is_none_or(|at| now.duration_since(at) < Duration::from_secs(15))
    }
}

pub(crate) struct MachineSocketLease {
    pub generation: u64,
    pub cancelled: tokio::sync::watch::Receiver<bool>,
    disconnected: Arc<Mutex<Option<tokio::time::Instant>>>,
}

impl Drop for MachineSocketLease {
    fn drop(&mut self) {
        *self.disconnected.lock() = Some(tokio::time::Instant::now());
    }
}

/// Headless session authority; owns metadata, claims and spawn idempotency.
/// Holds no server/gateway or AppHandle. Handover is weak to avoid a callback cycle.
pub struct DaemonSessionService {
    pub(crate) workspace_service: Arc<DaemonWorkspaceService>,
    pub(crate) machine_id: Result<String, String>,
    pub(super) terminal_service: Arc<TerminalService>,
    pub(super) session_router: Arc<super::proxy::SessionRouter>,
    pub(super) handover_manager: Weak<super::handover::HandoverManager>,
    pub(super) remote_event_tx: broadcast::Sender<DaemonRemoteEvent>,
    pub(super) spawn_idempotency_cache: Arc<Mutex<HashMap<String, SpawnCacheEntry>>>,
    pub(super) spawn_operations: Mutex<HashMap<String, Arc<Mutex<SpawnOperation>>>>,
    pub(super) admission_clock: Mutex<AdmissionClock>,
    pub(super) epoch: u64,
    pub(super) machine_capacity: Arc<Mutex<HashSet<String>>>,
    pub(super) capacity_initialized: tokio::sync::OnceCell<()>,
    pub(super) pending_provider_claims: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    pub(super) requested_session_ids: Arc<Mutex<HashSet<String>>>,
    /// Sole authority shared by socket replacement, input, resize and HTTP close.
    pub(crate) machine_controllers: tokio::sync::Mutex<HashMap<String, MachineController>>,
    pub(super) machine_lifecycles: Arc<Mutex<HashMap<String, tokio::sync::watch::Receiver<bool>>>>,
    pub(super) remote_persistence_lock: Arc<tokio::sync::Mutex<()>>,
    pub(super) remote_sessions_path: PathBuf,
    pub(super) ssh_store_path: PathBuf,
    pub(super) session_metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
    pub(super) provider_session_claims: Arc<Mutex<HashMap<ProviderSessionClaimKey, String>>>,
    pub(super) desktop_geometries: Arc<Mutex<HashMap<String, (u16, u16)>>>,
    pub(super) agent_states: Arc<AgentStateHub>,
    #[cfg(test)]
    pub(super) split_probe: RwLock<Option<Arc<dyn Fn(&str, &str) + Send + Sync>>>,
    #[cfg(test)]
    pub(super) cwd_probe: RwLock<Option<Arc<dyn Fn(u32) -> Option<PathBuf> + Send + Sync>>>,
    #[cfg(test)]
    pub(super) split_wire_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    #[cfg(test)]
    pub(super) split_git_observer: RwLock<Option<crate::worktree::git::GitObserver>>,
    #[cfg(test)]
    pub(super) split_probe_workers: Mutex<Vec<tokio::sync::oneshot::Receiver<()>>>,
    #[cfg(test)]
    pub(super) split_close_failure: AtomicBool,
}

impl DaemonSessionService {
    #[cfg(test)]
    pub(super) fn advance_admission_clock_for_test(&self, milliseconds: u64) {
        let mut clock = self.admission_clock.lock();
        clock.logical_ms = clock.now().saturating_add(milliseconds);
    }
    #[cfg(test)]
    pub(super) async fn drain_split_fixture(&self) {
        let probes = std::mem::take(&mut *self.split_probe_workers.lock());
        for probe in probes { tokio::time::timeout(Duration::from_secs(5), probe).await.unwrap().unwrap(); }
        let operations: Vec<_> = self.spawn_operations.lock().values().cloned().collect();
        for operation in operations {
            let mut result = operation.lock().result.subscribe();
            if operation.lock().fingerprint.is_none() { continue; }
            tokio::time::timeout(Duration::from_secs(5), async {
                while result.borrow().is_none() { result.changed().await.unwrap(); }
            }).await.unwrap();
        }
        let tasks = std::mem::take(&mut *self.split_wire_tasks.lock());
        for task in tasks { tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap(); }
    }

    #[cfg(test)]
    async fn split_test_stage(&self, request: &str, stage: &'static str) {
        let probe = self.split_probe.read().clone();
        if let Some(probe) = probe {
            let request = request.to_owned();
            if let Err(error) = crate::ipc::run_blocking(move || { probe(&request, stage); Ok(()) }).await {
                tracing::error!(%error, "Split test barrier failed");
            }
        }
    }

    pub(super) fn admission_time_unix_ms(&self) -> u64 {
        self.admission_clock.lock().now()
    }

    fn split_error(code: crate::ipc::IpcErrorCode, request: &str, epoch: u64, stage: &str) -> SpawnError {
        SpawnError::Structured(crate::ipc::IpcError::new(code, stage).with_details(
            serde_json::json!({"requestId":request,"originEpoch":epoch.to_string(),
                "stage":stage,"delivery":"confirmed","operationState":"pending"})))
    }

    fn operation_key(request: &str, envelope: Option<&LocalSplitEnvelope>) -> String {
        match envelope {
            Some(envelope) => format!("split:{}:{request}", envelope.origin_epoch),
            None => format!("legacy:{request}"),
        }
    }

    pub(super) async fn admit_spawn(
        self: &Arc<Self>, request: &str, workspace: &str,
        worktree: Option<WorktreeIdentity>, cwd: Option<String>, cols: u16, rows: u16,
        shell: Option<String>, startup: Option<TerminalStartup>,
        requested_session_id: Option<String>,
        machine: Option<Arc<MachineSpawn>>, envelope: Option<LocalSplitEnvelope>,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        use crate::ipc::IpcErrorCode;
        if request.trim().is_empty() {
            return Err("clientRequestId cannot be empty".into());
        }
        // Machine (remote-issued) spawns carry their own target id.
        let requested_session_id = if machine.is_some() { None } else { requested_session_id };
        if let Some(id) = &requested_session_id {
            if uuid::Uuid::parse_str(id).ok().is_none_or(|parsed| parsed.to_string() != *id) {
                return Err(SpawnError::Structured(crate::ipc::IpcError::new(
                    IpcErrorCode::InvalidArgument,
                    "sessionId must be a canonical lowercase UUID",
                ).with_details(serde_json::json!({"sessionId": id}))));
            }
        }
        if let Some(split) = &envelope {
            if uuid::Uuid::parse_str(request).ok().is_none_or(|id| id.to_string() != request)
            {
                return Err(Self::split_error(IpcErrorCode::InvalidArgument, request, split.origin_epoch, "admission"));
            }
            if split.origin_epoch != self.epoch {
                return Err(Self::split_error(IpcErrorCode::SpawnEpochChanged, request, split.origin_epoch, "admission"));
            }
        }
        let fingerprint = SpawnRequestFingerprint {
            workspace_id: workspace.into(), worktree: worktree.clone(), cwd: cwd.clone(), cols, rows,
            shell: shell.clone(), provider_claim: ProviderSessionClaimKey::from_startup(startup.as_ref()),
            startup: startup.clone(), requested_session_id: requested_session_id.clone(),
        };
        let key = Self::operation_key(request, envelope.as_ref());
        let now = self.admission_time_unix_ms();
        let (operation, owner) = {
        let mut operations = self.spawn_operations.lock();
        operations.retain(|_, operation| {
            let operation = operation.lock();
            !matches!(operation.state, SplitOperationResult::Cancelled | SplitOperationResult::Exited | SplitOperationResult::Failed { .. })
                || operation.envelope.as_ref().is_none_or(|split| split.expires_at_unix_ms > now)
        });
        let digest = machine.as_ref().map(|machine| machine.digest.clone());
        if let Some(operation) = operations.get(&key) {
            let record = operation.lock();
            if record.envelope.as_ref().map(|split| split.expires_at_unix_ms)
                    != envelope.as_ref().map(|split| split.expires_at_unix_ms)
                || record.fingerprint.as_ref().is_some_and(|stored| stored != &fingerprint)
                || record.machine_digest != digest
            {
                return Err(match &envelope {
                    Some(split) => Self::split_error(IpcErrorCode::SpawnRequestConflict, request, split.origin_epoch, "fingerprint"),
                    None if machine.is_some() => SpawnError::Other("REQUEST_CONFLICT".into()),
                    None => SpawnError::Other(format!("clientRequestId '{request}' was reused with a different spawn request")),
                });
            }
            if matches!(record.state, SplitOperationResult::Cancelled) {
                return Err(Self::split_error(IpcErrorCode::SpawnCancelled, request, self.epoch, "cancelled"));
            }
            (operation.clone(), false)
        } else {
            if let Some(split) = &envelope {
                if machine.is_some() || startup.is_some() || crate::ssh::projects::is_remote(workspace) || cwd.is_none() {
                    return Err(Self::split_error(IpcErrorCode::InvalidArgument, request, split.origin_epoch, "admission"));
                }
                if split.expires_at_unix_ms <= now || split.expires_at_unix_ms > now.saturating_add(LOCAL_SPLIT_VALIDITY_MS) {
                    return Err(Self::split_error(IpcErrorCode::SpawnRequestExpired, request, split.origin_epoch, "admission"));
                }
            }
            let operation = Arc::new(Mutex::new(SpawnOperation {
                fingerprint: Some(fingerprint), machine_digest: digest,
                deadline: envelope.as_ref().map(|split| Instant::now() + Duration::from_millis(split.remaining_ms.min(9_000)))
                    .or_else(|| machine.as_ref().map(|machine| machine.deadline)),
                envelope: envelope.clone(), cancelled: Arc::new(AtomicBool::new(false)),
                cancellation: Arc::new(tokio::sync::Notify::new()),
                result: tokio::sync::watch::channel(None).0,
                state: SplitOperationResult::Pending { cancel_requested: false }, child: None,
                machine_target: machine.as_ref().map(|machine| machine.target.session_id.clone()),
                cleanup_started: false, changed: tokio::sync::watch::channel(0).0,
                workspace_guard: None,
            }));
            operations.insert(key.clone(), operation.clone());
            (operation, true)
        }
        };
        let mut receiver = operation.lock().result.subscribe();
        if owner {
            let service = self.clone();
            let request = request.to_owned();
            let workspace = workspace.to_owned();
            let admission = self.handover_manager.upgrade()
                .ok_or_else(|| SpawnError::Other("HOST_UNAVAILABLE".into()))
                .and_then(|manager| manager.retain_spawn_owner().map_err(SpawnError::Other));
            tokio::spawn(async move {
                let _retirement = service.handover_manager.upgrade()
                    .and_then(|manager| manager.retain_request(service.terminal_service.clone()).ok());
                let started = Instant::now();
                tracing::info!(request_id = %request, epoch = service.epoch, stage = "admission", boundary = "begin");
                let mut retained_owner = None;
                let machine_settlement = machine.clone();
                let mut result = match admission {
                    Ok(owner) => {
                        retained_owner = Some(owner);
                        service.spawn_owned(&request, &workspace, worktree, cwd, cols, rows,
                            shell, startup, requested_session_id, machine, operation.clone(), #[cfg(test)] helper_home).await
                    },
                    Err(error) => Err(error),
                };
                if result.is_err() {
                    if let Some(machine) = machine_settlement {
                        let workspace = service.workspace_service.clone();
                        let settled = crate::ipc::run_blocking(move || {
                            let record = workspace.journal.reconcile(&machine.device, &machine.request.request_id)
                                .map_err(crate::ipc::IpcError::internal)?;
                            if record.is_some_and(|record| matches!(record.operation, crate::remote::machine_protocol::Operation::Pending { .. })) {
                                workspace.journal.mark_unknown(&machine.device, &machine.request.request_id)
                                    .map_err(crate::ipc::IpcError::internal)?;
                                return Ok(true);
                            }
                            Ok(false)
                        }).await;
                        if !matches!(settled, Ok(false)) { result = Err("OPERATION_OUTCOME_UNKNOWN".into()); }
                    }
                }
                service.finish_operation(&operation, &result).await;
                tracing::info!(request_id = %request, epoch = service.epoch, stage = "admission", boundary = if result.is_ok() { "end" } else { "failure" }, elapsed_ms = started.elapsed().as_millis() as u64);
                {
                    let record = operation.lock();
                    record.result.send_replace(Some(result));
                    record.changed.send_modify(|revision| *revision += 1);
                }
                operation.lock().workspace_guard.take();
                drop(retained_owner);
                if envelope.is_none() {
                    let mut map = service.spawn_operations.lock();
                    if map.get(&key).is_some_and(|current| Arc::ptr_eq(current, &operation)) {
                        map.remove(&key);
                    }
                }
            });
        }
        loop {
            if let Some(result) = receiver.borrow().clone() { return result; }
            receiver.changed().await.map_err(|_| SpawnError::Other("OPERATION_OUTCOME_UNKNOWN".into()))?;
        }
    }

    async fn finish_operation(&self, operation: &Arc<Mutex<SpawnOperation>>, result: &Result<String, SpawnError>) {
        let child = operation.lock().child.clone();
        let described = child.as_deref().map(|id| self.handle_describe_session(id));
        {
            let mut record = operation.lock();
            if matches!(record.state, SplitOperationResult::Exited | SplitOperationResult::Cancelled) {
                return;
            }
            record.state = match (result, described) {
                (Ok(id), Some(DaemonResponse::DescribeSessionOk { session })) => SplitOperationResult::Created {
                    session_id: id.clone(), daemon_epoch: self.epoch, session, ownership: SplitOwnership::Created,
                },
                (_, _) if record.child.is_some() => SplitOperationResult::Unknown { reason: SplitUnknownReason::PublicationUncertain },
                (Err(error), _) => SplitOperationResult::Failed {
                    error: match error { SpawnError::Structured(error) => error.clone(), _ => crate::ipc::IpcError::internal(error.to_string()) },
                    no_child: SplitNoChild,
                },
                _ => SplitOperationResult::Unknown { reason: SplitUnknownReason::PublicationUncertain },
            };
            if record.cancelled.load(Ordering::Acquire) && record.child.is_none() {
                record.state = SplitOperationResult::Cancelled;
            }
        }
        let cancelled = operation.lock().cancelled.load(Ordering::Acquire);
        if cancelled {
            self.cancel_operation_child(operation).await;
        }
    }

    async fn cancel_operation_child(&self, operation: &Arc<Mutex<SpawnOperation>>) {
        let child = {
            let mut record = operation.lock();
            if record.cleanup_started || matches!(record.state, SplitOperationResult::Cancelled | SplitOperationResult::Exited) { return; }
            if record.child.is_none() {
                record.state = SplitOperationResult::Cancelled;
                record.changed.send_modify(|revision| *revision += 1);
                return;
            }
            record.cleanup_started = true;
            record.state = SplitOperationResult::Pending { cancel_requested: true };
            record.child.clone()
        };
        if let Some(child) = child {
            let lifecycle = self.machine_lifecycles.lock().get(&child).cloned();
            #[cfg(test)]
            let injected_failure = self.split_close_failure.swap(false, Ordering::AcqRel);
            #[cfg(not(test))]
            let injected_failure = false;
            let closed = if injected_failure {
                Err(crate::terminal::PtyError::Other("injected close failure".into()))
            } else { self.terminal_service.close_session(&child).await };
            if let Err(error) = closed {
                tracing::error!(%error, session_id = %child, "Split cancellation cleanup failed");
                let mut record = operation.lock();
                record.cleanup_started = false;
                record.state = SplitOperationResult::Pending { cancel_requested: true };
                record.changed.send_modify(|revision| *revision += 1);
                return;
            }
            if let Some(mut lifecycle) = lifecycle {
                while !*lifecycle.borrow() {
                    if lifecycle.changed().await.is_err() {
                        let mut record = operation.lock();
                        record.cleanup_started = false;
                        record.changed.send_modify(|revision| *revision += 1);
                        return;
                    }
                }
            }
            let mut record = operation.lock();
            if matches!(record.state, SplitOperationResult::Unknown { .. }) { return; }
            record.state = SplitOperationResult::Cancelled;
            record.changed.send_modify(|revision| *revision += 1);
        }
    }

    pub(super) fn split_operation(self: &Arc<Self>, request: &str, origin_epoch: u64, expiry: u64, cancel: bool) -> Result<SplitOperationResult, SpawnError> {
        use crate::ipc::IpcErrorCode;
        if uuid::Uuid::parse_str(request).ok().is_none_or(|id| id.to_string() != request) {
            return Err(Self::split_error(IpcErrorCode::InvalidArgument, request, origin_epoch, "identity"));
        }
        let envelope = LocalSplitEnvelope { origin_epoch, expires_at_unix_ms: expiry, remaining_ms: 0 };
        let key = Self::operation_key(request, Some(&envelope));
        let now = self.admission_time_unix_ms();
        let mut operations = self.spawn_operations.lock();
        if let Some(operation) = operations.get(&key).cloned() {
            let mut record = operation.lock();
            if record.envelope.as_ref().is_none_or(|split| split.expires_at_unix_ms != expiry) {
                return Err(Self::split_error(IpcErrorCode::SpawnRequestConflict, request, origin_epoch, "identity"));
            }
            if cancel && !matches!(record.state, SplitOperationResult::Cancelled | SplitOperationResult::Exited | SplitOperationResult::Failed { .. }) {
                record.cancelled.store(true, Ordering::Release);
                record.cancellation.notify_waiters();
                record.state = SplitOperationResult::Pending { cancel_requested: true };
                if record.result.borrow().is_some() {
                    let service = self.clone();
                    let cleanup = operation.clone();
                    tokio::spawn(async move { service.cancel_operation_child(&cleanup).await; });
                }
            }
            return Ok(record.state.clone());
        }
        if origin_epoch != self.epoch {
            return Ok(SplitOperationResult::Unknown { reason: SplitUnknownReason::EpochChanged });
        }
        let valid = expiry > now && expiry <= now.saturating_add(LOCAL_SPLIT_VALIDITY_MS);
        if cancel && valid {
            operations.insert(key, Arc::new(Mutex::new(SpawnOperation {
                fingerprint: None, machine_digest: None, envelope: Some(envelope), deadline: None,
                cancelled: Arc::new(AtomicBool::new(true)), cancellation: Arc::new(tokio::sync::Notify::new()),
                result: tokio::sync::watch::channel(None).0, state: SplitOperationResult::Cancelled, child: None,
                machine_target: None,
                cleanup_started: false, changed: tokio::sync::watch::channel(0).0,
                workspace_guard: None,
            })));
            return Ok(SplitOperationResult::Cancelled);
        }
        Ok(SplitOperationResult::Absent { can_create: valid })
    }

    pub(super) async fn cancel_split_until(self: &Arc<Self>, request: &str, epoch: u64, expiry: u64) -> Result<SplitOperationResult, SpawnError> {
        let key = format!("split:{epoch}:{request}");
        let mut changed = self.spawn_operations.lock().get(&key).map(|operation| operation.lock().changed.subscribe());
        let initial = self.split_operation(request, epoch, expiry, true)?;
        let Some(ref mut changed) = changed else { return Ok(initial); };
        let deadline = tokio::time::Instant::now() + Duration::from_millis(2_500);
        loop {
            let state = self.split_operation(request, epoch, expiry, false)?;
            if !matches!(state, SplitOperationResult::Pending { .. }) { return Ok(state); }
            if !matches!(tokio::time::timeout_at(deadline, changed.changed()).await, Ok(Ok(()))) {
                return self.split_operation(request, epoch, expiry, false);
            }
        }
    }

    fn check_spawn_operation(operation: &Mutex<SpawnOperation>) -> Result<(), SpawnError> {
        let record = operation.lock();
        if record.cancelled.load(Ordering::Acquire) {
            return Err(SpawnError::Structured(crate::ipc::IpcError::new(crate::ipc::IpcErrorCode::SpawnCancelled, "cancelled before child")));
        }
        if record.deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(SpawnError::Structured(crate::ipc::IpcError::new(crate::ipc::IpcErrorCode::SpawnAttemptTimeout, "pre-child deadline")));
        }
        Ok(())
    }

    pub(crate) fn subscribe_agent_states(&self, session_id: &str) -> crate::daemon::agent_state::AgentStateSubscription {
        self.agent_states.subscribe(session_id)
    }

    #[cfg(test)]
    pub fn publish_agent_state_for_test(&self, state: crate::daemon::agent_state::AgentState) {
        self.agent_states.publish_canonical(state);
    }

    pub fn ensure_agent_sink(&self) {
        // Bind, then unsize: `Arc::clone` cannot coerce through `&Arc<_>` at the call site.
        let sink: Arc<dyn crate::terminal::remote::AgentStateSink> = self.agent_states.clone();
        self.terminal_service.remote().set_agent_sink(sink.clone());
        self.terminal_service.set_paired_agent_sink(sink);
    }

    pub fn record_desktop_geometry(&self, session_id: &str, cols: u16, rows: u16) {
        self.desktop_geometries
            .lock()
            .insert(session_id.to_string(), (cols, rows));
    }

    pub fn desktop_geometry(&self, session_id: &str) -> Option<(u16, u16)> {
        self.desktop_geometries.lock().get(session_id).copied()
    }
    pub fn session_activity_state(&self, session_id: &str) -> Option<String> {
        let current = self.agent_states.current(session_id)?;
        match current.state.trim().to_ascii_lowercase().as_str() {
            "working" => Some("working".to_string()),
            "waiting" | "blocked" => Some("waiting".to_string()),
            "done" => Some("done".to_string()),
            _ => None,
        }
    }

    /// The agent's own session identity, when its extension reported one. A transcript file is
    /// named after that id, so preferring it resolves a session to its own conversation instead of
    /// to whichever transcript in the same cwd happened to be written last.
    pub fn session_provider_session(
        &self,
        session_id: &str,
    ) -> Option<crate::daemon::protocol::AgentProviderSession> {
        self.agent_states.current(session_id)?.provider_session
    }

    /// Durable store of paired-host sessions, keyed by `descriptor.backendSessionId`.
    ///
    /// That key is the same id this service and the gateway route by, because a remote session is
    /// registered in the terminal hub under it. Callers that need a session's host read the store
    /// rather than holding the live runtime, which is owned by the daemon.
    pub fn remote_sessions_store_path(&self) -> &std::path::Path {
        &self.remote_sessions_path
    }

    pub(crate) fn max_machine_sessions(&self) -> usize {
        if let Ok(val) = std::env::var("FERRYX_MAX_MACHINE_SESSIONS") {
            if let Ok(parsed) = val.parse::<usize>() {
                return parsed;
            }
        }
        if cfg!(test) {
            64
        } else {
            256
        }
    }

    #[cfg(test)]
    pub(crate) fn journal_spawn_probe_handles(
        &self,
        workspace: &str,
    ) -> (Arc<tokio::sync::Mutex<()>>, Arc<TerminalService>) {
        (self.workspace_service.spawn_queue(workspace), self.terminal_service.clone())
    }

    pub(crate) fn attach_machine_output(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Option<
        Result<
            crate::terminal::output_hub::machine_output::MachineAttachment,
            crate::terminal::output_hub::machine_output::MachineOutputError,
        >,
    > {
        self.terminal_service
            .output_hub()
            .subscribe_machine(session_id, after_sequence)
    }

    pub(crate) async fn validate_machine_target(
        self: &Arc<Self>,
        target: &crate::remote::machine_protocol::RemoteTerminalTarget,
    ) -> Result<crate::remote::machine_protocol::Session, String> {
        let service = Arc::clone(self);
        let target = target.clone();
        tokio::task::spawn_blocking(move || service.validate_machine_target_blocking(&target))
            .await
            .map_err(|error| format!("Machine target validation task failed: {error}"))?
    }

    pub(crate) fn project_desktop_gui_session(
        &self,
        session_id: &str,
        epoch: crate::scoped_contracts::Epoch,
        catalog: &crate::remote::workspace_catalog::Catalog,
    ) -> Result<crate::remote::machine_protocol::Session, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let metadata = self
            .session_metadata
            .try_read_until(deadline)
            .ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        let meta = metadata.get(session_id).ok_or("SESSION_NOT_FOUND")?.clone();
        drop(metadata);

        if crate::ssh::projects::is_remote(&meta.workspace_id)
            || meta.workspace_id.starts_with("ssh:")
            || meta.workspace_id.contains("::")
        {
            // A remote/ssh session's cwd lives on the remote host, so the local root jail
            // cannot apply. Serve it from the remote runtime instead of refusing it: desktop
            // parity means the machine inventory describes every session the desktop shows.
            let details = self
                .terminal_service
                .remote()
                .details(session_id)
                .ok_or("SESSION_NOT_FOUND")?;
            if epoch.0 != self.epoch {
                return Err("STALE_EPOCH".into());
            }
            let machine_id = self
                .machine_id
                .as_ref()
                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?
                .clone();
            let (start, end) = self
                .terminal_service
                .output_hub()
                .session_sequence_range(session_id)
                .unwrap_or_default();
            let descriptor = details.descriptor;
            return Ok(crate::remote::machine_protocol::Session {
                title: None,
                agent_type: None,
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id,
                    daemon_epoch: epoch,
                    session_id: session_id.to_owned(),
                },
                workspace_id: meta.workspace_id.clone(),
                worktree: meta.worktree.as_ref().map(|w| {
                    crate::remote::machine_protocol::WorktreeIdentity {
                        ws_id: w.ws_id.clone(),
                        slug: w.slug.clone(),
                    }
                }),
                cwd: descriptor.config.project_path.clone(),
                cols: descriptor.cols,
                rows: descriptor.rows,
                running: details.state == crate::terminal::remote::RemoteConnectionState::Connected,
                provider_session: None,
                start_sequence: crate::scoped_contracts::Epoch(start.unwrap_or(0)),
                end_sequence: crate::scoped_contracts::Epoch(end.unwrap_or(0)),
            });
        }
        if epoch.0 != self.epoch {
            return Err("STALE_EPOCH".into());
        }

        let cat_row = catalog
            .workspaces
            .get(&meta.workspace_id)
            .ok_or("SESSION_NOT_FOUND")?;
        if !matches!(cat_row.availability, crate::remote::machine_protocol::Availability::Ready) {
            return Err("SESSION_NOT_FOUND".into());
        }
        let root_dir = if let Some(ref identity) = meta.worktree {
            if identity.ws_id != meta.workspace_id {
                return Err("SESSION_OWNERSHIP_CHANGED".into());
            }
            if let Ok((_mgr, path)) = self
                .workspace_service
                .registry
                .resolve_terminal_target(&meta.workspace_id, Some(identity))
            {
                path
            } else {
                let manager = self.workspace_service.worktree_manager(&meta.workspace_id, false)?;
                let worktree = manager
                    .find_worktree_by_slug(&identity.ws_id, &identity.slug)
                    .map_err(|_| "WORKTREE_NOT_FOUND")?
                    .ok_or("WORKTREE_NOT_FOUND")?;
                manager
                    .canonical_allowed_path(&worktree.path)
                    .map_err(|_| "INVALID_PATH")?
            }
        } else if let Ok((_mgr, path)) = self
            .workspace_service
            .registry
            .resolve_terminal_target(&meta.workspace_id, None)
        {
            path
        } else {
            cat_row.repo_root.clone()
        };

        let canonical_meta_cwd = std::fs::canonicalize(&meta.cwd)
            .map_err(|_| "INVALID_PATH")?;
        let canonical_root = std::fs::canonicalize(&root_dir)
            .map_err(|_| "INVALID_PATH")?;
        if !canonical_meta_cwd.starts_with(&canonical_root) {
            return Err("SESSION_OWNERSHIP_CHANGED".into());
        }
        let rel_cwd = canonical_meta_cwd
            .strip_prefix(&canonical_root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .map_err(|_| "INVALID_PATH")?;
        let cwd_str = if rel_cwd.is_empty() { ".".to_string() } else { rel_cwd };

        let pty = self.terminal_service.get_session(session_id).ok_or("SESSION_EXPIRED")?;
        let running = matches!(pty.state(), PtySessionState::Starting | PtySessionState::Running);
        let (cols, rows) = pty.get_size();
        let (start, end) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(session_id)
            .unwrap_or_default();
        let machine_id = self
            .machine_id
            .as_ref()
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?
            .clone();

        Ok(crate::remote::machine_protocol::Session {
            title: None,
            agent_type: None,
            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                machine_id,
                daemon_epoch: epoch,
                session_id: session_id.to_owned(),
            },
            workspace_id: meta.workspace_id.clone(),
            worktree: meta.worktree.as_ref().map(|w| crate::remote::machine_protocol::WorktreeIdentity {
                ws_id: w.ws_id.clone(),
                slug: w.slug.clone(),
            }),
            cwd: cwd_str,
            cols,
            rows,
            running,
            provider_session: None,
            start_sequence: crate::scoped_contracts::Epoch(start.unwrap_or(0)),
            end_sequence: crate::scoped_contracts::Epoch(end.unwrap_or(0)),
        })
    }

    fn validate_machine_target_blocking(
        &self,
        target: &crate::remote::machine_protocol::RemoteTerminalTarget,
    ) -> Result<crate::remote::machine_protocol::Session, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let metadata = self
            .session_metadata
            .try_read_until(deadline)
            .ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        let meta = metadata.get(&target.session_id).ok_or("SESSION_EXPIRED")?.clone();
        drop(metadata);

        let catalog = self
            .workspace_service
            .catalog
            .try_lock_until(deadline)
            .ok_or("MACHINE_SERVICE_UNAVAILABLE")?
            .as_ref()
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?
            .clone();

        match meta.machine_session.as_ref() {
            Some(session) => {
                if session.target != *target {
                    return Err("STALE_EPOCH".into());
                }
                if meta.workspace_id != session.workspace_id
                    || meta.worktree.as_ref().map(|w| (&w.ws_id, &w.slug))
                        != session.worktree.as_ref().map(|w| (&w.ws_id, &w.slug))
                    || !catalog.workspaces.contains_key(&meta.workspace_id)
                {
                    return Err("SESSION_OWNERSHIP_CHANGED".into());
                }
                let pty = self
                    .terminal_service
                    .get_session(&target.session_id)
                    .ok_or("SESSION_EXPIRED")?;
                if !matches!(
                    pty.state(),
                    PtySessionState::Starting | PtySessionState::Running
                ) {
                    return Err("SESSION_EXPIRED".into());
                }
                let mut session = session.clone();
                (session.cols, session.rows) = pty.get_size();
                Ok(session)
            }
            None => {
                let machine_id = self
                    .machine_id
                    .as_ref()
                    .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                if target.daemon_epoch.0 != self.epoch || &target.machine_id != machine_id {
                    return Err("STALE_EPOCH".into());
                }
                let session = self.project_desktop_gui_session(&target.session_id, target.daemon_epoch, &catalog)?;
                if !session.running {
                    return Err("SESSION_EXPIRED".into());
                }
                Ok(session)
            }
        }
    }

    pub(crate) fn acquire_machine_controller(
        controllers: &mut HashMap<String, MachineController>,
        id: &str,
        device: &str,
    ) -> Result<MachineSocketLease, String> {
        let generation = if let Some(previous) = controllers.get(id) {
            if previous.device != device && previous.reserved() {
                return Err("CONTROL_CONFLICT".into());
            }
            previous
                .generation
                .checked_add(1)
                .ok_or("CAPACITY_EXCEEDED")?
        } else {
            1
        };
        let (cancelled, receiver) = tokio::sync::watch::channel(false);
        let disconnected = Arc::new(Mutex::new(None));
        let entry = MachineController {
            device: device.into(),
            generation,
            cancelled,
            disconnected: disconnected.clone(),
        };
        if let Some(previous) = controllers.insert(id.into(), entry) {
            previous.cancelled.send_replace(true);
        }
        Ok(MachineSocketLease {
            generation,
            cancelled: receiver,
            disconnected,
        })
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
                .await
                .map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?
                .map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
        }
        Ok(())
    }

    pub(crate) async fn spawn_machine(
        self: &Arc<Self>,
        request: crate::remote::machine_protocol::CreateSessionRequest,
        device: String,
        digest: String,
        target: crate::remote::machine_protocol::RemoteTerminalTarget,
        deadline: Instant,
        check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<String, String> {
        use crate::remote::machine_protocol::Startup;
        let startup = match &request.startup {
            Startup::Shell => None,
            Startup::AgentResume {
                agent_type,
                provider_session,
            } => {
                // Only OMO currently has an ID-based authoritative transcript/CWD resolver.
                // Other providers' argv support alone cannot prove transcript ownership.
                if agent_type != "omo" || provider_session.transcript_path.is_some() {
                    return Err("AGENT_RESUME_UNSUPPORTED".into());
                }
                Some(TerminalStartup::AgentResume {
                    agent_type: agent_type.clone(),
                    provider_session: provider_session.clone(),
                })
            }
        };
        let worktree = request.worktree.as_ref().map(|w| WorktreeIdentity {
            ws_id: w.ws_id.clone(),
            slug: w.slug.clone(),
        });
        let cache_key = serde_json::to_string(&("machine", &device, &request.request_id))
            .expect("request identity");
        let context = Arc::new(MachineSpawn {
            request: request.clone(),
            device: device.clone(),
            digest,
            target,
            deadline,
            check,
        });
        let result = MACHINE_SPAWN
            .scope(
                context,
                self.handle_spawn(
                    &cache_key,
                    &request.workspace_id,
                    worktree,
                    None,
                    request.cols,
                    request.rows,
                    None,
                    startup,
                    None,
                    #[cfg(test)]
                    None,
                ),
            )
            .await;
        if result.is_err() {
            let workspaces = self.workspace_service.clone();
            let request_id = request.request_id.clone();
            let ambiguous = crate::ipc::run_blocking(move || {
                if workspaces
                    .journal
                    .reconcile(&device, &request_id)
                    .map_err(crate::ipc::IpcError::internal)?
                    .is_some_and(|r| {
                        matches!(
                            r.operation,
                            crate::remote::machine_protocol::Operation::Pending { .. }
                        )
                    })
                {
                    workspaces
                        .journal
                        .mark_unknown(&device, &request_id)
                        .map_err(crate::ipc::IpcError::internal)?;
                    return Ok(true);
                }
                Ok(false)
            })
            .await
            .map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
            if ambiguous {
                return Err("OPERATION_OUTCOME_UNKNOWN".into());
            }
        }
        result.map_err(|e| match e {
            SpawnError::AgentSessionConflict { .. } => "AGENT_SESSION_CONFLICT".into(),
            SpawnError::InvalidAgentResume(_) => "AGENT_RESUME_INVALID".into(),
            SpawnError::Structured(error) => error.to_string(),
            SpawnError::Other(code) => match code.as_str() {
                "UNAUTHORIZED"
                | "TIMEOUT"
                | "PROJECT_NOT_FOUND"
                | "WORKTREE_NOT_FOUND"
                | "SESSION_NOT_FOUND"
                | "SESSION_EXPIRED"
                | "PARENT_SESSION_MISMATCH"
                | "CAPACITY_EXCEEDED"
                | "MACHINE_SERVICE_UNAVAILABLE"
                | "OPERATION_OUTCOME_UNKNOWN"
                | "REQUEST_CONFLICT"
                | "AGENT_RESUME_UNSUPPORTED" => code,
                _ => "INVALID_PATH".into(),
            },
        })
    }

    pub(crate) fn machine_detail(
        &self,
        id: &str,
        epoch: crate::scoped_contracts::Epoch,
    ) -> Result<crate::remote::machine_protocol::SessionDetail, String> {
        use crate::remote::machine_protocol::SessionDetail;
        let mut record = match self.workspace_service.journal.session(id)? {
            Some(record) => record,
            None => {
                let catalog = self.workspace_service.catalog().map_err(|e| e.to_string())?;
                let session = match self.project_desktop_gui_session(id, epoch, &catalog) {
                    Ok(s) => s,
                    Err(e) if e == "SESSION_EXPIRED" => {
                        let machine_id = self
                            .machine_id
                            .as_ref()
                            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?
                            .clone();
                        return Ok(SessionDetail::Expired {
                            target: crate::remote::machine_protocol::RemoteTerminalTarget {
                                machine_id,
                                daemon_epoch: epoch,
                                session_id: id.to_owned(),
                            },
                        });
                    }
                    Err(e) => return Err(e.to_string()),
                };
                if session.running {
                    return Ok(SessionDetail::Running { session });
                } else {
                    return Ok(SessionDetail::Exited {
                        session,
                        exit: crate::remote::machine_protocol::ExitMetadata { code: None, signal: None },
                    });
                }
            }
        };
        if let Some(exit) = record.exit {
            record.session.running = false;
            return Ok(SessionDetail::Exited {
                session: record.session,
                exit,
            });
        }
        if record.session.target.daemon_epoch != epoch {
            return Ok(SessionDetail::Expired {
                target: record.session.target,
            });
        }
        let Some(pty) = self.terminal_service.get_session(id) else {
            return Ok(SessionDetail::Expired {
                target: record.session.target,
            });
        };
        if let PtySessionState::Exited { code } = pty.state() {
            record.session.running = false;
            return Ok(SessionDetail::Exited {
                session: record.session,
                exit: crate::remote::machine_protocol::ExitMetadata { code, signal: None },
            });
        }
        if matches!(pty.state(), PtySessionState::Failed { .. }) {
            return Ok(SessionDetail::Expired {
                target: record.session.target,
            });
        }
        (record.session.cols, record.session.rows) = pty.get_size();
        let (start, end) = self
            .terminal_service
            .output_hub()
            .session_sequence_range(id)
            .unwrap_or_default();
        record.session.start_sequence = crate::scoped_contracts::Epoch(start.unwrap_or(0));
        record.session.end_sequence = crate::scoped_contracts::Epoch(end.unwrap_or(0));
        Ok(SessionDetail::Running {
            session: record.session,
        })
    }

    pub(crate) fn machine_sessions(
        &self,
        epoch: crate::scoped_contracts::Epoch,
    ) -> Result<crate::remote::machine_protocol::Sessions, String> {
        use crate::remote::machine_protocol::*;
        let mut sessions = Vec::new();
        for record in self.workspace_service.journal.sessions()? {
            let mut session = record.session;
            match self.machine_detail(&session.target.session_id, epoch)? {
                SessionDetail::Running { session: live }
                | SessionDetail::Exited { session: live, .. } => session = live,
                SessionDetail::Expired { .. } => session.running = false,
            }
            sessions.push(session);
        }
        Ok(Sessions {
            revision: self.workspace_service.journal.session_revision()?,
            completeness: Completeness::Complete,
            sessions,
            unavailable_workspace_ids: Vec::new(),
        })
    }

    pub(crate) async fn close_machine(
        &self,
        device: &str,
        request: &str,
        digest: &str,
        id: &str,
        expected_epoch: crate::scoped_contracts::Epoch,
        owner_epoch: crate::scoped_contracts::Epoch,
        check: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> Result<(), String> {
        use crate::remote::{machine_operation_journal::Begin, machine_protocol::*};
        let _retirement = self.retain_machine_request()?;
        let pending: Vec<_> = self.spawn_operations.lock().values().cloned().collect();
        for operation in pending {
            let mut completion = {
                let record = operation.lock();
                (record.child.as_deref() == Some(id) || record.machine_target.as_deref() == Some(id)).then(|| record.result.subscribe())
            };
            if let Some(ref mut completion) = completion {
                while completion.borrow().is_none() {
                    completion.changed().await.map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
                }
            }
        }
        let workspaces = self.workspace_service.clone();
        let owned_id = id.to_owned();
        let workspace = crate::ipc::run_blocking(move || {
            Ok(workspaces.journal.session(&owned_id).map_err(crate::ipc::IpcError::internal)?
                .ok_or_else(|| crate::ipc::IpcError::internal("SESSION_NOT_FOUND"))?.session.workspace_id)
        }).await.map_err(|_| "SESSION_NOT_FOUND")?;
        let _spawn = self.workspace_service.spawn_queue(&workspace).lock_owned().await;
        let controllers = self.machine_controllers.lock().await;
        check()?;
        let workspaces = self.workspace_service.clone();
        let owned_id = id.to_owned();
        let mut record = crate::ipc::run_blocking(move || {
            workspaces.journal.session(&owned_id).map_err(crate::ipc::IpcError::internal)?
                .ok_or_else(|| crate::ipc::IpcError::internal("SESSION_NOT_FOUND"))
        }).await.map_err(|error| error.message)?;
        if record.session.target.daemon_epoch != expected_epoch {
            return Err("STALE_EPOCH".into());
        }
        let controller = controllers
            .get(id)
            .filter(|c| c.reserved())
            .map(|c| &c.device)
            .unwrap_or(&record.creator_device);
        if controller != device {
            return Err("CONTROL_CONFLICT".into());
        }
        if record.exit.is_none() && expected_epoch != owner_epoch {
            return Err("SESSION_EXPIRED".into());
        }
        let services = self.workspace_service.clone();
        let device_owned = device.to_owned();
        let request_owned = request.to_owned();
        let digest = digest.to_owned();
        let resource = serde_json::to_string(&record.session.target).expect("target");
        let check_begin = check.clone();
        let begin = crate::ipc::run_blocking(move || {
            check_begin().map_err(crate::ipc::IpcError::internal)?;
            services
                .journal
                .begin(
                    &device_owned,
                    &request_owned,
                    "closeSession",
                    &digest,
                    &resource,
                )
                .map_err(crate::ipc::IpcError::internal)
        })
        .await
        .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        if let Begin::Existing(_) = begin {
            return Ok(());
        }
        let pty = self.terminal_service.get_session(id);
        if record.exit.is_none() {
            if pty.is_none() {
                return Err("OPERATION_OUTCOME_UNKNOWN".into());
            }
            check()?;
            self.terminal_service
                .close_machine_session(id, check.clone())
                .await
                .map_err(|_| "OPERATION_OUTCOME_UNKNOWN")?;
            if !pty.as_ref().is_some_and(|p| p.is_reaped()) {
                return Err("OPERATION_OUTCOME_UNKNOWN".into());
            }
            record.session.running = false;
            record.exit = Some(ExitMetadata {
                code: pty.and_then(|p| match p.state() {
                    PtySessionState::Exited { code } => code,
                    _ => None,
                }),
                signal: None,
            });
        }
        self.release_session_ownership(id);
        let services = self.workspace_service.clone();
        let device = device.to_owned();
        let request = request.to_owned();
        self.wait_machine_lifecycle(id).await?;
        crate::ipc::run_blocking(move || {
            services
                .journal
                .save_session(record)
                .map_err(crate::ipc::IpcError::internal)?;
            services
                .journal
                .complete(&device, &request, 204, OperationOutcome::NoContent)
                .map_err(crate::ipc::IpcError::internal)?;
            Ok(())
        })
        .await
        .map_err(|_| "OPERATION_OUTCOME_UNKNOWN".into())
    }

    pub(crate) fn retain_machine_request(
        &self,
    ) -> Result<Option<super::handover::RetirementGuard>, String> {
        self.handover_manager
            .upgrade()
            .map(|manager| manager.retain_request(self.terminal_service.clone()))
            .transpose()
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

    pub(crate) async fn resize_session(
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
        Self::checkpoint_remote_sessions(
            path,
            Arc::clone(&self.remote_persistence_lock),
            Arc::clone(self.terminal_service.remote()),
            Arc::clone(&self.session_metadata),
        )
        .await
    }

    /// Single ownership-agnostic checkpoint used by explicit saves and watcher
    /// checkpoints alike. The durable snapshot is ADDITIVE: prior records are
    /// preserved (a draining predecessor or a successor daemon may own sessions
    /// this runtime has never seen), locally owned records win on id conflict.
    /// Record removal happens only through `remove_persisted_remote_record`, the
    /// targeted teardown path with positive evidence that a session ended.
    async fn checkpoint_remote_sessions(
        path: PathBuf,
        lock: Arc<tokio::sync::Mutex<()>>,
        runtime: Arc<crate::terminal::remote::RemoteRuntime>,
        metadata: Arc<RwLock<HashMap<String, StoredSessionMeta>>>,
    ) -> Result<(), String> {
        let _guard = lock.lock().await;
        // Read failures must never become destructive writes: an unreadable or
        // mis-shaped durable snapshot aborts the checkpoint so the prior bytes
        // survive for diagnosis and the next attempt.
        //
        // The flock sidecar spans the whole read-modify-write: during a drain
        // chain two live daemons share one durable file, and the in-process tokio
        // mutex cannot serialize across processes.
        let sidecar = path.with_extension("lock");
        let read_path = path.clone();
        let (durable_records, _file_lock) = crate::ipc::run_blocking(move || {
            let file_lock =
                RemoteSnapshotFileLock::acquire(&sidecar).map_err(|error| {
                    crate::ipc::IpcError::internal(format!(
                        "Failed to lock durable remote snapshot: {error}"
                    ))
                })?;
            let records = {
                let Some(mut persisted) = load_session_from_path(&read_path)? else {
                    return Ok((Vec::new(), file_lock));
                };
                let value = persisted
                    .extra
                    .remove("remoteSessions")
                    .ok_or_else(|| {
                        crate::ipc::IpcError::internal(
                            "durable remote snapshot is missing the remoteSessions key",
                        )
                    })?;
                serde_json::from_value::<Vec<DurableRemoteSession>>(value).map_err(|error| {
                    crate::ipc::IpcError::internal(format!(
                        "durable remote snapshot failed to decode: {error}"
                    ))
                })?
            };
            Ok::<_, crate::ipc::IpcError>((records, file_lock))
        })
        .await
        .map_err(|error| error.to_string())?;
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
        let mut merged: std::collections::BTreeMap<String, DurableRemoteSession> =
            std::collections::BTreeMap::new();
        for record in durable_records {
            merged.insert(record.descriptor.backend_session_id.clone(), record);
        }
        for record in records {
            merged.insert(record.descriptor.backend_session_id.clone(), record);
        }
        let records: Vec<DurableRemoteSession> = merged.into_values().collect();
        crate::ipc::run_blocking(move || {
            // Hold the cross-process lock (acquired above, moved in here) across
            // the write so no other daemon interleaves a snapshot replacement.
            let _held = _file_lock;
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

    /// Targeted durable-record removal used by the session teardown path.
    /// Removal carries positive evidence that this exact session ended, so it
    /// deletes exactly one record and never rewrites predecessor/successor
    /// entries it knows nothing about.
    pub(super) async fn remove_persisted_remote_record(
        path: PathBuf,
        lock: Arc<tokio::sync::Mutex<()>>,
        session_id: String,
    ) -> Result<(), String> {
        let _guard = lock.lock().await;
        let sidecar = path.with_extension("lock");
        crate::ipc::run_blocking(move || {
            let _file_lock =
                RemoteSnapshotFileLock::acquire(&sidecar).map_err(|error| {
                    crate::ipc::IpcError::internal(format!(
                        "Failed to lock durable remote snapshot: {error}"
                    ))
                })?;
            let Some(mut persisted) = load_session_from_path(&path)? else {
                return Ok(());
            };
            let Some(value) = persisted.extra.remove("remoteSessions") else {
                return Ok(());
            };
            let mut records: Vec<DurableRemoteSession> =
                serde_json::from_value(value).map_err(|error| {
                    crate::ipc::IpcError::internal(format!(
                        "durable remote snapshot failed to decode: {error}"
                    ))
                })?;
            let before = records.len();
            records.retain(|record| record.descriptor.backend_session_id != session_id);
            if records.len() == before {
                return Ok(());
            }
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
        .map_err(|error| error.to_string())
    }

    pub(super) async fn restore_remote_sessions_at(&self, path: PathBuf) -> Result<(), String> {
        self.ensure_agent_sink();
        self.migrate_legacy_remote_persistence(&path).await?;
        let paired_path = path.with_file_name("paired_descriptors.json");
        self.terminal_service.paired().set_store_path(paired_path);
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

    /// One-time relocation of remote persistence out of the reboot-volatile runtime dir.
    ///
    /// `get_runtime_dir()` lives under `/tmp`, which the OS wipes on reboot: the daemon then
    /// restored zero remote descriptors even though the host-side helper daemons (and their
    /// PTYs) were still alive. The durable location is derived from the machine identity dir.
    /// Copy rather than move, so a predecessor daemon still reading the legacy file keeps it.
    async fn migrate_legacy_remote_persistence(&self, path: &Path) -> Result<(), String> {
        // Test builds run against isolated fixtures: never pull the developer's live
        // /tmp runtime state into them. The migration test points FERRYX_RUNTIME_DIR at
        // its own fake runtime directory, which is exactly this opt-in.
        #[cfg(test)]
        if std::env::var_os("FERRYX_RUNTIME_DIR").is_none() {
            return Ok(());
        }
        let _guard = self.remote_persistence_lock.lock().await;
        let legacy_dir = crate::daemon::get_runtime_dir();
        let targets = [
            (
                legacy_dir.join("remote_sessions.json"),
                path.to_path_buf(),
            ),
            (
                legacy_dir.join("paired_descriptors.json"),
                path.with_file_name("paired_descriptors.json"),
            ),
        ];
        let sidecar = path.with_extension("lock");
        crate::ipc::run_blocking(move || {
            // The flock sidecar serializes against successor checkpoints that
            // publish valid snapshots concurrently (handover boot overlap).
            let sidecar_lock =
                RemoteSnapshotFileLock::acquire(&sidecar).map_err(|error| {
                    crate::ipc::IpcError::internal(format!(
                        "Failed to lock durable remote persistence for migration: {error}"
                    ))
                })?;
            for (legacy, durable) in targets {
                if legacy == durable || !legacy.is_file() {
                    continue;
                }
                // A durable snapshot that parses is authoritative. An unparseable
                // leftover from an interrupted previous migration must not suppress
                // re-migration: after a reboot the volatile source may be all that
                // is left, and a partial file would parse as zero records.
                let durable_parses = std::fs::read(&durable)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .is_some();
                if durable_parses {
                    continue;
                }
                if let Some(parent) = durable.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        crate::ipc::IpcError::internal(format!(
                            "Failed to create durable remote persistence directory {}: {error}",
                            parent.display()
                        ))
                    })?;
                }
                // Stage, fsync, then atomically publish, so an interrupted copy can
                // never leave a half-written durable snapshot behind.
                let staging = durable.with_extension(format!(
                    "json.migrating-{}",
                    std::process::id()
                ));
                let publish = || -> std::io::Result<()> {
                    fs::copy(&legacy, &staging)?;
                    let staged = fs::File::open(&staging)?;
                    staged.sync_all()?;
                    drop(staged);
                    fs::rename(&staging, &durable)?;
                    // Complete the durable-publication sequence: the rename must be
                    // durable itself before the sidecar lock is released.
                    //
                    // The parent-directory fsync is unix-only: Windows does not permit
                    // opening a directory as a file, so `File::open(parent)` fails there
                    // and would turn an already-published rename into an error. On
                    // non-unix the rename is already ordered by the filesystem and the
                    // staged bytes were fsynced above.
                    #[cfg(unix)]
                    if let Some(parent) = durable.parent() {
                        fs::File::open(parent)?.sync_all()?;
                    }
                    Ok(())
                };
                publish().map_err(|error| {
                    let _ = fs::remove_file(&staging);
                    crate::ipc::IpcError::internal(format!(
                        "Failed to migrate {} to {}: {error}",
                        legacy.display(),
                        durable.display()
                    ))
                })?;
            }
            drop(sidecar_lock);
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())
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
        let cleanup_session_id = id.to_string();
        let mut record_removable = false;
        tokio::spawn(async move {
            loop {
                let details = rx.borrow_and_update().clone();
                // A naturally expired session keeps the subscription sender alive, so the
                // loop would never reach the teardown below without this explicit check.
                let (expired, vanished) = match runtime.upgrade() {
                    Some(runtime) => (
                        details.state
                            == crate::terminal::remote::RemoteConnectionState::Expired,
                        runtime.details(cleanup_session_id.as_str()).is_none(),
                    ),
                    // The runtime itself is gone (daemon teardown): keep durable
                    // records so a restart can restore them.
                    None => (false, false),
                };
                if expired || vanished {
                    record_removable = true;
                    break;
                }
                // Checkpoints share the same additive merge implementation as
                // explicit saves; a local-only snapshot here would erase durable
                // descriptors owned by other daemon generations.
                let Some(runtime_arc) = runtime.upgrade() else {
                    break;
                };
                if let Err(error) = Self::checkpoint_remote_sessions(
                    path.clone(),
                    Arc::clone(&lock),
                    runtime_arc,
                    Arc::clone(&metadata),
                )
                .await
                {
                    tracing::error!(%error, "Remote session checkpoint persistence failed");
                }
                let _ = tx.send(DaemonRemoteEvent { event: "terminal_remote_status".into(), payload: serde_json::json!({"sessionId":details.descriptor.backend_session_id,"state":details.state,"generation":details.generation,"failure":details.failure,"replayGap":details.replay_gap}) });
                if rx.changed().await.is_err() {
                    // The entry's sender dropped: either the session was closed out
                    // of a live runtime (remove its record), or the whole runtime is
                    // being torn down (keep records for restart).
                    record_removable = runtime
                        .upgrade()
                        .map(|runtime| runtime.details(cleanup_session_id.as_str()).is_none())
                        .unwrap_or(false);
                    break;
                }
            }
            // The remote session ended (or its runtime was dropped): a forwarding child must
            // never outlive its session, whether or not a close/hibernate path ran.
            if record_removable {
                if let Err(error) = Self::remove_persisted_remote_record(
                    path,
                    lock,
                    cleanup_session_id.clone(),
                )
                .await
                {
                    tracing::error!(%error, "Remote session record removal failed");
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
        let helper = crate::ipc::ssh::ensure_qualified_ssh_helper(&host, &environment)
            .await
            .map_err(|e| SpawnError::Other(e.to_string()))?;
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
        self.ensure_agent_sink();
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
            .map_err(|error| SpawnError::Other(error.to_string()))?;
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
    pub async fn handle_unregister_workspace(self: &Arc<Self>, workspace_id: &str) -> Result<(), String> {
        let service = self.clone();
        let workspace = workspace_id.to_owned();
        tokio::spawn(async move { service.unregister_workspace_owned(&workspace).await })
            .await.map_err(|error| error.to_string())?
    }

    async fn unregister_workspace_owned(&self, workspace_id: &str) -> Result<(), String> {
        let _spawn_guard = self.workspace_service.spawn_queue(workspace_id).lock_owned().await;
        // SSH owns a separate inventory, but still needs the session cleanup
        // below. Never send remote identities through the local catalog gate.
        if !crate::ssh::projects::is_remote(workspace_id) {
            let workspaces = Arc::clone(&self.workspace_service);
            let workspace = workspace_id.to_string();
            crate::ipc::run_blocking(move || {
                workspaces
                    .unregister(&workspace)
                    .map_err(crate::ipc::IpcError::internal)
            })
            .await
            .map_err(|error| error.to_string())?;
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
        self: &Arc<Self>,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
        requested_session_id: Option<String>,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        let machine = MACHINE_SPAWN.try_with(Arc::clone).ok();
        self.admit_spawn(client_request_id, workspace_id, worktree, cwd, cols, rows,
            shell, startup, requested_session_id, machine, None, #[cfg(test)] helper_home).await
    }

    async fn spawn_owned(
        self: &Arc<Self>,
        client_request_id: &str,
        workspace_id: &str,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
        requested_session_id: Option<String>,
        machine: Option<Arc<MachineSpawn>>,
        operation: Arc<Mutex<SpawnOperation>>,
        #[cfg(test)] helper_home: Option<String>,
    ) -> Result<String, SpawnError> {
        #[cfg(test)]
        self.split_test_stage(client_request_id, "ownerEntered").await;
        if let Some(machine) = &machine {
            (machine.check)()?;
        }
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

        #[cfg(test)]
        if machine.is_some() {
            let probe = self.workspace_service.transaction_probe.read().clone();
            if let Some(probe) = probe {
                probe("sessionBeforeSpawnGate");
            }
        }
        let deadline = operation.lock().deadline;
        let wait_started = Instant::now();
        tracing::info!(request_id = client_request_id, epoch = self.epoch, stage = "workspaceWait", boundary = "begin");
        let queue = self.workspace_service.spawn_queue(workspace_id);
        let mut acquisition = Box::pin(queue.lock_owned());
        #[cfg(test)]
        let acquisition = {
            use std::future::Future;
            use std::task::Poll;
            let probe = self.split_probe.read().clone();
            let mut reported = false;
            std::future::poll_fn(move |cx| {
                let polled = acquisition.as_mut().poll(cx);
                if polled.is_pending() && !reported {
                    reported = true;
                    if let Some(probe) = &probe { probe(client_request_id, "workspaceQueued"); }
                }
                match polled { Poll::Ready(guard) => Poll::Ready(guard), Poll::Pending => Poll::Pending }
            })
        };
        let _spawn_guard = if let Some(deadline) = deadline {
            tokio::time::timeout_at(
                tokio::time::Instant::from_std(deadline),
                acquisition,
            )
            .await
            .map_err(|_| SpawnError::Other("TIMEOUT".into()))?
        } else {
            acquisition.await
        };
        operation.lock().workspace_guard = Some(_spawn_guard);
        #[cfg(test)]
        self.split_test_stage(client_request_id, "workspaceAcquired").await;
        tracing::info!(request_id = client_request_id, epoch = self.epoch, stage = "workspaceWait", boundary = "end", elapsed_ms = wait_started.elapsed().as_millis() as u64);
        if let Some(machine) = &machine { (machine.check)()?; }
        Self::check_spawn_operation(&operation)?;
        if self.handover_manager.upgrade().is_none_or(|manager| manager.is_draining()) {
            return Err("HANDOVER_BUSY".into());
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

        let previous = if let Some(machine) = &machine {
            (machine.check)()?;
            let context = machine.clone();
            let workspaces = self.workspace_service.clone();
            crate::ipc::run_blocking(move || {
                let gate = workspaces.worktree_gate(&context.request.workspace_id);
                #[cfg(test)]
                if let Some(probe) = workspaces.transaction_probe.read().clone() { probe("sessionBeforeWorktreeGate"); }
                let _gate = gate.try_lock_until(context.deadline)
                    .ok_or_else(|| crate::ipc::IpcError::internal("TIMEOUT"))?;
                (context.check)().map_err(crate::ipc::IpcError::internal)?;
                let record = workspaces
                    .journal
                    .reconcile(&context.device, &context.request.request_id);
                Ok(record)
            })
            .await
            .map_err(|error| SpawnError::Other(error.message))?
        } else {
            Ok(None)
        };
        let now = Instant::now();
        if let Some(machine) = &machine {
            (machine.check)()?;
            if let Some(record) = previous? {
                if record.digest != machine.digest || record.kind != "createSession" {
                    return Err("REQUEST_CONFLICT".to_string().into());
                }
                return match record.operation {
                    crate::remote::machine_protocol::Operation::Completed {
                        outcome:
                            crate::remote::machine_protocol::OperationOutcome::Session { session },
                        ..
                    } => Ok(session.target.session_id),
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
            requested_session_id: requested_session_id.clone(),
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

        let candidates: Vec<_> = self.session_metadata.read().iter()
            .filter(|(_, meta)| meta.client_request_id == client_request_id)
            .map(|(id, meta)| (id.clone(), meta.spawn_fingerprint.clone())).collect();
        let existing_request = candidates.into_iter().find(|(id, _)| self.session_is_live(id));
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
        if let Some(machine) = &machine {
            (machine.check)()?;
        }
        let resume_startup = startup.clone();
        let machine_resume = machine.is_some();
        let resume_cwd = crate::ipc::run_blocking(move || {
            if machine_resume {
                return Ok(None);
            }
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
        let desktop_geometries = Arc::clone(&self.desktop_geometries);
        let handover_manager = self.handover_manager.clone();
        let machine_lifecycles = self.machine_lifecycles.clone();
        let client_request_id = client_request_id.to_string();
        if machine.is_some() {
            self.capacity_initialized.get_or_try_init(|| async {
                let workspaces = self.workspace_service.clone();
                let terminals = self.terminal_service.clone();
                let ids = crate::ipc::run_blocking(move || {
                    let records = workspaces.journal.sessions().map_err(crate::ipc::IpcError::internal)?;
                    Ok(records.into_iter().filter(|record| terminals.get_session(&record.session.target.session_id).is_some())
                        .map(|record| record.session.target.session_id).collect::<HashSet<_>>())
                }).await.map_err(|error| SpawnError::Other(error.to_string()))?;
                self.machine_capacity.lock().extend(ids);
                Ok::<(), SpawnError>(())
            }).await?;
        }
        // A GUI-minted id that is already known (live, exited-but-registered, remote)
        // or reserved by another in-flight spawn belongs to a different request:
        // same-request retries were answered by the idempotency checks above.
        let _requested_id_claim = match (&machine, &requested_session_id) {
            (None, Some(id)) => {
                let mut reserved = self.requested_session_ids.lock();
                if reserved.contains(id)
                    || self.terminal_service.get_session(id).is_some()
                    || self.terminal_service.remote().contains(id)
                    || self.session_metadata.read().contains_key(id)
                {
                    return Err(SpawnError::Structured(crate::ipc::IpcError::new(
                        crate::ipc::IpcErrorCode::SessionIdConflict,
                        format!("sessionId '{id}' already belongs to another terminal session"),
                    ).with_details(serde_json::json!({"sessionId": id, "requestId": client_request_id}))));
                }
                reserved.insert(id.clone());
                Some(RequestedSessionIdClaim { id: id.clone(), claims: self.requested_session_ids.clone() })
            }
            _ => None,
        };
        let raw_id = machine.as_ref().map(|machine| machine.target.session_id.clone())
            .or_else(|| requested_session_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let machine_capacity = self.machine_capacity.clone();
        let pending_claims = self.pending_provider_claims.clone();
        let max_machine_sessions = self.max_machine_sessions();
        let reservation = SpawnReservation::reserve(raw_id.clone(), spawn_claim.clone(),
            machine.is_some(), claims.clone(), pending_claims.clone(), machine_capacity.clone(), max_machine_sessions)?;
        let (deadline, cancelled, cancellation, reliable) = {
            let record = operation.lock();
            (record.deadline, record.cancelled.clone(), record.cancellation.clone(), record.envelope.is_some())
        };
        let epoch = self.epoch;
        #[cfg(test)]
        let split_probe = self.split_probe.read().clone();
        #[cfg(test)]
        let git_observer = self.split_git_observer.read().clone();
        crate::ipc::run_blocking(move || {
            let workspace_gate = workspace_service.worktree_gate(&workspace_id_owned);
            let fence_started = Instant::now();
            tracing::info!(request_id = %client_request_id, epoch, stage = "workspaceFence", boundary = "begin");
            let _workspace_gate = match deadline {
                Some(deadline) => workspace_gate
                    .try_lock_until(deadline)
                    .ok_or_else(|| crate::ipc::IpcError::internal("TIMEOUT"))?,
                None => workspace_gate.lock(),
            };
            tracing::info!(request_id = %client_request_id, epoch, stage = "workspaceFence", boundary = "end", elapsed_ms = fence_started.elapsed().as_millis() as u64);
            let mut reservation = reservation;
            let preparation_started = Instant::now();
            tracing::info!(request_id = %client_request_id, epoch, stage = "preparation", boundary = "begin");
            let mut work = || -> Result<_, SpawnError> {
                if let Some(machine) = &machine {
                    (machine.check)()?;
                }
                Self::check_spawn_operation(&operation)?;
                #[cfg(test)]
                if let Some(probe) = &split_probe { probe(&client_request_id, "preparation"); }
                // Machine roots stay out of the shared mirror registry.
                let (mgr, default_cwd) = if machine.is_some() {
                    let manager = workspace_service.worktree_manager(&workspace_id_owned, false)?;
                    let private_registry = crate::worktree::WorkspaceRegistry::new();
                    private_registry.publish(workspace_id_owned.clone(), manager);
                    private_registry
                        .resolve_terminal_target(&workspace_id_owned, spawn_worktree.as_ref())
                } else {
                    workspace_service
                        .registry
                        .resolve_terminal_target(&workspace_id_owned, spawn_worktree.as_ref())
                }
                .map_err(|e| {
                    if machine.is_some()
                        && matches!(
                            e,
                            crate::worktree::WorktreeError::WorktreeIdentityNotFound { .. }
                        )
                    {
                        SpawnError::Other("WORKTREE_NOT_FOUND".into())
                    } else if reliable {
                        SpawnError::Structured(e.into())
                    } else {
                        SpawnError::Other(e.to_string())
                    }
                })?;

                let cwd = if let Some(machine) = &machine {
                    if let Some(parent) = &machine.request.inherit_from_session_id {
                        let record = workspace_service
                            .journal
                            .session(parent)?
                            .ok_or("SESSION_NOT_FOUND".to_string())?;
                        if record.session.target.daemon_epoch != machine.target.daemon_epoch
                            || record.session.target.machine_id != machine.target.machine_id
                            || record.session.workspace_id != workspace_id_owned
                            || record.session.worktree != machine.request.worktree
                        {
                            return Err("PARENT_SESSION_MISMATCH".to_string().into());
                        }
                        let parent = terminal_service
                            .get_session(parent)
                            .ok_or("SESSION_EXPIRED".to_string())?;
                        if !matches!(parent.state(), PtySessionState::Running) {
                            return Err("SESSION_EXPIRED".to_string().into());
                        }
                        let pid = parent.pid().ok_or("SESSION_EXPIRED".to_string())?;
                        Some(
                            crate::ipc::terminal::process_cwd(pid)
                                .ok_or("CWD_UNAVAILABLE".to_string())?
                                .to_str()
                                .ok_or("INVALID_PATH".to_string())?
                                .to_owned(),
                        )
                    } else {
                        machine.request.cwd_relative.as_ref().map(|relative| {
                            let rel_norm = relative.replace('\\', "/");
                            if let Some(ref w) = spawn_worktree {
                                let expected_wt_rel = format!(".orca-worktrees/wt-{}", w.slug);
                                if rel_norm == expected_wt_rel {
                                    default_cwd.to_string_lossy().into_owned()
                                } else if let Some(sub) =
                                    rel_norm.strip_prefix(&format!("{}/", expected_wt_rel))
                                {
                                    default_cwd.join(sub).to_string_lossy().into_owned()
                                } else if default_cwd
                                    .to_string_lossy()
                                    .replace('\\', "/")
                                    .ends_with(&rel_norm)
                                {
                                    default_cwd.to_string_lossy().into_owned()
                                } else {
                                    default_cwd.join(relative).to_string_lossy().into_owned()
                                }
                            } else if default_cwd
                                .to_string_lossy()
                                .replace('\\', "/")
                                .ends_with(&rel_norm)
                            {
                                default_cwd.to_string_lossy().into_owned()
                            } else {
                                default_cwd.join(relative).to_string_lossy().into_owned()
                            }
                        })
                    }
                } else {
                    cwd
                };
                let resume_cwd = if machine.is_some() {
                    if let Some(TerminalStartup::AgentResume {
                        agent_type,
                        provider_session,
                    }) = &spawn_startup
                    {
                        let plan = crate::terminal::shell::resolve_agent_resume_plan(
                            agent_type,
                            provider_session,
                        )
                        .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        if crate::ipc::agents::resolve_binary(
                            &plan.program,
                            &crate::ipc::agents::search_paths(),
                        )
                        .is_none()
                        {
                            return Err("AGENT_RESUME_UNSUPPORTED".to_string().into());
                        }
                    }
                    crate::terminal::resume_cwd::resolve_agent_resume_cwd(spawn_startup.as_ref())
                        .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?
                } else {
                    resume_cwd
                };
                if machine.is_some() {
                    if let Some(resume) = &resume_cwd {
                        let requested = cwd
                            .as_ref()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| default_cwd.clone());
                        let requested = fs::canonicalize(requested)
                            .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        let resume = fs::canonicalize(resume)
                            .map_err(|e| SpawnError::InvalidAgentResume(e.to_string()))?;
                        if resume != requested {
                            return Err(SpawnError::InvalidAgentResume(
                                "Provider CWD does not match the selected target".into(),
                            ));
                        }
                    }
                }
                let cwd = resume_cwd
                    .map(|path| path.to_string_lossy().into_owned())
                    .or(cwd);
                let selected_root = default_cwd.clone();
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

                let mut cmd = if reliable {
                    crate::terminal::shell::resolve_ordinary_shell_command(shell.as_deref())
                        .map_err(|err| SpawnError::Structured(err.into()))?
                } else {
                    crate::terminal::shell::resolve_startup_command(
                        shell.as_deref(),
                        spawn_startup.as_ref(),
                    )
                    .map_err(|err| SpawnError::InvalidAgentResume(err.to_string()))?
                };
                cmd.env("PROMPT_EOL_MARK", "");
                cmd.cwd(normalize_process_cwd(&resolved_cwd));

                if let Some(machine) = &machine {
                    (machine.check)()?;
                    Self::check_spawn_operation(&operation)?;
                    reservation.retain_capacity = true;
                    #[cfg(test)]
                    if let Some(probe) = workspace_service.transaction_probe.read().clone() { probe("sessionBeforeIntent"); }
                    match workspace_service.journal.begin(
                        &machine.device,
                        &machine.request.request_id,
                        "createSession",
                        &machine.digest,
                        &serde_json::to_string(&machine.target).expect("target"),
                    )? {
                        crate::remote::machine_operation_journal::Begin::New => {}
                        crate::remote::machine_operation_journal::Begin::Existing(_) => {
                            return Err("OPERATION_OUTCOME_UNKNOWN".to_string().into())
                        }
                    }
                }
                #[cfg(test)]
                if machine.is_some() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe {
                        probe("sessionIntent");
                    }
                }
                if let Some(machine) = &machine { (machine.check)()?; }
                #[cfg(test)]
                if let Some(probe) = &split_probe { probe(&client_request_id, "preChild"); }
                Self::check_spawn_operation(&operation)?;
                tracing::info!(request_id = %client_request_id, epoch, stage = "preparation", boundary = "end", elapsed_ms = preparation_started.elapsed().as_millis() as u64);
                let pty_started = Instant::now();
                tracing::info!(request_id = %client_request_id, epoch, stage = "ptySpawn", boundary = "begin");
                let spawned = if reliable {
                    terminal_service.spawn_resolved_with_id(raw_id.clone(), cmd, cols, rows,
                        crate::terminal::pty::ResolvedSpawnContext {
                            root: selected_root, cwd: resolved_cwd.clone(),
                            managed_workspace_id: spawn_worktree.as_ref().map(|worktree| worktree.ws_id.clone()),
                        })
                } else {
                    terminal_service.spawn_in_worktree_with_id(raw_id.clone(), cmd, cols, rows, &mgr, &resolved_cwd)
                };
                let (session_id, mut lifecycle_rx) = spawned
                    .map_err(|e| SpawnError::Other(e.to_string()))?;
                operation.lock().child = Some(session_id.clone());
                reservation.retain_capacity = true;
                reservation.published = true;
                #[cfg(test)]
                if let Some(probe) = &split_probe { probe(&client_request_id, "childCreated"); }
                tracing::info!(request_id = %client_request_id, epoch, session_id = %session_id, stage = "ptySpawn", boundary = "end", elapsed_ms = pty_started.elapsed().as_millis() as u64);
                let publication_started = Instant::now();
                tracing::info!(request_id = %client_request_id, epoch, stage = "publication", boundary = "begin");
                #[cfg(test)]
                if machine.is_some() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe {
                        probe("sessionSpawned");
                    }
                }
                let (start_sequence, end_sequence) = terminal_service
                    .output_hub()
                    .session_sequence_range(&session_id)
                    .unwrap_or_default();
                let durable = machine.as_ref().map(|machine| {
                    crate::remote::machine_operation_journal::MachineSession {
                        creator_device: machine.device.clone(),
                        session: crate::remote::machine_protocol::Session {
                            target: machine.target.clone(),
                            workspace_id: workspace_id_owned.clone(),
                            worktree: machine.request.worktree.clone(),
                            cwd: resolved_cwd.to_string_lossy().into_owned(),
                            cols,
                            rows,
                            running: true,
                            title: None,
                            agent_type: match &machine.request.startup {
                                crate::remote::machine_protocol::Startup::Shell => None,
                                crate::remote::machine_protocol::Startup::AgentResume {
                                    agent_type,
                                    ..
                                } => Some(agent_type.clone()),
                            },
                            provider_session: match &machine.request.startup {
                                crate::remote::machine_protocol::Startup::Shell => None,
                                crate::remote::machine_protocol::Startup::AgentResume {
                                    provider_session,
                                    ..
                                } => Some(provider_session.clone()),
                            },
                            start_sequence: crate::scoped_contracts::Epoch(
                                start_sequence.unwrap_or(0),
                            ),
                            end_sequence: crate::scoped_contracts::Epoch(end_sequence.unwrap_or(0)),
                        },
                        exit: None,
                    }
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
                reservation.published = true;

                let cleanup_session_id = session_id.clone();
                let cleanup_pending_claims = pending_claims.clone();
                let cleanup_router = Arc::clone(&session_router);
                let cleanup_cache = Arc::clone(&spawn_idempotency_cache);
                let cleanup_metadata = Arc::clone(&session_metadata);
                let cleanup_claims = Arc::clone(&provider_session_claims);
                let cleanup_desktop_geometries = Arc::clone(&desktop_geometries);
                let cleanup_agent_states = Arc::clone(&agent_states);
                let handover_manager = handover_manager.clone();
                let terminal_service = Arc::clone(&terminal_service);
                let exited_pty = terminal_service.get_session(&session_id);
                let lifecycle_workspaces = workspace_service.clone();
                let durable_exit = durable.clone();
                let metadata_target = durable.as_ref().map(|record| record.session.target.clone());
                let lifecycle_done = {
                    let (done, receiver) = tokio::sync::watch::channel(false);
                    machine_lifecycles
                        .lock()
                        .insert(session_id.clone(), receiver);
                    Some(done)
                };
                // Persist before starting exit publication, so a fast exit cannot be overwritten
                // by the initial running record. Even on failure, install lifecycle cleanup.
                let persisted = match (&machine, durable) {
                    (Some(machine), Some(record)) => {
                        let result = workspace_service.journal.commit_spawn(
                            &machine.device,
                            &machine.request.request_id,
                            record.clone(),
                        );
                        if result.is_ok() {
                            workspace_service.machine_events.publish(
                                "sessionStarted",
                                Some(&record.session.workspace_id),
                                Some(&record.session.target.session_id),
                                serde_json::json!(record.session),
                            );
                        }
                        result
                    }
                    _ => {
                        if let Ok(catalog) = workspace_service.catalog() {
                            if catalog.workspaces.contains_key(&workspace_id_owned)
                                && !crate::ssh::projects::is_remote(&workspace_id_owned)
                                && !workspace_id_owned.starts_with("ssh:")
                                && !workspace_id_owned.contains("::")
                            {
                                workspace_service.machine_events.publish(
                                    "sessionStarted",
                                    Some(&workspace_id_owned),
                                    Some(&session_id),
                                    serde_json::json!({
                                        "sessionId": session_id,
                                        "workspaceId": workspace_id_owned,
                                    }),
                                );
                            }
                        }
                        Ok(())
                    }
                };
                if persisted.is_ok() {
                    if let Some(claim) = &spawn_claim {
                        let mut pending = pending_claims.lock();
                        if pending.get(claim) == Some(&session_id) { pending.remove(claim); }
                    }
                }
                #[cfg(test)]
                if machine.is_some() && persisted.is_ok() {
                    let probe = workspace_service.transaction_probe.read().clone();
                    if let Some(probe) = probe {
                        probe("sessionCommitted");
                    }
                }
                let metadata_task = if persisted.is_ok() {
                    metadata_target
                        .map(|target| {
                            session_metadata_events::MetadataOwner {
                                workspaces: workspace_service.clone(),
                                terminals: terminal_service.clone(),
                                metadata: session_metadata.clone(),
                            }
                            .subscribe(target)
                        })
                        .transpose()
                } else {
                    Ok(None)
                };
                let cleanup_operation = operation.clone();
                let cleanup_capacity = machine_capacity.clone();
                tokio::spawn(async move {
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] waiting for PTY lifecycle change");
                    let _ = lifecycle_rx.changed().await;
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] PTY lifecycle change received");
                    match metadata_task {
                        Ok(Some(task)) => {
                            if let Err(error) = task.await {
                                tracing::warn!(%error, "Machine metadata task failed");
                            }
                        }
                        Ok(None) => {}
                        Err(error) => {
                            tracing::warn!(%error, "Machine metadata subscription failed")
                        }
                    }
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] metadata task complete");
                    let is_gui = durable_exit.is_none();
                    if let Some(mut record) = durable_exit {
                        record.session.running = false;
                        record.exit = Some(crate::remote::machine_protocol::ExitMetadata {
                            code: exited_pty.as_ref().and_then(|p| match p.state() {
                                PtySessionState::Exited { code } => code,
                                _ => None,
                            }),
                            signal: None,
                        });
                        let persistence_workspaces = Arc::clone(&lifecycle_workspaces);
                        if let Err(error) = crate::ipc::run_blocking(move || {
                            persistence_workspaces
                                .journal
                                .save_session(record.clone())
                                .map_err(crate::ipc::IpcError::internal)?;
                            let record = persistence_workspaces
                                .journal
                                .session(&record.session.target.session_id)
                                .map_err(crate::ipc::IpcError::internal)?
                                .ok_or_else(|| {
                                    crate::ipc::IpcError::internal("SESSION_NOT_FOUND")
                                })?;
                            persistence_workspaces.machine_events.publish(
                                "sessionExited",
                                Some(&record.session.workspace_id),
                                Some(&record.session.target.session_id),
                                serde_json::json!({"session":record.session,"exit":record.exit}),
                            );
                            Ok(())
                        })
                        .await
                        {
                            tracing::error!(%error, "Machine exit persistence failed");
                            let mut operation = cleanup_operation.lock();
                            operation.state = SplitOperationResult::Unknown { reason: SplitUnknownReason::PublicationUncertain };
                            operation.changed.send_modify(|revision| *revision += 1);
                            if let Some(done) = lifecycle_done { done.send_replace(true); }
                            return;
                        }
                    }
                    cleanup_router.remove_workspace(&cleanup_session_id);
                    cleanup_agent_states.remove(&cleanup_session_id);
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] router cleanup complete");
                    cleanup_cache
                        .lock()
                        .retain(|_, entry| entry.session_id != cleanup_session_id);
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] cache cleanup complete");
                    let removed_meta = cleanup_metadata.write().remove(&cleanup_session_id);
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] metadata removed");
                    if let Some(meta) = removed_meta {
                        if is_gui {
                            let exit_workspaces = Arc::clone(&lifecycle_workspaces);
                            let workspace_id = meta.workspace_id.clone();
                            let event_session_id = cleanup_session_id.clone();
                            #[cfg(test)]
                            eprintln!("[session_exit:{cleanup_session_id}] starting GUI exit catalog/event task");
                            if let Err(error) = crate::ipc::run_blocking(move || {
                                let catalog = exit_workspaces
                                    .catalog()
                                    .map_err(crate::ipc::IpcError::internal)?;
                                if catalog.workspaces.contains_key(&workspace_id)
                                    && !crate::ssh::projects::is_remote(&workspace_id)
                                    && !workspace_id.starts_with("ssh:")
                                    && !workspace_id.contains("::")
                                {
                                    exit_workspaces.machine_events.publish(
                                        "sessionExited",
                                        Some(&workspace_id),
                                        Some(&event_session_id),
                                        serde_json::json!({
                                            "sessionId": event_session_id,
                                            "workspaceId": workspace_id,
                                        }),
                                    );
                                }
                                Ok(())
                            })
                            .await
                            {
                                tracing::error!(%error, session_id = %cleanup_session_id, "Desktop session exit event publication failed");
                            } else {
                                #[cfg(test)]
                                eprintln!("[session_exit:{cleanup_session_id}] GUI exit catalog/event task complete");
                            }
                        }
                        #[cfg(test)]
                        eprintln!("[session_exit:{cleanup_session_id}] metadata branch complete");
                        if let Some(claim) = meta.provider_claim {
                            cleanup_claims
                                .lock()
                                .retain(|key, owner| key != &claim || owner != &cleanup_session_id);
                            cleanup_pending_claims.lock()
                                .retain(|key, owner| key != &claim || owner != &cleanup_session_id);
                        }
                    }
                    cleanup_desktop_geometries
                        .lock()
                        .remove(&cleanup_session_id);
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] geometry cleanup complete");
                    if let Some(manager) = handover_manager.upgrade() {
                        manager.check_retirement_if_empty(&terminal_service);
                    }
                    #[cfg(test)]
                    eprintln!("[session_exit:{cleanup_session_id}] retirement check complete");
                    if let Some(done) = lifecycle_done {
                        #[cfg(test)]
                        eprintln!("[session_exit:{cleanup_session_id}] signaling lifecycle completion");
                        cleanup_capacity.lock().remove(&cleanup_session_id);
                        let mut operation = cleanup_operation.lock();
                        operation.state = if operation.cancelled.load(Ordering::Acquire) {
                            SplitOperationResult::Cancelled
                        } else { SplitOperationResult::Exited };
                        operation.changed.send_modify(|revision| *revision += 1);
                        done.send_replace(true);
                        machine_lifecycles.lock().remove(&cleanup_session_id);
                        #[cfg(test)]
                        eprintln!("[session_exit:{cleanup_session_id}] cleanup complete");
                    }
                });

                tracing::info!(request_id = %client_request_id, epoch, stage = "publication", boundary = "end", elapsed_ms = publication_started.elapsed().as_millis() as u64, certain = persisted.is_ok());
                #[cfg(test)]
                if let Some(probe) = &split_probe { probe(&client_request_id, "published"); }
                persisted?;
                Ok(session_id)
            };
            #[cfg(test)]
            let mut work = || match git_observer {
                Some(observer) => crate::worktree::git::with_git_observer(observer, work),
                None => work(),
            };
            let result = match deadline {
                Some(deadline) => crate::worktree::git::with_preparation_git_budget(deadline, cancelled, Some(cancellation), work),
                None => work(),
            };
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

    pub(super) async fn handle_suspend(
        &self,
        session_id: &str,
    ) -> Result<(), crate::terminal::PtyError> {
        self.terminal_service.suspend_session(session_id).await
    }

    pub(super) async fn handle_resume(
        &self,
        session_id: &str,
    ) -> Result<(), crate::terminal::PtyError> {
        self.terminal_service.resume_session(session_id).await
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
        self.desktop_geometries.lock().remove(session_id);
        self.spawn_idempotency_cache
            .lock()
            .retain(|_, entry| entry.session_id != session_id);
        if let Some(meta) = self.session_metadata.write().remove(session_id) {
            if let Some(claim) = meta.provider_claim {
                self.provider_session_claims
                    .lock()
                    .retain(|key, owner| key != &claim || owner != session_id);
                self.pending_provider_claims.lock()
                    .retain(|key, owner| key != &claim || owner != session_id);
            }
        }
    }

    pub(super) fn prune_dead_spawn_ownership(&self, now: Instant) {
        let cached: Vec<_> = self.spawn_idempotency_cache.lock().iter()
            .map(|(key, entry)| (key.clone(), entry.clone())).collect();
        for (key, entry) in cached {
            if now.duration_since(entry.created_at) > SPAWN_REQUEST_TTL || !self.session_is_live(&entry.session_id) {
                let mut cache = self.spawn_idempotency_cache.lock();
                if cache.get(&key).is_some_and(|current| current.session_id == entry.session_id
                    && current.created_at == entry.created_at && current.fingerprint == entry.fingerprint) {
                    cache.remove(&key);
                }
            }
        }
        let claims: Vec<_> = self.provider_session_claims.lock().iter()
            .map(|(key, id)| (key.clone(), id.clone())).collect();
        for (key, id) in claims {
            if !self.session_is_live(&id) {
                let mut claims = self.provider_session_claims.lock();
                let pending = self.pending_provider_claims.lock();
                if claims.get(&key) == Some(&id) && pending.get(&key) != Some(&id) {
                    claims.remove(&key);
                }
            }
        }
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
                    requested_session_id: None,
                },
            },
        );
        Ok(session_id.to_string())
    }

    pub(super) async fn describe_session_until(&self, session_id: &str, deadline: tokio::time::Instant) -> DaemonResponse {
        let mut response = self.handle_describe_session(session_id);
        if self.terminal_service.remote().contains(session_id)
            || super::super::terminal::paired_runtime::Runtime::owns(session_id) { return response; }
        let DaemonResponse::DescribeSessionOk { session } = &mut response else { return response; };
        let fallback = session.cwd.take();
        let Some(permit) = self.terminal_service.try_acquire_cwd_probe(session_id) else { return response; };
        let pty = self.terminal_service.get_session(session_id);
        let pid = pty.as_ref().and_then(|pty| pty.pid());
        let worktree = pty.and_then(|pty| pty.worktree_path());
        let workspace = session.workspace_id.clone();
        let identity = session.worktree.clone();
        let workspaces = self.workspace_service.clone();
        let preparation_deadline = deadline.into_std();
        let (latest, validated) = tokio::sync::watch::channel(None);
        #[cfg(test)]
        let probe = self.cwd_probe.read().clone();
        #[cfg(test)]
        let completed = {
            let (completed, receiver) = tokio::sync::oneshot::channel();
            self.split_probe_workers.lock().push(receiver);
            completed
        };
        let worker = crate::ipc::run_blocking(move || {
            let _permit = permit;
            let validate = |path: PathBuf| {
                if crate::ipc::terminal::is_plausible_absolute_cwd(&path) && path.is_dir() {
                    Some(path.to_string_lossy().into_owned())
                } else { None }
            };
            let fallback = fallback.map(PathBuf::from).and_then(&validate)
                .or_else(|| worktree.and_then(&validate))
                .or_else(|| workspace.and_then(|workspace| {
                    crate::worktree::git::with_preparation_git_budget(preparation_deadline,
                        Arc::new(AtomicBool::new(false)), None, || {
                            workspaces.registry.resolve_terminal_target(&workspace, identity.as_ref())
                                .ok().and_then(|(_, root)| validate(root))
                        })
                }));
            latest.send_replace(fallback.clone());
            let cwd = pid.and_then(|pid| {
                #[cfg(test)]
                if let Some(probe) = &probe { return probe(pid); }
                crate::ipc::terminal::process_cwd(pid)
            }).and_then(validate).or(fallback);
            latest.send_replace(cwd.clone());
            drop(_permit);
            #[cfg(test)]
            if completed.send(()).is_err() { tracing::debug!("Describe fixture receiver dropped"); }
            Ok(cwd)
        });
        session.cwd = match tokio::time::timeout_at(deadline, worker).await {
            Ok(Ok(cwd)) => cwd,
            _ => validated.borrow().clone(),
        };
        response
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
                            .filter(|cwd| {
                                crate::ipc::terminal::is_plausible_session_cwd_text(cwd)
                            })
                            .unwrap_or(d.config.project_path),
                    ),
                    cols: d.cols,
                    rows: d.rows,
                    running: details.state
                        == crate::terminal::remote::RemoteConnectionState::Connected,
                    start_sequence,
                    end_sequence,
                    last_output_age_ms: None,
                    suspended: false,
                },
            };
        }
        if super::super::terminal::paired_runtime::Runtime::owns(session_id) {
            if let Some(_descriptor) = self.terminal_service.paired().descriptor(session_id) {
                let (start_sequence, end_sequence) = self
                    .terminal_service
                    .output_hub()
                    .session_sequence_range(session_id)
                    .unwrap_or((None, None));
                let running = self.terminal_service.paired().contains(session_id);
                return DaemonResponse::DescribeSessionOk {
                    session: DaemonSessionDetails {
                        session_id: session_id.into(),
                        workspace_id: None,
                        worktree: None,
                        cwd: None,
                        cols: 80,
                        rows: 24,
                        running,
                        start_sequence,
                        end_sequence,
                        last_output_age_ms: None,
                        suspended: false,
                    },
                };
            }
        }
        let Some(pty_session) = self.terminal_service.get_session(session_id) else {
            return DaemonResponse::Error {
                message: format!("Session '{session_id}' not found"),
                code: Some("SESSION_NOT_FOUND".to_string()),
                details: Some(serde_json::json!({
                    "source": "daemon_describe",
                    "kind": "session_not_found",
                    "sessionId": session_id,
                })),
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
        let worktree_cwd = pty_session
            .worktree_path()
            .map(|p| p.to_string_lossy().to_string());
        let (workspace_id, worktree, cwd) = match meta {
            Some(m) => (
                Some(m.workspace_id),
                m.worktree,
                serveable_local_cwd(&m.cwd, worktree_cwd),
            ),
            None => (None, None, worktree_cwd),
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
                last_output_age_ms: pty_session.last_output_age_ms(),
                // Unix answers from the kernel; Windows has no queryable NtSuspendProcess
                // state, so it falls back to the daemon's own lifecycle record.
                suspended: pty_session.process_stopped()
                    || (cfg!(windows)
                        && self.terminal_service.process_state(session_id)
                            == Some(crate::daemon::session_lifecycle::SessionProcessState::Suspended)),
            },
        }
    }
}
