use crate::terminal::output_hub::{SessionAttachment, TerminalOutputHub};
use crate::terminal::{PtyError, PtyManager, PtySession, TerminalSignal};
use crate::worktree::manager::WorktreeManager;
use portable_pty::CommandBuilder;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct TerminalService {
    pty_manager: Arc<PtyManager>,
    output_hub: Arc<TerminalOutputHub>,
}

impl Default for TerminalService {
    fn default() -> Self {
        Self::new(
            Arc::new(PtyManager::new()),
            Arc::new(TerminalOutputHub::default()),
        )
    }
}

impl TerminalService {
    pub fn new(pty_manager: Arc<PtyManager>, output_hub: Arc<TerminalOutputHub>) -> Self {
        Self {
            pty_manager,
            output_hub,
        }
    }

    pub fn pty_manager(&self) -> &Arc<PtyManager> {
        &self.pty_manager
    }

    pub fn output_hub(&self) -> &Arc<TerminalOutputHub> {
        &self.output_hub
    }

    pub fn spawn_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<(String, broadcast::Receiver<Vec<u8>>), PtyError> {
        self.spawn_in_worktree_internal(cmd, cols, rows, worktree_manager, worktree_path, true)
    }

    pub fn spawn_reconnectable_in_worktree(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<
        (
            String,
            broadcast::Receiver<Vec<u8>>,
            tokio::sync::oneshot::Receiver<()>,
        ),
        PtyError,
    > {
        let (session_id, pty_rx) =
            self.pty_manager
                .spawn_in_worktree(cmd, cols, rows, worktree_manager, worktree_path)?;
        let broadcast_rx = self.output_hub.register_session(&session_id);
        self.output_hub.record_initial_size(&session_id, cols, rows);
        let ended = self.pump_output(session_id.clone(), pty_rx, false);
        Ok((session_id, broadcast_rx, ended))
    }

    fn spawn_in_worktree_internal(
        &self,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
        remove_hub_on_end: bool,
    ) -> Result<(String, broadcast::Receiver<Vec<u8>>), PtyError> {
        let (session_id, pty_rx) =
            self.pty_manager
                .spawn_in_worktree(cmd, cols, rows, worktree_manager, worktree_path)?;

        let broadcast_rx = self.output_hub.register_session(&session_id);
        self.output_hub.record_initial_size(&session_id, cols, rows);
        let _ = self.pump_output(session_id.clone(), pty_rx, remove_hub_on_end);

        Ok((session_id, broadcast_rx))
    }

    /// Respawn a PTY while retaining the pane's logical id and OutputHub history.
    pub fn respawn_in_worktree(
        &self,
        session_id: &str,
        cmd: CommandBuilder,
        cols: u16,
        rows: u16,
        worktree_manager: &WorktreeManager,
        worktree_path: &Path,
    ) -> Result<tokio::sync::oneshot::Receiver<()>, PtyError> {
        let pty_rx = self.pty_manager.spawn_with_id_in_worktree(
            session_id.to_string(),
            cmd,
            cols,
            rows,
            worktree_manager,
            worktree_path,
        )?;
        Ok(self.pump_output(session_id.to_string(), pty_rx, false))
    }

    fn pump_output(
        &self,
        session_id: String,
        mut pty_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
        remove_hub_on_end: bool,
    ) -> tokio::sync::oneshot::Receiver<()> {
        let output_hub = Arc::clone(&self.output_hub);
        let (ended_tx, ended_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            while let Some(chunk) = pty_rx.recv().await {
                let read_unix_micros =
                    crate::terminal::metrics::take_pty_read_timestamp(&session_id, chunk.len());
                output_hub.publish_with_read_timestamp(&session_id, chunk, read_unix_micros);
            }
            crate::terminal::metrics::clear_pty_read_timestamps(&session_id);
            if remove_hub_on_end {
                // Reconnectable sessions keep the receiver alive past PTY EOF. The daemon
                // removes their hub explicitly when the pane closes or retries exhaust.
                output_hub.remove_session(&session_id);
            }
            let _ = ended_tx.send(());
        });
        ended_rx
    }

    pub fn attach(
        &self,
        session_id: &str,
    ) -> Result<(Vec<u8>, broadcast::Receiver<Vec<u8>>), PtyError> {
        if !self
            .pty_manager
            .list_sessions()
            .contains(&session_id.to_string())
        {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    pub fn attach_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<SessionAttachment, PtyError> {
        if !self
            .pty_manager
            .list_sessions()
            .contains(&session_id.to_string())
        {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe_with_sequence(session_id, after_sequence)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        self.pty_manager.write_input(session_id, data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pty_manager.resize(session_id, cols, rows)?;
        // Single choke point for ALL resize callers (daemon request arm, remote gateway):
        // every PTY resize must leave a ledger marker or segmented replay misattributes
        // post-resize bytes to the previous width.
        self.output_hub.record_resize(session_id, cols, rows);
        Ok(())
    }

    pub fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), PtyError> {
        self.pty_manager.signal(session_id, signal)
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), PtyError> {
        self.output_hub.remove_session(session_id);
        self.pty_manager.close_session(session_id).await
    }

    pub fn list_sessions(&self) -> Vec<String> {
        let mut sessions = self.pty_manager.list_sessions();
        for session_id in self.output_hub.list_sessions() {
            if !sessions.contains(&session_id) {
                sessions.push(session_id);
            }
        }
        sessions
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.pty_manager.get_session(session_id)
    }

    pub fn take_last_exit_code(&self, session_id: &str) -> Option<i32> {
        self.pty_manager.take_last_exit_code(session_id)
    }
}
