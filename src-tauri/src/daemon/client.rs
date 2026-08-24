use crate::daemon::protocol::{
    DaemonRemoteStatus, DaemonRequest, DaemonResponse, DaemonSessionDetails, DaemonStreamMessage,
    DAEMON_PROTOCOL_VERSION,
};
use crate::daemon::server::{get_socket_path, validate_runtime_socket_path};
use crate::ipc::{IpcError, IpcErrorCode};
use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::protocol::RemoteActiveDesktopSelection;
use crate::remote::state::RemoteGatewayConfig;
use crate::session::PersistedWorkspaceSession;
use crate::terminal::output_hub::ReplayGap;
use crate::terminal::TerminalSignal;
use crate::worktree::WorktreeIdentity;
use serde_json::json;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

const DAEMON_READY_TOKEN: &str = "FERRYX_DAEMON_READY";
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(5);

async fn wait_for_daemon_ready<R>(reader: R, timeout: Duration) -> Result<(), IpcError>
where
    R: AsyncBufRead + Unpin,
{
    let mut lines = reader.lines();
    tokio::time::timeout(timeout, async {
        while let Some(line) = lines.next_line().await.map_err(|error| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Failed to read daemon readiness output: {error}"),
            )
        })? {
            if line.trim() == DAEMON_READY_TOKEN {
                return Ok(());
            }
        }
        Err(IpcError::new(
            IpcErrorCode::InternalError,
            "Daemon process stdout closed before emitting readiness signal",
        ))
    })
    .await
    .map_err(|_| {
        IpcError::new(
            IpcErrorCode::InternalError,
            "Ferryx daemon startup timed out waiting for readiness signal",
        )
    })?
}

#[derive(Debug)]
struct RequestAttemptError {
    error: IpcError,
    may_have_been_delivered: bool,
}

impl RequestAttemptError {
    fn not_delivered(error: IpcError) -> Self {
        Self {
            error,
            may_have_been_delivered: false,
        }
    }

    fn ambiguous(error: IpcError) -> Self {
        Self {
            error,
            may_have_been_delivered: true,
        }
    }

    fn into_ipc_error(self, req: &DaemonRequest, report_ambiguous: bool) -> IpcError {
        if report_ambiguous && self.may_have_been_delivered {
            ambiguous_delivery_error(req, &self.error)
        } else {
            self.error
        }
    }
}

fn request_is_retry_safe(req: &DaemonRequest) -> bool {
    matches!(
        req,
        DaemonRequest::Handshake { .. }
            | DaemonRequest::Ping
            | DaemonRequest::Spawn { .. }
            | DaemonRequest::ListSessions
            | DaemonRequest::DescribeSession { .. }
            | DaemonRequest::LoadSession
    )
}

fn request_type_name(req: &DaemonRequest) -> &'static str {
    match req {
        DaemonRequest::Handshake { .. } => "handshake",
        DaemonRequest::Ping => "ping",
        DaemonRequest::RegisterWorkspace { .. } => "registerWorkspace",
        DaemonRequest::Spawn { .. } => "spawn",
        DaemonRequest::Write { .. } => "write",
        DaemonRequest::Resize { .. } => "resize",
        DaemonRequest::Signal { .. } => "signal",
        DaemonRequest::Close { .. } => "close",
        DaemonRequest::ListSessions => "listSessions",
        DaemonRequest::DescribeSession { .. } => "describeSession",
        DaemonRequest::Attach { .. } => "attach",
        DaemonRequest::SaveSession { .. } => "saveSession",
        DaemonRequest::LoadSession => "loadSession",
        DaemonRequest::ClearSession => "clearSession",
        DaemonRequest::RemoteGetStatus => "remoteGetStatus",
        DaemonRequest::RemoteConfigure { .. } => "remoteConfigure",
        DaemonRequest::RemoteCreatePairingCode { .. } => "remoteCreatePairingCode",
        DaemonRequest::RemoteListDevices => "remoteListDevices",
        DaemonRequest::RemoteRevokeDevice { .. } => "remoteRevokeDevice",
        DaemonRequest::RemoteSetActiveSelection { .. } => "remoteSetActiveSelection",
        DaemonRequest::RemoteGetActiveSelection => "remoteGetActiveSelection",
        DaemonRequest::Shutdown => "shutdown",
    }
}

fn ambiguous_delivery_error(req: &DaemonRequest, cause: &IpcError) -> IpcError {
    IpcError::new(
        IpcErrorCode::IoError,
        format!(
            "Daemon request '{}' may have been delivered before the connection failed; refusing automatic retry",
            request_type_name(req)
        ),
    )
    .with_details(json!({
        "type": "ambiguousDelivery",
        "requestType": request_type_name(req),
        "causeCode": cause.code,
        "causeMessage": cause.message,
    }))
}

