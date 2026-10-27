//! The desktop's Supervisor view: a snapshot of what the hub supervisor drives and watches, and
//! the pause a person sets on one lane. Both go to the daemon, which owns the hub.

use super::error::{IpcError, IpcErrorCode};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, SupervisorGoalAction};
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

fn goal_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorGoalOk { snapshot } => Ok(snapshot),
        DaemonResponse::Error { message, code, .. } => Err(failure(message, code)),
        _ => Err(IpcError::internal("Unexpected daemon supervisor goal response")),
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

/// Show or steer one lane's goal contract; returns the supervisor snapshot afterwards.
#[tauri::command]
pub async fn cmd_supervisor_goal(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    request: SupervisorGoalAction,
) -> Result<serde_json::Value, IpcError> {
    goal_from(daemon_client.send_request(DaemonRequest::SupervisorGoal { request }).await?)
}

fn receipt_from(response: DaemonResponse) -> Result<serde_json::Value, IpcError> {
    match response {
        DaemonResponse::SupervisorRequestV1Ok { receipt } => Ok(receipt),
        DaemonResponse::Error { message, code, details } => {
            let mut error = failure(message, code);
            if details.is_some() {
                error.details = details;
            }
            Err(error)
        }
        _ => Err(IpcError::internal("Unexpected daemon supervisor request response")),
    }
}

/// One unified supervisor request envelope; the daemon derives the actor.
#[tauri::command]
pub async fn cmd_supervisor_request(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    envelope: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    receipt_from(
        daemon_client
            .send_request(DaemonRequest::SupervisorRequestV1 { envelope, cli: false })
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

    #[test]
    fn a_goal_reply_carries_the_snapshot_and_a_conflict_stays_typed() {
        let ok = DaemonResponse::SupervisorGoalOk { snapshot: serde_json::json!({"lanes": []}) };
        assert_eq!(goal_from(ok).unwrap(), serde_json::json!({"lanes": []}));
        let conflict = DaemonResponse::Error {
            message: "lane ledger has no proposal awaiting approval".into(),
            code: Some("REQUEST_CONFLICT".into()),
            details: None,
        };
        assert_eq!(goal_from(conflict).unwrap_err().code, IpcErrorCode::RequestConflict);
        let request: SupervisorGoalAction =
            serde_json::from_value(serde_json::json!({"action": "approve", "laneKey": "ledger"})).unwrap();
        assert_eq!(request, SupervisorGoalAction::Approve { lane_key: "ledger".into() });
    }

    #[test]
    fn a_supervisor_request_reply_carries_the_receipt_and_a_refusal_keeps_its_details() {
        let ok = DaemonResponse::SupervisorRequestV1Ok { receipt: serde_json::json!({"goalId": "g", "revision": 2}) };
        assert_eq!(receipt_from(ok).unwrap()["revision"], 2);
        let stale = DaemonResponse::Error {
            message: "goal g revision changed".into(),
            code: Some("REVISION_CONFLICT".into()),
            details: Some(serde_json::json!({"code": "REVISION_CONFLICT", "status": 409})),
        };
        let error = receipt_from(stale).unwrap_err();
        assert_ne!(error.code, IpcErrorCode::DaemonUpgradeRequired);
        assert_eq!(error.details.unwrap()["status"], 409);
    }
}
