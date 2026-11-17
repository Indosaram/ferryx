//! Daemon-owned local forwards. No master idle grace, PTY, or on-disk log.
//!
//! Lock order for the dispatch owner: definitions flock -> registry -> runtime
//! sidecar. Detach/request kills under the mutex, persist revisions synchronously
//! while still holding the flock, release both locks, THEN await post-wait watches.
//! Draining is a property of a generation, including generations of retained failed
//! entries. It reserves its port until wait has completed, regardless of caller.
use super::{argv::{self, ConfigSource}, *};
use super::runtime_records::{RuntimeRecords, SweptRecord};
use crate::ssh::{password, SshHost};
use crate::ssh::tunnel_store::{Clock, SystemClock};
use serde::Serialize;
use std::{collections::{HashMap, VecDeque}, future::Future, io, net::{Ipv4Addr, SocketAddr}, path::{Path, PathBuf}, pin::Pin, process::{ExitStatus, Stdio}, sync::{Arc, Mutex, Weak, atomic::{AtomicUsize, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{io::{AsyncRead, AsyncReadExt}, net::{TcpSocket, TcpStream}, sync::{mpsc, watch}, time::{Instant, timeout}};

const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(6);
const TERM_GRACE: Duration = Duration::from_secs(2);
const START_DEADLINE: Duration = Duration::from_secs(10);
const PROBE_DEADLINE: Duration = Duration::from_millis(200);
const CHANNEL_DEADLINE: Duration = Duration::from_secs(2);
type Wait<'a> = Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal { Term, Kill }
/// The production child is tokio::process::Child, never std::process::Child.
/// Scripted children implement this seam only for deterministic state timing.
pub trait TunnelChild: Send {
    fn id(&self) -> u32;
    fn wait(&mut self) -> Wait<'_>;
}
impl TunnelChild for tokio::process::Child {
    fn id(&self) -> u32 { self.id().expect("unreaped tunnel child") }
    fn wait(&mut self) -> Wait<'_> { Box::pin(self.wait()) }
}
pub struct SpawnedTunnel {
    pub child: Box<dyn TunnelChild>,
    pub stderr: Pin<Box<dyn AsyncRead + Send>>,
}
pub trait TunnelSpawner: Send + Sync {
    fn spawn(&self, argv: &[String]) -> io::Result<SpawnedTunnel>;
    fn kill_group(&self, pgid: i32, sig: Signal);
}
#[derive(Default)]
pub struct ProcessSpawner;
impl TunnelSpawner for ProcessSpawner {
    fn spawn(&self, argv: &[String]) -> io::Result<SpawnedTunnel> {
        let mut command = tokio::process::Command::new(&argv[0]);
        command.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        for (key, value) in password::environment(argv).map_err(|e| io::Error::other(e.message))? {
            command.env(key, value);
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        let stderr = child.stderr.take().expect("piped stderr");
        Ok(SpawnedTunnel { child: Box::new(child), stderr: Box::pin(stderr) })
    }
    fn kill_group(&self, pgid: i32, sig: Signal) {
        #[cfg(unix)]
        // Only the reaper calls this, while owning the unreaped leader. Its pid
        // cannot be recycled. Checking pgid anywhere after reap would be unsound.
        unsafe {
            if libc::getpgid(pgid) == pgid {
                libc::kill(-pgid, match sig { Signal::Term => libc::SIGTERM, Signal::Kill => libc::SIGKILL });
            }
        }
        #[cfg(not(unix))]
        let _ = (pgid, sig);
    }
}

#[derive(Default)]
pub struct ProbeSocketFactory {
    #[cfg(test)]
    pub availability_source_port_override: Mutex<Option<u16>>,
    #[cfg(test)]
    pub readiness_source_port_override: Mutex<Option<u16>>,
    #[cfg(test)]
    pub readiness_override_consumed: Arc<tokio::sync::Notify>,
    #[cfg(test)]
    pub readiness_override_consumptions: AtomicUsize,
}
impl ProbeSocketFactory {
    async fn connect(&self, addr: SocketAddr, readiness: bool) -> io::Result<(TcpStream, bool)> {
        let socket = TcpSocket::new_v4()?;
        #[allow(unused_mut)]
        let mut overridden = false;
        #[cfg(test)]
        {
            let source = if readiness { &self.readiness_source_port_override } else { &self.availability_source_port_override };
            if let Some(port) = source.lock().unwrap().take() {
                socket.bind((Ipv4Addr::LOCALHOST, port).into()).expect("probe source override must bind");
                overridden = true;
                if readiness {
                    self.readiness_override_consumptions.fetch_add(1, Ordering::SeqCst);
                    self.readiness_override_consumed.notify_one();
                }
            }
        }
        #[cfg(not(test))]
        let _ = readiness;
        socket.connect(addr).await.map(|stream| (stream, overridden))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortAvailability { Available, Occupied, SelfConnected }
pub fn is_self_connect(local: SocketAddr, peer: SocketAddr) -> bool { local == peer }
fn self_connected(socket: &TcpStream) -> io::Result<bool> {
    Ok(is_self_connect(socket.local_addr()?, socket.peer_addr()?))
}
/// Never bind to check availability: reuse-address diverges across BSD/Linux and
/// disabling it rejects our own TIME_WAIT sockets. Equal endpoints mean TCP
/// simultaneous open without a listener, NOT our own ssh listener.
pub async fn probe_local_port_available(addr: SocketAddr, factory: &ProbeSocketFactory) -> PortAvailability {
    match timeout(PROBE_DEADLINE, factory.connect(addr, false)).await {
        Ok(Ok((socket, _))) if self_connected(&socket).unwrap_or(false) => PortAvailability::SelfConnected,
        Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => PortAvailability::Available,
        _ => PortAvailability::Occupied,
    }
}
fn allocate_port() -> io::Result<u16> {
    // Allocation HINT, not a reservation. Close before ssh receives the exact port.
    let probe = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = probe.local_addr()?.port();
    drop(probe);
    Ok(port)
}
fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}
fn io_error(reason: &str) -> IpcError {
    IpcError::new(IpcErrorCode::IoError, "Tunnel runtime I/O failed").with_details(serde_json::json!({"reason": reason}))
}
fn closed_error() -> IpcError {
    IpcError::new(IpcErrorCode::RequestConflict, "Daemon is shutting down").with_details(serde_json::json!({"reason": "daemon_shutting_down"}))
}
fn unsupported_path() -> IpcError {
    IpcError::new(IpcErrorCode::InvalidArgument, "Unsafe tunnel config path").with_details(serde_json::json!({"reason": "unsupported_config", "piece": "config_path"}))
}
fn ensure_empty_config(dir: &Path) -> Result<PathBuf, IpcError> {
    let path = dir.join("ssh-tunnel-empty.conf");
    if !path.to_string_lossy().chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) {
        return Err(unsupported_path());
    }
    crate::daemon::server::ensure_runtime_directory(dir).map_err(|_| io_error("empty_config_unwritable"))?;
    if argv::validate_empty_config(&path).is_ok() { return Ok(path); }
    // Replace suspect inodes without following symlinks or truncating an existing
    // target. A trusted existing inode is reused without any rewrite.
    let temp = dir.join(format!(".ssh-tunnel-empty-{}", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600).custom_flags(libc::O_NOFOLLOW); }
    let result = (|| {
        let file = options.open(&temp)?; file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() {
        if let Err(e) = std::fs::remove_file(&temp) { if e.kind() != io::ErrorKind::NotFound { tracing::warn!("Cannot remove temporary tunnel config: {e}"); } }
        return Err(io_error("empty_config_unwritable"));
    }
    argv::validate_empty_config(&path)?;
    Ok(path)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelTransition { pub at: u64, pub from: TunnelState, pub to: TunnelState, pub code: Option<TunnelFailureCode> }
/// Counted-entry shape only: no master IDLE_GRACE or delayed release.
pub struct TunnelLease { active: Arc<AtomicUsize>, live: watch::Sender<usize>, held: bool }
impl TunnelLease {
    fn new(active: Arc<AtomicUsize>, live: watch::Sender<usize>) -> Self { let count = active.fetch_add(1, Ordering::SeqCst) + 1; live.send_replace(count); Self { active, live, held: true } }
    fn release(&mut self) { if self.held { self.held = false; let count = self.active.fetch_sub(1, Ordering::SeqCst) - 1; self.live.send_replace(count); } }
}
impl Drop for TunnelLease { fn drop(&mut self) { self.release(); } }
#[derive(Clone)]
pub struct Generation {
    pub generation: u64,
    pub local_port: u16,
    pub pid: u32,
    kill: mpsc::UnboundedSender<()>,
    pub exit: watch::Receiver<Option<ExitStatus>>,
}
impl Generation {
    fn request_kill(&self) { let _ = self.kill.send(()); }
    pub async fn wait(&mut self) -> Result<(), IpcError> {
        while self.exit.borrow().is_none() {
            self.exit.changed().await.map_err(|_| io_error("child_reap_failed"))?;
        }
        Ok(())
    }
}
pub struct ChildGuard {
    cancel: watch::Sender<bool>,
    pub current: Option<Generation>,
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
        if let Some(child) = &self.current { child.request_kill(); }
    }
}
pub struct TunnelEntry {
    pub view: TunnelView,
    pub child: ChildGuard,
    pub lease: TunnelLease,
    pub log: VecDeque<TunnelTransition>,
    state: watch::Sender<TunnelState>,
    spec: TunnelSpec,
}
impl TunnelEntry {
    fn transition(&mut self, to: TunnelState, code: Option<TunnelFailureCode>, piece: Option<String>, now: u64) {
        let from = self.view.state;
        self.view.state = to; self.view.since = Some(now);
        self.view.failure = code; self.view.failure_piece = piece;
        if self.log.len() == 100 { self.log.pop_front(); }
        self.log.push_back(TunnelTransition { at: now, from, to, code });
        self.state.send_replace(to);
        if to == TunnelState::Failed { self.lease.release(); }
    }
}
struct RegistryState {
    entries: HashMap<TunnelRuntimeId, TunnelEntry>,
    draining: Vec<Generation>,
    closed: bool,
}
enum OwnedConfig { Real, Fixture { user: String, system: String } }
impl OwnedConfig {
    fn source(&self) -> ConfigSource<'_> {
        match self { Self::Real => ConfigSource::Real, Self::Fixture { user, system } => ConfigSource::Fixture { user, system } }
    }
}
struct Core {
    state: Mutex<RegistryState>,
    spawner: Arc<dyn TunnelSpawner>,
    config: OwnedConfig,
    probes: Arc<ProbeSocketFactory>,
    active: Arc<AtomicUsize>,
    /// Live-runtime count, so the host observer can be cancelled on idle and
    /// re-armed by the first runtime that becomes live after that.
    live: watch::Sender<usize>,
    clock: Arc<dyn Clock>,
    records: Option<Arc<RuntimeRecords>>,
    generation: AtomicUsize,
    runtime_dir: PathBuf,
}
pub struct TunnelRegistry { core: Arc<Core> }
impl TunnelRegistry {
    /// The clock seam is injected here so every timing decision in the daemon
    /// (identity grace, transition stamps) is driven by one fake in tests.
    pub fn new(spawner: Arc<dyn TunnelSpawner>, config_source: ConfigSource<'_>, probe_factory: Arc<ProbeSocketFactory>, clock: Arc<dyn Clock>) -> Self {
        Self { core: Arc::new(Core {
            state: Mutex::new(RegistryState { entries: HashMap::new(), draining: Vec::new(), closed: false }),
            spawner, config: match config_source { ConfigSource::Real => OwnedConfig::Real, ConfigSource::Fixture { user, system } => OwnedConfig::Fixture { user: user.into(), system: system.into() } },
            probes: probe_factory, active: Arc::new(AtomicUsize::new(0)), live: watch::channel(0).0, clock,
            records: None, generation: AtomicUsize::new(0), runtime_dir: crate::daemon::server::get_runtime_dir(),
        }) }
    }
    /// Attach the crash-visible runtimes sidecar. Called once at startup, before
    /// the registry is shared; without it a start leaves no reclaimable record.
    pub fn with_records(mut self, records: Arc<RuntimeRecords>) -> Self {
        Arc::get_mut(&mut self.core).expect("records are attached before the registry is shared").records = Some(records);
        self
    }
    pub fn clock(&self) -> u64 { self.core.clock.now_unix_ms() }
    pub fn records(&self) -> Option<&Arc<RuntimeRecords>> { self.core.records.as_ref() }
    /// The sweep lives with the child owner because only the registry holds the
    /// spawner that can kill a process group.
    pub fn sweep_records(&self, records: &RuntimeRecords) -> Vec<SweptRecord> {
        records.sweep(&*self.core.spawner)
    }
    /// Resolves once any runtime is live. The observer cancels on idle and calls
    /// this again, which is what re-arms it for the next live runtime.
    pub async fn wait_until_live(&self) {
        let mut rx = self.core.live.subscribe();
        while *rx.borrow() == 0 { if rx.changed().await.is_err() { return; } }
    }
    pub async fn wait_until_idle(&self) {
        let mut rx = self.core.live.subscribe();
        while *rx.borrow() != 0 { if rx.changed().await.is_err() { return; } }
    }
    /// Daemon-path tests place the empty config inside their own fixture directory
    /// instead of the operator's runtime directory. Behavior is otherwise identical.
    #[cfg(all(test, unix))]
    pub fn with_runtime_dir(mut self, dir: &Path) -> Self {
        Arc::get_mut(&mut self.core).expect("registry must be unshared").runtime_dir = dir.to_path_buf();
        self
    }
    /// Saved starts are idempotent by host+spec id. Ad-hoc ids are minted BEFORE
    /// validation and never persisted, regardless of a client-supplied id.
    pub async fn start(&self, host: &SshHost, mut spec: TunnelSpec, saved: bool) -> Result<TunnelView, IpcError> {
        if self.core.state.lock().unwrap().closed { return Err(closed_error()); }
        if !saved { spec.id = TunnelSpecId(uuid::Uuid::new_v4().to_string()); }
        if saved { validate_saved_spec(&spec)?; } else { validate_spec(&spec)?; }
        let port = spec.local_port.map(Ok).unwrap_or_else(allocate_port).map_err(|_| io_error("port_allocation_failed"))?;
        // Creation errors remain IO_ERROR; unsupported config paths become failed
        // runtime views below, so details.piece never escapes on a wire Err.
        if let Err(error) = ensure_empty_config(&self.core.runtime_dir) {
            if error.code == IpcErrorCode::IoError { return Err(error); }
        }
        let mut id = TunnelRuntimeId(uuid::Uuid::new_v4().to_string());
        let mut state_rx;
        loop {
            let pending = {
                let mut state = self.core.state.lock().unwrap();
                if state.closed { return Err(closed_error()); }
                state.draining.retain(|g| g.exit.borrow().is_none());
                if saved {
                    if let Some(entry) = state.entries.values().find(|e| e.view.host_id == host.id && e.view.spec_id == spec.id && e.view.state != TunnelState::Failed) { return Ok(entry.view.clone()); }
                }
                let draining: Vec<_> = state.draining.iter().filter(|g| g.local_port == port).cloned().collect();
                if !draining.is_empty() { draining } else {
                    if saved {
                        let previous = state.entries.iter().find(|(_, e)| e.view.host_id == host.id && e.view.spec_id == spec.id).map(|(id, _)| id.clone());
                        if let Some(previous) = previous { state.entries.remove(&previous); id = previous; }
                    }
                    let occupied = state.entries.values().any(|e| e.view.local_port == port && e.view.state != TunnelState::Failed);
                    let (cancel, _) = watch::channel(false);
                    let (tx, rx) = watch::channel(TunnelState::Reconnecting); state_rx = rx;
                    let mut entry = TunnelEntry {
                        view: TunnelView { spec_id: spec.id.clone(), runtime_id: Some(id.clone()), label: spec.label.clone(), host_id: host.id.clone(), local_port: port, remote_host: spec.remote_host.clone(), remote_port: spec.remote_port, state: TunnelState::Reconnecting, since: Some(now()), attempt: 0, failure: None, failure_piece: None },
                        child: ChildGuard { cancel, current: None }, lease: TunnelLease::new(self.core.active.clone(), self.core.live.clone()), log: VecDeque::new(), state: tx, spec: spec.clone(),
                    };
                    if occupied { entry.transition(TunnelState::Failed, Some(TunnelFailureCode::PortInUse), None, self.core.clock.now_unix_ms()); }
                    let view = entry.view.clone();
                    state.entries.insert(id.clone(), entry);
                    if occupied { return Ok(view); }
                    break;
                }
            };
            wait_generations(pending).await?;
        }
        let cancel = {
            let state = self.core.state.lock().unwrap();
            if state.closed { return Err(closed_error()); }
            state.entries.get(&id).ok_or_else(|| IpcError::internal("Tunnel start was cancelled"))?.child.cancel.subscribe()
        };
        let weak = Arc::downgrade(&self.core); let task_id = id.clone(); let host = host.clone();
        tokio::runtime::Handle::try_current().map_err(|_| IpcError::internal("Tunnel start requires a runtime"))?.spawn(async move { run_entry(weak, task_id, host, cancel).await; });
        let outcome = timeout(START_DEADLINE, async {
            loop {
                {
                    let state = self.core.state.lock().unwrap();
                    let entry = state.entries.get(&id).ok_or_else(|| IpcError::internal("Tunnel start was cancelled"))?;
                    if entry.view.state != TunnelState::Reconnecting || entry.view.failure.is_some() { return Ok::<_, IpcError>(entry.view.clone()); }
                }
                state_rx.changed().await.map_err(|_| IpcError::internal("Tunnel start was cancelled"))?;
            }
        }).await;
        match outcome {
            Ok(result) => result,
            Err(_) => {
                let mut state = self.core.state.lock().unwrap();
                let entry = state.entries.get_mut(&id).ok_or_else(|| IpcError::internal("Tunnel start was cancelled"))?;
                entry.transition(TunnelState::Reconnecting, Some(TunnelFailureCode::HostUnreachable), None, self.core.clock.now_unix_ms());
                Ok(entry.view.clone())
            }
        }
    }
    pub fn list(&self, saved: &HashMap<String, Vec<TunnelSpec>>) -> Result<Vec<TunnelView>, IpcError> {
        let state = self.core.state.lock().unwrap();
        let mut result: Vec<_> = state.entries.values().map(|e| e.view.clone()).collect();
        for (host, specs) in saved { for spec in specs {
            if !result.iter().any(|v| &v.host_id == host && v.spec_id == spec.id) { result.push(TunnelView::stopped(spec, host.clone())?); }
        } }
        result.sort_by(|a,b| (&a.host_id, &a.spec_id.0, &a.runtime_id.as_ref().map(|r| &r.0)).cmp(&(&b.host_id, &b.spec_id.0, &b.runtime_id.as_ref().map(|r| &r.0))));
        Ok(result)
    }
    pub fn subscribe_exit(&self, id: &TunnelRuntimeId) -> Option<watch::Receiver<Option<ExitStatus>>> {
        self.core.state.lock().unwrap().entries.get(id)?.child.current.as_ref().map(|g| g.exit.clone())
    }
    pub fn subscribe_state(&self, id: &TunnelRuntimeId) -> Option<watch::Receiver<TunnelState>> {
        self.core.state.lock().unwrap().entries.get(id).map(|e| e.state.subscribe())
    }
    pub fn transitions(&self, id: &TunnelRuntimeId) -> Vec<TunnelTransition> {
        self.core.state.lock().unwrap().entries.get(id).map(|e| e.log.iter().cloned().collect()).unwrap_or_default()
    }
    /// Synchronous phase for dispatch's revision barrier. Callers persist the
    /// new revision BEFORE awaiting these generations, and never hold a flock
    /// or this registry mutex across an await. Retained failures drain too.
    pub fn invalidate_host(&self, host_id: &str) -> Vec<Generation> {
        let mut state = self.core.state.lock().unwrap();
        let mut generations = Vec::new();
        for entry in state.entries.values_mut().filter(|e| e.view.host_id == host_id) {
            entry.child.cancel.send_replace(true);
            if let Some(g) = &entry.child.current { if g.exit.borrow().is_none() { g.request_kill(); generations.push(g.clone()); } }
            entry.transition(TunnelState::Failed, Some(TunnelFailureCode::HostChanged), None, self.core.clock.now_unix_ms());
        }
        state.draining.extend(generations.clone()); generations
    }
    pub async fn stop(&self, id: &TunnelRuntimeId) -> Result<(), IpcError> {
        let pending = {
            let mut state = self.core.state.lock().unwrap();
            if let Some(mut entry) = state.entries.remove(id) {
                entry.view.failure = None; entry.view.failure_piece = None;
                entry.child.cancel.send_replace(true);
                if let Some(g) = &entry.child.current { if g.exit.borrow().is_none() { g.request_kill(); state.draining.push(g.clone()); } }
            }
            state.draining.clone()
        };
        wait_generations(pending).await
    }
    pub async fn shutdown(&self) -> Result<(), IpcError> {
        let pending = {
            let mut state = self.core.state.lock().unwrap();
            state.closed = true;
            let entries = std::mem::take(&mut state.entries);
            for entry in entries.into_values() {
                entry.child.cancel.send_replace(true);
                if let Some(g) = &entry.child.current { if g.exit.borrow().is_none() { g.request_kill(); state.draining.push(g.clone()); } }
            }
            state.draining.clone()
        };
        // One total 6s deadline, NOT per entry. Backoff has no live child and
        // therefore no watch to await. Mutex is released before this point.
        wait_generations(pending).await
    }
}
impl Drop for TunnelRegistry {
    fn drop(&mut self) {
        let mut state = self.core.state.lock().unwrap(); state.closed = true;
        state.entries.clear();
        for g in &state.draining { if g.exit.borrow().is_none() { g.request_kill(); } }
    }
}
pub async fn wait_generations(mut pending: Vec<Generation>) -> Result<(), IpcError> {
    timeout(SHUTDOWN_DEADLINE, async { for g in &mut pending { g.wait().await?; } Ok(()) }).await.map_err(|_| io_error("child_reap_timeout"))?
}

/// Classify complete bounded diagnostics in contract order. Raw text never
/// enters a view or transition. At exit only terminal attempt failures persist;
/// a channel complaint after establishment cannot mask later transport loss.
pub fn classify_ssh_stderr(stderr: &str, transport_established: bool) -> Option<TunnelFailureCode> {
    use TunnelFailureCode::*;
    let s = stderr.to_ascii_lowercase();
    let has = |phrases: &[&str]| phrases.iter().any(|p| s.contains(p));
    if has(&["cannot listen to port", "could not request local forwarding"]) || s.lines().any(|l| l.contains("bind") && ["address already in use", "cannot assign requested address", "can't assign requested address", "permission denied"].iter().any(|p| l.contains(p))) { return Some(PortInUse); }
    if has(&["remote host identification has changed", "host key verification failed"]) { return Some(HostKeyRejected); }
    if has(&["permission denied (", "too many authentication failures"]) { return Some(AuthFailed); }
    if s.contains("administratively prohibited") && !transport_established { return Some(ChannelFailed); }
    if has(&["command-line line", "bad configuration option", "bad local forwarding specification", "no argument after keyword", "unable to negotiate with"]) { return Some(UnsupportedConfig); }
    if transport_established && has(&["open failed", "connect failed", "administratively prohibited"]) && !has(&["connection closed by", "connection reset by", "broken pipe", "not responding"]) { return Some(ChannelFailed); }
    if has(&["could not resolve hostname", "network is unreachable", "no route to host", "connection timed out", "operation timed out", "connection refused", "connection closed by", "connection reset by", "broken pipe", "not responding", "connect to host", "stdio forwarding failed"]) { return Some(HostUnreachable); }
    None
}
fn failure_piece(text: &str) -> String {
    if text.contains("Unable to negotiate with") { "algorithm_mismatch" } else { "bad_option" }.into()
}
fn update(core: &Weak<Core>, id: &TunnelRuntimeId, state: TunnelState, code: Option<TunnelFailureCode>, piece: Option<String>) {
    if let Some(core) = core.upgrade() {
        let now = core.clock.now_unix_ms();
        let mut registry = core.state.lock().unwrap();
        if let Some(entry) = registry.entries.get_mut(id) {
            if !*entry.child.cancel.borrow() { entry.transition(state, code, piece, now); }
        }
    }
}
fn register_child(core: &Arc<Core>, id: &TunnelRuntimeId, args: &[String], generation: u64) -> Result<Option<(Generation, mpsc::UnboundedReceiver<String>)>, IpcError> {
    // The closed check, synchronous spawn, and per-generation registration are
    // atomic under exactly the same lock as shutdown's detach.
    let mut state = core.state.lock().unwrap();
    if state.closed { return Ok(None); }
    let Some(entry) = state.entries.get_mut(id) else { return Ok(None); };
    if *entry.child.cancel.borrow() { return Ok(None); }
    let local_port = entry.view.local_port;
    // An intent record exists BEFORE spawn, so a crash in this window still names
    // the port to reclaim. It carries no pid: the sweep releases the claim and
    // never kills whoever holds the port.
    if let Some(records) = &core.records {
        if let Err(error) = records.record_intent(id, generation, local_port) {
            tracing::warn!("Cannot record tunnel spawn intent: {error}");
        }
    }
    let handle = tokio::runtime::Handle::try_current().map_err(|_| IpcError::internal("Tunnel reaper requires a runtime"))?;
    let SpawnedTunnel { mut child, mut stderr } = match core.spawner.spawn(args) {
        Ok(spawned) => spawned,
        Err(_) => {
            // No child exists, so the intent record has nothing to reclaim.
            if let Some(records) = &core.records {
                let _ = records.remove(id, generation);
            }
            return Err(io_error("tunnel_spawn_failed"));
        }
    };
    let pid = child.id();
    // The pid record lands before the reaper is created: the reaper is what makes
    // the entry live, so a crash before it leaves a killable record rather than
    // an orphan only the port reveals.
    if let Some(records) = &core.records {
        if let Err(error) = records.record_pid(id, generation, local_port, pid, pid as i32) {
            tracing::warn!("Cannot record tunnel child pid: {error}");
        }
    }
    let (kill, mut kill_rx) = mpsc::unbounded_channel();
    let (exit_tx, exit) = watch::channel(None);
    let (diagnostics, rx) = mpsc::unbounded_channel();
    let spawner = core.spawner.clone();
    let records = core.records.clone();
    let reaped_id = id.clone();
    // Reaper is the ONLY owner of Child and the ONLY issuer of group kills.
    // Dropping all senders (recv=None) is intentionally a kill request too.
    handle.spawn(async move {
        let result = tokio::select! {
            biased;
            result = child.wait() => result,
            _ = kill_rx.recv() => {
                spawner.kill_group(pid as i32, Signal::Term);
                match timeout(TERM_GRACE, child.wait()).await {
                    Ok(result) => result,
                    Err(_) => { spawner.kill_group(pid as i32, Signal::Kill); child.wait().await }
                }
            }
        };
        match result {
            Ok(status) => {
                exit_tx.send_replace(Some(status));
                // Removal happens ONLY after this reaper published its post-wait
                // status, and ONLY for the generation it owned: an old
                // generation's late removal must never delete a newer live record.
                if let Some(records) = records {
                    if let Err(error) = records.remove(&reaped_id, generation) {
                        tracing::warn!("Cannot retire a tunnel runtime record: {error}");
                    }
                }
            }
            Err(e) => { tracing::error!("Tunnel child wait failed: {e}"); }
        }
    });
    // Drain continuously, bounded per line even without a newline. No raw
    // diagnostics survive this task/attempt, and the pipe cannot fill and stall.
    handle.spawn(async move {
        let mut buf = [0u8; 4096]; let mut line = Vec::new();
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => { if !line.is_empty() { let _ = diagnostics.send(String::from_utf8_lossy(&line).into_owned()); } break; }
                Ok(n) => for byte in &buf[..n] {
                    if *byte == b'\n' || line.len() == 8192 {
                        let text = String::from_utf8_lossy(&line).into_owned(); line.clear();
                        if diagnostics.send(text).is_err() { return; }
                    } else { line.push(*byte); }
                },
                Err(e) => { tracing::warn!("Tunnel stderr read failed: {e}"); break; }
            }
        }
    });
    let g = Generation { generation, local_port: entry.view.local_port, pid, kill, exit };
    // A replaced sender is dropped only AFTER the previous watch has fired.
    entry.child.current = Some(g.clone()); entry.view.attempt = entry.view.attempt.saturating_add(1);
    Ok(Some((g, rx)))
}
async fn readiness(port: u16, factory: Arc<ProbeSocketFactory>) -> Result<(), usize> {
    let addr = (Ipv4Addr::LOCALHOST, port).into();
    let deadline = Instant::now() + START_DEADLINE;
    let mut attempts = 0;
    loop {
        if Instant::now() >= deadline { return Err(attempts); }
        attempts += 1;
        match tokio::time::timeout_at(deadline.min(Instant::now() + PROBE_DEADLINE), factory.connect(addr, true)).await {
            Ok(Ok((mut socket, _overridden))) => {
                if self_connected(&socket).unwrap_or(true) {
                    // Simultaneous open owns P itself: close IMMEDIATELY, never
                    // hold this socket through the 2s channel-attribution window.
                    drop(socket);
                    tokio::task::yield_now().await;
                    continue;
                }
                // Exactly one destination channel. Retire it before publishing
                // listening. Filtered destinations can report late (accepted
                // residual); EOF/deadline can lose one diagnostic. An operator
                // connecting during this <=2s window shares its attribution.
                let mut byte = [0]; let _ = tokio::time::timeout_at(deadline.min(Instant::now() + CHANNEL_DEADLINE), socket.read(&mut byte)).await;
                drop(socket); return Ok(());
            }
            _ => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
}
async fn run_entry(weak: Weak<Core>, id: TunnelRuntimeId, host: SshHost, mut cancel: watch::Receiver<bool>) {
    let mut backoff = 1; let mut reallocated = false; let mut first = true;
    loop {
        if *cancel.borrow() { return; }
        let Some(core) = weak.upgrade() else { return; };
        let (spec, port) = {
            let state = core.state.lock().unwrap(); let Some(entry) = state.entries.get(&id) else { return; };
            (entry.spec.clone(), entry.view.local_port)
        };
        if first {
            // Availability probe only before FIRST spawn, never on the
            // host_unreachable retry path. Allocation is a separate operation.
            let availability = tokio::select! { result = probe_local_port_available((Ipv4Addr::LOCALHOST, port).into(), &core.probes) => result, _ = cancel.changed() => return };
            if availability == PortAvailability::Occupied { update(&weak, &id, TunnelState::Failed, Some(TunnelFailureCode::PortInUse), None); return; }
        }
        let config = match ensure_empty_config(&core.runtime_dir) {
            Ok(config) => config,
            Err(e) => {
                // I/O creation errors are returned by start's preflight below;
                // unsafe override paths are typed failed views, never wire Err.
                let piece = e.details.as_ref().and_then(|d| d["piece"].as_str()).unwrap_or("config_path").to_string();
                update(&weak, &id, TunnelState::Failed, Some(TunnelFailureCode::UnsupportedConfig), Some(piece)); return;
            }
        };
        if password::require(&host).is_err() { update(&weak, &id, TunnelState::Failed, Some(TunnelFailureCode::CredentialUnavailable), None); return; }
        let args = match argv::tunnel_argv(&host, &spec, port, &config, &core.config.source()) {
            Ok(args) => args,
            Err(e) => {
                let piece = e.details.as_ref().and_then(|d| d["piece"].as_str()).unwrap_or("unsupported_directive").to_string();
                update(&weak, &id, TunnelState::Failed, Some(TunnelFailureCode::UnsupportedConfig), Some(piece)); return;
            }
        };
        let generation = core.generation.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        let (mut child, mut stderr) = match register_child(&core, &id, &args, generation) {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(_) => { update(&weak, &id, TunnelState::Failed, Some(TunnelFailureCode::HostUnreachable), None); return; }
        };
        let factory = core.probes.clone(); drop(core);
        let ready = readiness(port, factory); tokio::pin!(ready);
        let mut established = false; let mut diagnostic = String::new(); let mut stderr_open = true;
        loop {
            tokio::select! {
                biased;
                _ = cancel.changed() => return,
                result = child.exit.changed() => {
                    if result.is_err() { return; }
                    if child.exit.borrow().is_some() {
                        // The drain task may still have buffered lines at exit.
                        while let Some(line) = stderr.recv().await { if diagnostic.len() < 32768 { diagnostic.push_str(&line); diagnostic.push('\n'); } }
                        break;
                    }
                }
                line = stderr.recv(), if stderr_open => {
                    match line {
                        Some(line) => {
                            let code = classify_ssh_stderr(&line, established);
                            if established && code == Some(TunnelFailureCode::ChannelFailed) { update(&weak, &id, TunnelState::Listening, code, None); }
                            if diagnostic.len() < 32768 { diagnostic.push_str(&line); diagnostic.push('\n'); }
                        }
                        None => stderr_open = false,
                    }
                }
                result = &mut ready, if !established => {
                    if let Err(attempts) = result {
                        // Keep the bounded diagnostic in this attempt only: no
                        // raw stderr is copied into a view, ring, or disk log.
                        let failure = format!("Readiness deadline for 127.0.0.1:{port} after {attempts} attempts; captured stderr: {diagnostic}");
                        #[cfg(test)]
                        eprintln!("{failure}");
                        tracing::warn!(port, attempts, stderr_code = ?classify_ssh_stderr(&diagnostic, false), "Tunnel readiness deadline expired");
                        drop(failure);
                        child.request_kill();
                        if let Some(core) = weak.upgrade() { core.state.lock().unwrap().draining.push(child.clone()); }
                        update(&weak, &id, TunnelState::Reconnecting, Some(TunnelFailureCode::HostUnreachable), None);
                        tokio::select! { _ = cancel.changed() => return, result = timeout(SHUTDOWN_DEADLINE, child.wait()) => { if !matches!(result, Ok(Ok(()))) { return; } } }
                        break;
                    }
                    // Check the post-wait watch before publishing. Readiness is
                    // not a latch: a stolen-port bind exit retracts listening.
                    if child.exit.borrow().is_none() {
                        established = true; backoff = 1;
                        diagnostic.clear();
                        update(&weak, &id, TunnelState::Listening, None, None);
                    }
                }
            }
        }
        let mut code = classify_ssh_stderr(&diagnostic, established).unwrap_or(TunnelFailureCode::HostUnreachable);
        if established && code == TunnelFailureCode::ChannelFailed { code = TunnelFailureCode::HostUnreachable; }
        let reconnecting = code == TunnelFailureCode::HostUnreachable || (code == TunnelFailureCode::PortInUse && !first && !reallocated);
        if !reconnecting {
            let piece = (code == TunnelFailureCode::UnsupportedConfig).then(|| failure_piece(&diagnostic));
            update(&weak, &id, TunnelState::Failed, Some(code), piece); return;
        }
        if code == TunnelFailureCode::PortInUse {
            reallocated = true;
            if let Some(core) = weak.upgrade() {
                let mut state = core.state.lock().unwrap();
                let new_port = match allocate_port() { Ok(p) => p, Err(_) => { drop(state); update(&weak, &id, TunnelState::Failed, Some(code), None); return; } };
                if state.entries.values().any(|e| e.view.local_port == new_port && e.view.state != TunnelState::Failed) || state.draining.iter().any(|g| g.local_port == new_port && g.exit.borrow().is_none()) { drop(state); update(&weak, &id, TunnelState::Failed, Some(code), None); return; }
                if let Some(entry) = state.entries.get_mut(&id) { entry.view.local_port = new_port; }
            }
        }
        first = false;
        update(&weak, &id, TunnelState::Reconnecting, Some(code), None);
        tokio::select! { _ = cancel.changed() => return, _ = tokio::time::sleep(Duration::from_secs(backoff)) => {} }
        backoff = (backoff * 2).min(4);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::ssh::{SshAuthMethod, SshHostSource};
    use tokio::io::AsyncWriteExt;
    use std::sync::atomic::AtomicBool;
    use std::os::unix::process::ExitStatusExt;

    // Allocation hints are not reservations. Keep every port-using case in
    // this module isolated for its full lifetime, including shutdown/reaping.
    static PORT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn host() -> SshHost {
        SshHost { id: uuid::Uuid::new_v4().to_string(), label: "fixture".into(), hostname: "127.0.0.1".into(), username: None, port: Some(allocate_port().unwrap()), identity_file: None, jump_host: None, source: SshHostSource::Manual, auth_method: SshAuthMethod::Key, disabled: None, shell: None }
    }
    fn spec(port: Option<u16>) -> TunnelSpec {
        TunnelSpec { id: TunnelSpecId("saved".into()), label: "database".into(), local_port: port, remote_host: "127.0.0.1".into(), remote_port: allocate_port().unwrap(), enabled: true }
    }
    fn make_registry(spawner: Arc<dyn TunnelSpawner>) -> (TunnelRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = TunnelRegistry::new(spawner, ConfigSource::Fixture { user: "", system: "" }, Arc::new(ProbeSocketFactory::default()), Arc::new(SystemClock));
        Arc::get_mut(&mut registry.core).unwrap().runtime_dir = dir.path().to_path_buf();
        (registry, dir)
    }
    async fn exit(mut rx: watch::Receiver<Option<ExitStatus>>) {
        timeout(SHUTDOWN_DEADLINE, async { while rx.borrow().is_none() { rx.changed().await.unwrap(); } }).await.expect("post-wait exit signal");
        assert!(rx.borrow().is_some());
    }
    async fn state(registry: &TunnelRegistry, id: &TunnelRuntimeId, expected: TunnelState) {
        let mut rx = registry.subscribe_state(id).unwrap();
        timeout(Duration::from_secs(90), async {
            while *rx.borrow() != expected { rx.changed().await.unwrap(); }
        }).await.expect("state event");
    }
    // An injected child holds a real listener, retires accepted probe channels,
    // and emits scripted attempt errors. Only timing is fake; argv and stderr
    // classification still traverse the production builder/classifier.
    struct FakeChild {
        pid: u32,
        result: watch::Receiver<Option<i32>>,
    }
    impl TunnelChild for FakeChild {
        fn id(&self) -> u32 { self.pid }
        fn wait(&mut self) -> Wait<'_> {
            Box::pin(async move {
                loop {
                    if let Some(code) = *self.result.borrow() { return Ok(ExitStatus::from_raw(code)); }
                    self.result.changed().await.map_err(|_| io::Error::other("fake exit channel closed"))?;
                }
            })
        }
    }
    #[derive(Clone, Copy)]
    enum Attempt { Listen, Fail(&'static str), Hang, WaitForProbe, ProbeFailure }
    struct Scripted {
        script: Mutex<VecDeque<Attempt>>,
        children: Mutex<HashMap<u32, watch::Sender<Option<i32>>>>,
        calls: AtomicUsize,
        kills: AtomicUsize,
        attempts: mpsc::UnboundedSender<Instant>,
        probe_factory: Mutex<Option<Arc<ProbeSocketFactory>>>,
        live_error: Mutex<HashMap<u32, mpsc::UnboundedSender<&'static str>>>,
    }
    impl Scripted {
        fn new(script: Vec<Attempt>) -> (Arc<Self>, mpsc::UnboundedReceiver<Instant>) {
            let (tx, rx) = mpsc::unbounded_channel();
            (Arc::new(Self { script: Mutex::new(script.into()), children: Mutex::new(HashMap::new()), calls: AtomicUsize::new(0), kills: AtomicUsize::new(0), attempts: tx, probe_factory: Mutex::new(None), live_error: Mutex::new(HashMap::new()) }), rx)
        }
        fn fail_live(&self, text: &'static str) {
            let children = self.live_error.lock().unwrap();
            children.iter().max_by_key(|(pid, _)| *pid).unwrap().1.send(text).unwrap();
        }
    }
    impl TunnelSpawner for Scripted {
        fn spawn(&self, args: &[String]) -> io::Result<SpawnedTunnel> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            let pid = 100000 + call as u32;
            let _ = self.attempts.send(Instant::now());
            let port: u16 = args.iter().find_map(|a| a.strip_prefix("-L127.0.0.1:")).unwrap().split(':').next().unwrap().parse().unwrap();
            let action = self.script.lock().unwrap().pop_front().unwrap_or(Attempt::Listen);
            let (tx, rx) = watch::channel(None); self.children.lock().unwrap().insert(pid, tx.clone());
            let (read, mut write) = tokio::io::duplex(65536);
            let (errors, mut error_rx) = mpsc::unbounded_channel(); self.live_error.lock().unwrap().insert(pid, errors);
            let (done, finished) = watch::channel(None);
            let mut killed = rx.clone();
            let factory = self.probe_factory.lock().unwrap().clone();
            tokio::spawn(async move {
                match action {
                    Attempt::Fail(text) => { write.write_all(text.as_bytes()).await.unwrap(); drop(write); done.send_replace(Some(255 << 8)); }
                    Attempt::Hang => { killed.changed().await.unwrap(); drop(write); done.send_replace(Some(15)); }
                    Attempt::Listen | Attempt::WaitForProbe | Attempt::ProbeFailure => {
                        // Real socket contention is independent of paused Tokio
                        // time. Bound retries by wall time and yield between
                        // attempts rather than relying on a fixed sleep.
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        let listener = loop {
                            match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
                                Ok(listener) => break listener,
                                Err(error) => {
                                    if *killed.borrow() != None {
                                        drop(write); done.send_replace(Some(15)); return;
                                    }
                                    if error.kind() != io::ErrorKind::AddrInUse || std::time::Instant::now() >= deadline {
                                        let diagnostic = format!("bind [127.0.0.1]:{port}: {error}\nScripted tunnel listener could not bind within 5 seconds; cannot listen to port: {port}\n");
                                        if let Err(error) = write.write_all(diagnostic.as_bytes()).await {
                                            eprintln!("Scripted tunnel bind diagnostic write failed: {error}; {diagnostic}");
                                        }
                                        drop(write); done.send_replace(Some(255 << 8)); return;
                                    }
                                    tokio::task::yield_now().await;
                                }
                            }
                        };
                        if matches!(action, Attempt::WaitForProbe) {
                            let factory = factory.unwrap();
                            tokio::select! {
                                _ = factory.readiness_override_consumed.notified() => {},
                                _ = killed.changed() => { drop(listener); drop(write); done.send_replace(Some(15)); return; }
                            }
                        }
                        loop {
                            tokio::select! {
                                _ = killed.changed() => break,
                                Some(text) = error_rx.recv() => {
                                    write.write_all(text.as_bytes()).await.unwrap();
                                    if !text.starts_with("channel ") { drop(listener); drop(write); done.send_replace(Some(255 << 8)); return; }
                                }
                                accepted = listener.accept() => {
                                    let (socket, _) = accepted.unwrap();
                                    if matches!(action, Attempt::ProbeFailure) { write.write_all(b"channel 1: open failed: connect failed: Connection refused\n").await.unwrap(); }
                                    drop(socket);
                                }
                            }
                        }
                        drop(listener); drop(write); done.send_replace(Some(15));
                    }
                }
            });
            Ok(SpawnedTunnel { child: Box::new(FakeChild { pid, result: finished }), stderr: Box::pin(read) })
        }
        fn kill_group(&self, pid: i32, _: Signal) {
            self.kills.fetch_add(1, Ordering::SeqCst);
            self.children.lock().unwrap()[&(pid as u32)].send_replace(Some(15));
        }
    }
    /// Real kernel child + counted signal seam. The Python fixture is a port
    /// binding process, not a timing mock. Its ready marker precedes its listener
    /// event; all tests await state/watch signals rather than sleep.
    struct CountingProcess {
        kills: AtomicUsize,
        calls: AtomicUsize,
        ignore_term: AtomicBool,
        flood: bool,
        last_pid: AtomicUsize,
    }
    impl CountingProcess { fn new(ignore: bool, flood: bool) -> Arc<Self> { Arc::new(Self { kills: AtomicUsize::new(0), calls: AtomicUsize::new(0), ignore_term: AtomicBool::new(ignore), flood, last_pid: AtomicUsize::new(0) }) } }
    impl TunnelSpawner for CountingProcess {
        fn spawn(&self, args: &[String]) -> io::Result<SpawnedTunnel> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let port = args.iter().find_map(|a| a.strip_prefix("-L127.0.0.1:")).unwrap().split(':').next().unwrap();
            let program = format!("import socket,signal,sys\n{}\ns=socket.socket(); s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1); s.bind(('127.0.0.1',{})); s.listen()\n{}\nwhile True:\n c,a=s.accept(); c.close()\n", if self.ignore_term.load(Ordering::SeqCst) { "signal.signal(signal.SIGTERM,signal.SIG_IGN)" } else { "pass" }, port, if self.flood { "sys.stderr.write('x'*262144+'\\n'); sys.stderr.flush()" } else { "pass" });
            let mut command = tokio::process::Command::new("python3");
            command.args(["-c", &program]).process_group(0).stderr(Stdio::piped()).stdout(Stdio::null()).stdin(Stdio::null());
            let mut child = command.spawn()?; self.last_pid.store(child.id().unwrap() as usize, Ordering::SeqCst);
            let stderr = child.stderr.take().unwrap();
            Ok(SpawnedTunnel { child: Box::new(child), stderr: Box::pin(stderr) })
        }
        fn kill_group(&self, pgid: i32, sig: Signal) {
            self.kills.fetch_add(1, Ordering::SeqCst); ProcessSpawner.kill_group(pgid, sig);
        }
    }

    #[tokio::test]
    async fn start_stop_releases_listener() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        let spawner = CountingProcess::new(false, false);
        let (registry, _dir) = make_registry(spawner.clone()); let host = host();
        let port = allocate_port().unwrap(); let definition = spec(Some(port));
        let started = registry.start(&host, definition.clone(), true).await.unwrap();
        assert_eq!(started.state, TunnelState::Listening);
        let twice = registry.start(&host, definition.clone(), true).await.unwrap();
        assert_eq!(twice.runtime_id, started.runtime_id); assert_eq!(spawner.calls.load(Ordering::SeqCst), 1);
        let id = started.runtime_id.unwrap(); let exited = registry.subscribe_exit(&id).unwrap();
        assert_eq!(registry.list(&HashMap::from([(host.id.clone(), vec![definition])])).unwrap().len(), 1);
        registry.stop(&id).await.unwrap(); assert!(exited.borrow().is_some()); exit(exited).await;
        assert!(std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());
        assert_eq!(spawner.kills.load(Ordering::SeqCst), 1);
        let adhoc = registry.start(&host, spec(None), false).await.unwrap();
        assert_ne!(adhoc.spec_id, TunnelSpecId("saved".into()));
        let empty_id = registry.start(&host, TunnelSpec { id: TunnelSpecId(String::new()), ..spec(None) }, false).await.unwrap();
        assert_ne!(adhoc.spec_id, empty_id.spec_id); registry.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn drop_reaps_child() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        for ignore in [false, true] {
            let spawner = CountingProcess::new(ignore, false);
            let (registry, _dir) = make_registry(spawner.clone());
            let view = registry.start(&host(), spec(None), false).await.unwrap(); assert_eq!(view.state, TunnelState::Listening);
            let id = view.runtime_id.unwrap(); let exited = registry.subscribe_exit(&id).unwrap();
            drop(registry); exit(exited).await;
            assert!(std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, view.local_port)).is_ok());
            assert_eq!(spawner.kills.load(Ordering::SeqCst), if ignore { 2 } else { 1 });
        }
        let (fake, _) = Scripted::new(vec![Attempt::Fail("Permission denied (publickey)")]);
        let (registry, _dir) = make_registry(fake.clone());
        let view = registry.start(&host(), spec(None), false).await.unwrap();
        exit(registry.subscribe_exit(&view.runtime_id.unwrap()).unwrap()).await;
        drop(registry); assert_eq!(fake.kills.load(Ordering::SeqCst), 0, "no recycled-pid kill after reap");

        let spawner = CountingProcess::new(true, false);
        let (registry, _dir) = make_registry(spawner); let registry = Arc::new(registry);
        let view = registry.start(&host(), spec(None), false).await.unwrap(); let id = view.runtime_id.unwrap();
        let exited = registry.subscribe_exit(&id).unwrap();
        let (stop, shutdown) = tokio::join!(registry.stop(&id), registry.shutdown());
        stop.unwrap(); shutdown.unwrap(); assert!(exited.borrow().is_some(), "shutdown awaits draining generation");

        // Reserve the next fixture's tunnel port BEFORE the self-connect case,
        // so the allocator cannot recycle that case's socket/TIME_WAIT port.
        let reservation = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let readiness_port = reservation.local_addr().unwrap().port();
        let port = allocate_port().unwrap();
        let addr: SocketAddr = (Ipv4Addr::LOCALHOST, port).into();
        assert!(is_self_connect(addr, addr)); assert!(!is_self_connect(addr, (Ipv4Addr::LOCALHOST, readiness_port).into()));
        let factory = ProbeSocketFactory::default(); *factory.availability_source_port_override.lock().unwrap() = Some(port);
        assert_eq!(probe_local_port_available(addr, &factory).await, PortAvailability::SelfConnected);
        let port = readiness_port;
        let addr: SocketAddr = (Ipv4Addr::LOCALHOST, port).into();
        drop(reservation);
        let (fake, _) = Scripted::new(vec![Attempt::WaitForProbe]);
        let (mut registry, _dir) = make_registry(fake.clone());
        let factory = Arc::new(ProbeSocketFactory::default());
        // Distinct endpoints: exercise one-shot source override consumption,
        // not simultaneous open on the listener's own address.
        *factory.readiness_source_port_override.lock().unwrap() = Some(allocate_port().unwrap());
        *fake.probe_factory.lock().unwrap() = Some(factory.clone());
        Arc::get_mut(&mut registry.core).unwrap().probes = factory;
        assert_eq!(probe_local_port_available(addr, &registry.core.probes).await, PortAvailability::Available);
        let view = registry.start(&host(), spec(Some(port)), false).await.unwrap();
        let id = view.runtime_id.as_ref().unwrap();
        state(&registry, id, TunnelState::Listening).await;
        let current = registry.list(&HashMap::new()).unwrap().remove(0);
        assert_eq!(current.state, TunnelState::Listening); assert_eq!(current.failure, None);
        assert!(registry.core.probes.readiness_source_port_override.lock().unwrap().is_none());
        assert_eq!(registry.core.probes.readiness_override_consumptions.load(Ordering::SeqCst), 1);
        let exited = registry.subscribe_exit(id).unwrap();
        registry.shutdown().await.unwrap();
        assert!(exited.borrow().is_some());
        exit(exited).await;
        assert!(std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).is_ok());

        let (fake, _) = Scripted::new(vec![Attempt::ProbeFailure]); let (registry, _dir) = make_registry(fake.clone());
        let view = registry.start(&host(), spec(None), false).await.unwrap(); let id = view.runtime_id.unwrap();
        assert_eq!(view.state, TunnelState::Listening); assert_eq!(view.failure, None);
        assert!(registry.transitions(&id).iter().all(|t| t.code != Some(TunnelFailureCode::ChannelFailed)), "readiness diagnostic is not a traffic transition");
        let mut changed = registry.subscribe_state(&id).unwrap(); changed.borrow_and_update();
        fake.fail_live("channel 2: open failed: connect failed: Connection refused\n");
        timeout(Duration::from_secs(2), changed.changed()).await.unwrap().unwrap();
        let views = registry.list(&HashMap::new()).unwrap(); assert_eq!(views[0].state, TunnelState::Listening); assert_eq!(views[0].failure, Some(TunnelFailureCode::ChannelFailed));
        assert_eq!(registry.transitions(&id).iter().filter(|t| t.code == Some(TunnelFailureCode::ChannelFailed)).count(), 1);
        registry.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_start_same_port_is_arbitrated() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        let process = CountingProcess::new(false, false); let (registry, _dir) = make_registry(process.clone()); let registry = Arc::new(registry);
        let port = allocate_port().unwrap(); let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let mut tasks = Vec::new();
        for _ in 0..2 { let registry = registry.clone(); let barrier = barrier.clone(); tasks.push(tokio::spawn(async move { barrier.wait().await; registry.start(&host(), spec(Some(port)), false).await.unwrap() })); }
        barrier.wait().await;
        let a = tasks.remove(0).await.unwrap(); let b = tasks.remove(0).await.unwrap();
        assert_eq!([a.state, b.state].iter().filter(|s| **s == TunnelState::Listening).count(), 1);
        assert_eq!([a.failure, b.failure].iter().filter(|f| **f == Some(TunnelFailureCode::PortInUse)).count(), 1);
        assert_eq!(process.calls.load(Ordering::SeqCst), 1); registry.shutdown().await.unwrap();

        let process = CountingProcess::new(true, false); let (registry, _dir) = make_registry(process.clone());
        let port = allocate_port().unwrap();
        let host = host(); let first = registry.start(&host, spec(Some(port)), false).await.unwrap();
        let exited = registry.subscribe_exit(first.runtime_id.as_ref().unwrap()).unwrap();
        let pending = registry.invalidate_host(&host.id);
        process.ignore_term.store(false, Ordering::SeqCst);
        let next = registry.start(&host, spec(Some(port)), false).await.unwrap();
        assert!(exited.borrow().is_some()); assert_eq!(next.state, TunnelState::Listening); assert_eq!(next.failure, None);
        wait_generations(pending).await.unwrap(); registry.shutdown().await.unwrap();
        let factory = ProbeSocketFactory::default();
        let port = allocate_port().unwrap(); *factory.availability_source_port_override.lock().unwrap() = Some(port);
        assert_eq!(probe_local_port_available((Ipv4Addr::LOCALHOST, port).into(), &factory).await, PortAvailability::SelfConnected);
    }

    #[tokio::test]
    async fn saved_definition_without_runtime_lists_as_stopped() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        let (registry, _dir) = make_registry(Arc::new(ProcessSpawner)); let definition = spec(Some(allocate_port().unwrap()));
        let views = registry.list(&HashMap::from([("host".into(), vec![definition])])).unwrap();
        assert_eq!(views.len(), 1); assert_eq!(views[0].state, TunnelState::Stopped); assert_eq!(views[0].runtime_id, None); assert_eq!(views[0].since, None);
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (fake, _) = Scripted::new(vec![]); let (registry, _dir) = make_registry(fake.clone());
        let view = registry.start(&host(), spec(Some(occupied.local_addr().unwrap().port())), false).await.unwrap();
        assert_eq!(view.failure, Some(TunnelFailureCode::PortInUse)); assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        assert!(registry.transitions(view.runtime_id.as_ref().unwrap()).iter().all(|t| t.to != TunnelState::Listening));
    }

    #[tokio::test(start_paused = true)]
    async fn reconnect_is_time_bounded() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        let (fake, mut attempts) = Scripted::new((0..17).map(|_| Attempt::Fail("Connection refused")).chain([Attempt::Listen]).collect());
        let (registry, _dir) = make_registry(fake.clone());
        let initial = registry.start(&host(), spec(None), false).await.unwrap();
        assert_eq!(initial.state, TunnelState::Reconnecting); assert_eq!(initial.failure, Some(TunnelFailureCode::HostUnreachable)); assert_eq!(initial.failure_piece, None);
        assert_eq!(registry.list(&HashMap::new()).unwrap()[0].failure, initial.failure);
        let start = attempts.recv().await.unwrap(); let id = initial.runtime_id.unwrap();
        state(&registry, &id, TunnelState::Listening).await;
        let mut times = vec![start]; while let Ok(time) = attempts.try_recv() { times.push(time); }
        assert_eq!(times.len(), 18); assert_eq!(times[1] - times[0], Duration::from_secs(1)); assert_eq!(times[2] - times[1], Duration::from_secs(2));
        for pair in times[2..].windows(2) { assert_eq!(pair[1] - pair[0], Duration::from_secs(4)); }
        assert!(times.last().unwrap().duration_since(start) >= Duration::from_secs(60));
        let recovered = registry.list(&HashMap::new()).unwrap(); assert_eq!(recovered[0].local_port, initial.local_port); assert_eq!(recovered[0].failure, None);
        fake.script.lock().unwrap().push_back(Attempt::Fail("Permission denied (publickey)"));
        fake.fail_live("Connection closed by fixture"); state(&registry, &id, TunnelState::Failed).await;
        assert!(registry.transitions(&id).iter().any(|t| t.to == TunnelState::Reconnecting && t.code == Some(TunnelFailureCode::HostUnreachable)));
        assert_eq!(registry.list(&HashMap::new()).unwrap()[0].failure, Some(TunnelFailureCode::AuthFailed)); registry.shutdown().await.unwrap();
        for text in ["Permission denied (password)", "Host key verification failed"] {
            let (fake, _) = Scripted::new(vec![Attempt::Fail(text)]); let (registry, _dir) = make_registry(fake.clone());
            let failed = registry.start(&host(), spec(None), false).await.unwrap(); assert_eq!(failed.state, TunnelState::Failed); assert_eq!(fake.calls.load(Ordering::SeqCst), 1); registry.shutdown().await.unwrap();
        }
        let (fake, _) = Scripted::new(vec![Attempt::Fail("Connection refused")]); let (registry, _dir) = make_registry(fake.clone());
        let view = registry.start(&host(), spec(None), false).await.unwrap(); registry.stop(view.runtime_id.as_ref().unwrap()).await.unwrap();
        tokio::time::advance(Duration::from_secs(60)).await; assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        let (fake, _) = Scripted::new(vec![Attempt::Hang]); let (registry, _dir) = make_registry(fake);
        let deadline = Instant::now(); let view = registry.start(&host(), spec(None), false).await.unwrap();
        assert!(deadline.elapsed() <= START_DEADLINE); assert_eq!(view.state, TunnelState::Reconnecting); assert_eq!(view.failure, Some(TunnelFailureCode::HostUnreachable)); assert_eq!(view.failure_piece, None);
        assert_eq!(registry.list(&HashMap::new()).unwrap()[0].failure, view.failure); registry.shutdown().await.unwrap();

        let (fake, _) = Scripted::new(vec![Attempt::Listen]); let (registry, _dir) = make_registry(fake.clone());
        let mut imported = host(); imported.source = SshHostSource::Config;
        let definition = spec(Some(allocate_port().unwrap()));
        let failed = registry.start(&imported, definition.clone(), true).await.unwrap();
        for view in [failed.clone(), registry.list(&HashMap::new()).unwrap().remove(0)] {
            assert_eq!(view.state, TunnelState::Failed); assert_eq!(view.failure, Some(TunnelFailureCode::UnsupportedConfig)); assert_eq!(view.failure_piece.as_deref(), Some("config_alias"));
        }
        assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
        // Same saved definition, now authoritative endpoint rather than stale
        // imported alias: re-arm in place and clear both failure fields.
        imported.source = SshHostSource::Manual;
        let restored = registry.start(&imported, definition, true).await.unwrap();
        assert_eq!(restored.runtime_id, failed.runtime_id); assert_eq!(restored.state, TunnelState::Listening);
        assert_eq!(restored.failure, None); assert_eq!(restored.failure_piece, None);
        registry.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn transition_ring_keeps_newest_100_and_no_secret() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        let (fake, _) = Scripted::new(vec![Attempt::Fail("s3cr3t\nSTDERR_SENTINEL\nPermission denied (publickey)")]);
        let (registry, _dir) = make_registry(fake); let view = registry.start(&host(), spec(None), false).await.unwrap(); let id = view.runtime_id.unwrap();
        assert_eq!(view.failure, Some(TunnelFailureCode::AuthFailed));
        let mut expected = VecDeque::new();
        {
            let mut state = registry.core.state.lock().unwrap(); let entry = state.entries.get_mut(&id).unwrap(); entry.log.clear();
            for n in 0..101 { let to = if n % 2 == 0 { TunnelState::Reconnecting } else { TunnelState::Listening }; entry.transition(to, None, None, registry.core.clock.now_unix_ms()); expected.push_back(to); }
        }
        expected.pop_front(); let log = registry.transitions(&id);
        assert_eq!(log.len(), 100); assert_eq!(log.iter().map(|t| t.to).collect::<Vec<_>>(), expected.into_iter().collect::<Vec<_>>());
        let json = serde_json::to_string(&log).unwrap(); assert!(!json.contains("s3cr3t")); assert!(!json.contains("STDERR_SENTINEL")); registry.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn empty_config_reuse_and_stderr_drain() {
        let _port_guard = PORT_TEST_LOCK.lock().await;
        use std::os::unix::fs::MetadataExt;
        let process = CountingProcess::new(false, true); let (registry, dir) = make_registry(process);
        let a = registry.start(&host(), spec(None), false).await.unwrap(); assert_eq!(a.state, TunnelState::Listening);
        let path = dir.path().join("ssh-tunnel-empty.conf"); let first = std::fs::metadata(&path).unwrap();
        let b = registry.start(&host(), spec(None), false).await.unwrap(); assert_eq!(b.state, TunnelState::Listening);
        let second = std::fs::metadata(&path).unwrap(); assert_eq!(first.ino(), second.ino()); assert_eq!(second.len(), 0); assert_eq!(second.mode() & 0o777, 0o600);
        assert_eq!(ensure_empty_config(dir.path()).unwrap(), path); registry.shutdown().await.unwrap();
        let file = dir.path().join("not-a-directory"); std::fs::write(&file, b"x").unwrap();
        let error = ensure_empty_config(&file).unwrap_err(); assert_eq!(error.code, IpcErrorCode::IoError); assert_eq!(error.details.unwrap()["reason"], "empty_config_unwritable");
        let error = ensure_empty_config(&dir.path().join("space path")).unwrap_err(); assert_eq!(error.details.unwrap()["piece"], "config_path");
    }

    #[test]
    fn classify_ssh_stderr_table() {
        use TunnelFailureCode::*;
        let rows = [
            ("bind [127.0.0.1]:5432: Address already in use\nchannel_setup_fwd_listener_tcpip: cannot listen to port: 5432", false, Some(PortInUse)),
            ("cannot listen to port: 80", false, Some(PortInUse)), ("Could not request local forwarding", false, Some(PortInUse)),
            ("bind [127.0.0.1]:80: Permission denied", false, Some(PortInUse)),
            ("bind [127.0.0.1]:5432: Cannot assign requested address", false, Some(PortInUse)),
            ("bind [127.0.0.1]:5432: Can't assign requested address", false, Some(PortInUse)),
            ("REMOTE HOST IDENTIFICATION HAS CHANGED", false, Some(HostKeyRejected)), ("Host key verification failed", false, Some(HostKeyRejected)),
            ("Permission denied (publickey)", false, Some(AuthFailed)), ("Permission denied (password)", false, Some(AuthFailed)),
            ("Permission denied (keyboard-interactive)", false, Some(AuthFailed)), ("Permission denied (password,keyboard-interactive)", false, Some(AuthFailed)),
            ("Too many authentication failures", false, Some(AuthFailed)),
            ("administratively prohibited\nstdio forwarding failed", false, Some(ChannelFailed)),
            ("open failed: connect failed: Connection refused\nstdio forwarding failed", false, Some(HostUnreachable)),
            ("open failed: connect failed: Connection refused", true, Some(ChannelFailed)),
            ("administratively prohibited\nConnection closed by host", true, Some(HostUnreachable)),
            ("administratively prohibited\nConnection closed by host", false, Some(ChannelFailed)),
            ("command-line line 0: Bad configuration option: Test\nterminating, 1 bad configuration options", false, Some(UnsupportedConfig)),
            ("Bad configuration option", false, Some(UnsupportedConfig)), ("Bad local forwarding specification", false, Some(UnsupportedConfig)),
            ("no argument after keyword", false, Some(UnsupportedConfig)), ("Unable to negotiate with host", false, Some(UnsupportedConfig)),
            ("Could not resolve hostname", false, Some(HostUnreachable)), ("Network is unreachable", false, Some(HostUnreachable)),
            ("No route to host", false, Some(HostUnreachable)), ("Connection timed out", false, Some(HostUnreachable)), ("Operation timed out", false, Some(HostUnreachable)),
            ("Connection refused", false, Some(HostUnreachable)), ("Connection closed by host", true, Some(HostUnreachable)),
            ("Connection reset by host port 22", true, Some(HostUnreachable)), ("client_loop: send disconnect: Broken pipe", true, Some(HostUnreachable)),
            ("Timeout, server host not responding", true, Some(HostUnreachable)),
            ("connect to host host port 22: Cannot assign requested address", false, Some(HostUnreachable)),
            ("connect to host host port 22: Can't assign requested address", false, Some(HostUnreachable)),
            ("cannot listen to port\nPermission denied (password)\nHost key verification failed", false, Some(PortInUse)),
            ("unmatched diagnostic", false, None),
        ];
        for (text, established, code) in rows { assert_eq!(classify_ssh_stderr(text, established), code, "{text}; established={established}"); }
    }
}
