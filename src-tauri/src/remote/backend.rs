use crate::terminal::{PtySessionState, SessionAttachment, TerminalService, TerminalSignal};
use futures_util::future::BoxFuture;
use std::path::PathBuf;

/// Minimal details of a terminal session required by Remote Gateway routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSessionDetails {
    pub session_id: String,
    pub workspace_id: Option<String>,
    pub worktree_label: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub running: bool,
    pub cols: u16,
    pub rows: u16,
}

/// Object-safe abstraction for session routing across local and legacy daemon backends.
pub trait RemoteSessionBackend: Send + Sync {
    fn list_sessions(&self) -> BoxFuture<'_, Vec<String>>;
    fn describe_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>>;
    fn attach_with_sequence<'a>(
        &'a self,
        session_id: &'a str,
        after_sequence: Option<u64>,
    ) -> BoxFuture<'a, Result<SessionAttachment, String>>;
    fn write_input<'a>(
        &'a self,
        session_id: &'a str,
        data: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>>;
    fn resize<'a>(
        &'a self,
        session_id: &'a str,
        cols: u16,
        rows: u16,
    ) -> BoxFuture<'a, Result<(), String>>;
    fn signal<'a>(
        &'a self,
        session_id: &'a str,
        signal: TerminalSignal,
    ) -> BoxFuture<'a, Result<(), String>>;
}

impl RemoteSessionBackend for TerminalService {
    fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
        let sessions = TerminalService::list_sessions(self);
        Box::pin(async move { sessions })
    }

    fn describe_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>> {
        let session = self.get_session(session_id);
        let id_owned = session_id.to_string();
        Box::pin(async move {
            match session {
                Some(session) => {
                    let (cols, rows) = session.get_size();
                    let running = matches!(
                        session.state(),
                        PtySessionState::Running | PtySessionState::Starting
                    );
                    let worktree_path = session.worktree_path();
                    Ok(RemoteSessionDetails {
                        session_id: session.id().to_string(),
                        workspace_id: None,
                        worktree_label: None,
                        worktree_path,
                        running,
                        cols,
                        rows,
                    })
                }
                None => Err(format!("Session '{id_owned}' not found")),
            }
        })
    }

    fn attach_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> BoxFuture<'_, Result<SessionAttachment, String>> {
        let res = self
            .attach_with_sequence(session_id, after_sequence)
            .map_err(|e| e.to_string());
        Box::pin(async move { res })
    }

    fn write_input(&self, session_id: &str, data: &[u8]) -> BoxFuture<'_, Result<(), String>> {
        let res = self
            .write_input(session_id, data)
            .map_err(|e| e.to_string());
        Box::pin(async move { res })
    }

    fn resize(&self, session_id: &str, cols: u16, rows: u16) -> BoxFuture<'_, Result<(), String>> {
        let res = self
            .resize(session_id, cols, rows)
            .map_err(|e| e.to_string());
        Box::pin(async move { res })
    }

    fn signal(
        &self,
        session_id: &str,
        signal: TerminalSignal,
    ) -> BoxFuture<'_, Result<(), String>> {
        let res = self.signal(session_id, signal).map_err(|e| e.to_string());
        Box::pin(async move { res })
    }
}
