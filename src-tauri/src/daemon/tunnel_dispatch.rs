//! Daemon-side owner of saved tunnel definitions, live runtimes and the
//! authoritative host record.
//!
//! Every request here answers for the daemon because the daemon is the only
//! process that can spawn and reap `ssh -N -L` children. A peer may not start a
//! tunnel, and the last two arms (`SpecsSave`, `SpecsReset`) could rewrite or
//! quarantine the operator's file, so `local_only` is checked per variant rather
//! than once for the family.
//!
//! Windows refuses all seven before the store is opened. The platform refusal is
//! the `#[cfg(windows)]` alternative body of each arm, not a `return` ahead of
//! the unix body: on Windows the registry, the runtimes sidecar and the
//! process-group half do not exist, so a `return` in front of code that names
//! them would be a type error in this crate's own files. The store's
//! `cfg(not(unix))` no-lock fallback is therefore unreachable, not relied upon.
//!
//! Lock order, one total order: definitions sidecar -> registry mutex ->
//! runtimes sidecar. `ssh_tunnels.lock` is held only across synchronous sections
//! and dropped before any await. A start's revision read, identity gate and
//! adoption all happen inside that section, so a concurrent save cannot land
//! between the read and the decision; holding it across the ten-second first
//! attempt would instead stall every save, invalidate and list.
//!
//! Revision authority is `max(file, memory)`, never "the file wins": during a
//! handover the draining predecessor bumps a host past what the successor
//! loaded. Cache refreshes are `cache = max(file, cache)` for the mirrored
//! reason: an overwrite moves memory backwards after a bump whose write failed,
//! and a client holding the pre-bump revision is then accepted against a
//! torn-down identity.
//!
//! The start check order is fixed: revision, then store health, then the identity
//! tuple, then `host_disabled`, then resolve, then spawn. A mismatched revision
//! is always `host_revision_stale` whatever the file's state, which is the only
//! ordering that keeps "revision before store" observable.
//!
//! The host-record observer lives here. One interval per daemon (never per host
//! or tunnel) re-reads the authoritative host record, tears that host's live
//! runtimes down to `Failed { failure: host_changed }`, and adopts a changed
//! fingerprint only on a clean strict load, once five seconds have passed since
//! the record was written. It is cancelled when nothing is live and re-armed by
//! the first runtime that becomes live afterwards: a cancelled-once observer that
//! never re-arms passes every lib test, so the re-arm is what matters. `list` and
//! `start` run the same adoption inline, gated per host, because host A holding a
//! live tunnel keeps the observer armed while the observer rightly ignores a
//! silent host B.
//!
//! Accepted V1 residual (restated from the runtimes module): a crash in the one
//! syscall between a successful `spawn()` and the pid write leaves a live child
//! the sweep cannot prove is ours. It keeps the port; the next start reports
//! `port_in_use` and the operator kills the recorded child or changes the port.
use super::{
    protocol::{
        DaemonResponse, HostRevision, HostTunnelDefinitions, StoreStatus, StoreStatusKind,
        TunnelForgetHostRequest, TunnelForgetHostResponse, TunnelInvalidateHostRequest,
        TunnelInvalidateHostResponse, TunnelListResponse, TunnelSpecsResetResponse,
        TunnelSpecsSaveRequest, TunnelSpecsSaveResponse, TunnelStartRequest, TunnelStartResponse,
        TunnelStopRequest, TunnelStopResponse,
    },
    server::{load_hosts_strict, StrictHosts},
};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::ssh::{
    tunnel::{validate_saved_spec, TunnelHostIdentity, TunnelRuntimeId, TunnelSpec},
    tunnel_store::{self, Clock, HostIdentityEntry, SystemClock, TunnelStore},
};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// How long a recorded fingerprint must be at rest before a mismatch may be
/// adopted. A save still in flight, or a backwards clock step, is "not yet".
const ADOPTION_GRACE_MS: u64 = 5_000;
/// One observer tick per daemon. Not per host, not per tunnel.
const OBSERVER_TICK: std::time::Duration = std::time::Duration::from_secs(5);
/// The re-sweep does NOT share the observer's interval: that one is cancelled
/// when nothing is live, and the case the re-sweep exists for — a successor whose
/// predecessor was SIGKILLed mid-drain — is exactly a daemon with zero live
/// runtimes. The boot pass is just this timer's first tick.
const SWEEP_TICK: std::time::Duration = std::time::Duration::from_secs(5);

/// Where a daemon's two tunnel files live, plus the data directory that holds the
/// runtimes sidecar and the instance locks. Explicit paths, so a test never has
/// to redirect process-global environment variables out from under another test.
#[derive(Debug, Clone)]
pub struct TunnelPaths {
    pub definitions: PathBuf,
    pub hosts: PathBuf,
    pub data: PathBuf,
}

impl TunnelPaths {
    pub fn new(definitions: PathBuf, hosts: PathBuf) -> Self {
        let data = definitions
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self { definitions, hosts, data }
    }
}

fn refuse(code: IpcErrorCode, message: &str, reason: &str) -> IpcError {
    IpcError::new(code, message).with_details(json!({ "reason": reason }))
}

fn stale() -> IpcError {
    refuse(
        IpcErrorCode::RequestConflict,
        "Tunnel host definition changed; re-list and retry",
        "host_revision_stale",
    )
}

