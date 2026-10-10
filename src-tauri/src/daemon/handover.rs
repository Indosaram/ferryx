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

/// Stable prefix of every refusal produced by the spawn/handover gate: the token
/// `retain_spawn_owner` returns to a spawn that arrives while a handover is prepared, and
/// the token `prepare_handover`, `commit_handover_v4` and `commit_handover_v5` return
/// while a spawn is in flight.
///
/// The token is the caller's retry signal, so it is part of the contract: the gate reopens
/// as soon as the last in-flight spawn's `SpawnOwnerGuard` drops, and a caller that
/// receives the refusal must retry rather than read it as a structural handover failure.
/// Every refusal also names the guard identity (the handover status, or the in-flight
/// spawn count), so a blocked handover is observable instead of a bare token.
pub(crate) const HANDOVER_BUSY: &str = "HANDOVER_BUSY";

/// True when `message` is the retryable spawn-gate refusal. Only this module produces the
/// token, so the check cannot misclassify another handover failure.
pub(crate) fn is_spawn_gate_busy(message: &str) -> bool {
    message.starts_with(HANDOVER_BUSY)
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

pub(crate) struct SpawnOwnerGuard {
    manager: Arc<HandoverManager>,
}

impl Drop for SpawnOwnerGuard {
    fn drop(&mut self) {
        let _status = self.manager.status.write();
        self.manager.spawn_owners.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod spawn_owner_tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn local_split_reliability_handover_private_prepare_after_publication() {
        let root = tempfile::tempdir().unwrap();
        let canonical = root.path().join("private.sock");
        let manager = Arc::new(HandoverManager::new(canonical.clone()));
        let terminals = Arc::new(TerminalService::default());
        let owner = manager.retain_spawn_owner().unwrap();
        // F2-1 (`ecf80277`) made every spawn-gate refusal a typed, retryable busy token that
        // ALSO names the guard, so the bare-token equality this test predates is stale:
        // assert the token and the guard it names, as the F2 contract states.
        let refusal = manager.prepare_handover(&terminals).unwrap_err();
        assert!(is_spawn_gate_busy(&refusal), "{refusal}");
        assert!(refusal.contains("1 in-flight spawn"), "{refusal}");
        assert_eq!(manager.status(), HandoverStatus::Active);
        drop(owner);
        // `prepare_handover` allocates the legacy socket inside the daemon runtime directory
        // (`get_runtime_dir()`), never inside this fixture's canonical socket directory, so the
        // containment target is that runtime directory rather than `root`.
        let runtime_dir = get_runtime_dir();
        fs::create_dir_all(&runtime_dir).expect("daemon runtime directory");
        let (legacy, sessions, listener) = manager.prepare_handover(&terminals).unwrap();
        assert!(
            legacy.starts_with(&runtime_dir),
            "legacy socket {} must live inside the daemon runtime directory {}",
            legacy.display(),
            runtime_dir.display()
        );
        assert!(legacy
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("legacy-")));
        assert!(sessions.is_empty());
        assert!(manager.retain_spawn_owner().is_err());
        manager.abort_handover().unwrap();
        drop(listener);
        assert!(!legacy.exists());
        assert!(manager.retain_spawn_owner().is_ok());
    }

    #[test]
    fn local_split_reliability_handover_prepared_owner_and_abort() {
        let root = tempfile::tempdir().unwrap();
        let manager = Arc::new(HandoverManager::new(root.path().join("private.sock")));
        let owner = manager.retain_spawn_owner().unwrap();
        let terminals = Arc::new(TerminalService::default());
        // F2-1 (`ecf80277`): the refusal is the typed, retryable busy token naming the
        // in-flight spawn, not the bare `HANDOVER_BUSY` this test predates.
        let refusal = manager.commit_handover_v5(&terminals).unwrap_err();
        assert!(is_spawn_gate_busy(&refusal), "{refusal}");
        assert!(refusal.contains("1 in-flight spawn"), "{refusal}");
        drop(owner);
        *manager.status.write() = HandoverStatus::Prepared;
        // The same typed token covers the other guard: a prepared handover refuses a new
        // spawn, and the refusal names the status it is in. (`SpawnOwnerGuard` is not
        // `Debug`, so this matches rather than calling `unwrap_err`.)
        let prepared_refusal = match manager.retain_spawn_owner() {
            Ok(_) => panic!("a prepared handover must not admit a new spawn owner"),
            Err(refusal) => refusal,
        };
        assert!(is_spawn_gate_busy(&prepared_refusal), "{prepared_refusal}");
        assert!(prepared_refusal.contains("Prepared"), "{prepared_refusal}");
        manager.abort_handover().unwrap();
        assert!(manager.retain_spawn_owner().is_ok());
    }

    /// F2 retry contract. A handover blocked by an in-flight spawn is a *retryable* busy
    /// outcome, not a structural failure: while the spawn holds the gate the refusal is the
    /// typed `HANDOVER_BUSY` outcome naming the guard, and the same handover succeeds once
    /// that spawn completes.
    #[cfg(unix)]
    #[test]
    fn a_blocked_handover_reports_busy_and_succeeds_on_retry_after_the_spawn_completes() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _runtime_guard = runtime.enter();
        let root = tempfile::tempdir().unwrap();
        let manager = Arc::new(HandoverManager::new(root.path().join("private.sock")));
        let terminals = Arc::new(TerminalService::default());

        // Given: one in-flight spawn holds the gate for its whole lifecycle.
        let spawn = manager.retain_spawn_owner().expect("spawn claims the gate");

        // When: a handover is prepared while that spawn is still in flight.
        let refusal = match manager.prepare_handover(&terminals) {
            Ok(_) => panic!("a handover must not prepare while a spawn is in flight"),
            Err(refusal) => refusal,
        };

        // Then: the refusal is the retryable spawn-gate outcome, and it names the guard.
        assert!(is_spawn_gate_busy(&refusal), "{refusal}");
        assert!(refusal.contains("1 in-flight spawn"), "{refusal}");
        assert_eq!(manager.status(), HandoverStatus::Active);

        // When: the spawn completes and the caller retries the same handover.
        drop(spawn);
        let runtime_dir = get_runtime_dir();
        fs::create_dir_all(&runtime_dir).expect("daemon runtime directory");
        let (legacy, sessions, listener) = manager
            .prepare_handover(&terminals)
            .expect("the retry succeeds once the spawn has completed");

        // Then: the retry takes the handover instead of reporting busy again.
        assert!(sessions.is_empty());
        assert!(legacy.exists());
        assert_eq!(manager.status(), HandoverStatus::Prepared);
        manager.abort_handover().unwrap();
        drop(listener);
    }

    #[test]
    fn generated_legacy_socket_paths_are_unique_and_parseable_for_predecessor_pid() {
        let pid = std::process::id();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..100 {
            let path = HandoverManager::generate_legacy_socket_path();
            assert!(
                seen.insert(path.clone()),
                "legacy path {} generated more than once",
                path.display()
            );
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .expect("valid legacy socket file name");
            assert!(file_name.starts_with(&format!("legacy-{pid}-")));
            #[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
            {
                assert_eq!(
                    crate::daemon::qa_producers::predecessor_pid_from_legacy_path(&path),
                    Some(pid)
                );
            }
        }
    }
}

