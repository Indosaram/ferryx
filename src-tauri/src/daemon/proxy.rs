use crate::daemon::manifest::{get_manifest_path, HandoverManifest, HandoverRoute};
use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
};
use crate::remote::backend::{RemoteSessionBackend, RemoteSessionDetails};
use crate::terminal::{
    AttachmentSnapshot, OutputChunk, PtySessionState, SessionAttachment, TerminalService,
    TerminalSignal,
};
use futures_util::future::BoxFuture;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(unix))]
use tokio::net::TcpStream as LegacyStream;
#[cfg(unix)]
use tokio::net::UnixStream as LegacyStream;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct LegacyPeer {
    socket_path: PathBuf,
    known_sessions: Arc<RwLock<Vec<String>>>,
}

impl LegacyPeer {
    pub fn new(socket_path: PathBuf, initial_sessions: Vec<String>) -> Self {
        Self {
            socket_path,
            known_sessions: Arc::new(RwLock::new(initial_sessions)),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn contains_session(&self, session_id: &str) -> bool {
        self.known_sessions.read().iter().any(|s| s == session_id)
    }

    pub fn remove_session(&self, session_id: &str) {
        self.known_sessions.write().retain(|s| s != session_id);
    }

    #[cfg(unix)]
    async fn connect_stream(&self) -> Result<LegacyStream, String> {
        let socket_path = self.socket_path.clone();
        tokio::task::spawn_blocking(move || {
            crate::daemon::server::validate_runtime_socket_path(&socket_path)
        })
        .await
        .map_err(|e| format!("Validation task failed: {e}"))??;

        LegacyStream::connect(&self.socket_path).await.map_err(|e| {
            format!(
                "Failed to connect to legacy daemon at {}: {e}",
                self.socket_path.display()
            )
        })
    }

    #[cfg(not(unix))]
    async fn connect_stream(&self) -> Result<LegacyStream, String> {
        let port_str = std::fs::read_to_string(&self.socket_path)
            .map_err(|e| format!("Failed to read legacy daemon port file: {e}"))?;
        let port: u16 = port_str
            .trim()
            .parse()
            .map_err(|e| format!("Invalid legacy daemon port: {e}"))?;
        LegacyStream::connect(format!("127.0.0.1:{port}"))
            .await
            .map_err(|e| format!("Failed to connect to legacy daemon: {e}"))
    }

    pub async fn send_request(&self, req: &DaemonRequest) -> Result<DaemonResponse, String> {
        let stream = self.connect_stream().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).map_err(|e| e.to_string())?;
        hs_json.push('\n');
        write_half
            .write_all(hs_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .map_err(|_| "Legacy handshake timed out".to_string())?
            .map_err(|e| format!("Legacy handshake read failed: {e}"))?;

        let hs_resp: DaemonResponse =
            serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        match hs_resp {
            DaemonResponse::HandshakeOk { .. } => {}
            other => {
                return Err(format!(
                    "Unexpected handshake from legacy daemon: {other:?}"
                ))
            }
        }

        let mut req_json = serde_json::to_string(req).map_err(|e| e.to_string())?;
        req_json.push('\n');
        write_half
            .write_all(req_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        line.clear();
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .map_err(|_| "Legacy request timed out".to_string())?
            .map_err(|e| format!("Legacy response read failed: {e}"))?;

        if line.trim().is_empty() {
            return Err("Legacy daemon closed connection without response".to_string());
        }

        serde_json::from_str(line.trim())
            .map_err(|e| format!("Failed to parse legacy response: {e}"))
    }

    pub async fn list_sessions(&self) -> Result<Vec<String>, String> {
        match self.send_request(&DaemonRequest::ListSessions).await? {
            DaemonResponse::ListSessionsOk { sessions, .. } => {
                *self.known_sessions.write() = sessions.clone();
                Ok(sessions)
            }
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!("Unexpected response for ListSessions: {other:?}")),
        }
    }

    pub async fn describe_session(&self, session_id: &str) -> Result<DaemonSessionDetails, String> {
        match self
            .send_request(&DaemonRequest::DescribeSession {
                session_id: session_id.to_string(),
            })
            .await?
        {
            DaemonResponse::DescribeSessionOk { session } => Ok(session),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!(
                "Unexpected response for DescribeSession: {other:?}"
            )),
        }
    }

    pub async fn write_input(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        match self
            .send_request(&DaemonRequest::Write {
                session_id: session_id.to_string(),
                data: data.to_vec(),
            })
            .await?
        {
            DaemonResponse::WriteOk => Ok(()),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!("Unexpected response for Write: {other:?}")),
        }
    }