fn daemon_socket_trust_error(message: String) -> IpcError {
    IpcError::new(
        IpcErrorCode::IoError,
        format!("Refusing untrusted daemon socket: {message}"),
    )
    .with_details(json!({
        "type": "daemonSocketTrustValidation",
    }))
}

struct ActiveConnection {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl ActiveConnection {
    async fn request(
        &mut self,
        req: &DaemonRequest,
    ) -> Result<DaemonResponse, RequestAttemptError> {
        let mut req_json = serde_json::to_string(req).map_err(|e| {
            RequestAttemptError::not_delivered(IpcError::new(
                IpcErrorCode::ParseError,
                format!("Request serialization failed: {e}"),
            ))
        })?;
        req_json.push('\n');
        self.writer
            .write_all(req_json.as_bytes())
            .await
            .map_err(|e| {
                RequestAttemptError::ambiguous(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Request write failed: {e}"),
                ))
            })?;
        self.writer.flush().await.map_err(|e| {
            RequestAttemptError::ambiguous(IpcError::new(
                IpcErrorCode::IoError,
                format!("Request flush failed: {e}"),
            ))
        })?;

        let mut line = String::new();
        let bytes_read = self.reader.read_line(&mut line).await.map_err(|e| {
            RequestAttemptError::ambiguous(IpcError::new(
                IpcErrorCode::IoError,
                format!("Response read failed: {e}"),
            ))
        })?;
        if bytes_read == 0 || line.trim().is_empty() {
            return Err(RequestAttemptError::ambiguous(IpcError::new(
                IpcErrorCode::IoError,
                "Connection closed by daemon (EOF)",
            )));
        }

        let resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            RequestAttemptError::ambiguous(IpcError::new(
                IpcErrorCode::ParseError,
                format!("Response parse failed: {e}"),
            ))
        })?;

        Ok(resp)
    }
}

pub struct DaemonAttachment {
    pub session_id: String,
    pub epoch: u64,
    pub start_sequence: Option<u64>,
    pub end_sequence: Option<u64>,
    pub gap: Option<ReplayGap>,
    pub history: Vec<u8>,
    pub messages: mpsc::Receiver<DaemonStreamMessage<'static>>,
    pub stream_task: tokio::task::JoinHandle<()>,
}