pub struct HandoverManager {
    status: Arc<RwLock<HandoverStatus>>,
    spawn_owners: std::sync::atomic::AtomicUsize,
    legacy_socket_path: Arc<RwLock<Option<PathBuf>>>,
    canonical_lock_files: Arc<Mutex<Option<DaemonLockFiles>>>,
    canonical_socket_path: PathBuf,
    is_draining: Arc<AtomicBool>,
    in_flight: Mutex<usize>,
    retirement_action: RwLock<Arc<dyn Fn() + Send + Sync>>,
    client_abort_tx: broadcast::Sender<()>,
    commit_notify_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    commit_callbacks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>>,
}

impl HandoverManager {
    pub fn recorded_decision(legacy: &std::path::Path) -> Result<Option<super::handover_transaction::HandoverState>, String> {
        use super::handover_transaction::{HandoverState, HandoverTransaction};
        let bytes = match fs::read(legacy.with_extension("transaction")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let transaction: HandoverTransaction = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if transaction.transfer_id != legacy.to_string_lossy() { return Err("Handover transaction identity mismatch".into()); }
        Ok(match transaction.state {
            HandoverState::Retired => Some(HandoverState::Retired),
            HandoverState::Active if transaction.rollback_reason.is_some() => Some(HandoverState::Active),
            _ => None,
        })
    }

    fn record_decision(&self, committed: bool) -> Result<(), String> {
        use super::handover_transaction::{HandoverState, HandoverTransaction};
        let Some(legacy) = self.legacy_socket_path.read().clone() else { return Ok(()); };
        if let Some(decision) = Self::recorded_decision(&legacy)? {
            return if decision == if committed { HandoverState::Retired } else { HandoverState::Active } {
                Ok(())
            } else {
                Err("Handover transaction already has the opposite decision".into())
            };
        }
        let identity = legacy.to_string_lossy().into_owned();
        let mut transaction = HandoverTransaction::new_simple(&identity, &identity,
            std::process::id(), 0, 0, 0, &identity);
        transaction.state = if committed { HandoverState::Retired } else { HandoverState::Active };
        transaction.rollback_reason = (!committed).then(|| "predecessor abort decision".into());
        let bytes = serde_json::to_vec(&transaction).map_err(|error| error.to_string())?;
        let path = legacy.with_extension("transaction");
        let temporary = legacy.with_extension("transaction.pending");
        use std::io::Write;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|error| error.to_string())?;
        fs::rename(&temporary, &path).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        fs::File::open(path.parent().ok_or("Missing transaction parent")?)
            .and_then(|directory| directory.sync_all()).map_err(|error| error.to_string())?;
        Ok(())
    }

