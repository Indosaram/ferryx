use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION,
};
use crate::daemon::server::{get_socket_path, DaemonServer};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::session::PersistedWorkspaceSession;
use crate::terminal::TerminalSignal;
use crate::worktree::WorktreeIdentity;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Clone)]
pub struct DaemonClient {
    // In dev or embedded mode, if daemon is not run standalone, we can either connect to UDS or fallback to direct DaemonServer
    direct_server: Arc<DaemonServer>,
}

impl DaemonClient {
    pub fn new(direct_server: Arc<DaemonServer>) -> Self {
        Self { direct_server }
    }

    async fn connect_or_spawn(&self) -> Result<UnixStream, IpcError> {
        let socket_path = get_socket_path();

        for _ in 0..10 {
            if socket_path.exists() {
                if let Ok(stream) = UnixStream::connect(&socket_path).await {
                    return Ok(stream);
                }
            }
            // Auto spawn daemon in background task
            let server_clone = Arc::clone(&self.direct_server);
            tokio::spawn(async move {
                let _ = server_clone.run_server().await;
            });
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        UnixStream::connect(&socket_path).await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("Failed to connect to Ferryx daemon: {e}"),
            )
        })
    }

    pub async fn send_request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        let stream = self.connect_or_spawn().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        // Perform handshake first
        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut json = serde_json::to_string(&handshake).unwrap();
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Handshake write failed: {e}"))
        })?;

        let mut line = String::new();
        reader.read_line(&mut line).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Handshake read failed: {e}"))
        })?;
        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(IpcErrorCode::ParseError, format!("Handshake parse failed: {e}"))
        })?;

        if let DaemonResponse::Error { message } = hs_resp {
            return Err(IpcError::new(IpcErrorCode::InternalError, message));
        }

        // Send actual request
        line.clear();
        let mut req_json = serde_json::to_string(&req).unwrap();
        req_json.push('\n');
        write_half.write_all(req_json.as_bytes()).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Request write failed: {e}"))
        })?;

        reader.read_line(&mut line).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Response read failed: {e}"))
        })?;

        let resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(IpcErrorCode::ParseError, format!("Response parse failed: {e}"))
        })?;

        Ok(resp)
    }

    pub async fn spawn_terminal(
        &self,
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cols: u16,
        rows: u16,
    ) -> Result<String, IpcError> {
        let resp = self
            .send_request(DaemonRequest::Spawn {
                workspace_id,
                worktree,
                cols,
                rows,
            })
            .await?;

        match resp {
            DaemonResponse::SpawnOk { session_id } => Ok(session_id),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn write_terminal(&self, session_id: String, data: Vec<u8>) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Write { session_id, data })
            .await?;

        match resp {
            DaemonResponse::WriteOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn resize_terminal(&self, session_id: String, cols: u16, rows: u16) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Resize {
                session_id,
                cols,
                rows,
            })
            .await?;

        match resp {
            DaemonResponse::ResizeOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn signal_terminal(&self, session_id: String, signal: TerminalSignal) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Signal { session_id, signal })
            .await?;

        match resp {
            DaemonResponse::SignalOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn close_terminal(&self, session_id: String) -> Result<(), IpcError> {
        let resp = self.send_request(DaemonRequest::Close { session_id }).await?;

        match resp {
            DaemonResponse::CloseOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<String>, IpcError> {
        let resp = self.send_request(DaemonRequest::ListSessions).await?;

        match resp {
            DaemonResponse::ListSessionsOk { sessions } => Ok(sessions),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn save_session(&self, session: PersistedWorkspaceSession) -> Result<(), IpcError> {
        let resp = self.send_request(DaemonRequest::SaveSession { session }).await?;

        match resp {
            DaemonResponse::SaveSessionOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn load_session(&self) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
        let resp = self.send_request(DaemonRequest::LoadSession).await?;

        match resp {
            DaemonResponse::LoadSessionOk { session } => Ok(session),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }

    pub async fn clear_session(&self) -> Result<(), IpcError> {
        let resp = self.send_request(DaemonRequest::ClearSession).await?;

        match resp {
            DaemonResponse::ClearSessionOk => Ok(()),
            DaemonResponse::Error { message } => Err(IpcError::new(IpcErrorCode::InternalError, message)),
            _ => Err(IpcError::new(IpcErrorCode::InternalError, "Unexpected daemon response")),
        }
    }
}
