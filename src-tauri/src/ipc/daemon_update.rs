//! Pending daemon update commands (todo 9): the "Restart terminals now" action and its re-read.
//! Both talk to the previous daemon only over the frozen control channel.

use super::error::IpcError;
use crate::daemon::client::DaemonClient;
use crate::daemon::update_pending::RestartOutcome;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonUpdatePendingStatus {
    pub running_version: Option<String>,
    pub live_sessions: Option<u32>,
    pub retire_requested: bool,
}

/// Re-reads Inventory after CountChanged. RetireWhenIdle is never sent twice in one GUI run.
#[tauri::command]
pub async fn cmd_daemon_update_pending_status(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<DaemonUpdatePendingStatus, IpcError> {
    let pending = daemon_client.update_pending_status().await;
    Ok(DaemonUpdatePendingStatus {
        running_version: pending.running_version,
        live_sessions: pending.live_sessions,
        retire_requested: pending.retire_requested,
    })
}

/// Sends control Shutdown { confirm_live_sessions } exactly once, after the user confirmed n.
#[tauri::command]
pub async fn cmd_daemon_restart_previous_terminals(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    confirm_live_sessions: u32,
) -> Result<RestartOutcome, IpcError> {
    daemon_client
        .restart_previous_version_terminals(confirm_live_sessions)
        .await
}
