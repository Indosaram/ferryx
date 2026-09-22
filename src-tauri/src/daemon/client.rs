// allow: SIZE_OK — Daemon client UDS connection management, retries, and streaming channels
use crate::daemon::protocol::{
    DaemonRemoteEvent, DaemonRemoteStatus, DaemonRequest, DaemonResponse, DaemonSessionDetails,
    DaemonStreamMessage, TerminalStartup, DAEMON_PROTOCOL_VERSION,
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
use bytes::Bytes;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
#[cfg(not(unix))]
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
#[cfg(unix)]
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
#[cfg(not(unix))]
use tokio::net::TcpStream as DaemonStream;
#[cfg(unix)]
use tokio::net::UnixStream as DaemonStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

const DAEMON_READY_TOKEN: &str = "FERRYX_DAEMON_READY";

#[cfg(all(test, unix))]
mod paired_host_compatibility_tests {
    use super::*;
    #[tokio::test]
    async fn old_daemon_unknown_capability_never_receives_upgrade_or_secret() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("old.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let peer = async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(&line).unwrap(),
                DaemonRequest::Handshake { .. }
            ));
            writer.write_all(format!("{{\"type\":\"handshakeOk\",\"version\":{},\"pid\":1,\"epoch\":1,\"daemonVersion\":\"old\"}}\n", DAEMON_PROTOCOL_VERSION).as_bytes()).await.unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(&line).unwrap(),
                DaemonRequest::GetCapabilities
            ));
            writer
                .write_all(b"{\"type\":\"error\",\"message\":\"unknown request\"}\n")
                .await
                .unwrap();
            line.clear();
            assert_eq!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "client sent request after capability refusal"
            );
        };
        let client = DaemonClient::new_with_socket(socket);
        let action = client.paired_host_pair(crate::paired_host::service::PairRequest {
            relay_origin: "https://relay.example".into(),
            pin: crate::paired_host::service::Secret("private-pin".into()),
            display_label: "host".into(),
        });
        let (_, result) =
            tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(peer, action) })
                .await
                .unwrap();
        assert_eq!(result.unwrap_err().code, "PAIRED_HOST_UNAVAILABLE");
        assert!(!client.upgrade_requested.load(Ordering::SeqCst));
        let path = root.path().to_owned();
        root.close().unwrap();
        eprintln!(
            "A13 old_daemon_unavailable=true no_upgrade=true no_secret_sent=true cleanup={}",
            !path.exists()
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSpawnResult {
    pub session_id: String,
    pub epoch: u64,
    pub session: DaemonSessionDetails,
}
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
            | DaemonRequest::MachineSessionDetail { .. }
            | DaemonRequest::Spawn { .. }
            | DaemonRequest::Hibernate { .. }
            | DaemonRequest::Suspend { .. }
            | DaemonRequest::Resume { .. }
            | DaemonRequest::ListSessions
            | DaemonRequest::DescribeSession { .. }
            | DaemonRequest::DiscoverAgentSession { .. }
            | DaemonRequest::ResetAgentState { .. }
            | DaemonRequest::LoadSession
            | DaemonRequest::RemoteSetActiveSelection { .. }
            | DaemonRequest::PairedTerminalDescriptor { .. }
            | DaemonRequest::UpgradeBinary { .. }
    )
}