    pub async fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        match self
            .send_request(&DaemonRequest::Resize {
                session_id: session_id.to_string(),
                cols,
                rows,
            })
            .await?
        {
            DaemonResponse::ResizeOk => Ok(()),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!("Unexpected response for Resize: {other:?}")),
        }
    }

    pub async fn signal(&self, session_id: &str, signal: TerminalSignal) -> Result<(), String> {
        match self
            .send_request(&DaemonRequest::Signal {
                session_id: session_id.to_string(),
                signal,
            })
            .await?
        {
            DaemonResponse::SignalOk => Ok(()),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!("Unexpected response for Signal: {other:?}")),
        }
    }

    pub async fn close(&self, session_id: &str) -> Result<(), String> {
        let resp = self
            .send_request(&DaemonRequest::Close {
                session_id: session_id.to_string(),
            })
            .await?;
        self.remove_session(session_id);
        match resp {
            DaemonResponse::CloseOk => Ok(()),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!("Unexpected response for Close: {other:?}")),
        }
    }

    pub async fn discover_agent_session(
        &self,
        session_id: &str,
        agent_type: &str,
    ) -> Result<Option<String>, String> {
        match self
            .send_request(&DaemonRequest::DiscoverAgentSession {
                session_id: session_id.to_string(),
                agent_type: agent_type.to_string(),
            })
            .await?
        {
            DaemonResponse::DiscoverAgentSessionOk {
                provider_session_id,
            } => Ok(provider_session_id),
            DaemonResponse::Error { message } => Err(message),
            other => Err(format!(
                "Unexpected response for DiscoverAgentSession: {other:?}"
            )),
        }
    }

    pub async fn attach_and_stream<W>(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
        client_writer: &mut W,
    ) -> Result<(), String>
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let stream = self.connect_stream().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).map_err(|e| e.to_string())?;
        hs_json.push('\n');
        write_half
            .write_all(hs_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Legacy attach handshake read failed: {e}"))?;
        let hs_resp: DaemonResponse =
            serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        match hs_resp {
            DaemonResponse::HandshakeOk { .. } => {}
            other => {
                return Err(format!(
                    "Unexpected handshake response from legacy: {other:?}"
                ))
            }
        }

        let attach_req = DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        };
        let mut req_json = serde_json::to_string(&attach_req).map_err(|e| e.to_string())?;
        req_json.push('\n');
        write_half
            .write_all(req_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        line.clear();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Legacy attach response read failed: {e}"))?;
        if line.trim().is_empty() {
            return Err("Legacy daemon closed connection on attach".to_string());
        }

        // Forward AttachOk frame directly to client writer
        client_writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        client_writer.flush().await.map_err(|e| e.to_string())?;

        // Pump remaining stream messages directly
        line.clear();
        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            let is_exit =
                if let Ok(msg) = serde_json::from_str::<DaemonStreamMessage<'_>>(line.trim()) {
                    matches!(msg, DaemonStreamMessage::Exit { .. })
                } else {
                    false
                };
            if client_writer.write_all(line.as_bytes()).await.is_err()
                || client_writer.flush().await.is_err()
            {
                break;
            }
            if is_exit {
                break;
            }
            line.clear();
        }

        Ok(())
    }

    pub async fn attach_session(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<SessionAttachment, String> {
        let stream = self.connect_stream().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hs = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut hs_json = serde_json::to_string(&hs).map_err(|e| e.to_string())?;
        hs_json.push('\n');
        write_half
            .write_all(hs_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Legacy attach handshake read failed: {e}"))?;
        let hs_resp: DaemonResponse =
            serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        match hs_resp {
            DaemonResponse::HandshakeOk { .. } => {}
            other => {
                return Err(format!(
                    "Unexpected handshake response from legacy: {other:?}"
                ))
            }
        }

        let attach_req = DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        };
        let mut req_json = serde_json::to_string(&attach_req).map_err(|e| e.to_string())?;
        req_json.push('\n');
        write_half
            .write_all(req_json.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        write_half.flush().await.map_err(|e| e.to_string())?;

        line.clear();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Legacy attach response read failed: {e}"))?;
        if line.trim().is_empty() {
            return Err("Legacy daemon closed connection on attach".to_string());
        }

        let attach_resp: DaemonResponse =
            serde_json::from_str(line.trim()).map_err(|e| e.to_string())?;
        let (snapshot, tx, rx) = match attach_resp {
            DaemonResponse::AttachOk {
                start_sequence,
                end_sequence,
                gap,
                history,
                history_segments,
                ..
            } => {
                let segments = history_segments
                    .into_iter()
                    .map(|seg| crate::terminal::output_hub::HistorySegment {
                        cols: seg.cols,
                        rows: seg.rows,
                        bytes: seg.bytes,
                    })
                    .collect();
                let snapshot = AttachmentSnapshot {
                    session_id: session_id.to_string(),
                    history,
                    history_segments: segments,
                    history_start_sequence: start_sequence,
                    history_end_sequence: end_sequence,
                    gap,
                };
                let (tx, rx) = broadcast::channel(2048);
                (snapshot, tx, rx)
            }
            DaemonResponse::Error { message } => return Err(message),
            other => return Err(format!("Unexpected response for Attach: {other:?}")),
        };

        tokio::spawn(async move {
            let _keepalive = write_half;
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    line.clear();
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<DaemonStreamMessage<'_>>(trimmed) {
                    match msg {
                        DaemonStreamMessage::Output {
                            sequence,
                            data,
                            metrics_read_unix_micros,
                            ..
                        } => {
                            let _ = tx.send(OutputChunk {
                                sequence,
                                bytes: data.into_owned(),
                                metrics_read_unix_micros,
                            });
                        }
                        DaemonStreamMessage::Exit { .. } => {
                            break;
                        }
                        _ => {}
                    }
                }
                line.clear();
            }
        });

        Ok(SessionAttachment {
            snapshot,
            receiver: rx,
        })
    }
}