/// Windows' refusal, carrying BOTH halves: `failureCode` is the `TunnelFailureCode`
/// classification the UI reads as a tunnel state, `reason` is the route/store layer's
/// vocabulary. It names the missing facility (the control socket the registry and the
/// runtimes sidecar are built on) so an operator is not left with a bare "unsupported".
fn unsupported_platform() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "SSH tunnels need a Unix control socket, which this platform does not provide",
    )
    .with_details(json!({
        "reason": crate::ipc::ssh_tunnel::UNSUPPORTED_PLATFORM,
        "failureCode": crate::ipc::ssh_tunnel::UNSUPPORTED_PLATFORM,
        "platform": "windows",
        // The facility this platform lacks, as a machine value: the registry's
        // process-group reaping and the runtimes sidecar both need a Unix control
        // socket, so silence about *why* would be a bare "unsupported".
        "missingFacility": "control_socket",
    }))
}

fn local_only() -> IpcError {
    refuse(
        IpcErrorCode::Unsupported,
        "Tunnels are available only over the local owner socket",
        "local_only",
    )
}

fn runtime_unavailable() -> IpcError {
    refuse(
        IpcErrorCode::IoError,
        "Tunnel runtime is unavailable",
        "runtime_unavailable",
    )
}

fn error_response(error: &IpcError) -> DaemonResponse {
    DaemonResponse::Error {
        message: error.message.clone(),
        code: Some(error.code.to_string()),
        details: error.details.clone(),
    }
}

fn kind(store: &TunnelStore) -> StoreStatusKind {
    match store.store_status.kind.as_str() {
        "partial" => StoreStatusKind::Partial,
        "corrupt" => StoreStatusKind::Corrupt,
        "unsupported_version" => StoreStatusKind::UnsupportedVersion,
        _ => StoreStatusKind::Ok,
    }
}

fn healthy(store: &TunnelStore) -> bool {
    matches!(kind(store), StoreStatusKind::Ok | StoreStatusKind::Partial)
}

fn status(store: &TunnelStore, path: &Path) -> StoreStatus {
    StoreStatus {
        kind: kind(store),
        path: path.to_string_lossy().into_owned(),
        invalid_hosts: store.store_status.invalid_hosts.clone(),
        quarantined_path: None,
    }
}

fn tuple(host: &crate::ssh::SshHost) -> TunnelHostIdentity {
    TunnelHostIdentity {
        hostname: host.hostname.clone(),
        username: host.username.clone(),
        port: host.port,
        identity_file: host.identity_file.clone(),
        jump_host: host.jump_host.clone(),
        disabled: host.disabled,
    }
}