    /// The refusal to return while an in-flight spawn owns the handover gate, naming the
    /// guard count so the block is observable.
    ///
    /// Activation cost (deliberate, per F2-2): `retain_spawn_owner` is the only thing that
    /// ever increments `spawn_owners`, so from the first production spawn onward a long
    /// spawn -- notably `spawn_remote` waiting on a relay tunnel -- delays every handover
    /// for its whole duration. That delay is bounded by the spawn, never unbounded: the
    /// gate reopens when that spawn's guard drops, and the refusal is the typed, retryable
    /// `HANDOVER_BUSY` outcome, so a blocked prepare or commit fails cleanly and a retry
    /// after the spawn completes succeeds.
    fn spawn_gate_refusal(&self) -> Option<String> {
        let in_flight = self.spawn_owners.load(Ordering::Relaxed);
        (in_flight != 0).then(|| {
            format!("{HANDOVER_BUSY}: {in_flight} in-flight spawn(s) hold the handover gate")
        })
    }

    pub(crate) fn retain_spawn_owner(self: &Arc<Self>) -> Result<SpawnOwnerGuard, String> {
        let status = self.status.write();
        if *status != HandoverStatus::Active {
            return Err(format!(
                "{HANDOVER_BUSY}: handover is {:?} and does not accept new spawns",
                *status
            ));
        }
        self.spawn_owners.fetch_add(1, Ordering::Relaxed);
        Ok(SpawnOwnerGuard { manager: self.clone() })
    }

    pub fn new(canonical_socket_path: PathBuf) -> Self {
        let (client_abort_tx, _) = broadcast::channel(16);
        Self {
            status: Arc::new(RwLock::new(HandoverStatus::Active)),
            spawn_owners: std::sync::atomic::AtomicUsize::new(0),
            legacy_socket_path: Arc::new(RwLock::new(None)),
            canonical_lock_files: Arc::new(Mutex::new(None)),
            canonical_socket_path,
            is_draining: Arc::new(AtomicBool::new(false)),
            in_flight: Mutex::new(0),
            retirement_action: RwLock::new(Arc::new(|| std::process::exit(0))),
            client_abort_tx,
            commit_notify_tx: Arc::new(Mutex::new(None)),
            commit_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
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
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let disambiguator = if seq == 0 {
            format!("{timestamp}")
        } else {
            format!("{timestamp}-{seq}")
        };
        #[cfg(unix)]
        {
            runtime_dir.join(format!("legacy-{pid}-{disambiguator}.sock"))
        }
        #[cfg(not(unix))]
        {
            runtime_dir.join(format!("legacy-{pid}-{disambiguator}.port"))
        }
    }

    #[cfg(unix)]
    pub fn prepare_handover(
        &self,
        terminal_service: &Arc<TerminalService>,
    ) -> Result<(PathBuf, Vec<String>, tokio::net::UnixListener), String> {
        let mut status_guard = self.status.write();
        if let Some(refusal) = self.spawn_gate_refusal() {
            return Err(refusal);
        }
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
        if let Some(refusal) = self.spawn_gate_refusal() {
            return Err(refusal);
        }
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

        self.record_decision(true)?;
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
        if let Some(refusal) = self.spawn_gate_refusal() {
            return Err(refusal);
        }
        if *status_guard != HandoverStatus::Prepared && *status_guard != HandoverStatus::Active {
            return Err(format!(
                "Cannot commit v5 handover in state {:?}",
                *status_guard
            ));
        }

        self.record_decision(true)?;
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

        self.record_decision(false)?;
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
