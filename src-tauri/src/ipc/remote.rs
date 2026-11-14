// allow: SIZE_OK — IPC remote gateway command handlers and forwarding bridge within constrained write scope
use crate::daemon::client::DaemonClient;
use crate::ipc::IpcError;
use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::protocol::{
    RemoteActiveDesktopSelection, RemoteTerminalTabInfo, RemoteWorktreeAttention,
};
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{
    RemoteGatewayConfig, RemoteGatewayState, RemoteNetworkMode, RemoteRestartPolicy,
    REMOTE_GATEWAY_PORT,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(feature = "desktop")]
use tauri::State;

#[derive(Clone)]
pub struct RemoteGatewayManager {
    inner: RemoteGatewayManagerInner,
    browser_backend: Arc<
        parking_lot::RwLock<Option<Arc<dyn crate::remote::browser_backend::RemoteBrowserBackend>>>,
    >,
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
            browser_backend: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn from_daemon(daemon_client: Arc<DaemonClient>) -> Self {
        Self {
            inner: RemoteGatewayManagerInner::Daemon(daemon_client),
            browser_backend: Arc::new(parking_lot::RwLock::new(None)),
        }
    }

    pub fn state(&self) -> Option<&Arc<RemoteGatewayState>> {
        match &self.inner {
            RemoteGatewayManagerInner::State { state, .. } => Some(state),
            RemoteGatewayManagerInner::Daemon(_) => None,
        }
    }

    pub(crate) fn daemon_client(&self) -> Option<&Arc<DaemonClient>> {
        match &self.inner {
            RemoteGatewayManagerInner::Daemon(client) => Some(client),
            RemoteGatewayManagerInner::State { .. } => None,
        }
    }

    pub fn set_browser_backend(
        &self,
        backend: Arc<dyn crate::remote::browser_backend::RemoteBrowserBackend>,
    ) {
        *self.browser_backend.write() = Some(Arc::clone(&backend));
        match &self.inner {
            RemoteGatewayManagerInner::State { state, .. } => {
                state.set_browser_backend(backend);
            }
            RemoteGatewayManagerInner::Daemon(_) => {
                // In daemon mode, retain the configured backend reference so the GUI
                // bridge and callers can access it directly (R5-1).
            }
        }
    }

