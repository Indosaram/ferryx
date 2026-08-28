// allow: SIZE_OK — IPC remote gateway command handlers and forwarding bridge within constrained write scope
use crate::daemon::client::DaemonClient;
use crate::ipc::IpcError;
use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::protocol::{RemoteActiveDesktopSelection, RemoteTerminalTabInfo};
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{
    RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode, RemoteRestartPolicy,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Clone)]
pub struct RemoteGatewayManager {
    inner: RemoteGatewayManagerInner,
}

#[derive(Clone)]
enum RemoteGatewayManagerInner {
    Daemon(Arc<DaemonClient>),
    State {
        state: Arc<RemoteGatewayState>,
        server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
    },
}

impl RemoteGatewayManager {
    pub fn new(state: Arc<RemoteGatewayState>) -> Self {
        Self {
            inner: RemoteGatewayManagerInner::State {
                state,
                server_handle: Arc::new(Mutex::new(None)),
            },
        }
    }

    pub fn from_daemon(daemon_client: Arc<DaemonClient>) -> Self {
        Self {
            inner: RemoteGatewayManagerInner::Daemon(daemon_client),
        }
    }

    pub fn state(&self) -> Option<&Arc<RemoteGatewayState>> {
        match &self.inner {
            RemoteGatewayManagerInner::State { state, .. } => Some(state),
            RemoteGatewayManagerInner::Daemon(_) => None,
        }
    }

    pub async fn restore_persisted_listener(&self) -> Result<bool, String> {
        match &self.inner {
            RemoteGatewayManagerInner::Daemon(client) => {
                let status = client
                    .remote_get_status()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(status.is_running)
            }
            RemoteGatewayManagerInner::State {
                state,
                server_handle,
            } => {
                let mode = state.config.read().mode;
                if mode == RemoteNetworkMode::Off {
                    return Ok(false);
                }

                if let Some(handle) = server_handle.lock().take() {
                    handle.stop();
                }

                let (handle, _addr) = start_remote_server(Arc::clone(state))
                    .await
                    .map_err(|e| format!("Failed to restore remote gateway listener: {e}"))?;
                *server_handle.lock() = Some(handle);
                Ok(true)
            }
        }
    }

    pub fn stop(&self) {
        match &self.inner {
            RemoteGatewayManagerInner::Daemon(_) => {}
            RemoteGatewayManagerInner::State { server_handle, .. } => {
                if let Some(handle) = server_handle.lock().take() {
                    handle.stop();
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayStatusResponse {
    /// True only while this process has a bound listener. Never restored from disk.
    pub enabled: bool,
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub bound_address: Option<String>,
    pub local_ip: Option<String>,
    pub restart_policy: RemoteRestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableRemoteGatewayRequest {
    pub mode: RemoteNetworkMode,
    pub port: Option<u16>,
    pub allow_control: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePairingCodeResponse {
    pub code: String,
    pub expires_in_seconds: u32,
}

#[tauri::command]
pub async fn cmd_remote_status(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    let local_ip = get_local_ip();

    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let status = client.remote_get_status().await?;
            Ok(RemoteGatewayStatusResponse {
                enabled: status.is_running,
                mode: status.mode,
                port: status.port,
                bound_address: status.bound_address,
                local_ip,
                restart_policy: RemoteRestartPolicy::RestoreListener,
            })
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            let config = state.config.read().clone();
            let is_running = *state.is_running.read();
            let bound_address = state.bound_address.read().clone();
            Ok(RemoteGatewayStatusResponse {
                enabled: is_running,
                mode: config.mode,
                port: config.port,
                bound_address,
                local_ip,
                restart_policy: config.restart_policy(),
            })
        }
    }
}

#[tauri::command]
pub async fn cmd_remote_enable(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    request: EnableRemoteGatewayRequest,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let current = client.remote_get_status().await?;
            let config = RemoteGatewayConfig {
                mode: request.mode,
                port: request.port.unwrap_or(current.port),
                allow_control: request.allow_control.unwrap_or(current.allow_control),
            };
            client.remote_configure(config).await?;
        }
        RemoteGatewayManagerInner::State {
            state,
            server_handle,
        } => {
            if let Some(handle) = server_handle.lock().take() {
                handle.stop();
            }
            {
                let mut config = state.config.write();
                config.mode = request.mode;
                if let Some(port) = request.port {
                    config.port = port;
                }
                if let Some(allow_ctrl) = request.allow_control {
                    config.allow_control = allow_ctrl;
                }
            }
            let _ = state.persist_config();
            if request.mode != RemoteNetworkMode::Off {
                let (handle, _addr) =
                    start_remote_server(Arc::clone(state)).await.map_err(|e| {
                        IpcError::internal(format!("Failed to start remote gateway: {e}"))
                    })?;
                *server_handle.lock() = Some(handle);
            }
        }
    }
    cmd_remote_status(manager).await
}

#[tauri::command]
pub async fn cmd_remote_disable(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let current = client.remote_get_status().await?;
            let config = RemoteGatewayConfig {
                mode: RemoteNetworkMode::Off,
                port: current.port,
                allow_control: current.allow_control,
            };
            client.remote_configure(config).await?;
        }
        RemoteGatewayManagerInner::State {
            state,
            server_handle,
        } => {
            if let Some(handle) = server_handle.lock().take() {
                handle.stop();
            }
            {
                let mut config = state.config.write();
                config.mode = RemoteNetworkMode::Off;
                *state.is_running.write() = false;
                *state.bound_address.write() = None;
            }
            let _ = state.persist_config();
        }
    }
    cmd_remote_status(manager).await
}

#[tauri::command]
pub async fn cmd_remote_pairing_create(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    permission: Option<DevicePermission>,
) -> Result<CreatePairingCodeResponse, IpcError> {
    let perm = permission.unwrap_or(DevicePermission::Control);
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let code = client.remote_create_pairing_code(Some(perm)).await?;
            Ok(CreatePairingCodeResponse {
                code,
                expires_in_seconds: 60,
            })
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            let code = state.auth_manager.create_pairing_code(perm);
            Ok(CreatePairingCodeResponse {
                code,
                expires_in_seconds: 60,
            })
        }
    }
}

