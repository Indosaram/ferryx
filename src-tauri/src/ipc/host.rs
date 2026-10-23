use crate::ipc::IpcError;

/// Host boot time in ms since the UNIX epoch; `None` when the platform cannot report it.
#[tauri::command]
pub async fn cmd_host_boot_time_ms() -> Result<Option<u64>, IpcError> {
    crate::ipc::run_blocking(|| Ok(crate::host_boot::host_boot_time_ms())).await
}
