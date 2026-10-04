// allow: SIZE_OK — Daemon client UDS connection management, retries, and streaming channels
use crate::daemon::protocol::{
    DaemonRemoteEvent, DaemonRemoteStatus, DaemonRequest, DaemonResponse, DaemonSessionDetails,
    DaemonStreamMessage, TerminalStartup, DAEMON_PROTOCOL_VERSION,
};
use crate::daemon::protocol::{
    LocalSplitEnvelope, PreparedLocalSplit, SplitDelivery, SplitIdentity, SplitOperationResult,
    LOCAL_SPLIT_LIFECYCLE_CAPABILITY, LOCAL_SPLIT_VALIDITY_MS,
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
use std::collections::HashMap;
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

#[cfg(test)]
mod transport_pair_tests {
    use super::*;

    #[test]
    fn an_absent_or_empty_token_is_not_a_credential() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("daemon.token");
        // Nothing published yet: a reader must treat the pair as incomplete, never as a credential
        // it can present.
        assert_eq!(read_transport_token_at(&token_path), None);
        fs::write(&token_path, "   \n").unwrap();
        assert_eq!(read_transport_token_at(&token_path), None);
        fs::write(&token_path, " 9Zq3r8Tf2kLp\n").unwrap();
        assert_eq!(
            read_transport_token_at(&token_path).as_deref(),
            Some("9Zq3r8Tf2kLp")
        );
    }

    #[test]
    fn a_rejected_credential_is_reread_and_no_other_error_is() {
        // The daemon's rejection of the credential is the straddled pair: the token was published
        // by one boot and the port by another, so the pair is re-read rather than reported. The
        // error code callers see is unchanged.
        let rejected = handshake_error_for_response(
            "Daemon connection rejected: transport token missing or invalid".into(),
            Some("TRANSPORT_UNAUTHORIZED"),
        );
        assert!(transport_pair_is_stale(&rejected));
        assert_eq!(rejected.code, IpcErrorCode::InternalError);

        // A credential that was not on disk is the same condition, reached without a daemon
        // round-trip.
        assert!(transport_pair_is_stale(&transport_pair_stale_error(
            "daemon transport token was not published when its port was read"
        )));

        for unrelated in [
            handshake_error_for_response("boom".into(), Some("INTERNAL_ERROR")),
            handshake_error_for_response("boom".into(), None),
            IpcError::new(IpcErrorCode::IoError, "unrelated")
                .with_details(serde_json::json!({ "type": "ambiguousDelivery" })),
        ] {
            assert!(
                !transport_pair_is_stale(&unrelated),
                "an error that is not a rejected credential must never be retried as one"
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSpawnResult {
    pub session_id: String,
    pub epoch: u64,
    pub session: DaemonSessionDetails,
}
// The first launch of a freshly installed bundle pays Gatekeeper/XProtect signature
// validation before the daemon can emit its readiness token, which regularly exceeds a
// few seconds on a cold cache. A daemon that genuinely fails to start closes stdout and is
// reported immediately by wait_for_daemon_ready, so this budget only bounds a live but slow
// start; it never delays a real failure.
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(30);

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
/// Reads a published transport token from `path`. An absent file, or one holding only whitespace,
/// is no credential at all: it is reported as absent so a reader treats the pair it is reading as
/// incomplete rather than presenting an empty token the daemon rejects.
#[cfg(any(not(unix), test))]
fn read_transport_token_at(path: &Path) -> Option<String> {
    let token = fs::read_to_string(path).ok()?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_owned())
    }
}

/// Reads this boot's daemon transport token: the bearer credential the loopback transport
/// requires on the first frame before it dispatches a request. The daemon publishes the token
/// beside the port it binds, so a connection made before the daemon exists simply has none and
/// is rejected by the daemon exactly like a wrong one.
#[cfg(not(unix))]
pub(crate) fn read_transport_token() -> Option<String> {
    read_transport_token_at(&crate::daemon::server::get_transport_token_path())
}

/// Unix authenticates the transport by socket ownership and mode, so it presents no token and
/// touches no file.
#[cfg(unix)]
pub(crate) fn read_transport_token() -> Option<String> {
    None
}

/// The code the daemon answers with when the credential it was presented cannot belong to the
/// port it answered on.
const TRANSPORT_UNAUTHORIZED_CODE: &str = "TRANSPORT_UNAUTHORIZED";

/// The detail type marking the one failure a straddled pair is retried from.
const TRANSPORT_PAIR_STALE_DETAIL: &str = "transportPairStale";

/// The error a token and a port published by different boots are reported as, and the shape
/// `transport_pair_is_stale` recognises.
fn transport_pair_stale_error(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::InternalError, message)
        .with_details(json!({ "type": TRANSPORT_PAIR_STALE_DETAIL }))
}

/// Whether an attempt failed because the token and the port it read were published by different
/// boots.
///
/// A credential the daemon rejects and a credential that was not on disk when the port was read
/// are the same condition, so both are recognised here and neither is reused.
fn transport_pair_is_stale(error: &IpcError) -> bool {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("type"))
        .and_then(serde_json::Value::as_str)
        == Some(TRANSPORT_PAIR_STALE_DETAIL)
}

