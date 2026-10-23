//! The desktop's Supervisor view: a snapshot of what the hub supervisor drives and watches, and
//! the pause a person sets on one lane. Both go to the daemon, which owns the hub.

use super::error::{IpcError, IpcErrorCode};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use std::sync::Arc;

/// A daemon that knows these requests types every failure, so an untyped error is an older daemon.
fn failure(message: String, code: Option<String>) -> IpcError {
    match code {
        None => IpcError::new(
            IpcErrorCode::DaemonUpgradeRequired,
            "The running Ferryx daemon predates the Supervisor view; it appears after the daemon restarts",
        ),
        Some(code) => IpcError::new(IpcErrorCode::from_code_str(&code), message),
    }
}

fn snapshot_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorSnapshotOk { snapshot } => Ok(snapshot),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor snapshot response")),
    }
}

fn paused_from(response: DaemonResponse) -> Result<bool, IpcError> {
    match response {
        DaemonResponse::SupervisorSetPausedOk { paused } => Ok(paused),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor pause response")),
    }
}

#[tauri::command]
pub async fn cmd_supervisor_snapshot(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<serde_json::Value, IpcError> {
    snapshot_from(daemon_client.send_request(DaemonRequest::SupervisorSnapshot).await?)
}

#[tauri::command]
pub async fn cmd_supervisor_set_paused(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    lane_key: String,
    paused: bool,
) -> Result<bool, IpcError> {
    paused_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorSetPaused { lane_key, paused })
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untyped_error_means_the_daemon_is_older() {
        let old = DaemonResponse::Error { message: "unknown variant".into(), code: None, details: None };
        assert_eq!(snapshot_from(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
        let typed = DaemonResponse::Error {
            message: "the supervisor hub is not open in this daemon".into(),
            code: Some("SUPERVISOR_UNAVAILABLE".into()),
            details: None,
        };
        assert_ne!(paused_from(typed).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
        assert!(paused_from(DaemonResponse::SupervisorSetPausedOk { paused: true }).unwrap());
    }
}
