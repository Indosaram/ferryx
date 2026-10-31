//! Remote Browser Backend Trait & Providers (Local IPC & Unavailable)
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§1.1, §1.3, §4.4, §6.3)

use crate::browser::manager::BrowserManager;
use crate::browser::remote_service::BrowserRemoteService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopScope {
    pub workspace_id: String,
    pub worktree_slug: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBrowserSessionSummary {
    pub browser_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSubscribeIdentity {
    pub browser_instance_id: String,
    pub browser_service_epoch: String,
    pub desktop_epoch: String,
    pub document_generation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRemoteState {
    pub browser_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_instance_id: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub document_generation: String,
    pub viewport_revision: String,
    pub loading: bool,
    pub paused: bool,
    pub pause_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCapabilities {
    pub browser_available: bool,
    pub supported_formats: Vec<String>,
    pub supported_commands: Vec<String>,
    pub max_edge: u32,
    pub max_fps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteBrowserError {
    #[error("Browser unavailable: {0}")]
    Unavailable(String),
    #[error("Browser session not found: {0}")]
    NotFound(String),
    #[error("Security or permission violation: {0}")]
    Forbidden(String),
    #[error("Invalid request parameters: {0}")]
    InvalidRequest(String),
    #[error("Wait timeout expired")]
    WaitTimeout,
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    /// The command's target element or reference could not be resolved.
    #[error("Browser target not found: {0}")]
    TargetNotFound(String),
    /// The command was issued against a frame/viewport revision that is no longer current.
    #[error("Stale frame: {0}")]
    StaleFrame(String),
    /// Remote input was refused for a typed reason (iframe / canvas / dialog point input).
    /// The explanation travels to the client as the `details` field of the BrowserError frame.
    #[error("Input refused: {0}")]
    InputRefused(crate::browser::remote_input::RefusedInputExplanation),
    /// Another device owns the driver lease. Retryable once that lease ends.
    #[error("Driver busy: {0}")]
    DriverBusy(String),
    /// The driver-claim rate limit was exceeded. Retryable after the window passes.
    #[error("Rate limited: {0}")]
    RateLimited(String),
}

/// Maps a driver-broker failure onto its own typed error. Classification is by variant only,
/// and the busy message never names the current owner, which is another device's identity.
pub(crate) fn map_driver_error(
    err: crate::browser::remote_driver::RemoteDriverError,
) -> RemoteBrowserError {
    use crate::browser::remote_driver::RemoteDriverError;
    match err {
        RemoteDriverError::BrowserDriverBusy { .. } => {
            RemoteBrowserError::DriverBusy("another device holds the driver lease".into())
        }
        RemoteDriverError::RateLimited => {
            RemoteBrowserError::RateLimited("driver claim rate limit exceeded".into())
        }
        other @ (RemoteDriverError::Unauthorized(_)
        | RemoteDriverError::StaleLease { .. }
        | RemoteDriverError::DesktopReclaimed
        | RemoteDriverError::InvalidLease(_)) => RemoteBrowserError::Forbidden(other.to_string()),
    }
}

/// Classifies a GUI error envelope by its structured `code` field, never by message text.
/// `details` is the optional structured payload the GUI attaches to typed input refusals.
pub(crate) fn typed_ipc_error(
    code: &str,
    msg: &str,
    details: Option<&serde_json::Value>,
) -> RemoteBrowserError {
    match code {
        "UNSUPPORTED" => {
            let explanation = details
                .and_then(|d| {
                    serde_json::from_value::<crate::browser::remote_input::RefusedInputExplanation>(
                        d.clone(),
                    )
                    .ok()
                })
                .unwrap_or_else(|| crate::browser::remote_input::RefusedInputExplanation {
                    code: "UNSUPPORTED".to_string(),
                    reason: msg.to_string(),
                    input_class: "unsupported".to_string(),
                    remediation: String::new(),
                });
            RemoteBrowserError::InputRefused(explanation)
        }
        "BROWSER_UNAVAILABLE" => RemoteBrowserError::Unavailable(msg.to_string()),
        "BROWSER_DRIVER_BUSY" => RemoteBrowserError::DriverBusy(msg.to_string()),
        "RATE_LIMITED" => RemoteBrowserError::RateLimited(msg.to_string()),
        _ => RemoteBrowserError::ExecutionFailed(msg.to_string()),
    }
}

/// Default bound on each GUI round-trip step (handshake reply, response frame).
const DEFAULT_IPC_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Longest handshake reply line accepted from the GUI.
#[cfg(unix)]
const MAX_HANDSHAKE_LINE_BYTES: u64 = 64 * 1024;

/// Reads one newline-terminated line of at most `MAX_HANDSHAKE_LINE_BYTES`.
#[cfg(unix)]
async fn read_bounded_line<R>(reader: &mut R) -> Result<String, RemoteBrowserError>
where
    R: tokio::io::AsyncRead + tokio::io::AsyncBufRead + Unpin,
{
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let mut line = String::new();
    let mut limited = reader.take(MAX_HANDSHAKE_LINE_BYTES);
    limited.read_line(&mut line).await.map_err(|e| {
        RemoteBrowserError::Unavailable(format!("Failed to read handshake response: {e}"))
    })?;
    if !line.ends_with('\n') {
        return Err(RemoteBrowserError::Unavailable(
            "Handshake response was not a complete line within the size limit".into(),
        ));
    }
    Ok(line)
}

/// Reads one length-prefixed IPC frame. The prefix is checked against the per-type cap before
/// any buffer is allocated, so a desynced or hostile peer cannot request a multi-GiB buffer.
#[cfg(unix)]
async fn read_ipc_frame<R>(reader: &mut R) -> Result<(u8, Vec<u8>), RemoteBrowserError>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut header = [0u8; 5];
    reader.read_exact(&mut header).await.map_err(|e| {
        RemoteBrowserError::ExecutionFailed(format!("Failed to read response frame header: {e}"))
    })?;
    let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
    let content_type = header[4];
    let max_len = if content_type == LocalIpcBrowserBackend::IPC_CONTENT_TYPE_IMAGE {
        LocalIpcBrowserBackend::MAX_IMAGE_PAYLOAD
    } else {
        LocalIpcBrowserBackend::MAX_JSON_PAYLOAD
    };
    if payload_len > max_len {
        return Err(RemoteBrowserError::ExecutionFailed(format!(
            "IPC frame payload too large: {payload_len} bytes (max {max_len})"
        )));
    }
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        reader.read_exact(&mut payload).await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to read response payload: {e}"))
        })?;
    }
    Ok((content_type, payload))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommandContext {
    pub browser_id: String,
    pub command: String,
    pub params: Option<serde_json::Value>,
    pub document_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_epoch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_epoch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommandResult {
    pub success: bool,
    pub value: Option<serde_json::Value>,
}

pub trait RemoteBrowserBackend: Send + Sync {
    fn list_sessions<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>>;

    fn identify_session<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>>;

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>>;

    fn execute_command(
        &self,
        ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>>;

    fn subscribe_frames<'a>(
        &'a self,
        browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        let _ = browser_id;
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast frame subscription unavailable on this backend".into(),
            ))
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<
        'a,
        Result<
            (
                String,
                u32,
                crate::remote::browser_protocol::BrowserSubscribeOptions,
                BrowserSubscribeIdentity,
            ),
            RemoteBrowserError,
        >,
    > {
        let _ = (browser_id, device_id, viewer_instance_id);
        Box::pin(async move {
            Ok((
                format!("sub-{}", uuid::Uuid::new_v4()),
                1,
                options.unwrap_or_default(),
                BrowserSubscribeIdentity {
                    browser_instance_id: "bi1".into(),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: "1".into(),
                },
            ))
        })
    }

    fn claim_driver<'a>(
        &'a self,
        _browser_id: &'a str,
        _device_id: &'a str,
        _connection_id: &'a str,
        _subscription_id: &'a str,
        _lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move { Ok(()) })
    }

    fn release_driver<'a>(
        &'a self,
        _subscription_id: &'a str,
        _lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move { Ok(()) })
    }

    fn heartbeat_driver<'a>(
        &'a self,
        _device_id: &'a str,
        _connection_id: &'a str,
        _subscription_id: &'a str,
        _lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move { Ok(()) })
    }

    fn unsubscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        subscription_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        let _ = (browser_id, subscription_id);
        Box::pin(async move { Ok(()) })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities>;
}

/// Unavailable provider: used when running headless or when GUI is disconnected
pub struct UnavailableBrowserBackend;

impl RemoteBrowserBackend for UnavailableBrowserBackend {
    fn list_sessions<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn identify_session<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn get_state<'a>(
        &'a self,
        _browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn execute_command(
        &self,
        _ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn subscribe_frames<'a>(
        &'a self,
        _browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        _browser_id: &'a str,
        _device_id: &'a str,
        _viewer_instance_id: &'a str,
        _options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<
        'a,
        Result<
            (
                String,
                u32,
                crate::remote::browser_protocol::BrowserSubscribeOptions,
                BrowserSubscribeIdentity,
            ),
            RemoteBrowserError,
        >,
    > {
        Box::pin(async move {
            Err(RemoteBrowserError::Unavailable(
                "Browser screencast is unavailable without GUI session".into(),
            ))
        })
    }

    fn unsubscribe_viewer<'a>(
        &'a self,
        _browser_id: &'a str,
        _subscription_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move { Ok(()) })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities> {
        Box::pin(async move {
            BrowserCapabilities {
                browser_available: false,
                supported_formats: Vec::new(),
                supported_commands: Vec::new(),
                max_edge: 0,
                max_fps: 0,
            }
        })
    }
}

pub trait BrowserCommandExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        ctx: BrowserCommandContext,
    ) -> BoxFuture<'a, Result<BrowserCommandResult, RemoteBrowserError>>;
}

pub fn normalize_wait_params(
    params: &serde_json::Value,
) -> Result<
    (
        crate::browser::model::BrowserWaitCondition,
        Option<std::time::Duration>,
    ),
    RemoteBrowserError,
> {
    let timeout = params
        .get("timeoutMs")
        .or_else(|| params.get("timeout_ms"))
        .and_then(|v| v.as_u64())
        .map(std::time::Duration::from_millis);

    if let Some(cond_val) = params.get("condition") {
        if let Some(s) = cond_val.as_str() {
            return Ok((
                crate::browser::model::BrowserWaitCondition::Function {
                    script: s.to_string(),
                },
                timeout,
            ));
        }
        if let Ok(cond) =
            serde_json::from_value::<crate::browser::model::BrowserWaitCondition>(cond_val.clone())
        {
            return Ok((cond, timeout));
        }
    }

    if let Some(script) = params.get("script").and_then(|v| v.as_str()) {
        return Ok((
            crate::browser::model::BrowserWaitCondition::Function {
                script: script.to_string(),
            },
            timeout,
        ));
    }

    if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
        return Ok((
            crate::browser::model::BrowserWaitCondition::Selector {
                selector: selector.to_string(),
            },
            timeout,
        ));
    }

    Err(RemoteBrowserError::InvalidRequest(
        "Missing or invalid wait condition: expected string script, tagged condition, or selector"
            .into(),
    ))
}

/// Reason this build cannot serve the local browser IPC transport.
///
/// The `#[cfg(not(unix))]` stubs in `send_framed_request` and
/// `subscribe_viewer` report `RemoteBrowserError::Unavailable` with this same
/// text: Windows publishes a loopback port (`runtime/browser.port`) that this
/// backend has no TCP transport for, so the capability probe must say so
/// instead of probing a socket path that can never exist.
const LOCAL_IPC_TRANSPORT_UNSUPPORTED_REASON: &str =
    "Local browser IPC socket is not supported on this platform";

/// Whether this build can speak the GUI's local browser IPC transport.
const LOCAL_IPC_TRANSPORT_SUPPORTED: bool = cfg!(unix);

/// Availability of the local browser IPC transport, plus the reason it is
/// unavailable. Pure apart from the path existence probe, so the capability
/// probe is unit-testable on every platform without a live socket.
fn local_ipc_availability(
    transport_supported: bool,
    socket_path: &str,
) -> (bool, Option<&'static str>) {
    if !transport_supported {
        return (false, Some(LOCAL_IPC_TRANSPORT_UNSUPPORTED_REASON));
    }
    if socket_path.is_empty() {
        return (false, Some("Local IPC socket path empty"));
    }
    if !std::path::Path::new(socket_path).exists() {
        return (false, Some("GUI process not running or socket unconnected"));
    }
    (true, None)
}

