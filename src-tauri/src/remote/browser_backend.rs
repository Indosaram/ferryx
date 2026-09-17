//! Remote Browser Backend Trait & Providers (Local IPC & Unavailable)
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§1.1, §1.3, §4.4, §6.3)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::browser::manager::BrowserManager;
use crate::browser::remote_service::BrowserRemoteService;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRemoteState {
    pub browser_id: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCommandContext {
    pub browser_id: String,
    pub command: String,
    pub params: Option<serde_json::Value>,
    pub document_generation: Option<String>,
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
    ) -> BoxFuture<'a, Result<(String, u32, crate::remote::browser_protocol::BrowserSubscribeOptions), RemoteBrowserError>> {
        let _ = (browser_id, device_id, viewer_instance_id);
        Box::pin(async move {
            Ok((format!("sub-{}", uuid::Uuid::new_v4()), 1, options.unwrap_or_default()))
        })
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
    ) -> BoxFuture<'a, Result<(String, u32, crate::remote::browser_protocol::BrowserSubscribeOptions), RemoteBrowserError>> {
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
) -> Result<(crate::browser::model::BrowserWaitCondition, Option<std::time::Duration>), RemoteBrowserError> {
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
        if let Ok(cond) = serde_json::from_value::<crate::browser::model::BrowserWaitCondition>(cond_val.clone()) {
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
        "Missing or invalid wait condition: expected string script, tagged condition, or selector".into(),
    ))
}

/// Local IPC framed provider connecting daemon to GUI process (§1.1, §4.4)
pub struct LocalIpcBrowserBackend {
    socket_path: String,
    local_credential: Option<String>,
    frame_senders: parking_lot::Mutex<HashMap<String, tokio::sync::broadcast::Sender<Vec<u8>>>>,
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
            frame_senders: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    pub fn encode_ipc_frame(content_type: u8, payload: &[u8]) -> Result<Vec<u8>, RemoteBrowserError> {
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

    pub fn decode_ipc_frame(bytes: &[u8]) -> Result<Option<(u8, Vec<u8>, usize)>, RemoteBrowserError> {
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

    async fn send_framed_request(&self, request_bytes: &[u8]) -> Result<Vec<u8>, RemoteBrowserError> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        if self.socket_path.is_empty() {
            return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
        }
        let sock_path = std::path::Path::new(&self.socket_path);
        if !sock_path.exists() {
            return Err(RemoteBrowserError::Unavailable(
                "GUI process not running or socket unconnected".into(),
            ));
        }
        let stream = tokio::net::UnixStream::connect(sock_path).await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("GUI process not running or socket unconnected: {e}"))
        })?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let token = if let Some(ref t) = self.local_credential {
            t.clone()
        } else {
            let token_path = sock_path.with_file_name(match sock_path.file_name().and_then(|n| n.to_str()) {
                Some(name) => format!("{name}.token"),
                None => "browser.token".to_string(),
            });
            std::fs::read_to_string(&token_path).unwrap_or_default().trim().to_string()
        };

        let handshake = serde_json::json!({
            "command": "remoteAttach",
            "token": token,
        });
        let mut handshake_bytes = serde_json::to_vec(&handshake).map_err(|e| {
            RemoteBrowserError::ExecutionFailed(e.to_string())
        })?;
        handshake_bytes.push(b'\n');
        writer.write_all(&handshake_bytes).await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("Failed to write handshake: {e}"))
        })?;
        writer.flush().await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("Failed to flush handshake: {e}"))
        })?;

        let mut resp_line = String::new();
        reader.read_line(&mut resp_line).await.map_err(|e| {
            RemoteBrowserError::Unavailable(format!("Failed to read handshake response: {e}"))
        })?;

        let resp_val: serde_json::Value = serde_json::from_str(resp_line.trim()).map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Invalid handshake response: {e}"))
        })?;
        if resp_val.get("type").and_then(|v| v.as_str()) != Some("remoteAttached") {
            let err_msg = resp_val.get("message").and_then(|v| v.as_str()).unwrap_or("Handshake rejected");
            return Err(RemoteBrowserError::Forbidden(err_msg.to_string()));
        }

        let frame = Self::encode_ipc_frame(Self::IPC_CONTENT_TYPE_JSON, request_bytes)?;
        writer.write_all(&frame).await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to write framed request: {e}"))
        })?;
        writer.flush().await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to flush framed request: {e}"))
        })?;

        let mut header = [0u8; 5];
        reader.read_exact(&mut header).await.map_err(|e| {
            RemoteBrowserError::ExecutionFailed(format!("Failed to read response frame header: {e}"))
        })?;
        let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut payload = vec![0u8; payload_len];
        if payload_len > 0 {
            reader.read_exact(&mut payload).await.map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to read response payload: {e}"))
            })?;
        }
        Ok(payload)
    }
}

