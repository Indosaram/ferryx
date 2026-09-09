use crate::terminal::{PtySessionState, SessionAttachment, TerminalService, TerminalSignal};
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use crate::terminal::remote::RemoteConnectionState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRecoveryStatus {
    pub state: RemoteConnectionState,
    pub generation: u64,
}

pub type RecoveryStream = BoxStream<'static, RemoteRecoveryStatus>;

pub(crate) fn recovery_stream(rx: tokio::sync::watch::Receiver<crate::terminal::remote::RemoteSessionDetails>) -> RecoveryStream {
    Box::pin(futures_util::stream::unfold((rx, true), |(mut rx, initial)| async move {
        if !initial && rx.changed().await.is_err() { return None; }
        let status = {
            let details = rx.borrow_and_update();
            RemoteRecoveryStatus { state: details.state, generation: details.generation }
        };
        Some((status, (rx, false)))
    }))
}
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
    /// None identifies a local PTY. Errors must never downgrade SSH to local input.
    fn recovery<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Result<Option<RecoveryStream>, String>> {
        Box::pin(async { Ok(None) })
    }
    fn write_generation<'a>(&'a self, _id: &'a str, _generation: u64, _data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("Generation input unsupported".into()) })
    }
    fn resize_generation<'a>(&'a self, _id: &'a str, _generation: u64, _cols: u16, _rows: u16) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Err("Generation resize unsupported".into()) })
    }
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
    fn recovery<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Option<RecoveryStream>, String>> {
        Box::pin(async move {
            if !self.remote().contains(id) { return Ok(None); }
            self.remote().subscribe(id).map(recovery_stream).map(Some).map_err(|e| e.to_string())
        })
    }
    fn write_generation<'a>(&'a self, id: &'a str, generation: u64, data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.write_input_operation(id, generation, data.to_vec()).map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())
        })
    }
    fn resize_generation<'a>(&'a self, id: &'a str, generation: u64, cols: u16, rows: u16) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.resize_operation(id, generation, cols, rows).map_err(|e| e.to_string())?.await.map_err(|e| e.to_string())
        })
    }
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