fn request_type_name(req: &DaemonRequest) -> &'static str {
    match req {
        DaemonRequest::SshPassword { .. } => "sshPassword",
        DaemonRequest::Handshake { .. } => "handshake",
        DaemonRequest::Ping => "ping",
        DaemonRequest::MachineSessionDetail { .. } => "machineSessionDetail",
        DaemonRequest::MachineSessionMetadata { .. } => "machineSessionMetadata",
        DaemonRequest::MachineGateway => "machineGateway",
        DaemonRequest::MachineMetadataSubscribe { .. } => "machineMetadataSubscribe",
        DaemonRequest::RetryRemoteSession { .. } => "retryRemoteSession",
        DaemonRequest::RemoteSessionDetails { .. } => "remoteSessionDetails",
        DaemonRequest::RemoteWrite { .. } => "remoteWrite",
        DaemonRequest::RemoteResize { .. } => "remoteResize",
        DaemonRequest::CreateWorktree { .. } => "createWorktree",
        DaemonRequest::DeleteWorktree { .. } => "deleteWorktree",
        DaemonRequest::RegisterWorkspace { .. } => "registerWorkspace",
        DaemonRequest::UnregisterWorkspace { .. } => "unregisterWorkspace",
        DaemonRequest::Spawn { .. } => "spawn",
        DaemonRequest::Write { .. } => "write",
        DaemonRequest::Resize { .. } => "resize",
        DaemonRequest::Signal { .. } => "signal",
        DaemonRequest::Close { .. } => "close",
        DaemonRequest::Hibernate { .. } => "hibernate",
        DaemonRequest::Suspend { .. } => "suspend",
        DaemonRequest::Resume { .. } => "resume",
        DaemonRequest::ListSessions => "listSessions",
        DaemonRequest::DescribeSession { .. } => "describeSession",
        DaemonRequest::DiscoverAgentSession { .. } => "discoverAgentSession",
        DaemonRequest::ResetAgentState { .. } => "resetAgentState",
        DaemonRequest::Attach { .. } => "attach",
        DaemonRequest::SaveSession { .. } => "saveSession",
        DaemonRequest::LoadSession => "loadSession",
        DaemonRequest::ClearSession => "clearSession",
        DaemonRequest::RemoteGetStatus => "remoteGetStatus",
        DaemonRequest::GetCapabilities => "getCapabilities",
        DaemonRequest::PairedHostList => "pairedHostList",
        DaemonRequest::PairedTerminalReattach { .. } => "pairedTerminalReattach",
        DaemonRequest::PairedTerminalDetach { .. } => "pairedTerminalDetach",
        DaemonRequest::PairedTerminalDescriptor { .. } => "pairedTerminalDescriptor",
        DaemonRequest::PairedHostOperation { .. } => "pairedHostOperation",
        DaemonRequest::PairedHostRead { .. } => "pairedHostRead",
        DaemonRequest::PairedHostPair { .. } => "pairedHostPair",
        DaemonRequest::PairedHostMigrateLegacy { .. } => "pairedHostMigrateLegacy",
        DaemonRequest::PairedHostForget { .. } => "pairedHostForget",
        DaemonRequest::PairedHostRevoke { .. } => "pairedHostRevoke",
        DaemonRequest::RemoteCreateMachinePairingCode => "remoteCreateMachinePairingCode",
        DaemonRequest::RemoteConfigure { .. } => "remoteConfigure",
        DaemonRequest::RemoteCreatePairingCode { .. } => "remoteCreatePairingCode",
        DaemonRequest::RemoteListDevices => "remoteListDevices",
        DaemonRequest::RemoteRevokeDevice { .. } => "remoteRevokeDevice",
        DaemonRequest::RemoteSetActiveSelection { .. } => "remoteSetActiveSelection",
        DaemonRequest::RemoteGetActiveSelection => "remoteGetActiveSelection",
        DaemonRequest::SubscribeRemoteEvents => "subscribeRemoteEvents",
        DaemonRequest::UpgradeBinary { .. } => "upgradeBinary",
        DaemonRequest::PrepareHandover => "prepareHandover",
        DaemonRequest::TransferSessions { .. } => "transferSessions",
        DaemonRequest::CommitHandover { .. } => "commitHandover",
        DaemonRequest::AbortHandover => "abortHandover",
        DaemonRequest::UploadClipboardImage { .. } => "uploadClipboardImage",
        DaemonRequest::SubscribeDag { .. } => "subscribeDag",
        DaemonRequest::UnsubscribeDag { .. } => "unsubscribeDag",
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

fn daemon_protocol_mismatch_error(expected_version: u32, received_version: u32) -> IpcError {
    IpcError::new(
        IpcErrorCode::DaemonProtocolMismatch,
        "Ferryx daemon protocol version mismatch",
    )
    .with_details(json!({
        "expectedVersion": expected_version,
        "receivedVersion": received_version,
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
        // A daemon that never answers must not hold the shared connection mutex
        // forever: without a response timeout, one stalled handler on the daemon
        // silently deadlocks every subsequent terminal request in this process.
        let mut line = String::new();
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.reader.read_line(&mut line),
        )
        .await;
        let bytes_read = match read_result {
            Err(_) => {
                return Err(RequestAttemptError::ambiguous(IpcError::new(
                    IpcErrorCode::IoError,
                    "Timed out waiting for daemon response (15s)",
                )))
            }
            Ok(result) => result.map_err(|e| {
                RequestAttemptError::ambiguous(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Response read failed: {e}"),
                ))
            })?,
        };
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
    pub history: Bytes,
    pub history_segments: Vec<crate::terminal::output_hub::HistorySegment>,
    pub pty_cols: Option<u16>,
    pub pty_rows: Option<u16>,
    pub remote_generation: Option<u64>,
    pub messages: mpsc::Receiver<DaemonStreamMessage<'static>>,
    pub stream_task: tokio::task::JoinHandle<()>,
}

/// Pure function determining whether a daemon self-upgrade should be requested.
/// Returns `true` if:
/// 1. `daemon_version` is provided and differs from `own_version` (CalVer package date version)
/// 2. Or, for legacy daemons without version metadata, fallback to `own_mtime > daemon_mtime`.
pub fn should_request_upgrade(
    daemon_version: Option<&str>,
    own_version: &str,
    daemon_mtime: Option<u64>,
    own_mtime: Option<u64>,
) -> bool {
    if let Some(daemon_ver) = daemon_version {
        return daemon_ver != own_version;
    }
    match (daemon_mtime, own_mtime) {
        (Some(daemon), Some(own)) => own > daemon,
        _ => false,
    }
}

pub(crate) fn parse_attach_error_response(
    message: String,
    code: Option<String>,
    details: Option<serde_json::Value>,
    session_id: &str,
) -> IpcError {
    if let Some(ref c) = code {
        if c == "SESSION_NOT_FOUND" {
            let details = details.unwrap_or_else(|| {
                serde_json::json!({
                    "source": "daemon_attach",
                    "kind": "session_not_found",
                    "sessionId": session_id,
                })
            });
            return IpcError::new(IpcErrorCode::SessionNotFound, message).with_details(details);
        } else {
            let ipc_code = IpcErrorCode::from_code_str(c);
            let mut err = IpcError::new(ipc_code, message);
            if let Some(d) = details {
                err = err.with_details(d);
            }
            return err;
        }
    }

    // Backward tolerance: older daemons without structured wire error responses
    // fall back to matching legacy string prefixes. Kept strictly for backward
    // compatibility with unversioned daemons.
    let is_session_not_found = (message
        .strip_prefix("Session '")
        .and_then(|m| m.strip_suffix("' not found"))
        .is_some_and(|id| id == session_id))
        || (message
            .strip_prefix("PTY session '")
            .and_then(|m| m.strip_suffix("' not found"))
            .is_some_and(|id| id == session_id));

    if is_session_not_found {
        IpcError::new(IpcErrorCode::SessionNotFound, message).with_details(serde_json::json!({
            "source": "daemon_attach",
            "kind": "session_not_found",
            "sessionId": session_id,
        }))
    } else {
        IpcError::new(IpcErrorCode::InternalError, message)
    }
}

#[derive(Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    connection: Arc<Mutex<Option<ActiveConnection>>>,
    /// Dedicated control connection for interactive per-session requests (keystroke
    /// writes, resizes). A slow Spawn holding the shared connection mutex must never
    /// head-of-line block keystrokes queued behind it (audit H7).
    interactive_connection: Arc<Mutex<Option<ActiveConnection>>>,
    epoch: Arc<parking_lot::RwLock<Option<u64>>>,
    upgrade_requested: Arc<AtomicBool>,
    spawn_lock: Arc<Mutex<()>>,
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
            interactive_connection: Arc::new(Mutex::new(None)),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
            upgrade_requested: Arc::new(AtomicBool::new(false)),
            spawn_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn new_with_socket(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            connection: Arc::new(Mutex::new(None)),
            interactive_connection: Arc::new(Mutex::new(None)),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
            upgrade_requested: Arc::new(AtomicBool::new(false)),
            spawn_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn paired_host_exchange(
        connection: &mut ActiveConnection,
        request: &DaemonRequest,
    ) -> crate::paired_host::service::Result<DaemonResponse> {
        use crate::paired_host::service::ServiceError;
        use tokio::io::AsyncReadExt;
        let mut bytes = serde_json::to_vec(request).map_err(|_| ServiceError::unavailable())?;
        if bytes.len() > 32 * 1024 {
            return Err(ServiceError::unavailable());
        }
        bytes.push(b'\n');
        connection
            .writer
            .write_all(&bytes)
            .await
            .map_err(|_| ServiceError::unavailable())?;
        connection
            .writer
            .flush()
            .await
            .map_err(|_| ServiceError::unavailable())?;
        let mut response = Vec::new();
        const LIMIT: usize = 1024 * 1024;
        (&mut connection.reader)
            .take((LIMIT + 1) as u64)
            .read_until(b'\n', &mut response)
            .await
            .map_err(|_| ServiceError::unavailable())?;
        if response.len() > LIMIT || response.last() != Some(&b'\n') {
            return Err(ServiceError::unavailable());
        }
        serde_json::from_slice(&response).map_err(|_| ServiceError::unavailable())
    }
    pub const PAIRED_MUTATION_BUDGET_SECS: u64 = 60;
    pub const PAIRED_CAPABILITIES_BUDGET_SECS: u64 = 45;
    pub const PAIRED_JOURNAL_BUDGET_SECS: u64 = 45;
    pub const PAIRED_QUERY_BUDGET_SECS: u64 = 45;
    pub const PAIRED_TICKET_BUDGET_SECS: u64 = 45;
    pub const PAIRED_HANDSHAKE_MARGIN_SECS: u64 = 30;
    pub const PAIRED_MUTATION_OUTER_TIMEOUT: Duration = Duration::from_secs(
        Self::PAIRED_CAPABILITIES_BUDGET_SECS
            + Self::PAIRED_JOURNAL_BUDGET_SECS
            + Self::PAIRED_MUTATION_BUDGET_SECS
            + Self::PAIRED_HANDSHAKE_MARGIN_SECS,
    );
    pub const PAIRED_QUERY_OUTER_TIMEOUT: Duration = Duration::from_secs(
        Self::PAIRED_CAPABILITIES_BUDGET_SECS
            + Self::PAIRED_JOURNAL_BUDGET_SECS
            + Self::PAIRED_QUERY_BUDGET_SECS
            + Self::PAIRED_HANDSHAKE_MARGIN_SECS,
    );
    pub const PAIRED_REATTACH_OUTER_TIMEOUT: Duration = Duration::from_secs(
        Self::PAIRED_CAPABILITIES_BUDGET_SECS
            + Self::PAIRED_QUERY_BUDGET_SECS
            + Self::PAIRED_TICKET_BUDGET_SECS
            + Self::PAIRED_HANDSHAKE_MARGIN_SECS,
    );
    pub const PAIRED_DEFAULT_OUTER_TIMEOUT: Duration = Duration::from_secs(45);

    pub fn outer_deadline_for_request(request: &DaemonRequest) -> Duration {
        match request {
            DaemonRequest::PairedHostOperation { request: op } => {
                if op.operation.is_mutation() {
                    Self::PAIRED_MUTATION_OUTER_TIMEOUT
                } else {
                    Self::PAIRED_QUERY_OUTER_TIMEOUT
                }
            }
            DaemonRequest::PairedTerminalReattach { .. } => Self::PAIRED_REATTACH_OUTER_TIMEOUT,
            _ => Self::PAIRED_DEFAULT_OUTER_TIMEOUT,
        }
    }

    /// Connect only: capability absence never triggers spawn, upgrade, or retry.
    async fn paired_host_request(
        &self,
        request: DaemonRequest,
    ) -> crate::paired_host::service::Result<DaemonResponse> {
        let timeout = Self::outer_deadline_for_request(&request);
        self.paired_host_request_with_timeout(request, timeout)
            .await
    }

    async fn paired_host_request_with_timeout(
        &self,
        request: DaemonRequest,
        timeout: Duration,
    ) -> crate::paired_host::service::Result<DaemonResponse> {
        use crate::paired_host::service::ServiceError;
        tokio::time::timeout(timeout, async {
            let socket_path = self.socket_path.clone();
            crate::ipc::run_blocking(move || Self::validate_existing_socket_path(&socket_path)).await.map_err(|_| ServiceError::unavailable())?;
            let stream = Self::connect_socket(&self.socket_path).await.map_err(|_| ServiceError::unavailable())?;
            let (reader, writer) = stream.into_split();
            let mut connection = ActiveConnection { reader: BufReader::new(reader), writer };
            let handshake = Self::paired_host_exchange(&mut connection, &DaemonRequest::Handshake { version: DAEMON_PROTOCOL_VERSION }).await?;
            if !matches!(handshake, DaemonResponse::HandshakeOk { version: DAEMON_PROTOCOL_VERSION, .. }) { return Err(ServiceError::unavailable()); }
            let capabilities = Self::paired_host_exchange(&mut connection, &DaemonRequest::GetCapabilities).await?;
            if !matches!(capabilities, DaemonResponse::CapabilitiesOk { ref capabilities } if capabilities.iter().any(|c| c == "pairedHostInventoryV1")) { return Err(ServiceError::unavailable()); }
            match Self::paired_host_exchange(&mut connection, &request).await? {
                DaemonResponse::PairedHostError { error } => Err(error),
                response => Ok(response),
            }
        }).await.map_err(|_| ServiceError::new("TIMEOUT", "operation timed out"))?
    }
    pub async fn paired_terminal_reattach(
        &self,
        descriptor: crate::terminal::paired_daemon::Descriptor,
    ) -> Result<(String, crate::scoped_contracts::Epoch), crate::paired_host::client::ClientError>
    {
        use crate::paired_host::client::ClientError;
        match self
            .paired_host_request(DaemonRequest::PairedTerminalReattach { descriptor })
            .await
            .map_err(|e| ClientError::local(&e.code))?
        {
            DaemonResponse::PairedTerminalReattachOk {
                session_id,
                generation,
            } => Ok((session_id, generation)),
            DaemonResponse::PairedHostOperationError { error } => Err(error),
            _ => Err(ClientError::local("PAIRED_PROXY_UNAVAILABLE")),
        }
    }
    pub async fn paired_terminal_detach(
        &self,
        session_id: String,
    ) -> crate::paired_host::service::Result<()> {
        match self
            .paired_host_request(DaemonRequest::PairedTerminalDetach { session_id })
            .await?
        {
            DaemonResponse::CloseOk => Ok(()),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }
    pub async fn paired_terminal_descriptor(
        &self,
        session_id: String,
    ) -> Result<
        Option<crate::terminal::paired_daemon::Descriptor>,
        crate::paired_host::client::ClientError,
    > {
        match self
            .paired_host_request(DaemonRequest::PairedTerminalDescriptor {
                session_id: session_id.clone(),
            })
            .await
            .map_err(|e| crate::paired_host::client::ClientError::local(&e.code))?
        {
            DaemonResponse::PairedTerminalDescriptorOk { descriptor } => Ok(descriptor),
            // P12 (round 2 & 3): daemon errors and unexpected response variants
            // are NOT "no descriptor" — propagate them so close_terminal cannot
            // silently skip the remote close on a lookup failure.
            DaemonResponse::Error {
                message: _, code, ..
            } => Err(crate::paired_host::client::ClientError {
                code: code.unwrap_or_else(|| "DESCRIPTOR_LOOKUP_FAILED".into()),
                machine_error: None,
                ambiguous: false,
                request_id: Some(session_id),
            }),
            _ => Err(crate::paired_host::client::ClientError {
                code: "UNEXPECTED_DAEMON_RESPONSE".into(),
                machine_error: None,
                ambiguous: false,
                request_id: Some(session_id),
            }),
        }
    }
    pub async fn paired_host_list(
        &self,
    ) -> crate::paired_host::service::Result<Vec<crate::paired_host::inventory::HostView>> {
        match self
            .paired_host_request(DaemonRequest::PairedHostList)
            .await?
        {
            DaemonResponse::PairedHostListOk { hosts } => Ok(hosts),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }
    pub async fn paired_host_operation(
        &self,
        request: crate::paired_host::client::OperationRequest,
    ) -> Result<
        crate::paired_host::client::OperationResponse,
        crate::paired_host::client::ClientError,
    > {
        use crate::paired_host::client::{ClientError, Operation};
        let request_id = match &request.operation {
            Operation::RegisterProject { request } => Some(request.request_id.clone()),
            Operation::UnregisterProject { request, .. } => Some(request.request_id.clone()),
            Operation::CreateWorktree { request } => Some(request.request_id.clone()),
            Operation::DeleteWorktree { request } => Some(request.request_id.clone()),
            Operation::CreateSession { request } => Some(request.request_id.clone()),
            Operation::CloseSession { request, .. } => Some(request.request_id.clone()),
            Operation::PasteUploadChunk { request } => Some(request.request_id.clone()),
            Operation::Capabilities
            | Operation::Directories { .. }
            | Operation::Projects
            | Operation::Worktrees { .. }
            | Operation::WorktreeStatus { .. }
            | Operation::Sessions { .. }
            | Operation::Session { .. }
            | Operation::DagStream { .. }
            | Operation::Operation { .. } => None,
        };
        let transport_error = |error: crate::paired_host::service::ServiceError| ClientError {
            code: error.code,
            machine_error: None,
            ambiguous: request_id.is_some(),
            request_id: request_id.clone(),
        };
        // IPC cannot establish whether the remote mutation committed. In particular,
        // its retained 35s deadline can expire during the inner 40s HTTP attempt.
        // Preserve reconciliation identity without retrying or changing daemon errors.
        let host_id = request.host_id.clone();
        let generation = request.generation;
        let result = match self
            .paired_host_request(DaemonRequest::PairedHostOperation { request })
            .await
            .map_err(&transport_error)?
        {
            DaemonResponse::PairedHostOperationOk { response } => Ok(response),
            DaemonResponse::PairedHostOperationError { error } => Err(error),
            _ => Err(transport_error(
                crate::paired_host::service::ServiceError::unavailable(),
            )),
        };
        match result {
            Ok(response) => Ok(response),
            Err(error) => {
                // R5-N4: a definitive UNAUTHORIZED must fence the native inventory
                // to a revoked state; otherwise a later same-generation cached
                // event or list resurrects paired UI state without reauthentication.
                if error.code == "UNAUTHORIZED" {
                    let _ = self
                        .paired_host_request(DaemonRequest::PairedHostRevoke {
                            host_id,
                            generation,
                        })
                        .await;
                }
                Err(error)
            }
        }
    }
    pub async fn paired_host_capabilities(
        &self,
    ) -> crate::paired_host::service::Result<serde_json::Value> {
        // The connect-only path checks inventory support before forwarding this query.
        self.paired_host_request(DaemonRequest::GetCapabilities)
            .await?;
        Ok(serde_json::json!({"pairedHostInventoryV1": true, "pairedDaemonProxyV1": true}))
    }
    pub async fn paired_host_read(
        &self,
        request: crate::paired_host::inventory::MigrationReceipt,
    ) -> crate::paired_host::service::Result<crate::paired_host::inventory::HostView> {
        match self
            .paired_host_request(DaemonRequest::PairedHostRead { request })
            .await?
        {
            DaemonResponse::PairedHostReadOk { host } => Ok(host),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }
    pub async fn paired_host_pair(
        &self,
        request: crate::paired_host::service::PairRequest,
    ) -> crate::paired_host::service::Result<crate::paired_host::inventory::HostView> {
        match self
            .paired_host_request(DaemonRequest::PairedHostPair { request })
            .await?
        {
            DaemonResponse::PairedHostPairOk { host } => Ok(host),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }
    pub async fn paired_host_migrate_legacy(
        &self,
        request: crate::paired_host::service::MigrationRequest,
    ) -> crate::paired_host::service::Result<crate::paired_host::inventory::MigrationReceipt> {
        match self
            .paired_host_request(DaemonRequest::PairedHostMigrateLegacy { request })
            .await?
        {
            DaemonResponse::PairedHostMigrateLegacyOk { receipt } => Ok(receipt),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }
    pub async fn paired_host_forget(
        &self,
        host_id: String,
        expected_generation: crate::scoped_contracts::Epoch,
    ) -> crate::paired_host::service::Result<()> {
        match self
            .paired_host_request(DaemonRequest::PairedHostForget {
                host_id,
                expected_generation,
            })
            .await?
        {
            DaemonResponse::PairedHostForgetOk => Ok(()),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
        }
    }

    pub async fn upgrade_binary(&self) -> Result<DaemonResponse, IpcError> {
        let own_binary_path = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        self.send_request(DaemonRequest::UpgradeBinary {
            new_binary_path: own_binary_path,
        })
        .await
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

    #[cfg(all(test, unix))]
    fn validate_existing_socket_path_for_uid(
        path: &Path,
        expected_uid: libc::uid_t,
    ) -> Result<(), IpcError> {
        crate::daemon::server::validate_runtime_socket_path_for_uid(path, expected_uid)
            .map_err(daemon_socket_trust_error)
    }

    #[cfg(unix)]
    async fn connect_socket(path: &Path) -> Result<DaemonStream, std::io::Error> {
        DaemonStream::connect(path).await
    }

    #[cfg(not(unix))]
    async fn connect_socket(path: &Path) -> Result<DaemonStream, std::io::Error> {
        let port_str = fs::read_to_string(path)?;
        let port: u16 = port_str
            .trim()
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        DaemonStream::connect(format!("127.0.0.1:{port}")).await
    }

    fn upgrade_rpc_client(&self) -> Self {
        Self {
            socket_path: self.socket_path.clone(),
            connection: Arc::new(Mutex::new(None)),
            interactive_connection: Arc::new(Mutex::new(None)),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
            upgrade_requested: Arc::clone(&self.upgrade_requested),
            spawn_lock: Arc::new(Mutex::new(())),
        }
    }

    fn maybe_trigger_upgrade_if_stale(
        &self,
        daemon_version: Option<String>,
        daemon_mtime_ms: Option<u64>,
    ) {
        let own_version = env!("CARGO_PKG_VERSION");
        let own_exe = std::env::current_exe().ok();
        let own_mtime = own_exe
            .as_ref()
            .and_then(|exe| crate::daemon::server::get_file_mtime_ms(exe));

        if !should_request_upgrade(
            daemon_version.as_deref(),
            own_version,
            daemon_mtime_ms,
            own_mtime,
        ) {
            return;
        }

        if self
            .upgrade_requested
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let temp_client = self.upgrade_rpc_client();
        let connection_slot = Arc::clone(&self.connection);
        let own_binary_path = own_exe.map(|p| p.to_string_lossy().to_string());

        tokio::spawn(async move {
            tracing::info!(
                "Daemon binary is stale (running daemon version: {daemon_version:?}, mtime: {daemon_mtime_ms:?}; own GUI version: {own_version}, mtime: {own_mtime:?}). Sending UpgradeBinary request."
            );
            match temp_client
                .send_request(DaemonRequest::UpgradeBinary {
                    new_binary_path: own_binary_path,
                })
                .await
            {
                Ok(DaemonResponse::UpgradeScheduled) => {
                    tracing::info!(
                        "Daemon upgrade scheduled successfully. Invalidating cached connection."
                    );
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    *connection_slot.lock().await = None;
                }
                Ok(DaemonResponse::UpgradeNotNeeded) => {
                    tracing::info!("Daemon reported upgrade not needed.");
                }
                Ok(DaemonResponse::UpgradeDeferred) => {
                    tracing::info!(
                        "Daemon reported upgrade deferred because active sessions are running."
                    );
                }
                Ok(DaemonResponse::UpgradeUnsupported) => {
                    tracing::info!(
                        "Daemon reported upgrade unsupported on this platform. Suppressing further upgrade requests."
                    );
                }
                Ok(other) => {
                    tracing::warn!("Unexpected response to daemon upgrade request: {other:?}");
                }
                Err(e) => {
                    tracing::warn!("Failed to send daemon upgrade request: {e}");
                }
            }
        });
    }

    async fn try_connect_existing_socket(&self) -> Result<DaemonStream, std::io::Error> {
        if fs::symlink_metadata(&self.socket_path).is_ok()
            && Self::validate_existing_socket_path(&self.socket_path).is_ok()
        {
            Self::connect_socket(&self.socket_path).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "daemon socket not found or invalid",
            ))
        }
    }

    async fn connect_or_spawn(&self) -> Result<DaemonStream, IpcError> {
        // Fast path: if daemon is already running, connect immediately without sleep or spawn lock.
        if let Ok(stream) = self.try_connect_existing_socket().await {
            return Ok(stream);
        }

        // Bounded retry loop only if the socket file already exists on disk (e.g. during rolling handover when old
        // daemon unlinks the socket and new daemon binds it). If no socket file exists, do not waste 200ms sleeping.
        if fs::symlink_metadata(&self.socket_path).is_ok() {
            for attempt in 0..5 {
                if let Ok(stream) = self.try_connect_existing_socket().await {
                    return Ok(stream);
                }
                if attempt < 4 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }

        // Single-flight spawn lock: ensures only one task attempts to spawn the daemon process at a time.
        let _spawn_guard = self.spawn_lock.lock().await;

        // Re-check after acquiring the lock: a concurrent task may have just finished spawning the daemon.
        if let Ok(stream) = self.try_connect_existing_socket().await {
            return Ok(stream);
        }

        // Launch external ferryx --daemon binary with exact bounded readiness event
        let binary_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferryx"));

        let mut child = crate::util::no_window_tokio_command(&binary_path)
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
        Self::connect_socket(&self.socket_path).await.map_err(|e| {
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
        let bytes_read = tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .map_err(|_| IpcError::new(IpcErrorCode::IoError, "Handshake timed out"))?
            .map_err(|e| {
                IpcError::new(IpcErrorCode::IoError, format!("Handshake read failed: {e}"))
            })?;
        if bytes_read == 0 || line.trim().is_empty() {
            return Err(IpcError::new(
                IpcErrorCode::IoError,
                "Handshake failed: daemon disconnected unexpectedly",
            ));
        }

        let mut hs_resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
            IpcError::new(
                IpcErrorCode::ParseError,
                format!("Handshake parse failed: {e}"),
            )
        })?;

        // If the running daemon speaks an older protocol version, it closed the connection
        // after emitting ProtocolMismatch. Reconnect with a fresh stream and perform a compatibility
        // handshake so that we can send UpgradeBinary to trigger rolling handover.
        if let DaemonResponse::ProtocolMismatch {
            expected_version, ..
        } = hs_resp
        {
            if let Ok(stream) = Self::connect_socket(&self.socket_path).await {
                let (rh, mut wh) = stream.into_split();
                let mut r = BufReader::new(rh);
                let compat_hs = DaemonRequest::Handshake {
                    version: expected_version,
                };
                if let Ok(mut json) = serde_json::to_string(&compat_hs) {
                    json.push('\n');
                    if wh.write_all(json.as_bytes()).await.is_ok() && wh.flush().await.is_ok() {
                        let mut compat_line = String::new();
                        if let Ok(Ok(n)) = tokio::time::timeout(
                            Duration::from_secs(5),
                            r.read_line(&mut compat_line),
                        )
                        .await
                        {
                            if n > 0 {
                                if let Ok(compat_resp) =
                                    serde_json::from_str::<DaemonResponse>(compat_line.trim())
                                {
                                    reader = r;
                                    write_half = wh;
                                    hs_resp = compat_resp;
                                }
                            }
                        }
                    }
                }
            }
        }

        match hs_resp {
            DaemonResponse::HandshakeOk {
                version,
                epoch,
                binary_mtime_ms,
                daemon_version,
                ..
            } => {
                if version != DAEMON_PROTOCOL_VERSION {
                    if self.upgrade_requested.load(Ordering::SeqCst) {
                        // This connection was created specifically to send an UpgradeBinary request
                        // to the running daemon. Allow the connection to proceed so rolling handover
                        // can be requested even across protocol version boundaries.
                        *self.epoch.write() = Some(epoch);
                        return Ok(ActiveConnection {
                            reader,
                            writer: write_half,
                        });
                    }
                    self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
                    return Err(daemon_protocol_mismatch_error(
                        DAEMON_PROTOCOL_VERSION,
                        version,
                    ));
                }
                *self.epoch.write() = Some(epoch);
                self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
                Ok(ActiveConnection {
                    reader,
                    writer: write_half,
                })
            }
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => Err(daemon_protocol_mismatch_error(
                expected_version,
                received_version,
            )),
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected handshake response from daemon",
            )),
        }
    }

    pub async fn send_request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        self.send_on_connection(&self.connection, req).await
    }

    /// Interactive requests (keystroke writes, resizes) use a dedicated connection so
    /// a long-running request on the shared connection (e.g. a Spawn holding it for
    /// blocking canonicalize + PTY startup) cannot stall queued keystrokes.
    async fn send_interactive_request(
        &self,
        req: DaemonRequest,
    ) -> Result<DaemonResponse, IpcError> {
        if matches!(
            req,
            DaemonRequest::RemoteWrite { .. } | DaemonRequest::RemoteResize { .. }
        ) {
            // Remote control must never wait in the local interactive queue or
            // retry ambiguous delivery. The request retains the input generation.
            let mut slot = self.interactive_connection.try_lock().map_err(|_| {
                IpcError::internal("Remote control is busy; input was not queued")
                    .with_details(serde_json::json!({"kind":"busy", "inputWritten":false}))
            })?;
            // Take the connection out of the pool while awaiting the reply: if this
            // future is cancelled mid-request, `active` is dropped, cleanly closing
            // the socket instead of leaving a half-written request pooled with its
            // unread response to corrupt the next request's framing.
            let mut conn = slot.take();
            if conn.is_none() {
                conn = Some(self.connect_and_handshake().await?);
            }
            let mut active = conn.expect("connected");
            let res = active.request(&req).await;
            return match res {
                Ok(reply) => {
                    *slot = Some(active);
                    Ok(reply)
                }
                Err(error) => Err(error.into_ipc_error(&req, true)),
            };
        }
        self.send_on_connection(&self.interactive_connection, req)
            .await
    }

    async fn send_on_connection(
        &self,
        slot: &Arc<Mutex<Option<ActiveConnection>>>,
        req: DaemonRequest,
    ) -> Result<DaemonResponse, IpcError> {
        let mut conn_guard = slot.lock().await;
        let retry_safe = request_is_retry_safe(&req);

        // Take the connection out of the slot while awaiting the reply.
        // If this future is cancelled mid-request, `conn` is dropped, closing
        // the socket and preventing an unconsumed response from corrupting the pool.
        let active = conn_guard.take();
        if let Some(mut conn) = active {
            match conn.request(&req).await {
                Ok(resp) => {
                    *conn_guard = Some(conn);
                    return Ok(resp);
                }
                Err(error) => {
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
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    /// Revokes a workspace binding in the daemon (idempotent: an unknown
    /// workspace unregisters as success). Also terminates every live PTY the
    /// daemon owns for the workspace, so remote clients cannot keep using an
    /// already-spawned session of a project the user removed.
    pub async fn unregister_workspace(&self, workspace_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::UnregisterWorkspace {
                workspace_id: workspace_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::UnregisterWorkspaceOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
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
        shell: Option<String>,
    ) -> Result<String, IpcError> {
        Ok(self
            .spawn_terminal_with_startup(
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                shell,
                None,
            )
            .await?
            .session_id)
    }

    pub async fn spawn_terminal_with_startup(
        &self,
        client_request_id: String,
        workspace_id: String,
        worktree: Option<WorktreeIdentity>,
        cwd: Option<String>,
        cols: u16,
        rows: u16,
        shell: Option<String>,
        startup: Option<TerminalStartup>,
    ) -> Result<DaemonSpawnResult, IpcError> {
        let resp = self
            .send_request(DaemonRequest::Spawn {
                client_request_id,
                workspace_id,
                worktree,
                cwd,
                cols,
                rows,
                shell,
                startup,
            })
            .await?;

        match resp {
            DaemonResponse::SpawnOk {
                session_id,
                epoch,
                session,
            } => Ok(DaemonSpawnResult {
                session_id,
                epoch,
                session,
            }),
            DaemonResponse::AgentResumeInvalid { message } => Err(IpcError::new(
                IpcErrorCode::AgentResumeInvalid,
                "Agent resume startup validation failed",
            )
            .with_details(serde_json::json!({
                "reason": message,
            }))),
            DaemonResponse::AgentSessionConflict {
                agent_type,
                provider_key,
                provider_id,
                existing_session_id,
            } => Err(IpcError::new(
                IpcErrorCode::AgentSessionConflict,
                "Agent provider session is already owned by another terminal",
            )
            .with_details(serde_json::json!({
                "agentType": agent_type,
                "providerKey": provider_key,
                "providerId": provider_id,
                "existingSessionId": existing_session_id,
            }))),
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => Err(daemon_protocol_mismatch_error(
                expected_version,
                received_version,
            )),
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn discover_agent_session(
        &self,
        session_id: &str,
        agent_type: &str,
    ) -> Result<Option<String>, IpcError> {
        match self
            .send_request(DaemonRequest::DiscoverAgentSession {
                session_id: session_id.to_string(),
                agent_type: agent_type.to_string(),
            })
            .await?
        {
            DaemonResponse::DiscoverAgentSessionOk {
                provider_session_id,
            } => Ok(provider_session_id),
            DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
            _ => Err(IpcError::internal(
                "Unexpected daemon agent-session discovery response",
            )),
        }
    }

    pub async fn reset_agent_state(&self, session_id: &str) -> Result<(), IpcError> {
        match self
            .send_request(DaemonRequest::ResetAgentState {
                session_id: session_id.to_string(),
            })
            .await?
        {
            DaemonResponse::ResetAgentStateOk => Ok(()),
            DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
            _ => Err(IpcError::internal(
                "Unexpected daemon reset agent state response",
            )),
        }
    }

    /// Streams desktop-directed remote events (the gateway lives in the daemon,
    /// so this is the only path a remote-issued request has to reach the GUI).
    /// The returned receiver closes when the daemon goes away.
    pub async fn subscribe_remote_events(
        &self,
    ) -> Result<mpsc::Receiver<DaemonRemoteEvent>, IpcError> {
        let stream = self.connect_or_spawn().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        for req in [
            DaemonRequest::Handshake {
                version: DAEMON_PROTOCOL_VERSION,
            },
            DaemonRequest::SubscribeRemoteEvents,
        ] {
            let mut json = serde_json::to_string(&req).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::ParseError,
                    format!("Remote event subscription serialization failed: {e}"),
                )
            })?;
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Remote event subscription write failed: {e}"),
                )
            })?;
            write_half.flush().await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Remote event subscription flush failed: {e}"),
                )
            })?;

            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Remote event subscription read failed: {e}"),
                )
            })?;
            if bytes_read == 0 {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    "Remote event subscription failed: daemon disconnected",
                ));
            }
            let resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::ParseError,
                    format!("Remote event subscription parse failed: {e}"),
                )
            })?;
            match resp {
                DaemonResponse::HandshakeOk {
                    version,
                    binary_mtime_ms,
                    daemon_version,
                    ..
                } if version == DAEMON_PROTOCOL_VERSION => {
                    self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
                }
                DaemonResponse::SubscribeRemoteEventsOk => {}
                DaemonResponse::Error { message, .. } => {
                    return Err(IpcError::new(IpcErrorCode::InternalError, message));
                }
                other => {
                    return Err(IpcError::new(
                        IpcErrorCode::InternalError,
                        format!("Unexpected daemon response for remote events: {other:?}"),
                    ));
                }
            }
        }

        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                if let Ok(event) = serde_json::from_str::<DaemonRemoteEvent>(line.trim()) {
                    if tx.send(event).await.is_err() {
                        break;
                    }
                }
                line.clear();
            }
        });

        Ok(rx)
    }

    pub async fn subscribe_dag(
        &self,
        workspace_id: &str,
        project_path: &str,
    ) -> Result<mpsc::Receiver<DaemonStreamMessage<'static>>, IpcError> {
        self.subscribe_dag_bound(workspace_id, project_path, None)
            .await
    }

    /// Subscribes with an optional paired binding. With a binding the daemon streams
    /// from the authenticated remote host rather than scanning `project_path` locally.
    pub async fn subscribe_dag_bound(
        &self,
        workspace_id: &str,
        project_path: &str,
        paired: Option<crate::daemon::protocol::PairedDagBinding>,
    ) -> Result<mpsc::Receiver<DaemonStreamMessage<'static>>, IpcError> {
        let stream = self.connect_or_spawn().await?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        for req in [
            DaemonRequest::Handshake {
                version: DAEMON_PROTOCOL_VERSION,
            },
            DaemonRequest::SubscribeDag {
                workspace_id: workspace_id.to_string(),
                project_path: project_path.to_string(),
                paired: paired.clone(),
            },
        ] {
            let mut json = serde_json::to_string(&req).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::ParseError,
                    format!("DAG subscription serialization failed: {e}"),
                )
            })?;
            json.push('\n');
            write_half.write_all(json.as_bytes()).await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("DAG subscription write failed: {e}"),
                )
            })?;
            write_half.flush().await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("DAG subscription flush failed: {e}"),
                )
            })?;

            let mut line = String::new();
            let bytes_read = reader.read_line(&mut line).await.map_err(|e| {
                IpcError::new(
                    IpcErrorCode::IoError,
                    format!("DAG subscription read failed: {e}"),
                )
            })?;
            if bytes_read == 0 {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    "DAG subscription failed: daemon disconnected",
                ));
            }
            let resp: DaemonResponse = serde_json::from_str(line.trim()).map_err(|e| {
                IpcError::new(
                    IpcErrorCode::ParseError,
                    format!("DAG subscription parse failed: {e}"),
                )
            })?;
            match resp {
                DaemonResponse::HandshakeOk {
                    version,
                    binary_mtime_ms,
                    daemon_version,
                    ..
                } if version == DAEMON_PROTOCOL_VERSION => {
                    self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
                }
                DaemonResponse::SubscribeDagOk => {}
                DaemonResponse::Error { message, .. } => {
                    return Err(IpcError::new(IpcErrorCode::InternalError, message));
                }
                other => {
                    return Err(IpcError::new(
                        IpcErrorCode::InternalError,
                        format!("Unexpected daemon response for DAG subscription: {other:?}"),
                    ));
                }
            }
        }

        let (tx, rx) = mpsc::channel(64);
        let unsubscribe = DaemonRequest::UnsubscribeDag {
            workspace_id: workspace_id.to_string(),
            project_path: project_path.to_string(),
        };
        tokio::spawn(async move {
            let mut line = String::new();
            // Consumer-driven cancellation must not wait for the next stream frame:
            // `tx.closed()` resolves as soon as the receiver is dropped, even while the
            // socket is idle.
            let cancelled = loop {
                tokio::select! {
                    biased;
                    _ = tx.closed() => break true,
                    read = reader.read_line(&mut line) => {
                        match read {
                            Ok(0) | Err(_) => break false,
                            Ok(_) => {}
                        }
                        if let Ok(msg) =
                            serde_json::from_str::<DaemonStreamMessage<'static>>(line.trim())
                        {
                            if tx.send(msg).await.is_err() {
                                break true;
                            }
                        }
                        line.clear();
                    }
                }
            };

            if cancelled {
                if let Ok(mut json) = serde_json::to_string(&unsubscribe) {
                    json.push('\n');
                    let _ = write_half.write_all(json.as_bytes()).await;
                    let _ = write_half.flush().await;
                }
            }
            // Dropping both halves closes the owned connection so the daemon side
            // observes EOF even if the unsubscribe write failed.
            drop(write_half);
            drop(reader);
        });

        Ok(rx)
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
            DaemonResponse::HandshakeOk {
                version,
                binary_mtime_ms,
                daemon_version,
                ..
            } if version == DAEMON_PROTOCOL_VERSION => {
                self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
            }
            DaemonResponse::HandshakeOk { version, .. } => {
                return Err(daemon_protocol_mismatch_error(
                    DAEMON_PROTOCOL_VERSION,
                    version,
                ));
            }
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => {
                return Err(daemon_protocol_mismatch_error(
                    expected_version,
                    received_version,
                ));
            }
            DaemonResponse::Error { message, .. } => {
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
                pty_cols,
                pty_rows,
                history_segments,
                remote_generation,
            } => {
                let segments = history_segments
                    .into_iter()
                    .map(|wire| crate::terminal::output_hub::HistorySegment {
                        cols: wire.cols,
                        rows: wire.rows,
                        bytes: wire.bytes.to_vec(),
                    })
                    .collect();

                let (tx, rx) = mpsc::channel(256);
                let task = tokio::spawn(async move {
                    let _keepalive = write_half;
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
                    history_segments: segments,
                    pty_cols,
                    pty_rows,
                    remote_generation,
                    messages: rx,
                    stream_task: task,
                })
            }
            DaemonResponse::Error {
                message,
                code,
                details,
            } => Err(parse_attach_error_response(
                message, code, details, session_id,
            )),
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response for attach",
            )),
        }
    }

    pub async fn remote_session_status(
        &self,
        session_id: &str,
    ) -> Result<DaemonResponse, IpcError> {
        self.send_request(DaemonRequest::RemoteSessionDetails {
            session_id: session_id.into(),
        })
        .await
    }

    pub async fn retry_remote_session(&self, session_id: &str) -> Result<DaemonResponse, IpcError> {
        self.send_request(DaemonRequest::RetryRemoteSession {
            session_id: session_id.into(),
        })
        .await
    }

    // Never infer a remote generation after an await. Generation-less platform
    // callbacks are local-only; remote panes must supply their observed generation.
    async fn require_local_control(&self, session_id: &str) -> Result<(), IpcError> {
        match self.remote_session_status(session_id).await? {
            DaemonResponse::RemoteSessionDetailsOk { details: None, .. } => Ok(()),
            DaemonResponse::RemoteSessionDetailsOk {
                details: Some(_), ..
            } => Err(IpcError::internal(
                "Remote input requires the generation observed at input time",
            )
            .with_details(serde_json::json!({"kind":"staleGeneration", "inputWritten":false}))),
            DaemonResponse::RemoteSessionError { failure } => {
                Err(IpcError::internal(failure.to_string()).with_details(
                    serde_json::to_value(failure)
                        .map_err(|error| IpcError::internal(error.to_string()))?,
                ))
            }
            DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
            _ => Err(IpcError::internal(
                "Unexpected remote session classification response",
            )),
        }
    }

    pub async fn write_terminal_at_generation(
        &self,
        session_id: &str,
        generation: Option<u64>,
        data: Vec<u8>,
    ) -> Result<(), IpcError> {
        match generation {
            Some(generation) => crate::ipc::terminal::remote_control_result(
                self.send_interactive_request(DaemonRequest::RemoteWrite {
                    session_id: session_id.into(),
                    generation,
                    data,
                })
                .await?,
            ),
            None => self.write_terminal(session_id, data).await,
        }
    }

    pub async fn resize_terminal_at_generation(
        &self,
        session_id: &str,
        generation: Option<u64>,
        cols: u16,
        rows: u16,
    ) -> Result<(), IpcError> {
        match generation {
            Some(generation) => crate::ipc::terminal::remote_control_result(
                self.send_interactive_request(DaemonRequest::RemoteResize {
                    session_id: session_id.into(),
                    generation,
                    cols,
                    rows,
                })
                .await?,
            ),
            None => self.resize_terminal(session_id, cols, rows).await,
        }
    }

    pub async fn write_terminal(&self, session_id: &str, data: Vec<u8>) -> Result<(), IpcError> {
        self.require_local_control(session_id).await?;
        let resp = self
            .send_interactive_request(DaemonRequest::Write {
                session_id: session_id.to_string(),
                data,
            })
            .await?;

        match resp {
            DaemonResponse::WriteOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
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
        // The native surface dispatcher is generation-less. Geometry is not payload
        // input: resolving the remote's CURRENT generation here is safe (the daemon
        // applies it under its live connection), unlike generation-less writes which
        // must be rejected for remote sessions.
        match self.remote_session_status(session_id).await? {
            DaemonResponse::RemoteSessionDetailsOk {
                details: Some(details),
                ..
            } => {
                return crate::ipc::terminal::remote_control_result(
                    self.send_interactive_request(DaemonRequest::RemoteResize {
                        session_id: session_id.into(),
                        generation: details.generation,
                        cols,
                        rows,
                    })
                    .await?,
                );
            }
            DaemonResponse::RemoteSessionError { failure } => {
                return Err(IpcError::internal(failure.to_string()).with_details(
                    serde_json::to_value(failure)
                        .map_err(|error| IpcError::internal(error.to_string()))?,
                ));
            }
            DaemonResponse::Error { message, .. } => return Err(IpcError::internal(message)),
            _ => {}
        }
        let resp = self
            .send_interactive_request(DaemonRequest::Resize {
                session_id: session_id.to_string(),
                cols,
                rows,
            })
            .await?;

        match resp {
            DaemonResponse::ResizeOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn close_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        if session_id.starts_with("daemon-session:") {
            // P12 (round 2): a failed descriptor lookup must not silently skip
            // the remote close — local close is then not proof that the remote
            // PTY terminated, so surface uncertainty instead of masking it.
            match self
                .paired_terminal_descriptor(session_id.to_string())
                .await
            {
                Ok(Some(descriptor)) => {
                    let cleanup_req_id = uuid::Uuid::new_v4().to_string();
                    let close_op = crate::paired_host::client::OperationRequest {
                        host_id: descriptor.host_id.clone(),
                        generation: descriptor.generation,
                        operation: crate::paired_host::client::Operation::CloseSession {
                            session_id: descriptor.target.session_id.clone(),
                            request: crate::remote::machine_protocol::CloseSessionRequest {
                                request_id: cleanup_req_id.clone(),
                                daemon_epoch: descriptor.target.daemon_epoch.clone(),
                            },
                        },
                    };
                    match self.paired_host_operation(close_op).await {
                        Ok(_) => {}
                        Err(ref e) if e.code == "SESSION_NOT_FOUND" => {}
                        Err(ref e)
                            if e.ambiguous
                                || matches!(
                                    e.code.as_str(),
                                    "TIMEOUT" | "OPERATION_OUTCOME_UNKNOWN"
                                ) =>
                        {
                            // P12: an ambiguous close is not a success. Poll the operation
                            // journal a bounded number of times for a definitive outcome;
                            // if it stays unknown, refuse to acknowledge local-only success.
                            let mut determined = false;
                            for _attempt in 0..5 {
                                let journal_req = crate::paired_host::client::OperationRequest {
                                    host_id: descriptor.host_id.clone(),
                                    generation: descriptor.generation,
                                    operation: crate::paired_host::client::Operation::Operation {
                                        request_id: cleanup_req_id.clone(),
                                    },
                                };
                                match self.paired_host_operation(journal_req).await {
                                Ok(op_resp) => match op_resp.result {
                                    crate::paired_host::client::OperationResult::Operation(
                                        crate::remote::machine_protocol::Operation::Completed { outcome, .. },
                                    ) => match outcome {
                                        crate::remote::machine_protocol::OperationOutcome::Error { error } => {
                                            return Err(IpcError::new(
                                                IpcErrorCode::Custom("REMOTE_CLOSE_FAILED".to_string()),
                                                format!("Remote session close failed: {}", error.message),
                                            ));
                                        }
                                        _ => {
                                            determined = true;
                                            break;
                                        }
                                    },
                                    _ => {}
                                },
                                Err(journal_err) if journal_err.code == "SESSION_NOT_FOUND" => {
                                    // The paired host does not know the host/session at all:
                                    // the remote session is gone.
                                    determined = true;
                                    break;
                                }
                                // P12 (round 2): OPERATION_NOT_FOUND means the close
                                // request never committed remotely — i.e. the remote
                                // session may still be running. It is NOT proof of a
                                // closed session, so stay unresolved.
                                Err(_) => {}
                            }
                                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            }
                            if !determined {
                                return Err(IpcError::new(
                                IpcErrorCode::Custom("REMOTE_CLOSE_UNCERTAIN".to_string()),
                                "Remote paired-session close outcome is unknown; the session may still be running on the paired host",
                            )
                            .with_details(serde_json::json!({
                                "cleanupRequestId": cleanup_req_id,
                                "hostId": descriptor.host_id,
                                "generation": descriptor.generation,
                                "remoteSessionId": descriptor.target.session_id,
                                "remoteCloseUnknown": true,
                            })));
                            }
                        }
                        Err(e) => {
                            // P12: a definitive remote failure must not be masked by the
                            // local daemon close succeeding afterwards.
                            return Err(IpcError::new(
                                IpcErrorCode::Custom("REMOTE_CLOSE_FAILED".to_string()),
                                format!("Remote paired-session close failed: {}", e.code),
                            )
                            .with_details(serde_json::json!({
                                "hostId": descriptor.host_id,
                                "generation": descriptor.generation,
                                "remoteSessionId": descriptor.target.session_id,
                                "cause": e.code,
                            })));
                        }
                    }
                }
                Ok(None) => {}
                Err(descriptor_err) => {
                    return Err(IpcError::new(
                        IpcErrorCode::Custom("REMOTE_CLOSE_UNCERTAIN".to_string()),
                        format!(
                            "Paired descriptor lookup failed while closing; the remote session may still be running: {}",
                            descriptor_err.code
                        ),
                    )
                    .with_details(serde_json::json!({
                        "sessionId": session_id,
                        "cause": descriptor_err.code,
                        "remoteCloseUnknown": true,
                    })));
                }
            }
        }

        let resp = self
            .send_request(DaemonRequest::Close {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::CloseOk => Ok(()),
            DaemonResponse::Error {
                message,
                code,
                details,
            } => {
                if code.as_deref() == Some("SESSION_NOT_FOUND") {
                    let det = details.unwrap_or_else(|| {
                        serde_json::json!({
                            "source": "daemon_close",
                            "kind": "session_not_found",
                            "sessionId": session_id,
                        })
                    });
                    Err(IpcError::new(IpcErrorCode::SessionNotFound, message).with_details(det))
                } else if let Some(ref c) = code {
                    let ipc_code = IpcErrorCode::from_code_str(c);
                    let mut err = IpcError::new(ipc_code, message);
                    if let Some(d) = details {
                        err = err.with_details(d);
                    }
                    Err(err)
                } else {
                    Err(IpcError::new(IpcErrorCode::InternalError, message))
                }
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn detach_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        if !session_id.starts_with("daemon-session:") {
            return Err(IpcError::new(
                IpcErrorCode::InvalidArgument,
                "Detach is supported only for paired sessions",
            ));
        }
        self.paired_terminal_detach(session_id.to_string())
            .await
            .map_err(|e| IpcError::new(IpcErrorCode::InternalError, e.code))
    }

    pub async fn hibernate_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Hibernate {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::HibernateOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn suspend_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Suspend {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::SuspendOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn resume_terminal(&self, session_id: &str) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::Resume {
                session_id: session_id.to_string(),
            })
            .await?;

        match resp {
            DaemonResponse::ResumeOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::RemotePairingCodeOk { code, .. } => Ok(code),
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }

    pub async fn remote_create_pairing_code_detailed(
        &self,
        permission: Option<DevicePermission>,
    ) -> Result<(String, Option<String>, Option<String>, Option<String>), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteCreatePairingCode { permission })
            .await?;
        match resp {
            DaemonResponse::RemotePairingCodeOk {
                code,
                pairing_token,
                machine_id,
                relay_url,
            } => Ok((code, pairing_token, machine_id, relay_url)),
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
        self.remote_set_active_selection_with_ssh_store(selection, None)
            .await
    }

    pub async fn remote_set_active_selection_with_ssh_store(
        &self,
        selection: Option<RemoteActiveDesktopSelection>,
        ssh_store_path: Option<PathBuf>,
    ) -> Result<(), IpcError> {
        let resp = self
            .send_request(DaemonRequest::RemoteSetActiveSelection {
                selection,
                ssh_store_path,
            })
            .await?;
        match resp {
            DaemonResponse::RemoteSetActiveSelectionOk => Ok(()),
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
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
            DaemonResponse::Error { message, .. } => {
                Err(IpcError::new(IpcErrorCode::InternalError, message))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected daemon response",
            )),
        }
    }
}

#[cfg(all(test, unix))]
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

    #[test]
    fn test_p09_outer_budget_covers_all_underlying_phase_budgets() {
        // Mutation performs: capabilities (45) + journal (45) + mutation (60) = 150s minimum before margin
        let mutation_phases_sum = DaemonClient::PAIRED_CAPABILITIES_BUDGET_SECS
            + DaemonClient::PAIRED_JOURNAL_BUDGET_SECS
            + DaemonClient::PAIRED_MUTATION_BUDGET_SECS;
        assert!(
            DaemonClient::PAIRED_MUTATION_OUTER_TIMEOUT.as_secs() >= mutation_phases_sum,
            "Mutation outer timeout ({}s) must be >= phase sum ({}s)",
            DaemonClient::PAIRED_MUTATION_OUTER_TIMEOUT.as_secs(),
            mutation_phases_sum
        );

        // Query performs: capabilities (45) + journal (45) + query (45) = 135s minimum before margin
        let query_phases_sum = DaemonClient::PAIRED_CAPABILITIES_BUDGET_SECS
            + DaemonClient::PAIRED_JOURNAL_BUDGET_SECS
            + DaemonClient::PAIRED_QUERY_BUDGET_SECS;
        assert!(
            DaemonClient::PAIRED_QUERY_OUTER_TIMEOUT.as_secs() >= query_phases_sum,
            "Query outer timeout ({}s) must be >= phase sum ({}s)",
            DaemonClient::PAIRED_QUERY_OUTER_TIMEOUT.as_secs(),
            query_phases_sum
        );

        // Reattach performs: capabilities (45) + session query (45) + ticket (45) = 135s minimum before margin
        let reattach_phases_sum = DaemonClient::PAIRED_CAPABILITIES_BUDGET_SECS
            + DaemonClient::PAIRED_QUERY_BUDGET_SECS
            + DaemonClient::PAIRED_TICKET_BUDGET_SECS;
        let reattach_req = DaemonRequest::PairedTerminalReattach {
            descriptor: crate::terminal::paired_daemon::Descriptor {
                host_id: "host-1".into(),
                generation: crate::scoped_contracts::Epoch(1),
                target: crate::remote::machine_protocol::RemoteTerminalTarget {
                    machine_id: "m-1".into(),
                    daemon_epoch: crate::scoped_contracts::Epoch(1),
                    session_id: "s-1".into(),
                },
                after_sequence: None,
            },
        };
        let reattach_timeout = DaemonClient::outer_deadline_for_request(&reattach_req);
        assert!(
            reattach_timeout.as_secs() >= reattach_phases_sum,
            "Reattach outer timeout ({}s) must be >= phase sum ({}s)",
            reattach_timeout.as_secs(),
            reattach_phases_sum
        );
    }

    #[test]
    fn p08_upgrade_rpc_inherits_admission() {
        let client = DaemonClient::new_with_socket(PathBuf::from("unused-p08.sock"));
        client.upgrade_requested.store(true, Ordering::SeqCst);
        let rpc = client.upgrade_rpc_client();
        assert!(
            Arc::ptr_eq(&client.upgrade_requested, &rpc.upgrade_requested),
            "internal upgrade RPC must share admission with its originating client"
        );
        assert!(
            rpc.upgrade_requested
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err(),
            "an internal stale handshake must not admit another upgrade"
        );
    }

    #[tokio::test]
    async fn test_p09_paired_host_request_timeout_budget_and_ambiguity() {
        tokio::time::pause();

        let dir = tempdir().unwrap();
        let socket = dir.path().join("p09.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let reply = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 1,
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&reply).unwrap()).as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let caps_reply = DaemonResponse::CapabilitiesOk {
                capabilities: vec!["pairedHostInventoryV1".into()],
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&caps_reply).unwrap()).as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            tokio::time::sleep(Duration::from_secs(300)).await;
        });

        let client = DaemonClient::new_with_socket(socket);
        let req_id = "p09-test-request-id".to_string();
        let op_req = crate::paired_host::client::OperationRequest {
            host_id: "host-1".into(),
            generation: crate::scoped_contracts::Epoch(1),
            operation: crate::paired_host::client::Operation::CreateSession {
                request: crate::remote::machine_protocol::CreateSessionRequest {
                    request_id: req_id.clone(),
                    workspace_id: "ws-1".into(),
                    worktree: None,
                    cols: 80,
                    rows: 24,
                    inherit_from_session_id: None,
                    cwd_relative: None,
                    startup: crate::remote::machine_protocol::Startup::Shell,
                },
            },
        };

        let op_task = tokio::spawn(async move { client.paired_host_operation(op_req).await });

        // Give the task a moment to connect and do handshake before advancing time
        tokio::task::yield_now().await;

        // Advance time by 36 seconds:
        // Pre-fix: 35s flat timeout has already fired and failed with PAIRED_HOST_UNAVAILABLE.
        // Post-fix: Outer deadline exceeds 120s (inner 60 + 45 + margin), so it is STILL pending at 36s.
        tokio::time::advance(Duration::from_secs(36)).await;
        tokio::task::yield_now().await;

        assert!(!op_task.is_finished(), "Mutation timed out prematurely at <= 36s; outer deadline must exceed inner budget (> 120s)");

        tokio::time::advance(Duration::from_secs(150)).await;
        let err = op_task.await.unwrap().unwrap_err();

        assert_eq!(
            err.code, "TIMEOUT",
            "Expected TIMEOUT error code on deadline expiry, got {}",
            err.code
        );
        assert!(
            err.ambiguous,
            "Ambiguous must be true when mutation transport deadline expires"
        );
        assert_eq!(err.request_id, Some(req_id));

        server.abort();
    }

    #[tokio::test]
    async fn ssh_reconnect_safety_desktop_missing_generation_never_uses_legacy_write() {
        let dir = tempdir().unwrap();
        let socket = dir.path().join("desktop.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let reply = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 1,
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&reply).unwrap()).as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let request: DaemonRequest = serde_json::from_str(line.trim()).unwrap();
            let reply = DaemonResponse::RemoteSessionError {
                failure: crate::terminal::remote::RemoteFailure {
                    kind: crate::terminal::remote::RemoteFailureKind::Disconnected,
                    message: "offline".into(),
                },
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&reply).unwrap()).as_bytes())
                .await
                .unwrap();
            request
        });
        let client = DaemonClient::new_with_socket(socket);
        client
            .write_terminal("remote", b"never replay".to_vec())
            .await
            .expect_err("fail closed");
        let request = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(request, DaemonRequest::RemoteSessionDetails { .. }),
            "must classify before legacy dispatch: {request:?}"
        );
    }

    #[tokio::test]
    async fn ssh_reconnect_safety_desktop_remote_control_is_not_queued() {
        let client =
            DaemonClient::new_with_socket(std::path::PathBuf::from("/unused-desktop-test.sock"));
        let _busy = client.interactive_connection.lock().await;
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            client.write_terminal_at_generation("remote", Some(7), b"key".to_vec()),
        )
        .await
        .expect("remote input must reject immediately, not wait behind control")
        .unwrap_err();
        assert_eq!(error.details.unwrap()["kind"], "busy");
    }

    #[tokio::test]
    #[cfg(feature = "native-terminal")]
    async fn ssh_reconnect_safety_desktop_encoded_input_retains_generation_and_typed_failure() {
        use crate::ipc::native_terminal::{
            encode_attached_native_input, encode_attached_native_mouse,
            encode_attached_native_paste,
        };
        use crate::native_terminal::surface_host::NativeTerminalSurfaceHostState;
        use crate::native_terminal::{MouseEvent, NativeTerminalInput};
        let state = NativeTerminalSurfaceHostState::default();
        let (_tx, rx) = mpsc::channel(4);
        state
            .attach_daemon_attachment::<tauri::test::MockRuntime>(
                "remote",
                DaemonAttachment {
                    session_id: "remote".into(),
                    epoch: 1,
                    start_sequence: None,
                    end_sequence: None,
                    gap: None,
                    history: bytes::Bytes::from_static(b"\x1b[?1000h\x1b[?1006h"),
                    history_segments: vec![],
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages: rx,
                    stream_task: tokio::spawn(std::future::pending()),
                },
                None,
            )
            .unwrap();
        let key = serde_json::from_value::<NativeTerminalInput>(serde_json::json!({"keyEvent":{
            "key":"Enter","action":"Press","modifiers":{"shift":false,"ctrl":false,"alt":false,"superKey":false,"capsLock":false,"numLock":false},"utf8":null
        }})).unwrap();
        let mouse: MouseEvent = serde_json::from_value(serde_json::json!({
            "action":"Press","button":"Right","position":{"x":10.0,"y":10.0},
            "size":{"screenWidth":800,"screenHeight":480,"cellWidth":10,"cellHeight":20,"paddingTop":0,"paddingBottom":0,"paddingLeft":0,"paddingRight":0},
            "modifiers":{"shift":false,"ctrl":false,"alt":false,"superKey":false,"capsLock":false,"numLock":false}
        })).unwrap();
        let payloads = vec![
            encode_attached_native_input(
                &state,
                "remote",
                &NativeTerminalInput::Text {
                    text: "IME commit".into(),
                },
            )
            .unwrap(),
            encode_attached_native_input(&state, "remote", &key).unwrap(),
            encode_attached_native_paste(&state, "remote", "paste\nline").unwrap(),
            encode_attached_native_mouse(&state, "remote", &mouse).unwrap(),
        ];
        assert!(payloads.iter().all(|bytes| !bytes.is_empty()));
        let dir = tempdir().unwrap();
        let socket = dir.path().join("encoded.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let expected = payloads.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let handshake = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 1,
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
            };
            write
                .write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes())
                .await
                .unwrap();
            for bytes in expected {
                line.clear();
                reader.read_line(&mut line).await.unwrap();
                match serde_json::from_str::<DaemonRequest>(line.trim()).unwrap() {
                    DaemonRequest::RemoteWrite {
                        session_id,
                        generation,
                        data,
                    } => {
                        assert_eq!(session_id, "remote");
                        assert_eq!(generation, 7);
                        assert_eq!(data, bytes);
                    }
                    other => panic!("generation bypass: {other:?}"),
                }
                let reply = serde_json::json!({"type":"remoteSessionError","failure":{"kind":"staleGeneration","message":"generation is now 8"}});
                write
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::RemoteResize {
                    generation: 7,
                    cols: 100,
                    rows: 30,
                    ..
                }
            ));
            write.write_all(b"{\"type\":\"remoteSessionError\",\"failure\":{\"kind\":\"disconnected\",\"message\":\"offline\"}}\n").await.unwrap();
        });
        let client = DaemonClient::new_with_socket(socket);
        for bytes in payloads {
            let error = client
                .write_terminal_at_generation("remote", Some(7), bytes)
                .await
                .unwrap_err();
            assert_eq!(error.details.unwrap()["kind"], "staleGeneration");
        }
        let error = client
            .resize_terminal_at_generation("remote", Some(7), 100, 30)
            .await
            .unwrap_err();
        assert_eq!(error.details.unwrap()["kind"], "disconnected");
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        state.close_session("remote");
    }

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

    #[tokio::test]
    async fn test_client_handshake_read_times_out() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test_handshake_timeout.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let (handshake_seen_tx, handshake_seen_rx) = oneshot::channel();
        let (release_server_tx, release_server_rx) = oneshot::channel::<()>();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, _write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Handshake { .. }
            ));
            let _ = handshake_seen_tx.send(());
            let _ = release_server_rx.await;
        });

        let client = DaemonClient::new_with_socket(socket_path);
        let request_task =
            tokio::spawn(async move { client.send_request(DaemonRequest::Ping).await });

        tokio::time::timeout(Duration::from_secs(1), handshake_seen_rx)
            .await
            .expect("server must receive the handshake")
            .expect("server must signal handshake receipt");

        let result = tokio::time::timeout(Duration::from_secs(6), request_task)
            .await
            .expect("client must stop waiting after the five-second handshake timeout")
            .expect("client request task must not panic");
        let error = result.expect_err("silent daemon must fail the handshake");
        assert_eq!(error.code, IpcErrorCode::IoError);
        assert_eq!(error.message, "Handshake timed out");

        let _ = release_server_tx.send(());
        server_task.await.unwrap();
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
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
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
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
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
                None,
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
                ..
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
                None,
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
                None,
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
            ..Default::default()
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
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
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
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
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

        let error =
            DaemonClient::validate_existing_socket_path_for_uid(&symlinked_socket, current_uid)
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

    #[test]
    fn test_attach_session_not_found_exact_match() {
        let err = parse_attach_error_response(
            "Session 'test-id' not found".to_string(),
            None,
            None,
            "test-id",
        );
        assert_eq!(err.code, IpcErrorCode::SessionNotFound);
        assert_eq!(err.message, "Session 'test-id' not found");
        let details = err.details.expect("expected details");
        assert_eq!(
            details.get("source").and_then(|v| v.as_str()),
            Some("daemon_attach")
        );
        assert_eq!(
            details.get("kind").and_then(|v| v.as_str()),
            Some("session_not_found")
        );
        assert_eq!(
            details.get("sessionId").and_then(|v| v.as_str()),
            Some("test-id")
        );

        let pty_err = parse_attach_error_response(
            "PTY session 'test-id' not found".to_string(),
            None,
            None,
            "test-id",
        );
        assert_eq!(pty_err.code, IpcErrorCode::SessionNotFound);
        assert_eq!(pty_err.message, "PTY session 'test-id' not found");
        let pty_details = pty_err.details.expect("expected details");
        assert_eq!(
            pty_details.get("source").and_then(|v| v.as_str()),
            Some("daemon_attach")
        );
        assert_eq!(
            pty_details.get("kind").and_then(|v| v.as_str()),
            Some("session_not_found")
        );
        assert_eq!(
            pty_details.get("sessionId").and_then(|v| v.as_str()),
            Some("test-id")
        );
    }

    #[test]
    fn test_attach_session_not_found_wrong_id() {
        let err = parse_attach_error_response(
            "Session 'other-id' not found".to_string(),
            None,
            None,
            "test-id",
        );
        assert_eq!(err.code, IpcErrorCode::InternalError);
        assert_eq!(err.message, "Session 'other-id' not found");
        assert!(err.details.is_none());
    }

    #[test]
    fn test_attach_arbitrary_error_not_session_not_found() {
        let err =
            parse_attach_error_response("Internal server error".to_string(), None, None, "test-id");
        assert_eq!(err.code, IpcErrorCode::InternalError);
        assert_eq!(err.message, "Internal server error");
        assert!(err.details.is_none());
    }

    #[test]
    fn test_attach_malformed_overlapping_not_found_message_does_not_panic() {
        // "Session ' not found" has length 19; prefix is 9, suffix is 10.
        // In the old code, slicing [9..19-10] was [9..9] or would panic on shorter overlapping messages.
        let err =
            parse_attach_error_response("Session ' not found".to_string(), None, None, "test-id");
        assert_eq!(err.code, IpcErrorCode::InternalError);
        assert!(err.details.is_none());
    }

    #[test]
    fn test_p07_attach_error_with_structured_session_not_found_ignores_divergent_message() {
        // P07 regression: When daemon sends SESSION_NOT_FOUND via structured wire response, but message
        // text is divergent (e.g. localized or from different PTY backend),
        // it must classify as SessionNotFound, NOT InternalError.
        let err = parse_attach_error_response(
            "Terminal process terminated unexpectedly".to_string(),
            Some("SESSION_NOT_FOUND".to_string()),
            Some(serde_json::json!({
                "source": "daemon_attach",
                "kind": "session_not_found",
                "sessionId": "test-id",
            })),
            "test-id",
        );
        assert_eq!(err.code, IpcErrorCode::SessionNotFound);
        assert_ne!(err.code, IpcErrorCode::InternalError);
        assert_eq!(err.message, "Terminal process terminated unexpectedly");
        let details = err.details.expect("expected details");
        assert_eq!(
            details.get("source").and_then(|v| v.as_str()),
            Some("daemon_attach")
        );
        assert_eq!(
            details.get("sessionId").and_then(|v| v.as_str()),
            Some("test-id")
        );
    }

    #[test]
    fn worktree_mutations_are_never_blindly_resent() {
        let worktree = crate::worktree::WorktreeIdentity {
            ws_id: "ws".into(),
            slug: "feature".into(),
        };
        for request in [
            DaemonRequest::CreateWorktree {
                workspace_id: "ws".into(),
                worktree: worktree.clone(),
                base_ref: None,
            },
            DaemonRequest::DeleteWorktree {
                workspace_id: "ws".into(),
                worktree,
                delete_branch: false,
                destructive: false,
            },
        ] {
            assert!(!request_is_retry_safe(&request));
            let error = ambiguous_delivery_error(&request, &IpcError::internal("lost reply"));
            assert_eq!(
                error.details.unwrap()["requestType"],
                request_type_name(&request)
            );
        }
    }

    #[test]
    fn test_remote_set_active_selection_is_retry_safe() {
        let req = DaemonRequest::RemoteSetActiveSelection {
            ssh_store_path: None,
            selection: Some(RemoteActiveDesktopSelection {
                workspace_id: Some("ws".into()),
                worktree_slug: None,
                worktree_label: None,
                session_id: None,
                tab_id: None,
                terminal_tabs: Vec::new(),
                attention_inventory: Vec::new(),
            }),
        };
        assert!(request_is_retry_safe(&req));
    }

    #[test]
    fn test_protocol_mismatch_uses_stable_typed_code_and_versions() {
        let error = daemon_protocol_mismatch_error(DAEMON_PROTOCOL_VERSION, 2);
        assert_eq!(error.code, IpcErrorCode::DaemonProtocolMismatch);
        assert_eq!(error.message, "Ferryx daemon protocol version mismatch");
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("expectedVersion"))
                .and_then(serde_json::Value::as_u64),
            Some(u64::from(DAEMON_PROTOCOL_VERSION))
        );
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|details| details.get("receivedVersion"))
                .and_then(serde_json::Value::as_u64),
            Some(2)
        );
    }

    #[test]
    fn test_should_request_upgrade_all_branches() {
        // Version mismatch -> true
        assert!(should_request_upgrade(
            Some("2026.831.1"),
            "2026.902.2",
            None,
            None
        ));
        assert!(should_request_upgrade(
            Some("2026.902.1"),
            "2026.902.2",
            None,
            None
        ));

        // Version identical -> false
        assert!(!should_request_upgrade(
            Some("2026.902.2"),
            "2026.902.2",
            None,
            None
        ));

        // Version None (legacy daemon) -> fallback to mtime
        assert!(should_request_upgrade(
            None,
            "2026.902.2",
            Some(1000),
            Some(2000)
        ));
        assert!(!should_request_upgrade(
            None,
            "2026.902.2",
            Some(2000),
            Some(2000)
        ));
        assert!(!should_request_upgrade(
            None,
            "2026.902.2",
            Some(3000),
            Some(2000)
        ));
        assert!(!should_request_upgrade(
            None,
            "2026.902.2",
            None,
            Some(2000)
        ));
        assert!(!should_request_upgrade(
            None,
            "2026.902.2",
            Some(1000),
            None
        ));
        assert!(!should_request_upgrade(None, "2026.902.2", None, None));
    }

    #[tokio::test]
    async fn test_daemon_client_has_spawn_lock_for_single_flight() {
        let client = DaemonClient::new();
        // Verifies the single-flight spawn_lock is instantiated and functions cleanly
        let guard = client.spawn_lock.try_lock();
        assert!(
            guard.is_ok(),
            "spawn_lock must be available on construction"
        );
        drop(guard);
    }

    #[tokio::test]
    async fn test_client_attach_retains_write_half_until_abort() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::AsyncReadExt;
        let dir = tempfile::Builder::new()
            .prefix("fx-client")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = dir.path().join("client_attach.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (server_done_tx, server_done_rx) = tokio::sync::oneshot::channel();
        let (abort_signal_tx, abort_signal_rx) = tokio::sync::oneshot::channel();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut read, mut write) = stream.into_split();
            let mut line = String::new();
            let mut reader = BufReader::new(&mut read);

            // Handshake
            reader.read_line(&mut line).await.unwrap();
            assert!(line.contains("handshake"));
            write
                .write_all(
                    format!(
                        "{{\"type\":\"handshakeOk\",\"version\":{},\"pid\":1,\"epoch\":1}}\n",
                        DAEMON_PROTOCOL_VERSION
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();

            // Attach
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(line.contains("attach"));
            write.write_all(b"{\"type\":\"attachOk\",\"epoch\":1,\"sessionId\":\"s1\",\"startSequence\":null,\"endSequence\":null,\"gap\":null,\"history\":\"\"}\n").await.unwrap();

            // Wait for test to confirm attach() has returned
            abort_signal_rx.await.unwrap();

            // Now read from the client socket. It must observe EOF once stream_task is aborted.
            let mut buf = [0u8; 1];
            let n = read.read(&mut buf).await.unwrap();
            assert_eq!(
                n, 0,
                "client socket write side must close when stream_task is aborted"
            );
            let _ = server_done_tx.send(());
        });

        let client = DaemonClient::new_with_socket(socket_path);
        let attachment = client.attach("s1", None).await.expect("attach succeeds");

        // Signal server that attach has completed and write_half should be retained in stream_task
        abort_signal_tx.send(()).unwrap();

        // Aborting the stream_task drops _keepalive (write_half)
        attachment.stream_task.abort();

        let res = tokio::time::timeout(Duration::from_secs(2), server_done_rx).await;
        assert!(
            res.is_ok(),
            "server must observe EOF within timeout after stream_task.abort()"
        );

        server_task.await.unwrap();
    }

    /// Idle DAG transport teardown: after the consumer drops the receiver and
    /// without any stream frame ever arriving, the spawned reader must send
    /// UnsubscribeDag and close the owned connection on its own.
    #[tokio::test]
    async fn test_subscribe_dag_releases_idle_transport_when_receiver_dropped() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("dag_idle_cancel.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let (subscribed_tx, subscribed_rx) = oneshot::channel::<()>();
        let (teardown_tx, teardown_rx) = oneshot::channel::<(String, String)>();

        let server_task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Handshake { .. }
            ));
            write_half
                .write_all(
                    format!(
                        "{{\"type\":\"handshakeOk\",\"version\":{DAEMON_PROTOCOL_VERSION},\"pid\":1,\"epoch\":1,\"daemonVersion\":\"test\"}}\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();

            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::SubscribeDag { .. }
            ));
            write_half
                .write_all(b"{\"type\":\"subscribeDagOk\"}\n")
                .await
                .unwrap();
            let _ = subscribed_tx.send(());

            // Never emit a stream frame: teardown must be driven purely by the
            // consumer dropping the receiver.
            line.clear();
            let n = reader.read_line(&mut line).await.unwrap();
            assert_ne!(n, 0, "transport closed without sending UnsubscribeDag");
            let observed = match serde_json::from_str::<DaemonRequest>(line.trim()).unwrap() {
                DaemonRequest::UnsubscribeDag {
                    workspace_id,
                    project_path,
                } => (workspace_id, project_path),
                other => panic!("unexpected request after cancellation: {other:?}"),
            };

            // The owned connection must also be released, not merely unsubscribed.
            line.clear();
            assert_eq!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "client must close the owned DAG connection after unsubscribing"
            );
            let _ = teardown_tx.send(observed);
        });

        let client = DaemonClient::new_with_socket(socket_path);
        let rx = client
            .subscribe_dag("ws-idle", "/paired/project")
            .await
            .expect("dag subscription succeeds");

        tokio::time::timeout(Duration::from_secs(5), subscribed_rx)
            .await
            .expect("daemon acknowledges subscription within timeout")
            .expect("subscription signal delivered");

        drop(rx);

        let (workspace_id, project_path) = tokio::time::timeout(Duration::from_secs(5), teardown_rx)
            .await
            .expect("idle DAG transport must tear down after receiver drop")
            .expect("teardown signal delivered");
        assert_eq!(workspace_id, "ws-idle");
        assert_eq!(project_path, "/paired/project");

        tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("fake daemon completes within timeout")
            .unwrap();
    }
}