/// Local IPC framed provider connecting daemon to GUI process (§1.1, §4.4)
pub struct LocalIpcBrowserBackend {
    socket_path: String,
    local_credential: Option<String>,
    io_timeout: std::time::Duration,
    frame_senders:
        Arc<parking_lot::Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>>,
    active_subscriptions: Arc<tokio::sync::Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

fn json_val_to_decimal_string(val: &serde_json::Value) -> Option<String> {
    match val {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl LocalIpcBrowserBackend {
    pub const IPC_CONTENT_TYPE_JSON: u8 = 0x01;
    pub const IPC_CONTENT_TYPE_IMAGE: u8 = 0x02;
    pub const MAX_JSON_PAYLOAD: usize = 512 * 1024; // 512 KiB
    pub const MAX_IMAGE_PAYLOAD: usize = 2 * 1024 * 1024; // 2 MiB

    pub fn new(socket_path: String, local_credential: Option<String>) -> Self {
        Self {
            socket_path,
            local_credential,
            io_timeout: DEFAULT_IPC_IO_TIMEOUT,
            frame_senders: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            active_subscriptions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Overrides the bound on each GUI round-trip step (handshake reply, response frame).
    pub fn with_io_timeout(mut self, io_timeout: std::time::Duration) -> Self {
        self.io_timeout = io_timeout;
        self
    }

    pub fn publish_frame(&self, browser_id: &str, frame: Vec<u8>) {
        let senders = self.frame_senders.lock();
        if let Some(sender) = senders.get(browser_id) {
            let _ = sender.send(frame);
        }
    }

    pub fn encode_ipc_frame(
        content_type: u8,
        payload: &[u8],
    ) -> Result<Vec<u8>, RemoteBrowserError> {
        let max_len = if content_type == Self::IPC_CONTENT_TYPE_IMAGE {
            Self::MAX_IMAGE_PAYLOAD
        } else {
            Self::MAX_JSON_PAYLOAD
        };

        if payload.len() > max_len {
            return Err(RemoteBrowserError::InvalidRequest(format!(
                "Payload exceeds limit ({} > {})",
                payload.len(),
                max_len
            )));
        }

        let mut out = Vec::with_capacity(5 + payload.len());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.push(content_type);
        out.extend_from_slice(payload);
        Ok(out)
    }

    pub fn decode_ipc_frame(
        bytes: &[u8],
    ) -> Result<Option<(u8, Vec<u8>, usize)>, RemoteBrowserError> {
        if bytes.len() < 5 {
            return Ok(None);
        }
        let payload_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let content_type = bytes[4];
        let total_frame_len = 5 + payload_len;

        if bytes.len() < total_frame_len {
            return Ok(None);
        }

        let payload = bytes[5..total_frame_len].to_vec();
        Ok(Some((content_type, payload, total_frame_len)))
    }

    #[cfg(not(unix))]
    async fn send_framed_request(
        &self,
        _request_bytes: &[u8],
    ) -> Result<Vec<u8>, RemoteBrowserError> {
        Err(RemoteBrowserError::Unavailable(
            LOCAL_IPC_TRANSPORT_UNSUPPORTED_REASON.into(),
        ))
    }

    #[cfg(unix)]
    async fn send_framed_request(
        &self,
        request_bytes: &[u8],
    ) -> Result<Vec<u8>, RemoteBrowserError> {
        use tokio::io::{AsyncWriteExt, BufReader};

        if self.socket_path.is_empty() {
            return Err(RemoteBrowserError::Unavailable(
                "Local IPC socket path empty".into(),
            ));
        }
        let sock_path = std::path::Path::new(&self.socket_path);
        if !sock_path.exists() {
            return Err(RemoteBrowserError::Unavailable(
                "GUI process not running or socket unconnected".into(),
            ));
        }
        let stream = tokio::net::UnixStream::connect(sock_path)
            .await
            .map_err(|e| {
                RemoteBrowserError::Unavailable(format!(
                    "GUI process not running or socket unconnected: {e}"
                ))
            })?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let token = if let Some(ref t) = self.local_credential {
            t.clone()
        } else {
            let token_path =
                sock_path.with_file_name(match sock_path.file_name().and_then(|n| n.to_str()) {
                    Some(name) => format!("{name}.token"),
                    None => "browser.token".to_string(),
                });
            read_local_ipc_token(&token_path).await
        };

        let handshake = serde_json::json!({
            "command": "remoteAttach",
            "token": token,
        });
        let mut handshake_bytes = serde_json::to_vec(&handshake)
            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
        handshake_bytes.push(b'\n');
        writer.write_all(&handshake_bytes).await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("Failed to write handshake: {e}"))
        })?;
        writer.flush().await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("Failed to flush handshake: {e}"))
        })?;

        let resp_line = tokio::time::timeout(self.io_timeout, read_bounded_line(&mut reader))
            .await
            .map_err(|_| {
                RemoteBrowserError::Unavailable("Timed out waiting for the GUI handshake".into())
            })??;

        let resp_val: serde_json::Value = serde_json::from_str(resp_line.trim()).map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Invalid handshake response: {e}"))
        })?;
        if resp_val.get("type").and_then(|v| v.as_str()) != Some("remoteAttached") {
            let err_msg = resp_val
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Handshake rejected");
            return Err(RemoteBrowserError::Forbidden(err_msg.to_string()));
        }

        let frame = Self::encode_ipc_frame(Self::IPC_CONTENT_TYPE_JSON, request_bytes)?;
        writer.write_all(&frame).await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to write framed request: {e}"))
        })?;
        writer.flush().await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to flush framed request: {e}"))
        })?;

        let (_, payload) = tokio::time::timeout(self.io_timeout, read_ipc_frame(&mut reader))
            .await
            .map_err(|_| {
                RemoteBrowserError::Unavailable("Timed out waiting for the GUI response".into())
            })??;
        Ok(payload)
    }
}

pub fn matches_worktree_slug(
    session_worktree_path: Option<&str>,
    scope_worktree_slug: &str,
) -> bool {
    if scope_worktree_slug.is_empty() {
        return true;
    }
    let Some(path) = session_worktree_path else {
        return false;
    };
    if path == scope_worktree_slug {
        return true;
    }
    let norm = path.replace('\\', "/");
    let slug = scope_worktree_slug;
    if norm.ends_with(&format!("/{slug}"))
        || norm.ends_with(&format!("/wt-{slug}"))
        || norm.contains(&format!("/wt-{slug}/"))
        || crate::worktree::naming::managed_worktree_subdir(&norm, slug).is_some()
    {
        return true;
    }
    if let Some(inferred) =
        crate::ipc::terminal::infer_worktree_slug(None, Some(std::path::Path::new(path)))
    {
        if inferred == slug {
            return true;
        }
    }
    false
}

