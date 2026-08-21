use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DaemonStreamMessage, DAEMON_PROTOCOL_VERSION,
};
use crate::remote::server::start_remote_server;
use crate::remote::state::{RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode};
use crate::session::{
    clear_session_from_path, load_session_from_path, save_session_to_path,
};
use crate::terminal::{PtyManager, TerminalOutputHub, TerminalService};
use crate::worktree::WorkspaceRegistry;
use portable_pty::CommandBuilder;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

pub fn get_runtime_dir() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    let path = PathBuf::from(format!("/tmp/rorca-{uid}"));
    let _ = fs::create_dir_all(&path);
    if let Ok(metadata) = fs::metadata(&path) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o700);
        let _ = fs::set_permissions(&path, permissions);
    }
    path
}

pub fn get_socket_path() -> PathBuf {
    get_runtime_dir().join("daemon.sock")
}

pub fn get_lock_path() -> PathBuf {
    get_runtime_dir().join("daemon.lock")
}

pub fn get_default_session_path() -> PathBuf {
    if let Some(mut base) = dirs_next().or_else(dirs_fallback) {
        base.push("rorca");
        let _ = fs::create_dir_all(&base);
        base.join("session_state.json")
    } else {
        get_runtime_dir().join("session_state.json")
    }
}

fn dirs_next() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".rorca"))
}

pub struct DaemonServer {
    terminal_service: Arc<TerminalService>,
    workspace_registry: WorkspaceRegistry,
    _remote_state: Arc<RemoteGatewayState>,
}

impl Default for DaemonServer {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonServer {
    pub fn new() -> Self {
        let pty_manager = Arc::new(PtyManager::new());
        let output_hub = Arc::new(TerminalOutputHub::default());
        let terminal_service = Arc::new(TerminalService::new(
            Arc::clone(&pty_manager),
            Arc::clone(&output_hub),
        ));
        let workspace_registry = WorkspaceRegistry::new();
        let remote_state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            workspace_registry.clone(),
        ));

