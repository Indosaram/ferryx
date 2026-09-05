use crate::ipc::{run_blocking, IpcError};
use crate::permissions::{
    get_system_permissions_status, open_system_settings_for_target, request_accessibility,
    OpenPermissionsSettingsResult, SystemPermissionsStatus,
};

#[tauri::command]
pub async fn cmd_permissions_get_status() -> Result<SystemPermissionsStatus, IpcError> {
    run_blocking(|| Ok(get_system_permissions_status())).await
}

#[tauri::command]
pub async fn cmd_permissions_open_settings(
    target: String,
) -> Result<OpenPermissionsSettingsResult, IpcError> {
    run_blocking(move || Ok(open_system_settings_for_target(&target))).await
}

#[tauri::command]
pub async fn cmd_permissions_request_accessibility() -> Result<bool, IpcError> {
    run_blocking(|| Ok(request_accessibility())).await
}