impl RemoteBrowserBackend for LocalIpcBrowserBackend {
    fn list_sessions<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::List {
                workspace_id: if scope.workspace_id.is_empty() {
                    None
                } else {
                    Some(scope.workspace_id.clone())
                },
                worktree_slug: if scope.worktree_slug.is_empty() {
                    None
                } else {
                    Some(scope.worktree_slug.clone())
                },
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize list request: {e}"
                ))
            })?;
            let payload = self.send_framed_request(&req_bytes).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to parse session list: {e}"))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("List failed");
                return match err_code {
                    "BROWSER_CLI_REQUEST_INVALID" | "BROWSER_INVALID_REQUEST" => {
                        Err(RemoteBrowserError::InvalidRequest(msg.to_string()))
                    }
                    "BROWSER_CLI_UNAUTHORIZED" => {
                        Err(RemoteBrowserError::Forbidden(msg.to_string()))
                    }
                    _ => Err(typed_ipc_error(err_code, msg, val.get("details"))),
                };
            }
            let data = val.get("result").unwrap_or(&val);
            let items = if let Some(arr) = data.as_array() {
                arr.clone()
            } else if let Some(arr) = data.get("sessions").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                return Err(RemoteBrowserError::ExecutionFailed(
                    "Unrecognized session list response shape".into(),
                ));
            };

            let mut out = Vec::new();
            for item in items {
                // An item without an ID cannot be addressed by any later command: skip it.
                let Some(browser_id) = item
                    .get("browserId")
                    .or_else(|| item.get("browser_id"))
                    .and_then(|v| v.as_str())
                    .filter(|id| !id.is_empty())
                    .map(str::to_string)
                else {
                    continue;
                };
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let visible = item
                    .get("visible")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                let ws_id = item
                    .get("workspaceId")
                    .or_else(|| item.get("workspace_id"))
                    .and_then(|v| v.as_str());
                let wt_path = item
                    .get("worktreePath")
                    .or_else(|| item.get("worktree_path"))
                    .and_then(|v| v.as_str());
                let wt_slug = item
                    .get("worktreeSlug")
                    .or_else(|| item.get("worktree_slug"))
                    .and_then(|v| v.as_str());

                let ws_match = scope.workspace_id.is_empty() || ws_id == Some(&scope.workspace_id);
                let wt_match = if scope.worktree_slug.is_empty() {
                    true
                } else {
                    wt_slug == Some(&scope.worktree_slug)
                        || matches_worktree_slug(wt_path, &scope.worktree_slug)
                };

                if ws_match && wt_match {
                    out.push(RemoteBrowserSessionSummary {
                        browser_id,
                        title,
                        url,
                        visible,
                    });
                }
            }
            Ok(out)
        })
    }

    fn identify_session<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            let list = self.list_sessions(scope).await?;
            Ok(list
                .iter()
                .find(|s| s.visible)
                .cloned()
                .or_else(|| list.into_iter().next()))
        })
    }

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::GetState {
                browser_id: browser_id.to_string(),
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize getState request: {e}"
                ))
            })?;
            let payload = self.send_framed_request(&req_bytes).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to parse getState response: {e}"
                ))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("getState failed");
                return match err_code {
                    "BROWSER_NOT_FOUND" | "BROWSER_CLI_NOT_FOUND" => {
                        Err(RemoteBrowserError::NotFound(msg.to_string()))
                    }
                    "BROWSER_CLI_REQUEST_INVALID" | "BROWSER_INVALID_REQUEST" => {
                        Err(RemoteBrowserError::InvalidRequest(msg.to_string()))
                    }
                    _ => Err(typed_ipc_error(err_code, msg, val.get("details"))),
                };
            }
            let data = val.get("result").unwrap_or(&val);
            if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                return Err(RemoteBrowserError::ExecutionFailed(err.to_string()));
            }
            let inst_id = data
                .get("browserInstanceId")
                .or_else(|| data.get("instanceId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let doc_gen = data
                .get("documentGeneration")
                .or_else(|| data.get("generation"))
                .and_then(json_val_to_decimal_string)
                .ok_or_else(|| {
                    RemoteBrowserError::ExecutionFailed(
                        "Missing document generation in getState response".into(),
                    )
                })?;

            let vp_rev = data
                .get("viewportRevision")
                .and_then(json_val_to_decimal_string)
                .ok_or_else(|| {
                    RemoteBrowserError::ExecutionFailed(
                        "Missing viewport revision in getState response".into(),
                    )
                })?;

            let visible = data
                .get("visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            Ok(BrowserRemoteState {
                browser_id: data
                    .get("browserId")
                    .and_then(|v| v.as_str())
                    .unwrap_or(browser_id)
                    .to_string(),
                browser_instance_id: inst_id,
                url: data
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                title: data
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                document_generation: doc_gen,
                viewport_revision: vp_rev,
                loading: data
                    .get("loading")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                paused: !visible,
                pause_reason: if !visible {
                    Some("hidden".into())
                } else {
                    None
                },
            })
        })
    }

    fn execute_command(
        &self,
        ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::Execute {
                browser_id: ctx.browser_id,
                command: ctx.command,
                params: ctx.params,
                document_generation: ctx.document_generation,
                browser_instance_id: ctx.browser_instance_id,
                desktop_epoch: ctx.desktop_epoch,
                lease_epoch: ctx.lease_epoch,
                device_id: ctx.device_id,
                connection_id: ctx.connection_id,
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize execute request: {e}"
                ))
            })?;
            let payload = self.send_framed_request(&req_bytes).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to parse command response: {e}"
                ))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Operation failed");
                return match err_code {
                    "BROWSER_CLI_REQUEST_INVALID" | "BROWSER_INVALID_REQUEST" => {
                        Err(RemoteBrowserError::InvalidRequest(msg.to_string()))
                    }
                    "BROWSER_WAIT_TIMEOUT" => Err(RemoteBrowserError::WaitTimeout),
                    "BROWSER_CLI_UNAUTHORIZED" | "BROWSER_FORBIDDEN" => {
                        Err(RemoteBrowserError::Forbidden(msg.to_string()))
                    }
                    "BROWSER_NOT_FOUND" => Err(RemoteBrowserError::NotFound(msg.to_string())),
                    "BROWSER_TARGET_NOT_FOUND" => {
                        Err(RemoteBrowserError::TargetNotFound(msg.to_string()))
                    }
                    "BROWSER_STALE_FRAME" => Err(RemoteBrowserError::StaleFrame(msg.to_string())),
                    _ => Err(typed_ipc_error(err_code, msg, val.get("details"))),
                };
            }
            let result_val = val.get("result").cloned().or(Some(val));
            Ok(BrowserCommandResult {
                success: true,
                value: result_val,
            })
        })
    }

    fn subscribe_frames<'a>(
        &'a self,
        browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable(
                    "Local IPC socket path empty".into(),
                ));
            }
            if !std::path::Path::new(&self.socket_path).exists() {
                return Err(RemoteBrowserError::Unavailable(
                    "GUI process not running or socket unconnected".into(),
                ));
            }
            let mut senders = self.frame_senders.lock();
            let sender = senders
                .entry(browser_id.to_string())
                .or_insert_with(|| tokio::sync::broadcast::channel(16).0);
            Ok(sender.subscribe())
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<
        'a,
        Result<
            (
                String,
                u32,
                crate::remote::browser_protocol::BrowserSubscribeOptions,
                BrowserSubscribeIdentity,
            ),
            RemoteBrowserError,
        >,
    > {
        #[cfg(not(unix))]
        {
            let _ = (browser_id, device_id, viewer_instance_id, options);
            Box::pin(async move {
                Err(RemoteBrowserError::Unavailable(
                    LOCAL_IPC_TRANSPORT_UNSUPPORTED_REASON.into(),
                ))
            })
        }
        #[cfg(unix)]
        Box::pin(async move {
            use tokio::io::{AsyncWriteExt, BufReader};

            let req = crate::ipc::browser_cli::RemoteBrowserOperation::SubscribeViewer {
                browser_id: browser_id.to_string(),
                device_id: device_id.to_string(),
                viewer_instance_id: viewer_instance_id.to_string(),
                options: options.clone(),
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize subscribe request: {e}"
                ))
            })?;

            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable(
                    "Local IPC socket path empty".into(),
                ));
            }
            let sock_path = std::path::Path::new(&self.socket_path);
            if !sock_path.exists() {
                return Err(RemoteBrowserError::Unavailable(
                    "GUI process not running or socket unconnected".into(),
                ));
            }
            let stream = tokio::net::UnixStream::connect(sock_path)
                .await
                .map_err(|e| {
                    RemoteBrowserError::Unavailable(format!(
                        "GUI process not running or socket unconnected: {e}"
                    ))
                })?;

            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);

            let token = if let Some(ref t) = self.local_credential {
                t.clone()
            } else {
                let token_path = sock_path.with_file_name(
                    match sock_path.file_name().and_then(|n| n.to_str()) {
                        Some(name) => format!("{name}.token"),
                        None => "browser.token".to_string(),
                    },
                );
                read_local_ipc_token(&token_path).await
            };

            let handshake = serde_json::json!({
                "command": "remoteAttach",
                "token": token,
            });
            let mut handshake_bytes = serde_json::to_vec(&handshake)
                .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
            handshake_bytes.push(b'\n');
            writer.write_all(&handshake_bytes).await.map_err(|e| {
                RemoteBrowserError::Unavailable(format!("Failed to write handshake: {e}"))
            })?;
            writer.flush().await.map_err(|e| {
                RemoteBrowserError::Unavailable(format!("Failed to flush handshake: {e}"))
            })?;

            let resp_line = tokio::time::timeout(self.io_timeout, read_bounded_line(&mut reader))
                .await
                .map_err(|_| {
                    RemoteBrowserError::Unavailable(
                        "Timed out waiting for the GUI handshake".into(),
                    )
                })??;

            let resp_val: serde_json::Value =
                serde_json::from_str(resp_line.trim()).map_err(|e| {
                    RemoteBrowserError::ExecutionFailed(format!("Invalid handshake response: {e}"))
                })?;
            if resp_val.get("type").and_then(|v| v.as_str()) != Some("remoteAttached") {
                let err_msg = resp_val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Handshake rejected");
                return Err(RemoteBrowserError::Forbidden(err_msg.to_string()));
            }

            let frame = Self::encode_ipc_frame(Self::IPC_CONTENT_TYPE_JSON, &req_bytes)?;
            writer.write_all(&frame).await.map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to write framed request: {e}"))
            })?;
            writer.flush().await.map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to flush framed request: {e}"))
            })?;

            let (_, payload) = tokio::time::timeout(self.io_timeout, read_ipc_frame(&mut reader))
                .await
                .map_err(|_| {
                    RemoteBrowserError::Unavailable("Timed out waiting for the GUI response".into())
                })??;

            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to parse subscribe response: {e}"
                ))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Subscribe failed");
                return match err_code {
                    "BROWSER_NOT_FOUND" => Err(RemoteBrowserError::NotFound(msg.to_string())),
                    "BROWSER_SUBSCRIPTION_FAILED" => {
                        Err(RemoteBrowserError::ExecutionFailed(msg.to_string()))
                    }
                    "BROWSER_CLI_REQUEST_INVALID" | "BROWSER_INVALID_REQUEST" => {
                        Err(RemoteBrowserError::InvalidRequest(msg.to_string()))
                    }
                    "BROWSER_CLI_UNAUTHORIZED" | "BROWSER_FORBIDDEN" => {
                        Err(RemoteBrowserError::Forbidden(msg.to_string()))
                    }
                    _ => Err(typed_ipc_error(err_code, msg, val.get("details"))),
                };
            }
            let data = val.get("result").unwrap_or(&val);
            let sub_id = data
                .get("subscriptionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    RemoteBrowserError::ExecutionFailed(
                        "Missing subscriptionId in subscribe response".into(),
                    )
                })?
                .to_string();
            let stream_id = data
                .get("streamId")
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| {
                    RemoteBrowserError::ExecutionFailed(
                        "Missing or invalid streamId in subscribe response".into(),
                    )
                })?;
            let negotiated_opts: crate::remote::browser_protocol::BrowserSubscribeOptions = data
                .get("options")
                .ok_or_else(|| {
                    RemoteBrowserError::ExecutionFailed(
                        "Missing options in subscribe response".into(),
                    )
                })
                .and_then(|opts_val| {
                    serde_json::from_value(opts_val.clone()).map_err(|e| {
                        RemoteBrowserError::ExecutionFailed(format!(
                            "Failed to parse negotiated options: {e}"
                        ))
                    })
                })?;
            let ident_val = data.get("identity").ok_or_else(|| {
                RemoteBrowserError::ExecutionFailed("Missing identity in subscribe response".into())
            })?;
            let identity: BrowserSubscribeIdentity = serde_json::from_value(ident_val.clone())
                .map_err(|e| {
                    RemoteBrowserError::ExecutionFailed(format!("Failed to parse identity: {e}"))
                })?;

            // R6-1: Spawn owned persistent framed reader that forwards frames to broadcast channel.
            // The task owns the write half too: dropping it would half-close the socket, and the
            // GUI would read EOF, unsubscribe and stop forwarding.
            let b_id = browser_id.to_string();
            let senders = Arc::clone(&self.frame_senders);
            let task = tokio::spawn(async move {
                let _writer = writer;
                loop {
                    let Ok((c_type, p_buf)) = read_ipc_frame(&mut reader).await else {
                        break;
                    };
                    if c_type == Self::IPC_CONTENT_TYPE_IMAGE {
                        let guard = senders.lock();
                        if let Some(tx) = guard.get(&b_id) {
                            let _ = tx.send(p_buf);
                        }
                    }
                }
            });

            self.active_subscriptions
                .lock()
                .await
                .insert(sub_id.clone(), task);

            Ok((sub_id, stream_id, negotiated_opts, identity))
        })
    }

    fn unsubscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        subscription_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            if let Some(task) = self
                .active_subscriptions
                .lock()
                .await
                .remove(subscription_id)
            {
                task.abort();
            }
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::UnsubscribeViewer {
                browser_id: browser_id.to_string(),
                subscription_id: subscription_id.to_string(),
            };
            if let Ok(req_bytes) = serde_json::to_vec(&req) {
                let _ = self.send_framed_request(&req_bytes).await;
            }
            Ok(())
        })
    }

    fn claim_driver<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        connection_id: &'a str,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::ClaimDriver {
                browser_id: browser_id.to_string(),
                device_id: device_id.to_string(),
                connection_id: connection_id.to_string(),
                subscription_id: subscription_id.to_string(),
                lease_epoch: Some(lease_epoch),
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize claim request: {e}"
                ))
            })?;
            let payload = self.send_framed_request(&req_bytes).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to parse claim response: {e}"))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Claim failed");
                return match err_code {
                    "BROWSER_FORBIDDEN" => Err(RemoteBrowserError::Forbidden(msg.to_string())),
                    "BROWSER_NOT_FOUND" => Err(RemoteBrowserError::NotFound(msg.to_string())),
                    "BROWSER_TARGET_NOT_FOUND" => {
                        Err(RemoteBrowserError::TargetNotFound(msg.to_string()))
                    }
                    "BROWSER_STALE_FRAME" => Err(RemoteBrowserError::StaleFrame(msg.to_string())),
                    "BROWSER_DRIVER_BUSY" => Err(RemoteBrowserError::DriverBusy(msg.to_string())),
                    "RATE_LIMITED" => Err(RemoteBrowserError::RateLimited(msg.to_string())),
                    _ => Err(RemoteBrowserError::ExecutionFailed(msg.to_string())),
                };
            }
            Ok(())
        })
    }

    fn release_driver<'a>(
        &'a self,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::ReleaseDriver {
                subscription_id: subscription_id.to_string(),
                lease_epoch: Some(lease_epoch),
            };
            if let Ok(req_bytes) = serde_json::to_vec(&req) {
                let _ = self.send_framed_request(&req_bytes).await;
            }
            Ok(())
        })
    }

    fn heartbeat_driver<'a>(
        &'a self,
        device_id: &'a str,
        connection_id: &'a str,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            let req = crate::ipc::browser_cli::RemoteBrowserOperation::HeartbeatDriver {
                device_id: device_id.to_string(),
                connection_id: connection_id.to_string(),
                subscription_id: subscription_id.to_string(),
                lease_epoch: Some(lease_epoch),
            };
            let req_bytes = serde_json::to_vec(&req).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to serialize heartbeat request: {e}"
                ))
            })?;
            let payload = self.send_framed_request(&req_bytes).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!(
                    "Failed to parse heartbeat response: {e}"
                ))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Heartbeat failed");
                return match err_code {
                    "BROWSER_FORBIDDEN" | "BROWSER_CLI_UNAUTHORIZED" => {
                        Err(RemoteBrowserError::Forbidden(msg.to_string()))
                    }
                    "BROWSER_INVALID_REQUEST" | "BROWSER_CLI_REQUEST_INVALID" => {
                        Err(RemoteBrowserError::InvalidRequest(msg.to_string()))
                    }
                    _ => Err(typed_ipc_error(err_code, msg, val.get("details"))),
                };
            }
            Ok(())
        })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities> {
        Box::pin(async move {
            // Honest probe: a platform without the local IPC transport must not
            // stat a path that can never exist, so it reports unavailable with
            // the reason its transport stubs use.
            let (available, unavailable_reason) =
                local_ipc_availability(LOCAL_IPC_TRANSPORT_SUPPORTED, &self.socket_path);
            if let Some(reason) = unavailable_reason {
                tracing::debug!("Local browser IPC backend unavailable: {reason}");
            }
            BrowserCapabilities {
                browser_available: available,
                supported_formats: vec!["jpeg".into(), "png".into()],
                supported_commands: vec![
                    "navigate".into(),
                    "back".into(),
                    "forward".into(),
                    "reload".into(),
                    "click".into(),
                    "fill".into(),
                    "keypress".into(),
                    "eval".into(),
                    "wait".into(),
                    "getState".into(),
                ],
                max_edge: 2048,
                max_fps: 8,
            }
        })
    }
}

/// In-process test provider for unit/integration testing
pub struct InProcessTestBackend {
    pub sessions: Mutex<Vec<RemoteBrowserSessionSummary>>,
    pub states: Mutex<HashMap<String, BrowserRemoteState>>,
    pub frame_senders: Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>,
    pub negotiated_options:
        Mutex<HashMap<String, crate::remote::browser_protocol::BrowserSubscribeOptions>>,
    pub identities: Mutex<HashMap<String, BrowserSubscribeIdentity>>,
}

impl Default for InProcessTestBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InProcessTestBackend {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            states: Mutex::new(HashMap::new()),
            frame_senders: Mutex::new(HashMap::new()),
            negotiated_options: Mutex::new(HashMap::new()),
            identities: Mutex::new(HashMap::new()),
        }
    }
}

impl RemoteBrowserBackend for InProcessTestBackend {
    fn list_sessions<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move { Ok(self.sessions.lock().await.clone()) })
    }

    fn identify_session<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move { Ok(self.sessions.lock().await.first().cloned()) })
    }

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            self.states
                .lock()
                .await
                .get(browser_id)
                .cloned()
                .ok_or_else(|| RemoteBrowserError::NotFound(browser_id.to_string()))
        })
    }

    fn execute_command(
        &self,
        _ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            Ok(BrowserCommandResult {
                success: true,
                value: None,
            })
        })
    }

    fn subscribe_frames<'a>(
        &'a self,
        browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        Box::pin(async move {
            let mut senders = self.frame_senders.lock().await;
            let sender = senders
                .entry(browser_id.to_string())
                .or_insert_with(|| tokio::sync::broadcast::channel(16).0);
            Ok(sender.subscribe())
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        _device_id: &'a str,
        _viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<
        'a,
        Result<
            (
                String,
                u32,
                crate::remote::browser_protocol::BrowserSubscribeOptions,
                BrowserSubscribeIdentity,
            ),
            RemoteBrowserError,
        >,
    > {
        Box::pin(async move {
            let mut opts = self.negotiated_options.lock().await;
            let negotiated = opts
                .entry(browser_id.to_string())
                .or_insert_with(|| options.unwrap_or_default())
                .clone();
            let sub_id = format!("sub-{}", uuid::Uuid::new_v4());
            let idents = self.identities.lock().await;
            let identity = if let Some(id) = idents.get(browser_id) {
                id.clone()
            } else {
                let states = self.states.lock().await;
                let (gen, inst) = states
                    .get(browser_id)
                    .map(|s| (s.document_generation.clone(), s.browser_instance_id.clone()))
                    .unwrap_or_else(|| ("1".into(), None));
                BrowserSubscribeIdentity {
                    browser_instance_id: inst.unwrap_or_else(|| "bi1".into()),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: gen,
                }
            };
            Ok((sub_id, 1, negotiated, identity))
        })
    }

    fn unsubscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        _subscription_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            self.negotiated_options.lock().await.remove(browser_id);
            Ok(())
        })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities> {
        Box::pin(async move {
            BrowserCapabilities {
                browser_available: true,
                supported_formats: vec!["jpeg".into(), "png".into()],
                supported_commands: vec![
                    "navigate".into(),
                    "click".into(),
                    "fill".into(),
                    "keypress".into(),
                    "eval".into(),
                    "wait".into(),
                    "getState".into(),
                ],
                max_edge: 2048,
                max_fps: 8,
            }
        })
    }
}

