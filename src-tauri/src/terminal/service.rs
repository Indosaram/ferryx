use crate::terminal::output_hub::TerminalOutputHub;
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
        Self::new(Arc::new(PtyManager::new()), Arc::new(TerminalOutputHub::default()))
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
        let (session_id, mut pty_rx) = self
            .pty_manager
            .spawn_in_worktree(cmd, cols, rows, worktree_manager, worktree_path)?;

        let broadcast_rx = self.output_hub.register_session(&session_id);

        // Spawn output pump task from PTY reader to OutputHub
        let output_hub = Arc::clone(&self.output_hub);
        let session_id_clone = session_id.clone();
        tokio::spawn(async move {
            while let Some(chunk) = pty_rx.recv().await {
                output_hub.publish(&session_id_clone, chunk);
            }
        });

        Ok((session_id, broadcast_rx))
    }

    pub fn attach(&self, session_id: &str) -> Result<(Vec<u8>, broadcast::Receiver<Vec<u8>>), PtyError> {
        if !self.pty_manager.list_sessions().contains(&session_id.to_string()) {
            return Err(PtyError::SessionNotFound(session_id.to_string()));
        }

        self.output_hub
            .subscribe(session_id)
            .ok_or_else(|| PtyError::SessionNotFound(session_id.to_string()))
    }

    pub fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), PtyError> {
        self.pty_manager.write_input(session_id, data)
    }

    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pty_manager.resize(session_id, cols, rows)
    }

    pub fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), PtyError> {
        self.pty_manager.signal(session_id, signal)
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), PtyError> {
        self.output_hub.remove_session(session_id);
        self.pty_manager.close_session(session_id).await
    }

    pub fn list_sessions(&self) -> Vec<String> {
        self.pty_manager.list_sessions()
    }

    pub fn get_session(&self, session_id: &str) -> Option<Arc<PtySession>> {
        self.pty_manager.get_session(session_id)
    }
}
