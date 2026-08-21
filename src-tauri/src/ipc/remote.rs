use crate::ipc::IpcError;
use crate::remote::auth::{DeviceInfo, DevicePermission};
use crate::remote::server::{start_remote_server, RemoteServerHandle};
use crate::remote::state::{RemoteGatewayState, RemoteNetworkMode};
use crate::remote::tailscale::{check_tailscale_status, SystemCommandRunner, TailscaleStatus};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

#[derive(Clone)]
pub struct RemoteGatewayManager {
    state: Arc<RemoteGatewayState>,
    server_handle: Arc<Mutex<Option<RemoteServerHandle>>>,
}

impl RemoteGatewayManager {
    pub fn new(state: Arc<RemoteGatewayState>) -> Self {
        Self {
            state,
            server_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn state(&self) -> &Arc<RemoteGatewayState> {
        &self.state
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteGatewayStatusResponse {
    pub enabled: bool,
    pub mode: RemoteNetworkMode,
    pub port: u16,
    pub bound_address: Option<String>,
    pub local_ip: Option<String>,
    pub tailscale: TailscaleStatus,
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
    let state = manager.state();
    let config = state.config.read().clone();
    let is_running = *state.is_running.read();
    let bound_address = state.bound_address.read().clone();
    let tailscale = check_tailscale_status(&SystemCommandRunner);

    let local_ip = get_local_ip();
    Ok(RemoteGatewayStatusResponse {
        enabled: is_running,
        mode: config.mode,
        port: config.port,
        bound_address,
        local_ip,
        tailscale,
    })
}

#[tauri::command]
pub async fn cmd_remote_enable(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    request: EnableRemoteGatewayRequest,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    let state = manager.state();

    // Stop existing server if running
    if let Some(handle) = manager.server_handle.lock().take() {
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

    if request.mode != RemoteNetworkMode::Off {
        let (handle, _addr) = start_remote_server(Arc::clone(state))
            .await
            .map_err(|e| IpcError::internal(format!("Failed to start remote gateway: {e}")))?;
        *manager.server_handle.lock() = Some(handle);
    }

    cmd_remote_status(manager).await
}

#[tauri::command]
pub async fn cmd_remote_disable(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<RemoteGatewayStatusResponse, IpcError> {
    if let Some(handle) = manager.server_handle.lock().take() {
        handle.stop();
    }
    state_set_off(manager.state());
    cmd_remote_status(manager).await
}

fn state_set_off(state: &Arc<RemoteGatewayState>) {
    let mut config = state.config.write();
    config.mode = RemoteNetworkMode::Off;
    *state.is_running.write() = false;
    *state.bound_address.write() = None;
}

#[tauri::command]
pub async fn cmd_remote_pairing_create(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    permission: Option<DevicePermission>,
) -> Result<CreatePairingCodeResponse, IpcError> {
    let perm = permission.unwrap_or(DevicePermission::Control);
    let code = manager.state().auth_manager.create_pairing_code(perm);
    Ok(CreatePairingCodeResponse {
        code,
        expires_in_seconds: 60,
    })
}

#[tauri::command]
pub async fn cmd_remote_devices(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<Vec<DeviceInfo>, IpcError> {
    Ok(manager.state().auth_manager.list_devices())
}

#[tauri::command]
pub async fn cmd_remote_device_revoke(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    device_id: String,
) -> Result<bool, IpcError> {
    Ok(manager.state().auth_manager.revoke_device(&device_id))
}

#[tauri::command]
pub async fn cmd_tailscale_status() -> Result<TailscaleStatus, IpcError> {
    Ok(check_tailscale_status(&SystemCommandRunner))
}

fn get_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|addr| addr.ip().to_string())
}