/// Production in-process backend connecting remote WebSocket and HTTP APIs directly
/// to BrowserRemoteService and BrowserManager (§1.1, §1.3, R1).
pub struct InProcessBrowserServiceBackend {
    pub remote_service: Arc<BrowserRemoteService>,
    pub manager: Arc<BrowserManager>,
    active_subscriptions: Arc<Mutex<HashMap<String, String>>>, // subscription_id -> browser_id
    executor: Arc<parking_lot::RwLock<Option<Arc<dyn BrowserCommandExecutor>>>>,
}

impl InProcessBrowserServiceBackend {
    pub fn new(remote_service: Arc<BrowserRemoteService>, manager: Arc<BrowserManager>) -> Self {
        Self {
            remote_service,
            manager,
            active_subscriptions: Arc::new(Mutex::new(HashMap::new())),
            executor: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn set_executor(&self, executor: Arc<dyn BrowserCommandExecutor>) {
        *self.executor.write() = Some(executor);
    }
}

impl RemoteBrowserBackend for InProcessBrowserServiceBackend {
    fn list_sessions<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            let sessions = self.manager.list_sessions();
            let filtered = sessions
                .into_iter()
                .filter(|s| {
                    let ws_match = if scope.workspace_id.is_empty() {
                        true
                    } else {
                        s.workspace_id.as_deref() == Some(&scope.workspace_id)
                    };
                    let wt_path = self
                        .manager
                        .get_state(&s.browser_id)
                        .ok()
                        .and_then(|st| st.worktree_path);
                    let wt_match = matches_worktree_slug(wt_path.as_deref(), &scope.worktree_slug);
                    ws_match && wt_match
                })
                .map(|s| RemoteBrowserSessionSummary {
                    browser_id: s.browser_id,
                    title: s.title,
                    url: Some(s.url),
                    visible: s.visible,
                })
                .collect();
            Ok(filtered)
        })
    }

    fn identify_session<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            let list = self.list_sessions(scope).await?;
            Ok(list
                .iter()
                .find(|s| s.visible)
                .cloned()
                .or_else(|| list.first().cloned()))
        })
    }

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            let state = self.manager.get_state(browser_id).map_err(|e| match e {
                crate::browser::security::BrowserError::NotFound(id) => {
                    RemoteBrowserError::NotFound(id)
                }
                other => RemoteBrowserError::ExecutionFailed(other.to_string()),
            })?;
            let inst_id = self.manager.get_instance_id(browser_id).ok();
            let (_, _, vp_rev) = self.manager.get_geometry(browser_id).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("viewport revision unavailable: {e}"))
            })?;
            Ok(BrowserRemoteState {
                browser_id: state.browser_id,
                browser_instance_id: inst_id,
                url: Some(state.url),
                title: state.title,
                document_generation: state.generation.to_string(),
                viewport_revision: vp_rev.to_string(),
                loading: state.loading,
                paused: !state.visible,
                pause_reason: if !state.visible {
                    Some("hidden".into())
                } else {
                    None
                },
            })
        })
    }

    fn execute_command(
        &self,
        ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            let executor = self.executor.read().clone();

            match ctx.command.as_str() {
                "snapshot" | "getState" | "navigate" | "back" | "forward" | "reload" | "click"
                | "fill" | "keypress" | "eval" | "wait" => {}
                _ => {
                    return Err(RemoteBrowserError::InvalidRequest(format!(
                        "Unsupported command: '{}'",
                        ctx.command
                    )))
                }
            }

            let is_mutation = ctx.command != "getState" && ctx.command != "snapshot";
            if is_mutation {
                // Fencing and standalone guard validation (remote_service.rs:686)
                if let (
                    Some(lease_str),
                    Some(dev_id),
                    Some(conn_id),
                    Some(inst),
                    Some(dt_ep),
                    Some(doc_gen),
                ) = (
                    &ctx.lease_epoch,
                    &ctx.device_id,
                    &ctx.connection_id,
                    &ctx.browser_instance_id,
                    &ctx.desktop_epoch,
                    &ctx.document_generation,
                ) {
                    let lease_epoch = lease_str.parse::<u64>().unwrap_or(0);
                    let desktop_epoch = dt_ep.parse::<u64>().unwrap_or(0);
                    let generation = doc_gen.parse::<u64>().unwrap_or(0);
                    self.remote_service.execute_command_guard(crate::browser::remote_service::CommandGuardParams { browser_id: &ctx.browser_id, lease_epoch, device_id: dev_id, connection_id: conn_id, instance_id: inst, desktop_epoch, generation }).map_err(|e| match e {
                        crate::browser::remote_service::RemoteServiceError::BrowserNotFound(id) => RemoteBrowserError::NotFound(id),
                        crate::browser::remote_service::RemoteServiceError::StaleLease
                        | crate::browser::remote_service::RemoteServiceError::DesktopReclaimed => RemoteBrowserError::Forbidden(e.to_string()),
                        crate::browser::remote_service::RemoteServiceError::StaleInstance
                        | crate::browser::remote_service::RemoteServiceError::StaleGeneration => RemoteBrowserError::InvalidRequest(e.to_string()),
                        other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                    })?;
                } else {
                    // A mutation must carry the complete fencing tuple. A partial or empty one
                    // is refused, so no caller can skip the lease/instance/epoch/generation guard.
                    return Err(RemoteBrowserError::Forbidden(format!(
                        "Complete driver fencing tuple required for '{}'",
                        ctx.command
                    )));
                }
            } else {
                if let Some(inst) = &ctx.browser_instance_id {
                    let actual_inst =
                        self.manager
                            .get_instance_id(&ctx.browser_id)
                            .map_err(|e| match e {
                                crate::browser::security::BrowserError::NotFound(id) => {
                                    RemoteBrowserError::NotFound(id)
                                }
                                other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                            })?;
                    if inst != &actual_inst {
                        return Err(RemoteBrowserError::InvalidRequest(format!(
                            "Stale instance ID: expected {actual_inst}, got {inst}"
                        )));
                    }
                }
                if let Some(dt_ep) = &ctx.desktop_epoch {
                    let actual_epoch = self.remote_service.desktop_epoch();
                    if dt_ep.parse::<u64>().unwrap_or(0) != actual_epoch {
                        return Err(RemoteBrowserError::Forbidden(format!(
                            "Stale desktop epoch: expected {actual_epoch}, got {dt_ep}"
                        )));
                    }
                }
                if let Some(doc_gen) = &ctx.document_generation {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|e| match e {
                            crate::browser::security::BrowserError::NotFound(id) => {
                                RemoteBrowserError::NotFound(id)
                            }
                            other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                        })?;
                    if doc_gen != &state.generation.to_string() {
                        return Err(RemoteBrowserError::InvalidRequest(format!(
                            "Stale document generation: expected {}, got {doc_gen}",
                            state.generation
                        )));
                    }
                }
            }

            match ctx.command.as_str() {
                "getState" => {
                    let state = self
                        .manager
                        .get_state(&ctx.browser_id)
                        .map_err(|_e| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                    let (_, _, vp_rev) =
                        self.manager
                            .get_geometry(&ctx.browser_id)
                            .map_err(|e| {
                                RemoteBrowserError::ExecutionFailed(format!(
                                    "viewport revision unavailable: {e}"
                                ))
                            })?;
                    let val = serde_json::json!({
                        "browserId": state.browser_id,
                        "url": state.url,
                        "title": state.title,
                        "generation": state.generation.to_string(),
                        "viewportRevision": vp_rev.to_string(),
                        "loading": state.loading,
                        "visible": state.visible,
                    });
                    Ok(BrowserCommandResult {
                        success: true,
                        value: Some(val),
                    })
                }
                "navigate" => {
                    let url = ctx
                        .params
                        .as_ref()
                        .and_then(|p| p.get("url").and_then(|v| v.as_str()))
                        .ok_or_else(|| {
                            RemoteBrowserError::InvalidRequest("missing url param".into())
                        })?;
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        self.manager
                            .update_url_with_context(
                                &ctx.browser_id,
                                url,
                                crate::browser::security::BrowserSecurityContext::Remote,
                            )
                            .map_err(|e| match e {
                                crate::browser::security::BrowserError::SchemeDenied(_) => {
                                    RemoteBrowserError::Forbidden(e.to_string())
                                }
                                crate::browser::security::BrowserError::InvalidUrl(_) => {
                                    RemoteBrowserError::InvalidRequest(e.to_string())
                                }
                                other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                            })?;
                        Ok(BrowserCommandResult {
                            success: true,
                            value: Some(serde_json::json!({ "url": url })),
                        })
                    }
                }
                "back" => {
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        self.manager
                            .begin_history_navigation(&ctx.browser_id, false)
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                        Ok(BrowserCommandResult {
                            success: true,
                            value: None,
                        })
                    }
                }
                "forward" => {
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        self.manager
                            .begin_history_navigation(&ctx.browser_id, true)
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                        Ok(BrowserCommandResult {
                            success: true,
                            value: None,
                        })
                    }
                }
                "reload" => {
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        self.manager
                            .begin_reload(&ctx.browser_id)
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                        Ok(BrowserCommandResult {
                            success: true,
                            value: None,
                        })
                    }
                }
                "snapshot" => {
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        let state = self
                            .manager
                            .get_state(&ctx.browser_id)
                            .map_err(|_e| RemoteBrowserError::NotFound(ctx.browser_id.clone()))?;
                        let (snap_id, map_rev) = self
                            .remote_service
                            .record_snapshot(&ctx.browser_id, state.generation, Vec::new())
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
                        Ok(BrowserCommandResult {
                            success: true,
                            value: Some(serde_json::json!({
                                "snapshotId": snap_id,
                                "mapRevision": map_rev,
                                "mapRevisionString": map_rev.to_string(),
                                "documentGeneration": state.generation.to_string(),
                            })),
                        })
                    }
                }
                "click" | "fill" | "keypress" => {
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        Err(RemoteBrowserError::ExecutionFailed(format!(
                            "GUI executor not available for command '{}'",
                            ctx.command
                        )))
                    }
                }
                "eval" => {
                    let active_lease = self.remote_service.driver_broker().current_lease();
                    let lease = active_lease.ok_or_else(|| {
                        RemoteBrowserError::Forbidden("eval requires an active driver lease".into())
                    })?;
                    if let Some(params) = &ctx.params {
                        if let Some(epoch) = params.get("leaseEpoch").and_then(|v| v.as_u64()) {
                            if epoch != lease.lease_epoch {
                                return Err(RemoteBrowserError::Forbidden(format!(
                                    "Stale lease epoch: expected {}, actual {}",
                                    lease.lease_epoch, epoch
                                )));
                            }
                        }
                    }
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        Err(RemoteBrowserError::ExecutionFailed(
                            "GUI executor not available for eval".into(),
                        ))
                    }
                }
                "wait" => {
                    let (condition, _timeout) = match &ctx.params {
                        Some(p) => normalize_wait_params(p)?,
                        None => {
                            return Err(RemoteBrowserError::InvalidRequest(
                                "wait command requires params".into(),
                            ))
                        }
                    };
                    if matches!(
                        condition,
                        crate::browser::model::BrowserWaitCondition::Function { .. }
                    ) {
                        let active_lease = self.remote_service.driver_broker().current_lease();
                        let lease = active_lease.ok_or_else(|| {
                            RemoteBrowserError::Forbidden(
                                "function wait requires an active driver lease".into(),
                            )
                        })?;
                        if let Some(params) = &ctx.params {
                            if let Some(epoch) = params.get("leaseEpoch").and_then(|v| v.as_u64()) {
                                if epoch != lease.lease_epoch {
                                    return Err(RemoteBrowserError::Forbidden(format!(
                                        "Stale lease epoch: expected {}, actual {}",
                                        lease.lease_epoch, epoch
                                    )));
                                }
                            }
                        }
                    }
                    if let Some(exec) = executor {
                        exec.execute(ctx).await
                    } else {
                        Err(RemoteBrowserError::ExecutionFailed(
                            "GUI executor not available for wait".into(),
                        ))
                    }
                }
                _ => Err(RemoteBrowserError::InvalidRequest(format!(
                    "Unsupported command: '{}'",
                    ctx.command
                ))),
            }
        })
    }

    fn subscribe_frames<'a>(
        &'a self,
        browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        Box::pin(async move {
            // Receiver only: capture is owned by `subscribe_viewer`. Subscribing here under a
            // random device ID took a real viewer slot that nothing ever released.
            Ok(self.remote_service.subscribe_frames(browser_id))
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<
        'a,
        Result<
            (
                String,
                u32,
                crate::remote::browser_protocol::BrowserSubscribeOptions,
                BrowserSubscribeIdentity,
            ),
            RemoteBrowserError,
        >,
    > {
        Box::pin(async move {
            let instance_id = self
                .manager
                .get_instance_id(browser_id)
                .map_err(|e| match e {
                    crate::browser::security::BrowserError::NotFound(id) => {
                        RemoteBrowserError::NotFound(id)
                    }
                    other => RemoteBrowserError::ExecutionFailed(other.to_string()),
                })?;
            let state = self.manager.get_state(browser_id).map_err(|e| match e {
                crate::browser::security::BrowserError::NotFound(id) => {
                    RemoteBrowserError::NotFound(id)
                }
                other => RemoteBrowserError::ExecutionFailed(other.to_string()),
            })?;

            let requested_profile = options.as_ref().map(|opt| {
                let format = match opt.format {
                    crate::remote::browser_protocol::BrowserImageFormat::Png => {
                        crate::browser::snapshot_source::SnapshotFormat::Png
                    }
                    crate::remote::browser_protocol::BrowserImageFormat::Jpeg => {
                        crate::browser::snapshot_source::SnapshotFormat::Jpeg {
                            quality: opt.quality.unwrap_or(70).clamp(1, 100),
                        }
                    }
                };
                crate::browser::remote_service::NegotiatedCaptureProfile {
                    format,
                    quality: opt.quality.unwrap_or(70).clamp(1, 100),
                    interval_ms: opt.interval_ms.unwrap_or(80).clamp(50, 5000),
                    max_edge: opt.max_edge.unwrap_or(2048).clamp(64, 2048),
                }
            });
            let (sub_id, negotiated_prof) = self
                .remote_service
                .subscribe_with_profile(
                    browser_id,
                    device_id,
                    viewer_instance_id,
                    requested_profile,
                )
                .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
            let stream_id = self
                .remote_service
                .active_stream_id(browser_id)
                .unwrap_or(1);
            self.active_subscriptions
                .lock()
                .await
                .insert(sub_id.clone(), browser_id.to_string());

            let negotiated_options = crate::remote::browser_protocol::BrowserSubscribeOptions {
                format: match negotiated_prof.format {
                    crate::browser::snapshot_source::SnapshotFormat::Png => {
                        crate::remote::browser_protocol::BrowserImageFormat::Png
                    }
                    crate::browser::snapshot_source::SnapshotFormat::Jpeg { .. } => {
                        crate::remote::browser_protocol::BrowserImageFormat::Jpeg
                    }
                },
                quality: match negotiated_prof.format {
                    crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality } => {
                        Some(quality)
                    }
                    _ => None,
                },
                interval_ms: Some(negotiated_prof.interval_ms),
                max_edge: Some(negotiated_prof.max_edge),
            };

            let identity = BrowserSubscribeIdentity {
                browser_instance_id: instance_id,
                browser_service_epoch: self.remote_service.service_epoch().to_string(),
                desktop_epoch: self.remote_service.desktop_epoch().to_string(),
                document_generation: state.generation.to_string(),
            };

            Ok((sub_id, stream_id, negotiated_options, identity))
        })
    }

    fn claim_driver<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        connection_id: &'a str,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        if self.manager.get_state(browser_id).is_err() {
            return Box::pin(
                async move { Err(RemoteBrowserError::NotFound(browser_id.to_string())) },
            );
        }
        let res = self.remote_service.driver_broker().claim_with_epoch(
            device_id,
            connection_id,
            subscription_id,
            browser_id,
            true,
            lease_epoch,
        );
        Box::pin(async move {
            match res {
                Ok(_) => Ok(()),
                Err(e) => Err(map_driver_error(e)),
            }
        })
    }

    fn release_driver<'a>(
        &'a self,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        let _ = self
            .remote_service
            .driver_broker()
            .release(subscription_id, lease_epoch);
        Box::pin(async move { Ok(()) })
    }

    fn heartbeat_driver<'a>(
        &'a self,
        device_id: &'a str,
        connection_id: &'a str,
        subscription_id: &'a str,
        lease_epoch: u64,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        let res = self.remote_service.driver_broker().heartbeat(
            device_id,
            connection_id,
            subscription_id,
            lease_epoch,
        );
        Box::pin(async move {
            match res {
                Ok(_) => Ok(()),
                Err(e) => Err(map_driver_error(e)),
            }
        })
    }

    fn unsubscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        subscription_id: &'a str,
    ) -> BoxFuture<'a, Result<(), RemoteBrowserError>> {
        Box::pin(async move {
            self.remote_service.unsubscribe(browser_id, subscription_id);
            self.active_subscriptions
                .lock()
                .await
                .remove(subscription_id);
            Ok(())
        })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities> {
        Box::pin(async move {
            let source = self.remote_service.snapshot_source();
            let is_supported = source.is_supported();
            let supported_formats = source.supported_formats();
            let mut commands = vec![
                "navigate".into(),
                "back".into(),
                "forward".into(),
                "reload".into(),
                "click".into(),
                "fill".into(),
                "keypress".into(),
                "eval".into(),
                "wait".into(),
                "getState".into(),
            ];
            if is_supported {
                commands.push("snapshot".into());
            }
            BrowserCapabilities {
                browser_available: true,
                supported_formats,
                supported_commands: commands,
                max_edge: if is_supported { 2048 } else { 0 },
                max_fps: if is_supported { 15 } else { 0 },
            }
        })
    }
}