#[derive(Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    connection: Arc<Mutex<Option<ActiveConnection>>>,
    epoch: Arc<parking_lot::RwLock<Option<u64>>>,
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonClient {
    pub fn new() -> Self {
        Self {
            socket_path: get_socket_path(),
            connection: Arc::new(Mutex::new(None)),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn new_with_socket(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            connection: Arc::new(Mutex::new(None)),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn epoch(&self) -> Option<u64> {
        *self.epoch.read()
    }

    fn validate_existing_socket_path(path: &Path) -> Result<(), IpcError> {
        // `new_with_socket` is a dependency-injection hook used by tests and local harnesses
        // with arbitrary temporary UDS paths. Section-9 trust requirements govern the fixed
        // production runtime endpoint, so do not impose `/tmp/rorca-{uid}` directory-mode
        // semantics on unrelated injected sockets.
        if path != get_socket_path() {
            return Ok(());
        }
        validate_runtime_socket_path(path).map_err(daemon_socket_trust_error)
    }

    #[cfg(test)]
    fn validate_existing_socket_path_for_uid(
        path: &Path,
        expected_uid: libc::uid_t,
    ) -> Result<(), IpcError> {
        crate::daemon::server::validate_runtime_socket_path_for_uid(path, expected_uid)
            .map_err(daemon_socket_trust_error)
    }

    async fn connect_or_spawn(&self) -> Result<UnixStream, IpcError> {
        match fs::symlink_metadata(&self.socket_path) {
            Ok(_) => {
                Self::validate_existing_socket_path(&self.socket_path)?;
                if let Ok(stream) = UnixStream::connect(&self.socket_path).await {
                    return Ok(stream);
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    format!(
                        "Failed to inspect daemon socket {}: {error}",
                        self.socket_path.display()
                    ),
                ));
            }
        }

        // Launch external ferryx --daemon binary with exact bounded readiness event
        let binary_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferryx"));

        let mut child = tokio::process::Command::new(&binary_path)
            .arg("--daemon")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!(
                        "Failed to spawn Ferryx daemon process ({}): {e}",
                        binary_path.display()
                    ),
                )
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::InternalError,
                "Failed to capture daemon process stdout",
            )
        })?;

        if let Err(error) =
            wait_for_daemon_ready(BufReader::new(stdout), DAEMON_READY_TIMEOUT).await
        {
            let _ = child.kill().await;
            return Err(error);
        }

        if let Err(error) = Self::validate_existing_socket_path(&self.socket_path) {
            let _ = child.kill().await;
            return Err(error);
        }

        // Exactly one connection attempt after the readiness event; never poll and never fall back.
        UnixStream::connect(&self.socket_path).await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::InternalError,
                format!("Failed to connect to daemon socket after readiness signal: {e}"),
            )
        })
    }

    async fn connect_and_handshake(&self) -> Result<ActiveConnection, IpcError> {
        let stream = self.connect_or_spawn().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut json = serde_json::to_string(&handshake).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Handshake serialization failed: {e}"),
            )
        })?;
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Handshake write failed: {e}"),
            )
        })?;
        write_half.flush().await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Handshake flush failed: {e}"),
            )
        })?;

        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Handshake read failed: {e}"))
        })?;
        if bytes_read == 0 || line.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                "Handshake failed: daemon disconnected unexpectedly",
            ));
        }

        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Handshake parse failed: {e}"),
            )
        })?;

        match hs_resp {
            DaemonResponse::HandshakeOk { version, epoch, .. } => {
                if version != DAEMON_PROTOCOL_VERSION {
                    return Err(IpcError::new(
                        IpcErrorCode::InternalError,
                        format!(
                            "Daemon protocol version mismatch: expected {DAEMON_PROTOCOL_VERSION}, got {version}"
                        ),
                    ));
                }
                *self.epoch.write() = Some(epoch);
                Ok(ActiveConnection {
                    reader,
                    writer: write_half,
                })
            }
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => Err(IpcError::new(
                IpcErrorCode::InternalError,
                format!(
                    "Daemon protocol version mismatch: expected {expected_version}, got {received_version}"
                ),
            )),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected handshake response from daemon",
            )),
        }
    }

    pub async fn send_request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        let mut conn_guard = self.connection.lock().await;
        let retry_safe = request_is_retry_safe(&req);

        if let Some(conn) = conn_guard.as_mut() {
            match conn.request(&req).await {
                Ok(resp) => return Ok(resp),
                Err(error) => {
                    *conn_guard = None;
                    if !retry_safe {
                        return Err(error.into_ipc_error(&req, true));
                    }
                }
            }
        }

        let mut fresh_conn = self.connect_and_handshake().await?;
        let resp = match fresh_conn.request(&req).await {
            Ok(resp) => resp,
            Err(error) => return Err(error.into_ipc_error(&req, !retry_safe)),
        };
        *conn_guard = Some(fresh_conn);
        Ok(resp)
    }

    pub async fn register_workspace(
        &self,
        workspace_id: &str,
        repo_root: &str,
    ) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RegisterWorkspace {
                workspace_id: workspace_id.to_string(),
                repo_root: repo_root.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::RegisterWorkspaceOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn spawn_terminal(
        &self,
        client_request_id: String,
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<String, IpcError> {
        let resp = self
            .send_request(DaemonRequest::Spawn {
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
            })
            .await?;

        match resp {
            DaemonResponse::SpawnOk { session_id } => Ok(session_id),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn describe_session(
        &self,
        session_id: &str,
    ) -> Result<DaemonSessionDetails, IpcError> {
        let resp = self
            .send_request(DaemonRequest::DescribeSession {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::DescribeSessionOk { session } => Ok(session),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn attach(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
    ) -> Result<DaemonAttachment, IpcError> {
        let stream = self.connect_or_spawn().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
        };
        let mut json = serde_json::to_string(&handshake).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Handshake serialization failed: {e}"),
            )
        })?;
        json.push('\n');
        write_half.write_all(json.as_bytes()).await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Handshake write failed: {e}"),
            )
        })?;
        write_half.flush().await.map_err(|e| {
            IpcError::new(
                IpcErrorCode::IoError,
                format!("Handshake flush failed: {e}"),
            )
        })?;

        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Handshake read failed: {e}"))
        })?;
        if bytes_read == 0 || line.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                "Handshake failed: daemon disconnected unexpectedly",
            ));
        }

        let hs_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Handshake parse failed: {e}"),
            )
        })?;

        match hs_resp {
            DaemonResponse::HandshakeOk { version, .. } if version == DAEMON_PROTOCOL_VERSION => {}
            DaemonResponse::HandshakeOk { version, .. } => {
                return Err(IpcError::new(
                    IpcErrorCode::InternalError,
                    format!(
                        "Daemon protocol version mismatch: expected {DAEMON_PROTOCOL_VERSION}, got {version}"
                    ),
                ));
            }
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => {
                return Err(IpcError::new(
                    IpcErrorCode::InternalError,
                    format!(
                        "Daemon protocol version mismatch: expected {expected_version}, got {received_version}"
                    ),
                ));
            }
            DaemonResponse::Error { message } => {
                return Err(IpcError::new(IpcErrorCode::InternalError, message));
            }
            _ => {
                return Err(IpcError::new(
                    IpcErrorCode::InternalError,
                    "Unexpected handshake response from daemon",
                ));
            }
        }

        let attach_req = DaemonRequest::Attach {
            session_id: session_id.to_string(),
            after_sequence,
        };
        let mut attach_json = serde_json::to_string(&attach_req).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Attach serialization failed: {e}"),
            )
        })?;
        attach_json.push('\n');
        write_half
            .write_all(attach_json.as_bytes())
            .await
            .map_err(|e| {
                IpcError::new(IpcErrorCode::IoError, format!("Attach write failed: {e}"))
            })?;
        write_half.flush().await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Attach flush failed: {e}"))
        })?;

        line.clear();
        let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
            IpcError::new(IpcErrorCode::IoError, format!("Attach read failed: {e}"))
        })?;
        if bytes_read == 0 || line.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                "Attach failed: daemon disconnected unexpectedly",
            ));
        }

        let attach_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Attach parse failed: {e}"),
            )
        })?;

        match attach_resp {
            DaemonResponse::AttachOk {
                epoch,
                session_id: resp_session_id,
                start_sequence,
                end_sequence,
                gap,
                history,
            } => {
                let (tx, rx) = mpsc::channel(256);
                let task = tokio::spawn(async move {
                    let mut stream_line = String::new();
                    while let Ok(n) = reader.read_line(&mut stream_line).await {
                        if n == 0 {
                            break;
                        }
                        if let Ok(msg) =
                            serde_json::from_str::<DaemonStreamMessage<'static>>(stream_line.trim())
                        {
                            let is_exit = matches!(msg, DaemonStreamMessage::Exit { .. });
                            if tx.send(msg).await.is_err() {
                                break;
                            }
                            if is_exit {
                                break;
                            }
                        }
                        stream_line.clear();
                    }
                });

                Ok(DaemonAttachment {
                    session_id: resp_session_id,
                    epoch,
                    start_sequence,
                    end_sequence,
                    gap,
                    history,
                    messages: rx,
                    stream_task: task,
                })
            }
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response for attach",
            )),
        }
    }

    pub async fn write_terminal(&self, session_id: &str, data: Vec<u8>) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Write {
                session_id: session_id.to_string(),
                data,
            })
            .await?;

        match resp {
            DaemonResponse::WriteOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn resize_terminal(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Resize {
                session_id: session_id.to_string(),
                cols,
                rows,
            })
            .await?;

        match resp {
            DaemonResponse::ResizeOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn signal_terminal(
        &self,
        session_id: &str,
        signal: TerminalSignal,
    ) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Signal {
                session_id: session_id.to_string(),
                signal,
            })
            .await?;

        match resp {
            DaemonResponse::SignalOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn close_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Close {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::CloseOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn list_sessions(&self) -> Result<Vec<String>, IpcError> {
        let resp = self.send_request(DaemonRequest::ListSessions).await?;

        match resp {
            DaemonResponse::ListSessionsOk { epoch, sessions } => {
                *self.epoch.write() = Some(epoch);
                Ok(sessions)
            }
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn ping(&self) -> Result<(), IpcError> {
        let resp = self.send_request(DaemonRequest::Ping).await?;
        match resp {
            DaemonResponse::Pong => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_get_status(&self) -> Result<DaemonRemoteStatus, IpcError> {
        let resp = self.send_request(DaemonRequest::RemoteGetStatus).await?;
        match resp {
            DaemonResponse::RemoteStatusOk { status } => Ok(status),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_configure(&self, config: RemoteGatewayConfig) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteConfigure { config })
            .await?;
        match resp {
            DaemonResponse::RemoteConfigureOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_create_pairing_code(
        &self,
        permission: Option<DevicePermission>,
    ) -> Result<String, IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteCreatePairingCode { permission })
            .await?;
        match resp {
            DaemonResponse::RemotePairingCodeOk { code } => Ok(code),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_list_devices(&self) -> Result<Vec<DeviceInfo>, IpcError> {
        let resp = self.send_request(DaemonRequest::RemoteListDevices).await?;
        match resp {
            DaemonResponse::RemoteListDevicesOk { devices } => Ok(devices),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_revoke_device(&self, device_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteRevokeDevice {
                device_id: device_id.to_string(),
            })
            .await?;
        match resp {
            DaemonResponse::RemoteRevokeDeviceOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_set_active_selection(
        &self,
        selection: Option<RemoteActiveDesktopSelection>,
    ) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteSetActiveSelection { selection })
            .await?;
        match resp {
            DaemonResponse::RemoteSetActiveSelectionOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_get_active_selection(
        &self,
    ) -> Result<Option<RemoteActiveDesktopSelection>, IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteGetActiveSelection)
            .await?;
        match resp {
            DaemonResponse::RemoteGetActiveSelectionOk { selection } => Ok(selection),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn save_session(&self, session: PersistedWorkspaceSession) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::SaveSession { session })
            .await?;

        match resp {
            DaemonResponse::SaveSessionOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn load_session(&self) -> Result<Option<PersistedWorkspaceSession>, IpcError> {
        let resp = self.send_request(DaemonRequest::LoadSession).await?;

        match resp {
            DaemonResponse::LoadSessionOk { session } => Ok(session),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn clear_session(&self) -> Result<(), IpcError> {
        let resp = self.send_request(DaemonRequest::ClearSession).await?;

        match resp {
            DaemonResponse::ClearSessionOk => Ok(()),
            DaemonResponse::Error { message } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::protocol::{
        DaemonRequest, DaemonResponse, DaemonStreamMessage, DAEMON_PROTOCOL_VERSION,
    };
    use crate::daemon::server::DaemonServer;
    use crate::remote::state::RemoteNetworkMode;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn test_daemon_readiness_requires_exact_stdout_token_without_polling() {
        let (mut writer, reader) = tokio::io::duplex(256);
        let wait = tokio::spawn(async move {
            wait_for_daemon_ready(BufReader::new(reader), Duration::from_secs(1)).await
        });

        writer
            .write_all(b"booting\nFERRYX_DAEMON_READY_EXTRA\n")
            .await
            .unwrap();
        tokio::task::yield_now().await;
        assert!(!wait.is_finished(), "near-match must not signal readiness");
        writer.write_all(b"FERRYX_DAEMON_READY\n").await.unwrap();
        wait.await.unwrap().expect("exact token signals readiness");
    }

    fn init_test_git_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        dir
    }

    #[tokio::test]
    async fn test_client_reuses_persistent_connection_without_rehandshaking() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_reuse.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let (tx_done, rx_done) = oneshot::channel();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            // 1. Handshake
            reader.read_line(&mut line).await.unwrap();
            let hs: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(hs, DaemonRequest::Handshake { .. }));
            let hs_resp = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 999,
            };
            let mut hs_json = serde_json::to_string(&hs_resp).unwrap();
            hs_json.push('\n');
            write_half.write_all(hs_json.as_bytes()).await.unwrap();

            // 2. First request: ListSessions
            line.clear();
            let n = reader.read_line(&mut line).await.unwrap();
            assert!(n > 0, "Expected first request on persistent connection");
            let req1: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(req1, DaemonRequest::ListSessions));
            let resp1 = DaemonResponse::ListSessionsOk {
                epoch: 999,
                sessions: vec!["session-1".to_string()],
            };
            let mut resp1_json = serde_json::to_string(&resp1).unwrap();
            resp1_json.push('\n');
            write_half.write_all(resp1_json.as_bytes()).await.unwrap();

            // 3. Second request: Ping (MUST be on same stream without handshake)
            line.clear();
            let n2 = reader.read_line(&mut line).await.unwrap();
            assert!(
                n2 > 0,
                "Expected second request on same persistent connection without disconnect"
            );
            let req2: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            assert!(
                matches!(req2, DaemonRequest::Ping),
                "Expected Ping request on persistent stream, got {line}"
            );
            let resp2 = DaemonResponse::Pong;
            let mut resp2_json = serde_json::to_string(&resp2).unwrap();
            resp2_json.push('\n');
            write_half.write_all(resp2_json.as_bytes()).await.unwrap();

            let _ = tx_done.send(true);
        });

        let client = DaemonClient::new_with_socket(socket_path);
        let res1 = client
            .send_request(DaemonRequest::ListSessions)
            .await
            .unwrap();
        assert!(matches!(res1, DaemonResponse::ListSessionsOk { .. }));

        let res2 = client.send_request(DaemonRequest::Ping).await.unwrap();
        assert!(matches!(res2, DaemonResponse::Pong));

        server_task.await.unwrap();
        assert!(rx_done.await.unwrap());
    }

    #[tokio::test]
    async fn test_client_reconnects_transparently_when_socket_dropped() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_reconnect.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let (tx_done, rx_done) = oneshot::channel();

        let server_task = tokio::spawn(async move {
            // --- Connection 1 ---
            let (stream1, _) = listener.accept().await.unwrap();
            let (read1, mut write1) = stream1.into_split();
            let mut reader1 = BufReader::new(read1);
            let mut line = String::new();

            // Handshake 1
            reader1.read_line(&mut line).await.unwrap();
            let hs_resp = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 999,
            };
            let mut hs_json = serde_json::to_string(&hs_resp).unwrap();
            hs_json.push('\n');
            write1.write_all(hs_json.as_bytes()).await.unwrap();

            // Request 1: ListSessions
            line.clear();
            reader1.read_line(&mut line).await.unwrap();
            let resp1 = DaemonResponse::ListSessionsOk {
                epoch: 999,
                sessions: vec!["s1".to_string()],
            };
            let mut resp1_json = serde_json::to_string(&resp1).unwrap();
            resp1_json.push('\n');
            write1.write_all(resp1_json.as_bytes()).await.unwrap();

            // Drop connection 1 explicitly
            drop(write1);
            drop(reader1);

            // --- Connection 2 (transparent reconnect)
            let (stream2, _) = listener.accept().await.unwrap();
            let (read2, mut write2) = stream2.into_split();
            let mut reader2 = BufReader::new(read2);

            // Handshake 2 on new connection
            line.clear();
            reader2.read_line(&mut line).await.unwrap();
            let hs2: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(hs2, DaemonRequest::Handshake { .. }));
            write2.write_all(hs_json.as_bytes()).await.unwrap();

            // Request 2 on new connection: Ping
            line.clear();
            reader2.read_line(&mut line).await.unwrap();
            let req2: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            assert!(matches!(req2, DaemonRequest::Ping));
            let resp2 = DaemonResponse::Pong;
            let mut resp2_json = serde_json::to_string(&resp2).unwrap();
            resp2_json.push('\n');
            write2.write_all(resp2_json.as_bytes()).await.unwrap();

            let _ = tx_done.send(true);
        });

        let client = DaemonClient::new_with_socket(socket_path);
        let res1 = client
            .send_request(DaemonRequest::ListSessions)
            .await
            .unwrap();
        assert!(matches!(res1, DaemonResponse::ListSessionsOk { .. }));

        let res2 = client.send_request(DaemonRequest::Ping).await.unwrap();
        assert!(matches!(res2, DaemonResponse::Pong));

        server_task.await.unwrap();
        assert!(rx_done.await.unwrap());
    }

    #[tokio::test]
    async fn test_client_dedicated_attach_stream_does_not_monopolize_control_connection() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_dedicated.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&server_clone);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = DaemonClient::new_with_socket(socket_path.clone());

        // 1. Spawn a terminal session
        let session_id = client
            .spawn_terminal(
                "test-req-1".to_string(),
                "default".to_string(),
                None,
                None,
                80,
                24,
            )
            .await
            .expect("spawn terminal");

        // 2. Attach dedicated stream
        let mut attachment = client
            .attach(&session_id, None)
            .await
            .expect("attach dedicated stream");

        assert_eq!(attachment.session_id, session_id);
        assert!(attachment.epoch > 0);

        // 3. Send control requests on persistent connection WHILE attachment stream is alive
        client.ping().await.expect("ping on control connection");
        client
            .write_terminal(&session_id, b"echo v2_test\n".to_vec())
            .await
            .expect("write on control connection");
        client
            .resize_terminal(&session_id, 100, 30)
            .await
            .expect("resize on control connection");

        let desc = client
            .describe_session(&session_id)
            .await
            .expect("describe on control connection");
        assert_eq!(desc.cols, 100);
        assert_eq!(desc.rows, 30);

        let sessions = client
            .list_sessions()
            .await
            .expect("list on control connection");
        assert!(sessions.contains(&session_id));

        // 4. Verify attachment receives streamed output
        let msg = tokio::time::timeout(Duration::from_secs(2), attachment.messages.recv())
            .await
            .expect("timed out waiting for output")
            .expect("received stream message");

        match msg {
            DaemonStreamMessage::Output {
                session_id: s_id,
                sequence,
                data,
            } => {
                assert_eq!(s_id, session_id);
                assert!(sequence >= 1);
                assert!(!data.is_empty());
            }
            other => panic!("Expected Output message, got {other:?}"),
        }

        client
            .close_terminal(&session_id)
            .await
            .expect("close terminal");

        server_task.abort();
    }

    #[tokio::test]
    async fn test_client_spawn_describe_attach_write_cycle() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_cycle.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = Arc::new(DaemonServer::new());
        let repo = init_test_git_repo();
        server
            .handle_register_workspace("default", repo.path().to_str().unwrap())
            .unwrap();

        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&server_clone);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = DaemonClient::new_with_socket(socket_path);

        let session_id = client
            .spawn_terminal(
                "cycle-req-1".to_string(),
                "default".to_string(),
                None,
                None,
                90,
                35,
            )
            .await
            .expect("spawn terminal");

        let session_id_2 = client
            .spawn_terminal(
                "cycle-req-1".to_string(),
                "default".to_string(),
                None,
                None,
                90,
                35,
            )
            .await
            .expect("idempotent spawn");
        assert_eq!(session_id, session_id_2);

        let details = client
            .describe_session(&session_id)
            .await
            .expect("describe session");
        assert_eq!(details.session_id, session_id);
        assert_eq!(details.cols, 90);
        assert_eq!(details.rows, 35);
        assert!(details.running);

        client
            .signal_terminal(&session_id, TerminalSignal::Interrupt)
            .await
            .expect("signal terminal");

        client
            .close_terminal(&session_id)
            .await
            .expect("close terminal");

        let sessions = client.list_sessions().await.expect("list sessions");
        assert!(!sessions.contains(&session_id));

        server_task.abort();
    }

    #[tokio::test]
    async fn test_client_workspace_registration_and_remote_apis() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_workspace_remote.sock");

        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = Arc::new(DaemonServer::new());
        let server_clone = Arc::clone(&server);

        let server_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&server_clone);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = DaemonClient::new_with_socket(socket_path);

        // 1. Workspace registration
        let repo = init_test_git_repo();
        client
            .register_workspace("ws-project", repo.path().to_str().unwrap())
            .await
            .expect("register workspace");

        // 2. Remote control typed APIs
        let status = client.remote_get_status().await.expect("remote get status");
        assert_eq!(status.mode, RemoteNetworkMode::Off);
        assert!(!status.is_running);

        let pair_code = client
            .remote_create_pairing_code(Some(DevicePermission::Control))
            .await
            .expect("create pairing code");
        assert_eq!(pair_code.len(), 6);

        let devices = client.remote_list_devices().await.expect("list devices");
        assert!(devices.is_empty());

        let sel = RemoteActiveDesktopSelection {
            workspace_id: Some("ws-project".to_string()),
            worktree_slug: None,
            worktree_label: None,
            session_id: None,
        };
        client
            .remote_set_active_selection(Some(sel.clone()))
            .await
            .expect("set active selection");

        let fetched_sel = client
            .remote_get_active_selection()
            .await
            .expect("get active selection");
        assert_eq!(fetched_sel, Some(sel));

        server_task.abort();
    }

    #[tokio::test]
    async fn test_mutating_request_is_not_retried_after_ambiguous_delivery() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_ambiguous_write.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let server_task = tokio::spawn(async move {
            let (stream1, _) = listener.accept().await.unwrap();
            let (read1, mut write1) = stream1.into_split();
            let mut reader1 = BufReader::new(read1);
            let mut line = String::new();

            reader1.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Handshake { .. }
            ));
            let handshake = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 321,
            };
            let mut handshake_json = serde_json::to_string(&handshake).unwrap();
            handshake_json.push('\n');
            write1.write_all(handshake_json.as_bytes()).await.unwrap();

            line.clear();
            reader1.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::ListSessions
            ));
            let mut list_json = serde_json::to_string(&DaemonResponse::ListSessionsOk {
                epoch: 321,
                sessions: Vec::new(),
            })
            .unwrap();
            list_json.push('\n');
            write1.write_all(list_json.as_bytes()).await.unwrap();

            line.clear();
            reader1.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Write { .. }
            ));
            drop(write1);
            drop(reader1);

            match tokio::time::timeout(Duration::from_millis(400), listener.accept()).await {
                Ok(Ok((stream2, _))) => {
                    let (read2, mut write2) = stream2.into_split();
                    let mut reader2 = BufReader::new(read2);
                    line.clear();
                    reader2.read_line(&mut line).await.unwrap();
                    assert!(matches!(
                        serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                        DaemonRequest::Handshake { .. }
                    ));
                    write2.write_all(handshake_json.as_bytes()).await.unwrap();
                    line.clear();
                    reader2.read_line(&mut line).await.unwrap();
                    assert!(matches!(
                        serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                        DaemonRequest::Write { .. }
                    ));
                    let mut response = serde_json::to_string(&DaemonResponse::WriteOk).unwrap();
                    response.push('\n');
                    write2.write_all(response.as_bytes()).await.unwrap();
                    true
                }
                _ => false,
            }
        });

        let client = DaemonClient::new_with_socket(socket_path);
        client
            .send_request(DaemonRequest::ListSessions)
            .await
            .expect("prime persistent connection");

        let result = client
            .send_request(DaemonRequest::Write {
                session_id: "ambiguous-session".into(),
                data: b"echo once\n".to_vec(),
            })
            .await;
        let resent = server_task.await.unwrap();
        let error = result.expect_err("mutating request with lost response must be ambiguous");

        assert!(
            !resent,
            "mutating request must not be re-sent after delivery became ambiguous"
        );
        assert_eq!(error.code, IpcErrorCode::IoError);
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("type"))
                .and_then(|value| value.as_str()),
            Some("ambiguousDelivery")
        );
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("requestType"))
                .and_then(|value| value.as_str()),
            Some("write")
        );
    }

    #[tokio::test]
    async fn test_retry_safe_read_is_retried_after_ambiguous_delivery() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_ambiguous_read.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let server_task = tokio::spawn(async move {
            let (stream1, _) = listener.accept().await.unwrap();
            let (read1, mut write1) = stream1.into_split();
            let mut reader1 = BufReader::new(read1);
            let mut line = String::new();

            reader1.read_line(&mut line).await.unwrap();
            let handshake = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 654,
            };
            let mut handshake_json = serde_json::to_string(&handshake).unwrap();
            handshake_json.push('\n');
            write1.write_all(handshake_json.as_bytes()).await.unwrap();

            line.clear();
            reader1.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::ListSessions
            ));
            let mut list_json = serde_json::to_string(&DaemonResponse::ListSessionsOk {
                epoch: 654,
                sessions: Vec::new(),
            })
            .unwrap();
            list_json.push('\n');
            write1.write_all(list_json.as_bytes()).await.unwrap();

            line.clear();
            reader1.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Ping
            ));
            drop(write1);
            drop(reader1);

            let (stream2, _) = listener.accept().await.unwrap();
            let (read2, mut write2) = stream2.into_split();
            let mut reader2 = BufReader::new(read2);
            line.clear();
            reader2.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Handshake { .. }
            ));
            write2.write_all(handshake_json.as_bytes()).await.unwrap();
            line.clear();
            reader2.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Ping
            ));
            let mut pong = serde_json::to_string(&DaemonResponse::Pong).unwrap();
            pong.push('\n');
            write2.write_all(pong.as_bytes()).await.unwrap();
        });

        let client = DaemonClient::new_with_socket(socket_path);
        client
            .send_request(DaemonRequest::ListSessions)
            .await
            .expect("prime persistent connection");
        let response = client.send_request(DaemonRequest::Ping).await.unwrap();
        assert!(matches!(response, DaemonResponse::Pong));
        server_task.await.unwrap();
    }

    #[test]
    fn test_client_rejects_symlinked_socket_before_connecting() {
        let dir = tempdir().unwrap();
        fs::set_permissions(
            dir.path(),
            <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
        )
        .unwrap();
        let real_socket = dir.path().join("real.sock");
        let symlinked_socket = dir.path().join("daemon.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&real_socket).unwrap();
        symlink(&real_socket, &symlinked_socket).unwrap();
        let current_uid = unsafe { libc::getuid() };

        let error = DaemonClient::validate_existing_socket_path_for_uid(
            &symlinked_socket,
            current_uid,
        )
        .expect_err("symlinked daemon socket must be rejected before connect");
        assert_eq!(error.code, IpcErrorCode::IoError);
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("type"))
                .and_then(|value| value.as_str()),
            Some("daemonSocketTrustValidation")
        );
        assert!(error.message.contains("symlink"));
    }

    #[test]
    fn test_client_rejects_wrong_uid_runtime_dir_before_connecting() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("daemon.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        let current_uid = unsafe { libc::getuid() };
        let wrong_uid = current_uid.wrapping_add(1);

        let error = DaemonClient::validate_existing_socket_path_for_uid(&socket_path, wrong_uid)
            .expect_err("wrong-UID runtime directory must be rejected");
        assert_eq!(error.code, IpcErrorCode::IoError);
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("type"))
                .and_then(|value| value.as_str()),
            Some("daemonSocketTrustValidation")
        );
        assert!(error.message.contains("owned by UID"));
    }
}