#[tauri::command]
pub async fn cmd_remote_devices(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<Vec<DeviceInfo>, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => client.remote_list_devices().await,
        RemoteGatewayManagerInner::State { state, .. } => Ok(state.auth_manager.list_devices()),
    }
}

#[tauri::command]
pub async fn cmd_remote_device_revoke(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    device_id: String,
) -> Result<bool, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            match client.remote_revoke_device(&device_id).await {
                Ok(()) => Ok(true),
                Err(_) => Ok(false),
            }
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            Ok(state.auth_manager.revoke_device(&device_id))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveDesktopSelectionRequest {
    pub workspace_id: Option<String>,
    pub worktree_slug: Option<String>,
    pub worktree_label: Option<String>,
    pub session_id: Option<String>,
    #[serde(default, alias = "activeTabId")]
    pub tab_id: Option<String>,
    #[serde(default, alias = "tabs")]
    pub terminal_tabs: Vec<RemoteTerminalTabInfo>,
}

fn remote_terminal_tab_label(label: String) -> String {
    let trimmed = label.trim();
    if trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("~\\")
        || trimmed.starts_with("file:")
        || trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().first(), Some(b'A'..=b'Z' | b'a'..=b'z'))
    {
        "Terminal".to_string()
    } else if trimmed.is_empty() {
        "Terminal".to_string()
    } else {
        trimmed.to_string()
    }
}

fn sanitize_activity_state(state: Option<String>) -> Option<String> {
    let s = state?;
    match s.trim().to_ascii_lowercase().as_str() {
        "working" => Some("working".to_string()),
        "waiting" | "blocked" => Some("waiting".to_string()),
        "done" => Some("done".to_string()),
        _ => None,
    }
}

fn sanitize_agent_type(agent_type: Option<String>) -> Option<String> {
    let s = agent_type?;
    let trimmed = s.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("~\\")
        || trimmed.starts_with("file:")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || (trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().first(), Some(b'A'..=b'Z' | b'a'..=b'z')))
    {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[tauri::command]
pub async fn cmd_remote_set_active_selection(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    request: SetActiveDesktopSelectionRequest,
) -> Result<(), IpcError> {
    let selection = if request.workspace_id.is_none()
        && request.worktree_slug.is_none()
        && request.worktree_label.is_none()
        && request.session_id.is_none()
        && request.tab_id.is_none()
        && request.terminal_tabs.is_empty()
    {
        None
    } else {
        Some(RemoteActiveDesktopSelection {
            workspace_id: request.workspace_id,
            worktree_slug: request.worktree_slug,
            worktree_label: request.worktree_label,
            session_id: request.session_id,
            tab_id: request.tab_id,
            terminal_tabs: request
                .terminal_tabs
                .into_iter()
                .map(|tab| RemoteTerminalTabInfo {
                    id: tab.id,
                    label: remote_terminal_tab_label(tab.label),
                    activity_state: sanitize_activity_state(tab.activity_state),
                    agent_type: sanitize_agent_type(tab.agent_type),
                })
                .collect(),
        })
    };
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            client.remote_set_active_selection(selection).await
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            state.set_active_selection_opt(selection);
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn cmd_remote_get_active_selection(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<Option<RemoteActiveDesktopSelection>, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => client.remote_get_active_selection().await,
        RemoteGatewayManagerInner::State { state, .. } => Ok(state.active_selection()),
    }
}

fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip().to_string())
}