/// The error one daemon response is reported as.
///
/// The daemon answers a credential it rejects with `TRANSPORT_UNAUTHORIZED`; that is the straddled
/// pair `connect_and_handshake` re-reads, so it is marked here and every other response keeps the
/// error shape it had.
fn handshake_error_for_response(message: String, code: Option<&str>) -> IpcError {
    if code == Some(TRANSPORT_UNAUTHORIZED_CODE) {
        transport_pair_stale_error(message)
    } else {
        IpcError::new(IpcErrorCode::InternalError, message)
    }
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

pub(crate) fn manual_ssh_response(
    resp: DaemonResponse,
) -> Result<Option<crate::terminal::manual_ssh::ManualSshProcess>, IpcError> {
    use crate::terminal::manual_ssh::{CODE_AMBIGUOUS, CODE_CHANGED, CODE_IO, CODE_UNSUPPORTED};
    match resp {
        DaemonResponse::ManualSshOk { ssh } => Ok(ssh),
        DaemonResponse::Error { message, code, .. } => Err(match code.as_deref() {
            Some(CODE_AMBIGUOUS | CODE_CHANGED | CODE_UNSUPPORTED) => {
                IpcError::new(IpcErrorCode::Unsupported, message)
                    .with_details(serde_json::json!({ "code": code }))
            }
            Some("SESSION_NOT_FOUND") => IpcError::new(IpcErrorCode::SessionNotFound, message),
            Some(CODE_IO) => IpcError::new(IpcErrorCode::IoError, message)
                .with_details(serde_json::json!({ "code": code })),
            Some(_) => IpcError::new(IpcErrorCode::InternalError, message),
            None => IpcError::new(
                IpcErrorCode::DaemonProtocolMismatch,
                format!("Daemon cannot inspect manual SSH sessions: {message}"),
            ),
        }),
        _ => Err(IpcError::internal("Unexpected daemon response")),
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
            | DaemonRequest::SpawnOperationStatus { .. }
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
        DaemonRequest::RemoteAllocateAttachSession => "remoteAllocateAttachSession",
        DaemonRequest::MachineGateway => "machineGateway",
        DaemonRequest::MachineMetadataSubscribe { .. } => "machineMetadataSubscribe",
        DaemonRequest::RetryRemoteSession { .. } => "retryRemoteSession",
        DaemonRequest::RemoteSessionDetails { .. } => "remoteSessionDetails",
        DaemonRequest::DetectManualSsh { .. } => "detectManualSsh",
        DaemonRequest::RemoteWrite { .. } => "remoteWrite",
        DaemonRequest::RemoteResize { .. } => "remoteResize",
        DaemonRequest::CreateWorktree { .. } => "createWorktree",
        DaemonRequest::DeleteWorktree { .. } => "deleteWorktree",
        DaemonRequest::RegisterWorkspace { .. } => "registerWorkspace",
        DaemonRequest::UnregisterWorkspace { .. } => "unregisterWorkspace",
        DaemonRequest::Spawn { .. } => "spawn",
        DaemonRequest::SpawnOperationStatus { .. } => "spawnOperationStatus",
        DaemonRequest::CancelSpawnOperation { .. } => "cancelSpawnOperation",
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

/// Whether an incompatible predecessor daemon still owns its published endpoint.
///
/// The Windows kill path can decline to terminate: the pid guard rejects a system or own pid,
/// the identity check refuses a recycled pid that is not a Ferryx image, `tasklist` can be
/// unreadable, and the forced kill itself can fail. Every one of those is `NotTerminated`,
/// because the predecessor may still be alive and holding `daemon.port`/`daemon.lock`.
#[cfg(any(not(unix), test))]
#[must_use = "a caller that ignores the outcome may delete a live daemon's endpoint files"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleDaemonTermination {
    /// The predecessor is gone: it exited inside the wait window, or `taskkill` removed it.
    Terminated,
    /// The predecessor was not terminated and may still own its endpoint files.
    NotTerminated,
}

/// What one `tasklist` probe established about a pid.
///
/// The probe has three outcomes, not two: it can report the process, report that the pid is not
/// running, or fail to answer at all. Collapsing the last into "not running" turns a missing or
/// blocked `tasklist` into proof of death and authorises deleting a live daemon's endpoint files.
#[cfg(any(not(unix), test))]
#[derive(Debug, Clone, PartialEq, Eq)]
enum ProcessLiveness {
    /// The probe listed the pid under this image name.
    Alive(String),
    /// The probe succeeded and did not list the pid: the process is gone.
    Absent,
    /// The probe could not answer: it could not be spawned, exited non-zero, or produced output
    /// that cannot be read.
    Unknown,
}

/// Whether the endpoint files of an incompatible predecessor may be removed.
///
/// Deleting `daemon.port`/`daemon.lock` while a live daemon still owns them leaves the machine
/// with no reachable endpoint at all, so the destructive steps are gated on a predecessor that
/// is actually gone.
#[cfg(any(not(unix), test))]
fn stale_daemon_endpoint_files_removable(termination: StaleDaemonTermination) -> bool {
    matches!(termination, StaleDaemonTermination::Terminated)
}

/// Image names `tasklist` may report for the Ferryx executable this process runs from.
///
/// Pure so the Windows kill path's identity check is testable without Windows.
#[cfg(any(not(unix), test))]
fn expected_daemon_image_names(exe: Option<&Path>) -> Vec<String> {
    exe.and_then(Path::file_name)
        .map(|name| vec![name.to_string_lossy().into_owned()])
        .unwrap_or_default()
}

/// Extracts the image name `tasklist` reports for exactly `pid`.
///
/// The pid must equal the pid column of a row; a pid that merely appears inside another row's
/// fields (memory usage, session number) is not the process being asked about.
#[cfg(any(not(unix), test))]
fn tasklist_image_name_for_pid(csv: &str, pid: u32) -> Option<String> {
    for line in csv.lines() {
        let fields = parse_tasklist_csv_row(line);
        let (Some(image_name), Some(pid_field)) = (fields.first(), fields.get(1)) else {
            continue;
        };
        if pid_field.trim().parse::<u32>() == Ok(pid) {
            return Some(image_name.clone());
        }
    }
    None
}

/// One `tasklist` probe: whether it exited zero and its raw stdout, or `None` when it could not
/// be spawned at all.
#[cfg(any(not(unix), test))]
type TasklistProbe = Option<(bool, Vec<u8>)>;

/// Classifies one `tasklist` probe into liveness.
///
/// Only a probe that succeeded and whose stdout can be read decides between `Alive` and
/// `Absent`; a spawn failure, a non-zero exit, or unreadable output is `Unknown`, because "the
/// probe failed" and "the process is gone" must never produce the same answer.
#[cfg(any(not(unix), test))]
fn classify_tasklist_liveness(probe: TasklistProbe, pid: u32) -> ProcessLiveness {
    let Some((status_success, stdout)) = probe else {
        return ProcessLiveness::Unknown;
    };
    if !status_success {
        return ProcessLiveness::Unknown;
    }
    let Ok(stdout) = String::from_utf8(stdout) else {
        return ProcessLiveness::Unknown;
    };
    match tasklist_image_name_for_pid(&stdout, pid) {
        Some(image_name) => ProcessLiveness::Alive(image_name),
        None => ProcessLiveness::Absent,
    }
}

/// Splits one `tasklist /FO CSV` row into its quoted fields, keeping empty fields and treating
/// separators inside quotes as data.
#[cfg(any(not(unix), test))]
fn parse_tasklist_csv_row(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => fields.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    if !current.is_empty() || !fields.is_empty() {
        fields.push(current);
    }
    fields
}

/// True when the image name `tasklist` reported for a pid is the expected Ferryx executable.
///
/// Comparison ignores case and the optional `.exe` suffix, so the reported image name and the
/// basename of the running executable are comparable. Any other image, including a
/// `ferryx`-prefixed helper, never matches, and an empty expectation never matches either.
#[cfg(any(not(unix), test))]
fn image_name_matches_expected_daemon_image(image_name: &str, expected: &[String]) -> bool {
    fn normalize(name: &str) -> String {
        name.trim()
            .trim_matches('"')
            .to_ascii_lowercase()
            .trim_end_matches(".exe")
            .to_string()
    }
    let candidate = normalize(image_name);
    !candidate.is_empty() && expected.iter().any(|name| normalize(name) == candidate)
}

fn split_transport_error(
    mut error: IpcError,
    identity: Option<&SplitIdentity>,
    stage: &str,
    delivery: SplitDelivery,
) -> IpcError {
    let mut details = error.details.take().unwrap_or_else(|| json!({}));
    if !details.is_object() {
        details = json!({"causeDetails": details});
    }
    details["stage"] = json!(stage);
    details["delivery"] = json!(delivery);
    // A transport failure never proves an operation absent or a child dead.
    if details.get("operationState").is_none() {
        details["operationState"] = json!("unknown");
    }
    if let Some(identity) = identity {
        details["requestId"] = json!(identity.request_id);
        details["originEpoch"] = json!(identity.origin_epoch);
    }
    error.with_details(details)
}

async fn split_until<T>(
    deadline: tokio::time::Instant,
    identity: Option<&SplitIdentity>,
    stage: &str,
    delivery: SplitDelivery,
    future: impl std::future::Future<Output = Result<T, IpcError>>,
) -> Result<T, IpcError> {
    // timeout_at polls a ready future before its timer. Explicitly prevent an
    // already-expired attempt from writing even on a writable socket.
    if tokio::time::Instant::now() >= deadline {
        return Err(split_transport_error(
            IpcError::new(IpcErrorCode::SpawnAttemptTimeout, "Split attempt deadline elapsed"),
            identity,
            stage,
            delivery,
        ));
    }
    match tokio::time::timeout_at(deadline, future).await {
        Ok(result) => result.map_err(|error| split_transport_error(error, identity, stage, delivery)),
        Err(_) => Err(split_transport_error(
            IpcError::new(IpcErrorCode::SpawnAttemptTimeout, "Split attempt deadline elapsed"),
            identity,
            stage,
            delivery,
        )),
    }
}

/// Every await uses the original deadline. The caller owns and drops the whole
/// connection on error/cancellation; a partial line must never return to a pool.
async fn split_exchange_until<R, W>(
    reader: &mut R,
    writer: &mut W,
    request: &DaemonRequest,
    identity: Option<&SplitIdentity>,
    deadline: tokio::time::Instant,
    handshake: bool,
) -> Result<DaemonResponse, IpcError>
where
    R: AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(request).map_err(|error| {
        split_transport_error(
            IpcError::new(IpcErrorCode::ParseError, error.to_string()),
            identity,
            "serialize",
            SplitDelivery::NotSent,
        )
    })?;
    bytes.push(b'\n');
    // Before polling write_all there can be no delivery. Once polled, an
    // unknown prefix may reach the peer, including the complete request.
    split_until(
        deadline,
        identity,
        "beforeWrite",
        SplitDelivery::NotSent,
        async { Ok(()) },
    )
    .await?;
    let delivery = if handshake {
        SplitDelivery::NotSent
    } else {
        SplitDelivery::Ambiguous
    };
    split_until(
        deadline,
        identity,
        if handshake { "handshakeWrite" } else { "write" },
        delivery,
        async {
            writer
                .write_all(&bytes)
                .await
                .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
        },
    )
    .await?;
    split_until(
        deadline,
        identity,
        if handshake { "handshakeFlush" } else { "flush" },
        delivery,
        async {
            writer
                .flush()
                .await
                .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
        },
    )
    .await?;
    let mut line = String::new();
    split_until(
        deadline,
        identity,
        if handshake { "handshakeRead" } else { "read" },
        delivery,
        async {
            let count = reader
                .read_line(&mut line)
                .await
                .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
            if count == 0 || !line.ends_with('\n') {
                return Err(IpcError::new(
                    IpcErrorCode::IoError,
                    "Daemon disconnected before a complete response",
                ));
            }
            serde_json::from_str(&line)
                .map_err(|error| IpcError::new(IpcErrorCode::ParseError, error.to_string()))
        },
    )
    .await
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
        self.request_with_timeout(req, std::time::Duration::from_secs(15))
            .await
    }

    async fn request_with_timeout(
        &mut self,
        req: &DaemonRequest,
        timeout: std::time::Duration,
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
        let read_result = tokio::time::timeout(timeout, self.reader.read_line(&mut line)).await;
        let bytes_read = match read_result {
            Err(_) => {
                return Err(RequestAttemptError::ambiguous(IpcError::new(
                    IpcErrorCode::IoError,
                    format!("Timed out waiting for daemon response ({timeout:?})"),
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
///
/// Returns `true` only when this binary is strictly NEWER than the running daemon:
/// 1. `daemon_version` is provided and orders BEFORE `own_version` (CalVer package date version).
/// 2. Or, for legacy daemons without version metadata, fallback to `own_mtime > daemon_mtime`.
///
/// The comparison is deliberately asymmetric. A symmetric `!=` made an OLDER client reconnecting
/// to a NEWER daemon request a downgrade, which produced a second handover hop moments after the
/// first and destroyed sessions the first hop had not finished adopting.
pub fn should_request_upgrade(
    daemon_version: Option<&str>,
    own_version: &str,
    daemon_mtime: Option<u64>,
    own_mtime: Option<u64>,
) -> bool {
    if let Some(daemon_ver) = daemon_version {
        return match compare_calver(own_version, daemon_ver) {
            Some(std::cmp::Ordering::Greater) => true,
            Some(_) => false,
            // Unparseable on either side: the strings cannot say which build is newer, and
            // inequality alone would let an older GUI hand a newer daemon's sessions down to its
            // own binary. Only a strictly newer binary on disk may upgrade.
            None => matches!((daemon_mtime, own_mtime), (Some(daemon), Some(own)) if own > daemon),
        };
    }
    match (daemon_mtime, own_mtime) {
        (Some(daemon), Some(own)) => own > daemon,
        _ => false,
    }
}

/// The first daemon version that keeps the sessions it exports alive during a handover.
///
/// Every earlier v5 daemon stopped each exported session's reader in a way its own lifecycle
/// watcher read as "the session ended", then closed those sessions (SIGTERM, then SIGKILL) while
/// the successor was still adopting them. Such a daemon cannot be handed over without losing
/// terminals, so it is replaced only once it has none.
const FIRST_SESSION_PRESERVING_HANDOVER_VERSION: &str = "2026.927.1";

/// Whether replacing a daemon that serves `live_sessions` sessions would lose some of them.
///
/// An idle daemon is always safe to replace. A busy one is safe only when its version provably
/// includes the session-preserving handover; an unknown or unparseable version cannot prove it.
pub(crate) fn handover_would_lose_sessions(daemon_version: Option<&str>, live_sessions: usize) -> bool {
    if live_sessions == 0 {
        return false;
    }
    !matches!(
        daemon_version.and_then(|version| compare_calver(version, FIRST_SESSION_PRESERVING_HANDOVER_VERSION)),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    )
}

/// Orders two CalVer strings (`YYYY.MDD.N`) numerically per dot-separated component.
/// Returns `None` when either side is not fully numeric, so callers can pick their own fallback.
fn compare_calver(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let parse = |value: &str| -> Option<Vec<u64>> {
        value.split('.').map(|part| part.parse::<u64>().ok()).collect()
    };
    let (left_parts, right_parts) = (parse(left)?, parse(right)?);
    let width = left_parts.len().max(right_parts.len());
    for index in 0..width {
        let l = left_parts.get(index).copied().unwrap_or(0);
        let r = right_parts.get(index).copied().unwrap_or(0);
        if l != r {
            return Some(l.cmp(&r));
        }
    }
    Some(std::cmp::Ordering::Equal)
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

struct ClientQueuePermits {
    allocated_entries: usize,
    allocated_bytes: usize,
}

const CLIENT_MAX_PENDING_OPERATIONS: usize = 17;
const CLIENT_MAX_PENDING_BYTES: usize = 256 * 1024;

struct RemoteSessionSlot {
    connection: Mutex<Option<ActiveConnection>>,
    permits: parking_lot::Mutex<ClientQueuePermits>,
    active_refs: std::sync::atomic::AtomicUsize,
    closed: std::sync::atomic::AtomicBool,
}

struct LocalSessionSlot {
    connection: Mutex<Option<ActiveConnection>>,
    active_refs: std::sync::atomic::AtomicUsize,
    closed: std::sync::atomic::AtomicBool,
}

#[derive(Clone)]
pub struct DaemonClient {
    socket_path: PathBuf,
    connection: Arc<Mutex<Option<ActiveConnection>>>,
    interactive_connection: Arc<Mutex<Option<ActiveConnection>>>,
    remote_connections: Arc<parking_lot::Mutex<HashMap<String, Arc<RemoteSessionSlot>>>>,
    local_connections: Arc<parking_lot::Mutex<HashMap<String, Arc<LocalSessionSlot>>>>,
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
            remote_connections: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            local_connections: Arc::new(parking_lot::Mutex::new(HashMap::new())),
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
            remote_connections: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            local_connections: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            epoch: Arc::new(parking_lot::RwLock::new(None)),
            upgrade_requested: Arc::new(AtomicBool::new(false)),
            spawn_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn split_connect_until(
        &self,
        identity: Option<&SplitIdentity>,
        deadline: tokio::time::Instant,
        require_capability: bool,
    ) -> Result<(ActiveConnection, u64, Option<u64>), IpcError> {
        let path = self.socket_path.clone();
        let (credential, endpoint) = split_until(
            deadline,
            identity,
            "connect",
            SplitDelivery::NotSent,
            async {
                crate::ipc::run_blocking(move || {
                    Self::validate_existing_socket_path(&path)?;
                    let credential = read_transport_token();
                    #[cfg(unix)]
                    let endpoint = path;
                    #[cfg(not(unix))]
                    let endpoint = {
                        let port = fs::read_to_string(&path)
                            .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))?;
                        let port: u16 = port.trim().parse().map_err(|error: std::num::ParseIntError| {
                            IpcError::new(IpcErrorCode::ParseError, error.to_string())
                        })?;
                        std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, port)
                    };
                    Ok((credential, endpoint))
                })
                .await
            },
        )
        .await?;
        let stream = split_until(
            deadline,
            identity,
            "connect",
            SplitDelivery::NotSent,
            async {
                DaemonStream::connect(endpoint)
                    .await
                    .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
            },
        )
        .await?;
        let (reader, writer) = stream.into_split();
        let mut connection = ActiveConnection {
            reader: BufReader::new(reader),
            writer,
        };
        let response = split_exchange_until(
            &mut connection.reader,
            &mut connection.writer,
            &DaemonRequest::Handshake {
                version: DAEMON_PROTOCOL_VERSION,
                token: credential,
            },
            identity,
            deadline,
            true,
        )
        .await?;
        match response {
            DaemonResponse::HandshakeOk {
                version,
                epoch,
                capabilities,
                admission_time_unix_ms,
                ..
            } if version == DAEMON_PROTOCOL_VERSION => {
                if require_capability
                    && (!capabilities
                        .iter()
                        .any(|value| value == LOCAL_SPLIT_LIFECYCLE_CAPABILITY)
                        || admission_time_unix_ms.is_none())
                {
                    return Err(split_transport_error(
                        IpcError::new(
                            IpcErrorCode::UnsupportedCapability,
                            "Daemon does not support local split lifecycle",
                        ),
                        identity,
                        "handshake",
                        SplitDelivery::NotSent,
                    ));
                }
                Ok((connection, epoch, admission_time_unix_ms))
            }
            DaemonResponse::HandshakeOk { version, .. } => Err(split_transport_error(
                daemon_protocol_mismatch_error(DAEMON_PROTOCOL_VERSION, version),
                identity,
                "handshake",
                SplitDelivery::NotSent,
            )),
            DaemonResponse::ProtocolMismatch {
                expected_version,
                received_version,
            } => Err(split_transport_error(
                daemon_protocol_mismatch_error(expected_version, received_version),
                identity,
                "handshake",
                SplitDelivery::NotSent,
            )),
            DaemonResponse::Error {
                message,
                code,
                details,
            } => {
                let mut error = IpcError::new(
                    code.as_deref()
                        .map(IpcErrorCode::from_code_str)
                        .unwrap_or(IpcErrorCode::InternalError),
                    message,
                );
                error.details = details;
                Err(split_transport_error(
                    error,
                    identity,
                    "handshake",
                    SplitDelivery::NotSent,
                ))
            }
            _ => Err(split_transport_error(
                IpcError::internal("Unexpected handshake response"),
                identity,
                "handshake",
                SplitDelivery::NotSent,
            )),
        }
    }

    pub async fn prepare_local_split_until(
        &self,
        request_id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<SplitIdentity, IpcError> {
        let (_connection, epoch, admission_time) =
            self.split_connect_until(None, deadline, true).await?;
        let expires_at_unix_ms = admission_time
            .and_then(|time| time.checked_add(LOCAL_SPLIT_VALIDITY_MS))
            .ok_or_else(|| IpcError::internal("Invalid daemon admission clock"))?;
        Ok(SplitIdentity {
            request_id: request_id.into(),
            origin_epoch: epoch.to_string(),
            expires_at_unix_ms,
        })
    }

    async fn split_request_until(
        &self,
        mut request: DaemonRequest,
        identity: &SplitIdentity,
        deadline: tokio::time::Instant,
    ) -> Result<DaemonResponse, IpcError> {
        let (mut connection, epoch, _) = self
            .split_connect_until(Some(identity), deadline, true)
            .await?;
        if let DaemonRequest::Spawn {
            local_split: Some(envelope),
            ..
        } = &mut request
        {
            if envelope.origin_epoch != epoch {
                return Err(split_transport_error(
                    IpcError::new(
                        IpcErrorCode::SpawnEpochChanged,
                        "Daemon epoch changed before Create",
                    ),
                    Some(identity),
                    "beforeWrite",
                    SplitDelivery::NotSent,
                ));
            }
            envelope.remaining_ms = u64::try_from(
                deadline
                    .saturating_duration_since(tokio::time::Instant::now())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX);
        }
        let response = split_exchange_until(
            &mut connection.reader,
            &mut connection.writer,
            &request,
            Some(identity),
            deadline,
            false,
        )
        .await?;
        match response {
            DaemonResponse::Error {
                message,
                code,
                details,
            } => {
                let mut error = IpcError::new(
                    code.as_deref()
                        .map(IpcErrorCode::from_code_str)
                        .unwrap_or(IpcErrorCode::InternalError),
                    message,
                );
                error.details = details;
                Err(split_transport_error(
                    error,
                    Some(identity),
                    "response",
                    SplitDelivery::Confirmed,
                ))
            }
            response => Ok(response),
        }
    }

    pub async fn describe_session_until(
        &self,
        session_id: &str,
        identity: &SplitIdentity,
        deadline: tokio::time::Instant,
    ) -> Result<DaemonSessionDetails, IpcError> {
        match self
            .split_request_until(
                DaemonRequest::DescribeSession {
                    session_id: session_id.into(),
                },
                identity,
                deadline,
            )
            .await?
        {
            DaemonResponse::DescribeSessionOk { session } => Ok(session),
            _ => Err(split_transport_error(
                IpcError::internal("Unexpected describe response"),
                Some(identity),
                "response",
                SplitDelivery::Ambiguous,
            )),
        }
    }

    pub async fn describe_session_bounded_until(
        &self, session_id: &str, deadline: tokio::time::Instant,
    ) -> Result<DaemonSessionDetails, IpcError> {
        let (mut connection, _, _) = self.split_connect_until(None, deadline, false).await?;
        match split_exchange_until(&mut connection.reader, &mut connection.writer,
            &DaemonRequest::DescribeSession { session_id: session_id.into() },
            None, deadline, false).await?
        {
            DaemonResponse::DescribeSessionOk { session } => Ok(session),
            DaemonResponse::Error { message, code, details } => {
                let mut error = IpcError::new(code.as_deref().map(IpcErrorCode::from_code_str)
                    .unwrap_or(IpcErrorCode::InternalError), message);
                error.details = details;
                Err(error)
            }
            _ => Err(IpcError::internal("Unexpected bounded describe response")),
        }
    }

    pub async fn create_local_split_until(
        &self,
        prepared: &PreparedLocalSplit,
        deadline: tokio::time::Instant,
    ) -> Result<DaemonSpawnResult, IpcError> {
        let identity = &prepared.identity;
        let origin_epoch = identity
            .origin_epoch
            .parse::<u64>()
            .map_err(|_| IpcError::new(IpcErrorCode::InvalidArgument, "Invalid split origin epoch"))?;
        match self.local_split_status_until(identity, deadline).await? {
            SplitOperationResult::Created { session_id, daemon_epoch, session, .. } => {
                return Ok(DaemonSpawnResult { session_id, epoch: daemon_epoch, session });
            }
            SplitOperationResult::Absent { can_create: true } => {}
            operation => {
                let error = match operation {
                    SplitOperationResult::Cancelled => IpcError::spawn_cancelled("Split request was cancelled"),
                    SplitOperationResult::Absent { can_create: false } => IpcError::spawn_request_expired("Split identity expired; prepare a new request"),
                    SplitOperationResult::Failed { error, .. } => error,
                    _ => IpcError::new(IpcErrorCode::OperationOutcomeUnknown, "Split publication requires status reconciliation; do not create another request"),
                };
                return Err(error);
            }
        }
        let request = DaemonRequest::Spawn {
            client_request_id: identity.request_id.clone(),
            workspace_id: prepared.workspace_id.clone(),
            worktree: prepared.worktree.clone(),
            cwd: Some(prepared.cwd.clone()),
            cols: prepared.cols,
            rows: prepared.rows,
            shell: prepared.shell.clone(),
            startup: None,
            local_split: Some(LocalSplitEnvelope {
                origin_epoch,
                expires_at_unix_ms: identity.expires_at_unix_ms,
                remaining_ms: 0,
            }),
        };
        match self.split_request_until(request, identity, deadline).await? {
            DaemonResponse::SpawnOk {
                session_id,
                epoch,
                session,
            } => Ok(DaemonSpawnResult {
                session_id,
                epoch,
                session,
            }),
            _ => Err(split_transport_error(
                IpcError::internal("Unexpected Create response"),
                Some(identity),
                "response",
                SplitDelivery::Ambiguous,
            )),
        }
    }

    pub async fn local_split_status_until(
        &self,
        identity: &SplitIdentity,
        deadline: tokio::time::Instant,
    ) -> Result<SplitOperationResult, IpcError> {
        let origin_epoch = identity
            .origin_epoch
            .parse::<u64>()
            .map_err(|_| IpcError::new(IpcErrorCode::InvalidArgument, "Invalid split origin epoch"))?;
        let request = DaemonRequest::SpawnOperationStatus {
            client_request_id: identity.request_id.clone(),
            origin_epoch,
            expires_at_unix_ms: identity.expires_at_unix_ms,
        };
        self.local_split_operation_until(request, identity, deadline)
            .await
    }

    pub async fn cancel_local_split_until(
        &self,
        identity: &SplitIdentity,
        deadline: tokio::time::Instant,
    ) -> Result<SplitOperationResult, IpcError> {
        let origin_epoch = identity
            .origin_epoch
            .parse::<u64>()
            .map_err(|_| IpcError::new(IpcErrorCode::InvalidArgument, "Invalid split origin epoch"))?;
        let request = DaemonRequest::CancelSpawnOperation {
            client_request_id: identity.request_id.clone(),
            origin_epoch,
            expires_at_unix_ms: identity.expires_at_unix_ms,
        };
        self.local_split_operation_until(request, identity, deadline)
            .await
    }

    async fn local_split_operation_until(
        &self,
        request: DaemonRequest,
        identity: &SplitIdentity,
        deadline: tokio::time::Instant,
    ) -> Result<SplitOperationResult, IpcError> {
        match self.split_request_until(request, identity, deadline).await? {
            DaemonResponse::SpawnOperationOk { operation } => Ok(operation),
            _ => Err(split_transport_error(
                IpcError::internal("Unexpected operation response"),
                Some(identity),
                "response",
                SplitDelivery::Ambiguous,
            )),
        }
    }

    pub async fn attach_until(
        &self,
        session_id: &str,
        after_sequence: Option<u64>,
        deadline: tokio::time::Instant,
    ) -> Result<DaemonAttachment, IpcError> {
        let (mut connection, _, _) = self.split_connect_until(None, deadline, false).await?;
        let response = split_exchange_until(
            &mut connection.reader,
            &mut connection.writer,
            &DaemonRequest::Attach {
                session_id: session_id.into(),
                after_sequence,
            },
            None,
            deadline,
            false,
        )
        .await?;
        split_until(
            deadline,
            None,
            "attachInstall",
            SplitDelivery::Confirmed,
            async { Ok(()) },
        )
        .await?;
        Self::attachment_from_response(session_id, response, connection.reader, connection.writer)
    }

    fn attachment_from_response(
        session_id: &str,
        attach_resp: DaemonResponse,
        mut reader: BufReader<OwnedReadHalf>,
        write_half: OwnedWriteHalf,
    ) -> Result<DaemonAttachment, IpcError> {
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
                if resp_session_id != session_id {
                    return Err(IpcError::new(
                        IpcErrorCode::InvalidArgument,
                        "Attach response session mismatch",
                    ));
                }
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
                    loop {
                        let n = tokio::select! {
                            biased;
                            _ = tx.closed() => break,
                            result = reader.read_line(&mut stream_line) => match result {
                                Ok(n) => n,
                                Err(_) => break,
                            },
                        };
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
    pub const REMOTE_CONTROL_OUTER_TIMEOUT: Duration = Duration::from_secs(
        crate::ssh::bridge::DEFAULT_RPC_TIMEOUT.as_secs() * 2
            + crate::terminal::remote::INPUT_QUEUE_TIMEOUT.as_secs()
            + 5,
    );

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
            DaemonRequest::RemoteWrite { .. } | DaemonRequest::RemoteResize { .. } => {
                Self::REMOTE_CONTROL_OUTER_TIMEOUT
            }
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
            // The credential is read before the port is, the order the daemon publishes them in:
            // the token presented can then never be newer than the port it is presented to.
            let credential = read_transport_token();
            let stream = Self::connect_socket(&self.socket_path).await.map_err(|_| ServiceError::unavailable())?;
            let (reader, writer) = stream.into_split();
            let mut connection = ActiveConnection { reader: BufReader::new(reader), writer };
            let handshake = Self::paired_host_exchange(&mut connection, &DaemonRequest::Handshake { version: DAEMON_PROTOCOL_VERSION, token: credential }).await?;
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
    pub async fn remote_allocate_attach_session(
        &self,
    ) -> crate::paired_host::service::Result<crate::remote::attach_client::AttachSession> {
        match self
            .paired_host_request(DaemonRequest::RemoteAllocateAttachSession)
            .await?
        {
            DaemonResponse::RemoteAttachSessionOk {
                session_id,
                machine_id,
                machine_attach_public_key,
                enrollment_epoch,
                relay_origin,
            } => Ok(crate::remote::attach_client::AttachSession {
                session_id,
                machine_id,
                machine_attach_public_key,
                enrollment_epoch,
                relay_origin,
            }),
            _ => Err(crate::paired_host::service::ServiceError::unavailable()),
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
            remote_connections: Arc::clone(&self.remote_connections),
            local_connections: Arc::clone(&self.local_connections),
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
            // Handing a busy daemon of a lossy version over to this binary would kill the
            // terminals it serves, so that daemon keeps running until it is idle. The flag stays
            // set, so this GUI checks once per run instead of on every reconnect.
            match temp_client.list_sessions().await {
                Ok(sessions)
                    if handover_would_lose_sessions(daemon_version.as_deref(), sessions.len()) =>
                {
                    tracing::warn!(
                        daemon_version = ?daemon_version,
                        live_sessions = sessions.len(),
                        "Deferring the daemon upgrade: this daemon version loses sessions during a handover, so it is replaced only once it has none"
                    );
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "Deferring the daemon upgrade: could not count the daemon's live sessions"
                    );
                    return;
                }
            }
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

    /// Connects and handshakes, re-reading the published pair once when the attempt that failed
    /// read a pair that straddles two boots.
    ///
    /// The daemon writes its token before it publishes its port, so a reader that reads the token
    /// first and the port second either presents the credential that belongs to the port it
    /// connected to, or presents one that boot rejects. The rejected attempt is never reported as
    /// a request failure, because re-reading both halves is what makes the pair coherent again.
    async fn connect_and_handshake(&self) -> Result<ActiveConnection, IpcError> {
        match self.connect_and_handshake_once().await {
            Err(error) if transport_pair_is_stale(&error) => {
                self.connect_and_handshake_once().await
            }
            outcome => outcome,
        }
    }

    /// One attempt: the credential is read before the port is, so the token presented can never be
    /// newer than the port it is presented to.
    async fn connect_and_handshake_once(&self) -> Result<ActiveConnection, IpcError> {
        let credential = read_transport_token();
        let stream = self.connect_or_spawn().await?;
        // A port was published but the credential that authenticates it was not on disk when it
        // was read: the same straddled pair a rejection reports, so re-read both halves instead of
        // connecting with a token that cannot belong to this port.
        #[cfg(not(unix))]
        let credential = Some(credential.ok_or_else(|| {
            transport_pair_stale_error(
                "daemon transport token was not published when its port was read",
            )
        })?);
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let handshake = DaemonRequest::Handshake {
            version: DAEMON_PROTOCOL_VERSION,
            token: credential.clone(),
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
                    token: credential.clone(),
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
                pid,
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
                    #[cfg(not(unix))]
                    {
                        tracing::warn!(
                            old_version = version,
                            expected_version = DAEMON_PROTOCOL_VERSION,
                            old_pid = pid,
                            "Incompatible daemon protocol version detected on Windows. Shutting down stale predecessor daemon."
                        );
                        let shutdown_req = DaemonRequest::Shutdown;
                        if let Ok(mut s) = serde_json::to_string(&shutdown_req) {
                            s.push('\n');
                            let _ = write_half.write_all(s.as_bytes()).await;
                            let _ = write_half.flush().await;
                        }
                        drop(reader);
                        drop(write_half);

                        let termination =
                            Self::terminate_stale_daemon_process_windows(pid).await;
                        if !stale_daemon_endpoint_files_removable(termination) {
                            tracing::warn!(
                                old_pid = pid,
                                old_version = version,
                                expected_version = DAEMON_PROTOCOL_VERSION,
                                "A live, incompatible daemon could not be terminated; keeping its daemon.port and daemon.lock instead of deleting the endpoint of a daemon that still owns it"
                            );
                            return Err(daemon_protocol_mismatch_error(
                                DAEMON_PROTOCOL_VERSION,
                                version,
                            ));
                        }

                        let _ = fs::remove_file(&self.socket_path);
                        let lock_path = crate::daemon::server::get_lock_path();
                        let _ = fs::remove_file(lock_path);

                        tokio::time::sleep(Duration::from_millis(100)).await;
                        return Box::pin(self.connect_and_handshake()).await;
                    }
                    #[cfg(unix)]
                    {
                        self.maybe_trigger_upgrade_if_stale(daemon_version, binary_mtime_ms);
                        return Err(daemon_protocol_mismatch_error(
                            DAEMON_PROTOCOL_VERSION,
                            version,
                        ));
                    }
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
            } => {
                #[cfg(not(unix))]
                {
                    tracing::warn!(
                        expected_version,
                        received_version,
                        "Incompatible daemon protocol mismatch could not be negotiated on Windows. Requesting shutdown and starting a declared successor instead of deleting the running daemon's endpoint."
                    );
                    // Mirrors the HandshakeOk mismatch arm, with one deliberate difference: this
                    // variant carries no pid, so there is no identity-checked way to terminate the
                    // holder (see terminate_stale_daemon_process_windows). Never delete
                    // daemon.port or daemon.lock here: if the daemon that answered is still alive
                    // and holding them, the successor cannot take the instance lock and the
                    // machine is left with no reachable endpoint at all.
                    let shutdown_req = DaemonRequest::Shutdown;
                    if let Ok(mut s) = serde_json::to_string(&shutdown_req) {
                        s.push('\n');
                        let _ = write_half.write_all(s.as_bytes()).await;
                        let _ = write_half.flush().await;
                    }
                    drop(reader);
                    drop(write_half);

                    if let Err(error) = self.spawn_successor_daemon().await {
                        tracing::warn!(
                            %error,
                            "Successor daemon could not be started; the incompatible daemon keeps its endpoint"
                        );
                        return Err(daemon_protocol_mismatch_error(
                            expected_version,
                            received_version,
                        ));
                    }
                    return Box::pin(self.connect_and_handshake()).await;
                }
                #[cfg(unix)]
                {
                    Err(daemon_protocol_mismatch_error(
                        expected_version,
                        received_version,
                    ))
                }
            }
            DaemonResponse::Error { message, code, .. } => {
                Err(handshake_error_for_response(message, code.as_deref()))
            }
            _ => Err(IpcError::new(
                IpcErrorCode::InternalError,
                "Unexpected handshake response from daemon",
            )),
        }
    }

    /// Starts a daemon allowed to outlast the instance lock of an incompatible predecessor.
    ///
    /// `FERRYX_DAEMON_SUCCESSOR` arms the child's instance-lock wait window
    /// (`daemon::handover::successor_lock_wait`), which is what makes a takeover possible at all:
    /// without it the child fails fast on the lock and exits, leaving the machine with no daemon.
    /// The predecessor's `daemon.port` and `daemon.lock` are deliberately left in place; the child
    /// removes the stale port file itself once it owns the lock.
    #[cfg(not(unix))]
    async fn spawn_successor_daemon(&self) -> Result<(), IpcError> {
        let _spawn_guard = self.spawn_lock.lock().await;
        let binary_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ferryx"));
        let mut child = crate::util::no_window_tokio_command(&binary_path)
            .arg("--daemon")
            .env("FERRYX_DAEMON_SUCCESSOR", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                IpcError::new(
                    IpcErrorCode::InternalError,
                    format!(
                        "Failed to spawn Ferryx successor daemon process ({}): {e}",
                        binary_path.display()
                    ),
                )
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::InternalError,
                "Failed to capture successor daemon process stdout",
            )
        })?;

        if let Err(error) =
            wait_for_daemon_ready(BufReader::new(stdout), DAEMON_READY_TIMEOUT).await
        {
            let _ = child.kill().await;
            return Err(error);
        }
        Ok(())
    }

    /// Terminates the stale predecessor daemon that owns `pid`, reporting whether it is gone.
    ///
    /// The identity check and the forced kill can both leave the process in place; that is
    /// reported as `NotTerminated` so no caller treats an untouched live daemon as terminated.
    #[cfg(not(unix))]
    async fn terminate_stale_daemon_process_windows(pid: u32) -> StaleDaemonTermination {
        if pid <= 4 || pid == std::process::id() {
            return StaleDaemonTermination::NotTerminated;
        }
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Only a probe that positively answered "not running" proves the predecessor exited.
            // An unanswered probe keeps waiting and then falls through to the identity-checked
            // force-terminate path below, which refuses to touch a pid it cannot identify.
            if matches!(Self::tasklist_liveness_windows(pid), ProcessLiveness::Absent) {
                return StaleDaemonTermination::Terminated;
            }
        }
        // A pid is not an identity: the predecessor can exit inside the wait window and Windows
        // can hand the same pid to an unrelated process. Force-terminate only when the process
        // that owns the pid right now is actually a Ferryx image.
        let expected_images =
            expected_daemon_image_names(std::env::current_exe().ok().as_deref());
        let Some(image_name) = Self::tasklist_image_name_windows(pid) else {
            return StaleDaemonTermination::NotTerminated;
        };
        if !image_name_matches_expected_daemon_image(&image_name, &expected_images) {
            tracing::warn!(
                pid,
                image_name = %image_name,
                "Refusing to force-terminate a recycled pid that is not a Ferryx process"
            );
            return StaleDaemonTermination::NotTerminated;
        }
        tracing::warn!(pid, "Stale predecessor daemon did not exit within timeout, forcing termination");
        // The kill's own outcome is the evidence that the predecessor is gone: a `taskkill` that
        // failed leaves a live daemon behind, while a process that exited in the race with the
        // identity check is gone either way.
        let kill = crate::util::no_window_tokio_command("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .await;
        let killed = matches!(kill, Ok(output) if output.status.success());
        if killed || matches!(Self::tasklist_liveness_windows(pid), ProcessLiveness::Absent) {
            return StaleDaemonTermination::Terminated;
        }
        StaleDaemonTermination::NotTerminated
    }

    /// The tri-state `tasklist` probe for `pid`.
    ///
    /// `Unknown` is a real outcome, not a synonym for "gone": the probe could not be spawned,
    /// exited non-zero, or printed bytes that cannot be read as text. Callers may act on `Absent`
    /// only, because treating an unanswered probe as an exited process is what deletes a live
    /// daemon's endpoint files.
    #[cfg(not(unix))]
    fn tasklist_liveness_windows(pid: u32) -> ProcessLiveness {
        let probe = crate::util::no_window_command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()
            .map(|output| (output.status.success(), output.stdout));
        classify_tasklist_liveness(probe, pid)
    }

    /// Reads the image name `tasklist` reports for exactly `pid`.
    ///
    /// The CSV projection is required because the human-readable table cannot be parsed
    /// reliably: matching the pid as a substring of it also matches a pid that only appears
    /// inside another process's memory-usage column. Only an `Alive` probe yields a name, so a
    /// probe that failed to answer never looks like an identified Ferryx process either.
    #[cfg(not(unix))]
    fn tasklist_image_name_windows(pid: u32) -> Option<String> {
        match Self::tasklist_liveness_windows(pid) {
            ProcessLiveness::Alive(image_name) => Some(image_name),
            ProcessLiveness::Absent | ProcessLiveness::Unknown => None,
        }
    }

    pub async fn send_request(&self, req: DaemonRequest) -> Result<DaemonResponse, IpcError> {
        self.send_on_connection(&self.connection, req).await
    }

    /// Interactive requests (keystroke writes, resizes) use a dedicated connection so
    /// a long-running request on the shared connection (e.g. a Spawn holding it for
    /// blocking canonicalize + PTY startup) cannot stall queued keystrokes.
    fn remote_connection_for_session(
        &self,
        session_id: &str,
    ) -> Result<Arc<RemoteSessionSlot>, IpcError> {
        let mut map = self.remote_connections.lock();
        if let Some(slot) = map.get(session_id) {
            if slot.closed.load(Ordering::SeqCst) {
                return Err(IpcError::new(
                    IpcErrorCode::SessionNotFound,
                    format!("Remote session {session_id} is closed"),
                ));
            }
            slot.active_refs.fetch_add(1, Ordering::SeqCst);
            return Ok(Arc::clone(slot));
        }
        let slot = Arc::new(RemoteSessionSlot {
            connection: Mutex::new(None),
            permits: parking_lot::Mutex::new(ClientQueuePermits {
                allocated_entries: 0,
                allocated_bytes: 0,
            }),
            active_refs: std::sync::atomic::AtomicUsize::new(1),
            closed: std::sync::atomic::AtomicBool::new(false),
        });
        map.insert(session_id.to_string(), Arc::clone(&slot));
        Ok(slot)
    }

    pub fn remove_remote_connection_for_session(&self, session_id: &str) {
        let mut map = self.remote_connections.lock();
        if let Some(slot) = map.get(session_id) {
            slot.closed.store(true, Ordering::SeqCst);
            if slot.active_refs.load(Ordering::SeqCst) == 0 {
                map.remove(session_id);
            }
        }
    }

    fn local_connection_for_session(
        &self,
        session_id: &str,
    ) -> Result<Arc<LocalSessionSlot>, IpcError> {
        let mut map = self.local_connections.lock();
        if let Some(slot) = map.get(session_id) {
            if slot.closed.load(Ordering::SeqCst) {
                return Err(IpcError::new(
                    IpcErrorCode::SessionNotFound,
                    format!("Session {session_id} is closed"),
                ));
            }
            slot.active_refs.fetch_add(1, Ordering::SeqCst);
            return Ok(Arc::clone(slot));
        }
        let slot = Arc::new(LocalSessionSlot {
            connection: Mutex::new(None),
            active_refs: std::sync::atomic::AtomicUsize::new(1),
            closed: std::sync::atomic::AtomicBool::new(false),
        });
        map.insert(session_id.to_string(), Arc::clone(&slot));
        Ok(slot)
    }

    pub fn remove_local_connection_for_session(&self, session_id: &str) {
        let mut map = self.local_connections.lock();
        if let Some(slot) = map.get(session_id) {
            slot.closed.store(true, Ordering::SeqCst);
            if slot.active_refs.load(Ordering::SeqCst) == 0 {
                map.remove(session_id);
            }
        }
    }

    async fn send_interactive_request(
        &self,
        req: DaemonRequest,
    ) -> Result<DaemonResponse, IpcError> {
        if let DaemonRequest::RemoteWrite { ref session_id, .. }
        | DaemonRequest::RemoteResize { ref session_id, .. } = req
        {
            let session_slot = self.remote_connection_for_session(session_id)?;

            struct ActiveRefGuard<'a> {
                client: &'a DaemonClient,
                session_id: String,
                slot: Arc<RemoteSessionSlot>,
            }
            impl<'a> Drop for ActiveRefGuard<'a> {
                fn drop(&mut self) {
                    let prev = self.slot.active_refs.fetch_sub(1, Ordering::SeqCst);
                    if prev == 1 && self.slot.closed.load(Ordering::SeqCst) {
                        let mut map = self.client.remote_connections.lock();
                        if let Some(cur) = map.get(&self.session_id) {
                            if Arc::ptr_eq(cur, &self.slot) {
                                map.remove(&self.session_id);
                            }
                        }
                    }
                }
            }
            let _ref_guard = ActiveRefGuard {
                client: self,
                session_id: session_id.clone(),
                slot: Arc::clone(&session_slot),
            };

            let req_bytes = match req {
                DaemonRequest::RemoteWrite { ref data, .. } => data.len(),
                _ => 32,
            };

            {
                let mut permits = session_slot.permits.lock();
                if permits.allocated_entries >= CLIENT_MAX_PENDING_OPERATIONS
                    || permits.allocated_bytes + req_bytes > CLIENT_MAX_PENDING_BYTES
                {
                    return Err(IpcError::internal(
                        "Remote control queue is full; input was not queued",
                    )
                    .with_details(serde_json::json!({
                        "kind": "busy",
                        "inputWritten": false,
                    })));
                }
                permits.allocated_entries += 1;
                permits.allocated_bytes += req_bytes;
            }

            struct PermitGuard {
                slot: Arc<RemoteSessionSlot>,
                bytes: usize,
            }
            impl Drop for PermitGuard {
                fn drop(&mut self) {
                    let mut p = self.slot.permits.lock();
                    p.allocated_entries = p.allocated_entries.saturating_sub(1);
                    p.allocated_bytes = p.allocated_bytes.saturating_sub(self.bytes);
                }
            }
            let _permit_guard = PermitGuard {
                slot: Arc::clone(&session_slot),
                bytes: req_bytes,
            };

            let mut slot = session_slot.connection.lock().await;
            if session_slot.closed.load(Ordering::SeqCst) {
                return Err(IpcError::new(
                    IpcErrorCode::SessionNotFound,
                    "Remote session is closed",
                ));
            }

            let mut conn = slot.take();
            if conn.is_none() {
                conn = Some(self.connect_and_handshake().await?);
            }
            let mut active = conn.expect("connected");
            let timeout = Self::REMOTE_CONTROL_OUTER_TIMEOUT;
            let res = active.request_with_timeout(&req, timeout).await;
            return match res {
                Ok(reply) => {
                    *slot = Some(active);
                    Ok(reply)
                }
                Err(error) => Err(error.into_ipc_error(&req, true)),
            };
        }

        if let DaemonRequest::Write { ref session_id, .. }
        | DaemonRequest::Resize { ref session_id, .. }
        | DaemonRequest::RemoteSessionDetails { ref session_id } = req
        {
            // Per-session connection slots for local Write/Resize isolate
            // cross-session head-of-line blocking (e.g. one stalled local session
            // does not hold a shared mutex for up to 15s and block other sessions).
            // Note explicitly: this isolation fixes cross-session HOL only; the
            // affected session may still overflow or stall during its own 15s timeout.
            // Mutating operations maintain strict at-most-once delivery without retries.
            let session_slot = self.local_connection_for_session(session_id)?;

            struct LocalActiveRefGuard<'a> {
                client: &'a DaemonClient,
                session_id: String,
                slot: Arc<LocalSessionSlot>,
            }
            impl<'a> Drop for LocalActiveRefGuard<'a> {
                fn drop(&mut self) {
                    let prev = self.slot.active_refs.fetch_sub(1, Ordering::SeqCst);
                    if prev == 1 && self.slot.closed.load(Ordering::SeqCst) {
                        let mut map = self.client.local_connections.lock();
                        if let Some(cur) = map.get(&self.session_id) {
                            if Arc::ptr_eq(cur, &self.slot) {
                                map.remove(&self.session_id);
                            }
                        }
                    }
                }
            }
            let _ref_guard = LocalActiveRefGuard {
                client: self,
                session_id: session_id.clone(),
                slot: Arc::clone(&session_slot),
            };

            let mut slot = session_slot.connection.lock().await;
            if session_slot.closed.load(Ordering::SeqCst) {
                return Err(IpcError::new(
                    IpcErrorCode::SessionNotFound,
                    format!("Session {session_id} is closed"),
                ));
            }

            let mut conn = slot.take();
            if conn.is_none() {
                conn = Some(self.connect_and_handshake().await?);
            }
            let mut active = conn.expect("connected");
            let timeout = std::time::Duration::from_secs(15);
            let res = active.request_with_timeout(&req, timeout).await;
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
                local_split: None,
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

    /// Old daemons answer an unknown request with an uncoded Error; that is a protocol gap,
    /// never evidence of a local shell, so it must not fall back to a local save.
    pub async fn detect_manual_ssh(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::terminal::manual_ssh::ManualSshProcess>, IpcError> {
        let resp = self
            .send_request(DaemonRequest::DetectManualSsh {
                session_id: session_id.to_string(),
            })
            .await?;
        manual_ssh_response(resp)
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
                token: read_transport_token(),
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
                token: read_transport_token(),
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
            token: read_transport_token(),
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
        match self
            .send_interactive_request(DaemonRequest::RemoteSessionDetails {
                session_id: session_id.into(),
            })
            .await?
        {
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
        match self
            .send_interactive_request(DaemonRequest::RemoteSessionDetails {
                session_id: session_id.into(),
            })
            .await?
        {
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

        self.remove_remote_connection_for_session(session_id);
        self.remove_local_connection_for_session(session_id);

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
        self.remove_remote_connection_for_session(session_id);
        self.remove_local_connection_for_session(session_id);
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

#[cfg(test)]
mod local_control_isolation_tests {
    use super::*;

    #[cfg(unix)]
    fn stream_pair() -> (DaemonStream, DaemonStream) {
        tokio::net::UnixStream::pair().unwrap()
    }

    #[cfg(not(unix))]
    async fn stream_pair() -> (DaemonStream, DaemonStream) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap());
        let (client, accepted) = tokio::join!(client, listener.accept());
        (client.unwrap(), accepted.unwrap().0)
    }

    #[tokio::test]
    async fn public_local_control_bypasses_busy_general_and_shared_interactive_connections() {
        #[cfg(unix)]
        let (client_stream, server_stream) = stream_pair();
        #[cfg(not(unix))]
        let (client_stream, server_stream) = stream_pair().await;

        let client = DaemonClient::new();
        let session_id = "isolated-public-local-control";
        let (reader, writer) = client_stream.into_split();
        client.local_connections.lock().insert(
            session_id.into(),
            Arc::new(LocalSessionSlot {
                connection: Mutex::new(Some(ActiveConnection {
                    reader: BufReader::new(reader),
                    writer,
                })),
                active_refs: std::sync::atomic::AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }),
        );

        // Hold BOTH the general connection lock AND the shared interactive_connection lock.
        // Public local write and resize MUST finish over the session-dedicated slot without waiting on either.
        let general_guard = client.connection.lock().await;
        let interactive_guard = client.interactive_connection.lock().await;

        let server = tokio::spawn(async move {
            let (reader, mut writer) = server_stream.into_split();
            let mut reader = BufReader::new(reader);
            for index in 0..4 {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).await.unwrap() > 0);
                let request: DaemonRequest = serde_json::from_str(&line).unwrap();
                let response = match (index, request) {
                    (0 | 2, DaemonRequest::RemoteSessionDetails { session_id: id }) => {
                        assert_eq!(id, session_id);
                        DaemonResponse::RemoteSessionDetailsOk {
                            details: None,
                            legacy_direct_ssh: false,
                        }
                    }
                    (1, DaemonRequest::Write { session_id: id, data }) => {
                        assert_eq!(id, session_id);
                        assert_eq!(data, b"x");
                        DaemonResponse::WriteOk
                    }
                    (3, DaemonRequest::Resize { session_id: id, cols, rows }) => {
                        assert_eq!(id, session_id);
                        assert_eq!((cols, rows), (100, 30));
                        DaemonResponse::ResizeOk
                    }
                    (_, request) => panic!("unexpected request: {request:?}"),
                };
                writer
                    .write_all(format!("{}\n", serde_json::to_string(&response).unwrap()).as_bytes())
                    .await
                    .unwrap();
            }
        });

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            client.write_terminal_at_generation(session_id, None, b"x".to_vec()).await?;
            client.resize_terminal_at_generation(session_id, None, 100, 30).await
        })
        .await;

        drop(interactive_guard);
        drop(general_guard);

        if result.is_err() {
            server.abort();
        }
        result
            .expect("local control waited on general or shared interactive connection")
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn two_session_contention_session_b_proceeds_while_session_a_classification_withheld() {
        #[cfg(unix)]
        let (client_a, server_a) = stream_pair();
        #[cfg(not(unix))]
        let (client_a, server_a) = stream_pair().await;

        #[cfg(unix)]
        let (client_b, server_b) = stream_pair();
        #[cfg(not(unix))]
        let (client_b, server_b) = stream_pair().await;

        let client = DaemonClient::new();
        let session_a = "session-a-withheld";
        let session_b = "session-b-unblocked";

        let (reader_a, writer_a) = client_a.into_split();
        client.local_connections.lock().insert(
            session_a.into(),
            Arc::new(LocalSessionSlot {
                connection: Mutex::new(Some(ActiveConnection {
                    reader: BufReader::new(reader_a),
                    writer: writer_a,
                })),
                active_refs: std::sync::atomic::AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }),
        );

        let (reader_b, writer_b) = client_b.into_split();
        client.local_connections.lock().insert(
            session_b.into(),
            Arc::new(LocalSessionSlot {
                connection: Mutex::new(Some(ActiveConnection {
                    reader: BufReader::new(reader_b),
                    writer: writer_b,
                })),
                active_refs: std::sync::atomic::AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }),
        );

        let (a_entered_tx, a_entered_rx) = tokio::sync::oneshot::channel::<()>();
        let (a_release_tx, a_release_rx) = tokio::sync::oneshot::channel::<()>();

        let server_a_task = tokio::spawn(async move {
            let (reader, mut writer) = server_a.into_split();
            let mut reader = BufReader::new(reader);

            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let req: DaemonRequest = serde_json::from_str(&line).unwrap();
            match req {
                DaemonRequest::RemoteSessionDetails { session_id } => {
                    assert_eq!(session_id, session_a);
                }
                other => panic!("expected RemoteSessionDetails for session A, got: {other:?}"),
            }

            let _ = a_entered_tx.send(());
            let _ = a_release_rx.await;

            let details_resp = DaemonResponse::RemoteSessionDetailsOk {
                details: None,
                legacy_direct_ssh: false,
            };
            writer
                .write_all(format!("{}\n", serde_json::to_string(&details_resp).unwrap()).as_bytes())
                .await
                .unwrap();

            line.clear();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let write_req: DaemonRequest = serde_json::from_str(&line).unwrap();
            match write_req {
                DaemonRequest::Write { session_id, data } => {
                    assert_eq!(session_id, session_a);
                    assert_eq!(data, b"bytes-a");
                }
                other => panic!("expected Write for session A, got: {other:?}"),
            }

            let write_resp = DaemonResponse::WriteOk;
            writer
                .write_all(format!("{}\n", serde_json::to_string(&write_resp).unwrap()).as_bytes())
                .await
                .unwrap();

            // Assert exactly-once delivery: no further requests, client closes connection
            line.clear();
            assert_eq!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "server A observed unexpected extra request"
            );
        });

        let server_b_task = tokio::spawn(async move {
            let (reader, mut writer) = server_b.into_split();
            let mut reader = BufReader::new(reader);

            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let req: DaemonRequest = serde_json::from_str(&line).unwrap();
            match req {
                DaemonRequest::RemoteSessionDetails { session_id } => {
                    assert_eq!(session_id, session_b);
                }
                other => panic!("expected RemoteSessionDetails for session B, got: {other:?}"),
            }
            let details_resp = DaemonResponse::RemoteSessionDetailsOk {
                details: None,
                legacy_direct_ssh: false,
            };
            writer
                .write_all(format!("{}\n", serde_json::to_string(&details_resp).unwrap()).as_bytes())
                .await
                .unwrap();

            line.clear();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let write_req: DaemonRequest = serde_json::from_str(&line).unwrap();
            match write_req {
                DaemonRequest::Write { session_id, data } => {
                    assert_eq!(session_id, session_b);
                    assert_eq!(data, b"bytes-b");
                }
                other => panic!("expected Write for session B, got: {other:?}"),
            }
            let write_resp = DaemonResponse::WriteOk;
            writer
                .write_all(format!("{}\n", serde_json::to_string(&write_resp).unwrap()).as_bytes())
                .await
                .unwrap();

            // Assert exactly-once delivery: no further requests, client closes connection
            line.clear();
            assert_eq!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "server B observed unexpected extra request"
            );
        });

        let client_a = client.clone();
        let client_b = client.clone();

        let a_task = tokio::spawn(async move {
            client_a
                .write_terminal_at_generation(session_a, None, b"bytes-a".to_vec())
                .await
        });

        if let Err(e) = tokio::time::timeout(Duration::from_secs(2), a_entered_rx).await {
            let _ = a_release_tx.send(());
            a_task.abort();
            server_a_task.abort();
            server_b_task.abort();
            panic!("session A classification timed out before reaching server: {e}");
        }

        let b_result = tokio::time::timeout(Duration::from_secs(2), async {
            client_b
                .write_terminal_at_generation(session_b, None, b"bytes-b".to_vec())
                .await
        })
        .await;

        if let Err(e) = &b_result {
            let _ = a_release_tx.send(());
            a_task.abort();
            server_a_task.abort();
            server_b_task.abort();
            panic!("session B local write blocked on session A: {e}");
        }
        b_result.unwrap().expect("session B write returned error");

        // Now release session A and ensure its write completes
        let _ = a_release_tx.send(());

        let a_result = tokio::time::timeout(Duration::from_secs(2), a_task).await;
        if let Err(e) = &a_result {
            server_a_task.abort();
            server_b_task.abort();
            panic!("session A timed out after release: {e}");
        }
        a_result
            .unwrap()
            .unwrap()
            .expect("session A write returned error");

        // Drop client slots to close write halves so server tasks see EOF for exactly-once check
        client.local_connections.lock().clear();

        server_a_task.await.unwrap();
        server_b_task.await.unwrap();
    }

    #[tokio::test]
    async fn remote_generation_fence_rejects_unversioned_write_and_preserves_local_slot() {
        #[cfg(unix)]
        let (client_stream, server_stream) = stream_pair();
        #[cfg(not(unix))]
        let (client_stream, server_stream) = stream_pair().await;

        let client = DaemonClient::new();
        let session_id = "remote-session-fenced";
        let (reader, writer) = client_stream.into_split();
        client.local_connections.lock().insert(
            session_id.into(),
            Arc::new(LocalSessionSlot {
                connection: Mutex::new(Some(ActiveConnection {
                    reader: BufReader::new(reader),
                    writer,
                })),
                active_refs: std::sync::atomic::AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }),
        );

        let server = tokio::spawn(async move {
            let (reader, mut writer) = server_stream.into_split();
            let mut reader = BufReader::new(reader);

            let mut line = String::new();
            assert!(reader.read_line(&mut line).await.unwrap() > 0);
            let request: DaemonRequest = serde_json::from_str(&line).unwrap();
            match request {
                DaemonRequest::RemoteSessionDetails { session_id: id } => {
                    assert_eq!(id, session_id);
                    let details = crate::terminal::remote::RemoteSessionDetails {
                        descriptor: crate::terminal::remote::RemoteSessionDescriptor {
                            backend_session_id: id.to_string(),
                            target: crate::scoped_contracts::TargetRef {
                                host_id: "test-host".to_string(),
                                owner_id: "test-owner".to_string(),
                                epoch: crate::scoped_contracts::Epoch(42),
                                backend_session_id: "remote-target-id".to_string(),
                            },
                            config: crate::terminal::remote::RemoteSessionConfig {
                                host: crate::ssh::SshHost {
                                    id: "test-host".to_string(),
                                    label: "test-host".to_string(),
                                    hostname: "127.0.0.1".to_string(),
                                    username: None,
                                    port: None,
                                    identity_file: None,
                                    jump_host: None,
                                    source: crate::ssh::SshHostSource::Config,
                                    auth_method: crate::ssh::SshAuthMethod::Agent,
                                    disabled: None,
                                },
                                environment: crate::ssh::runtime::RemoteEnvironment {
                                    platform: crate::ssh::runtime::RemotePlatform::Posix,
                                    executor: crate::ssh::runtime::RemoteExecutor::Sh,
                                    version: "test".to_string(),
                                    home: "/home".to_string(),
                                    temp: "/tmp".to_string(),
                                    git: true,
                                },
                                helper: crate::ssh::helper_setup::HelperLocation {
                                    executable: "/bin/helper".to_string(),
                                    root: "/tmp".to_string(),
                                },
                                project_id: "proj".to_string(),
                                project_path: "/proj".to_string(),
                                worktree: None,
                                agent_identity: None,
                            },
                            client_request_id: "req-123".to_string(),
                            remote_cursor: crate::ssh::bridge::RemoteCursor(0),
                            cols: 100,
                            rows: 30,
                        },
                        state: crate::terminal::remote::RemoteConnectionState::Connected,
                        generation: 42,
                        attempts: 0,
                        failure: None,
                        replay_gap: None,
                        pid: None,
                    };
                    let resp = DaemonResponse::RemoteSessionDetailsOk {
                        details: Some(details),
                        legacy_direct_ssh: false,
                    };
                    writer
                        .write_all(format!("{}\n", serde_json::to_string(&resp).unwrap()).as_bytes())
                        .await
                        .unwrap();
                }
                other => panic!("expected RemoteSessionDetails, got {other:?}"),
            }

            line.clear();
            assert_eq!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "server observed mutating write after classification rejected"
            );
        });

        let write_result = tokio::time::timeout(
            Duration::from_secs(2),
            client.write_terminal_at_generation(session_id, None, b"unversioned-payload".to_vec()),
        )
        .await;

        client.local_connections.lock().clear();

        if let Err(e) = &write_result {
            server.abort();
            panic!("generation fence write timed out: {e}");
        }

        let err = write_result
            .unwrap()
            .expect_err("generation-less write on remote session must be rejected");

        assert_eq!(err.code, IpcErrorCode::InternalError);
        let details = err.details.expect("details object");
        assert_eq!(details["kind"], "staleGeneration");
        assert_eq!(details["inputWritten"], false);

        server.await.unwrap();
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

    #[tokio::test]
    async fn remote_input_socket_sessions_do_not_block_each_other() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("isolated-input.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut entered = Some(entered_tx);
            let mut release = Some(release_rx);
            let mut peers = tokio::task::JoinSet::new();
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let entered = if index == 0 { entered.take() } else { None };
                let release = if index == 0 { release.take() } else { None };
                peers.spawn(async move {
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
                        // Legacy mock daemon does not advertise split admission.
                        capabilities: Vec::new(),
                        admission_time_unix_ms: None,
                    };
                    write
                        .write_all(
                            format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes(),
                        )
                        .await
                        .unwrap();
                    line.clear();
                    reader.read_line(&mut line).await.unwrap();
                    let request: DaemonRequest = serde_json::from_str(&line).unwrap();
                    assert!(matches!(request, DaemonRequest::RemoteWrite { .. }));
                    if let Some(entered) = entered {
                        entered.send(()).unwrap();
                    }
                    if let Some(release) = release {
                        release.await.unwrap();
                    }
                    write.write_all(b"{\"type\":\"pong\"}\n").await.unwrap();
                });
            }
            while let Some(peer) = peers.join_next().await {
                peer.unwrap();
            }
        });
        let client = DaemonClient::new_with_socket(socket);
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .send_interactive_request(DaemonRequest::RemoteWrite {
                    session_id: "first".into(),
                    generation: 1,
                    data: b"a".to_vec(),
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_interactive_request(DaemonRequest::RemoteWrite {
                session_id: "second".into(),
                generation: 1,
                data: b"b".to_vec(),
            }),
        )
        .await;
        release_tx.send(()).unwrap();
        assert!(matches!(second.unwrap().unwrap(), DaemonResponse::Pong));
        assert!(matches!(
            first.await.unwrap().unwrap(),
            DaemonResponse::Pong
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn local_input_socket_sessions_do_not_block_each_other() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("isolated-local-input.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut entered = Some(entered_tx);
            let mut release = Some(release_rx);
            let mut peers = tokio::task::JoinSet::new();
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let entered = if index == 0 { entered.take() } else { None };
                let release = if index == 0 { release.take() } else { None };
                peers.spawn(async move {
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
                        // Legacy mock daemon does not advertise split admission.
                        capabilities: Vec::new(),
                        admission_time_unix_ms: None,
                    };
                    write
                        .write_all(
                            format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes(),
                        )
                        .await
                        .unwrap();
                    line.clear();
                    reader.read_line(&mut line).await.unwrap();
                    let request: DaemonRequest = serde_json::from_str(&line).unwrap();
                    if let Some(entered) = entered {
                        assert!(matches!(request, DaemonRequest::Write { .. }));
                        entered.send(()).unwrap();
                    }
                    if let Some(release) = release {
                        release.await.unwrap();
                        write.write_all(b"{\"type\":\"writeOk\"}\n").await.unwrap();
                    } else {
                        // Session B arrives while session A is stalled
                        assert!(matches!(request, DaemonRequest::Write { .. }));
                        write.write_all(b"{\"type\":\"writeOk\"}\n").await.unwrap();
                    }
                });
            }
            while let Some(peer) = peers.join_next().await {
                peer.unwrap();
            }
        });
        let client = DaemonClient::new_with_socket(socket);
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .send_interactive_request(DaemonRequest::Write {
                    session_id: "local-first".into(),
                    data: b"hello".to_vec(),
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        // Session A is stalled in server waiting for release_rx.
        // Session B sends Write on its own per-session slot and must complete without blocking.
        let second = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_interactive_request(DaemonRequest::Write {
                session_id: "local-second".into(),
                data: b"world".to_vec(),
            }),
        )
        .await;
        release_tx.send(()).unwrap();
        assert!(matches!(second.unwrap().unwrap(), DaemonResponse::WriteOk));
        assert!(matches!(
            first.await.unwrap().unwrap(),
            DaemonResponse::WriteOk
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn local_resize_session_does_not_block_on_stalled_write_session() {
        let directory = tempdir().unwrap();
        let socket = directory.path().join("isolated-local-resize.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut entered = Some(entered_tx);
            let mut release = Some(release_rx);
            let mut peers = tokio::task::JoinSet::new();
            for index in 0..2 {
                let (stream, _) = listener.accept().await.unwrap();
                let entered = if index == 0 { entered.take() } else { None };
                let release = if index == 0 { release.take() } else { None };
                peers.spawn(async move {
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
                        // Legacy mock daemon does not advertise split admission.
                        capabilities: Vec::new(),
                        admission_time_unix_ms: None,
                    };
                    write
                        .write_all(
                            format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes(),
                        )
                        .await
                        .unwrap();
                    line.clear();
                    reader.read_line(&mut line).await.unwrap();
                    let request: DaemonRequest = serde_json::from_str(&line).unwrap();
                    if let Some(entered) = entered {
                        assert!(matches!(request, DaemonRequest::Write { .. }));
                        entered.send(()).unwrap();
                    }
                    if let Some(release) = release {
                        release.await.unwrap();
                        write.write_all(b"{\"type\":\"writeOk\"}\n").await.unwrap();
                    } else {
                        // Session B arrives with Resize while session A is stalled on Write
                        assert!(matches!(request, DaemonRequest::Resize { .. }));
                        write.write_all(b"{\"type\":\"resizeOk\"}\n").await.unwrap();
                    }
                });
            }
            while let Some(peer) = peers.join_next().await {
                peer.unwrap();
            }
        });
        let client = DaemonClient::new_with_socket(socket);
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .send_interactive_request(DaemonRequest::Write {
                    session_id: "local-first".into(),
                    data: b"hello".to_vec(),
                })
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        // Session A is stalled. Session B sends Resize and must succeed immediately.
        let second = tokio::time::timeout(
            Duration::from_secs(2),
            client.send_interactive_request(DaemonRequest::Resize {
                session_id: "local-second".into(),
                cols: 80,
                rows: 24,
            }),
        )
        .await;
        release_tx.send(()).unwrap();
        assert!(matches!(second.unwrap().unwrap(), DaemonResponse::ResizeOk));
        assert!(matches!(
            first.await.unwrap().unwrap(),
            DaemonResponse::WriteOk
        ));
        server.await.unwrap();
    }

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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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
        let dir = tempdir().unwrap();
        let socket = dir.path().join("control-busy.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (ping_seen_tx, ping_seen_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let handshake = DaemonResponse::HandshakeOk {
                version: DAEMON_PROTOCOL_VERSION,
                pid: std::process::id(),
                epoch: 1,
                binary_path: None,
                binary_mtime_ms: None,
                daemon_version: None,
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
            };
            // Connection 1 (desktop control): answer the handshake, receive the Ping,
            // and hold the reply so the control slot stays busy for the whole test.
            // Its handling is parked on a subtask so the session connection below can
            // still be accepted while the control reply is withheld.
            let (control, _) = listener.accept().await.unwrap();
            let (read, mut write) = control.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            write
                .write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::Ping
            ));
            let ping_seen_tx = ping_seen_tx;
            let release_rx = release_rx;
            let control_task = tokio::spawn(async move {
                ping_seen_tx.send(()).unwrap();
                release_rx.await.unwrap();
                write.write_all(b"{\"type\":\"pong\"}\n").await.unwrap();
            });
            // Connection 2 (session slot): the remote input must arrive here while the
            // control connection is still pending.
            let (session, _) = listener.accept().await.unwrap();
            let (read, mut write) = session.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            write
                .write_all(format!("{}\n", serde_json::to_string(&handshake).unwrap()).as_bytes())
                .await
                .unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            assert!(matches!(
                serde_json::from_str::<DaemonRequest>(line.trim()).unwrap(),
                DaemonRequest::RemoteWrite { .. }
            ));
            write.write_all(b"{\"type\":\"writeOk\"}\n").await.unwrap();
            control_task.await.unwrap();
        });
        let client = DaemonClient::new_with_socket(socket);
        let control = {
            let client = client.clone();
            tokio::spawn(async move { client.send_request(DaemonRequest::Ping).await })
        };
        tokio::time::timeout(Duration::from_secs(2), ping_seen_rx)
            .await
            .expect("control connection must deliver the Ping")
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            client.write_terminal_at_generation("remote", Some(7), b"key".to_vec()),
        )
        .await
        .expect("remote input must not wait behind a busy desktop control connection")
        .expect("remote input must complete on its own session slot");
        assert!(
            !control.is_finished(),
            "held control request must still be pending while remote input completes"
        );
        release_tx.send(()).unwrap();
        control.await.unwrap().unwrap();
        server.await.unwrap();
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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

    #[test]
    fn test_daemon_ready_budget_covers_first_launch_signature_validation() {
        // The first launch of a freshly installed (re-signed or re-notarized) bundle pays
        // Gatekeeper/XProtect validation before the daemon can emit its readiness token.
        // A budget under that cost makes spawn_daemon_process kill the daemon it just
        // spawned, which surfaces as "daemon startup timed out waiting for readiness signal"
        // and leaves a stale socket behind.
        assert!(
            DAEMON_READY_TIMEOUT >= Duration::from_secs(20),
            "readiness budget {DAEMON_READY_TIMEOUT:?} is too tight for first-launch signature validation"
        );
    }

    #[tokio::test]
    async fn test_daemon_readiness_fails_immediately_when_daemon_process_dies() {
        // A wider budget must not slow down genuine failures: a daemon that exits without
        // emitting the token closes stdout, and that EOF has to fail right away instead of
        // burning the whole readiness budget.
        let (writer, reader) = tokio::io::duplex(256);
        drop(writer);

        let started = std::time::Instant::now();
        let error = wait_for_daemon_ready(BufReader::new(reader), DAEMON_READY_TIMEOUT)
            .await
            .expect_err("closed stdout must not be reported as readiness");

        assert!(
            error.message.contains("stdout closed"),
            "unexpected error: {}",
            error.message
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "EOF must fail fast, took {:?}",
            started.elapsed()
        );
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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
                // Legacy mock daemon does not advertise split admission.
                capabilities: Vec::new(),
                admission_time_unix_ms: None,
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

    #[test]
    fn an_unparseable_version_never_requests_a_downgrade() {
        // A newer daemon binary on disk than the GUI's: no upgrade, whatever the strings say.
        assert!(!should_request_upgrade(
            Some("2026.927.1-beta"),
            "2026.926.1-beta",
            Some(2000),
            Some(1000)
        ));
        // Unparseable and no mtimes to order the builds: stay put.
        assert!(!should_request_upgrade(
            Some("2026.927.1-beta"),
            "2026.926.1-beta",
            None,
            None
        ));
        // Unparseable, but the GUI's binary is strictly newer on disk: upgrade.
        assert!(should_request_upgrade(
            Some("2026.926.1-beta"),
            "2026.927.1-beta",
            Some(1000),
            Some(2000)
        ));
        // Parseable versions keep ordering by CalVer, never by mtime.
        assert!(!should_request_upgrade(
            Some("2026.927.1"),
            "2026.926.1",
            Some(1000),
            Some(2000)
        ));
    }

    #[test]
    fn a_busy_daemon_that_kills_exported_sessions_is_never_handed_over() {
        // The running 2026.926.x daemons kill the sessions they export: replace them only idle.
        assert!(handover_would_lose_sessions(Some("2026.926.2"), 3));
        assert!(!handover_would_lose_sessions(Some("2026.926.2"), 0));
        // A version that preserves exported sessions may be handed over while busy.
        assert!(!handover_would_lose_sessions(Some("2026.927.1"), 3));
        assert!(!handover_would_lose_sessions(Some("2026.1001.1"), 3));
        // No version, or one that cannot be ordered, cannot prove it preserves them.
        assert!(handover_would_lose_sessions(None, 3));
        assert!(handover_would_lose_sessions(Some("dev-local"), 3));
    }

    /// A handover that destroyed 26 of 37 live sessions started here: an older GUI reconnecting to
    /// the freshly upgraded daemon asked for ANOTHER upgrade, because the comparison was `!=`
    /// rather than "am I newer". The second hop killed sessions the first had not yet adopted.
    #[test]
    fn an_older_binary_never_requests_an_upgrade_from_a_newer_daemon() {
        // Older client, newer daemon: must NOT trigger a downgrade hop.
        assert!(!should_request_upgrade(
            Some("2026.925.9"),
            "2026.925.8",
            None,
            None
        ));
        assert!(!should_request_upgrade(
            Some("2026.926.1"),
            "2026.925.9",
            None,
            None
        ));
        assert!(!should_request_upgrade(
            Some("2027.101.1"),
            "2026.925.9",
            None,
            None
        ));

        // The forward direction still upgrades.
        assert!(should_request_upgrade(
            Some("2026.925.8"),
            "2026.925.9",
            None,
            None
        ));

        // Component width differences order numerically, not lexicographically:
        // "2026.925.10" is newer than "2026.925.9" even though it sorts earlier as a string.
        assert!(should_request_upgrade(
            Some("2026.925.9"),
            "2026.925.10",
            None,
            None
        ));
        assert!(!should_request_upgrade(
            Some("2026.925.10"),
            "2026.925.9",
            None,
            None
        ));

        // A non-numeric build string cannot be ordered by version, so only a strictly newer
        // binary on disk may upgrade; without mtimes to compare, nothing does.
        assert!(!should_request_upgrade(
            Some("dev-local"),
            "2026.925.9",
            None,
            None
        ));
        assert!(should_request_upgrade(
            Some("dev-local"),
            "2026.925.9",
            Some(1000),
            Some(2000)
        ));
        assert!(!should_request_upgrade(
            Some("dev-local"),
            "dev-local",
            None,
            None
        ));
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

        let (workspace_id, project_path) =
            tokio::time::timeout(Duration::from_secs(5), teardown_rx)
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

/// Windows stale-daemon identity checks: `tasklist` output parsing and the image-name
/// comparison that gates `taskkill`. Platform independent so the pid-reuse guard is testable
/// without Windows.
#[cfg(test)]
mod tasklist_identity_tests {
    use super::{
        classify_tasklist_liveness, expected_daemon_image_names,
        image_name_matches_expected_daemon_image, parse_tasklist_csv_row,
        stale_daemon_endpoint_files_removable, tasklist_image_name_for_pid, ProcessLiveness,
        StaleDaemonTermination,
    };
    use std::path::Path;

    const TASKLIST_CSV_NO_HEADER: &str = concat!(
        "\"Ferryx.exe\",\"21432\",\"Console\",\"1\",\"412,340 K\"\r\n",
        "\"notepad.exe\",\"777\",\"Console\",\"1\",\"12,345 K\"\r\n"
    );

    #[test]
    fn image_name_is_read_from_the_exact_pid_column() {
        assert_eq!(
            tasklist_image_name_for_pid(TASKLIST_CSV_NO_HEADER, 21432).as_deref(),
            Some("Ferryx.exe")
        );
        assert_eq!(
            tasklist_image_name_for_pid(TASKLIST_CSV_NO_HEADER, 777).as_deref(),
            Some("notepad.exe")
        );
        assert_eq!(tasklist_image_name_for_pid(TASKLIST_CSV_NO_HEADER, 21433), None);
    }

    #[test]
    fn pid_inside_another_rows_fields_is_not_the_requested_process() {
        // 432 appears inside the memory usage of pid 21432 and the session number of the row
        // below. A substring check over the tasklist table reports pid 432 as alive and would
        // force-kill an unrelated process; the pid column comparison must not.
        let csv = concat!(
            "\"Ferryx.exe\",\"21432\",\"Console\",\"432\",\"1,432 K\"\r\n",
            "\"notepad.exe\",\"777\",\"Console\",\"1\",\"432 K\"\r\n"
        );
        assert_eq!(tasklist_image_name_for_pid(csv, 432), None);
        assert_eq!(
            tasklist_image_name_for_pid(csv, 21432).as_deref(),
            Some("Ferryx.exe")
        );
    }

    #[test]
    fn missing_process_and_filter_info_line_report_no_process() {
        assert_eq!(tasklist_image_name_for_pid("", 1234), None);
        assert_eq!(
            tasklist_image_name_for_pid(
                "INFO: No tasks are running which match the specified criteria.\r\n",
                1234
            ),
            None
        );
        assert_eq!(tasklist_image_name_for_pid("\r\n", 1234), None);
    }

    #[test]
    fn csv_fields_keep_commas_inside_quotes_and_empty_fields() {
        assert_eq!(
            parse_tasklist_csv_row("\"Ferryx.exe\",\"21432\",\"Console\",\"1\",\"412,340 K\""),
            vec![
                "Ferryx.exe".to_string(),
                "21432".to_string(),
                "Console".to_string(),
                "1".to_string(),
                "412,340 K".to_string(),
            ]
        );
        assert_eq!(
            parse_tasklist_csv_row("\"Ferryx.exe\",\"\",\"Services\",\"0\",\"N/A\""),
            vec![
                "Ferryx.exe".to_string(),
                String::new(),
                "Services".to_string(),
                "0".to_string(),
                "N/A".to_string(),
            ]
        );
        assert!(parse_tasklist_csv_row("").is_empty());
    }

    #[test]
    fn only_the_running_ferryx_image_matches() {
        let expected =
            expected_daemon_image_names(Some(Path::new("C:/Program Files/Ferryx/Ferryx.exe")));
        assert_eq!(expected, vec!["Ferryx.exe".to_string()]);
        assert!(image_name_matches_expected_daemon_image(
            "Ferryx.exe",
            &expected
        ));
        assert!(image_name_matches_expected_daemon_image(
            "ferryx.EXE",
            &expected
        ));
        // The reported image name may omit the extension the executable keeps.
        assert!(image_name_matches_expected_daemon_image(
            "ferryx",
            &expected
        ));
        assert!(!image_name_matches_expected_daemon_image(
            "notepad.exe",
            &expected
        ));
        assert!(!image_name_matches_expected_daemon_image(
            "ferryx-helper.exe",
            &expected
        ));
        assert!(!image_name_matches_expected_daemon_image("", &expected));
    }

    #[test]
    fn unknown_executable_name_never_matches() {
        assert!(expected_daemon_image_names(None).is_empty());
        let unknown = expected_daemon_image_names(None);
        assert!(!image_name_matches_expected_daemon_image(
            "Ferryx.exe",
            &unknown
        ));
    }

    #[test]
    fn a_daemon_that_could_not_be_terminated_keeps_its_endpoint_files() {
        assert!(stale_daemon_endpoint_files_removable(
            StaleDaemonTermination::Terminated
        ));
        assert!(
            !stale_daemon_endpoint_files_removable(StaleDaemonTermination::NotTerminated),
            "a predecessor that is still alive must keep daemon.port and daemon.lock"
        );
    }

    #[test]
    fn liveness_is_alive_only_when_a_successful_probe_lists_the_pid() {
        let csv = TASKLIST_CSV_NO_HEADER.as_bytes().to_vec();
        assert_eq!(
            classify_tasklist_liveness(Some((true, csv.clone())), 21432),
            ProcessLiveness::Alive("Ferryx.exe".to_string())
        );
        assert_eq!(
            classify_tasklist_liveness(Some((true, csv)), 21433),
            ProcessLiveness::Absent
        );
    }

    #[test]
    fn a_successful_probe_without_the_pid_is_absent() {
        // The documented no-match output of `tasklist /FI ... /NH /FO CSV` at exit code 0.
        assert_eq!(
            classify_tasklist_liveness(
                Some((
                    true,
                    b"INFO: No tasks are running which match the specified criteria.\r\n".to_vec()
                )),
                21432
            ),
            ProcessLiveness::Absent
        );
    }

    #[test]
    fn a_probe_that_could_not_answer_is_unknown_never_absent() {
        // A probe that could not be spawned never produces output at all.
        assert_eq!(
            classify_tasklist_liveness(None, 21432),
            ProcessLiveness::Unknown
        );
        for probe in [
            // `tasklist` missing, blocked, or denied exits non-zero.
            Some((false, Vec::new())),
            // Unreadable bytes cannot be parsed into rows.
            Some((true, b"not utf8 \xff\xfe".to_vec())),
        ] {
            assert_eq!(
                classify_tasklist_liveness(probe, 21432),
                ProcessLiveness::Unknown,
                "an unanswered probe is not a confirmed exit"
            );
        }
    }
}

#[cfg(test)]
mod local_split_transport_tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncReadExt, AsyncWrite};

    struct HeldWriter {
        inner: tokio::io::DuplexStream,
        stage: &'static str,
        entered: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl AsyncWrite for HeldWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.stage == "write" {
                if let Some(entered) = self.entered.take() {
                    entered.send(()).unwrap();
                }
                return Poll::Pending;
            }
            Pin::new(&mut self.inner).poll_write(cx, bytes)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            if self.stage == "flush" {
                if let Some(entered) = self.entered.take() {
                    entered.send(()).unwrap();
                }
                return Poll::Pending;
            }
            Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    struct HeldReader {
        entered: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl tokio::io::AsyncRead for HeldReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Pending
        }
    }

    impl AsyncBufRead for HeldReader {
        fn poll_fill_buf(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<&[u8]>> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
            }
            Poll::Pending
        }
        fn consume(self: Pin<&mut Self>, _amount: usize) {}
    }

    fn identity() -> SplitIdentity {
        SplitIdentity {
            request_id: "98df3cfa-9ea5-4220-b2b6-365f70695e4f".into(),
            origin_epoch: "7".into(),
            expires_at_unix_ms: 601_000,
        }
    }

    fn prepared() -> PreparedLocalSplit {
        PreparedLocalSplit {
            identity: identity(),
            workspace_id: "fixture".into(),
            worktree: None,
            cwd: "/fixture".into(),
            shell: None,
            cols: 80,
            rows: 24,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn local_split_reliability_wire_deadline_transport() {
        for stage in ["write", "flush", "read", "handshakeRead", "connect"] {
            let (stream, mut peer) = tokio::io::duplex(4096);
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let mut tasks = tokio::task::JoinSet::new();
            let deadline = tokio::time::Instant::now() + Duration::from_secs(9);
            tasks.spawn(async move {
                let identity = identity();
                if stage == "connect" {
                    return split_until(
                        deadline,
                        Some(&identity),
                        "connect",
                        SplitDelivery::NotSent,
                        async move {
                            let _owned_socket = stream;
                            entered_tx.send(()).unwrap();
                            std::future::pending::<Result<DaemonResponse, IpcError>>().await
                        },
                    )
                    .await;
                }
                let mut writer = HeldWriter {
                    inner: stream,
                    stage,
                    entered: Some(entered_tx),
                };
                let mut reader = HeldReader {
                    entered: if stage == "read" || stage == "handshakeRead" {
                        writer.entered.take()
                    } else {
                        None
                    },
                };
                split_exchange_until(
                    &mut reader,
                    &mut writer,
                    &DaemonRequest::SpawnOperationStatus {
                        client_request_id: identity.request_id.clone(),
                        origin_epoch: 7,
                        expires_at_unix_ms: identity.expires_at_unix_ms,
                    },
                    Some(&identity),
                    deadline,
                    stage == "handshakeRead",
                )
                .await
            });
            tokio::time::timeout(Duration::from_secs(5), entered_rx)
                .await
                .unwrap()
                .unwrap();
            tokio::time::advance(Duration::from_secs(9)).await;
            let error = tasks.join_next().await.unwrap().unwrap().unwrap_err();
            assert_eq!(error.code, IpcErrorCode::SpawnAttemptTimeout);
            let details = error.details.unwrap();
            assert_eq!(details["stage"], stage);
            assert_eq!(
                details["delivery"],
                if stage == "connect" || stage == "handshakeRead" {
                    "notSent"
                } else {
                    "ambiguous"
                }
            );
            let mut discarded = Vec::new();
            tokio::time::timeout(Duration::from_secs(5), peer.read_to_end(&mut discarded))
                .await
                .unwrap()
                .unwrap();
            assert!(tasks.is_empty());
            eprintln!("LOCAL_SPLIT_TRANSPORT stage={stage} socket_disposed=true workers_joined=true");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn local_split_reliability_expired_before_write_is_not_sent() {
        let (mut writer, mut peer) = tokio::io::duplex(64);
        let (reader, _keepalive) = tokio::io::duplex(64);
        let mut reader = BufReader::new(reader);
        let error = split_exchange_until(
            &mut reader,
            &mut writer,
            &DaemonRequest::Ping,
            Some(&identity()),
            tokio::time::Instant::now(),
            false,
        )
        .await
        .unwrap_err();
        assert_eq!(error.details.unwrap()["delivery"], "notSent");
        drop(writer);
        assert_eq!(
            peer.read_u8().await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    struct Fixture {
        root: tempfile::TempDir,
        #[cfg(unix)]
        listener: tokio::net::UnixListener,
        #[cfg(not(unix))]
        listener: tokio::net::TcpListener,
        client: DaemonClient,
    }

    impl Fixture {
        async fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("transport");
            #[cfg(unix)]
            let listener = tokio::net::UnixListener::bind(&path).unwrap();
            #[cfg(not(unix))]
            let listener = {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                fs::write(&path, listener.local_addr().unwrap().port().to_string()).unwrap();
                listener
            };
            Self {
                root,
                listener,
                client: DaemonClient::new_with_socket(path),
            }
        }

        async fn accept(&self, capable: bool) -> ActiveConnection {
            let (stream, _) = self.listener.accept().await.unwrap();
            let (reader, writer) = stream.into_split();
            let mut connection = ActiveConnection {
                reader: BufReader::new(reader),
                writer,
            };
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::Handshake {
                    version: DAEMON_PROTOCOL_VERSION,
                    ..
                }
            ));
            reply(
                &mut connection,
                json!({"type":"handshakeOk", "version":DAEMON_PROTOCOL_VERSION,
                "pid":1, "epoch":7, "capabilities": if capable { vec![LOCAL_SPLIT_LIFECYCLE_CAPABILITY] } else { vec![] },
                "admissionTimeUnixMs":1000}),
            )
            .await;
            connection
        }
    }

    async fn read_request(connection: &mut ActiveConnection) -> DaemonRequest {
        let mut line = String::new();
        assert!(connection.reader.read_line(&mut line).await.unwrap() > 0);
        serde_json::from_str(&line).unwrap()
    }

    async fn reply(connection: &mut ActiveConnection, value: serde_json::Value) {
        connection
            .writer
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn eof(connection: &mut ActiveConnection) {
        let mut line = String::new();
        assert_eq!(connection.reader.read_line(&mut line).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn local_split_reliability_isolated_lifecycle_bypasses_general_slot() {
        let fixture = Fixture::new().await;
        let general = fixture.client.connection.lock().await;
        let interactive = fixture.client.interactive_connection.lock().await;
        let action = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(9);
            assert_eq!(
                fixture
                    .client
                    .prepare_local_split_until(&identity().request_id, deadline)
                    .await
                    .unwrap(),
                identity()
            );
            let error = fixture
                .client
                .create_local_split_until(&prepared(), deadline)
                .await
                .unwrap_err();
            assert_eq!(error.code, IpcErrorCode::SpawnRequestConflict);
            assert_eq!(error.details.unwrap()["delivery"], "confirmed");
            assert!(matches!(
                fixture
                    .client
                    .local_split_status_until(&identity(), deadline)
                    .await
                    .unwrap(),
                SplitOperationResult::Pending {
                    cancel_requested: false
                }
            ));
            assert!(matches!(
                fixture
                    .client
                    .cancel_local_split_until(&identity(), deadline)
                    .await
                    .unwrap(),
                SplitOperationResult::Pending {
                    cancel_requested: true
                }
            ));
        };
        let peer = async {
            let mut connection = fixture.accept(true).await;
            eof(&mut connection).await;
            let mut connection = fixture.accept(true).await;
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::Spawn {
                    local_split: Some(LocalSplitEnvelope {
                        origin_epoch: 7,
                        remaining_ms: 1..=9000,
                        ..
                    }),
                    ..
                }
            ));
            reply(
                &mut connection,
                json!({"type":"error", "message":"conflict", "code":"SPAWN_REQUEST_CONFLICT"}),
            )
            .await;
            eof(&mut connection).await;
            let mut connection = fixture.accept(true).await;
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::SpawnOperationStatus { .. }
            ));
            reply(
                &mut connection,
                json!({"type":"spawnOperationOk", "operation":{"state":"pending", "cancelRequested":false}}),
            )
            .await;
            eof(&mut connection).await;
            let mut connection = fixture.accept(true).await;
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::CancelSpawnOperation { .. }
            ));
            reply(
                &mut connection,
                json!({"type":"spawnOperationOk", "operation":{"state":"pending", "cancelRequested":true}}),
            )
            .await;
            eof(&mut connection).await;
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(action, peer);
        })
        .await
        .unwrap();
        assert!(!fixture.client.upgrade_requested.load(Ordering::SeqCst));
        drop((general, interactive));
        drop(fixture.listener);
        fixture.root.close().unwrap();
        eprintln!("LOCAL_SPLIT_TRANSPORT isolated=true sockets_disposed=4 cleanup=true");
    }

    #[tokio::test]
    async fn local_split_reliability_unsupported_sends_no_create() {
        let fixture = Fixture::new().await;
        let action = async {
            let error = fixture
                .client
                .create_local_split_until(
                    &prepared(),
                    tokio::time::Instant::now() + Duration::from_secs(9),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, IpcErrorCode::UnsupportedCapability);
            assert_eq!(error.details.unwrap()["delivery"], "notSent");
        };
        let peer = async {
            let mut connection = fixture.accept(false).await;
            eof(&mut connection).await;
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(action, peer);
        })
        .await
        .unwrap();
        assert!(!fixture.client.upgrade_requested.load(Ordering::SeqCst));
        drop(fixture.listener);
        fixture.root.close().unwrap();
    }

    #[tokio::test]
    async fn local_split_reliability_attach_deadline_disposes_private_socket() {
        for handshake_stall in [true, false] {
            let fixture = Fixture::new().await;
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let client = fixture.client.clone();
            let mut tasks = tokio::task::JoinSet::new();
            tasks.spawn(async move {
                client
                    .attach_until(
                        "owned-backend",
                        None,
                        tokio::time::Instant::now() + Duration::from_secs(4),
                    )
                    .await
            });
            let peer = async {
                let mut connection = if handshake_stall {
                    let (stream, _) = fixture.listener.accept().await.unwrap();
                    let (reader, writer) = stream.into_split();
                    let mut connection = ActiveConnection {
                        reader: BufReader::new(reader),
                        writer,
                    };
                    assert!(matches!(
                        read_request(&mut connection).await,
                        DaemonRequest::Handshake { .. }
                    ));
                    connection
                } else {
                    let mut connection = fixture.accept(true).await;
                    assert!(matches!(
                        read_request(&mut connection).await,
                        DaemonRequest::Attach {
                            session_id,
                            after_sequence: None
                        } if session_id == "owned-backend"
                    ));
                    connection
                };
                entered_tx.send(()).unwrap();
                eof(&mut connection).await;
            };
            let check = async {
                entered_rx.await.unwrap();
                tokio::time::pause();
                struct ResumeClock;
                impl Drop for ResumeClock {
                    fn drop(&mut self) {
                        tokio::time::resume();
                    }
                }
                let clock = ResumeClock;
                tokio::time::advance(Duration::from_secs(4)).await;
                let result = tasks.join_next().await.unwrap().unwrap();
                drop(clock);
                let error = match result {
                    Err(error) => error,
                    Ok(attachment) => {
                        attachment.stream_task.abort();
                        panic!("stalled attach unexpectedly succeeded");
                    }
                };
                let details = error.details.unwrap();
                assert_eq!(error.code, IpcErrorCode::SpawnAttemptTimeout);
                assert_eq!(
                    details["stage"],
                    if handshake_stall {
                        "handshakeRead"
                    } else {
                        "read"
                    }
                );
                assert_eq!(
                    details["delivery"],
                    if handshake_stall {
                        "notSent"
                    } else {
                        "ambiguous"
                    }
                );
            };
            tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(peer, check);
            })
            .await
            .unwrap();
            assert!(!fixture.client.upgrade_requested.load(Ordering::SeqCst));
            assert!(tasks.is_empty());
            drop(fixture.listener);
            fixture.root.close().unwrap();
            eprintln!(
                "LOCAL_SPLIT_TRANSPORT attach_stall=true socket_disposed=true no_close=true cleanup=true"
            );
        }
    }

    #[tokio::test]
    async fn local_split_reliability_attach_stream_outlives_deadline_and_disposes_on_drop() {
        let fixture = Fixture::new().await;
        let (attached_tx, attached_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let action = async {
            let attachment = fixture
                .client
                .attach_until(
                    "owned-backend",
                    Some(17),
                    tokio::time::Instant::now() + Duration::from_secs(4),
                )
                .await
                .unwrap();
            assert_eq!(attachment.session_id, "owned-backend");
            assert_eq!(attachment.epoch, 7);
            attached_tx.send(()).unwrap();
            release_rx.await.unwrap();
            assert!(!attachment.stream_task.is_finished());
            drop(attachment.messages);
            attachment.stream_task.await.unwrap();
        };
        let peer = async {
            let mut connection = fixture.accept(true).await;
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::Attach {
                    after_sequence: Some(17),
                    ..
                }
            ));
            reply(
                &mut connection,
                json!({"type":"attachOk", "epoch":7, "sessionId":"owned-backend",
                "startSequence":17, "endSequence":17, "gap":null, "history":"", "historySegments":[],
                "ptyCols":80, "ptyRows":24, "remoteGeneration":null}),
            )
            .await;
            attached_rx.await.unwrap();
            tokio::time::pause();
            struct ResumeClock;
            impl Drop for ResumeClock {
                fn drop(&mut self) {
                    tokio::time::resume();
                }
            }
            let clock = ResumeClock;
            tokio::time::advance(Duration::from_secs(10)).await;
            let mut byte = [0_u8; 1];
            std::future::poll_fn(|cx| {
                let mut buffer = tokio::io::ReadBuf::new(&mut byte);
                match tokio::io::AsyncRead::poll_read(
                    Pin::new(&mut connection.reader),
                    cx,
                    &mut buffer,
                ) {
                    Poll::Pending => Poll::Ready(()),
                    Poll::Ready(result) => panic!(
                        "attachment lost its write half or sent unexpected control: {result:?}"
                    ),
                }
            })
            .await;
            drop(clock);
            release_tx.send(()).unwrap();
            eof(&mut connection).await;
        };
        tokio::time::timeout(Duration::from_secs(15), async {
            tokio::join!(action, peer);
        })
        .await
        .unwrap();
        assert!(!fixture.client.upgrade_requested.load(Ordering::SeqCst));
        drop(fixture.listener);
        fixture.root.close().unwrap();
        eprintln!(
            "LOCAL_SPLIT_TRANSPORT attach_stream_retained=true receiver_drop_disposes=true cleanup=true"
        );
    }

    #[tokio::test]
    async fn local_split_reliability_lost_create_reply_is_ambiguous_without_retry() {
        let fixture = Fixture::new().await;
        let action = async {
            let error = fixture
                .client
                .create_local_split_until(
                    &prepared(),
                    tokio::time::Instant::now() + Duration::from_secs(9),
                )
                .await
                .unwrap_err();
            assert_eq!(error.details.unwrap()["delivery"], "ambiguous");
            assert!(matches!(
                fixture
                    .client
                    .local_split_status_until(
                        &identity(),
                        tokio::time::Instant::now() + Duration::from_secs(9)
                    )
                    .await
                    .unwrap(),
                SplitOperationResult::Pending { .. }
            ));
        };
        let peer = async {
            let mut connection = fixture.accept(true).await;
            assert!(matches!(
                read_request(&mut connection).await,
                DaemonRequest::Spawn { .. }
            ));
            drop(connection);
            let mut connection = fixture.accept(true).await;
            assert!(
                matches!(
                    read_request(&mut connection).await,
                    DaemonRequest::SpawnOperationStatus { .. }
                ),
                "Create was retransmitted"
            );
            reply(
                &mut connection,
                json!({"type":"spawnOperationOk", "operation":{"state":"pending", "cancelRequested":false}}),
            )
            .await;
            eof(&mut connection).await;
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(action, peer);
        })
        .await
        .unwrap();
        drop(fixture.listener);
        fixture.root.close().unwrap();
    }
}
