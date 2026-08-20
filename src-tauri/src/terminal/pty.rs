use crate::terminal::{session::PtySessionConfig, PtyError, PtySession, PtySessionState, TerminalSignal};
use crate::worktree::manager::WriterLeaseGuard;
use crate::worktree::WorktreeManager;
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const LIFECYCLE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<String, Arc<PtySession>>>>,
    pty_system: Arc<Mutex<Box<dyn PtySystem + Send>>>,
}

impl Default for PtyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            pty_system: Arc::new(Mutex::new(native_pty_system())),
        }
    }

    pub fn spawn(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        let rx = self.spawn_with_id(session_id.clone(), cmd, cols, rows)?;
        Ok((session_id, rx))
    }

    pub fn spawn_shell(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let cmd = CommandBuilder::new_default_prog();
        self.spawn(cmd, cols, rows)
    }

    pub fn spawn_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        let lease = worktree_manager
            .acquire_writer_lease(worktree_path, &session_id)
            .map_err(|error| PtyError::Other(error.to_string()))?;
        let rx = self.spawn_with_id_and_lease(
            session_id.clone(),
            cmd,
            cols,
            rows,
            Some(lease),
        )?;
        Ok((session_id, rx))
    }

    pub fn spawn_with_id(
        &self,
        session_id: impl Into<String>,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        self.spawn_with_id_and_lease(session_id.into(), cmd, cols, rows, None)
    }

    fn spawn_with_id_and_lease(
        &self,
        session_id: String,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        writer_lease: Option<WriterLeaseGuard>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        if self.has_session(&session_id) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' already exists"
            )));
        }

        let pty_size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = {
            let pty_system = self.pty_system.lock();
            pty_system
                .openpty(pty_size)
                .map_err(|e| PtyError::PtyCreationError(e.to_string()))?
        };

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::IoError(format!("Failed to clone reader: {e}")))?;

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::IoError(format!("Failed to take writer: {e}")))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnError(format!("Failed to spawn command: {e}")))?;

        drop(pair.slave);

        let (tx, rx) = mpsc::channel::<Vec<u8>>(1024);
        let session = Arc::new(PtySession::new(PtySessionConfig {
            id: session_id.clone(),
            master: pair.master,
            child,
            writer,
            reader,
            cols,
            rows,
            tx,
            writer_lease,
        }));

        self.sessions
            .write()
            .insert(session_id.clone(), Arc::clone(&session));
        session.mark_running();
        self.start_lifecycle_watcher(session_id);
        Ok(rx)
    }

    fn start_lifecycle_watcher(&self, session_id: String) {
        let manager = self.clone();
        tokio::spawn(async move {
            loop {
                let Some(session) = manager.get_session(&session_id) else {
                    break;
                };

                match session.state() {
                    PtySessionState::Exited { .. } | PtySessionState::Failed { .. } => {
                        session.release_writer_lease();
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                    PtySessionState::Closing => {
                        tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
                        continue;
                    }
                    PtySessionState::Starting | PtySessionState::Running => {}
                }

                match session.poll_exit_code() {
                    Ok(Some(code)) => {
                        manager.finalize_natural_exit(&session_id, code).await;
                        break;
                    }
                    Ok(None)
                        if session.output_receiver_closed() || session.is_reader_finished() =>
                    {
                        if let Err(error) = manager.close_session(&session_id).await {
                            tracing::debug!(
                                "PTY receiver-drop cleanup failed for {}: {}",
                                session_id,
                                error
                            );
                        }
                        break;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        session.mark_failed(error.to_string());
                        session.close_io();
                        session.release_writer_lease();
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                }

                tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
            }
        });
    }

    async fn finalize_natural_exit(&self, session_id: &str, code: i32) {
        let Some(session) = self.get_session(session_id) else {
            return;
        };
        if !session.begin_closing() {
            return;
        }

        session.close_io();
        let _ = Self::join_reader_bounded(&session).await;
        session.release_writer_lease();
        session.mark_exited(Some(code));
        session.close_output();
        self.remove_from_registry(session_id);
    }

    async fn join_reader_bounded(session: &Arc<PtySession>) -> Result<(), PtyError> {
        let Some(mut reader_task) = session.take_reader_task() else {
            return Ok(());
        };

        match tokio::time::timeout(READER_SHUTDOWN_TIMEOUT, &mut reader_task).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(PtyError::Other(format!(
                "PTY reader task failed: {error}"
            ))),
            Err(_) => {
                reader_task.abort();
                Err(PtyError::Other(
                    "Timed out waiting for PTY reader shutdown".into(),
                ))
            }
        }
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), PtyError> {
        let Some(session) = self.get_session(session_id) else {
            return Ok(());
        };

        if !session.begin_closing() {
            if matches!(
                session.state(),
                PtySessionState::Exited { .. } | PtySessionState::Failed { .. }
            ) {
                session.release_writer_lease();
                session.close_output();
                self.remove_from_registry(session_id);
                return Ok(());
            }

            let deadline = tokio::time::Instant::now() + READER_SHUTDOWN_TIMEOUT;
            while tokio::time::Instant::now() < deadline {
                if !self.has_session(session_id) {
                    return Ok(());
                }
                tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
            }
            return Err(PtyError::Other(format!(
                "Timed out waiting for concurrent close of session '{session_id}'"
            )));
        }

        let mut first_error: Option<PtyError> = None;
        let mut exit_code = match session.poll_exit_code() {
            Ok(code) => code,
            Err(error) => {
                first_error = Some(error);
                None
            }
        };

        if exit_code.is_none() {
            if let Err(signal_error) = session.signal(TerminalSignal::Terminate) {
                if let Err(kill_error) = session.kill() {
                    first_error = Some(PtyError::KillError(format!(
                        "{signal_error}; fallback kill failed: {kill_error}"
                    )));
                }
            }
        }

        session.close_io();

        if let Err(reader_error) = Self::join_reader_bounded(&session).await {
            let _ = session.signal(TerminalSignal::Kill).or_else(|_| session.kill());
            if first_error.is_none() {
                first_error = Some(reader_error);
            }
        }

        if exit_code.is_none() && !session.is_reaped() {
            let session_for_wait = Arc::clone(&session);
            match tokio::task::spawn_blocking(move || session_for_wait.wait_and_reap()).await {
                Ok(Ok(code)) => exit_code = code,
                Ok(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(PtyError::Other(format!(
                            "PTY child reap task failed: {error}"
                        )));
                    }
                }
            }
        }

        session.release_writer_lease();

        if let Some(error) = first_error {
            session.mark_failed(error.to_string());
            session.close_output();
            self.remove_from_registry(session_id);
            return Err(error);
        }

        session.mark_exited(exit_code);
        session.close_output();
        self.remove_from_registry(session_id);
        Ok(())
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.write_input(data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.resize(cols, rows)
    }

    pub fn kill(&self, session_id: &str) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.kill()
    }

    pub fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        session.signal(signal)
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.read().get(session_id).cloned()
    }

    pub fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().contains_key(session_id)
    }

    fn remove_from_registry(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.sessions.write().remove(session_id)
    }

    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.read().keys().cloned().collect()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    pub fn is_alive(&self, session_id: &str) -> Result<bool, PtyError> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))?;
        Ok(session.is_alive())
    }
}
