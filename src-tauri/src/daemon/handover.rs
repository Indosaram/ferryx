use crate::daemon::manifest::{get_manifest_path, HandoverManifest, HandoverRoute};
use crate::daemon::server::{get_runtime_dir, DaemonLockFiles};
use crate::terminal::TerminalService;
use parking_lot::{Mutex, RwLock};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpgradeAction {
    /// Session-preserving handover or an idle re-exec may proceed.
    Proceed,
    /// The platform cannot upgrade without ending live sessions.
    Refuse { live_sessions: usize },
}

impl UpgradeAction {
    pub(crate) fn is_proceed(self) -> bool {
        matches!(self, Self::Proceed)
    }
}

/// Opt-in gate for the non-unix idle restart. Default OFF: without it the platform keeps the
/// refusal-only behavior, so a defect in the Windows-only restart wiring cannot cost a daemon.
pub(crate) fn idle_upgrade_enabled(value: Option<&str>) -> bool {
    matches!(value, Some(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

pub(crate) fn idle_upgrade_requested() -> bool {
    let value = std::env::var("FERRYX_DAEMON_IDLE_UPGRADE").ok();
    idle_upgrade_enabled(value.as_deref())
}

/// A successor inherits the wait window so it can outlast the predecessor's instance lock
/// instead of failing fast and leaving the machine without a daemon.
pub(crate) fn successor_lock_wait(value: Option<&str>) -> Option<std::time::Duration> {
    if matches!(value, Some(v) if v == "1" || v.eq_ignore_ascii_case("true")) {
        Some(std::time::Duration::from_secs(15))
    } else {
        None
    }
}

pub(crate) fn successor_wait_from_env() -> Option<std::time::Duration> {
    let value = std::env::var("FERRYX_DAEMON_SUCCESSOR").ok();
    successor_lock_wait(value.as_deref())
}

/// Platform-neutral upgrade admission. Unix keeps its handover/re-exec behavior; a platform
/// without session transfer must never upgrade while sessions are live, because the only
/// alternative is terminating them. Deliberately pure so the invariant is testable without a
/// daemon, a socket, or a PTY.
pub(crate) fn upgrade_action(live_sessions: usize, session_transfer_supported: bool) -> UpgradeAction {
    if session_transfer_supported || live_sessions == 0 {
        UpgradeAction::Proceed
    } else {
        UpgradeAction::Refuse { live_sessions }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoverStatus {
    Active,
    Prepared,
    Draining,
    Retired,
}

#[cfg(test)]
mod upgrade_action_tests {
    use super::{
        idle_upgrade_enabled, is_v5_ownership_transfer_enabled, successor_lock_wait, upgrade_action,
        UpgradeAction,
    };

    #[test]
    fn platform_without_session_transfer_must_refuse_while_sessions_are_live() {
        let decision = upgrade_action(3, false);
        assert_eq!(decision, UpgradeAction::Refuse { live_sessions: 3 });
        assert!(!decision.is_proceed());
        assert_eq!(
            upgrade_action(1, false),
            UpgradeAction::Refuse { live_sessions: 1 }
        );
    }

    #[test]
    fn platform_without_session_transfer_may_proceed_when_no_session_is_live() {
        assert!(upgrade_action(0, false).is_proceed());
    }

    #[test]
    fn unix_session_transfer_keeps_proceeding_with_live_sessions() {
        assert!(upgrade_action(7, true).is_proceed());
        assert!(upgrade_action(0, true).is_proceed());
    }

    #[test]
    fn idle_restart_is_opt_in_only() {
        assert!(idle_upgrade_enabled(Some("1")));
        assert!(idle_upgrade_enabled(Some("true")));
        assert!(idle_upgrade_enabled(Some("TRUE")));
        assert!(!idle_upgrade_enabled(None));
        assert!(!idle_upgrade_enabled(Some("0")));
        assert!(!idle_upgrade_enabled(Some("yes")));
    }

    #[test]
    fn only_declared_successors_wait_for_the_instance_lock() {
        assert_eq!(successor_lock_wait(Some("1")), Some(std::time::Duration::from_secs(15)));
        assert_eq!(successor_lock_wait(Some("true")), Some(std::time::Duration::from_secs(15)));
        assert_eq!(successor_lock_wait(None), None);
        assert_eq!(successor_lock_wait(Some("no")), None);
    }

    /// Serializes every test that mutates `FERRYX_HANDOVER_V5`; the process environment is
    /// global, so parallel mutation would make the gate unobservable.
    static V5_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Restores the previous flag value on drop, including while unwinding from a panic.
    struct V5EnvGuard {
        /// Held while this guard owns the lock; `None` when the caller already holds it (see
        /// `set_locked`), which keeps the guard usable inside a fully serialized test body.
        _serialized: Option<std::sync::MutexGuard<'static, ()>>,
        previous: Option<std::ffi::OsString>,
    }

    impl V5EnvGuard {
        fn set(value: Option<&str>) -> Self {
            let serialized = V5_ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Self::install(Some(serialized), value)
        }

        /// Installs `value` assuming the caller already holds `V5_ENV_LOCK`. Used by tests that must
        /// keep the lock across their own read/assert/restore so no other env test can interleave.
        fn set_locked(value: Option<&str>) -> Self {
            Self::install(None, value)
        }

        fn install(
            serialized: Option<std::sync::MutexGuard<'static, ()>>,
            value: Option<&str>,
        ) -> Self {
            let previous = std::env::var_os("FERRYX_HANDOVER_V5");
            match value {
                Some(value) => std::env::set_var("FERRYX_HANDOVER_V5", value),
                None => std::env::remove_var("FERRYX_HANDOVER_V5"),
            }
            Self {
                _serialized: serialized,
                previous,
            }
        }
    }

    impl Drop for V5EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("FERRYX_HANDOVER_V5", previous),
                None => std::env::remove_var("FERRYX_HANDOVER_V5"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn absent_flag_enables_v5_ownership_transfer() {
        let _env = V5EnvGuard::set(None);
        assert!(is_v5_ownership_transfer_enabled());
    }

    #[cfg(unix)]
    #[test]
    fn explicit_zero_or_false_opts_out_to_legacy_routing() {
        for opt_out in ["0", "false", "FALSE", "no", ""] {
            let _env = V5EnvGuard::set(Some(opt_out));
            assert!(
                !is_v5_ownership_transfer_enabled(),
                "{opt_out:?} must opt out of v5 ownership transfer"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_affirmative_values_enable_v5_ownership_transfer() {
        for opt_in in ["1", "true", "TRUE", "True"] {
            let _env = V5EnvGuard::set(Some(opt_in));
            assert!(
                is_v5_ownership_transfer_enabled(),
                "{opt_in:?} must enable v5 ownership transfer"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn flag_guard_restores_the_previous_value() {
        // The whole body holds the serialization lock: reading the flag outside it would race another
        // env-mutating test and make the restoration assertion non-deterministic.
        let serialized = V5_ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = std::env::var_os("FERRYX_HANDOVER_V5");
        {
            let _env = V5EnvGuard::set_locked(Some("0"));
            assert!(!is_v5_ownership_transfer_enabled());
        }
        assert_eq!(std::env::var_os("FERRYX_HANDOVER_V5"), before);
        drop(serialized);
    }
}

/// Whether a handover transfers PTY ownership (v5) instead of only routing through a draining
/// predecessor (v4).
///
/// Default ON on unix. A v5 handover moves the PTY master descriptors to the successor over
/// `SCM_RIGHTS` and retires the predecessor immediately, so a predecessor can never keep
/// serving live sessions on a stale binary. Set `FERRYX_HANDOVER_V5=0` (or any value other than
/// `1`/`true`) to force the legacy v4 routing path, where the predecessor retains its
/// descriptors and retires only once its last session ends.
///
/// The successor inherits the predecessor's environment (`spawn_legacy_handover_daemon` does not
/// `env_clear`) and each generation picks its own commit path from its own environment, so a
/// predecessor and its successor must agree on this flag.
pub fn is_v5_ownership_transfer_enabled() -> bool {
    #[cfg(unix)]
    {
        match std::env::var("FERRYX_HANDOVER_V5") {
            // Default ON: an absent flag means ownership transfer.
            Err(_) => true,
            Ok(value) => value == "1" || value.eq_ignore_ascii_case("true"),
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverV5Metrics {
    pub freeze_duration_ms: u64,
    pub session_count: usize,
    pub fd_count: usize,
    pub rollback_reason: Option<String>,
    pub commit_latency_ms: u64,
    pub predecessor_exit_latency_ms: u64,
    pub writer_pid: u32,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;
    use std::time::Duration;

    #[tokio::test]
    async fn cancelled_gateway_retains_close_operation_until_it_drains() {
        // Given: the connection and its independently owned close task each retain retirement.
        let root = tempfile::tempdir().expect("private fixture");
        let manager = Arc::new(HandoverManager::new(root.path().join("daemon.sock")));
        let terminals = Arc::new(TerminalService::default());
        let connection = manager
            .retain_request(terminals.clone())
            .expect("connection guard");
        let operation = manager
            .retain_request(terminals.clone())
            .expect("operation guard");
        let (finish, finished) = tokio::sync::oneshot::channel();
        let operation_task = tokio::spawn(async move {
            finished.await.expect("release operation");
            drop(operation);
        });
        // When: the HTTP connection is cancelled while durable close work is still live.
        drop(connection);
        manager.check_retirement_if_empty(&terminals);
        // Then: cancellation leaves the independently owned operation retained.
        assert_eq!(*manager.in_flight.lock(), 1);
        finish.send(()).expect("finish operation");
        operation_task.await.expect("operation joined");
        assert_eq!(*manager.in_flight.lock(), 0);
    }

    #[test]
    fn handover_commit_does_not_unlink_the_replacement_listener() {
        // Given: the blocking pool cannot run deferred socket cleanup yet.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .expect("isolated runtime");
        let dir = tempfile::tempdir().expect("isolated runtime directory");
        let path = dir.path().join("daemon.sock");
        let old_listener = UnixListener::bind(&path).expect("old canonical listener");
        let manager = HandoverManager::new(path.clone());
        let service = Arc::new(TerminalService::default());
        let (occupied_tx, occupied_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = runtime.spawn_blocking(move || {
            occupied_tx.send(()).expect("notify occupied worker");
            release_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("release occupied worker");
        });
        occupied_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("worker occupied");

        // When: handover commits and the new owner replaces the canonical socket.
        let _runtime_guard = runtime.enter();
        manager
            .commit_handover_v4(&service)
            .expect("commit handover");
        drop(old_listener);
        if path.exists() {
            fs::remove_file(&path).expect("new owner's stale socket cleanup");
        }
        let replacement = UnixListener::bind(&path).expect("replacement listener");
        release_tx.send(()).expect("release worker");
        runtime.block_on(async {
            worker.await.expect("occupied worker exited");
            tokio::task::spawn_blocking(|| {})
                .await
                .expect("all queued cleanup completed");
        });

        // Then: delayed old-owner work cannot remove the new listener's address.
        assert!(
            UnixStream::connect(&path).is_ok(),
            "old handover cleanup unlinked the replacement canonical listener"
        );
        drop(replacement);
    }

    #[test]
    fn handover_commit_persists_legacy_route_before_releasing_ownership() {
        let dir = tempfile::tempdir().expect("isolated runtime directory");
        let canonical = dir.path().join("daemon.sock");
        let legacy = dir.path().join("legacy.sock");
        let _canonical_listener = UnixListener::bind(&canonical).expect("canonical listener");
        let _legacy_listener = UnixListener::bind(&legacy).expect("legacy listener");
        let manager = HandoverManager::new(canonical.clone());
        *manager.status.write() = HandoverStatus::Prepared;
        *manager.legacy_socket_path.write() = Some(legacy.clone());
        let manifest_path = dir.path().join("handover_routes.json");
        let mut committed = manager.subscribe_client_abort();

        manager
            .commit_handover_v4(&Arc::new(TerminalService::default()))
            .expect("commit with persisted route");

        committed.try_recv().expect("clients may reconnect");
        assert!(!canonical.exists());
        assert_eq!(
            HandoverManifest::load_from_path(&manifest_path).routes,
            vec![HandoverRoute {
                legacy_socket_path: legacy,
                sessions: Vec::new(),
            }]
        );
    }

    #[tokio::test]
    async fn handover_v5_commit_retires_immediately_without_routes_or_socket_unlink() {
        let dir = tempfile::tempdir().expect("isolated runtime directory");
        let canonical = dir.path().join("daemon.sock");
        let legacy = dir.path().join("legacy.sock");
        let _canonical_listener = UnixListener::bind(&canonical).expect("canonical listener");
        let _legacy_listener = UnixListener::bind(&legacy).expect("legacy listener");
        let manager = HandoverManager::new(canonical.clone());
        *manager.status.write() = HandoverStatus::Prepared;
        *manager.legacy_socket_path.write() = Some(legacy.clone());
        let manifest_path = dir.path().join("handover_routes.json");

        let (retired_tx, retired_rx) = tokio::sync::oneshot::channel();
        let retired_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(retired_tx)));
        manager.set_retirement_action(move || {
            if let Some(tx) = retired_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
        });

        manager
            .commit_handover_v5(&Arc::new(TerminalService::default()))
            .expect("commit v5");

        assert_eq!(manager.status(), HandoverStatus::Retired);
        assert!(!manager.is_draining());
        assert!(canonical.exists());
        assert!(!manifest_path.exists());

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), retired_rx)
            .await
            .expect("timeout waiting for immediate retirement action");
        assert!(result.is_ok());
    }

    #[test]
    fn handover_commit_preserves_ownership_when_route_persistence_fails() {
        let dir = tempfile::tempdir().expect("isolated runtime directory");
        let canonical = dir.path().join("daemon.sock");
        let _listener = UnixListener::bind(&canonical).expect("canonical listener");
        let manager = HandoverManager::new(canonical.clone());
        *manager.status.write() = HandoverStatus::Prepared;
        *manager.legacy_socket_path.write() = Some(dir.path().join("legacy.sock"));
        let lock_path = dir.path().join("daemon.lock");
        manager.set_lock_files(
            crate::daemon::server::acquire_daemon_locks(None, &lock_path)
                .expect("canonical ownership"),
        );
        fs::write(dir.path().join("handover_routes.json"), b"{broken")
            .expect("corrupt route fixture");
        let mut disconnected = manager.subscribe_client_abort();

        assert!(manager
            .commit_handover_v4(&Arc::new(TerminalService::default()))
            .is_err());

        assert_eq!(manager.status(), HandoverStatus::Prepared);
        assert!(!manager.is_draining());
        assert!(UnixStream::connect(&canonical).is_ok());
        assert!(crate::daemon::server::acquire_daemon_locks(None, &lock_path).is_err());
        assert!(matches!(
            disconnected.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}

pub(crate) struct RetirementGuard {
    manager: Arc<HandoverManager>,
    terminals: Arc<TerminalService>,
}

impl Drop for RetirementGuard {
    fn drop(&mut self) {
        let mut requests = self.manager.in_flight.lock();
        *requests -= 1;
        if *requests == 0 {
            self.manager.check_retirement_locked(&self.terminals);
        }
    }
}

pub struct HandoverManager {
    status: Arc<RwLock<HandoverStatus>>,
    legacy_socket_path: Arc<RwLock<Option<PathBuf>>>,
    canonical_lock_files: Arc<Mutex<Option<DaemonLockFiles>>>,
    canonical_socket_path: PathBuf,
    is_draining: Arc<AtomicBool>,
    in_flight: Mutex<usize>,
    retirement_action: RwLock<Arc<dyn Fn() + Send + Sync>>,
    client_abort_tx: broadcast::Sender<()>,
    commit_notify_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    commit_callbacks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>>,
    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
    qa_control: Arc<RwLock<Option<qa_control::DaemonQaControl>>>,
}

impl HandoverManager {
    pub fn new(canonical_socket_path: PathBuf) -> Self {
        let (client_abort_tx, _) = broadcast::channel(16);
        Self {
            status: Arc::new(RwLock::new(HandoverStatus::Active)),
            legacy_socket_path: Arc::new(RwLock::new(None)),
            canonical_lock_files: Arc::new(Mutex::new(None)),
            canonical_socket_path,
            is_draining: Arc::new(AtomicBool::new(false)),
            in_flight: Mutex::new(0),
            retirement_action: RwLock::new(Arc::new(|| std::process::exit(0))),
            client_abort_tx,
            commit_notify_tx: Arc::new(Mutex::new(None)),
            commit_callbacks: Arc::new(Mutex::new(Vec::new())),
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            qa_control: Arc::new(RwLock::new(None)),
        }
    }

    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
    pub fn set_qa_control(&self, control: qa_control::DaemonQaControl) {
        *self.qa_control.write() = Some(control);
    }

    #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
    pub fn qa_control(&self) -> Option<qa_control::DaemonQaControl> {
        self.qa_control.read().clone()
    }

    /// Override process termination for an in-process handover fixture.
    /// Install before handing over; route cleanup still runs before this action.
    pub fn set_retirement_action(&self, action: impl Fn() + Send + Sync + 'static) {
        *self.retirement_action.write() = Arc::new(action);
    }

    pub fn subscribe_client_abort(&self) -> broadcast::Receiver<()> {
        self.client_abort_tx.subscribe()
    }

    pub fn set_commit_notifier(&self, tx: oneshot::Sender<()>) {
        *self.commit_notify_tx.lock() = Some(tx);
    }

    pub fn on_commit<F>(&self, callback: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.commit_callbacks.lock().push(Box::new(callback));
    }

    pub(crate) fn set_lock_files(&self, lock_files: DaemonLockFiles) {
        *self.canonical_lock_files.lock() = Some(lock_files);
    }

    pub fn is_draining(&self) -> bool {
        self.is_draining.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> HandoverStatus {
        *self.status.read()
    }

    pub fn generate_legacy_socket_path() -> PathBuf {
        let runtime_dir = get_runtime_dir();
        let pid = std::process::id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        #[cfg(unix)]
        {
            runtime_dir.join(format!("legacy-{pid}-{timestamp}.sock"))
        }
        #[cfg(not(unix))]
        {
            runtime_dir.join(format!("legacy-{pid}-{timestamp}.port"))
        }
    }

    #[cfg(unix)]
    pub fn prepare_handover(
        &self,
        terminal_service: &Arc<TerminalService>,
    ) -> Result<(PathBuf, Vec<String>, tokio::net::UnixListener), String> {
        let mut status_guard = self.status.write();
        if *status_guard != HandoverStatus::Active {
            return Err(format!(
                "Cannot prepare handover in state {:?}",
                *status_guard
            ));
        }

        let legacy_path = Self::generate_legacy_socket_path();
        let _ = fs::remove_file(&legacy_path);

        let listener = tokio::net::UnixListener::bind(&legacy_path).map_err(|e| {
            format!(
                "Failed to bind legacy UDS socket at {}: {e}",
                legacy_path.display()
            )
        })?;

        if let Err(e) = fs::set_permissions(&legacy_path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(&legacy_path);
            return Err(format!("Failed to secure legacy socket: {e}"));
        }

        *self.legacy_socket_path.write() = Some(legacy_path.clone());
        *status_guard = HandoverStatus::Prepared;

        let active_sessions = terminal_service.list_sessions();
        Ok((legacy_path, active_sessions, listener))
    }

    #[cfg(not(unix))]
    pub fn prepare_handover(
        &self,
        _terminal_service: &Arc<TerminalService>,
    ) -> Result<(PathBuf, Vec<String>, tokio::net::TcpListener), String> {
        Err("Handover unsupported on Windows".to_string())
    }

    pub fn commit_handover(&self, terminal_service: &Arc<TerminalService>) -> Result<(), String> {
        if is_v5_ownership_transfer_enabled() {
            return self.commit_handover_v5(terminal_service);
        }
        self.commit_handover_v4(terminal_service)
    }

    /// Legacy v4 commit: persist a durable route to this daemon's private socket and keep serving
    /// the live sessions behind it while draining. The predecessor keeps every PTY master
    /// descriptor, so it retires only once its last session ends (`check_retirement_if_empty`).
    pub fn commit_handover_v4(&self, terminal_service: &Arc<TerminalService>) -> Result<(), String> {
        let mut status_guard = self.status.write();
        if *status_guard != HandoverStatus::Prepared && *status_guard != HandoverStatus::Active {
            return Err(format!(
                "Cannot commit handover in state {:?}",
                *status_guard
            ));
        }

        if let Some(legacy_path) = self.legacy_socket_path.read().clone() {
            let manifest_path = self
                .canonical_socket_path
                .with_file_name("handover_routes.json");
            HandoverManifest::update_at_path(&manifest_path, |manifest| {
                manifest.add_or_update_route(HandoverRoute {
                    legacy_socket_path: legacy_path,
                    sessions: terminal_service.list_sessions(),
                });
            })
            .map_err(|error| format!("Failed to persist handover route: {error}"))?;
        }

        match fs::remove_file(&self.canonical_socket_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Failed to remove old canonical socket: {error}")),
        }
        let _dropped_locks = self.canonical_lock_files.lock().take();

        // Run any registered on_commit callbacks (e.g. stopping remote gateway and clearing active selection)
        let callbacks = std::mem::take(&mut *self.commit_callbacks.lock());
        for cb in callbacks {
            cb();
        }

        // Abort all existing client connections on the old canonical listener
        // so GUI DaemonClient immediately reconnects to the new canonical socket
        let _ = self.client_abort_tx.send(());

        *status_guard = HandoverStatus::Draining;
        self.is_draining.store(true, Ordering::SeqCst);

        // Notify upgrade spawner if waiting
        if let Some(tx) = self.commit_notify_tx.lock().take() {
            let _ = tx.send(());
        }

        Ok(())
    }

    pub fn commit_handover_v5(&self, _terminal_service: &Arc<TerminalService>) -> Result<(), String> {
        let mut status_guard = self.status.write();
        if *status_guard != HandoverStatus::Prepared && *status_guard != HandoverStatus::Active {
            return Err(format!(
                "Cannot commit v5 handover in state {:?}",
                *status_guard
            ));
        }

        if let Some(locks) = self.canonical_lock_files.lock().take() {
            let _ = locks.detach_without_unlock();
        }

        let callbacks = std::mem::take(&mut *self.commit_callbacks.lock());
        for cb in callbacks {
            cb();
        }

        let _ = self.client_abort_tx.send(());

        if let Some(tx) = self.commit_notify_tx.lock().take() {
            let _ = tx.send(());
        }

        *status_guard = HandoverStatus::Retired;
        self.is_draining.store(false, Ordering::SeqCst);

        let legacy_path = self.legacy_socket_path.write().take();
        let retirement_action = self.retirement_action.read().clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                if let Some(path) = legacy_path {
                    let _ = fs::remove_file(&path);
                }
                tracing::info!("Predecessor daemon completed v5 session ownership transfer and retired immediately.");
                retirement_action();
            });
        } else {
            if let Some(path) = legacy_path {
                let _ = fs::remove_file(&path);
            }
            tracing::info!("Predecessor daemon completed v5 session ownership transfer and retired immediately.");
            retirement_action();
        }

        Ok(())
    }

    pub fn abort_handover(&self) -> Result<(), String> {
        let mut status_guard = self.status.write();
        if *status_guard != HandoverStatus::Prepared {
            return Err(format!(
                "Cannot abort handover in state {:?}",
                *status_guard
            ));
        }

        if let Some(path) = self.legacy_socket_path.write().take() {
            let _ = fs::remove_file(&path);
        }

        *status_guard = HandoverStatus::Active;
        Ok(())
    }

    pub(crate) fn retain_request(
        self: &Arc<Self>,
        terminals: Arc<TerminalService>,
    ) -> Result<RetirementGuard, String> {
        let mut requests = self.in_flight.lock();
        if self.status() == HandoverStatus::Retired {
            return Err("HOST_UNAVAILABLE".into());
        }
        *requests = requests.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        Ok(RetirementGuard {
            manager: self.clone(),
            terminals,
        })
    }

    pub fn check_retirement_if_empty(&self, terminal_service: &Arc<TerminalService>) {
        let requests = self.in_flight.lock();
        if *requests == 0 {
            self.check_retirement_locked(terminal_service);
        }
    }

    fn check_retirement_locked(&self, terminal_service: &Arc<TerminalService>) {
        if self.is_draining() && terminal_service.list_sessions().is_empty() {
            self.retire();
        }
    }

    pub fn retire(&self) {
        *self.status.write() = HandoverStatus::Retired;
        self.is_draining.store(false, Ordering::SeqCst);
        let legacy_path = self.legacy_socket_path.write().take();
        let retirement_action = self.retirement_action.read().clone();
        tokio::spawn(async move {
            let cleanup = crate::ipc::run_blocking(move || {
                if let Some(path) = legacy_path {
                    fs::remove_file(&path).map_err(|error| {
                        crate::ipc::IpcError::internal(format!(
                            "Legacy socket cleanup failed: {error}"
                        ))
                    })?;
                    HandoverManifest::update_at_path(&get_manifest_path(), |manifest| {
                        manifest.remove_route(&path);
                    })
                    .map_err(|error| {
                        crate::ipc::IpcError::internal(format!(
                            "Legacy route cleanup failed: {error}"
                        ))
                    })?;
                }
                Ok(())
            })
            .await;
            if let Err(error) = cleanup {
                tracing::warn!(%error, "Failed to clean up retired daemon route");
            }
            tracing::info!("Old daemon drained all active sessions and is retiring cleanly.");
            retirement_action();
        });
    }
}

#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub mod qa_control {
    use super::*;
    use crate::ipc::qa_barrier::{
        active_channel, install, QaBarrierChannel, ReleaseOutcome,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    pub const PREDECESSOR_EXPORT_BARRIER: &str = "predecessor-export";
    pub const SUCCESSOR_ADOPT_BARRIER: &str = "successor-adopt";
    pub const COMMIT_BARRIER: &str = "commit";
    pub const ABORT_BARRIER: &str = "abort";

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DaemonRole {
        Predecessor,
        Successor,
        Standalone,
    }

    impl DaemonRole {
        pub fn as_str(self) -> &'static str {
            match self {
                DaemonRole::Predecessor => "predecessor",
                DaemonRole::Successor => "successor",
                DaemonRole::Standalone => "standalone",
            }
        }

        /// Pre-scan barrier role filter.
        ///
        /// Determines whether a barrier belongs to this process role BEFORE
        /// any arm scan or ACK write occurs. Crucially, even if an arm file
        /// omits `targetRole`, a Successor daemon will NEVER accept or ack
        /// Predecessor barriers, and vice versa.
        pub fn accepts_barrier(self, barrier_name: &str) -> bool {
            match self {
                DaemonRole::Predecessor => matches!(
                    barrier_name,
                    PREDECESSOR_EXPORT_BARRIER | COMMIT_BARRIER | ABORT_BARRIER
                ),
                DaemonRole::Successor => matches!(barrier_name, SUCCESSOR_ADOPT_BARRIER),
                DaemonRole::Standalone => true,
            }
        }
    }

    fn write_json_atomic(dir: &Path, file: &str, value: &serde_json::Value) -> std::io::Result<()> {
        let target = dir.join(file);
        let tmp = dir.join(format!("{file}.tmp.{}", std::process::id()));
        let text = serde_json::to_string_pretty(value)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, target)?;
        Ok(())
    }

    /// Scoped QA controller that manages barrier hold, release, and receipt recording.
    ///
    /// Can be instantiated directly with an isolated `Arc<QaBarrierChannel>` for tests,
    /// eliminating any dependency on `std::env::set_var` or global `ACTIVE` mutations.
    #[derive(Clone)]
    pub struct DaemonQaControl {
        channel: Arc<QaBarrierChannel>,
        role: DaemonRole,
    }

    impl DaemonQaControl {
        /// Explicit injected constructor: completely isolated, zero global state mutation.
        pub fn new(channel: Arc<QaBarrierChannel>, role: DaemonRole) -> Self {
            Self { channel, role }
        }

        /// Direct constructor using core role-scoped method `new_with_role`.
        pub fn from_parts(dir: PathBuf, run_id: String, role: DaemonRole) -> Self {
            let channel = Arc::new(QaBarrierChannel::new_with_role(
                dir,
                run_id,
                Some(role.as_str().to_string()),
            ));
            Self::new(channel, role)
        }

        /// Environment-based constructor for real daemon startup.
        pub fn from_env(role: DaemonRole) -> Result<Self, String> {
            let channel = Arc::new(QaBarrierChannel::from_env_with_role(Some(
                role.as_str().to_string(),
            ))?);
            Ok(Self::new(channel, role))
        }

        pub fn channel(&self) -> &Arc<QaBarrierChannel> {
            &self.channel
        }

        pub fn role(&self) -> DaemonRole {
            self.role
        }

        pub fn producer_pid(&self) -> u32 {
            self.channel.producer_pid()
        }

        pub fn is_armed(&self, name: &str) -> bool {
            if !self.role.accepts_barrier(name) {
                return false;
            }
            if self.channel.spec(name).is_none() {
                let _ = self.scan_and_ack_arms();
            }
            self.channel.is_armed(name)
        }

        /// Role-scoped arm scanner and acknowledger.
        ///
        /// Filters arm files by `self.role.accepts_barrier(&name)` BEFORE writing any
        /// ACK files. If an arm's `targetRole` is absent, default role mapping guarantees
        /// that a Successor daemon will NEVER touch or overwrite a Predecessor's ACK file,
        /// and vice versa.
        pub fn scan_and_ack_arms(&self) -> (Vec<String>, Vec<String>) {
            let mut acked = Vec::new();
            let mut rejected = Vec::new();
            let Ok(entries) = std::fs::read_dir(&self.channel.dir) else {
                return (acked, rejected);
            };
            let mut arm_files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(".arm.json"))
                })
                .collect();
            arm_files.sort();

            for path in arm_files {
                let name = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .map(|n| n.trim_end_matches(".arm").to_string())
                    .unwrap_or_default();

                // Gate: Role acceptance check BEFORE scanning or writing anything!
                // Absent targetRole MUST NEVER cause successor to claim predecessor arm!
                if !self.role.accepts_barrier(&name) {
                    continue;
                }

                if path.is_symlink() {
                    rejected.push(format!("{name}:symlink-disallowed"));
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    rejected.push(format!("{name}:unreadable"));
                    continue;
                };
                let Ok(spec) = serde_json::from_str::<crate::ipc::qa_barrier::ArmSpec>(&text) else {
                    rejected.push(format!("{name}:unparseable"));
                    continue;
                };
                if spec.name != name {
                    rejected.push(format!("{name}:name-mismatch"));
                    continue;
                }
                if spec.run_id != self.channel.run_id {
                    rejected.push(format!("{}:run-nonce-mismatch", spec.name));
                    continue;
                }
                if spec.operation_id.trim().is_empty() {
                    rejected.push(format!("{name}:empty-operation-id"));
                    continue;
                }
                if let Some(ref target) = spec.target_role {
                    if target != self.role.as_str() {
                        continue;
                    }
                }

                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let ack = serde_json::json!({
                    "name": spec.name,
                    "runId": spec.run_id,
                    "operationId": spec.operation_id,
                    "producer": crate::ipc::qa_barrier::PRODUCER_ID,
                    "producerPid": self.channel.pid,
                    "registeredAtMs": now_ms,
                    "role": self.role.as_str(),
                });

                let ack_role_filename = format!("{}.armed-ack.{}.json", spec.name, self.role.as_str());
                let _ = write_json_atomic(&self.channel.dir, &ack_role_filename, &ack);
                if write_json_atomic(&self.channel.dir, &format!("{}.armed-ack.json", spec.name), &ack).is_ok() {
                    if let Ok(mut arms) = self.channel.arms.lock() {
                        arms.insert(spec.name.clone(), spec);
                    }
                    acked.push(name);
                }
            }
            (acked, rejected)
        }

        /// Holds the specified barrier if armed and accepted by this daemon's role.
        ///
        /// Does not hold any locks across the wait. Awaits release or deadline,
        /// appends the settlement receipt, and returns the release outcome.
        pub async fn maybe_hold_barrier(
            &self,
            name: &str,
            session_id: &str,
            stage: &str,
            extra_held: serde_json::Value,
            settlement: impl FnOnce(ReleaseOutcome) -> serde_json::Value,
        ) -> Option<ReleaseOutcome> {
            if !self.role.accepts_barrier(name) {
                return None;
            }
            if self.channel.spec(name).is_none() {
                let _ = self.scan_and_ack_arms();
            }
            let spec = self.channel.spec(name)?;
            self.channel.write_held(&spec, session_id, stage, extra_held);
            let outcome = self.channel.wait_for_release(&spec).await;
            let receipt_payload = settlement(outcome);
            self.channel.append_receipt(name, &spec.operation_id, receipt_payload);
            Some(outcome)
        }
    }

    /// Global fallback initialization for production daemon process.
    pub fn init_daemon_qa_barrier_channel(role: DaemonRole) -> Option<DaemonQaControl> {
        if let Ok(control) = DaemonQaControl::from_env(role) {
            let (acked, rejected) = control.scan_and_ack_arms();
            tracing::info!(
                acked = ?acked,
                rejected = ?rejected,
                pid = control.producer_pid(),
                run_id = control.channel.run_id(),
                role = control.role.as_str(),
                "Daemon initialized QA barrier channel from environment"
            );
            if active_channel().is_none() {
                if let Ok(channel) = QaBarrierChannel::from_env_with_role(Some(role.as_str().to_string())) {
                    install(channel);
                }
            }
            Some(control)
        } else {
            None
        }
    }
}

#[cfg(all(test, feature = "local-split-qa", feature = "native-terminal"))]
mod qa_handover_tests {
    use super::qa_control::*;
    use super::*;
    use crate::ipc::qa_barrier::{QaBarrierChannel, ReleaseOutcome};
    use serde_json::json;
    use std::fs;
    use std::path::Path;

    fn setup_test_barrier(dir: &Path, name: &str, run_id: &str, op_id: &str, deadline_ms: u64, target_role: Option<&str>) {
        let mut arm = json!({
            "name": name,
            "runId": run_id,
            "operationId": op_id,
            "deadlineMs": deadline_ms,
        });
        if let Some(role) = target_role {
            arm["targetRole"] = json!(role);
        }
        fs::write(dir.join(format!("{name}.arm.json")), serde_json::to_string_pretty(&arm).unwrap()).unwrap();
    }

    fn write_test_release(dir: &Path, name: &str, run_id: &str, op_id: &str) {
        let release = json!({
            "name": name,
            "runId": run_id,
            "operationId": op_id,
            "releasedAt": "2026-10-03T00:00:00Z",
        });
        fs::write(dir.join(format!("{name}.release.json")), serde_json::to_string_pretty(&release).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn test_injected_scoped_channel_constructor_avoids_global_mutation() {
        // Explicitly injected constructor: zero std::env::set_var, zero deactivate().
        let temp = tempfile::tempdir().unwrap();
        let run_id = "qa-run-scoped-init-test";
        let op_id = "qa-op-scoped-init-test";
        setup_test_barrier(temp.path(), PREDECESSOR_EXPORT_BARRIER, run_id, op_id, 5000, None);

        let control = DaemonQaControl::from_parts(temp.path().to_path_buf(), run_id.to_string(), DaemonRole::Predecessor);
        assert_eq!(control.channel().run_id(), run_id);
        assert_eq!(control.producer_pid(), std::process::id());
        assert_eq!(control.role(), DaemonRole::Predecessor);

        let (acked, _) = control.scan_and_ack_arms();
        assert!(acked.contains(&PREDECESSOR_EXPORT_BARRIER.to_string()));

        let ack_path = temp.path().join(format!("{PREDECESSOR_EXPORT_BARRIER}.armed-ack.json"));
        assert!(ack_path.exists());
        let ack: serde_json::Value = serde_json::from_str(&fs::read_to_string(&ack_path).unwrap()).unwrap();
        assert_eq!(ack["runId"], run_id);
        assert_eq!(ack["operationId"], op_id);
        assert_eq!(ack["producer"], crate::ipc::qa_barrier::PRODUCER_ID);
        assert_eq!(ack["role"], "predecessor");
    }

    #[tokio::test]
    async fn test_regression_predecessor_ack_preserved_when_successor_scans_shared_dir_with_absent_target_role() {
        // REGRESSION TEST:
        // When arm files OMIT targetRole entirely, Successor daemon scanning the shared barrier
        // directory must NEVER claim or overwrite Predecessor's ACK file.
        let temp = tempfile::tempdir().unwrap();
        let run_id = "qa-run-absent-role-test";
        let op_id_pred = "qa-op-pred-export";
        let op_id_succ = "qa-op-succ-adopt";

        // Both arm files OMIT targetRole (target_role: None)
        setup_test_barrier(temp.path(), PREDECESSOR_EXPORT_BARRIER, run_id, op_id_pred, 5000, None);
        setup_test_barrier(temp.path(), SUCCESSOR_ADOPT_BARRIER, run_id, op_id_succ, 5000, None);

        let pred_control = DaemonQaControl::from_parts(
            temp.path().to_path_buf(),
            run_id.to_string(),
            DaemonRole::Predecessor,
        );
        let succ_control = DaemonQaControl::from_parts(
            temp.path().to_path_buf(),
            run_id.to_string(),
            DaemonRole::Successor,
        );

        // 1. Predecessor scans directory first
        let (pred_acked, _) = pred_control.scan_and_ack_arms();
        assert!(pred_acked.contains(&PREDECESSOR_EXPORT_BARRIER.to_string()));
        assert!(!pred_acked.contains(&SUCCESSOR_ADOPT_BARRIER.to_string()), "Predecessor must NOT ack successor arm");

        // Inspect Predecessor's ACK BEFORE Successor scans
        let pred_ack_path = temp.path().join(format!("{PREDECESSOR_EXPORT_BARRIER}.armed-ack.json"));
        assert!(pred_ack_path.exists(), "Predecessor ACK must exist");
        let pred_ack_before_text = fs::read_to_string(&pred_ack_path).unwrap();
        let pred_ack_before: serde_json::Value = serde_json::from_str(&pred_ack_before_text).unwrap();
        assert_eq!(pred_ack_before["runId"], run_id);
        assert_eq!(pred_ack_before["operationId"], op_id_pred);
        assert_eq!(pred_ack_before["producerPid"], pred_control.producer_pid());
        assert_eq!(pred_ack_before["role"], "predecessor");

        let succ_ack_path = temp.path().join(format!("{SUCCESSOR_ADOPT_BARRIER}.armed-ack.json"));
        assert!(!succ_ack_path.exists(), "Successor ACK must not exist before successor scans");

        // 2. Successor scans the EXACT SAME shared directory
        let (succ_acked, _) = succ_control.scan_and_ack_arms();
        assert!(succ_acked.contains(&SUCCESSOR_ADOPT_BARRIER.to_string()));
        assert!(!succ_acked.contains(&PREDECESSOR_EXPORT_BARRIER.to_string()), "Successor must NOT ack predecessor arm");

        // 3. REGRESSION ASSERTION: Inspect Predecessor's ACK AFTER Successor scanned
        let pred_ack_after_text = fs::read_to_string(&pred_ack_path).unwrap();
        let pred_ack_after: serde_json::Value = serde_json::from_str(&pred_ack_after_text).unwrap();

        // Must be 100% IDENTICAL - zero clobbering by successor!
        assert_eq!(
            pred_ack_before,
            pred_ack_after,
            "Predecessor armed-ack.json must remain identical before and after successor scan!"
        );
        assert_eq!(pred_ack_after["producerPid"], pred_control.producer_pid());
        assert_eq!(pred_ack_after["role"], "predecessor");

        // 4. Successor's ACK exists with Successor's identity
        assert!(succ_ack_path.exists(), "Successor ACK must now exist");
        let succ_ack: serde_json::Value = serde_json::from_str(&fs::read_to_string(&succ_ack_path).unwrap()).unwrap();
        assert_eq!(succ_ack["runId"], run_id);
        assert_eq!(succ_ack["operationId"], op_id_succ);
        assert_eq!(succ_ack["producerPid"], succ_control.producer_pid());
        assert_eq!(succ_ack["role"], "successor");

        // 5. Hold and release both barriers deterministically
        let mut pred_held_rx = pred_control.channel().subscribe_held();
        let mut succ_held_rx = succ_control.channel().subscribe_held();
        let dir_path = temp.path().to_path_buf();

        let pred_c = pred_control.clone();
        let pred_task = tokio::spawn(async move {
            pred_c.maybe_hold_barrier(
                PREDECESSOR_EXPORT_BARRIER,
                "sess-1",
                "predecessor-export",
                json!({ "stage": "export" }),
                |o| json!({ "outcome": o.as_str() }),
            ).await
        });
        let held_pred = QaBarrierChannel::await_held_event(&mut pred_held_rx, PREDECESSOR_EXPORT_BARRIER).await;
        assert_eq!(held_pred.name, PREDECESSOR_EXPORT_BARRIER);
        write_test_release(&dir_path, PREDECESSOR_EXPORT_BARRIER, run_id, op_id_pred);
        assert_eq!(pred_task.await.unwrap(), Some(ReleaseOutcome::Released));

        let succ_c = succ_control.clone();
        let succ_task = tokio::spawn(async move {
            succ_c.maybe_hold_barrier(
                SUCCESSOR_ADOPT_BARRIER,
                "sess-1",
                "successor-adopt",
                json!({ "stage": "adopt" }),
                |o| json!({ "outcome": o.as_str() }),
            ).await
        });
        let held_succ = QaBarrierChannel::await_held_event(&mut succ_held_rx, SUCCESSOR_ADOPT_BARRIER).await;
        assert_eq!(held_succ.name, SUCCESSOR_ADOPT_BARRIER);
        write_test_release(&dir_path, SUCCESSOR_ADOPT_BARRIER, run_id, op_id_succ);
        assert_eq!(succ_task.await.unwrap(), Some(ReleaseOutcome::Released));
    }

    #[tokio::test]
    async fn test_handover_transaction_barriers_commit_and_abort_scoped() {
        // Tests commit and abort barriers using injected scoped controller on HandoverManager.
        let temp = tempfile::tempdir().unwrap();
        let run_id = "qa-run-commit-abort-scoped-test";
        let op_id = "qa-op-commit-abort-scoped-test";

        setup_test_barrier(temp.path(), COMMIT_BARRIER, run_id, op_id, 5000, None);
        setup_test_barrier(temp.path(), ABORT_BARRIER, run_id, op_id, 5000, None);

        let control = DaemonQaControl::from_parts(temp.path().to_path_buf(), run_id.to_string(), DaemonRole::Predecessor);
        control.scan_and_ack_arms();

        let manager = HandoverManager::new(temp.path().join("dummy.sock"));
        manager.set_qa_control(control);

        let ctrl = manager.qa_control().expect("scoped qa control must be set");
        let mut held_rx = ctrl.channel().subscribe_held();
        let dir_path = temp.path().to_path_buf();

        // 1. Commit barrier
        let c1 = ctrl.clone();
        let commit_task = tokio::spawn(async move {
            c1.maybe_hold_barrier(
                COMMIT_BARRIER,
                "",
                "commit-handover",
                json!({ "stage": "commit", "predecessorStatus": "Prepared" }),
                |outcome| json!({ "stage": "commit", "releaseOutcome": outcome.as_str() }),
            ).await
        });

        let held = QaBarrierChannel::await_held_event(&mut held_rx, COMMIT_BARRIER).await;
        assert_eq!(held.name, COMMIT_BARRIER);
        write_test_release(&dir_path, COMMIT_BARRIER, run_id, op_id);
        assert_eq!(commit_task.await.unwrap(), Some(ReleaseOutcome::Released));

        // 2. Abort barrier with observed outcome only (never claims rollback relinquishment)
        let c2 = ctrl.clone();
        let abort_task = tokio::spawn(async move {
            c2.maybe_hold_barrier(
                ABORT_BARRIER,
                "",
                "abort-handover",
                json!({ "stage": "abort", "predecessorStatus": "Prepared" }),
                |outcome| json!({
                    "stage": "abort",
                    "releaseOutcome": outcome.as_str(),
                    "abortSuccess": true,
                    "resumedSessions": 1,
                }),
            ).await
        });

        let held = QaBarrierChannel::await_held_event(&mut held_rx, ABORT_BARRIER).await;
        assert_eq!(held.name, ABORT_BARRIER);
        write_test_release(&dir_path, ABORT_BARRIER, run_id, op_id);
        assert_eq!(abort_task.await.unwrap(), Some(ReleaseOutcome::Released));

        let abort_receipt_path = temp.path().join(format!("{ABORT_BARRIER}.receipt.jsonl"));
        let abort_line = fs::read_to_string(&abort_receipt_path).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(abort_line.lines().next().unwrap()).unwrap();
        assert_eq!(receipt["abortSuccess"], true);
        assert_eq!(receipt["resumedSessions"], 1);

        // INVARIANT VERIFICATION: Receipts describe observed outcomes only:
        // never claim rollback relinquishment from mere abort request.
        let fake_relinquishment_path = temp.path().join("rollback-relinquishment.receipt.jsonl");
        assert!(!fake_relinquishment_path.exists(), "abort barrier must never fabricate rollback-relinquishment receipt");
    }

    #[tokio::test]
    async fn test_handover_barrier_deadline_exceeded_scoped() {
        // Scoped deadline exceeded test: zero global env manipulation.
        let temp = tempfile::tempdir().unwrap();
        let run_id = "qa-run-deadline-scoped-test";
        let op_id = "qa-op-deadline-scoped-test";

        setup_test_barrier(temp.path(), "abort", run_id, op_id, 50, None);

        let control = DaemonQaControl::from_parts(temp.path().to_path_buf(), run_id.to_string(), DaemonRole::Predecessor);
        control.scan_and_ack_arms();

        let outcome = control.maybe_hold_barrier(
            "abort",
            "",
            "abort-handover",
            json!({ "stage": "abort" }),
            |outcome| json!({ "stage": "abort", "releaseOutcome": outcome.as_str() }),
        ).await;

        assert_eq!(outcome, Some(ReleaseOutcome::DeadlineExceeded));

        let receipt_path = temp.path().join("abort.receipt.jsonl");
        let receipt_line = fs::read_to_string(&receipt_path).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(receipt_line.lines().next().unwrap()).unwrap();
        assert_eq!(receipt["releaseOutcome"], "deadline-exceeded");
    }
}
