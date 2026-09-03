use crate::daemon::manifest::{get_manifest_path, HandoverManifest};
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

    pub fn commit_handover(&self, _terminal_service: &Arc<TerminalService>) -> Result<(), String> {
        let mut status_guard = self.status.write();
        if *status_guard != HandoverStatus::Prepared && *status_guard != HandoverStatus::Active {
            return Err(format!(
                "Cannot commit handover in state {:?}",
                *status_guard
            ));
        }

        // Release canonical locks so new daemon can acquire them immediately
        let _dropped_locks = self.canonical_lock_files.lock().take();

        // Unlink canonical socket so new daemon can bind it cleanly
        let sock = self.canonical_socket_path.clone();
        tokio::task::spawn_blocking(move || {
            let _ = fs::remove_file(&sock);
        });

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
        if let Some(path) = self.legacy_socket_path.write().take() {
            let manifest_path = get_manifest_path();
            let _ = fs::remove_file(&path);
            let mut manifest = HandoverManifest::load_from_path(&manifest_path);
            manifest.remove_route(&path);
            let _ = manifest.save_to_path(&manifest_path);
        }
        tracing::info!("Old daemon drained all active sessions and is retiring cleanly.");
        std::process::exit(0);
    }
}