pub struct SessionRouter {
    terminal_service: Arc<TerminalService>,
    legacy_peers: Arc<RwLock<Vec<Arc<LegacyPeer>>>>,
}

impl SessionRouter {
    pub fn new(terminal_service: Arc<TerminalService>) -> Self {
        Self {
            terminal_service,
            legacy_peers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn add_legacy_peer(&self, peer: Arc<LegacyPeer>) {
        self.legacy_peers.write().push(Arc::clone(&peer));
        let manifest_path = get_manifest_path();
        let socket_path = peer.socket_path().to_path_buf();
        let sessions = peer.known_sessions.read().clone();
        tokio::task::spawn_blocking(move || {
            let mut manifest = HandoverManifest::load_from_path(&manifest_path);
            manifest.add_or_update_route(HandoverRoute {
                legacy_socket_path: socket_path,
                sessions,
            });
            let _ = manifest.save_to_path(&manifest_path);
        });
    }

    pub async fn adopt_routes_from_manifest(&self) {
        let manifest_path = get_manifest_path();
        let routes = tokio::task::spawn_blocking(move || {
            let mut manifest = HandoverManifest::load_from_path(&manifest_path);
            manifest.prune_dead_routes();
            let _ = manifest.save_to_path(&manifest_path);
            manifest.routes
        })
        .await
        .unwrap_or_default();

        for route in routes {
            if self
                .legacy_peers
                .read()
                .iter()
                .any(|p| p.socket_path() == route.legacy_socket_path)
            {
                continue;
            }
            let peer = Arc::new(LegacyPeer::new(
                route.legacy_socket_path.clone(),
                route.sessions.clone(),
            ));
            if let Ok(live_sessions) = peer.list_sessions().await {
                *peer.known_sessions.write() = live_sessions;
                self.legacy_peers.write().push(peer);
            }
        }
    }

    pub fn is_local_session(&self, session_id: &str) -> bool {
        self.terminal_service.get_session(session_id).is_some()
    }

    pub fn find_legacy_peer_for_session(&self, session_id: &str) -> Option<Arc<LegacyPeer>> {
        let peers = self.legacy_peers.read();
        peers
            .iter()
            .find(|p| p.contains_session(session_id))
            .cloned()
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        let mut all_sessions = self.terminal_service.list_sessions();
        let peers = self.legacy_peers.read().clone();
        for peer in peers {
            if let Ok(legacy_sessions) = peer.list_sessions().await {
                for s in legacy_sessions {
                    if !all_sessions.contains(&s) {
                        all_sessions.push(s);
                    }
                }
            }
        }
        all_sessions
    }
}

impl RemoteSessionBackend for SessionRouter {
    fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
        Box::pin(async move { SessionRouter::list_sessions(self).await })
    }

    fn describe_session(
        &self,
        session_id: &str,
    ) -> BoxFuture<'_, Result<RemoteSessionDetails, String>> {
        let session_id = session_id.to_string();
        Box::pin(async move {
            if self.is_local_session(&session_id) {
                let session = self
                    .terminal_service
                    .get_session(&session_id)
                    .ok_or_else(|| format!("Session '{session_id}' not found"))?;
                let (cols, rows) = session.get_size();
                let running = matches!(
                    session.state(),
                    PtySessionState::Running | PtySessionState::Starting
                );
                let worktree_path = session.worktree_path();
                Ok(RemoteSessionDetails {
                    session_id,
                    workspace_id: None,
                    worktree_label: None,
                    worktree_path,
                    running,
                    cols,
                    rows,
                })
            } else if let Some(peer) = self.find_legacy_peer_for_session(&session_id) {
                let details = peer.describe_session(&session_id).await?;
                Ok(RemoteSessionDetails {
                    session_id: details.session_id,
                    workspace_id: details.workspace_id,
                    worktree_label: details.worktree.as_ref().and_then(|w| Some(w.slug.clone())),
                    worktree_path: details.cwd.map(PathBuf::from),
                    running: details.running,
                    cols: details.cols,
                    rows: details.rows,
                })
            } else {
                Err(format!("Session '{session_id}' not found"))
            }
        })
    }