        // Start Axum remote gateway by default in daemon
        *remote_state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 43821,
            allow_control: true,
        };
        let rs_clone = Arc::clone(&remote_state);
        tokio::spawn(async move {
            let _ = start_remote_server(rs_clone).await;
        });

        Self {
            terminal_service,
            workspace_registry,
            _remote_state: remote_state,
        }
    }

    pub async fn run_server(self: Arc<Self>) -> Result<(), String> {
        let socket_path = get_socket_path();
        let lock_path = get_lock_path();

        // Flock lock for atomic bind & stale socket recovery
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| format!("Failed to open lock file: {e}"))?;

        unsafe {
            use std::os::unix::io::AsRawFd;
            let fd = lock_file.as_raw_fd();
            if libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) != 0 {
                return Err("Another daemon instance is already holding the lock.".into());
            }
        }

        // Clean up stale socket if any
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("Failed to bind UDS socket at {}: {e}", socket_path.display()))?;

        // Ensure 0600 mode
        if let Ok(metadata) = fs::metadata(&socket_path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            let _ = fs::set_permissions(&socket_path, permissions);
        }

        tracing::info!("rorca daemon listening on {}", socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let server = Arc::clone(&self);
                    tokio::spawn(async move {
                        server.handle_client(stream).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Daemon accept error: {e}");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_client(self: Arc<Self>, stream: UnixStream) {
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        while let Ok(n) = reader.read_line(&mut line).await {
            if n == 0 {
                break;
            }
            let req: Result<DaemonRequest, _> = serde_json::from_str(line.trim());
            line.clear();

            let resp = match req {
                Ok(DaemonRequest::Handshake { version }) => {
                    if version != DAEMON_PROTOCOL_VERSION {
                        DaemonResponse::Error {
                            message: format!(
                                "Protocol version mismatch: expected {DAEMON_PROTOCOL_VERSION}, got {version}"
                            ),
                        }
                    } else {
                        DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                        }
                    }
                }
                Ok(DaemonRequest::Ping) => DaemonResponse::Pong,
                Ok(DaemonRequest::Spawn {
                    workspace_id,
                    worktree,
                    cols,
                    rows,
                }) => {
                    let res = self.handle_spawn(&workspace_id, worktree, cols, rows).await;
                    match res {
                        Ok(session_id) => DaemonResponse::SpawnOk { session_id },
                        Err(e) => DaemonResponse::Error { message: e },
                    }
                }
                Ok(DaemonRequest::Write { session_id, data }) => {
                    match self.terminal_service.write_input(&session_id, &data) {
                        Ok(()) => DaemonResponse::WriteOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Resize {
                    session_id,
                    cols,
                    rows,
                }) => match self.terminal_service.resize(&session_id, cols, rows) {
                    Ok(()) => DaemonResponse::ResizeOk,
                    Err(e) => DaemonResponse::Error {
                        message: e.to_string(),
                    },
                },
                Ok(DaemonRequest::Signal { session_id, signal }) => {
                    match self.terminal_service.signal(&session_id, signal) {
                        Ok(()) => DaemonResponse::SignalOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Close { session_id }) => {
                    match self.terminal_service.close_session(&session_id).await {
                        Ok(()) => DaemonResponse::CloseOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::ListSessions) => {
                    let sessions = self.terminal_service.list_sessions();
                    DaemonResponse::ListSessionsOk { sessions }
                }
                Ok(DaemonRequest::Attach { session_id }) => {
                    match self.terminal_service.attach(&session_id) {
                        Ok((history, rx)) => {
                            // Stream live PTY output as stream messages
                            let session_id_clone = session_id.clone();
                            let hub = Arc::clone(self.terminal_service.output_hub());
                            let mut write_clone = write_half; // will be handled
                            // Send initial attach ok
                            let resp = DaemonResponse::AttachOk { history };
                            let mut resp_json = serde_json::to_string(&resp).unwrap();
                            resp_json.push('\n');
                            let _ = write_clone.write_all(resp_json.as_bytes()).await;

                            // Pump output
                            Self::pump_stream(session_id_clone, rx, hub, write_clone).await;
                            return;
                        }
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::SaveSession { session }) => {
                    let path = get_default_session_path();
                    match save_session_to_path(&path, &session) {
                        Ok(()) => DaemonResponse::SaveSessionOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::LoadSession) => {
                    let path = get_default_session_path();
                    match load_session_from_path(&path) {
                        Ok(session) => DaemonResponse::LoadSessionOk { session },
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::ClearSession) => {
                    let path = get_default_session_path();
                    match clear_session_from_path(&path) {
                        Ok(()) => DaemonResponse::ClearSessionOk,
                        Err(e) => DaemonResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                Ok(DaemonRequest::Shutdown) => {
                    std::process::exit(0);
                }
                Err(e) => DaemonResponse::Error {
                    message: format!("Malformed request: {e}"),
                },
            };

            let mut resp_json = serde_json::to_string(&resp).unwrap();
            resp_json.push('\n');
            if write_half.write_all(resp_json.as_bytes()).await.is_err() {
                break;
            }
        }
    }

    async fn handle_spawn(
        &self,
        workspace_id: &str,
        worktree: Option<crate::worktree::WorktreeIdentity>,
        cols: u16,
        rows: u16,
    ) -> Result<String, String> {
        let (mgr, cwd) = self
            .workspace_registry
            .resolve_terminal_target(workspace_id, worktree.as_ref())
            .map_err(|e| e.to_string())?;

        let mut cmd = CommandBuilder::new_default_prog();
        cmd.cwd(&cwd);

        let (session_id, _) = self
            .terminal_service
            .spawn_in_worktree(cmd, cols, rows, &mgr, &cwd)
            .map_err(|e| e.to_string())?;

        Ok(session_id)
    }

    async fn pump_stream(
        session_id: String,
        mut rx: broadcast::Receiver<Vec<u8>>,
        hub: Arc<TerminalOutputHub>,
        mut writer: tokio::net::unix::OwnedWriteHalf,
    ) {
        loop {
            match rx.recv().await {
                Ok(data) => {
                    let msg = DaemonStreamMessage::Output {
                        session_id: session_id.clone(),
                        data,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    if writer.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Resync with ringbuffer history snapshot
                    if let Some((history, new_rx)) = hub.subscribe(&session_id) {
                        rx = new_rx;
                        let msg = DaemonStreamMessage::Lagged {
                            session_id: session_id.clone(),
                            history,
                        };
                        let mut json = serde_json::to_string(&msg).unwrap();
                        json.push('\n');
                        if writer.write_all(json.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    let msg = DaemonStreamMessage::Exit {
                        session_id: session_id.clone(),
                        exit_code: None,
                    };
                    let mut json = serde_json::to_string(&msg).unwrap();
                    json.push('\n');
                    let _ = writer.write_all(json.as_bytes()).await;
                    break;
                }
            }
        }
    }
}
