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
pub enum HandoverStatus {
    Active,
    Prepared,
    Draining,
    Retired,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;
    use std::time::Duration;

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
        manager.commit_handover(&service).expect("commit handover");
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
            .commit_handover(&Arc::new(TerminalService::default()))
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
            .commit_handover(&Arc::new(TerminalService::default()))
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

pub struct HandoverManager {
    status: Arc<RwLock<HandoverStatus>>,
    legacy_socket_path: Arc<RwLock<Option<PathBuf>>>,
    canonical_lock_files: Arc<Mutex<Option<DaemonLockFiles>>>,
    canonical_socket_path: PathBuf,
    is_draining: Arc<AtomicBool>,
    client_abort_tx: broadcast::Sender<()>,
    commit_notify_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    commit_callbacks: Arc<Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>>,
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
            client_abort_tx,
            commit_notify_tx: Arc::new(Mutex::new(None)),
            commit_callbacks: Arc::new(Mutex::new(Vec::new())),
        }
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

    pub fn check_retirement_if_empty(&self, terminal_service: &Arc<TerminalService>) {
        if self.is_draining() && terminal_service.list_sessions().is_empty() {
            self.retire();
        }
    }

    pub fn retire(&self) {
        *self.status.write() = HandoverStatus::Retired;
        self.is_draining.store(false, Ordering::SeqCst);
        let legacy_path = self.legacy_socket_path.write().take();
        tokio::spawn(async move {
            let cleanup = crate::ipc::run_blocking(move || {
                if let Some(path) = legacy_path {
                    fs::remove_file(&path).map_err(|error| {
                        crate::ipc::IpcError::internal(format!("Legacy socket cleanup failed: {error}"))
                    })?;
                    HandoverManifest::update_at_path(&get_manifest_path(), |manifest| {
                        manifest.remove_route(&path);
                    })
                    .map_err(|error| {
                        crate::ipc::IpcError::internal(format!("Legacy route cleanup failed: {error}"))
                    })?;
                }
                Ok(())
            })
            .await;
            if let Err(error) = cleanup {
                tracing::warn!(%error, "Failed to clean up retired daemon route");
            }
            tracing::info!("Old daemon drained all active sessions and is retiring cleanly.");
            std::process::exit(0);
        });
    }
}