    fn attach_with_sequence(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> BoxFuture<'_, Result<SessionAttachment, String>> {
        let session_id = session_id.to_string();
        Box::pin(async move {
            if self.is_local_session(&session_id) {
                self.terminal_service
                    .attach_with_sequence(&session_id, after_sequence)
                    .map_err(|e| e.to_string())
            } else if let Some(peer) = self.find_legacy_peer_for_session(&session_id) {
                peer.attach_session(&session_id, after_sequence).await
            } else {
                Err(format!("Session '{session_id}' not found"))
            }
        })
    }

    fn write_input(&self, session_id: &str, data: &[u8]) -> BoxFuture<'_, Result<(), String>> {
        let session_id = session_id.to_string();
        let data = data.to_vec();
        Box::pin(async move {
            if self.is_local_session(&session_id) {
                self.terminal_service
                    .write_input(&session_id, &data)
                    .map_err(|e| e.to_string())
            } else if let Some(peer) = self.find_legacy_peer_for_session(&session_id) {
                peer.write_input(&session_id, &data).await
            } else {
                Err(format!("Session '{session_id}' not found"))
            }
        })
    }

    fn resize(&self, session_id: &str, cols: u16, rows: u16) -> BoxFuture<'_, Result<(), String>> {
        let session_id = session_id.to_string();
        Box::pin(async move {
            if self.is_local_session(&session_id) {
                self.terminal_service
                    .resize(&session_id, cols, rows)
                    .map_err(|e| e.to_string())
            } else if let Some(peer) = self.find_legacy_peer_for_session(&session_id) {
                peer.resize(&session_id, cols, rows).await
            } else {
                Err(format!("Session '{session_id}' not found"))
            }
        })
    }

    fn signal(
        &self,
        session_id: &str,
        signal: TerminalSignal,
    ) -> BoxFuture<'_, Result<(), String>> {
        let session_id = session_id.to_string();
        Box::pin(async move {
            if self.is_local_session(&session_id) {
                self.terminal_service
                    .signal(&session_id, signal)
                    .map_err(|e| e.to_string())
            } else if let Some(peer) = self.find_legacy_peer_for_session(&session_id) {
                peer.signal(&session_id, signal).await
            } else {
                Err(format!("Session '{session_id}' not found"))
            }
        })
    }
}
