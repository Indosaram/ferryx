use crate::terminal::{
    session::PtySessionConfig, PtyError, PtySession, PtySessionState, TerminalSignal,
};
use crate::worktree::WorktreeManager;
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(crate) const LIFECYCLE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const READER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const TERM_GRACE_TIMEOUT: Duration = Duration::from_secs(1);
const KILL_REAP_TIMEOUT: Duration = Duration::from_secs(1);

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
        let rx = self.spawn_with_id_and_worktree(session_id.clone(), cmd, cols, rows, None)?;
        Ok((session_id, rx))
    }

    pub fn spawn_shell(
        &self,
        cols: u16,
        rows: u16,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let cmd = crate::terminal::shell::resolve_shell_command(None);
        self.spawn(cmd, cols, rows)
    }

    /// Spawn an interactive terminal owned by `worktree_path`.
    ///
    /// Interactive PTYs are deliberately multi-session.  The worktree writer lease is an
    /// exclusive mutation/agent guard and must not be used to serialize terminal panes:
    /// Orca-style split panes require two or more live PTYs in the same worktree.
    pub fn spawn_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, mpsc::Receiver<Vec<u8>>), PtyError> {
        let session_id = Uuid::new_v4().to_string();
        let canonical_worktree = worktree_manager
            .canonical_allowed_path(worktree_path)
            .map_err(|error| PtyError::Other(error.to_string()))?;
        let rx = self.spawn_with_id_and_worktree(
            session_id.clone(),
            cmd,
            cols,
            rows,
            Some(canonical_worktree),
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
        self.spawn_with_id_and_worktree(session_id.into(), cmd, cols, rows, None)
    }

    fn spawn_with_id_and_worktree(
        &self,
        session_id: String,
        mut cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_path: Option<std::path::PathBuf>,
    ) -> Result<mpsc::Receiver<Vec<u8>>, PtyError> {
        if self.has_session(&session_id) {
            return Err(PtyError::Other(format!(
                "PTY session '{session_id}' already exists"
            )));
        }

        // The agent extension reports state for the pane it runs in, so it needs the session
        // identity here: this is the first point where the id exists and the child is not yet
        // spawned.
        cmd.env("FERRYX_SESSION_ID", &session_id);
        cmd.env(
            "FERRYX_AGENT_STATE_SOCKET",
            crate::daemon::agent_state_socket_path(),
        );

        // A GUI-launched daemon inherits TERM=dumb, which agent TUIs read as a non-interactive
        // terminal: they drop to plain mode and stop reporting activity. A PTY is a real
        // terminal, so it must advertise one.
        if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(true) {
            cmd.env("TERM", "xterm-256color");
        }

        // TERM=xterm-256color only claims 256 indexed colors. Truecolor-capable agent TUIs read
        // COLORTERM instead, and degrade to a reduced palette when it is missing.
        cmd.env("COLORTERM", "truecolor");

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
            worktree_path,
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
            let Some(session) = manager.get_session(&session_id) else {
                return;
            };
            let mut reader_task = session.take_reader_task();

            loop {
                let Some(session) = manager.get_session(&session_id) else {
                    break;
                };

                match session.state() {
                    PtySessionState::Exited { .. } | PtySessionState::Failed { .. } => {
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
                        session.close_output();
                        manager.remove_from_registry(&session_id);
                        break;
                    }
                }

                if let Some(ref mut handle) = reader_task {
                    tokio::select! {
                        _ = handle => {
                            reader_task = None;
                        }
                        _ = tokio::time::sleep(LIFECYCLE_POLL_INTERVAL) => {}
                    }
                } else {
                    tokio::time::sleep(LIFECYCLE_POLL_INTERVAL).await;
                }
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
            Ok(Err(error)) => Err(PtyError::Other(format!("PTY reader task failed: {error}"))),
            Err(_) => {
                reader_task.abort();
                Err(PtyError::Other(
                    "Timed out waiting for PTY reader shutdown".into(),
                ))
            }
        }
    }

    async fn poll_reap_bounded(
        session: &Arc<PtySession>,
        timeout: Duration,
    ) -> Result<Option<i32>, PtyError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match session.poll_exit_code()? {
                Some(code) => return Ok(Some(code)),
                None if session.is_reaped() => return Ok(None),
                None => {}
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok(None);
            }
            tokio::time::sleep(LIFECYCLE_POLL_INTERVAL.min(Duration::from_millis(50))).await;
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

        if exit_code.is_none() && !session.is_reaped() {
            if let Err(signal_error) = session.signal(TerminalSignal::Terminate) {
                // TERM is a best-effort graceful phase. A process-group signal can become
                // unavailable after job-control transitions (for example, immediately after
                // VINTR/SIGINT), but Close still has a mandatory KILL+reap fallback below.
                // Do not report the graceful-phase error if escalation succeeds.
                tracing::debug!(
                    "PTY TERM signal failed for {}; escalating: {}",
                    session_id,
                    signal_error
                );
            } else {
                match Self::poll_reap_bounded(&session, TERM_GRACE_TIMEOUT).await {
                    Ok(Some(code)) => exit_code = Some(code),
                    Ok(None) => {}
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
        }

        if exit_code.is_none() && !session.is_reaped() {
            if let Err(signal_error) = session
                .signal(TerminalSignal::Kill)
                .or_else(|_| session.kill())
            {
                if first_error.is_none() {
                    first_error = Some(signal_error);
                }
            } else {
                match Self::poll_reap_bounded(&session, KILL_REAP_TIMEOUT).await {
                    Ok(Some(code)) => exit_code = Some(code),
                    Ok(None) if session.is_reaped() => {}
                    Ok(None) => {
                        if first_error.is_none() {
                            first_error = Some(PtyError::Other(format!(
                                "Timed out reaping killed PTY session '{session_id}'"
                            )));
                        }
                    }
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }
        }

        session.close_io();

        if let Err(reader_error) = Self::join_reader_bounded(&session).await {
            if first_error.is_none() {
                first_error = Some(reader_error);
            }
        }

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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn close_escalates_term_ignoring_process_group_and_reaps_bounded_without_touching_sibling(
    ) {
        let manager = PtyManager::new();

        let mut stubborn_cmd = CommandBuilder::new("/bin/sh");
        stubborn_cmd.arg("-c");
        stubborn_cmd.arg("trap '' TERM; sleep 30");
        let (stubborn_id, _stubborn_rx) = manager
            .spawn(stubborn_cmd, 80, 24)
            .expect("spawn TERM-resistant session");
        let stubborn_session = manager
            .get_session(&stubborn_id)
            .expect("stubborn session registered");

        let mut sibling_cmd = CommandBuilder::new("/bin/sh");
        sibling_cmd.arg("-c");
        sibling_cmd.arg("sleep 30");
        let (sibling_id, _sibling_rx) = manager
            .spawn(sibling_cmd, 80, 24)
            .expect("spawn sibling session");

        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = tokio::time::Instant::now();
        let close_result =
            tokio::time::timeout(Duration::from_secs(6), manager.close_session(&stubborn_id))
                .await
                .expect("close must be bounded");
        close_result.expect("TERM-resistant close should escalate and succeed");

        assert!(started.elapsed() < Duration::from_secs(6));
        assert!(stubborn_session.is_reaped(), "closed child must be reaped");
        assert!(!manager.has_session(&stubborn_id));
        assert!(manager.has_session(&sibling_id));
        assert!(manager.is_alive(&sibling_id).expect("sibling state"));

        manager
            .close_session(&sibling_id)
            .await
            .expect("cleanup sibling session");
    }

    #[tokio::test]
    async fn close_after_interrupt_still_succeeds_via_escalation_and_reap() {
        let manager = PtyManager::new();
        let (session_id, _rx) = manager.spawn_shell(80, 24).expect("spawn shell");
        let session = manager
            .get_session(&session_id)
            .expect("shell session registered");

        manager
            .signal(&session_id, TerminalSignal::Interrupt)
            .expect("send interrupt through PTY");
        tokio::time::sleep(Duration::from_millis(100)).await;

        tokio::time::timeout(Duration::from_secs(6), manager.close_session(&session_id))
            .await
            .expect("close after interrupt must be bounded")
            .expect("close after interrupt must succeed after escalation");

        assert!(session.is_reaped(), "closed shell must be reaped");
        assert!(!manager.has_session(&session_id));
    }
}
