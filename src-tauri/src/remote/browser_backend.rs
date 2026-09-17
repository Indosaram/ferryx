//! Remote Browser Backend Trait & Providers (Local IPC & Unavailable)
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§1.1, §1.3, §4.4, §6.3)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
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

/// Local IPC framed provider connecting daemon to GUI process (§1.1, §4.4)
pub struct LocalIpcBrowserBackend {
    socket_path: String,
    _local_credential: Option<String>,
}

impl LocalIpcBrowserBackend {
    pub const IPC_CONTENT_TYPE_JSON: u8 = 0x01;
    pub const IPC_CONTENT_TYPE_IMAGE: u8 = 0x02;
    pub const MAX_JSON_PAYLOAD: usize = 512 * 1024; // 512 KiB
    pub const MAX_IMAGE_PAYLOAD: usize = 2 * 1024 * 1024; // 2 MiB

    pub fn new(socket_path: String, local_credential: Option<String>) -> Self {
        Self {
            socket_path,
            _local_credential: local_credential,
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
}

impl RemoteBrowserBackend for LocalIpcBrowserBackend {
    fn list_sessions<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Vec<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            Err(RemoteBrowserError::Unavailable("GUI process not running or socket unconnected".into()))
        })
    }

    fn identify_session<'a>(
        &'a self,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<Option<RemoteBrowserSessionSummary>, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            Err(RemoteBrowserError::Unavailable("GUI process not running or socket unconnected".into()))
        })
    }

    fn get_state<'a>(
        &'a self,
        browser_id: &'a str,
        _scope: &'a DesktopScope,
    ) -> BoxFuture<'a, Result<BrowserRemoteState, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            Err(RemoteBrowserError::NotFound(browser_id.to_string()))
        })
    }

    fn execute_command(
        &self,
        _ctx: BrowserCommandContext,
    ) -> BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            Err(RemoteBrowserError::ExecutionFailed("GUI process not connected".into()))
        })
    }

    fn subscribe_frames<'a>(
        &'a self,
        _browser_id: &'a str,
    ) -> BoxFuture<'a, Result<tokio::sync::broadcast::Receiver<Vec<u8>>, RemoteBrowserError>> {
        Box::pin(async move {
            if self.socket_path.is_empty() {
                return Err(RemoteBrowserError::Unavailable("Local IPC socket path empty".into()));
            }
            Err(RemoteBrowserError::Unavailable("GUI process not running or socket unconnected".into()))
        })
    }

    fn capabilities(&self) -> BoxFuture<'_, BrowserCapabilities> {
        Box::pin(async move {
            BrowserCapabilities {
                browser_available: !self.socket_path.is_empty(),
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
}

impl InProcessTestBackend {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            states: Mutex::new(HashMap::new()),
            frame_senders: Mutex::new(HashMap::new()),
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
}