fn no_identity() -> TunnelHostIdentity {
    TunnelHostIdentity {
        hostname: String::new(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        disabled: None,
    }
}

/// Raise `memory` to the file's authority, never lower it. This is
/// `cache = max(file, cache)` for the fields this module compares and writes.
fn absorb(memory: &mut TunnelStore, disk: &TunnelStore) {
    memory.revision_floor = memory.revision_floor.max(disk.revision_floor);
    for (host, revision) in &disk.host_revisions {
        let slot = memory.host_revisions.entry(host.clone()).or_default();
        *slot = (*slot).max(*revision);
    }
    for (host, incoming) in &disk.host_identities {
        if incoming.revision != disk.host_revision(host) {
            continue;
        }
        match memory.host_identities.get_mut(host) {
            Some(existing) => {
                let take = incoming.revision > existing.revision
                    || (incoming.revision == existing.revision
                        && (existing.recorded_at_unix_ms.is_none()
                            || incoming.recorded_at_unix_ms.is_some_and(|stamp| {
                                existing.recorded_at_unix_ms.is_some_and(|current| stamp >= current)
                            })));
                if take {
                    *existing = incoming.clone();
                }
            }
            None => {
                memory.host_identities.insert(host.clone(), incoming.clone());
            }
        }
    }
}

struct Memory {
    store: TunnelStore,
    definitions: HashMap<String, Vec<TunnelSpec>>,
    definitions_revision: u64,
}

pub struct TunnelDispatch {
    paths: TunnelPaths,
    clock: Arc<dyn Clock>,
    memory: Arc<Mutex<Memory>>,
    /// One `Notify` per host, fired at the end of every detached teardown exit
    /// path, so a caller can await the drain instead of polling.
    drained: Arc<Mutex<HashMap<String, Arc<tokio::sync::Notify>>>>,
    background: Mutex<bool>,
    #[cfg(unix)]
    runtime: Option<Runtime>,
}

#[cfg(unix)]
struct Runtime {
    registry: Arc<crate::ssh::tunnel::registry::TunnelRegistry>,
    records: Arc<crate::ssh::tunnel::runtime_records::RuntimeRecords>,
}

#[cfg(unix)]
impl Runtime {
    fn build(
        paths: &TunnelPaths,
        clock: Arc<dyn Clock>,
        spawner: Arc<dyn crate::ssh::tunnel::registry::TunnelSpawner>,
    ) -> std::io::Result<Self> {
        use crate::ssh::tunnel::{
            argv::ConfigSource,
            registry::{ProbeSocketFactory, TunnelRegistry},
            runtime_records::{PsProcessIdentity, RuntimeRecords},
        };
        let records = Arc::new(RuntimeRecords::open(
            paths.data.clone(),
            Arc::new(PsProcessIdentity),
        )?);
        let registry = TunnelRegistry::new(
            spawner,
            ConfigSource::Real,
            Arc::new(ProbeSocketFactory::default()),
            clock,
        )
        .with_records(Arc::clone(&records));
        Ok(Self { registry: Arc::new(registry), records })
    }
}

impl TunnelDispatch {
    pub fn new(paths: TunnelPaths) -> Self {
        Self::with_clock(paths, Arc::new(SystemClock))
    }

    /// The clock seam is shared with the registry, so identity adoption and every
    /// transition stamp are driven by the same fake under test.
    pub fn with_clock(paths: TunnelPaths, clock: Arc<dyn Clock>) -> Self {
        #[cfg(unix)]
        let spawner: Arc<dyn crate::ssh::tunnel::registry::TunnelSpawner> =
            Arc::new(crate::ssh::tunnel::registry::ProcessSpawner);
        #[cfg(windows)]
        let spawner = ();
        Self::build(paths, clock, spawner)
    }

    /// Production uses the real spawner; a test drives the scripted one without
    /// reaching into the registry.
    #[cfg(unix)]
    pub fn with_spawner(
        paths: TunnelPaths,
        clock: Arc<dyn Clock>,
        spawner: Arc<dyn crate::ssh::tunnel::registry::TunnelSpawner>,
    ) -> Self {
        Self::build(paths, clock, spawner)
    }

    #[cfg(unix)]
    fn build(
        paths: TunnelPaths,
        clock: Arc<dyn Clock>,
        spawner: Arc<dyn crate::ssh::tunnel::registry::TunnelSpawner>,
    ) -> Self {
        let mut store = tunnel_store::load_store(&paths.definitions, clock.now_unix_ms())
            .unwrap_or_else(|_| TunnelStore::default());
        if kind(&store) == StoreStatusKind::Corrupt {
            // A corrupt boot seeds the floor from the wall clock: it exceeds any
            // counter this daemon issued before the restart, so a pre-reset
            // revision still fails the check. It stays for the process lifetime.
            store.seed_floor(clock.now_unix_ms() / 1_000);
        }
        let definitions = store.hosts.clone();
        let definitions_revision = store.definitions_revision;
        let runtime = match Runtime::build(&paths, Arc::clone(&clock), spawner) {
            Ok(runtime) => Some(runtime),
            Err(error) => {
                // Fails closed per request rather than leaking an unreclaimable
                // child: every unix body answers `runtime_unavailable`.
                tracing::error!("Cannot open the tunnel runtime sidecar: {error}");
                None
            }
        };
        Self {
            paths,
            clock,
            memory: Arc::new(Mutex::new(Memory { store, definitions, definitions_revision })),
            drained: Arc::new(Mutex::new(HashMap::new())),
            background: Mutex::new(false),
            runtime,
        }
    }

    #[cfg(windows)]
    #[allow(clippy::unused_unit)]
    fn build(paths: TunnelPaths, clock: Arc<dyn Clock>, _spawner: ()) -> Self {
        let mut store = tunnel_store::load_store(&paths.definitions, clock.now_unix_ms())
            .unwrap_or_else(|_| TunnelStore::default());
        if kind(&store) == StoreStatusKind::Corrupt {
            store.seed_floor(clock.now_unix_ms() / 1_000);
        }
        let definitions = store.hosts.clone();
        let definitions_revision = store.definitions_revision;
        Self {
            paths,
            clock,
            memory: Arc::new(Mutex::new(Memory { store, definitions, definitions_revision })),
            drained: Arc::new(Mutex::new(HashMap::new())),
            background: Mutex::new(false),
        }
    }

    pub fn paths(&self) -> &TunnelPaths {
        &self.paths
    }

    #[cfg(unix)]
    fn runtime(&self) -> Result<&Runtime, IpcError> {
        self.runtime.as_ref().ok_or_else(runtime_unavailable)
    }

    #[cfg(unix)]
    pub fn registry(&self) -> Result<Arc<crate::ssh::tunnel::registry::TunnelRegistry>, IpcError> {
        Ok(Arc::clone(&self.runtime()?.registry))
    }

    #[cfg(unix)]
    pub fn records(&self) -> Result<Arc<crate::ssh::tunnel::runtime_records::RuntimeRecords>, IpcError> {
        Ok(Arc::clone(&self.runtime()?.records))
    }

    #[cfg(test)]
    pub fn invalidation_drained(&self, host_id: &str) -> Arc<tokio::sync::Notify> {
        self.notify(host_id)
    }

    fn notify(&self, host_id: &str) -> Arc<tokio::sync::Notify> {
        self.drained
            .lock()
            .unwrap()
            .entry(host_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Notify::new()))
            .clone()
    }

    fn signal_drained(&self, host_id: &str) {
        self.notify(host_id).notify_waiters();
    }

    /// Boot pass plus the never-cancelled re-sweep. Calling this twice is a no-op,
    /// so a handover restart cannot double-run the boot pass.
    pub fn start_background_tasks(self: &Arc<Self>) {
        {
            let mut started = self.background.lock().unwrap();
            if *started {
                return;
            }
            *started = true;
        }
        #[cfg(unix)]
        if self.runtime.is_some() {
            let dispatch = Arc::clone(self);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(SWEEP_TICK);
                loop {
                    ticker.tick().await;
                    dispatch.sweep_once();
                }
            });
            let dispatch = Arc::clone(self);
            tokio::spawn(async move {
                // Cancelled on idle, re-armed by the next live runtime: a
                // cancelled-once observer that never re-arms is the bug this
                // shape exists to prevent.
                loop {
                    let Ok(registry) = dispatch.registry() else { return };
                    registry.wait_until_live().await;
                    loop {
                        let mut ticker = tokio::time::interval(OBSERVER_TICK);
                        ticker.tick().await;
                        tokio::select! {
                            _ = ticker.tick() => dispatch.observe_hosts().await,
                            _ = registry.wait_until_idle() => break,
                        }
                    }
                }
            });
        }
    }

    /// One sweep. Split out so a test drives it without a timer.
    pub fn sweep_once(&self) {
        #[cfg(unix)]
        if let Ok(runtime) = self.runtime() {
            for record in runtime.registry.sweep_records(&runtime.records) {
                tracing::warn!(
                    runtime_id = %record.runtime_id,
                    port = record.local_port,
                    killed = record.killed,
                    "Reclaimed a tunnel runtime of a dead daemon instance"
                );
            }
        }
    }

    /// One observer tick. Only a clean strict load may drive a teardown or an
    /// adoption: a missing or half-readable host file is not "every host left".
    pub async fn observe_hosts(&self) {
        #[cfg(unix)]
        {
            let StrictHosts::Ok(hosts) = load_hosts_strict(&self.paths.hosts) else {
                return;
            };
            for host in hosts {
                if self.adopt_if_changed(&host, false).await {
                    tracing::warn!(
                        host = %host.id,
                        "Adopted a changed SSH host tuple and tore its tunnels down"
                    );
                }
            }
        }
    }

    /// The single adoption rule, shared by the observer, `list` and `start`.
    /// Returns true when it bumped the revision.
    #[cfg(unix)]
    async fn adopt_if_changed(&self, host: &crate::ssh::SshHost, record_missing: bool) -> bool {
        self.adopt_if_changed_locked(host, record_missing, None).await
    }

    async fn adopt_if_changed_locked(
        &self,
        host: &crate::ssh::SshHost,
        record_missing: bool,
        held: Option<&tunnel_store::StoreLock>,
    ) -> bool {
        let disk = tuple(host);
        let now = self.clock.now_unix_ms();
        let recorded = {
            let memory = self.memory.lock().unwrap();
            memory.store.active_host_identity(&host.id).cloned()
        };
        let Some(recorded) = recorded else {
            if record_missing {
                // No active entry for this revision — revision 0, a pre-feature
                // file, or an invalidate that carried no identity. Record the
                // tuple and proceed; there is nothing to mismatch against.
                let mut memory = self.memory.lock().unwrap();
                let revision = memory.store.host_revision(&host.id);
                memory.store.host_identities.insert(
                    host.id.clone(),
                    HostIdentityEntry {
                        revision,
                        fingerprint: disk,
                        recorded_at_unix_ms: Some(now),
                        pending_since_disk: now,
                    },
                );
                match held {
                    Some(lock) => {
                        if let Err(error) = lock.persist_authority(&memory.store, now) {
                            tracing::warn!(host = %host.id, ?error, "Cannot persist a recorded host tuple");
                        }
                    }
                    None => {
                        let definitions = self.paths.definitions.clone();
                        if let Ok(lock) = tunnel_store::lock_store(&definitions) {
                            if let Err(error) = lock.persist_authority(&memory.store, now) {
                                tracing::warn!(host = %host.id, ?error, "Cannot persist a recorded host tuple");
                            }
                        }
                    }
                }
            }
            return false;
        };
        if recorded.fingerprint == disk {
            return false;
        }
        let adoptable = recorded.recorded_at_unix_ms.is_some_and(|stamp| {
            // Never a raw `u64` subtraction: a backwards clock step is "not yet",
            // not a wrapped giant.
            now.checked_sub(stamp)
                .is_some_and(|elapsed| elapsed >= ADOPTION_GRACE_MS)
        });
        if !adoptable {
            return false;
        }
        let Ok(registry) = self.registry() else {
            return false;
        };
        let pending = registry.invalidate_host(&host.id);
        self.bump(&host.id, disk, now);
        if let Err(error) = crate::ssh::tunnel::registry::wait_generations(pending).await {
            tracing::warn!(host = %host.id, ?error, "Host change teardown did not finish");
        }
        self.signal_drained(&host.id);
        true
    }

    /// Bumps to `max(file, memory) + 1` under the definitions lock and records
    /// the new tuple. The write lands before any reaper await ever starts.
    #[cfg(unix)]
    fn bump(&self, host_id: &str, identity: TunnelHostIdentity, now: u64) -> u64 {
        let lock = match tunnel_store::lock_store(&self.paths.definitions) {
            Ok(lock) => lock,
            Err(error) => {
                // Best effort: the in-memory bump still happens and the next lock
                // holder re-persists `max(file, memory)`.
                tracing::warn!(host = host_id, ?error, "Cannot lock the tunnel store to adopt a host");
                return self.bump_memory(host_id, identity, now);
            }
        };
        let fresh = lock.load(now).unwrap_or_else(|_| TunnelStore::default());
        let mut memory = self.memory.lock().unwrap();
        absorb(&mut memory.store, &fresh);
        let raised = memory
            .store
            .host_revision(host_id)
            .max(fresh.host_revision(host_id));
        memory.store.host_revisions.insert(host_id.into(), raised);
        let revision = memory
            .store
            .bump_host_revision(host_id, identity, now)
            .unwrap_or(raised);
        memory.store.stamp_identity(host_id, revision, now);
        if let Err(error) = lock.persist_authority(&memory.store, now) {
            // A failed repair is logged and never fails the call.
            tracing::warn!(host = host_id, ?error, "Cannot persist the adopted host revision");
        }
        revision
    }

    fn bump_memory(&self, host_id: &str, identity: TunnelHostIdentity, now: u64) -> u64 {
        let mut memory = self.memory.lock().unwrap();
        let revision = memory
            .store
            .bump_host_revision(host_id, identity, now)
            .unwrap_or_else(|_| memory.store.host_revision(host_id));
        memory.store.stamp_identity(host_id, revision, now);
        revision
    }

    // -- the seven arms -----------------------------------------------------
    //
    // `local_only` first: it is pure and touches no file, so a peer's request
    // never reaches the store on any platform. Then the platform split.

    pub async fn start(&self, request: TunnelStartRequest, is_local: bool) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.start_unix(request).await {
                Ok(response) => DaemonResponse::SshTunnelStart { response },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            let _ = request;
            error_response(&unsupported_platform())
        }
    }

    pub async fn stop(&self, request: TunnelStopRequest, is_local: bool) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.stop_unix(request).await {
                Ok(response) => DaemonResponse::SshTunnelStop { response },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            let _ = request;
            error_response(&unsupported_platform())
        }
    }

    pub async fn list(&self, is_local: bool) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.list_unix().await {
                Ok(response) => DaemonResponse::SshTunnelList { response },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            error_response(&unsupported_platform())
        }
    }

    pub async fn invalidate_host(
        &self,
        request: TunnelInvalidateHostRequest,
        is_local: bool,
    ) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.invalidate_host_unix(request).await {
                Ok(revision) => DaemonResponse::SshTunnelInvalidateHost {
                    response: TunnelInvalidateHostResponse { revision },
                },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            let _ = request;
            error_response(&unsupported_platform())
        }
    }

    pub async fn specs_save(
        &self,
        request: TunnelSpecsSaveRequest,
        is_local: bool,
    ) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.specs_save_unix(request).await {
                Ok(definitions_revision) => DaemonResponse::SshTunnelSpecsSave {
                    response: TunnelSpecsSaveResponse { definitions_revision },
                },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            let _ = request;
            error_response(&unsupported_platform())
        }
    }

    pub async fn specs_reset(&self, is_local: bool) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.specs_reset_unix() {
                Ok(quarantined_path) => DaemonResponse::SshTunnelSpecsReset {
                    response: TunnelSpecsResetResponse { quarantined_path },
                },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            error_response(&unsupported_platform())
        }
    }

    pub async fn forget_host(
        &self,
        request: TunnelForgetHostRequest,
        is_local: bool,
    ) -> DaemonResponse {
        if !is_local {
            return error_response(&local_only());
        }
        #[cfg(unix)]
        {
            match self.forget_host_unix(request).await {
                Ok(removed) => DaemonResponse::SshTunnelForgetHost {
                    response: TunnelForgetHostResponse { removed },
                },
                Err(error) => error_response(&error),
            }
        }
        #[cfg(windows)]
        {
            let _ = request;
            error_response(&unsupported_platform())
        }
    }

    // -- unix bodies --------------------------------------------------------

    #[cfg(unix)]
    async fn start_unix(
        &self,
        request: TunnelStartRequest,
    ) -> Result<TunnelStartResponse, IpcError> {
        self.runtime()?;
        let now = self.clock.now_unix_ms();
        // Revision first, then store health, then the identity tuple. All of it
        // under one hold of the definitions lock; the lock is gone before any
        // await, so it is never held across the start window.
        let (accepted_revision, store, lock) = self.revision_gate(&request, now)?;
        match kind(&store) {
            // A mismatched revision was refused above, so a matching revision on
            // a corrupt file reaches exactly this refusal.
            StoreStatusKind::Corrupt => {
                return Err(refuse(
                    IpcErrorCode::RequestConflict,
                    "Tunnel definitions are unreadable",
                    "corrupt_store",
                ))
            }
            StoreStatusKind::UnsupportedVersion => {
                return Err(refuse(
                    IpcErrorCode::Unsupported,
                    "Tunnel definitions use a newer format",
                    "unsupported_version",
                ))
            }
            StoreStatusKind::Ok | StoreStatusKind::Partial => {}
        }
        let (host, strict) = match load_hosts_strict(&self.paths.hosts) {
            StrictHosts::Ok(hosts) => (
                hosts.iter().find(|host| host.id == request.host_id).cloned(),
                true,
            ),
            StrictHosts::Degraded(hosts) => (
                hosts.iter().find(|host| host.id == request.host_id).cloned(),
                false,
            ),
            StrictHosts::Absent | StrictHosts::WholeFileInvalid => (None, false),
        };
        let Some(host) = host else {
            return Err(refuse(
                IpcErrorCode::NotFound,
                "No such SSH host",
                "host_not_found",
            ));
        };
        if strict {
            // Inline adoption is unconditional under its per-host guards, never
            // conditioned on "no live runtime daemon-wide": host A holding a
            // live tunnel keeps the observer armed while host B stays silent.
            if self.adopt_if_changed_locked(&host, true, Some(&lock)).await {
                return Err(stale());
            }
            let recorded = {
                let memory = self.memory.lock().unwrap();
                memory.store.active_host_identity(&host.id).cloned()
            };
            if recorded.is_some_and(|entry| entry.fingerprint != tuple(&host)) {
                return Err(stale());
            }
        }
        if host.disabled == Some(true) {
            return Err(refuse(
                IpcErrorCode::InvalidArgument,
                "SSH host is disabled",
                "host_disabled",
            ));
        }
        // Resolve from the file, never from a boot-time cache: a GUI save does not
        // notify the daemon, so a start-by-id right after a save must see the
        // edited port and target.
        let (saved, spec) = match (&request.spec, &request.spec_id) {
            (Some(spec), None) => (false, spec.clone()),
            (None, Some(spec_id)) => {
                match store
                    .hosts
                    .get(&request.host_id)
                    .and_then(|specs| specs.iter().find(|spec| &spec.id.0 == spec_id))
                    .cloned()
                {
                    Some(spec) => (true, spec),
                    None => {
                        return Err(refuse(
                            IpcErrorCode::NotFound,
                            "No such tunnel definition",
                            "tunnel_spec_not_found",
                        ))
                    }
                }
            }
            _ => {
                // The wire type forbids both and neither; a hand-built request
                // still answers with a code instead of hanging.
                return Err(refuse(
                    IpcErrorCode::InvalidArgument,
                    "Exactly one of spec and specId is required",
                    "invalid_request",
                ))
            }
        };
        if saved {
            validate_saved_spec(&spec)?;
        }
        self.cache_specs(&request.host_id, &store);
        drop(lock);
        let view = self.runtime()?.registry.start(&host, spec, saved).await?;
        Ok(TunnelStartResponse { view, accepted_revision })
    }

    /// Revision authority is `max(file, memory)`. The returned lock is held so the
    /// caller's decision stays inside the same synchronous section.
    #[cfg(unix)]
    fn revision_gate(
        &self,
        request: &TunnelStartRequest,
        now: u64,
    ) -> Result<(u64, TunnelStore, tunnel_store::StoreLock), IpcError> {
        let lock = tunnel_store::lock_store(&self.paths.definitions)?;
        let fresh = lock.load(now)?;
        let accepted = {
            let mut memory = self.memory.lock().unwrap();
            absorb(&mut memory.store, &fresh);
            let accepted = fresh
                .host_revision(&request.host_id)
                .max(memory.store.host_revision(&request.host_id));
            match request.host_revision {
                // Absent is wire-compatible with older callers and still refused:
                // an unversioned start is the stale start this barrier stops.
                Some(revision) if revision == accepted => {}
                _ => return Err(stale()),
            }
            // This call holds the lock on a healthy store, so a bump whose write
            // failed is re-persisted before it answers. Best effort by contract:
            // a failed repair is logged and never fails the call.
            if healthy(&fresh)
                && fresh.host_revision(&request.host_id) < memory.store.host_revision(&request.host_id)
            {
                if let Err(error) = lock.persist_authority(&memory.store, now) {
                    tracing::warn!(?error, "Cannot re-persist the tunnel revision authority");
                }
            }
            accepted
        };
        Ok((accepted, fresh, lock))
    }

    #[cfg(unix)]
    async fn stop_unix(&self, request: TunnelStopRequest) -> Result<TunnelStopResponse, IpcError> {
        let registry = self.registry()?;
        let id = TunnelRuntimeId(request.runtime_id);
        // Idempotent: an unknown or already removed runtime is not an error, so a
        // stop retried after a reconnect cannot fail its caller.
        if registry.subscribe_state(&id).is_none() {
            return Ok(TunnelStopResponse { stopped: false });
        }
        registry.stop(&id).await?;
        Ok(TunnelStopResponse { stopped: true })
    }

    #[cfg(unix)]
    async fn list_unix(&self) -> Result<TunnelListResponse, IpcError> {
        let registry = self.registry()?;
        let now = self.clock.now_unix_ms();
        let hosts = load_hosts_strict(&self.paths.hosts);
        // The same adoption rule the observer applies, so a client that never waits
        // for a tick still converges.
        if let StrictHosts::Ok(loaded) = &hosts {
            for host in loaded {
                self.adopt_if_changed(host, false).await;
            }
        }
        let Snapshot { store, definitions_revision } = self.snapshot(&hosts, now)?;
        let views = registry.list(&store.hosts)?;
        // Report every host the daemon can still see, including one whose only
        // trace is the host file (a corrupt boot reads no host_revisions rows but
        // must still report the floored revision, never 0).
        let mut ids: HashSet<String> = HashSet::new();
        ids.extend(views.iter().map(|view| view.host_id.clone()));
        ids.extend(store.hosts.keys().cloned());
        ids.extend(store.host_revisions.keys().cloned());
        if let StrictHosts::Ok(loaded) | StrictHosts::Degraded(loaded) = &hosts {
            ids.extend(loaded.iter().map(|host| host.id.clone()));
        }
        let mut host_revisions: Vec<HostRevision> = ids
            .into_iter()
            .map(|host| HostRevision {
                revision: store.host_revision(&host),
                host_id: host,
            })
            .collect();
        host_revisions.sort_by(|a, b| a.host_id.cmp(&b.host_id));
        let mut definitions: Vec<HostTunnelDefinitions> = store
            .hosts
            .iter()
            .map(|(host_id, specs)| HostTunnelDefinitions {
                host_id: host_id.clone(),
                specs: specs.clone(),
            })
            .collect();
        definitions.sort_by(|a, b| a.host_id.cmp(&b.host_id));
        Ok(TunnelListResponse {
            views,
            definitions,
            host_revisions,
            definitions_revision,
            store_status: status(&store, &self.paths.definitions),
        })
    }

    /// Convergent prune under the definitions lock. Definitions and invalid rows
    /// of a host the host store no longer has go; `host_revisions` deliberately
    /// stays, so a deleted-and-recreated host cannot accept a stale start.
    #[cfg(unix)]
    fn snapshot(&self, hosts: &StrictHosts, now: u64) -> Result<Snapshot, IpcError> {
        let lock = tunnel_store::lock_store(&self.paths.definitions)?;
        let fresh = lock.load(now)?;
        let mut memory = self.memory.lock().unwrap();
        absorb(&mut memory.store, &fresh);
        if healthy(&fresh) {
            match hosts {
                StrictHosts::Ok(loaded) => {
                    let present: HashSet<&str> =
                        loaded.iter().map(|host| host.id.as_str()).collect();
                    let mut pruned = fresh.clone();
                    pruned.hosts.retain(|host, _| present.contains(host.as_str()));
                    pruned
                        .invalid_rows
                        .retain(|host, _| present.contains(host.as_str()));
                    if pruned.hosts.len() != fresh.hosts.len()
                        || pruned.invalid_rows.len() != fresh.invalid_rows.len()
                    {
                        if let Err(error) = lock.persist_authority(&pruned, now) {
                            tracing::warn!(?error, "Cannot persist the tunnel definitions prune");
                        }
                    }
                    memory.definitions = pruned.hosts.clone();
                    memory.store.hosts = pruned.hosts;
                    memory.store.invalid_rows = pruned.invalid_rows;
                }
                _ => {
                    memory.definitions = fresh.hosts.clone();
                    memory.store.hosts = fresh.hosts.clone();
                    memory.store.invalid_rows = fresh.invalid_rows.clone();
                }
            }
        }
        Ok(Snapshot { store: memory.store.clone(), definitions_revision: memory.definitions_revision })
    }

    #[cfg(unix)]
    async fn invalidate_host_unix(
        &self,
        request: TunnelInvalidateHostRequest,
    ) -> Result<u64, IpcError> {
        let registry = self.registry()?;
        let now = self.clock.now_unix_ms();
        let identity = request.identity.clone().unwrap_or_else(no_identity);
        // Synchronous phase: bump memory, persist the revision, tear the runtimes
        // down, mark the stamp pending — then drop the locks BEFORE the reaper
        // await. The write is before the first await, never after it.
        let (revision, pending) = {
            let lock = tunnel_store::lock_store(&self.paths.definitions)?;
            let fresh = lock.load(now).unwrap_or_else(|_| TunnelStore::default());
            let mut memory = self.memory.lock().unwrap();
            absorb(&mut memory.store, &fresh);
            let raised = memory
                .store
                .host_revision(&request.host_id)
                .max(fresh.host_revision(&request.host_id));
            memory.store.host_revisions.insert(request.host_id.clone(), raised);
            let revision = memory
                .store
                .bump_host_revision(&request.host_id, identity, now)?;
            memory.store.mark_identity_pending(&request.host_id, now);
            let pending = registry.invalidate_host(&request.host_id);
            if let Err(error) = lock.persist_authority(&memory.store, now) {
                tracing::warn!(host = %request.host_id, ?error, "Cannot persist the invalidated host revision");
            }
            (revision, pending)
        };
        // The stamp is written by a detached task, so closing the client socket
        // mid-await cannot leave the record permanently pending. Every exit path
        // fires the per-host notify exactly once.
        let memory = Arc::clone(&self.memory);
        let drained = self.notify(&request.host_id);
        let definitions = self.paths.definitions.clone();
        let clock = Arc::clone(&self.clock);
        let host_id = request.host_id.clone();
        tokio::spawn(async move {
            let _ = crate::ssh::tunnel::registry::wait_generations(pending).await;
            let stamped_at = clock.now_unix_ms();
            let mut guard = memory.lock().unwrap();
            guard.store.stamp_identity(&host_id, revision, stamped_at);
            // A newer invalidation may already own the entry; `stamp_identity`
            // leaves a foreign revision alone by construction.
            if let Ok(lock) = tunnel_store::lock_store(&definitions) {
                if let Err(error) =
                    lock.stamp_identity(&mut guard.store, &host_id, revision, stamped_at)
                {
                    tracing::warn!(host = %host_id, ?error, "Cannot persist a host identity stamp");
                }
            }
            drop(guard);
            drained.notify_waiters();
        });
        Ok(revision)
    }

    #[cfg(unix)]
    async fn specs_save_unix(&self, request: TunnelSpecsSaveRequest) -> Result<u64, IpcError> {
        let now = self.clock.now_unix_ms();
        // A clean host load makes an absent host a refusal; a degraded or absent
        // file accepts the save rather than losing the operator's edit.
        if let StrictHosts::Ok(hosts) = load_hosts_strict(&self.paths.hosts) {
            if !hosts.iter().any(|host| host.id == request.host_id) {
                return Err(refuse(
                    IpcErrorCode::NotFound,
                    "No such SSH host",
                    "host_not_found",
                ));
            }
        }
        let lock = tunnel_store::lock_store(&self.paths.definitions)?;
        let mut memory = self.memory.lock().unwrap();
        let saved = lock.save_specs(
            &request.host_id,
            request.specs.clone(),
            request.expected_revision,
            request.force,
            request.discard_invalid,
            &memory.store,
            now,
        )?;
        absorb(&mut memory.store, &saved);
        memory.store.hosts = saved.hosts.clone();
        memory.store.invalid_rows = saved.invalid_rows.clone();
        memory.store.definitions_revision = saved.definitions_revision;
        memory.definitions = saved.hosts.clone();
        memory.definitions_revision = saved.definitions_revision;
        Ok(saved.definitions_revision)
    }

    #[cfg(unix)]
    fn specs_reset_unix(&self) -> Result<Option<String>, IpcError> {
        let now = self.clock.now_unix_ms();
        let lock = tunnel_store::lock_store(&self.paths.definitions)?;
        let mut memory = self.memory.lock().unwrap();
        // Reset seeds every host it can still see at `max(in-memory revision,
        // floor)`, so a pre-reset revision can never be accepted again.
        let quarantined = lock.reset(now / 1_000, &memory.store)?;
        let fresh = lock.load(now)?;
        memory.store = fresh;
        memory.definitions = memory.store.hosts.clone();
        memory.definitions_revision = memory.store.definitions_revision;
        Ok(Some(quarantined.to_string_lossy().into_owned()))
    }

    #[cfg(unix)]
    async fn forget_host_unix(&self, request: TunnelForgetHostRequest) -> Result<u32, IpcError> {
        let registry = self.registry()?;
        let now = self.clock.now_unix_ms();
        let removed = {
            let lock = tunnel_store::lock_store(&self.paths.definitions)?;
            let fresh = lock.load(now).unwrap_or_else(|_| TunnelStore::default());
            let mut memory = self.memory.lock().unwrap();
            absorb(&mut memory.store, &fresh);
            let removed = memory
                .store
                .hosts
                .remove(&request.host_id)
                .map(|specs| specs.len() as u32)
                .unwrap_or(0);
            memory.store.invalid_rows.remove(&request.host_id);
            memory.store.definitions_revision = memory
                .store
                .definitions_revision
                .checked_add(1)
                .unwrap_or(u64::MAX);
            if let Err(error) = lock.persist_authority(&memory.store, now) {
                tracing::warn!(host = %request.host_id, ?error, "Cannot persist a forgotten host");
            }
            memory.definitions = memory.store.hosts.clone();
            memory.definitions_revision = memory.store.definitions_revision;
            removed
        };
        let pending = registry.invalidate_host(&request.host_id);
        if let Err(error) = crate::ssh::tunnel::registry::wait_generations(pending).await {
            tracing::warn!(host = %request.host_id, ?error, "Forgotten host teardown did not finish");
        }
        Ok(removed)
    }

    #[cfg(unix)]
    fn cache_specs(&self, host_id: &str, store: &TunnelStore) {
        let mut memory = self.memory.lock().unwrap();
        if let Some(specs) = store.hosts.get(host_id) {
            memory.definitions.insert(host_id.to_string(), specs.clone());
        }
        memory.definitions_revision = memory
            .definitions_revision
            .max(store.definitions_revision);
    }

    /// Orderly exit: the daemon's shutdown path calls this beside the session
    /// service's, so a clean exit reaps every child. A SIGKILL leaves the runtimes
    /// sidecar for the next daemon's sweep.
    pub async fn shutdown(&self) -> Result<(), IpcError> {
        #[cfg(unix)]
        {
            let registry = self.registry()?;
            let result = registry.shutdown().await;
            // The children are reaped, so this instance's records are stale. Only
            // its own are removed; a live predecessor's must survive.
            let records = self.runtime()?.records.clone();
            let owner = records.owner().owner_instance_id.clone();
            for record in records.load().unwrap_or_default().into_values() {
                if record.owner.owner_instance_id == owner {
                    let _ = records.remove(
                        &TunnelRuntimeId(record.runtime_id.clone()),
                        record.generation,
                    );
                }
            }
            result
        }
        #[cfg(windows)]
        {
            Ok(())
        }
    }
}

struct Snapshot {
    store: TunnelStore,
    definitions_revision: u64,
}
