//! In-memory native relay ownership. No PTY, shell, or SSH launcher exists here.
use super::paired_daemon::Proxy;
use crate::scoped_contracts::Epoch;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

fn default_descriptors_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("FERRYX_PAIRED_DESCRIPTORS_DIR") {
        return Some(PathBuf::from(dir).join("paired_descriptors.json"));
    }
    #[cfg(not(test))]
    {
        crate::remote::auth::canonical_remote_dir().map(|dir| dir.join("paired_descriptors.json"))
    }
    #[cfg(test)]
    {
        None
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FsFault {
    None,
    FailTempWrite,
    FailRename,
}

#[cfg(test)]
thread_local! {
    static TEST_FS_FAULT: std::cell::Cell<FsFault> = const { std::cell::Cell::new(FsFault::None) };
}

#[cfg(test)]
impl FsFault {
    pub(crate) fn set(fault: Self) {
        TEST_FS_FAULT.with(|f| f.set(fault));
    }
    pub(crate) fn get() -> Self {
        TEST_FS_FAULT.with(|f| f.get())
    }
}

fn fs_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailTempWrite {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "injected temp write failure",
        ));
    }
    std::fs::write(path, bytes)
}

fn fs_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if FsFault::get() == FsFault::FailRename {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "injected rename failure",
        ));
    }
    std::fs::rename(from, to)
}