/// Reads the GUI's local IPC token; a missing or unreadable file yields an empty token.
#[cfg(unix)]
async fn read_local_ipc_token(token_path: &std::path::Path) -> String {
    tokio::fs::read_to_string(token_path)
        .await
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[tokio::test]
    async fn in_process_navigate_fallback_rejects_local_file_url() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), Arc::clone(&broker)));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-fallback-nav".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let lease = service
            .driver_broker()
            .claim("dev1", "conn1", "sub1", &state.browser_id, true)
            .unwrap();
        let fenced = |url: &str| BrowserCommandContext {
            browser_id: state.browser_id.clone(),
            command: "navigate".into(),
            params: Some(serde_json::json!({ "url": url })),
            document_generation: Some(state.generation.to_string()),
            browser_instance_id: Some(manager.get_instance_id(&state.browser_id).unwrap()),
            desktop_epoch: Some(service.desktop_epoch().to_string()),
            lease_epoch: Some(lease.lease_epoch.to_string()),
            device_id: Some("dev1".into()),
            connection_id: Some("conn1".into()),
        };
        let res = backend.execute_command(fenced("file:///tmp/x.html")).await;
        assert!(
            matches!(res, Err(RemoteBrowserError::Forbidden(ref m)) if m.contains("file:///tmp/x.html")),
            "fallback remote navigate to a local file must be denied, got {res:?}"
        );
        assert_eq!(manager.get_state(&state.browser_id).unwrap().url, state.url);
        let malformed = backend
            .execute_command(fenced("https://user:pw@example.com/"))
            .await;
        assert!(
            matches!(malformed, Err(RemoteBrowserError::InvalidRequest(_))),
            "fallback remote navigate to a malformed URL must be InvalidRequest, got {malformed:?}"
        );
    }

    /// B6: a mutation whose fencing tuple is partial or empty must be refused even when a
    /// driver lease exists, instead of skipping the guard.
    #[tokio::test]
    async fn in_process_mutation_requires_complete_fencing_tuple() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), Arc::clone(&broker)));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-fence".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let lease = broker
            .claim("dev1", "conn1", "sub1", &state.browser_id, true)
            .unwrap();
        let navigate = |lease_epoch: Option<String>, device_id: Option<String>| {
            BrowserCommandContext {
                browser_id: state.browser_id.clone(),
                command: "navigate".into(),
                params: Some(serde_json::json!({ "url": "https://example.org/" })),
                lease_epoch,
                device_id,
                ..Default::default()
            }
        };
        for ctx in [
            navigate(None, None),
            navigate(Some(lease.lease_epoch.to_string()), Some("dev1".into())),
        ] {
            let res = backend.execute_command(ctx).await;
            assert!(
                matches!(res, Err(RemoteBrowserError::Forbidden(_))),
                "an incomplete fencing tuple must be Forbidden, got {res:?}"
            );
        }
        assert_eq!(manager.get_state(&state.browser_id).unwrap().url, state.url);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_token_missing_file_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_local_ipc_token(&dir.path().join("absent.token")).await, "");
        let present = dir.path().join("browser.sock.token");
        std::fs::write(&present, "  tok-123\n").unwrap();
        assert_eq!(read_local_ipc_token(&present).await, "tok-123");
    }

    /// A token read that blocks (a FIFO with no writer yet) must not stall the async
    /// runtime: the writer task on the same single-threaded runtime still has to run.
    #[cfg(unix)]
    #[test]
    fn local_ipc_token_read_does_not_block_the_runtime_thread() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("browser.sock.token");
        let c_path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

        let (tx, rx) = std::sync::mpsc::channel();
        let reader_path = fifo.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let token = rt.block_on(async move {
                let writer = async {
                    tokio::task::yield_now().await;
                    let mut f = tokio::fs::OpenOptions::new()
                        .write(true)
                        .open(&fifo)
                        .await
                        .unwrap();
                    tokio::io::AsyncWriteExt::write_all(&mut f, b"fifo-token\n")
                        .await
                        .unwrap();
                };
                let (token, ()) = tokio::join!(read_local_ipc_token(&reader_path), writer);
                token
            });
            let _ = tx.send(token);
        });
        let token = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("token read blocked the runtime thread");
        assert_eq!(token, "fifo-token");
    }

    #[tokio::test]
    async fn test_unavailable_backend_returns_typed_error() {
        let backend = UnavailableBrowserBackend;
        let scope = DesktopScope {
            workspace_id: "w1".into(),
            worktree_slug: "wt1".into(),
        };

        let res = backend.list_sessions(&scope).await;
        assert!(matches!(res, Err(RemoteBrowserError::Unavailable(_))));

        let caps = backend.capabilities().await;
        assert!(!caps.browser_available);
    }

    #[tokio::test]
    async fn test_capability_probe_reports_unsupported_transport_with_reason() {
        let temp_dir = tempfile::tempdir().unwrap();
        // An existing path is exactly the Windows case: the GUI writes
        // `runtime/browser.port`, yet this backend has no transport for it.
        let existing_path = temp_dir.path().join("browser.port");
        std::fs::write(&existing_path, "12345").unwrap();
        let existing = existing_path.to_string_lossy().to_string();
        let absent = temp_dir
            .path()
            .join("absent.sock")
            .to_string_lossy()
            .to_string();

        // Unsupported transport: unavailable with a reason, never a path probe.
        let (available, reason) = local_ipc_availability(false, &existing);
        assert!(!available, "unsupported transport must report unavailable");
        assert_eq!(reason, Some(LOCAL_IPC_TRANSPORT_UNSUPPORTED_REASON));

        // Supported transport: the path probe still decides, with a reason.
        let (available, reason) = local_ipc_availability(true, &existing);
        assert!(available);
        assert_eq!(reason, None);
        let (available, reason) = local_ipc_availability(true, &absent);
        assert!(!available);
        assert_eq!(reason, Some("GUI process not running or socket unconnected"));
        let (available, reason) = local_ipc_availability(true, "");
        assert!(!available);
        assert_eq!(reason, Some("Local IPC socket path empty"));

        // The live capability probe follows the platform flag: an existing path
        // only means available where the transport exists at all.
        let backend = LocalIpcBrowserBackend::new(existing, None);
        let caps = backend.capabilities().await;
        assert_eq!(caps.browser_available, LOCAL_IPC_TRANSPORT_SUPPORTED);
    }

    #[tokio::test]
    async fn test_local_ipc_backend_framed_codec() {
        let payload = b"{\"command\":\"ping\"}";
        let frame = LocalIpcBrowserBackend::encode_ipc_frame(
            LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON,
            payload,
        )
        .unwrap();

        assert_eq!(frame.len(), 5 + payload.len());
        let decoded = LocalIpcBrowserBackend::decode_ipc_frame(&frame)
            .unwrap()
            .unwrap();
        assert_eq!(decoded.0, LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
        assert_eq!(&decoded.1, payload);
        assert_eq!(decoded.2, frame.len());
    }

    #[tokio::test]
    async fn test_in_process_test_backend_lifecycle() {
        let backend = InProcessTestBackend::new();
        let scope = DesktopScope {
            workspace_id: "w1".into(),
            worktree_slug: "wt1".into(),
        };

        let summary = RemoteBrowserSessionSummary {
            browser_id: "b1".into(),
            title: Some("Title".into()),
            url: Some("https://example.com".into()),
            visible: true,
        };
        backend.sessions.lock().await.push(summary);

        let listed = backend.list_sessions(&scope).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].browser_id, "b1");

        let identified = backend.identify_session(&scope).await.unwrap();
        assert!(identified.is_some());
        assert_eq!(identified.unwrap().browser_id, "b1");
    }

    #[tokio::test]
    async fn test_subscribe_frames_on_all_backends() {
        // 1. UnavailableBrowserBackend returns typed Unavailable
        let unavailable = UnavailableBrowserBackend;
        let sub_unavail = unavailable.subscribe_frames("b1").await;
        assert!(matches!(
            sub_unavail,
            Err(RemoteBrowserError::Unavailable(_))
        ));

        // 2. LocalIpcBrowserBackend with empty socket path returns typed Unavailable
        let local_empty = LocalIpcBrowserBackend::new("".into(), None);
        let sub_empty = local_empty.subscribe_frames("b1").await;
        assert!(
            matches!(sub_empty, Err(RemoteBrowserError::Unavailable(msg)) if msg.contains("empty"))
        );

        // LocalIpcBrowserBackend with non-empty path but disconnected socket
        let local_ipc = LocalIpcBrowserBackend::new("/tmp/test.sock".into(), None);
        let sub_local = local_ipc.subscribe_frames("b1").await;
        assert!(
            matches!(sub_local, Err(RemoteBrowserError::Unavailable(msg)) if msg.contains("not running"))
        );

        // 3. InProcessTestBackend returns live broadcast receiver
        let test_backend = InProcessTestBackend::new();
        let mut rx = test_backend.subscribe_frames("b1").await.unwrap();

        // Broadcast a test frame
        let sender = test_backend
            .frame_senders
            .lock()
            .await
            .get("b1")
            .unwrap()
            .clone();
        sender.send(vec![0xAA, 0xBB, 0xCC]).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received, vec![0xAA, 0xBB, 0xCC]);
    }

    #[tokio::test]
    async fn test_in_process_browser_service_backend_lifecycle_and_subscription() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));

        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));

        let scope = DesktopScope {
            workspace_id: "ws-1".into(),
            worktree_slug: "wt-1".into(),
        };

        // Register session in manager
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-prod-1".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: Some("wt-1".into()),
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: Some(crate::browser::model::LogicalRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1024.0,
                    height: 768.0,
                }),
                visible: Some(true),
            })
            .unwrap();

        // 1. List and identify
        let sessions = backend.list_sessions(&scope).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].browser_id, "b-prod-1");

        let identified = backend.identify_session(&scope).await.unwrap();
        assert!(identified.is_some());
        assert_eq!(identified.unwrap().browser_id, "b-prod-1");

        // 2. Get state
        let state = backend.get_state("b-prod-1", &scope).await.unwrap();
        assert_eq!(state.browser_id, "b-prod-1");
        assert_eq!(state.document_generation, "1");

        // 3. Execute command
        let cmd_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b-prod-1".into(),
                command: "getState".into(),
                params: None,
                document_generation: Some("1".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(cmd_res.success);

        // 4. Wire subscription ownership: subscribe_viewer activates producer and reconciles stream_id
        assert!(!service.is_producer_active("b-prod-1"));
        let (sub_id, stream_id, _, identity) = backend
            .subscribe_viewer("b-prod-1", "dev-backend-1", "viewer-1", None)
            .await
            .unwrap();
        assert!(service.is_producer_active("b-prod-1"));
        assert_eq!(stream_id, service.active_stream_id("b-prod-1").unwrap());
        assert_eq!(
            identity.browser_instance_id,
            manager.get_instance_id("b-prod-1").unwrap()
        );
        assert_eq!(
            identity.browser_service_epoch,
            service.service_epoch().to_string()
        );
        assert_eq!(identity.desktop_epoch, service.desktop_epoch().to_string());
        assert_eq!(identity.document_generation, "1");

        // 5. Wire teardown: unsubscribe_viewer calls unsubscribe and stops producer
        backend
            .unsubscribe_viewer("b-prod-1", &sub_id)
            .await
            .unwrap();
        assert!(!service.is_producer_active("b-prod-1"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r4_1_local_ipc_backend_connects_and_dispatches() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("test_browser.sock");
        let token = "auth-token-1234";

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
                let (reader, mut writer) = stream.split();
                let mut reader = BufReader::new(reader);
                let mut line = String::new();
                let _ = reader.read_line(&mut line).await;
                // Handshake response
                let resp = r#"{"type":"remoteAttached","protocolVersion":1}"#;
                let _ = writer.write_all(format!("{resp}\n").as_bytes()).await;
                let _ = writer.flush().await;

                // Read framed request
                let mut header = [0u8; 5];
                if reader.read_exact(&mut header).await.is_ok() {
                    let len =
                        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
                    let mut payload = vec![0u8; len];
                    let _ = reader.read_exact(&mut payload).await;

                    // Send response frame
                    let session_json = serde_json::json!([{
                        "browserId": "b-bridge-1",
                        "title": "Bridge Browser",
                        "url": "https://example.test",
                        "visible": true
                    }]);
                    let resp_payload = serde_json::to_vec(&session_json).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some(token.to_string()),
        );
        let scope = DesktopScope {
            workspace_id: "".into(),
            worktree_slug: "".into(),
        };

        // RED assertion: backend must connect and list sessions over framed IPC
        let sessions = backend
            .list_sessions(&scope)
            .await
            .expect("must list sessions over bridge IPC");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].browser_id, "b-bridge-1");
    }

    #[tokio::test]
    async fn test_r4_2_execute_command_rejects_unsupported_and_requires_executor() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));

        // 1. Unknown / unsupported command must be rejected with InvalidRequest, NOT succeed
        let unsupp_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "unsupportedCmd".into(),
                params: None,
                document_generation: None,
                ..Default::default()
            })
            .await;
        assert!(
            matches!(unsupp_res, Err(RemoteBrowserError::InvalidRequest(_))),
            "unsupported command must return InvalidRequest, got: {:?}",
            unsupp_res
        );

        // 2. Click without GUI executor must NOT silently report success
        let click_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "click".into(),
                params: Some(serde_json::json!({ "u": 0.5, "v": 0.5 })),
                document_generation: None,
                ..Default::default()
            })
            .await;
        assert!(
            click_res.is_err(),
            "click without executor must fail, got: {:?}",
            click_res
        );

        // 3. Fill without GUI executor must NOT silently report success
        let fill_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "fill".into(),
                params: Some(serde_json::json!({ "value": "text" })),
                document_generation: None,
                ..Default::default()
            })
            .await;
        assert!(
            fill_res.is_err(),
            "fill without executor must fail, got: {:?}",
            fill_res
        );
    }

    #[tokio::test]
    async fn test_r4_16_eval_wait_contract_normalization_and_lease_bound_approval() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));
        let state = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b1".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let instance_id = manager.get_instance_id("b1").unwrap();
        // Full fencing tuple for the lease epoch under test.
        let fence = |lease_epoch: u64| BrowserCommandContext {
            document_generation: Some(state.generation.to_string()),
            browser_instance_id: Some(instance_id.clone()),
            desktop_epoch: Some(service.desktop_epoch().to_string()),
            lease_epoch: Some(lease_epoch.to_string()),
            device_id: Some("dev1".into()),
            connection_id: Some("conn1".into()),
            ..Default::default()
        };

        // 1. Eval without active driver lease must be rejected as Forbidden
        let unapproved_eval = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "eval".into(),
                params: Some(serde_json::json!({
                    "script": "1 + 1"
                })),
                document_generation: None,
                ..Default::default()
            })
            .await;
        assert!(
            matches!(unapproved_eval, Err(RemoteBrowserError::Forbidden(_))),
            "eval without authoritative driver lease must return Forbidden, got: {:?}",
            unapproved_eval
        );

        // 2. Wait with string condition must be parsed/normalized
        let (cond, timeout_opt) = normalize_wait_params(&serde_json::json!({
            "condition": "document.title !== ''",
            "timeoutMs": 2500
        }))
        .expect("string condition must be normalized");
        assert_eq!(
            cond,
            crate::browser::model::BrowserWaitCondition::Function {
                script: "document.title !== ''".into()
            }
        );
        assert_eq!(timeout_opt, Some(std::time::Duration::from_millis(2500)));

        // 3. Claim active driver lease on broker -> eval with matching leaseEpoch succeeds (dispatches to executor)
        let lease = broker.claim("dev1", "conn1", "sub1", "b1", true).unwrap();
        struct MockExecutor;
        impl BrowserCommandExecutor for MockExecutor {
            fn execute<'a>(
                &'a self,
                ctx: BrowserCommandContext,
            ) -> BoxFuture<'a, Result<BrowserCommandResult, RemoteBrowserError>> {
                Box::pin(async move {
                    if ctx.command == "eval" {
                        Ok(BrowserCommandResult {
                            success: true,
                            value: Some(serde_json::json!({ "result": "42" })),
                        })
                    } else if ctx.command == "click" {
                        // Simulate JS failure result propagation
                        Err(RemoteBrowserError::ExecutionFailed(
                            "click failed: element not found".into(),
                        ))
                    } else {
                        Ok(BrowserCommandResult {
                            success: true,
                            value: None,
                        })
                    }
                })
            }
        }
        backend.set_executor(Arc::new(MockExecutor));

        // Matching lease epoch -> succeeds
        let eval_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "eval".into(),
                params: Some(serde_json::json!({
                    "script": "1 + 1",
                    "leaseEpoch": lease.lease_epoch
                })),
                ..fence(lease.lease_epoch)
            })
            .await
            .expect("eval with valid driver lease must succeed");
        assert!(eval_res.success);
        assert_eq!(eval_res.value.unwrap()["result"], "42");

        // Mismatched lease epoch -> rejected as Forbidden
        let stale_eval = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "eval".into(),
                params: Some(serde_json::json!({
                    "script": "1 + 1",
                    "leaseEpoch": lease.lease_epoch + 99
                })),
                ..fence(lease.lease_epoch + 99)
            })
            .await;
        assert!(
            matches!(stale_eval, Err(RemoteBrowserError::Forbidden(_))),
            "eval with stale lease epoch must return Forbidden, got: {:?}",
            stale_eval
        );

        // Function wait with mismatched lease epoch -> rejected as Forbidden
        let stale_wait = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "wait".into(),
                params: Some(serde_json::json!({
                    "condition": "document.readyState === 'complete'",
                    "leaseEpoch": lease.lease_epoch + 99
                })),
                ..fence(lease.lease_epoch + 99)
            })
            .await;
        assert!(
            matches!(stale_wait, Err(RemoteBrowserError::Forbidden(_))),
            "function wait with stale lease epoch must return Forbidden, got: {:?}",
            stale_wait
        );

        // 4. GUI executor propagates structured failure for click
        let click_err = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "click".into(),
                params: Some(serde_json::json!({ "selector": "#nonexistent" })),
                ..fence(lease.lease_epoch)
            })
            .await;
        assert!(
            matches!(click_err, Err(RemoteBrowserError::ExecutionFailed(ref msg)) if msg.contains("element not found")),
            "executor JS error must be propagated as ExecutionFailed, got: {:?}",
            click_err
        );
    }

    #[tokio::test]
    async fn test_r4_1_state_defaults_to_local_ipc_and_snapshot_source_injection() {
        // 1. RemoteGatewayState initializes with LocalIpcBrowserBackend
        let terminal = Arc::new(crate::terminal::TerminalService::default());
        let registry = crate::worktree::WorkspaceRegistry::new();
        let state = crate::remote::state::RemoteGatewayState::new(terminal, registry);
        let caps = state.browser_backend().capabilities().await;
        assert!(caps.supported_commands.contains(&"navigate".to_string()));
        assert!(caps.supported_commands.contains(&"click".to_string()));

        // 2. BrowserRemoteService accepts injected snapshot source
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));

        let fake_source = Arc::new(
            crate::browser::snapshot_source::FakeBrowserSnapshotSource::new(
                crate::browser::snapshot_source::FakeSnapshotBehavior::Auto {
                    width: 800,
                    height: 600,
                },
            ),
        );
        service.set_snapshot_source(fake_source);

        // 3. InProcessBrowserServiceBackend routes with the injected source
        let backend = Arc::new(InProcessBrowserServiceBackend::new(
            Arc::clone(&service),
            Arc::clone(&manager),
        ));
        state.set_browser_backend(Arc::clone(&backend) as Arc<dyn RemoteBrowserBackend>);
        let updated_caps = state.browser_backend().capabilities().await;
        assert!(updated_caps.browser_available);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r5_1_list_sessions_propagates_error_envelope() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("r5_1_test.sock");
        let token_path = temp_dir.path().join("r5_1_test.token");
        std::fs::write(&token_path, "secret-token").unwrap();

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = tokio::io::BufReader::new(reader);

                // Read handshake line
                let mut line = String::new();
                use tokio::io::AsyncBufReadExt;
                let _ = reader.read_line(&mut line).await;
                let _ = writer.write_all(b"{\"type\":\"remoteAttached\"}\n").await;
                let _ = writer.flush().await;

                // Read framed request
                let mut hdr = [0u8; 5];
                if reader.read_exact(&mut hdr).await.is_ok() {
                    let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
                    let mut payload = vec![0u8; len];
                    let _ = reader.read_exact(&mut payload).await;

                    // Respond with an error envelope
                    let err_json = serde_json::json!({
                        "code": "BROWSER_INVALID_REQUEST",
                        "message": "Bridge daemon simulated failure"
                    });
                    let resp_payload = serde_json::to_vec(&err_json).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );
        let scope = DesktopScope {
            workspace_id: "".into(),
            worktree_slug: "".into(),
        };

        // R5-1: Error envelope from bridge must propagate as Err, NEVER become Ok([]) empty inventory
        let res = backend.list_sessions(&scope).await;
        assert!(
            res.is_err(),
            "Bridge error envelope must propagate as Err, but got: {:?}",
            res
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r5_2_params_cannot_overwrite_authorized_target() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("r5_2_test.sock");
        let token_path = temp_dir.path().join("r5_2_test.token");
        std::fs::write(&token_path, "secret-token").unwrap();

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();

        let received_req = Arc::new(parking_lot::Mutex::new(None));
        let received_clone = Arc::clone(&received_req);

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = tokio::io::BufReader::new(reader);

                // Handshake
                let mut line = String::new();
                use tokio::io::AsyncBufReadExt;
                let _ = reader.read_line(&mut line).await;
                let _ = writer.write_all(b"{\"type\":\"remoteAttached\"}\n").await;
                let _ = writer.flush().await;

                // Read framed request
                let mut hdr = [0u8; 5];
                if reader.read_exact(&mut hdr).await.is_ok() {
                    let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
                    let mut payload = vec![0u8; len];
                    let _ = reader.read_exact(&mut payload).await;
                    let val: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                    *received_clone.lock() = Some(val);

                    // Respond ok
                    let resp = serde_json::json!({ "result": { "status": "ok" } });
                    let resp_payload = serde_json::to_vec(&resp).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );

        let ctx = BrowserCommandContext {
            browser_id: "authorized-browser-A".into(),
            command: "navigate".into(),
            params: Some(serde_json::json!({
                "browserId": "victim-browser-B",
                "command": "eval",
                "operation": "eval",
                "script": "stolen()"
            })),
            document_generation: Some("1".into()),
            ..Default::default()
        };

        let _ = backend.execute_command(ctx).await;

        let sent = received_req
            .lock()
            .clone()
            .expect("Must have received framed request");
        // R5-2: Authoritative envelope target must remain authorized-browser-A, NOT overwritten by params
        assert_eq!(
            sent.get("browserId").and_then(|v| v.as_str()),
            Some("authorized-browser-A"),
            "Authoritative browserId was overwritten by params! Sent: {:?}",
            sent
        );
        assert_ne!(
            sent.get("command").and_then(|v| v.as_str()),
            Some("eval"),
            "Command was overwritten by params! Sent: {:?}",
            sent
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r5_7_numeric_generation_and_revision_parsing() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("r5_7_test.sock");
        let token_path = temp_dir.path().join("r5_7_test.token");
        std::fs::write(&token_path, "secret-token").unwrap();

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = tokio::io::BufReader::new(reader);

                // Handshake
                let mut line = String::new();
                use tokio::io::AsyncBufReadExt;
                let _ = reader.read_line(&mut line).await;
                let _ = writer.write_all(b"{\"type\":\"remoteAttached\"}\n").await;
                let _ = writer.flush().await;

                // Read framed request
                let mut hdr = [0u8; 5];
                if reader.read_exact(&mut hdr).await.is_ok() {
                    let len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
                    let mut payload = vec![0u8; len];
                    let _ = reader.read_exact(&mut payload).await;

                    // Response has NUMERIC generation 42 and NUMERIC viewportRevision 7
                    let resp = serde_json::json!({
                        "result": {
                            "browserId": "b1",
                            "generation": 42,
                            "viewportRevision": 7,
                            "visible": true
                        }
                    });
                    let resp_payload = serde_json::to_vec(&resp).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );

        let state = backend
            .get_state(
                "b1",
                &DesktopScope {
                    workspace_id: "".into(),
                    worktree_slug: "".into(),
                },
            )
            .await
            .unwrap();

        // R5-7: Must parse numeric generation 42 as "42", not fall back to "1"
        assert_eq!(
            state.document_generation, "42",
            "Numeric generation was not parsed correctly, fell back to 1"
        );
        assert_eq!(
            state.viewport_revision, "7",
            "Numeric viewportRevision was not parsed correctly, fell back to 1"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r6_7_missing_viewport_revision_rejected_without_fallback() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("test-r6-7.sock");

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);

                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() {
                    let attach_resp = serde_json::json!({
                        "type": "remoteAttached",
                        "status": "ok"
                    });
                    let mut attach_bytes = serde_json::to_vec(&attach_resp).unwrap();
                    attach_bytes.push(b'\n');
                    let _ = writer.write_all(&attach_bytes).await;
                    let _ = writer.flush().await;

                    let mut header = [0u8; 5];
                    let _ = reader.read_exact(&mut header).await;
                    let payload_len =
                        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
                    let mut payload = vec![0u8; payload_len];
                    let _ = reader.read_exact(&mut payload).await;

                    // Response has documentGeneration 42, but MISSING viewportRevision
                    let resp = serde_json::json!({
                        "type": "remoteResult",
                        "status": "ok",
                        "result": {
                            "browserId": "b1",
                            "documentGeneration": 42,
                            "url": "https://example.com",
                            "title": "Example",
                            "visible": true
                        }
                    });
                    let resp_payload = serde_json::to_vec(&resp).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );

        let res = backend
            .get_state(
                "b1",
                &DesktopScope {
                    workspace_id: "".into(),
                    worktree_slug: "".into(),
                },
            )
            .await;

        assert!(
            res.is_err(),
            "Missing viewportRevision in getState must be rejected with error, not fabricated to '1'"
        );
    }

    #[tokio::test]
    async fn test_r6_3_list_inventory_filters_by_workspace_and_worktree() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));

        // Register session 1 in ws1, wt-1
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b1-wt1".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: Some("wt-1".into()),
                url: "https://example.com/1".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // Register session 2 in ws1, wt-2 (different worktree!)
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b2-wt2".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: Some("wt-2".into()),
                url: "https://example.com/2".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // Scope specifies ws1 AND wt-1
        let scope = DesktopScope {
            workspace_id: "ws1".into(),
            worktree_slug: "wt-1".into(),
        };

        let list = backend.list_sessions(&scope).await.unwrap();
        // Must only match b1-wt1, NOT b2-wt2!
        assert_eq!(
            list.len(),
            1,
            "Must only match sessions in the specified worktree"
        );
        assert_eq!(list[0].browser_id, "b1-wt1");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r6_3_local_ipc_list_filters_by_worktree() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("test-r6-3-ipc.sock");

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);

                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() {
                    let attach_resp = serde_json::json!({
                        "type": "remoteAttached",
                        "status": "ok"
                    });
                    let mut attach_bytes = serde_json::to_vec(&attach_resp).unwrap();
                    attach_bytes.push(b'\n');
                    let _ = writer.write_all(&attach_bytes).await;
                    let _ = writer.flush().await;

                    let mut header = [0u8; 5];
                    let _ = reader.read_exact(&mut header).await;
                    let payload_len =
                        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
                    let mut payload = vec![0u8; payload_len];
                    let _ = reader.read_exact(&mut payload).await;

                    let req_val: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                    assert_eq!(
                        req_val.get("worktreeSlug").and_then(|v| v.as_str()),
                        Some("wt-target")
                    );

                    let resp = serde_json::json!({
                        "type": "remoteResult",
                        "status": "ok",
                        "result": [
                            {
                                "browserId": "b-target",
                                "workspaceId": "ws1",
                                "worktreePath": "wt-target",
                                "url": "https://example.com/target",
                                "title": "Target",
                                "visible": true
                            },
                            {
                                "browserId": "b-other",
                                "workspaceId": "ws1",
                                "worktreePath": "wt-other",
                                "url": "https://example.com/other",
                                "title": "Other",
                                "visible": true
                            }
                        ]
                    });
                    let resp_payload = serde_json::to_vec(&resp).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );

        let scope = DesktopScope {
            workspace_id: "ws1".into(),
            worktree_slug: "wt-target".into(),
        };

        let list = backend.list_sessions(&scope).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].browser_id, "b-target");
    }

    #[tokio::test]
    async fn test_r5_8_service_lease_epoch_synchronization() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));

        // Register session
        let _ = manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-lease-sync".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();

        // R5-8: Gateway epoch 42 must be adopted by backend claim_driver so both brokers agree on epoch
        let gateway_epoch = 42u64;
        backend
            .claim_driver("b-lease-sync", "dev-1", "conn-1", "sub-1", gateway_epoch)
            .await
            .expect("claim_driver must succeed");

        // The service broker must now validate lease epoch 42
        let val_res = broker.validate_lease("b-lease-sync", gateway_epoch, "dev-1", "conn-1");
        assert!(
            val_res.is_ok(),
            "Broker did not synchronize gateway lease epoch 42: {:?}",
            val_res
        );

        // Heartbeat driver extends the lease
        let hb_res = backend
            .heartbeat_driver("dev-1", "conn-1", "sub-1", gateway_epoch)
            .await;
        assert!(
            hb_res.is_ok(),
            "Heartbeat driver must succeed: {:?}",
            hb_res
        );
    }

    #[tokio::test]
    async fn test_r5_1_remote_gateway_manager_from_daemon_retains_backend() {
        let client = Arc::new(crate::daemon::client::DaemonClient::new());
        let mgr = crate::ipc::remote::RemoteGatewayManager::from_daemon(client);
        assert!(mgr.browser_backend().is_none());

        let test_backend = Arc::new(InProcessTestBackend::new());
        mgr.set_browser_backend(test_backend.clone());
        assert!(
            mgr.browser_backend().is_some(),
            "RemoteGatewayManager::from_daemon must retain backend set via set_browser_backend"
        );
    }

    #[tokio::test]
    async fn test_r5_8_ws_claim_propagates_backend_error_and_heartbeat_renews() {
        use crate::remote::auth::DevicePermission;
        use crate::remote::browser_admission::AdmissionController;
        use crate::remote::browser_protocol::{ClientMessage, ServerMessage};
        use crate::remote::browser_ws::BrowserWsSession;
        use std::time::Instant;

        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "conn-err".into(),
            "dev-err".into(),
            "b-missing".into(),
            DevicePermission::Control,
            now,
        );
        let admission = AdmissionController::new();

        // Use InProcessBrowserServiceBackend without registered session so claim fails
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new(
            (*manager).clone(),
            Arc::clone(&broker),
        ));
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessBrowserServiceBackend::new(
            Arc::clone(&service),
            Arc::clone(&manager),
        ));

        // Fake a subscription on session
        session.subscription_id = Some("sub-test".into());

        // Attempt driver claim for nonexistent browser: backend will fail, and WS MUST NOT swallow the error
        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "req-fail".into(),
            subscription_id: "sub-test".into(),
            browser_id: "b-missing".into(),
        };
        let resp = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();

        match resp {
            ServerMessage::BrowserError { ref code, .. } => {
                assert!(
                    code == "BROWSER_FORBIDDEN"
                        || code == "BROWSER_NOT_FOUND"
                        || code == "BROWSER_DRIVER_BUSY",
                    "Expected propagated error code from backend failure, got {code}"
                );
            }
            other => panic!("Expected BrowserError, got {:?}", other),
        }
        assert!(
            !session.is_driver,
            "Session must not be marked as driver when backend claim fails"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_r6_1_framed_image_subscription_produces_frames_and_stops_on_unsubscribe() {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let temp_dir = tempfile::tempdir().unwrap();
        let sock_path = temp_dir.path().join("test-r6-1.sock");

        let listener = tokio::net::UnixListener::bind(&sock_path).unwrap();
        let (server_unsub_tx, mut server_unsub_rx) = tokio::sync::mpsc::channel(1);
        // Fired by the test once `subscribe_viewer` has returned and frame 1 was received.
        let (go_tx, mut go_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);

                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() {
                    let attach_resp = serde_json::json!({
                        "type": "remoteAttached",
                        "status": "ok"
                    });
                    let mut attach_bytes = serde_json::to_vec(&attach_resp).unwrap();
                    attach_bytes.push(b'\n');
                    let _ = writer.write_all(&attach_bytes).await;
                    let _ = writer.flush().await;

                    // Read SubscribeViewer frame
                    let mut header = [0u8; 5];
                    let _ = reader.read_exact(&mut header).await;
                    let payload_len =
                        u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
                    let mut payload = vec![0u8; payload_len];
                    let _ = reader.read_exact(&mut payload).await;

                    // Write SubscribeViewer response
                    let resp = serde_json::json!({
                        "type": "remoteResult",
                        "status": "ok",
                        "result": {
                            "subscriptionId": "sub-live-1",
                            "streamId": 1,
                            "options": {
                                "format": "jpeg",
                                "quality": 70,
                                "intervalMs": 250,
                                "maxEdge": 1280
                            },
                            "identity": {
                                "browserInstanceId": "bi1",
                                "browserServiceEpoch": "1",
                                "desktopEpoch": "1",
                                "documentGeneration": "1"
                            }
                        }
                    });
                    let resp_payload = serde_json::to_vec(&resp).unwrap();
                    let mut frame = Vec::new();
                    frame.extend_from_slice(&(resp_payload.len() as u32).to_le_bytes());
                    frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
                    frame.extend_from_slice(&resp_payload);
                    let _ = writer.write_all(&frame).await;
                    let _ = writer.flush().await;

                    // Stream an image frame
                    let dummy_img = vec![0xDE, 0xAD, 0xBE, 0xEF];
                    let mut img_frame = Vec::new();
                    img_frame.extend_from_slice(&(dummy_img.len() as u32).to_le_bytes());
                    img_frame.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_IMAGE);
                    img_frame.extend_from_slice(&dummy_img);
                    let _ = writer.write_all(&img_frame).await;
                    let _ = writer.flush().await;

                    // Behave like the GUI forwarding loop: stop streaming the moment the
                    // daemon half-closes the connection (EOF) or unsubscribes; otherwise send
                    // frame 2 once the test releases it, after `subscribe_viewer` returned.
                    let mut sent_second = false;
                    loop {
                        tokio::select! {
                            biased;
                            _ = reader.read_exact(&mut header) => {
                                let _ = server_unsub_tx.send(()).await;
                                break;
                            }
                            _ = &mut go_rx, if !sent_second => {
                                sent_second = true;
                                let second = vec![0xCA, 0xFE];
                                let mut f = Vec::new();
                                f.extend_from_slice(&(second.len() as u32).to_le_bytes());
                                f.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_IMAGE);
                                f.extend_from_slice(&second);
                                let _ = writer.write_all(&f).await;
                                let _ = writer.flush().await;
                            }
                        }
                    }
                }
            }
        });

        let backend = LocalIpcBrowserBackend::new(
            sock_path.to_str().unwrap().to_string(),
            Some("secret-token".to_string()),
        );

        let mut rx = backend.subscribe_frames("b1").await.unwrap();

        // Subscribe viewer starts persistent image stream
        let (sub_id, _, _, _) = backend
            .subscribe_viewer("b1", "dev1", "view1", None)
            .await
            .unwrap();
        assert_eq!(sub_id, "sub-live-1");

        // Verify frame delivery over the transport into subscribe_frames receiver
        let frame = tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
            .await
            .expect("Frame must arrive through subscribe_frames")
            .unwrap();
        assert_eq!(frame, vec![0xDE, 0xAD, 0xBE, 0xEF]);

        // B3: the stream must stay open after `subscribe_viewer` returned. If the daemon
        // dropped its write half, the mock GUI saw EOF and stopped, so frame 2 never arrives.
        go_tx.send(()).unwrap();
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("a second frame must arrive after subscribe_viewer returned")
            .unwrap();
        assert_eq!(second, vec![0xCA, 0xFE]);

        // Unsubscribe stops the stream
        backend.unsubscribe_viewer("b1", &sub_id).await.unwrap();
        let unsub_acked = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            server_unsub_rx.recv(),
        )
        .await;
        assert!(
            unsub_acked.is_ok(),
            "Unsubscribe must close or notify the server stream"
        );
    }

    /// Mock GUI: optionally answers the handshake, reads one framed request, writes `reply`
    /// verbatim (if any), then keeps the connection open until the client closes it.
    #[cfg(unix)]
    fn spawn_mock_gui(sock_path: &std::path::Path, handshake: bool, reply: Option<Vec<u8>>) {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::UnixListener::bind(sock_path).unwrap();
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            if handshake {
                let _ = writer
                    .write_all(b"{\"type\":\"remoteAttached\",\"status\":\"ok\"}\n")
                    .await;
                let mut header = [0u8; 5];
                if reader.read_exact(&mut header).await.is_ok() {
                    let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]])
                        as usize;
                    let mut payload = vec![0u8; len];
                    let _ = reader.read_exact(&mut payload).await;
                }
            }
            if let Some(bytes) = reply {
                let _ = writer.write_all(&bytes).await;
                let _ = writer.flush().await;
            }
            // Hold the connection open until the daemon side closes it.
            let mut sink = [0u8; 64];
            while matches!(reader.read(&mut sink).await, Ok(n) if n > 0) {}
        });
    }

    #[cfg(unix)]
    fn json_frame(value: &serde_json::Value) -> Vec<u8> {
        LocalIpcBrowserBackend::encode_ipc_frame(
            LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON,
            &serde_json::to_vec(value).unwrap(),
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn mock_backend(sock_path: &std::path::Path) -> LocalIpcBrowserBackend {
        LocalIpcBrowserBackend::new(sock_path.to_str().unwrap().to_string(), Some("tok".into()))
    }

    #[cfg(unix)]
    fn exec_ctx() -> BrowserCommandContext {
        BrowserCommandContext {
            browser_id: "b1".into(),
            command: "click".into(),
            ..Default::default()
        }
    }

    /// B9: the structured `code` (not the message) restores the typed error over LocalIpc,
    /// and `details` rebuilds the refusal explanation.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_restores_typed_errors_from_structured_code() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("b9.sock");
        let details = serde_json::json!({
            "code": "UNSUPPORTED_POINT_INPUT",
            "reason": "iframe point input refused",
            "inputClass": "iframe",
            "remediation": "use a reference",
        });
        spawn_mock_gui(
            &sock,
            true,
            Some(json_frame(&serde_json::json!({
                "type": "error",
                "code": "UNSUPPORTED",
                "message": "misleading message that says BROWSER_UNAVAILABLE",
                "details": details,
            }))),
        );
        let res = mock_backend(&sock).execute_command(exec_ctx()).await;
        match res {
            Err(RemoteBrowserError::InputRefused(explanation)) => {
                assert_eq!(explanation.to_details(), details);
            }
            other => panic!("UNSUPPORTED with details must be InputRefused, got {other:?}"),
        }

        let sock2 = dir.path().join("b9b.sock");
        spawn_mock_gui(
            &sock2,
            true,
            Some(json_frame(&serde_json::json!({
                "type": "error", "code": "BROWSER_UNAVAILABLE", "message": "no webview",
            }))),
        );
        let res = mock_backend(&sock2).execute_command(exec_ctx()).await;
        assert!(
            matches!(res, Err(RemoteBrowserError::Unavailable(_))),
            "BROWSER_UNAVAILABLE must be Unavailable, got {res:?}"
        );

        let sock3 = dir.path().join("b9c.sock");
        spawn_mock_gui(
            &sock3,
            true,
            Some(json_frame(&serde_json::json!({
                "type": "error", "code": "UNSUPPORTED", "message": "no details here",
            }))),
        );
        let res = mock_backend(&sock3).execute_command(exec_ctx()).await;
        assert!(
            matches!(res, Err(RemoteBrowserError::InputRefused(_))),
            "UNSUPPORTED without details must still be InputRefused, got {res:?}"
        );
    }

    /// B1: a length prefix above the cap is refused before any buffer is allocated.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_rejects_oversized_length_prefix_without_allocating() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("b1.sock");
        let mut header = Vec::new();
        header.extend_from_slice(&u32::MAX.to_le_bytes());
        header.push(LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON);
        spawn_mock_gui(&sock, true, Some(header));
        let backend = mock_backend(&sock).with_io_timeout(std::time::Duration::from_secs(5));
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            backend.execute_command(exec_ctx()),
        )
        .await
        .expect("an oversized prefix must be refused immediately, not waited on");
        assert!(
            matches!(res, Err(RemoteBrowserError::ExecutionFailed(_))),
            "oversized prefix must be ExecutionFailed, got {res:?}"
        );
    }

    /// B2: a GUI that never answers the handshake fails within the I/O timeout.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_handshake_read_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("b2.sock");
        spawn_mock_gui(&sock, false, None);
        let backend = mock_backend(&sock).with_io_timeout(std::time::Duration::from_millis(100));
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            backend.execute_command(exec_ctx()),
        )
        .await
        .expect("the handshake read must be bounded by the I/O timeout");
        assert!(
            matches!(res, Err(RemoteBrowserError::Unavailable(_))),
            "a silent GUI must be Unavailable, got {res:?}"
        );
    }

    /// B2: an endless handshake line is cut off at the line limit instead of buffered.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_handshake_line_is_length_bounded() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("b2line.sock");
        let listener = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            let _ = reader.read_line(&mut line).await;
            let _ = writer.write_all(&vec![b'a'; 200 * 1024]).await;
            let _ = writer.flush().await;
            let mut sink = [0u8; 64];
            while matches!(tokio::io::AsyncReadExt::read(&mut reader, &mut sink).await, Ok(n) if n > 0) {}
        });
        let backend = mock_backend(&sock).with_io_timeout(std::time::Duration::from_secs(30));
        let res = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            backend.execute_command(exec_ctx()),
        )
        .await
        .expect("an unterminated handshake line must be rejected at the size limit");
        assert!(
            matches!(res, Err(RemoteBrowserError::Unavailable(_))),
            "got {res:?}"
        );
    }

    /// B10: an unrecognised list shape is an error, and items without an ID are skipped.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_list_rejects_unknown_shape_and_skips_idless_items() {
        let dir = tempfile::tempdir().unwrap();
        let scope = DesktopScope {
            workspace_id: "".into(),
            worktree_slug: "".into(),
        };
        let sock = dir.path().join("b10a.sock");
        spawn_mock_gui(
            &sock,
            true,
            Some(json_frame(&serde_json::json!({ "result": { "unexpected": 1 } }))),
        );
        let res = mock_backend(&sock).list_sessions(&scope).await;
        assert!(
            matches!(res, Err(RemoteBrowserError::ExecutionFailed(_))),
            "unknown list shape must be ExecutionFailed, got {res:?}"
        );

        let sock2 = dir.path().join("b10b.sock");
        spawn_mock_gui(
            &sock2,
            true,
            Some(json_frame(&serde_json::json!({
                "result": [ { "title": "no id" }, { "browserId": "b-ok" } ]
            }))),
        );
        let list = mock_backend(&sock2).list_sessions(&scope).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].browser_id, "b-ok");
    }

    /// B-get: an `error` field in a getState result is a failure, not a NotFound.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_get_state_error_field_is_execution_failed() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("bget.sock");
        spawn_mock_gui(
            &sock,
            true,
            Some(json_frame(&serde_json::json!({ "result": { "error": "boom" } }))),
        );
        let res = mock_backend(&sock)
            .get_state(
                "b1",
                &DesktopScope {
                    workspace_id: "".into(),
                    worktree_slug: "".into(),
                },
            )
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::ExecutionFailed(_))),
            "got {res:?}"
        );
    }

    /// B-hb: heartbeat errors keep their own code instead of all becoming Forbidden.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_heartbeat_errors_are_typed_by_code() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("bhb.sock");
        spawn_mock_gui(
            &sock,
            true,
            Some(json_frame(&serde_json::json!({
                "type": "error", "code": "BROWSER_INVALID_REQUEST", "message": "bad",
            }))),
        );
        let res = mock_backend(&sock)
            .heartbeat_driver("d", "c", "s", 1)
            .await;
        assert!(
            matches!(res, Err(RemoteBrowserError::InvalidRequest(_))),
            "got {res:?}"
        );
    }

    /// B12: a subscribe response with a missing/oversized streamId or unparseable options is
    /// refused instead of being fabricated.
    #[cfg(unix)]
    #[tokio::test]
    async fn local_ipc_subscribe_refuses_fabricated_stream_id_and_options() {
        let dir = tempfile::tempdir().unwrap();
        let identity = serde_json::json!({
            "browserInstanceId": "bi1", "browserServiceEpoch": "1",
            "desktopEpoch": "1", "documentGeneration": "1"
        });
        let good_options = serde_json::json!({ "format": "jpeg" });
        let cases = [
            ("missing", serde_json::json!({
                "subscriptionId": "s", "options": good_options, "identity": identity })),
            ("oversized", serde_json::json!({
                "subscriptionId": "s", "streamId": 4_294_967_296u64,
                "options": good_options, "identity": identity })),
            ("bad-options", serde_json::json!({
                "subscriptionId": "s", "streamId": 1,
                "options": { "format": "bogus" }, "identity": identity })),
        ];
        for (name, result) in cases {
            let sock = dir.path().join(format!("b12-{name}.sock"));
            spawn_mock_gui(&sock, true, Some(json_frame(&serde_json::json!({ "result": result }))));
            let res = mock_backend(&sock)
                .subscribe_viewer("b1", "d", "v", None)
                .await;
            assert!(
                matches!(res, Err(RemoteBrowserError::ExecutionFailed(_))),
                "{name}: must be ExecutionFailed, got {res:?}"
            );
        }
    }

    /// B13: broker failures keep distinct typed errors.
    #[test]
    fn driver_broker_errors_map_to_distinct_variants() {
        use crate::browser::remote_driver::RemoteDriverError;
        assert!(matches!(
            map_driver_error(RemoteDriverError::BrowserDriverBusy {
                current_owner: "dev-secret".into(),
                lease_epoch: 3,
            }),
            RemoteBrowserError::DriverBusy(ref m) if !m.contains("dev-secret")
        ));
        assert!(matches!(
            map_driver_error(RemoteDriverError::RateLimited),
            RemoteBrowserError::RateLimited(_)
        ));
        assert!(matches!(
            map_driver_error(RemoteDriverError::DesktopReclaimed),
            RemoteBrowserError::Forbidden(_)
        ));
    }

    /// B13 end to end through the in-process backend: a second device gets DriverBusy and a
    /// claim burst gets RateLimited.
    #[tokio::test]
    async fn in_process_claim_reports_busy_and_rate_limited() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), Arc::clone(&broker)));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-claim".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        backend
            .claim_driver("b-claim", "dev1", "conn1", "sub1", 5)
            .await
            .unwrap();
        let busy = backend
            .claim_driver("b-claim", "dev2", "conn2", "sub2", 6)
            .await;
        assert!(matches!(busy, Err(RemoteBrowserError::DriverBusy(_))), "got {busy:?}");
        // dev1 already used 1 of its 2 claims per second; the renewal is the 2nd, the next is over.
        backend
            .claim_driver("b-claim", "dev1", "conn1", "sub1", 5)
            .await
            .unwrap();
        let limited = backend
            .claim_driver("b-claim", "dev1", "conn1", "sub1", 5)
            .await;
        assert!(
            matches!(limited, Err(RemoteBrowserError::RateLimited(_))),
            "got {limited:?}"
        );
    }

    /// B5: `subscribe_frames` only hands out a receiver. It must not take a viewer slot or
    /// start capture under a random device ID.
    #[tokio::test]
    async fn in_process_subscribe_frames_does_not_create_a_viewer() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));
        let backend =
            InProcessBrowserServiceBackend::new(Arc::clone(&service), Arc::clone(&manager));
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-frames".into()),
                workspace_id: Some("ws1".into()),
                worktree_path: None,
                url: "https://example.com".into(),
                profile: None,
                zoom_factor: None,
                bounds: None,
                visible: Some(true),
            })
            .unwrap();
        let _rx = backend.subscribe_frames("b-frames").await.unwrap();
        assert!(
            !service.is_producer_active("b-frames"),
            "subscribe_frames must not start capture"
        );
        // Both real viewer slots stay available.
        service.subscribe("b-frames", "dev-a", "view-a").unwrap();
        service.subscribe("b-frames", "dev-b", "view-b").unwrap();
    }
}
