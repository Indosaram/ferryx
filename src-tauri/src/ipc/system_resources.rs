//! Host and per-session resource sample, read from the daemon that owns the sessions.

use std::sync::Arc;

use tauri::State;

use crate::daemon::resource_usage::HostResourceSnapshot;
use crate::daemon::DaemonClient;
use crate::ipc::IpcError;

#[tauri::command]
pub async fn cmd_system_resources(
    daemon_client: State<'_, Arc<DaemonClient>>,
) -> Result<HostResourceSnapshot, IpcError> {
    daemon_client.inner().clone().resource_usage().await
}