fn load_descriptors(
    path: &Path,
) -> Result<HashMap<String, super::paired_daemon::Descriptor>, String> {
    // N2 (round 2): fail closed on load. A read or parse failure must NOT be
    // silently converted into an empty map — that would let a later save
    // clobber a corrupt-but-recoverable store with only the live memory.
    match std::fs::read(path) {
        Ok(data) => {
            serde_json::from_slice(&data).map_err(|e| format!("PAIRED_DESCRIPTOR_PARSE: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(format!("PAIRED_DESCRIPTOR_READ: {e}")),
    }
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PERSIST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
const PERSIST_MIN_INTERVAL: Duration = Duration::from_millis(100);
const RETRY_ADMISSION_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
static TEST_PERSIST_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static TEST_PERSIST_MAP: parking_lot::Mutex<Option<HashMap<PathBuf, usize>>> =
    parking_lot::Mutex::new(None);

#[cfg(test)]
pub(crate) fn test_persist_count() -> usize {
    TEST_PERSIST_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub(crate) fn test_persist_count_path(path: &Path) -> usize {
    TEST_PERSIST_MAP
        .lock()
        .as_ref()
        .and_then(|m| m.get(path).copied())
        .unwrap_or(0)
}

fn save_descriptors_locked(
    path: &Path,
    descriptors: &HashMap<String, super::paired_daemon::Descriptor>,
) -> Result<(), String> {
    #[cfg(test)]
    {
        TEST_PERSIST_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut map = TEST_PERSIST_MAP.lock();
        let m = map.get_or_insert_with(HashMap::new);
        *m.entry(path.to_path_buf()).or_insert(0) += 1;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let bytes = match serde_json::to_vec_pretty(descriptors) {
        Ok(bytes) => bytes,
        Err(e) => return Err(format!("PAIRED_DESCRIPTOR_SERIALIZE: {e}")),
    };
    // N2 & R3-N1: unique temp file per write and global mutex serialize disk replacement
    let tmp_seq = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), tmp_seq));
    fs_write(&tmp, &bytes).map_err(|e| format!("PAIRED_DESCRIPTOR_TEMP_WRITE: {e}"))?;
    if let Ok(f) = std::fs::File::open(&tmp) {
        let _ = f.sync_all();
    }
    if let Err(e) = fs_rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PAIRED_DESCRIPTOR_REPLACE: {e}"));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(windows)]
    {
        let _ = std::fs::File::open(path).and_then(|f| f.sync_all());
    }
    Ok(())
}

#[allow(dead_code)]
fn save_descriptors(
    path: &Path,
    descriptors: &HashMap<String, super::paired_daemon::Descriptor>,
) -> Result<(), String> {
    let _persist_guard = PERSIST_MUTEX.lock().map_err(|e| e.to_string())?;
    save_descriptors_locked(path, descriptors)
}

type Reply = oneshot::Sender<Result<(), String>>;
enum Command {
    Write(u64, Vec<u8>, Reply),
    Resize(u64, u16, u16, Reply),
    Interrupt(u64, Option<String>, Reply),
    Detach(Reply),
    Rearm(Reply),
    RetryOwned { daemon_epoch: String, incarnation: String, reply: Reply },
    Status(oneshot::Sender<(String, Option<Epoch>, Option<i32>, Option<String>)>),
    #[cfg(feature = "local-split-qa")]
    QaDropLoopback(Reply),
    #[cfg(feature = "local-split-qa")]
    QaCancelDial(Reply),
    #[cfg(feature = "local-split-qa")]
    QaRouteReceipt(oneshot::Sender<Result<super::paired_daemon::PairedRouteReceipt, String>>),
    #[cfg(feature = "local-split-qa")]
    QaDialAudit(oneshot::Sender<PairedDialAudit>),
    #[cfg(test)]
    QaBlockRecovery(Reply),
    #[cfg(test)]
    QaReleaseRecovery(Reply),
    #[cfg(test)]
    Block(oneshot::Sender<()>, oneshot::Receiver<()>),
    #[cfg(test)]
    HoldRecovery(oneshot::Sender<()>, oneshot::Receiver<()>),
    #[cfg(test)]
    RecoveryScheduled(oneshot::Sender<bool>),
    #[cfg(test)]
    SubscribeRecoveryEvents(oneshot::Sender<tokio::sync::broadcast::Receiver<RecoveryTestEvent>>),
}
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryTestEvent {
    Scheduled,
    Completed { failed: bool },
    /// One relay->direct upgrade attempt finished.
    Upgraded { epoch: u64, direct: bool },
}
struct Owner {
    sender: mpsc::Sender<Command>,
    task: tokio::task::JoinHandle<()>,
    identity: Arc<()>,
    target: crate::remote::machine_protocol::RemoteTerminalTarget,
    /// Lifetime incarnation of this proxy actor, minted by the runtime at install time.
    ///
    /// Constant for as long as this actor owns the id; a replacement actor (a reattach, or a
    /// reincarnation after the previous actor died) mints a different one.
    incarnation: String,
    /// Live identity the actor republishes on every loop turn.
    live: Arc<Mutex<super::paired_daemon::PairedLive>>,
    cancel: tokio::sync::watch::Sender<bool>,
    status: Arc<Mutex<(String, Option<Epoch>, Option<i32>, Option<String>)>>,
    outcome: tokio::sync::watch::Receiver<PairedRecoveryOutcome>,
    #[cfg(test)]
    completed: tokio::sync::watch::Receiver<bool>,
}

struct ActorPersistGuard {
    descriptors: Arc<Mutex<HashMap<String, super::paired_daemon::Descriptor>>>,
    store_path: Arc<Mutex<Option<PathBuf>>>,
    dirty: Arc<AtomicBool>,
}

impl Drop for ActorPersistGuard {
    fn drop(&mut self) {
        if !self.dirty.load(Ordering::Acquire) {
            return;
        }
        let descriptors = self.descriptors.clone();
        let store_path = self.store_path.clone();
        tokio::task::spawn_blocking(move || persist_snapshot(&descriptors, &store_path));
    }
}

fn persist_snapshot(
    descriptors: &Mutex<HashMap<String, super::paired_daemon::Descriptor>>,
    store_path: &Mutex<Option<PathBuf>>,
) {
    let Ok(_guard) = PERSIST_MUTEX.lock() else { return };
    let Some(path) = store_path.lock().clone() else { return };
    let snapshot = descriptors.lock().clone();
    if let Err(error) = save_descriptors_locked(&path, &snapshot) {
        tracing::error!(%error, "paired descriptor persistence failed; memory retained");
    }
}

async fn authority_changed(authority: &mut Option<tokio::sync::watch::Receiver<(u64, String)>>) -> String {
    if let Some(events) = authority.as_mut() {
        if events.changed().await.is_ok() { return events.borrow_and_update().1.clone(); }
        *authority = None;
    }
    std::future::pending::<String>().await
}

/// Weak ownership avoids keeping the runtime alive. Identity fencing prevents an
/// old actor (for example, after detach) from removing a replacement for its ID.
struct ReapOwner {
    owners: std::sync::Weak<Mutex<HashMap<String, Owner>>>,
    id: String,
    identity: Arc<()>,
    #[cfg(test)]
    completed: tokio::sync::watch::Sender<bool>,
}
impl Drop for ReapOwner {
    fn drop(&mut self) {
        if let Some(owners) = self.owners.upgrade() {
            // The cancel path can run synchronously while the aborting thread
            // already holds this mutex: install holds it across tokio::spawn of
            // the actor, and a runtime shutdown (or an Owner replaced under the
            // lock) cancels the task from that same thread. A blocking re-lock
            // would self-deadlock the non-reentrant mutex, so degrade to a
            // try-lock; under contention the aborting context owns the map
            // transition, and a skipped removal is inert because every live-task
            // check filters finished entries.
            if let Some(mut owners) = owners.try_lock() {
                if owners
                    .get(&self.id)
                    .is_some_and(|owner| Arc::ptr_eq(&owner.identity, &self.identity)
                        && owner.outcome.borrow().state != "affirmative_exit")
                {
                    owners.remove(&self.id);
                }
            }
        }
        #[cfg(test)]
        let _ = self.completed.send(true);
    }
}

pub struct Runtime {
    owners: Arc<Mutex<HashMap<String, Owner>>>,
    descriptors: Arc<Mutex<HashMap<String, super::paired_daemon::Descriptor>>>,
    store_path: Arc<Mutex<Option<PathBuf>>>,
    agent_sink: Arc<parking_lot::RwLock<Option<Arc<dyn crate::terminal::remote::AgentStateSink>>>>,
    installation: Mutex<()>,
    #[cfg(any(test, feature = "local-split-qa"))]
    status_events: tokio::sync::broadcast::Sender<(String, (String, Option<Epoch>, Option<i32>, Option<String>))>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryOwnedResult {
    pub admitted: bool,
    pub owner_unavailable: bool,
    pub stale_tuple: bool,
    pub blocked: bool,
    pub affirmative_exit: bool,
}

#[cfg(feature = "local-split-qa")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedOwnerSnapshot {
    pub backend_session_id: String,
    pub daemon_epoch: String,
    pub incarnation: String,
    pub generation: Option<u64>,
    pub state: String,
    pub blocked_reason: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedRecoveryOutcome {
    pub backend_session_id: String,
    pub daemon_epoch: String,
    pub incarnation: String,
    pub generation: Option<u64>,
    pub state: String,
    pub blocked_reason: Option<String>,
    pub exit_code: Option<i32>,
}

#[cfg(feature = "local-split-qa")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDialAudit {
    pub identity: PairedRecoveryOutcome,
    pub pending_dial: bool,
    pub cancellation_count: u64,
    pub recovery_attempts: u64,
    pub replay_cursor: Option<Epoch>,
    pub recovery_event_start_sequence: u64,
    pub recovery_events: Vec<PairedRecoveryAttemptReceipt>,
}
#[cfg(feature = "local-split-qa")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedRecoveryAttemptReceipt {
    pub sequence: u64,
    pub backend_session_id: String,
    pub host_id: String,
    pub target: crate::remote::machine_protocol::RemoteTerminalTarget,
    pub incarnation: String,
    pub generation: Option<u64>,
    pub replay_cursor: Option<Epoch>,
    pub kind: String,
    pub failure_code: Option<String>,
}
impl RetryOwnedResult {
    fn admitted() -> Self { Self { admitted: true, owner_unavailable: false, stale_tuple: false, blocked: false, affirmative_exit: false } }
    fn owner_unavailable() -> Self { Self { admitted: false, owner_unavailable: true, stale_tuple: false, blocked: false, affirmative_exit: false } }
    fn stale_tuple() -> Self { Self { admitted: false, owner_unavailable: false, stale_tuple: true, blocked: false, affirmative_exit: false } }
    fn blocked() -> Self { Self { admitted: false, owner_unavailable: false, stale_tuple: false, blocked: true, affirmative_exit: false } }
    fn affirmative_exit() -> Self { Self { admitted: false, owner_unavailable: false, stale_tuple: false, blocked: false, affirmative_exit: true } }
    fn protocol_flags_valid(self) -> bool {
        [self.admitted, self.owner_unavailable, self.stale_tuple, self.blocked, self.affirmative_exit]
            .into_iter().filter(|flag| *flag).count() == 1
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Runtime {
    pub fn new(store_path: Option<PathBuf>) -> Self {
        let store_path = store_path.or_else(default_descriptors_path);
        let _persist_guard = PERSIST_MUTEX.lock();
        let (descriptors, effective_store_path) = if let Some(ref path) = store_path {
            match load_descriptors(path) {
                Ok(map) => (map, store_path),
                Err(e) => {
                    eprintln!("[paired_runtime] descriptor store load failed; disabling persistence to protect file: {e}");
                    (HashMap::new(), None)
                }
            }
        } else {
            (HashMap::new(), None)
        };
        drop(_persist_guard);
        Self {
            owners: Arc::new(Mutex::new(HashMap::new())),
            descriptors: Arc::new(Mutex::new(descriptors)),
            store_path: Arc::new(Mutex::new(effective_store_path)),
            agent_sink: Arc::new(parking_lot::RwLock::new(None)),
            installation: Mutex::new(()),
            #[cfg(any(test, feature = "local-split-qa"))]
            status_events: tokio::sync::broadcast::channel(128).0,
        }
    }

    pub fn set_agent_sink(&self, sink: Arc<dyn crate::terminal::remote::AgentStateSink>) {
        *self.agent_sink.write() = Some(sink);
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_drop_loopback_transport(&self, id: &str) -> Result<(), &'static str> {
        let (reply, result) = oneshot::channel();
        {
            let owners = self.owners.lock();
            let owner = owners.get(id).ok_or("OWNER_UNAVAILABLE")?;
            owner.sender.try_send(Command::QaDropLoopback(reply)).map_err(|_| "OWNER_UNAVAILABLE")?;
        }
        tokio::time::timeout(RETRY_ADMISSION_TIMEOUT, result).await.map_err(|_| "OWNER_UNAVAILABLE")?
            .map_err(|_| "OWNER_UNAVAILABLE")?.map_err(|_| "QA_LOOPBACK_REQUIRED")
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_cancel_pending_dial(&self, id: &str) -> Result<(), &'static str> {
        let (reply, result) = oneshot::channel();
        {
            let owners = self.owners.lock();
            let owner = owners.get(id).ok_or("OWNER_UNAVAILABLE")?;
            owner.sender.try_send(Command::QaCancelDial(reply)).map_err(|_| "OWNER_UNAVAILABLE")?;
        }
        tokio::time::timeout(RETRY_ADMISSION_TIMEOUT, result).await.map_err(|_| "OWNER_UNAVAILABLE")?
            .map_err(|_| "OWNER_UNAVAILABLE")?.map_err(|_| "OWNER_UNAVAILABLE")
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_route_receipt(&self, id: &str) -> Result<super::paired_daemon::PairedRouteReceipt, &'static str> {
        let (reply, result) = oneshot::channel();
        {
            let owners = self.owners.lock();
            let owner = owners.get(id).ok_or("OWNER_UNAVAILABLE")?;
            owner.sender.try_send(Command::QaRouteReceipt(reply)).map_err(|_| "OWNER_UNAVAILABLE")?;
        }
        tokio::time::timeout(RETRY_ADMISSION_TIMEOUT, result).await.map_err(|_| "OWNER_UNAVAILABLE")?
            .map_err(|_| "OWNER_UNAVAILABLE")?.map_err(|_| "OWNER_UNAVAILABLE")
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_dial_audit(&self, id: &str) -> Result<PairedDialAudit, &'static str> {
        let (reply, result) = oneshot::channel();
        {
            let owners = self.owners.lock();
            let owner = owners.get(id).ok_or("OWNER_UNAVAILABLE")?;
            owner.sender.try_send(Command::QaDialAudit(reply)).map_err(|_| "OWNER_UNAVAILABLE")?;
        }
        tokio::time::timeout(RETRY_ADMISSION_TIMEOUT, result).await.map_err(|_| "OWNER_UNAVAILABLE")?
            .map_err(|_| "OWNER_UNAVAILABLE")
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_cold_descriptor(&self, id: &str) -> Result<super::paired_daemon::Descriptor, String> {
        let path = self.store_path.lock().clone().ok_or_else(|| "QA_DESCRIPTOR_STORE_REQUIRED".to_owned())?;
        let id = id.to_owned();
        crate::ipc::run_blocking(move || {
            let restored = Runtime::new(Some(path));
            Ok(restored.descriptor(&id).ok_or_else(|| "OWNER_UNAVAILABLE".to_owned()))
        }).await.map_err(|error| error.to_string())?
    }

    #[cfg(test)]
    pub(crate) async fn qa_block_recovery(&self, id: &str) -> Result<(), &'static str> {
        self.qa_actor_command(id, |reply| Command::QaBlockRecovery(reply)).await
    }
    #[cfg(test)]
    pub(crate) async fn qa_release_recovery(&self, id: &str) -> Result<(), &'static str> {
        self.qa_actor_command(id, |reply| Command::QaReleaseRecovery(reply)).await
    }
    #[cfg(test)]
    async fn qa_actor_command(&self, id: &str, command: impl FnOnce(Reply) -> Command) -> Result<(), &'static str> {
        let owners = self.owners.lock();
        let Some(owner) = owners.get(id).filter(|owner| !owner.task.is_finished()) else { return Err("OWNER_UNAVAILABLE") };
        let (reply, result) = oneshot::channel();
        owner.sender.try_send(command(reply)).map_err(|_| "PAIRED_PROXY_BUSY")?;
        drop(owners);
        result.await.map_err(|_| "OWNER_UNAVAILABLE")?.map_err(|_| "OWNER_UNAVAILABLE")
    }

    pub fn set_store_path(&self, path: PathBuf) {
        // N1: single lock order — the store-path snapshot is taken BEFORE the
        // descriptors lock is ever acquired; no site may acquire store_path
        // while holding descriptors.
        match load_descriptors(&path) {
            Err(e) => {
                // N2 (round 2): a failed load must not let the subsequent save
                // clobber a corrupt-but-recoverable store with live memory.
                eprintln!(
                    "[paired_runtime] descriptor store load failed; leaving file untouched: {e}"
                );
                return;
            }
            Ok(loaded) => {
                let _persist_guard = match PERSIST_MUTEX.lock() {
                    Ok(g) => g,
                    Err(_) => return,
                };
                *self.store_path.lock() = Some(path.clone());
                let current = {
                    let mut desc_guard = self.descriptors.lock();
                    for (k, v) in loaded {
                        desc_guard.entry(k).or_insert(v);
                    }
                    desc_guard.clone()
                };
                if let Err(e) = save_descriptors_locked(&path, &current) {
                    eprintln!("[paired_runtime] descriptor save failed after set_store_path: {e}");
                }
            }
        }
    }

    pub fn owns(id: &str) -> bool {
        id.starts_with("daemon-session:")
    }
    pub fn contains(&self, id: &str) -> bool {
        self.owners
            .lock()
            .get(id)
            .is_some_and(|o| !o.task.is_finished())
    }
    pub fn recovery_status(&self, id: &str) -> Option<(String, Option<Epoch>, Option<i32>, Option<String>)> {
        self.owners.lock().get(id).filter(|owner| !owner.task.is_finished())
            .map(|owner| owner.status.lock().clone())
    }
    pub fn recovery_outcome(&self, id: &str) -> Option<PairedRecoveryOutcome> {
        let owners = self.owners.lock();
        let owner = owners.get(id)?;
        let outcome = owner.outcome.borrow().clone();
        Some(outcome)
    }
    pub fn subscribe_recovery_outcome(&self, id: &str) -> Option<tokio::sync::watch::Receiver<PairedRecoveryOutcome>> {
        self.owners.lock().get(id).map(|owner| owner.outcome.clone())
    }
    #[cfg(test)]
    async fn subscribe_recovery_events(&self, id: &str) -> tokio::sync::broadcast::Receiver<RecoveryTestEvent> {
        let sender = self.sender(id).expect("actor sender exists");
        let (reply, received) = oneshot::channel();
        sender.send(Command::SubscribeRecoveryEvents(reply)).await.expect("actor accepts event subscription");
        received.await.expect("actor returns recovery event subscription")
    }
    #[cfg(feature = "local-split-qa")]
    pub fn owner_snapshot(&self, id: &str) -> Option<PairedOwnerSnapshot> {
        let owners = self.owners.lock();
        let owner = owners.get(id).filter(|owner| !owner.task.is_finished())?;
        let descriptor = self.descriptors.lock().get(id)?.clone();
        if descriptor.target != owner.target { return None; }
        let (state, generation, exit_code, blocked_reason) = owner.status.lock().clone();
        Some(PairedOwnerSnapshot {
            backend_session_id: id.to_owned(),
            daemon_epoch: descriptor.target.daemon_epoch.0.to_string(),
            incarnation: owner.incarnation.clone(),
            generation: generation.map(|epoch| epoch.0),
            state,
            blocked_reason,
            exit_code,
        })
    }
    #[cfg(feature = "local-split-qa")]
    pub fn qa_owner_audit(&self, id: &str) -> Option<(String, String, Option<Epoch>, super::paired_daemon::Descriptor)> {
        let owners = self.owners.lock();
        let owner = owners.get(id)?;
        let descriptor = self.descriptors.lock().get(id)?.clone();
        let outcome = owner.outcome.borrow().clone();
        Some((outcome.state, outcome.incarnation, outcome.generation.map(Epoch), descriptor))
    }
    #[cfg(feature = "local-split-qa")]
    pub async fn qa_wait_owner_state(&self, id: &str, state: &str, timeout: Duration) -> Result<(String, Option<Epoch>), &'static str> {
        let mut events = self.subscribe_recovery_outcome(id).ok_or("OWNER_UNAVAILABLE")?;
        tokio::time::timeout(timeout, async {
            loop {
                let outcome = events.borrow_and_update().clone();
                if outcome.state == state { return Ok((outcome.state, outcome.generation.map(Epoch))); }
                events.changed().await.map_err(|_| "OWNER_UNAVAILABLE")?;
            }
        }).await.map_err(|_| "OWNER_UNAVAILABLE")?
    }
    pub fn get_or_rearm(
        &self,
        descriptor: super::paired_daemon::Descriptor,
        hub: Arc<super::output_hub::TerminalOutputHub>,
        service: crate::paired_host::service::PairedHostService,
    ) -> Result<(String, Epoch), crate::paired_host::client::ClientError> {
        let _installation = self.installation.lock();
        let id = crate::remote::machine_protocol::proxy_backend_id(&descriptor.host_id, &descriptor.target)
            .map_err(|_| crate::paired_host::client::ClientError::local("INVALID_REQUEST"))?;
        if let Some(retained) = self.descriptor(&id) {
            if retained.host_id != descriptor.host_id || retained.generation != descriptor.generation
                || retained.target != descriptor.target {
                return Err(crate::paired_host::client::ClientError::local("PAIRED_HOST_STALE_GENERATION"));
            }
        }
        {
            let owners = self.owners.lock();
            if let Some(owner) = owners.get(&id).filter(|owner| !owner.task.is_finished()) {
                let generation = owner.status.lock().1.unwrap_or(Epoch(0));
                let (reply, _) = oneshot::channel();
                owner.sender.try_send(Command::Rearm(reply))
                    .map_err(|_| crate::paired_host::client::ClientError::local("PAIRED_PROXY_BUSY"))?;
                return Ok((id, generation));
            }
        }
        let retained = self.descriptor(&id).unwrap_or(descriptor);
        let mut proxy = Proxy::new(retained, hub)?;
        proxy.set_recovery(service);
        self.install(proxy).map_err(|code| crate::paired_host::client::ClientError::local(&code))?;
        Ok((id, Epoch(0)))
    }
    /// Lifetime incarnation of the proxy actor that currently owns `id`.
    ///
    /// `None` once no live actor owns the id: a session cannot be attached through a proxy that
    /// is gone, and the next reattach installs a new incarnation the caller must re-bind to.
    pub fn session_incarnation(&self, id: &str) -> Option<String> {
        self.owners
            .lock()
            .get(id)
            .filter(|owner| !owner.task.is_finished())
            .map(|owner| owner.incarnation.clone())
    }
    pub async fn retry_owned(&self, backend_session_id: &str, daemon_epoch: &str, incarnation: &str) -> Result<RetryOwnedResult, &'static str> {
        let result = {
            let owners = self.owners.lock();
            let Some(owner) = owners.get(backend_session_id) else {
                return Ok(RetryOwnedResult::owner_unavailable());
            };
            let status = owner.status.lock().clone();
            let descriptor = self.descriptors.lock().get(backend_session_id).cloned();
            let Some(descriptor) = descriptor else { return Ok(RetryOwnedResult::owner_unavailable()) };
            if descriptor.target != owner.target { return Ok(RetryOwnedResult::stale_tuple()); }
            if descriptor.target.daemon_epoch.0.to_string() != daemon_epoch || owner.incarnation != incarnation {
                return Ok(RetryOwnedResult::stale_tuple());
            }
            if status.0 == "affirmative_exit" { return Ok(RetryOwnedResult::affirmative_exit()); }
            if status.0 == "blocked" { return Ok(RetryOwnedResult::blocked()); }
            if owner.task.is_finished() { return Ok(RetryOwnedResult::owner_unavailable()); }
            let (reply, result) = oneshot::channel();
            if owner.sender.try_send(Command::RetryOwned {
                daemon_epoch: daemon_epoch.to_owned(),
                incarnation: incarnation.to_owned(),
                reply,
            }).is_err() { return Ok(RetryOwnedResult::owner_unavailable()); }
            result
        };
        match tokio::time::timeout(RETRY_ADMISSION_TIMEOUT, result).await {
            Err(_) => Ok(RetryOwnedResult::owner_unavailable()),
            Ok(Ok(Ok(()))) => Ok(RetryOwnedResult::admitted()),
            Ok(Ok(Err(ref code))) if code == "PAIRED_PROXY_EXITED" => Ok(RetryOwnedResult::affirmative_exit()),
            Ok(Ok(Err(ref code))) if code == "PAIRED_PROXY_BLOCKED" => Ok(RetryOwnedResult::blocked()),
            Ok(Ok(Err(ref code))) if code == "PAIRED_PROXY_STALE_TUPLE" => Ok(RetryOwnedResult::stale_tuple()),
            _ => Ok(RetryOwnedResult::owner_unavailable()),
        }
    }
    pub fn list(&self) -> Vec<String> {
        self.owners
            .lock()
            .iter()
            .filter(|(_, o)| !o.task.is_finished())
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub fn descriptor(&self, id: &str) -> Option<super::paired_daemon::Descriptor> {
        self.descriptors.lock().get(id).cloned()
    }
    /// Live identity of the running actor for `id`; `None` once no live actor owns it.
    pub fn live_state(&self, id: &str) -> Option<super::paired_daemon::PairedLive> {
        self.owners
            .lock()
            .get(id)
            .filter(|owner| !owner.task.is_finished())
            .map(|owner| *owner.live.lock())
    }
    pub fn remove_descriptor(&self, id: &str) {
        self.descriptors.lock().remove(id);
        let descriptors = self.descriptors.clone();
        let path = self.store_path.clone();
        tokio::task::spawn_blocking(move || persist_snapshot(&descriptors, &path));
    }
    async fn flush_descriptors(&self) -> Result<(), String> {
        let descriptors = self.descriptors.clone();
        let path = self.store_path.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = PERSIST_MUTEX.lock().map_err(|error| error.to_string())?;
            let Some(path) = path.lock().clone() else { return Ok(()) };
            save_descriptors_locked(&path, &descriptors.lock().clone())
        }).await.map_err(|error| error.to_string())?
    }
    pub fn install(&self, mut proxy: Proxy) -> Result<String, String> {
        if let Some(sink) = self.agent_sink.read().clone() {
            proxy.set_agent_sink(sink);
        }
        let id = proxy.id().to_owned();
        let descriptor = proxy.descriptor().clone();
        let target = descriptor.target.clone();
        let mut owners = self.owners.lock();
        if owners.get(&id).is_some_and(|o| !o.task.is_finished()) {
            return Err("CONTROL_CONFLICT".into());
        }
        owners.remove(&id);

        self.descriptors.lock().insert(id.clone(), descriptor);
        let persist_descriptors = self.descriptors.clone();
        let persist_path = self.store_path.clone();
        tokio::task::spawn_blocking(move || persist_snapshot(&persist_descriptors, &persist_path));

        let (sender, receiver) = mpsc::channel(32);
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        let (outcome, exit_code) = proxy.outcome();
        let status = Arc::new(Mutex::new((outcome.into(), proxy.controller(), exit_code, proxy.blocked_reason().map(str::to_owned))));
        let actor_status = status.clone();
        #[cfg(any(test, feature = "local-split-qa"))]
        let status_events = self.status_events.clone();
        let identity = Arc::new(());
        // The session's lifetime incarnation, minted by this runtime: it stays constant for as
        // long as this actor owns the id, and every replacement actor mints a different one.
        let incarnation = uuid::Uuid::new_v4().to_string();
        let actor_incarnation = incarnation.clone();
        let (outcome_tx, outcome_rx) = tokio::sync::watch::channel(PairedRecoveryOutcome {
            backend_session_id: id.clone(),
            daemon_epoch: target.daemon_epoch.0.to_string(),
            incarnation: incarnation.clone(),
            generation: proxy.controller().map(|epoch| epoch.0),
            state: outcome.into(),
            blocked_reason: proxy.blocked_reason().map(str::to_owned),
            exit_code,
        });
        #[cfg(test)]
        let (completed_tx, completed) = tokio::sync::watch::channel(false);
        let reap = ReapOwner {
            owners: Arc::downgrade(&self.owners),
            id: id.clone(),
            identity: identity.clone(),
            #[cfg(test)]
            completed: completed_tx,
        };
        let descriptors = self.descriptors.clone();
        let store_path = self.store_path.clone();
        let task_id = id.clone();
        let live = Arc::new(Mutex::new(proxy.live_state()));
        let task_live = live.clone();
        let task = tokio::spawn(async move {
            let _reap = reap;
            let dirty = Arc::new(AtomicBool::new(false));
            let _persist_drop_guard = ActorPersistGuard {
                descriptors: descriptors.clone(),
                store_path: store_path.clone(),
                dirty: dirty.clone(),
            };
            // Drop the proxy and receiver before publishing completion/removing
            // the map entry. A cloned sender then fails closed on this channel.
            let mut proxy = proxy;
            let mut receiver = receiver;
            let mut authority = proxy.subscribe_authority();
            // Relay->direct upgrade. Subscribed before the first poll so a route
            // published from here on is seen; a route already live is picked up
            // by the initial re-check. One attempt per direct route epoch.
            let route_host = proxy.descriptor().host_id.clone();
            let mut route_events = Some(crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES.subscribe());
            let mut route_recheck = true;
            let mut upgrade_due: Option<u64> = None;
            let mut upgrade_tried: Option<u64> = None;
            tokio::select! { biased;
                _ = cancelled.wait_for(|cancelled| *cancelled) => {},
                _ = async {
            let mut keepalive = tokio::time::interval(Duration::from_secs(5));
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            keepalive.tick().await;
            #[cfg(test)]
            let (recovery_events, _) = tokio::sync::broadcast::channel(64);
            let mut recovery_at = proxy.can_recover().then(tokio::time::Instant::now);
            #[cfg(test)]
            if recovery_at.is_some() { let _ = recovery_events.send(RecoveryTestEvent::Scheduled); }
            #[cfg(feature = "local-split-qa")]
            let mut cancellation_count = 0;
            #[cfg(feature = "local-split-qa")]
            let mut recovery_attempts = 0;
            #[cfg(feature = "local-split-qa")]
            let mut recovery_receipts: Vec<PairedRecoveryAttemptReceipt> = Vec::new();
            // Seed one interval in the past so the first change persists immediately:
            // a change arriving inside the first debounce window could otherwise be
            // lost when the actor is dropped before the window elapses.
            let mut last_persisted = tokio::time::Instant::now()
                .checked_sub(PERSIST_MIN_INTERVAL)
                .unwrap_or_else(tokio::time::Instant::now);
            loop {
                if std::mem::take(&mut route_recheck) {
                    if let Some((_, epoch)) = crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES.direct_for(&route_host) {
                        if upgrade_tried.is_none_or(|tried| epoch > tried) { upgrade_due = upgrade_due.max(Some(epoch)); }
                    }
                }
                *task_live.lock() = proxy.live_state();
                let (outcome, exit_code) = proxy.outcome();
                *actor_status.lock() = (outcome.into(), proxy.controller(), exit_code, proxy.blocked_reason().map(str::to_owned));
                outcome_tx.send_replace(PairedRecoveryOutcome {
                    backend_session_id: task_id.clone(),
                    daemon_epoch: proxy.descriptor().target.daemon_epoch.0.to_string(),
                    incarnation: actor_incarnation.clone(),
                    generation: proxy.controller().map(|epoch| epoch.0),
                    state: outcome.into(),
                    blocked_reason: proxy.blocked_reason().map(str::to_owned),
                    exit_code,
                });
                #[cfg(any(test, feature = "local-split-qa"))]
                let _ = status_events.send((task_id.clone(), actor_status.lock().clone()));
                tokio::select! {
                    biased;
                    host = authority_changed(&mut authority) => {
                        if matches!(tokio::time::timeout(Duration::from_secs(30), proxy.rearm_authority(&host)).await, Ok(true)) {
                            recovery_at = Some(tokio::time::Instant::now());
                            #[cfg(test)]
                            let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                        }
                    }
                    event = async { route_events.as_mut().expect("guarded").recv().await }, if route_events.is_some() => match event {
                        Ok(event) if event.host_id == route_host => {
                            if matches!(event.state, crate::paired_host::direct_route::RouteState::Direct { .. })
                                && upgrade_tried.is_none_or(|tried| event.epoch > tried) {
                                upgrade_due = upgrade_due.max(Some(event.epoch));
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => route_recheck = true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => route_events = None,
                    },
                    // Make-before-break: the relay transport stays live until the
                    // direct attach is validated. Mailbox commands wait (bounded by
                    // the upgrade timeout) instead of being refused, so no input is
                    // dropped; generation-0 input lands on whichever transport is
                    // current afterwards, and nothing is ever resent.
                    _ = async {}, if upgrade_due.is_some() => {
                        let epoch = upgrade_due.take().expect("guarded");
                        if proxy.relay_attached() {
                            upgrade_tried = Some(epoch);
                            let direct = matches!(proxy.upgrade_direct(epoch).await, Ok(true));
                            #[cfg(test)]
                            let _ = recovery_events.send(RecoveryTestEvent::Upgraded { epoch, direct });
                            if direct { keepalive.reset(); }
                        }
                    }
                    command = receiver.recv() => match command {
                        Some(Command::Write(g, data, reply)) => {
                            let gen = if g == 0 { proxy.controller().unwrap_or(Epoch(0)) } else { Epoch(g) };
                            let result = proxy.write(gen, &data).await;
                            if let Err(ref failure) = result {
                                if proxy.controller().is_none() && failure.code != "PAIRED_NOT_SENT" { proxy.recovery_failed(failure); }
                            }
                            let _ = reply.send(result.map_err(|e| e.code));
                        }
                        Some(Command::Resize(g, cols, rows, reply)) => {
                            let gen = if g == 0 { proxy.controller().unwrap_or(Epoch(0)) } else { Epoch(g) };
                            let _ = reply.send(proxy.resize(gen, cols, rows).await.map_err(|e| e.code));
                        }
                        Some(Command::Interrupt(g, intent_id, reply)) => { let _ = reply.send(proxy.interrupt_with_intent(Epoch(g), intent_id).await.map_err(|e| e.code)); }
                        Some(Command::Rearm(reply)) => {
                            if proxy.controller().is_none() && !proxy.exited() {
                                if proxy.blocked_reason().is_none() {
                                    recovery_at = Some(tokio::time::Instant::now());
                                    #[cfg(test)]
                                    let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                                } else {
                                    let host = proxy.descriptor().host_id.clone();
                                    if proxy.rearm_authority(&host).await {
                                        recovery_at = Some(tokio::time::Instant::now());
                                        #[cfg(test)]
                                        let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                                    }
                                }
                            }
                            let _ = reply.send(Ok(()));
                        }
                        Some(Command::RetryOwned { daemon_epoch, incarnation, reply }) => {
                            if reply.is_closed() { continue; }
                            let descriptor = proxy.descriptor();
                            if daemon_epoch != descriptor.target.daemon_epoch.0.to_string() || incarnation != actor_incarnation {
                                let _ = reply.send(Err("PAIRED_PROXY_STALE_TUPLE".into()));
                                continue;
                            }
                            let result = proxy.retry_admission().map_err(str::to_owned);
                            if result.is_ok() && proxy.blocked_reason().is_none() && recovery_at.is_none() {
                                recovery_at = Some(tokio::time::Instant::now());
                            }
                            let _ = reply.send(result);
                        }
                        Some(Command::Status(reply)) => {
                            let (outcome, exit_code) = proxy.outcome();
                            let _ = reply.send((outcome.into(), proxy.controller(), exit_code, proxy.blocked_reason().map(str::to_owned)));
                        }
                        #[cfg(feature = "local-split-qa")]
                        Some(Command::QaDropLoopback(reply)) => {
                            if reply.is_closed() { continue; }
                            let result = proxy.qa_drop_loopback_transport();
                            let _ = reply.send(result.map_err(str::to_owned));
                        }
                        #[cfg(feature = "local-split-qa")]
                        Some(Command::QaCancelDial(reply)) => {
                            if reply.is_closed() { continue; }
                            if recovery_at.is_some() { cancellation_count += 1; }
                            recovery_at = None;
                            let _ = reply.send(Ok(()));
                        }
                        #[cfg(feature = "local-split-qa")]
                        Some(Command::QaRouteReceipt(reply)) => {
                            let _ = reply.send(proxy.qa_route_receipt().map_err(str::to_owned));
                        }
                        #[cfg(feature = "local-split-qa")]
                        Some(Command::QaDialAudit(reply)) => {
                            let _ = reply.send(PairedDialAudit {
                                identity: outcome_tx.borrow().clone(),
                                pending_dial: false,
                                cancellation_count,
                                recovery_attempts,
                                replay_cursor: proxy.descriptor().after_sequence,
                                recovery_event_start_sequence: recovery_receipts.first().map_or(recovery_attempts + 1, |event| event.sequence),
                                recovery_events: recovery_receipts.clone(),
                            });
                        }
                        #[cfg(test)]
                        Some(Command::QaBlockRecovery(reply)) => {
                            proxy.mark_disconnected();
                            let _ = reply.send(Ok(()));
                        }
                        #[cfg(test)]
                        Some(Command::QaReleaseRecovery(reply)) => {
                            proxy.mark_disconnected();
                            recovery_at = Some(tokio::time::Instant::now());
                            let _ = reply.send(Ok(()));
                        }
                        Some(Command::Detach(reply)) => {
                            let result = proxy.detach().await.map_err(|e| e.code);
                            descriptors.lock().insert(task_id.clone(), proxy.descriptor().clone());
                            dirty.store(true, Ordering::Release);
                            let _ = reply.send(result);
                            return;
                        }
                        #[cfg(test)]
                        Some(Command::Block(started, rx)) => {
                            let _ = started.send(());
                            let _ = rx.await;
                        }
                        #[cfg(test)]
                        Some(Command::HoldRecovery(started, rx)) => {
                            let _ = started.send(());
                            let _ = rx.await;
                        }
                        #[cfg(test)]
                        Some(Command::RecoveryScheduled(reply)) => { let _ = reply.send(recovery_at.is_some()); }
                        #[cfg(test)]
                        Some(Command::SubscribeRecoveryEvents(reply)) => {
                            let _ = reply.send(recovery_events.subscribe());
                        }
                        None => {
                            descriptors.lock().insert(task_id.clone(), proxy.descriptor().clone());
                            dirty.store(true, Ordering::Release);
                            return;
                        }
                    },
                    _ = tokio::time::sleep_until(last_persisted + PERSIST_MIN_INTERVAL), if dirty.load(Ordering::Acquire) => {
                        let descs = descriptors.clone();
                        let path = store_path.clone();
                        tokio::task::spawn_blocking(move || persist_snapshot(&descs, &path));
                        last_persisted = tokio::time::Instant::now();
                        dirty.store(false, Ordering::Release);
                    }
                    _ = keepalive.tick(), if proxy.controller().is_some() => {
                        if let Some(g) = proxy.controller() {
                            if let Err(failure) = proxy.ping(g).await {
                                proxy.recovery_failed(&failure);
                                recovery_at = Some(tokio::time::Instant::now() + proxy.recovery_delay());
                                #[cfg(test)]
                                let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                            }
                        }
                    }
                    _ = async { if let Some(at) = recovery_at { tokio::time::sleep_until(at).await } }, if recovery_at.is_some() => {
                        recovery_at = None;
                        if proxy.can_recover() {
                            *actor_status.lock() = ("owner_unavailable".into(), None, None, None);
                            outcome_tx.send_modify(|outcome| {
                                outcome.state = "owner_unavailable".into();
                                outcome.generation = None;
                                outcome.blocked_reason = None;
                                outcome.exit_code = None;
                            });
                            let mut geometry = None;
                            let recovery_host = proxy.descriptor().host_id.clone();
                            let recovery_epoch = proxy.descriptor().target.daemon_epoch.0.to_string();
                            #[cfg(feature = "local-split-qa")]
                            let replay_cursor = proxy.descriptor().after_sequence;
                            #[cfg(feature = "local-split-qa")]
                            { recovery_attempts += 1; }
                            #[cfg(feature = "local-split-qa")]
                            let mut receipt = PairedRecoveryAttemptReceipt {
                                sequence: recovery_attempts, backend_session_id: task_id.clone(),
                                host_id: recovery_host.clone(), target: proxy.descriptor().target.clone(),
                                incarnation: actor_incarnation.clone(), generation: proxy.controller().map(|epoch| epoch.0),
                                replay_cursor, kind: "recovery-attempt-connected".into(), failure_code: None,
                            };
                            #[cfg(feature = "local-split-qa")]
                            let mut cancel_reply = None;
                            let result = {
                                let attempt = proxy.recover();
                                tokio::pin!(attempt);
                                loop {
                                    tokio::select! { biased;
                                        host = authority_changed(&mut authority) => {
                                            if host == recovery_host {
                                                #[cfg(feature = "local-split-qa")]
                                                { receipt.kind = "recovery-attempt-authority-interrupted".into(); }
                                                break Err(crate::paired_host::client::ClientError::local("HOST_UNAVAILABLE"));
                                            }
                                        },
                                        command = receiver.recv() => match command {
                                            Some(Command::Write(_, _, reply)) | Some(Command::Interrupt(_, _, reply)) => {
                                                let _ = reply.send(Err("PAIRED_NOT_SENT".into()));
                                            }
                                            Some(Command::Resize(generation, cols, rows, reply)) => {
                                                let valid = cols > 0 && rows > 0 && cols <= 1000 && rows <= 1000;
                                                if valid && generation == 0 { geometry = Some((cols, rows)); }
                                                let _ = reply.send(Err(if !valid { "INVALID_REQUEST" } else if generation != 0 { "STALE_GENERATION" } else { "PAIRED_NOT_SENT" }.into()));
                                            }
                                            Some(Command::Rearm(reply)) => { let _ = reply.send(Ok(())); }
                                            #[cfg(feature = "local-split-qa")]
                                            Some(Command::QaDropLoopback(reply)) => { let _ = reply.send(Err("OWNER_UNAVAILABLE".into())); }
                                            #[cfg(feature = "local-split-qa")]
                                            Some(Command::QaCancelDial(reply)) => {
                                                if reply.is_closed() { continue; }
                                                cancel_reply = Some(reply);
                                                break Err(crate::paired_host::client::ClientError::local("HOST_UNAVAILABLE"));
                                            }
                                            #[cfg(feature = "local-split-qa")]
                                            Some(Command::QaRouteReceipt(reply)) => { let _ = reply.send(Err("OWNER_UNAVAILABLE".into())); }
                                            #[cfg(feature = "local-split-qa")]
                                            Some(Command::QaDialAudit(reply)) => {
                                                let _ = reply.send(PairedDialAudit {
                                                    identity: outcome_tx.borrow().clone(),
                                                    pending_dial: true,
                                                    cancellation_count,
                                                    recovery_attempts,
                                                    replay_cursor,
                                                    recovery_event_start_sequence: recovery_receipts.first().map_or(recovery_attempts, |event| event.sequence),
                                                    recovery_events: recovery_receipts.clone(),
                                                });
                                            }
                                            Some(Command::RetryOwned { daemon_epoch, incarnation, reply }) => {
                                                if reply.is_closed() { continue; }
                                                if daemon_epoch != recovery_epoch || incarnation != actor_incarnation {
                                                    let _ = reply.send(Err("PAIRED_PROXY_STALE_TUPLE".into()));
                                                    continue;
                                                }
                                                let _ = reply.send(Ok(()));
                                            }
                                            Some(Command::Status(reply)) => {
                                                let _ = reply.send(("owner_unavailable".into(), None, None, None));
                                            }
                                            #[cfg(test)]
                                            Some(Command::QaBlockRecovery(reply)) => { let _ = reply.send(Ok(())); }
                                            #[cfg(test)]
                                            Some(Command::QaReleaseRecovery(reply)) => { recovery_at = Some(tokio::time::Instant::now()); let _ = reply.send(Ok(())); }
                                            Some(Command::Detach(reply)) => { let _ = reply.send(Ok(())); return; }
                                            #[cfg(test)]
                                            Some(Command::Block(started, gate)) => { let _ = started.send(()); let _ = gate.await; }
                                            #[cfg(test)]
                                            Some(Command::HoldRecovery(started, gate)) => { let _ = started.send(()); let _ = gate.await; }
                                            #[cfg(test)]
                                            Some(Command::RecoveryScheduled(reply)) => { let _ = reply.send(recovery_at.is_some()); }
                                            #[cfg(test)]
                                            Some(Command::SubscribeRecoveryEvents(reply)) => { let _ = reply.send(recovery_events.subscribe()); }
                                            None => return,
                                        },
                                        result = &mut attempt => break result,
                                    }
                                }
                            };
                            #[cfg(feature = "local-split-qa")]
                            if let Some(reply) = cancel_reply {
                                receipt.kind = "recovery-attempt-cancelled".into();
                                if recovery_receipts.len() == 256 { recovery_receipts.remove(0); }
                                recovery_receipts.push(receipt);
                                proxy.mark_disconnected();
                                recovery_at = None;
                                cancellation_count += 1;
                                let _ = reply.send(Ok(()));
                                continue;
                            }
                            if let Some((cols, rows)) = geometry {
                                proxy.remember_geometry(cols, rows);
                                if let Some(generation) = proxy.controller() {
                                    if let Err(failure) = proxy.resize(generation, cols, rows).await {
                                        proxy.recovery_failed(&failure);
                                    }
                                }
                            }
                            #[cfg(feature = "local-split-qa")]
                            {
                                if let Err(failure) = &result {
                                    if receipt.kind == "recovery-attempt-connected" {
                                        receipt.kind = "recovery-attempt-failed".into();
                                    }
                                    receipt.failure_code = Some(failure.code.clone());
                                }
                                if recovery_receipts.len() == 256 { recovery_receipts.remove(0); }
                                recovery_receipts.push(receipt);
                            }
                            if let Err(failure) = &result {
                                proxy.recovery_failed(failure);
                                proxy.backoff_recovery();
                            }
                            #[cfg(test)]
                            let _ = recovery_events.send(RecoveryTestEvent::Completed { failed: result.is_err() });
                            if proxy.can_recover() {
                                recovery_at = Some(tokio::time::Instant::now() + proxy.recovery_delay());
                                #[cfg(test)]
                                let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                            }
                        }
                        if proxy.controller().is_some() {
                            keepalive.reset();
                        }
                    }
                result = proxy.receive(), if proxy.controller().is_some() => {
                        if let Err(failure) = result {
                            proxy.recovery_failed(&failure);
                            #[cfg(test)]
                            let _ = recovery_events.send(RecoveryTestEvent::Completed { failed: true });
                        }
                        let seq = proxy.descriptor().after_sequence;
                        let changed = {
                            let mut descs = descriptors.lock();
                            if let Some(d) = descs.get_mut(&task_id) {
                                if d.after_sequence != seq {
                                    d.after_sequence = seq;
                                    true
                                } else {
                                    false
                                }
                            } else {
                                descs.insert(task_id.clone(), proxy.descriptor().clone());
                                true
                            }
                        };
                        if changed {
                            dirty.store(true, Ordering::Release);
                        }
                    }
                }
                if proxy.can_recover() && recovery_at.is_none() {
                    recovery_at = Some(tokio::time::Instant::now() + proxy.recovery_delay());
                    #[cfg(test)]
                    let _ = recovery_events.send(RecoveryTestEvent::Scheduled);
                }
                if !proxy.can_recover() { recovery_at = None; }
            }
                } => {},
            }
        });
        owners.insert(
            id.clone(),
            Owner {
                sender,
                task,
                identity,
                target,
                incarnation,
                live,
                cancel,
                status,
                outcome: outcome_rx,
                #[cfg(test)]
                completed,
            },
        );
        Ok(id)
    }
    #[cfg(test)]
    pub(crate) fn force_reap_owner(&self, id: &str) {
        // Simulates a dead actor whose descriptor is retained — the reattach
        // recovery scenario the reinstall path must never regress.
        self.owners.lock().remove(id);
    }
    #[cfg(test)]
    pub(crate) fn completion_probe(
        &self,
        id: &str,
        pending: super::remote::RemoteOperation,
    ) -> impl std::future::Future<Output = ()> + '_ {
        let (weak, mut completed) = {
            let owners = self.owners.lock();
            let owner = owners.get(id).unwrap();
            (owner.sender.downgrade(), owner.completed.clone())
        };
        let id = id.to_owned();
        async move {
            completed.wait_for(|done| *done).await.unwrap();
            assert!(
                !self.owners.lock().contains_key(&id),
                "completed owner retained in map"
            );
            let failure = pending.await.unwrap_err();
            assert_eq!(failure.kind, super::remote::RemoteFailureKind::Disconnected);
            assert_eq!(failure.message, "PAIRED_PROXY_UNAVAILABLE");
            assert_eq!(weak.strong_count(), 0, "owner sender retained");
            assert!(weak.upgrade().is_none());
        }
    }
    #[cfg(test)]
    pub(crate) fn block_actor(
        &self,
        id: &str,
        started: oneshot::Sender<()>,
        rx: oneshot::Receiver<()>,
    ) {
        if let Ok(sender) = self.sender(id) {
            let _ = sender.try_send(Command::Block(started, rx));
        }
    }
    #[cfg(test)]
    pub(crate) async fn abort_actor(&self, id: &str) {
        let mut completed = {
            let owners = self.owners.lock();
            owners.get(id).map(|o| {
                o.task.abort();
                o.completed.clone()
            })
        };
        if let Some(ref mut completed) = completed {
            let _ = completed.wait_for(|done| *done).await;
        }
        self.flush_descriptors().await.unwrap();
    }
    pub(crate) fn sender(&self, id: &str) -> Result<mpsc::Sender<Command>, String> {
        self.owners
            .lock()
            .get(id)
            .map(|o| o.sender.clone())
            .ok_or_else(|| "PAIRED_PROXY_MISSING".into())
    }
    pub fn write(
        self: &Arc<Self>,
        id: &str,
        generation: u64,
        data: Vec<u8>,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Write(generation, data, reply))
    }
    pub fn resize(
        self: &Arc<Self>,
        id: &str,
        generation: u64,
        cols: u16,
        rows: u16,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| {
            Command::Resize(generation, cols, rows, reply)
        })
    }
    pub fn interrupt(
        &self,
        id: &str,
        generation: u64,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.interrupt_with_intent(id, generation, None)
    }
    pub fn interrupt_with_intent(
        &self,
        id: &str,
        generation: u64,
        intent_id: Option<String>,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        self.operation(id, move |reply| Command::Interrupt(generation, intent_id, reply))
    }
    fn operation(
        &self,
        id: &str,
        command: impl FnOnce(Reply) -> Command + Send + 'static,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        let sender = self.sender(id).map_err(super::PtyError::Other)?;
        Ok(Self::operation_on_sender(sender, command))
    }
    #[cfg(feature = "local-split-qa")]
    pub fn qa_mutation_owned(
        &self, id: &str, daemon_epoch: &str, incarnation: &str,
        generation: u64, operation: &str, intent_id: String, data: Vec<u8>,
    ) -> Result<super::remote::RemoteOperation, super::PtyError> {
        if intent_id.is_empty() { return Err(super::PtyError::Other("INTENT_ID_REQUIRED".into())); }
        if operation != "write" && operation != "signal" {
            return Err(super::PtyError::Other("INVALID_OPERATION".into()));
        }
        let sender = {
            let owners = self.owners.lock();
            let owner = owners.get(id).ok_or_else(|| super::PtyError::Other("OWNER_UNAVAILABLE".into()))?;
            if owner.target.daemon_epoch.0.to_string() != daemon_epoch || owner.incarnation != incarnation {
                return Err(super::PtyError::Other("PAIRED_PROXY_STALE_TUPLE".into()));
            }
            owner.sender.clone()
        };
        let signal = operation == "signal";
        Ok(Self::operation_on_sender(sender, move |reply| {
            if signal { Command::Interrupt(generation, Some(intent_id), reply) }
            else { Command::Write(generation, data, reply) }
        }))
    }
    fn operation_on_sender(
        sender: mpsc::Sender<Command>,
        command: impl FnOnce(Reply) -> Command + Send + 'static,
    ) -> super::remote::RemoteOperation {
        Box::pin(async move {
            let submitted = Arc::new(AtomicBool::new(false));
            let result = tokio::time::timeout(Duration::from_secs(30), async {
                let (tx, rx) = oneshot::channel();
                sender
                    .send(command(tx))
                    .await
                    .map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?;
                submitted.store(true, Ordering::Release);
                rx.await
                    .map_err(|_| "PAIRED_PROXY_UNAVAILABLE".to_string())?
            })
            .await
            .unwrap_or_else(|_| Err(if submitted.load(Ordering::Acquire) { "PAIRED_UNCERTAIN_SEND" } else { "PAIRED_NOT_SENT" }.into()));
            result.map_err(|message| super::remote::RemoteFailure {
                kind: super::remote::RemoteFailureKind::Disconnected,
                message,
            })
        })
    }
    /// Detach only; deliberately not remote session close.
    pub async fn detach(&self, id: &str) -> Result<(), String> {
        // Detach is idempotent: a proxy that is already gone (never registered in
        // this daemon incarnation, reaped while idle, or detached earlier) already
        // satisfies the caller's desired end state. Erroring here stranded callers
        // that only want the proxy gone, e.g. closing a pane restored from a
        // previous daemon generation.
        let Some(mut owner) = self.owners.lock().remove(id) else {
            return Ok(());
        };
        if owner.task.is_finished() {
            return Ok(());
        }
        owner.cancel.send_replace(true);
        tokio::time::timeout(Duration::from_secs(30), &mut owner.task).await
            .map_err(|_| "TIMEOUT")?.map_err(|_| "PAIRED_PROXY_UNAVAILABLE")?;
        if let Err(error) = self.flush_descriptors().await {
            tracing::error!(%error, "paired detach retained descriptor after persistence failure");
        }
        Ok(())
    }

    #[cfg(test)]
    async fn hold_actor_mailbox(&self, id: &str) -> Result<oneshot::Sender<()>, String> {
        let owners = self.owners.lock();
        let owner = owners.get(id).ok_or_else(|| "OWNER_UNAVAILABLE".to_string())?;
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        owner.sender.try_send(Command::HoldRecovery(entered_tx, release_rx))
            .map_err(|_| "PAIRED_PROXY_BUSY".to_string())?;
        drop(owners);
        entered_rx.await.map_err(|_| "OWNER_UNAVAILABLE".to_string())?;
        Ok(release_tx)
    }

    #[cfg(test)]
    pub fn simulate_cursor_persistence(&self, task_id: &str, seq: Option<Epoch>) {
        let changed = {
            let mut descs = self.descriptors.lock();
            if let Some(d) = descs.get_mut(task_id) {
                if d.after_sequence != seq {
                    d.after_sequence = seq;
                    true
                } else {
                    false
                }
            } else {
                descs.insert(
                    task_id.to_string(),
                    super::paired_daemon::Descriptor {
                        host_id: "https://relay.example.com".into(),
                        generation: Epoch(1),
                        target: crate::remote::machine_protocol::RemoteTerminalTarget {
                            machine_id: "test-machine".into(),
                            daemon_epoch: Epoch(1),
                            session_id: task_id.to_string(),
                        },
                        after_sequence: seq,
                    },
                );
                true
            }
        };
        if changed {
            let path = self.store_path.lock().clone();
            let _persist_guard = PERSIST_MUTEX.lock();
            let snapshot = {
                let descs = self.descriptors.lock();
                descs.clone()
            };
            if let (Some(ref path), Ok(_)) = (path, _persist_guard) {
                let _ = save_descriptors_locked(path, &snapshot);
            }
        }
    }
}