impl RemoteBrowserBackend for LocalIpcBrowserBackend {
    fn list_sessions<'a>(
        &'a self,
        scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            let req = serde_json::json!({
                "command": "list",
                "workspaceId": scope.workspace_id,
            });
            let payload = self.send_framed_request(&serde_json::to_vec(&req).unwrap()).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to parse session list: {e}"))
            })?;
            let items = if let Some(arr) = val.as_array() {
                arr.clone()
            } else if let Some(arr) = val.get("result").and_then(|v| v.as_array()) {
                arr.clone()
            } else if let Some(arr) = val.get("sessions").and_then(|v| v.as_array()) {
                arr.clone()
            } else {
                Vec::new()
            };

            let mut out = Vec::new();
            for item in items {
                let browser_id = item.get("browserId").or_else(|| item.get("browser_id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = item.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                let url = item.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
                let visible = item.get("visible").and_then(|v| v.as_bool()).unwrap_or(true);
                let ws_id = item.get("workspaceId").or_else(|| item.get("workspace_id")).and_then(|v| v.as_str());

                if scope.workspace_id.is_empty() || ws_id == Some(&scope.workspace_id) {
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
            Ok(list.iter().find(|s| s.visible).cloned().or_else(|| list.into_iter().next()))
        })
    }

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            let req = serde_json::json!({
                "command": "getState",
                "browserId": browser_id,
            });
            let payload = self.send_framed_request(&serde_json::to_vec(&req).unwrap()).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to parse getState response: {e}"))
            })?;
            let data = val.get("result").unwrap_or(&val);
            if let Some(err) = data.get("error").and_then(|v| v.as_str()) {
                return Err(RemoteBrowserError::NotFound(err.to_string()));
            }
            Ok(BrowserRemoteState {
                browser_id: data.get("browserId").and_then(|v| v.as_str()).unwrap_or(browser_id).to_string(),
                url: data.get("url").and_then(|v| v.as_str()).map(|s| s.to_string()),
                title: data.get("title").and_then(|v| v.as_str()).map(|s| s.to_string()),
                document_generation: data.get("generation").or_else(|| data.get("documentGeneration")).and_then(|v| v.as_str()).unwrap_or("1").to_string(),
                viewport_revision: data.get("viewportRevision").and_then(|v| v.as_str()).unwrap_or("1").to_string(),
                loading: data.get("loading").and_then(|v| v.as_bool()).unwrap_or(false),
                paused: !data.get("visible").and_then(|v| v.as_bool()).unwrap_or(true),
                pause_reason: if !data.get("visible").and_then(|v| v.as_bool()).unwrap_or(true) { Some("hidden".into()) } else { None },
            })
        })
    }

    fn execute_command(
        &self,
        ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            let mut req = serde_json::json!({
                "command": ctx.command,
                "browserId": ctx.browser_id,
            });
            if let Some(params) = &ctx.params {
                if let Some(obj) = params.as_object() {
                    for (k, v) in obj {
                        req[k] = v.clone();
                    }
                }
            }
            if let Some(gen) = &ctx.document_generation {
                req["documentGeneration"] = serde_json::json!(gen);
            }
            let payload = self.send_framed_request(&serde_json::to_vec(&req).unwrap()).await?;
            let val: serde_json::Value = serde_json::from_slice(&payload).map_err(|e| {
                RemoteBrowserError::ExecutionFailed(format!("Failed to parse command response: {e}"))
            })?;
            if let Some(err_code) = val.get("code").and_then(|v| v.as_str()) {
                let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("Operation failed");
                return match err_code {
                    "BROWSER_CLI_REQUEST_INVALID" | "BROWSER_INVALID_REQUEST" => Err(RemoteBrowserError::InvalidRequest(msg.to_string())),
                    "BROWSER_WAIT_TIMEOUT" => Err(RemoteBrowserError::WaitTimeout),
                    "BROWSER_CLI_UNAUTHORIZED" => Err(RemoteBrowserError::Forbidden(msg.to_string())),
                    _ => Err(RemoteBrowserError::ExecutionFailed(msg.to_string())),
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
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
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
        _browser_id: &'a str,
        _device_id: &'a str,
        _viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<'a, Result<(String, u32, crate::remote::browser_protocol::BrowserSubscribeOptions), RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            if !std::path::Path::new(&self.socket_path).exists() {
                return Err(RemoteBrowserError::Unavailable(
                    "GUI process not running or socket unconnected".into(),
                ));
            }
            let sub_id = format!("ipc-sub-{}", uuid::Uuid::new_v4());
            Ok((sub_id, 1, options.unwrap_or_default()))
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
            let available = if self.socket_path.is_empty() {
                false
            } else {
                std::path::Path::new(&self.socket_path).exists()
            };
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
    pub negotiated_options: Mutex<HashMap<String, crate::remote::browser_protocol::BrowserSubscribeOptions>>,
}

impl InProcessTestBackend {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            states: Mutex::new(HashMap::new()),
            frame_senders: Mutex::new(HashMap::new()),
            negotiated_options: Mutex::new(HashMap::new()),
        }
    }
}