    pub fn browser_backend(
        &self,
    ) -> Option<Arc<dyn crate::remote::browser_backend::RemoteBrowserBackend>> {
        if let Some(backend) = self.browser_backend.read().clone() {
            return Some(backend);
        }
        match &self.inner {
            RemoteGatewayManagerInner::State { state, .. } => Some(state.browser_backend()),
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
    pub relay_url: Option<String>,
    /// The relay the running gateway actually uses (saved relayUrl > FERRYX_RELAY_URL >
    /// enrollment relayOrigin > default, or the fallback it failed over to). `relay_url` stays
    /// the saved value. Absent while no relay runs or from a daemon that does not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_relay_url: Option<String>,
    /// Where the primary of `resolved_relay_url` came from: `config`, `env`, `enrollment`, `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_relay_url_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_connected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_channel_connected: Option<bool>,
    // N5 (round 2): surface the direct-gateway gate projection to the renderer
    // so the UI can distinguish a gated LAN listener from ordinary status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_status: Option<crate::remote::server::DirectGatewayGateStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_reason: Option<String>,
    /// Insecure transport modes in effect (`direct`, `relay`) in the gateway process or this one.
    /// Non-empty means the desktop shows its persistent insecure-transport banner.
    #[serde(default)]
    pub insecure_modes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableRemoteGatewayRequest {
    pub mode: RemoteNetworkMode,
    /// Deprecated, kept for wire compatibility, always ignored; gateway port is fixed to REMOTE_GATEWAY_PORT.
    pub port: Option<u16>,
    pub allow_control: Option<bool>,
    #[serde(default)]
    pub relay_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePairingCodeResponse {
    pub code: String,
    pub expires_in_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
}

/// The desktop status for a daemon-owned gateway: the daemon's own view, including the relay URL
/// it resolved and its source, plus this process's insecure modes and local IP.
pub(crate) fn status_from_daemon(
    status: crate::daemon::protocol::DaemonRemoteStatus,
    local_ip: Option<String>,
    local_insecure: Vec<String>,
) -> RemoteGatewayStatusResponse {
    let mut insecure_modes = status.insecure_modes;
    for mode in local_insecure {
        if !insecure_modes.contains(&mode) {
            insecure_modes.push(mode);
        }
    }
    RemoteGatewayStatusResponse {
        enabled: status.is_running,
        mode: status.mode,
        port: status.port,
        bound_address: status.bound_address,
        local_ip,
        restart_policy: RemoteRestartPolicy::RestoreListener,
        relay_url: status.relay_url,
        resolved_relay_url: status.resolved_relay_url,
        resolved_relay_url_source: status.resolved_relay_url_source,
        machine_id: status.machine_id,
        relay_connected: status.relay_connected,
        control_channel_connected: status.control_channel_connected,
        gate_status: status.gate_status,
        gate_reason: status.gate_reason,
        insecure_modes,
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    /// D20-1: the desktop status carries the daemon's resolved relay URL and source, while
    /// `relayUrl` stays the saved value (here none: the URL came from FERRYX_RELAY_URL).
    #[test]
    fn the_desktop_status_carries_the_daemons_resolved_relay() {
        let status = crate::daemon::protocol::DaemonRemoteStatus {
            mode: RemoteNetworkMode::Relay,
            port: REMOTE_GATEWAY_PORT,
            allow_control: true,
            is_running: true,
            bound_address: None,
            relay_url: None,
            resolved_relay_url: Some("https://relay.selfhost.example".into()),
            resolved_relay_url_source: Some("env".into()),
            machine_id: Some("m-1".into()),
            relay_connected: Some(true),
            control_channel_connected: Some(true),
            gate_status: None,
            gate_reason: None,
            insecure_modes: vec!["relay".into()],
        };
        let response = status_from_daemon(status, Some("10.0.0.2".into()), vec!["direct".into(), "relay".into()]);
        assert_eq!(response.relay_url, None);
        assert_eq!(response.resolved_relay_url.as_deref(), Some("https://relay.selfhost.example"));
        assert_eq!(response.resolved_relay_url_source.as_deref(), Some("env"));
        assert_eq!(response.insecure_modes, ["relay", "direct"]);
        let wire = serde_json::to_value(&response).unwrap();
        assert_eq!(wire["resolvedRelayUrl"], "https://relay.selfhost.example");
        assert_eq!(wire["resolvedRelayUrlSource"], "env");
        assert!(wire["relayUrl"].is_null());
    }

    /// D20-1: the in-process gateway reports `RemoteGatewayState::active_relay`, not the saved URL.
    #[test]
    fn the_in_process_status_source_is_the_recorded_resolution() {
        let state = RemoteGatewayState::new(
            Arc::new(crate::terminal::TerminalService::default()),
            crate::worktree::WorkspaceRegistry::new(),
        );
        assert_eq!(state.active_relay(), None);
        *state.resolved_relay.write() = Some((
            "https://relay.enrolled.example".into(),
            crate::remote::state::RelayUrlSource::Enrollment,
        ));
        let (url, source) = state.active_relay().unwrap();
        assert_eq!(url, "https://relay.enrolled.example");
        assert_eq!(source.code(), "enrollment");
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_remote_status(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    let local_ip = get_local_ip();
    let local_insecure = crate::ipc::run_blocking(|| {
        Ok(crate::remote::security_settings::active_insecure_modes())
    })
    .await?;

    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let status = client.remote_get_status().await?;
            Ok(status_from_daemon(status, local_ip, local_insecure))
        }
        RemoteGatewayManagerInner::State {
            state,
            server_handle,
            ..
        } => {
            let config = state.config.read().clone();
            let is_running = *state.is_running.read();
            let bound_address = state.bound_address.read().clone();
            let identity = crate::remote::auth::canonical_identity_dir()
                .ok()
                .and_then(|dir| crate::remote::auth::load_or_generate_machine_identity(&dir).ok());
            let machine_id = identity.map(|id| id.machine_id);
            let (relay_connected, control_channel_connected) = {
                let relay_client = state.relay_client.read();
                crate::remote::relay_client::relay_connection_flags(relay_client.as_ref())
            };
            let resolved_relay = state.active_relay();
            Ok(RemoteGatewayStatusResponse {
                enabled: is_running,
                mode: config.mode,
                port: config.port,
                bound_address,
                local_ip,
                restart_policy: config.restart_policy(),
                relay_url: config.relay_url,
                resolved_relay_url: resolved_relay.as_ref().map(|(url, _)| url.clone()),
                resolved_relay_url_source: resolved_relay.map(|(_, source)| source.code().to_string()),
                machine_id,
                relay_connected,
                control_channel_connected,
                gate_status: {
                    let handle_guard = server_handle.lock();
                    handle_guard.as_ref().map(|h| h.gate_status())
                },
                gate_reason: {
                    let handle_guard = server_handle.lock();
                    handle_guard.as_ref().and_then(|h| match h.gate_status() {
                        crate::remote::server::DirectGatewayGateStatus::InsecureLanGated {
                            reason,
                            ..
                        } => Some(reason),
                        _ => None,
                    })
                },
                insecure_modes: local_insecure,
            })
        }
    }
}

#[cfg(feature = "desktop")]
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
                port: REMOTE_GATEWAY_PORT,
                allow_control: request.allow_control.unwrap_or(current.allow_control),
                relay_url: request.relay_url.or(current.relay_url),
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
                config.port = REMOTE_GATEWAY_PORT;
                if let Some(allow_ctrl) = request.allow_control {
                    config.allow_control = allow_ctrl;
                }
                if request.relay_url.is_some() {
                    config.relay_url = request.relay_url;
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

#[cfg(feature = "desktop")]
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
                relay_url: current.relay_url,
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

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_remote_pairing_create(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    permission: Option<DevicePermission>,
) -> Result<CreatePairingCodeResponse, IpcError> {
    let perm = permission.unwrap_or(DevicePermission::Control);
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let (code, pairing_token, machine_id, relay_url) = client
                .remote_create_pairing_code_detailed(Some(perm))
                .await?;
            Ok(CreatePairingCodeResponse {
                code,
                expires_in_seconds: 60,
                pairing_token,
                machine_id,
                relay_url,
            })
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            let coordinator = state
                .relay_pairing
                .read()
                .as_ref()
                .map(|published| published.coordinator.clone());
            if let Some(coordinator) = coordinator {
                let info = coordinator
                    .generate_pairing_with_permission(std::time::Duration::from_secs(60), perm)
                    .await
                    .map_err(IpcError::internal)?;
                let config = state.config.read().clone();
                let effective_relay = Some(crate::remote::state::resolve_relay_url(&config).0);
                Ok(CreatePairingCodeResponse {
                    code: info.pin,
                    expires_in_seconds: 60,
                    pairing_token: Some(info.pairing_token),
                    machine_id: Some(info.machine_id),
                    relay_url: effective_relay,
                })
            } else {
                let code = state.auth_manager.create_pairing_code(perm);
                let machine_id = crate::remote::auth::canonical_identity_dir()
                    .ok()
                    .and_then(|dir| {
                        crate::remote::auth::load_or_generate_machine_identity(&dir).ok()
                    })
                    .map(|id| id.machine_id);
                Ok(CreatePairingCodeResponse {
                    code,
                    expires_in_seconds: 60,
                    pairing_token: None,
                    machine_id,
                    relay_url: None,
                })
            }
        }
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_remote_devices(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<Vec<DeviceInfo>, IpcError> {
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => client.remote_list_devices().await,
        RemoteGatewayManagerInner::State { state, .. } => Ok(state.auth_manager.list_devices()),
    }
}

#[cfg(feature = "desktop")]
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
        RemoteGatewayManagerInner::State { state, .. } => state
            .auth_manager
            .revoke_device(&device_id)
            .map_err(|e| IpcError::internal(e.to_string())),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetActiveDesktopSelectionRequest {
    #[serde(default)]
    pub attention_inventory: Vec<RemoteWorktreeAttention>,
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
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("~\\")
        || trimmed.starts_with("file:")
        || trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().first(), Some(b'A'..=b'Z' | b'a'..=b'z'))
    {
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

/// Worktree slugs and branch labels legitimately contain `/` (`orca/<ws-id>/<slug>`), so this
/// rejects only filesystem-path leaks instead of every separator.
fn sanitize_worktree_text(value: Option<String>) -> Option<String> {
    let s = value?;
    let trimmed = s.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with("~/")
        || trimmed.starts_with("~\\")
        || trimmed.starts_with("file:")
        || (trimmed.as_bytes().get(1) == Some(&b':')
            && matches!(trimmed.as_bytes().first(), Some(b'A'..=b'Z' | b'a'..=b'z')))
    {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn sanitize_session_id(value: Option<String>) -> Option<String> {
    let s = value?;
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_remote_set_active_selection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    manager: State<'_, Arc<RemoteGatewayManager>>,
    request: SetActiveDesktopSelectionRequest,
) -> Result<(), IpcError> {
    let selection = if request.workspace_id.is_none()
        && request.worktree_slug.is_none()
        && request.worktree_label.is_none()
        && request.session_id.is_none()
        && request.tab_id.is_none()
        && request.terminal_tabs.is_empty()
        && request.attention_inventory.is_empty()
    {
        None
    } else {
        Some(RemoteActiveDesktopSelection {
            attention_inventory: request
                .attention_inventory
                .into_iter()
                .map(|entry| RemoteWorktreeAttention {
                    workspace_id: entry.workspace_id,
                    worktree_slug: sanitize_worktree_text(entry.worktree_slug),
                    worktree_label: sanitize_worktree_text(entry.worktree_label),
                    state: sanitize_activity_state(entry.state),
                })
                .collect(),
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
                    worktree_slug: sanitize_worktree_text(tab.worktree_slug),
                    worktree_label: sanitize_worktree_text(tab.worktree_label),
                    session_id: sanitize_session_id(tab.session_id),
                })
                .collect(),
        })
    };
    match &manager.inner {
        RemoteGatewayManagerInner::Daemon(client) => {
            let path = crate::ipc::ssh::get_ssh_store_path(&app)?;
            client
                .remote_set_active_selection_with_ssh_store(selection, Some(path))
                .await
        }
        RemoteGatewayManagerInner::State { state, .. } => {
            *state.ssh_store_path.write() = Some(crate::ipc::ssh::get_ssh_store_path(&app)?);
            state.set_active_selection_opt(selection);
            Ok(())
        }
    }
}

#[cfg(feature = "desktop")]
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