#[cfg(test)]
#[path = "paired_upgrade_tests.rs"]
mod upgrade_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paired_host::{client::MachineClient, service::PairedHostService};
    use crate::remote::machine_protocol::RemoteTerminalTarget;
    use crate::remote::terminal_wire::{encode_frame, Metadata};
    use crate::terminal::output_hub::TerminalOutputHub;
    use crate::terminal::paired_daemon::Descriptor;
    use axum::{
        extract::ws::{Message, WebSocketUpgrade},
        routing::{get, post},
        Json, Router,
    };
    use serde_json::json;

    static FAULT_SERIALIZER: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[tokio::test]
    async fn test_runtime_wires_agent_sink_to_installed_proxy() {
        struct TestSink(std::sync::Mutex<Vec<crate::daemon::agent_state::AgentState>>);
        impl crate::terminal::remote::AgentStateSink for TestSink {
            fn accept(&self, state: crate::daemon::agent_state::AgentState) -> bool {
                self.0.lock().unwrap().push(state);
                true
            }
        }

        let runtime = Runtime::default();
        let sink = Arc::new(TestSink(std::sync::Mutex::new(Vec::new())));
        runtime.set_agent_sink(sink.clone());

        let hub = Arc::new(TerminalOutputHub::new(32));
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "test-session".into(),
            },
            after_sequence: Some(Epoch(42)),
        };
        let proxy = Proxy::new(descriptor.clone(), hub.clone()).unwrap();
        let id = runtime.install(proxy).unwrap();

        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
    }

    #[tokio::test]
    async fn test_p13_descriptor_survives_actor_termination() {
        let runtime = Runtime::default();
        let hub = Arc::new(TerminalOutputHub::new(32));
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "test-session".into(),
            },
            after_sequence: Some(Epoch(42)),
        };
        let proxy = Proxy::new(descriptor.clone(), hub.clone()).unwrap();
        let id = runtime.install(proxy).unwrap();

        // While running, descriptor is accessible
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);

        runtime.abort_actor(&id).await;

        // P13 Assertion: The credential-free descriptor must survive actor death!
        let recovered = runtime.descriptor(&id);
        assert!(
            recovered.is_some(),
            "Descriptor was lost on actor death; expected durable retention"
        );
        let recovered = recovered.unwrap();
        assert_eq!(recovered.host_id, descriptor.host_id);
        assert_eq!(recovered.generation, descriptor.generation);
        assert_eq!(recovered.target, descriptor.target);
        assert_eq!(recovered.after_sequence, Some(Epoch(42)));
    }

    #[tokio::test]
    async fn test_p13_descriptor_survives_daemon_restart() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");

        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(2),
            target: RemoteTerminalTarget {
                machine_id: "restart-machine".into(),
                daemon_epoch: Epoch(20),
                session_id: "restart-session".into(),
            },
            after_sequence: Some(Epoch(100)),
        };

        let id = {
            let runtime = Runtime::new(Some(store_path.clone()));
            let hub = Arc::new(TerminalOutputHub::new(32));
            let proxy = Proxy::new(descriptor.clone(), hub).unwrap();
            let id = runtime.install(proxy).unwrap();
            runtime.flush_descriptors().await.unwrap();
            id
        };

        // Simulate daemon restart: create a fresh Runtime loading from same store_path
        let restarted_runtime = Runtime::new(Some(store_path));
        let recovered = restarted_runtime.descriptor(&id);
        assert!(
            recovered.is_some(),
            "Descriptor was lost across daemon restart"
        );
        let recovered = recovered.unwrap();
        assert_eq!(recovered.host_id, descriptor.host_id);
        assert_eq!(recovered.generation, descriptor.generation);
        assert_eq!(recovered.target, descriptor.target);
        assert_eq!(recovered.after_sequence, Some(Epoch(100)));
    }

    #[tokio::test]
    async fn test_n2_save_failure_temp_write_propagates_error() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1.clone(), hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        runtime.flush_descriptors().await.unwrap();
        assert!(store_path.exists());

        FsFault::set(FsFault::FailTempWrite);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s2".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let id2 = runtime.install(p2).unwrap();
        // Release the descriptors guard before save_descriptors takes PERSIST_MUTEX: a guard
        // temporary in the argument lives to the end of the statement and inverts the lock
        // order against the background persist_snapshot spawned by install.
        let snapshot = runtime.descriptors.lock().clone();
        let failed_save = save_descriptors(&store_path, &snapshot);
        FsFault::set(FsFault::None);

        assert!(failed_save.is_err());
        assert!(runtime.descriptor(&id2).is_some());
        runtime.flush_descriptors().await.unwrap();
        assert!(store_path.exists(), "existing store file must survive");
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert!(
            reloaded.descriptor(&id1).is_some(),
            "original descriptor must still be present"
        );
    }

    #[tokio::test]
    async fn test_r2n1_save_failure_restores_previous_descriptor() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1, hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        runtime.flush_descriptors().await.unwrap();
        // Reattach scenario: the prior actor is gone but its descriptor is
        // retained in the map (and on disk).
        runtime.force_reap_owner(&id1);
        assert!(runtime.descriptor(&id1).is_some());

        FsFault::set(FsFault::FailTempWrite);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(2),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(2),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let id2 = runtime.install(p2).unwrap();
        // Release the descriptors guard before save_descriptors takes PERSIST_MUTEX: a guard
        // temporary in the argument lives to the end of the statement and inverts the lock
        // order against the background persist_snapshot spawned by install.
        let snapshot = runtime.descriptors.lock().clone();
        let failed_save = save_descriptors(&store_path, &snapshot);
        FsFault::set(FsFault::None);

        assert!(failed_save.is_err());
        // R2-N1: the retained PRIOR descriptor must be restored, not deleted.
        let kept = runtime
            .descriptor(&id1)
            .expect("previous descriptor must survive a failed reinstall");
        assert_eq!(kept.generation, Epoch(1));
        assert_eq!(kept.target.daemon_epoch, Epoch(1));
        assert!(runtime.descriptor(&id2).is_some());
        runtime.flush_descriptors().await.unwrap();
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert_eq!(
            reloaded
                .descriptor(&id1)
                .expect("disk still holds prior")
                .generation,
            Epoch(1)
        );
    }

    #[tokio::test]
    async fn test_r3n2_corrupt_store_initial_load_disables_persistence_to_prevent_clobber() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        std::fs::write(&store_path, b"{corrupt-json").unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        assert!(runtime.store_path.lock().is_none());

        let hub = Arc::new(TerminalOutputHub::new(32));
        let desc = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let p = Proxy::new(desc, hub).unwrap();
        let _ = runtime.install(p);

        let disk = std::fs::read(&store_path).unwrap();
        assert_eq!(disk, b"{corrupt-json");
    }

    #[tokio::test]
    async fn test_r2n2_corrupt_store_not_clobbered_by_set_store_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        std::fs::write(&store_path, b"{not-json").unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        assert!(
            runtime.descriptor("daemon-session:s1").is_none(),
            "corrupt store loads empty"
        );
        runtime.set_store_path(store_path.clone());
        // The failed load must not let the follow-up save overwrite the
        // corrupt-but-recoverable file with live memory.
        let after = std::fs::read(&store_path).unwrap();
        assert_eq!(after, b"{not-json", "corrupt store must be left untouched");
    }

    #[tokio::test]
    async fn detach_of_unknown_proxy_is_idempotent_success() {
        // Closing a pane restored from a previous daemon generation hands the
        // runtime a proxy id it never registered. Detach must satisfy the caller
        // (the proxy is already gone) instead of stranding the close with
        // PAIRED_PROXY_MISSING.
        let runtime = Runtime::new(None);
        runtime
            .detach("daemon-session:never-registered")
            .await
            .expect("first detach of an unknown proxy must succeed");
        runtime
            .detach("daemon-session:never-registered")
            .await
            .expect("second detach must also succeed");
    }

    #[tokio::test]
    async fn test_n2_save_failure_rename_preserves_existing_file() {
        let _fault_guard = FAULT_SERIALIZER.lock();
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let hub = Arc::new(TerminalOutputHub::new(32));

        let desc1 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s1".into(),
            },
            after_sequence: None,
        };
        let runtime = Runtime::new(Some(store_path.clone()));
        let p1 = Proxy::new(desc1.clone(), hub.clone()).unwrap();
        let id1 = runtime.install(p1).expect("initial install should succeed");
        runtime.flush_descriptors().await.unwrap();
        assert!(store_path.exists());

        FsFault::set(FsFault::FailRename);
        let desc2 = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "test-machine".into(),
                daemon_epoch: Epoch(1),
                session_id: "s2".into(),
            },
            after_sequence: None,
        };
        let p2 = Proxy::new(desc2, hub.clone()).unwrap();
        let id2 = runtime.install(p2).unwrap();
        // Release the descriptors guard before save_descriptors takes PERSIST_MUTEX: a guard
        // temporary in the argument lives to the end of the statement and inverts the lock
        // order against the background persist_snapshot spawned by install.
        let snapshot = runtime.descriptors.lock().clone();
        let failed_save = save_descriptors(&store_path, &snapshot);
        FsFault::set(FsFault::None);

        assert!(failed_save.is_err());
        assert!(runtime.descriptor(&id2).is_some());
        runtime.flush_descriptors().await.unwrap();
        assert!(
            store_path.exists(),
            "existing store file must survive rename failure (destructive save defect)"
        );
        let reloaded = Runtime::new(Some(store_path.clone()));
        assert!(
            reloaded.descriptor(&id1).is_some(),
            "original descriptor must survive rename failure"
        );
    }

    #[tokio::test]
    async fn test_n1_concurrent_cursor_persistence_and_install_detach_deadlock() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let runtime = Arc::new(Runtime::new(Some(store_path)));
        let hub = Arc::new(TerminalOutputHub::new(32));

        let r1 = runtime.clone();
        let h1 = hub.clone();
        let t1 = tokio::task::spawn_blocking(move || {
            for i in 0..200 {
                let desc = Descriptor {
                    host_id: "https://relay.example.com".into(),
                    generation: Epoch(1),
                    target: RemoteTerminalTarget {
                        machine_id: format!("m-{i}"),
                        daemon_epoch: Epoch(1),
                        session_id: format!("s-{i}"),
                    },
                    after_sequence: None,
                };
                if let Ok(proxy) = Proxy::new(desc, h1.clone()) {
                    let _ = r1.install(proxy);
                }
            }
        });

        let r2 = runtime.clone();
        let t2 = tokio::task::spawn_blocking(move || {
            for i in 0..200 {
                let id = format!("daemon-session:https://relay.example.com:m-{i}:1:s-{i}");
                r2.simulate_cursor_persistence(&id, Some(Epoch(i as u64)));
            }
        });

        // The deadline exists to catch an opposite-lock-order deadlock, which
        // never completes; 200 serialized temp-file+rename cycles on this
        // volume legitimately exceed a few seconds under load, so a tight
        // bound produces false failures (and, before the ReapOwner try-lock
        // fix, a wedged suite shutdown). Keep it generous but finite.
        let res = tokio::time::timeout(Duration::from_secs(30), async {
            let (res1, res2) = tokio::join!(t1, t2);
            res1.unwrap();
            res2.unwrap();
        })
        .await;

        assert!(
            res.is_ok(),
            "deadlock detected: opposite lock order hung tasks"
        );
    }

    enum FixtureCommand {
        Attach,
        Drop,
        Output(u64),
        Exit,
        Frame(serde_json::Value),
    }
    struct FixtureSocket {
        commands: mpsc::Sender<FixtureCommand>,
        received: mpsc::Receiver<Message>,
    }
    async fn published_status(events: &mut tokio::sync::broadcast::Receiver<(String, (String, Option<Epoch>, Option<i32>, Option<String>))>, id: &str, expected: &str) -> (String, Option<Epoch>, Option<i32>, Option<String>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let (owner, status) = events.recv().await.unwrap();
                if owner == id && status.0 == expected { return status; }
            }
        }).await.unwrap()
    }
    async fn actor_barrier(runtime: &Runtime, id: &str) -> (String, Option<Epoch>, Option<i32>, Option<String>) {
        let (reply, received) = oneshot::channel();
        runtime.sender(id).unwrap().send(Command::Status(reply)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), received).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn recovery_consistency_paired_relay_drop_retains_actor_hub_and_no_input_replay() {
        let root = tempfile::tempdir().unwrap();
        let (arrived, mut arrivals) = mpsc::channel(32);
        let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
        let runtime = Arc::new(Runtime::new(Some(root.path().join("descriptors.json"))));
        let hub = Arc::new(TerminalOutputHub::new(32));
        let (id, _) = runtime.get_or_rearm(descriptor.clone(), hub.clone(), service.clone()).unwrap();
        let incarnation = runtime.session_incarnation(&id).unwrap();
        let mut output = hub.subscribe_with_sequence(&id, None).unwrap().receiver;
        let first = arrivals.recv().await.unwrap();
        first.commands.send(FixtureCommand::Attach).await.unwrap();
        first.commands.send(FixtureCommand::Output(11)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
        first.commands.send(FixtureCommand::Drop).await.unwrap();
        let mut second = tokio::time::timeout(Duration::from_secs(5), arrivals.recv()).await.unwrap().unwrap();
        assert_eq!(runtime.session_incarnation(&id).unwrap(), incarnation);
        assert_eq!(runtime.descriptor(&id).unwrap().after_sequence, Some(Epoch(11)));
        assert_eq!(runtime.write(&id, 0, b"never-replay".to_vec()).unwrap().await.unwrap_err().message, "PAIRED_NOT_SENT");
        assert_eq!(runtime.resize(&id, 0, 132, 43).unwrap().await.unwrap_err().message, "PAIRED_NOT_SENT");
        runtime.get_or_rearm(descriptor, hub, service).unwrap();
        second.commands.send(FixtureCommand::Attach).await.unwrap();
        let resize = tokio::time::timeout(Duration::from_secs(5), second.received.recv()).await.unwrap().unwrap();
        let Message::Text(text) = resize else { panic!("expected geometry convergence") };
        let geometry: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(geometry["type"], "resize");
        assert_eq!(geometry["generation"], "7");
        assert_eq!(geometry["cols"], 132);
        assert_eq!(geometry["rows"], 43);
        second.commands.send(FixtureCommand::Output(12)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
        assert_eq!(actor_barrier(&runtime, &id).await.0, "proxy_attached");
        assert_eq!(runtime.list(), vec![id.clone()]);
        assert!(arrivals.try_recv().is_err());
        assert!(second.received.try_recv().is_err());
        runtime.detach(&id).await.unwrap();
        assert!(!runtime.contains(&id));
        assert_eq!(runtime.descriptor(&id).unwrap().after_sequence, Some(Epoch(12)));
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn recovery_consistency_paired_close_preempts_held_handshake() {
        let root = tempfile::tempdir().unwrap();
        let (arrived, mut arrivals) = mpsc::channel(1);
        let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
        let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
        let hub = Arc::new(TerminalOutputHub::new(32));
        let (id, _) = runtime.get_or_rearm(descriptor.clone(), hub, service).unwrap();
        let held = arrivals.recv().await.unwrap();
        runtime.detach(&id).await.unwrap();
        assert!(!runtime.contains(&id));
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
        held.commands.send(FixtureCommand::Attach).await.unwrap();
        drop(held);
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn recovery_consistency_paired_revoke_preempts_held_handshake() {
        let root = tempfile::tempdir().unwrap();
        let (arrived, mut arrivals) = mpsc::channel(1);
        let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
        let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
        let (id, _) = runtime.get_or_rearm(descriptor.clone(), Arc::new(TerminalOutputHub::new(32)), service.clone()).unwrap();
        let held = arrivals.recv().await.unwrap();
        let mut events = runtime.status_events.subscribe();
        service.forget(descriptor.host_id.clone(), descriptor.generation).await.unwrap();
        let status = published_status(&mut events, &id, "blocked").await;
        assert_eq!(status.1, None);
        assert_eq!(status.3.as_deref(), Some("PAIRED_HOST_STALE_GENERATION"));
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
        runtime.detach(&id).await.unwrap();
        held.commands.send(FixtureCommand::Attach).await.unwrap();
        drop(held);
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn recovery_consistency_paired_backoff_is_bounded_without_lifetime_limit() {
        let root = tempfile::tempdir().unwrap();
        let descriptor = Descriptor { host_id: "https://fixture.invalid/host/a".into(), generation: Epoch(1), target: RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "s".into() }, after_sequence: Some(Epoch(31)) };
        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor.clone(), hub).unwrap();
        proxy.set_recovery(PairedHostService::open_test_loopback(root.path().join("authority")));
        for _ in 0..25 {
            proxy.recovery_failed(&crate::paired_host::client::ClientError::local("HOST_UNAVAILABLE"));
            assert!(proxy.can_recover());
            tokio::time::advance(proxy.recovery_delay()).await;
            proxy.backoff_recovery();
            assert!(proxy.recovery_delay() <= Duration::from_secs(30));
        }
        assert_eq!(proxy.descriptor().target, descriptor.target);
        assert_eq!(proxy.descriptor().after_sequence, Some(Epoch(31)));
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("CONTROL_CONFLICT"));
        assert!(!proxy.can_recover());
        assert_eq!(proxy.outcome().0, "blocked");
    }
    #[tokio::test]
    async fn recovery_consistency_paired_exit_publication_retains_code_and_target() {
        let root = tempfile::tempdir().unwrap();
        let (arrived, mut arrivals) = mpsc::channel(8);
        let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
        let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
        let (id, _) = runtime.get_or_rearm(descriptor.clone(), Arc::new(TerminalOutputHub::new(32)), service).unwrap();
        let socket = arrivals.recv().await.unwrap();
        let mut events = runtime.status_events.subscribe();
        socket.commands.send(FixtureCommand::Attach).await.unwrap();
        published_status(&mut events, &id, "proxy_attached").await;
        socket.commands.send(FixtureCommand::Exit).await.unwrap();
        let status = published_status(&mut events, &id, "affirmative_exit").await;
        assert_eq!(status.2, Some(0));
        assert_eq!(status.1, None);
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
        let outcome = runtime.recovery_outcome(&id).unwrap();
        assert_eq!(outcome.state, "affirmative_exit");
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.daemon_epoch, descriptor.target.daemon_epoch.0.to_string());
        runtime.detach(&id).await.unwrap();
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn recovery_consistency_paired_status_is_typed_and_target_fenced() {
        for (frame_type, wire_status, session, expected, reason) in [
            ("status", "running", "s", "proxy_attached", None),
            ("status", "exited", "s", "affirmative_exit", None),
            ("status", "expired", "s", "blocked", Some("SESSION_EXPIRED")),
            ("status", "exited", "stale", "blocked", Some("PAIRED_HOST_WRONG_MACHINE")),
            ("exit", "exited", "stale", "blocked", Some("PAIRED_HOST_WRONG_MACHINE")),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (arrived, mut arrivals) = mpsc::channel(8);
            let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
            let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
            let hub = Arc::new(TerminalOutputHub::new(32));
            let (id, _) = runtime.get_or_rearm(descriptor, hub.clone(), service).unwrap();
            let mut output = hub.subscribe_with_sequence(&id, None).unwrap().receiver;
            let socket = arrivals.recv().await.unwrap();
            let mut events = runtime.status_events.subscribe();
            socket.commands.send(FixtureCommand::Attach).await.unwrap();
            published_status(&mut events, &id, "proxy_attached").await;
            socket.commands.send(FixtureCommand::Frame(serde_json::json!({"type":frame_type,"target":{"machineId":"a","daemonEpoch":"1","sessionId":session},"status":wire_status,"exit":{"code":19,"signal":null}}))).await.unwrap();
            let status = published_status(&mut events, &id, expected).await;
            assert_eq!(status.3.as_deref(), reason);
            assert_eq!(status.2, None);
            if wire_status == "running" {
                assert_eq!(status.1, Some(Epoch(7)));
                socket.commands.send(FixtureCommand::Output(11)).await.unwrap();
                tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
            } else { assert_eq!(status.1, None); }
            runtime.detach(&id).await.unwrap();
            shutdown.send(()).unwrap();
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn recovery_consistency_paired_authority_rearm_requires_current_generation() {
        let root = tempfile::tempdir().unwrap();
        let (_, service, descriptor, shutdown, server) = start_mock_relay(&root, false, false).await;
        let mut proxy = Proxy::new(descriptor.clone(), Arc::new(TerminalOutputHub::new(32))).unwrap();
        proxy.set_recovery(service.clone());
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PAIRED_HOST_UNAUTHORIZED"));
        assert!(!proxy.rearm_authority("unrelated-host").await);
        assert_eq!(proxy.blocked_reason(), Some("PAIRED_HOST_UNAUTHORIZED"));
        assert!(proxy.rearm_authority(&descriptor.host_id).await);
        assert_eq!(proxy.blocked_reason(), None);
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("CONTROL_CONFLICT"));
        assert!(!proxy.rearm_authority(&descriptor.host_id).await);
        assert_eq!(proxy.blocked_reason(), Some("CONTROL_CONFLICT"));
        service.forget(descriptor.host_id.clone(), descriptor.generation).await.unwrap();
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PAIRED_HOST_STALE_GENERATION"));
        assert!(!proxy.rearm_authority(&descriptor.host_id).await);
        assert_eq!(proxy.blocked_reason(), Some("PAIRED_HOST_STALE_GENERATION"));
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn recovery_consistency_paired_stale_geometry_does_not_replace_desired_size() {
        let root = tempfile::tempdir().unwrap();
        let (_, service, descriptor, shutdown, server) = start_mock_relay(&root, false, false).await;
        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub).unwrap();
        proxy.reattach(&MachineClient::new(), &service).await.unwrap();
        proxy.resize(Epoch(7), 110, 32).await.unwrap();
        assert_eq!(proxy.resize(Epoch(6), 200, 60).await.unwrap_err().code, "STALE_GENERATION");
        proxy.mark_disconnected();
        assert_eq!(proxy.resize(Epoch(7), 200, 60).await.unwrap_err().code, "STALE_GENERATION");
        proxy.detach().await.unwrap();
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_consistency_paired_closed_authority_watch_disables_ready_branch() {
        let (sender, receiver) = tokio::sync::watch::channel((0, String::new()));
        let mut authority = Some(receiver);
        drop(sender);
        assert!(futures_util::FutureExt::now_or_never(authority_changed(&mut authority)).is_none());
        assert!(authority.is_none());
    }
    // The mock relay is a real loopback HTTP/WebSocket server, so socket I/O must run on the real
    // clock (an auto-advancing paused clock expires every timeout over it). Only the 30s backoff
    // is stepped: the clock is paused, advanced past the scheduled retry, and resumed.
    #[tokio::test]
    async fn recovery_consistency_paired_repeated_handshake_failures_keep_one_actor() {
        let root = tempfile::tempdir().unwrap();
        let (arrived, mut arrivals) = mpsc::channel(32);
        let (_, service, descriptor, shutdown, server) = start_mock_relay_controlled(&root, false, false, Some(arrived)).await;
        let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
        let hub = Arc::new(TerminalOutputHub::new(32));
        let (id, _) = runtime.get_or_rearm(descriptor.clone(), hub.clone(), service.clone()).unwrap();
        let incarnation = runtime.session_incarnation(&id).unwrap();
        let mut output = hub.subscribe_with_sequence(&id, None).unwrap().receiver;
        for _ in 0..21 {
            let failed = tokio::time::timeout(Duration::from_secs(35), arrivals.recv()).await.unwrap().unwrap();
            let mut recovery_events = runtime.subscribe_recovery_events(&id).await;
            failed.commands.send(FixtureCommand::Drop).await.unwrap();
            assert_eq!(runtime.session_incarnation(&id).unwrap(), incarnation);
            assert_eq!(runtime.list(), vec![id.clone()]);
            let failed_result = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let RecoveryTestEvent::Completed { failed } = recovery_events.recv().await.unwrap() {
                        break failed;
                    }
                }
            }).await.unwrap();
            assert!(failed_result, "drop must complete as a failed recovery result");
            #[cfg(feature = "local-split-qa")]
            {
                let audit = runtime.qa_dial_audit(&id).await.unwrap();
                let receipt = audit.recovery_events.last().unwrap();
                assert_eq!(receipt.kind, "recovery-attempt-failed");
                assert_eq!(receipt.backend_session_id, id);
                assert_eq!(receipt.target, descriptor.target);
                assert_eq!(receipt.incarnation, incarnation);
                assert_eq!(receipt.generation, None);
                assert!(receipt.failure_code.is_some());
                assert_eq!(audit.recovery_event_start_sequence, 1);
                assert_eq!(receipt.sequence as usize, audit.recovery_events.len());
                assert!(audit.recovery_events.windows(2).all(|events| events[1].sequence == events[0].sequence + 1));
            }
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if recovery_events.recv().await.unwrap() == RecoveryTestEvent::Scheduled {
                        break;
                    }
                }
            }).await.unwrap();
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(30)).await;
            tokio::time::resume();
        }
        let restored = tokio::time::timeout(Duration::from_secs(35), arrivals.recv()).await.unwrap().unwrap();
        restored.commands.send(FixtureCommand::Attach).await.unwrap();
        restored.commands.send(FixtureCommand::Output(42)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), output.recv()).await.unwrap().unwrap();
        assert_eq!(actor_barrier(&runtime, &id).await.0, "proxy_attached");
        assert_eq!(runtime.session_incarnation(&id).unwrap(), incarnation);
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
        runtime.detach(&id).await.unwrap();
        shutdown.send(()).unwrap();
        server.await.unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn recovery_consistency_paired_authority_deadline_preserves_block_and_services_commands() {
        let root = tempfile::tempdir().unwrap();
        let service = PairedHostService::open_test_loopback(root.path().join("authority"));
        let descriptor = Descriptor { host_id: "https://fixture.invalid/host/a".into(), generation: Epoch(1), target: RemoteTerminalTarget { machine_id: "a".into(), daemon_epoch: Epoch(1), session_id: "s".into() }, after_sequence: Some(Epoch(31)) };
        let runtime = Runtime::new(Some(root.path().join("descriptors.json")));
        let mut proxy = Proxy::new(descriptor.clone(), Arc::new(TerminalOutputHub::new(32))).unwrap();
        proxy.set_recovery(service.clone());
        proxy.recovery_failed(&crate::paired_host::client::ClientError::local("PAIRED_HOST_UNAUTHORIZED"));
        let mut events = runtime.status_events.subscribe();
        let id = runtime.install(proxy).unwrap();
        published_status(&mut events, &id, "blocked").await;
        let (entered, release) = service.test_hold_verification();
        service.test_authority_event(&descriptor.host_id);
        entered.await.unwrap();
        let (reply, mut result) = oneshot::channel();
        runtime.sender(&id).unwrap().try_send(Command::Status(reply)).unwrap();
        assert!(matches!(result.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
        tokio::time::advance(Duration::from_secs(30)).await;
        let status = tokio::time::timeout(Duration::from_secs(5), result).await.unwrap().unwrap();
        assert_eq!(status.0, "blocked");
        assert_eq!(status.3.as_deref(), Some("PAIRED_HOST_UNAUTHORIZED"));
        assert_eq!(status.1, None);
        assert_eq!(runtime.descriptor(&id).unwrap().target, descriptor.target);
        let _ = release.send(());
        runtime.detach(&id).await.unwrap();
    }
    async fn start_mock_relay(
        temp_dir: &tempfile::TempDir,
        send_output_frames: bool,
        malformed_agent_state: bool,
    ) -> (
        String,
        PairedHostService,
        Descriptor,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        start_mock_relay_controlled(temp_dir, send_output_frames, malformed_agent_state, None).await
    }
    async fn start_mock_relay_controlled(
        temp_dir: &tempfile::TempDir,
        send_output_frames: bool,
        malformed_agent_state: bool,
        arrivals: Option<mpsc::Sender<FixtureSocket>>,
    ) -> (String, PairedHostService, Descriptor, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        let router = Router::new()
            .route(
                "/api/v1/pair/exchange",
                post(|| async {
                    Json(json!({
                        "token": "fixture-secret",
                        "machineId": "a",
                        "device": {
                            "id": "d",
                            "name": "d",
                            "permission": "control",
                            "accessScope": "machine",
                            "createdAt": 1,
                            "lastSeenAt": 1
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/capabilities",
                get(|| async {
                    Json(json!({
                        "apiVersion": 1,
                        "machineId": "a",
                        "daemonEpoch": "1",
                        "platform": "linux",
                        "accessScope": "machine",
                        "permission": "control",
                        "capabilities": ["terminalCreateV1", "terminalStreamV1"],
                        "limits": {"directoryEntries": 1000, "terminalSessions": 64}
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/sessions/s",
                get(|| async {
                    Json(json!({
                        "status": "running",
                        "session": {
                            "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
                            "workspaceId": "w",
                            "worktree": null,
                            "cwd": "/fixture",
                            "cols": 80,
                            "rows": 24,
                            "running": true,
                            "providerSession": null,
                            "startSequence": "10",
                            "endSequence": "10"
                        }
                    }))
                }),
            )
            .route(
                "/host/a/api/v1/socket-ticket",
                post(|| async { Json(json!({"ticket": "fixture-ticket"})) }),
            )
            .route(
                "/host/a/api/v1/terminal/s",
                get(move |ws: WebSocketUpgrade| {
                    let arrivals = arrivals.clone();
                    async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let (commands, mut control) = mpsc::channel(32);
                        let (receipts, received) = mpsc::channel(64);
                        let controlled = arrivals.is_some();
                        if let Some(arrivals) = arrivals {
                            arrivals.send(FixtureSocket { commands, received }).await.unwrap();
                            match control.recv().await {
                                Some(FixtureCommand::Attach) => {},
                                _ => return,
                            }
                        }
                        if socket
                            .send(Message::Text(
                                json!({
                                    "type": "attached",
                                    "target": {"machineId": "a", "daemonEpoch": "1", "sessionId": "s"},
                                    "generation": "7",
                                    "cols": 80,
                                    "rows": 24,
                                    "startSequence": "10",
                                    "endSequence": "10",
                                    "replayGap": null
                                })
                                .to_string()
                                .into(),
                            ))
                            .await.is_err() { return; }
                        if malformed_agent_state {
                            // Required field `state` omitted: a decode failure, not a
                            // frame the boundary may silently skip.
                            let _ = socket
                                .send(Message::Text(
                                    json!({
                                        "type": "agent_state",
                                        "target": {
                                            "machineId": "a",
                                            "daemonEpoch": "1",
                                            "sessionId": "s"
                                        }
                                    })
                                    .to_string()
                                    .into(),
                                ))
                                .await;
                        }
                        if send_output_frames {
                            for seq in 11..=20 {
                                let frame = encode_frame(
                                    Metadata::Output {
                                        sequence: seq,
                                        gap: None,
                                    },
                                    b"x",
                                    false,
                                )
                                .unwrap();
                                let _ = socket.send(Message::Binary(frame.into())).await;
                            }
                        }
                        loop {
                            tokio::select! { biased;
                                command = control.recv(), if controlled => match command {
                                    Some(FixtureCommand::Drop) | None => break,
                                    Some(FixtureCommand::Attach) => {},
                                    Some(FixtureCommand::Output(sequence)) => {
                                        let frame = encode_frame(Metadata::Output { sequence, gap: None }, b"receipt", false).unwrap();
                                        if socket.send(Message::Binary(frame.into())).await.is_err() { break; }
                                    }
                                    Some(FixtureCommand::Exit) => {
                                        socket.send(Message::Text(json!({"type":"exit","target":{"machineId":"a","daemonEpoch":"1","sessionId":"s"},"exit":{"code":0,"signal":null}}).to_string().into())).await.unwrap();
                                    }
                                    Some(FixtureCommand::Frame(value)) => {
                                        if socket.send(Message::Text(value.to_string().into())).await.is_err() { break; }
                                    }
                                },
                                message = socket.recv() => match message {
                                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                                    Some(Ok(message)) => {
                                        if controlled { receipts.send(message).await.unwrap(); }
                                    }
                                },
                            }
                        }
                    })
                }}),
            );

        let (shutdown, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });

        let service = PairedHostService::open_test_loopback(temp_dir.path().join("service_data"));
        let host = service
            .pair(crate::paired_host::service::PairRequest {
                relay_origin: format!("http://{address}"),
                pin: crate::paired_host::service::Secret("fixture".into()),
                display_label: "fixture".into(),
            })
            .await
            .unwrap();

        let descriptor = Descriptor {
            host_id: host.host_id.clone(),
            generation: host.generation,
            target: RemoteTerminalTarget {
                machine_id: "a".into(),
                daemon_epoch: Epoch(1),
                session_id: "s".into(),
            },
            after_sequence: None,
        };

        (
            format!("http://{address}"),
            service,
            descriptor,
            shutdown,
            server,
        )
    }

    #[tokio::test]
    async fn test_output_batch_debounce_and_detach_persists_latest_cursor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, true, false).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub.clone()).unwrap();
        let mut output = hub
            .subscribe_with_sequence(proxy.id(), None)
            .unwrap()
            .receiver;
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        let id = runtime.install(proxy).unwrap();
        runtime.flush_descriptors().await.unwrap();
        // install persists once initially
        let initial_persists = test_persist_count_path(&store_path);

        // Wait until all 10 output chunks are delivered to the output hub
        for _ in 0..10 {
            tokio::time::timeout(Duration::from_secs(5), output.recv())
                .await
                .unwrap()
                .unwrap();
        }

        let batches_persists = test_persist_count_path(&store_path) - initial_persists;
        assert!(
            batches_persists <= 2,
            "10 sequential output batches must cause at most 2 persists, got {batches_persists}"
        );

        // Final detach must synchronously persist the latest cursor (sequence 20)
        runtime.detach(&id).await.unwrap();

        let loaded = load_descriptors(&store_path).unwrap();
        let persisted_desc = loaded.get(&id).expect("descriptor present after detach");
        assert_eq!(
            persisted_desc.after_sequence,
            Some(Epoch(20)),
            "detach must persist the latest cursor"
        );

        let _ = shutdown.send(());
        let _ = server.await;
    }

    /// A frame whose required fields do not decode must surface as a transport
    /// contract violation. Dropping it silently would leave the owner loop
    /// receiving while authoritative agent state is lost.
    #[tokio::test]
    async fn test_malformed_agent_state_frame_fails_the_transport_boundary() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, false, true).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub).unwrap();
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let failure = proxy
            .receive()
            .await
            .err()
            .expect("malformed agent_state frame must fail the boundary");
        assert_eq!(
            failure.code, "PAIRED_HOST_INVALID_RESPONSE",
            "{failure:?}"
        );

        let _ = shutdown.send(());
        let _ = server.await;
    }

    #[tokio::test]
    async fn test_backpressure_full_channel_write_succeeds_once_slots_free() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, false, false).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub).unwrap();
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Arc::new(Runtime::default());
        let id = runtime.install(proxy).unwrap();

        let (blocked_tx, blocked_rx) = oneshot::channel();
        let (unblock_tx, unblock_rx) = oneshot::channel();
        runtime.block_actor(&id, blocked_tx, unblock_rx);
        blocked_rx.await.expect("actor confirmed blocked");

        let sender = runtime.sender(&id).unwrap();
        // Channel capacity is 32. Fill all 32 buffer slots
        for _ in 0..32 {
            let (tx, _rx) = oneshot::channel();
            sender
                .try_send(Command::Write(0, b"filler".to_vec(), tx))
                .unwrap();
        }
        // Channel is now pre-filled to capacity
        let (probe_tx, _probe_rx) = oneshot::channel();
        assert!(
            sender
                .try_send(Command::Write(0, b"probe".to_vec(), probe_tx))
                .is_err(),
            "channel must be pre-filled to capacity"
        );

        // Initiate a write on full channel. Bounded backpressure waits for a slot.
        let (write_entered_tx, write_entered_rx) = oneshot::channel();
        let write_fut = {
            let runtime = runtime.clone();
            let id = id.clone();
            tokio::spawn(async move {
                let op = runtime.write(&id, 0, b"keystroke".to_vec()).unwrap();
                let _ = write_entered_tx.send(());
                op.await
            })
        };

        // Ensure write_fut has entered and is waiting on backpressure
        write_entered_rx.await.expect("write future entered");
        tokio::task::yield_now().await;
        assert!(
            !write_fut.is_finished(),
            "write must wait on backpressure instead of immediately dropping keystroke"
        );

        // Free slots by unblocking the actor
        let _ = unblock_tx.send(());

        let res = tokio::time::timeout(Duration::from_secs(5), write_fut)
            .await
            .expect("write must complete within timeout")
            .unwrap();

        assert!(
            res.is_ok(),
            "write must succeed with no PAIRED_PROXY_UNAVAILABLE once slots free: {res:?}"
        );

        let _ = runtime.detach(&id).await;
        let _ = shutdown.send(());
        let _ = server.await;
    }

    #[tokio::test]
    async fn test_actor_abort_inside_debounce_persists_latest_cursor() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_path = temp_dir.path().join("paired_descriptors.json");
        let (_relay_origin, service, descriptor, shutdown, server) =
            start_mock_relay(&temp_dir, true, false).await;

        let hub = Arc::new(TerminalOutputHub::new(32));
        let mut proxy = Proxy::new(descriptor, hub.clone()).unwrap();
        let mut output = hub
            .subscribe_with_sequence(proxy.id(), None)
            .unwrap()
            .receiver;
        proxy
            .reattach(&MachineClient::new(), &service)
            .await
            .unwrap();

        let runtime = Runtime::new(Some(store_path.clone()));
        let id = runtime.install(proxy).unwrap();

        // Wait until all 10 output chunks (sequences 11..=20) are delivered to output hub
        for _ in 0..10 {
            tokio::time::timeout(Duration::from_secs(5), output.recv())
                .await
                .unwrap()
                .unwrap();
        }

        // Chunks 11..=20 were received; chunks 12..=20 arrived inside the debounce window.
        // Abort the actor task before the debounce timer elapses.
        runtime.abort_actor(&id).await;

        // On-disk snapshot must carry the latest cursor (sequence 20), saved by the drop guard.
        let loaded = load_descriptors(&store_path).unwrap();
        let persisted_desc = loaded.get(&id).expect("descriptor present after abort");
        assert_eq!(
            persisted_desc.after_sequence,
            Some(Epoch(20)),
            "abort inside debounce window must persist the latest cursor"
        );

        let _ = shutdown.send(());
        let _ = server.await;
    }

    /// The identity the attach fence proves for a paired session: minted by the runtime per
    /// actor, constant for that actor's life, and never reused by a replacement actor.
    #[tokio::test]
    async fn paired_session_incarnation_is_stable_per_actor_and_distinct_across_reincarnation() {
        let runtime = Runtime::default();
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "incarnation-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "incarnation-session".into(),
            },
            after_sequence: None,
        };
        let first = runtime
            .install(
                Proxy::new(descriptor.clone(), Arc::new(TerminalOutputHub::new(32)))
                    .expect("proxy construction"),
            )
            .expect("install paired proxy");
        let incarnation = runtime
            .session_incarnation(&first)
            .expect("a live actor must report an incarnation");
        assert!(!incarnation.is_empty(), "an incarnation must be a real value");
        assert_eq!(
            runtime.session_incarnation(&first).as_deref(),
            Some(incarnation.as_str()),
            "the incarnation must be stable for the actor's whole life"
        );

        // A reattach installs a replacement actor for the same logical session id.
        runtime.force_reap_owner(&first);
        assert_eq!(
            runtime.session_incarnation(&first),
            None,
            "a session with no live actor must not report an incarnation"
        );
        let second = runtime
            .install(
                Proxy::new(descriptor, Arc::new(TerminalOutputHub::new(32)))
                    .expect("proxy construction"),
            )
            .expect("install replacement proxy");
        assert_eq!(
            second, first,
            "the same descriptor names the same paired session id"
        );
        assert_ne!(
            runtime.session_incarnation(&second).as_deref(),
            Some(incarnation.as_str()),
            "a replacement actor must not reuse the previous incarnation"
        );
    }

    #[tokio::test]
    async fn retry_owned_requires_exact_epoch_and_incarnation_and_rearms_same_actor() {
        let runtime = Runtime::default();
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "retry-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "retry-session".into(),
            },
            after_sequence: None,
        };
        let id = runtime.install(Proxy::new(descriptor, Arc::new(TerminalOutputHub::new(32))).unwrap()).unwrap();
        let incarnation = runtime.session_incarnation(&id).unwrap();
        let stale_epoch = runtime.retry_owned(&id, "11", &incarnation).await.unwrap();
        assert_eq!(stale_epoch, RetryOwnedResult::stale_tuple());
        let stale_incarnation = runtime.retry_owned(&id, "10", "not-current").await.unwrap();
        assert_eq!(stale_incarnation, RetryOwnedResult::stale_tuple());
        assert!(stale_epoch.protocol_flags_valid());
        assert!(stale_incarnation.protocol_flags_valid());
        let admitted = runtime.retry_owned(&id, "10", &incarnation).await.unwrap();
        assert_eq!(admitted, RetryOwnedResult::admitted());
        assert!(admitted.protocol_flags_valid());
        assert_eq!(runtime.session_incarnation(&id).as_deref(), Some(incarnation.as_str()));
    }

    #[tokio::test]
    async fn cancelled_queued_retry_does_not_schedule_recovery() {
        let runtime = Runtime::default();
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "cancel-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "cancel-session".into(),
            },
            after_sequence: None,
        };
        let id = runtime.install(Proxy::new(descriptor, Arc::new(TerminalOutputHub::new(32))).unwrap()).unwrap();
        let incarnation = runtime.session_incarnation(&id).unwrap();
        let release = runtime.hold_actor_mailbox(&id).await.unwrap();
        let sender = runtime.sender(&id).unwrap();
        let (reply, result) = oneshot::channel();
        assert!(sender.try_send(Command::RetryOwned { daemon_epoch: "10".into(), incarnation, reply }).is_ok());
        drop(result);
        let (snapshot, observed) = oneshot::channel();
        assert!(sender.try_send(Command::RecoveryScheduled(snapshot)).is_ok());
        release.send(()).unwrap();
        assert!(!tokio::time::timeout(Duration::from_secs(5), observed).await.unwrap().unwrap());
        runtime.detach(&id).await.unwrap();
    }

    #[tokio::test]
    async fn recovery_outcome_subscription_captures_retained_tuple() {
        let runtime = Runtime::default();
        let descriptor = Descriptor {
            host_id: "https://relay.example.com".into(),
            generation: Epoch(1),
            target: RemoteTerminalTarget {
                machine_id: "outcome-machine".into(),
                daemon_epoch: Epoch(10),
                session_id: "outcome-session".into(),
            },
            after_sequence: None,
        };
        let id = runtime.install(Proxy::new(descriptor, Arc::new(TerminalOutputHub::new(32))).unwrap()).unwrap();
        let mut events = runtime.subscribe_recovery_outcome(&id).unwrap();
        let snapshot = events.borrow_and_update().clone();
        assert_eq!(snapshot.backend_session_id, id);
        assert_eq!(snapshot.daemon_epoch, "10");
        assert_eq!(snapshot.incarnation, runtime.session_incarnation(&id).unwrap());
        assert_eq!(snapshot.state, "owner_unavailable");
        assert_eq!(snapshot.generation, None);
        assert_eq!(snapshot.exit_code, None);
        runtime.detach(&id).await.unwrap();
    }

    #[cfg(feature = "local-split-qa")]
    #[tokio::test]
    async fn qa_mutation_sender_is_bound_before_same_generation_replacement() {
        for operation in ["write", "signal"] {
            let runtime = Runtime::default();
            let descriptor = Descriptor {
                host_id: "https://relay.example.com".into(), generation: Epoch(7),
                target: RemoteTerminalTarget { machine_id: "machine".into(), daemon_epoch: Epoch(10), session_id: "session".into() },
                after_sequence: Some(Epoch(11)),
            };
            let id = runtime.install(Proxy::new(descriptor, Arc::new(TerminalOutputHub::new(32))).unwrap()).unwrap();
            let (a, mut received_a) = mpsc::channel(2);
            let (b, mut received_b) = mpsc::channel(2);
            {
                let mut owners = runtime.owners.lock();
                let owner = owners.get_mut(&id).unwrap();
                owner.sender = a;
                owner.incarnation = "actor-a".into();
            }
            let pending = runtime.qa_mutation_owned(&id, "10", "actor-a", 7, operation, "intent-a".into(), b"payload".to_vec()).unwrap();
            // Replace the map entry's dispatch identity before the future is polled.
            // Both endpoints accept generation 7, so a generation mismatch cannot hide retargeting.
            {
                let mut owners = runtime.owners.lock();
                let owner = owners.get_mut(&id).unwrap();
                owner.sender = b;
                owner.incarnation = "actor-b".into();
            }
            for (epoch, incarnation) in [("10", "actor-a"), ("11", "actor-b")] {
                let error = runtime.qa_mutation_owned(&id, epoch, incarnation, 7, operation, "stale".into(), vec![]).err().unwrap();
                assert!(matches!(error, super::super::PtyError::Other(message) if message == "PAIRED_PROXY_STALE_TUPLE"));
            }
            let task = tokio::spawn(pending);
            let command = tokio::time::timeout(Duration::from_secs(5), received_a.recv()).await.unwrap().unwrap();
            match command {
                Command::Write(7, data, reply) if operation == "write" => {
                    assert_eq!(data, b"payload"); reply.send(Ok(())).unwrap();
                }
                Command::Interrupt(7, intent, reply) if operation == "signal" => {
                    assert_eq!(intent.as_deref(), Some("intent-a")); reply.send(Ok(())).unwrap();
                }
                _ => panic!("wrong mutation command"),
            }
            task.await.unwrap().unwrap();
            assert!(matches!(received_b.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
            let current = runtime.qa_mutation_owned(&id, "10", "actor-b", 7, operation, "intent-b".into(), vec![]).unwrap();
            let task = tokio::spawn(current);
            let command = tokio::time::timeout(Duration::from_secs(5), received_b.recv()).await.unwrap().unwrap();
            match command {
                Command::Write(7, _, reply) if operation == "write" => { reply.send(Ok(())).unwrap(); }
                Command::Interrupt(7, intent, reply) if operation == "signal" => {
                    assert_eq!(intent.as_deref(), Some("intent-b")); reply.send(Ok(())).unwrap();
                }
                _ => panic!("wrong replacement command"),
            }
            task.await.unwrap().unwrap();
            runtime.detach(&id).await.unwrap();
        }
    }
}