impl RemoteBrowserBackend for InProcessTestBackend {
    fn list_sessions<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            Ok(self.sessions.lock().await.clone())
        })
    }

    fn identify_session<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            Ok(self.sessions.lock().await.first().cloned())
        })
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
    ) -> BoxFuture<'a, Result<(String, u32, crate::remote::browser_protocol::BrowserSubscribeOptions), RemoteBrowserError>> {
        Box::pin(async move {
            let mut opts = self.negotiated_options.lock().await;
            let negotiated = opts
                .entry(browser_id.to_string())
                .or_insert_with(|| options.unwrap_or_default())
                .clone();
            let sub_id = format!("sub-{}", uuid::Uuid::new_v4());
            Ok((sub_id, 1, negotiated))
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
    pub fn new(
        remote_service: Arc<BrowserRemoteService>,
        manager: Arc<BrowserManager>,
    ) -> Self {
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
                    if scope.workspace_id.is_empty() {
                        true
                    } else {
                        s.workspace_id.as_deref() == Some(&scope.workspace_id)
                    }
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
            Ok(list.iter().find(|s| s.visible).cloned().or_else(|| list.first().cloned()))
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
            let (_, _, vp_rev) = self
                .manager
                .get_geometry(browser_id)
                .unwrap_or((None, 1.0, 1));
            Ok(BrowserRemoteState {
                browser_id: state.browser_id,
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
                "getState" => {
                    let state = self.manager.get_state(&ctx.browser_id).map_err(|_e| {
                        RemoteBrowserError::NotFound(ctx.browser_id.clone())
                    })?;
                    let (_, _, vp_rev) = self
                        .manager
                        .get_geometry(&ctx.browser_id)
                        .unwrap_or((None, 1.0, 1));
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
                            .update_url(&ctx.browser_id, url)
                            .map_err(|e| RemoteBrowserError::ExecutionFailed(e.to_string()))?;
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
                        let state = self.manager.get_state(&ctx.browser_id).map_err(|_e| {
                            RemoteBrowserError::NotFound(ctx.browser_id.clone())
                        })?;
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
                    if matches!(condition, crate::browser::model::BrowserWaitCondition::Function { .. }) {
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
            if !self.remote_service.is_producer_active(browser_id) {
                let dev_id = format!("backend-sub-{}", uuid::Uuid::new_v4());
                let view_id = format!("backend-view-{}", uuid::Uuid::new_v4());
                if let Ok(sub_id) = self.remote_service.subscribe(browser_id, &dev_id, &view_id) {
                    self.active_subscriptions
                        .lock()
                        .await
                        .insert(sub_id, browser_id.to_string());
                }
            }
            Ok(self.remote_service.subscribe_frames(browser_id))
        })
    }

    fn subscribe_viewer<'a>(
        &'a self,
        browser_id: &'a str,
        device_id: &'a str,
        viewer_instance_id: &'a str,
        options: Option<crate::remote::browser_protocol::BrowserSubscribeOptions>,
    ) -> BoxFuture<'a, Result<(String, u32, crate::remote::browser_protocol::BrowserSubscribeOptions), RemoteBrowserError>> {
        Box::pin(async move {
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
                .subscribe_with_profile(browser_id, device_id, viewer_instance_id, requested_profile)
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
                    crate::browser::snapshot_source::SnapshotFormat::Jpeg { quality } => Some(quality),
                    _ => None,
                },
                interval_ms: Some(negotiated_prof.interval_ms),
                max_edge: Some(negotiated_prof.max_edge),
            };

            Ok((sub_id, stream_id, negotiated_options))
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

#[cfg(test)]
pub mod tests {
    use super::*;

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
    async fn test_local_ipc_backend_framed_codec() {
        let payload = b"{\"command\":\"ping\"}";
        let frame = LocalIpcBrowserBackend::encode_ipc_frame(
            LocalIpcBrowserBackend::IPC_CONTENT_TYPE_JSON,
            payload,
        )
        .unwrap();

        assert_eq!(frame.len(), 5 + payload.len());
        let decoded = LocalIpcBrowserBackend::decode_ipc_frame(&frame).unwrap().unwrap();
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
        assert!(matches!(sub_unavail, Err(RemoteBrowserError::Unavailable(_))));

        // 2. LocalIpcBrowserBackend with empty socket path returns typed Unavailable
        let local_empty = LocalIpcBrowserBackend::new("".into(), None);
        let sub_empty = local_empty.subscribe_frames("b1").await;
        assert!(matches!(sub_empty, Err(RemoteBrowserError::Unavailable(msg)) if msg.contains("empty")));

        // LocalIpcBrowserBackend with non-empty path but disconnected socket
        let local_ipc = LocalIpcBrowserBackend::new("/tmp/test.sock".into(), None);
        let sub_local = local_ipc.subscribe_frames("b1").await;
        assert!(matches!(sub_local, Err(RemoteBrowserError::Unavailable(msg)) if msg.contains("not running")));

        // 3. InProcessTestBackend returns live broadcast receiver
        let test_backend = InProcessTestBackend::new();
        let mut rx = test_backend.subscribe_frames("b1").await.unwrap();

        // Broadcast a test frame
        let sender = test_backend.frame_senders.lock().await.get("b1").unwrap().clone();
        sender.send(vec![0xAA, 0xBB, 0xCC]).unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received, vec![0xAA, 0xBB, 0xCC]);
    }

    #[tokio::test]
    async fn test_in_process_browser_service_backend_lifecycle_and_subscription() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));

        let backend = InProcessBrowserServiceBackend::new(
            Arc::clone(&service),
            Arc::clone(&manager),
        );

        let scope = DesktopScope {
            workspace_id: "ws-1".into(),
            worktree_slug: "wt-1".into(),
        };

        // Register session in manager
        manager
            .register_session(crate::browser::model::CreateBrowserRequest {
                browser_id: Some("b-prod-1".into()),
                workspace_id: Some("ws-1".into()),
                worktree_path: None,
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
            })
            .await
            .unwrap();
        assert!(cmd_res.success);

        // 4. Wire subscription ownership: subscribe_viewer activates producer and reconciles stream_id
        assert!(!service.is_producer_active("b-prod-1"));
        let (sub_id, stream_id, _) = backend
            .subscribe_viewer("b-prod-1", "dev-backend-1", "viewer-1", None)
            .await
            .unwrap();
        assert!(service.is_producer_active("b-prod-1"));
        assert_eq!(stream_id, service.active_stream_id("b-prod-1").unwrap());

        // 5. Wire teardown: unsubscribe_viewer calls unsubscribe and stops producer
        backend
            .unsubscribe_viewer("b-prod-1", &sub_id)
            .await
            .unwrap();
        assert!(!service.is_producer_active("b-prod-1"));
    }

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
                    let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
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
        let sessions = backend.list_sessions(&scope).await.expect("must list sessions over bridge IPC");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].browser_id, "b-bridge-1");
    }

    #[tokio::test]
    async fn test_r4_2_execute_command_rejects_unsupported_and_requires_executor() {
        let manager = Arc::new(BrowserManager::new());
        let broker = Arc::new(crate::browser::remote_driver::RemoteDriverBroker::new());
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), broker));
        let backend = InProcessBrowserServiceBackend::new(
            Arc::clone(&service),
            Arc::clone(&manager),
        );

        // 1. Unknown / unsupported command must be rejected with InvalidRequest, NOT succeed
        let unsupp_res = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "unsupportedCmd".into(),
                params: None,
                document_generation: None,
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
        let service = Arc::new(BrowserRemoteService::new((*manager).clone(), Arc::clone(&broker)));
        let backend = InProcessBrowserServiceBackend::new(
            Arc::clone(&service),
            Arc::clone(&manager),
        );

        // 1. Eval without active driver lease must be rejected as Forbidden
        let unapproved_eval = backend
            .execute_command(BrowserCommandContext {
                browser_id: "b1".into(),
                command: "eval".into(),
                params: Some(serde_json::json!({
                    "script": "1 + 1"
                })),
                document_generation: None,
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
                        Err(RemoteBrowserError::ExecutionFailed("click failed: element not found".into()))
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
                document_generation: None,
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
                document_generation: None,
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
                document_generation: None,
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
                document_generation: None,
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

        let fake_source = Arc::new(crate::browser::snapshot_source::FakeBrowserSnapshotSource::new(
            crate::browser::snapshot_source::FakeSnapshotBehavior::Auto {
                width: 800,
                height: 600,
            },
        ));
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
}
