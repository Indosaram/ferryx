//! Agent-state integration setting: the daemon owns the flag, and the desktop can also remove the
//! extension files Ferryx installed for the local user.

use super::error::{IpcError, IpcErrorCode};
use crate::daemon::agent_extension::RemovalReport;
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIntegrationStatus {
    pub enabled: bool,
}

fn daemon_upgrade_required() -> IpcError {
    IpcError::new(
        IpcErrorCode::DaemonUpgradeRequired,
        "The running Ferryx daemon predates this setting; it takes effect after restart",
    )
}

/// Maps the daemon reply. A daemon that knows these requests types every failure with a code, so
/// an untyped error is an older daemon that could not parse the request at all.
fn status_from(response: DaemonResponse) -> Result<AgentIntegrationStatus, IpcError> {
    match response {
        DaemonResponse::AgentIntegrationOk { enabled } => Ok(AgentIntegrationStatus { enabled }),
        DaemonResponse::Error { code: None, .. } => Err(daemon_upgrade_required()),
        DaemonResponse::Error { message, code: Some(code), details } => {
            let error = IpcError::new(IpcErrorCode::from_code_str(&code), message);
            Err(match details {
                Some(details) => error.with_details(details),
                None => error,
            })
        }
        _ => Err(IpcError::internal("Unexpected daemon agent integration response")),
    }
}

async fn request(
    daemon_client: &DaemonClient,
    request: DaemonRequest,
) -> Result<AgentIntegrationStatus, IpcError> {
    status_from(daemon_client.send_request(request).await?)
}

#[tauri::command]
pub async fn cmd_agent_integration_get(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
) -> Result<AgentIntegrationStatus, IpcError> {
    request(&daemon_client, DaemonRequest::GetAgentIntegration).await
}

#[tauri::command]
pub async fn cmd_agent_integration_set(
    daemon_client: tauri::State<'_, Arc<DaemonClient>>,
    enabled: bool,
) -> Result<AgentIntegrationStatus, IpcError> {
    request(&daemon_client, DaemonRequest::SetAgentIntegration { enabled }).await
}

#[tauri::command]
pub async fn cmd_agent_integration_remove_local() -> Result<RemovalReport, IpcError> {
    super::run_blocking(|| Ok(crate::daemon::agent_extension::remove_agent_state_extension()))
        .await
}

/// Whether the OS service manager owns the daemon's lifetime on this target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSupervisorStatus {
    pub supported: bool,
    pub enabled: bool,
}

#[tauri::command]
pub async fn cmd_daemon_supervisor_get() -> Result<DaemonSupervisorStatus, IpcError> {
    super::run_blocking(|| {
        let supported = crate::daemon::supervisor::supported();
        Ok(DaemonSupervisorStatus {
            supported,
            enabled: supported,
        })
    })
    .await
}

#[tauri::command]
pub async fn cmd_daemon_supervisor_set(enabled: bool) -> Result<DaemonSupervisorStatus, IpcError> {
    super::run_blocking(move || {
        if enabled {
            crate::daemon::supervisor::ensure().map(|_| ())
        } else {
            crate::daemon::supervisor::uninstall()
        }
        .map_err(|e| IpcError::internal(format!("daemon supervisor: {e}")))?;
        let supported = crate::daemon::supervisor::supported();
        Ok(DaemonSupervisorStatus {
            supported,
            enabled: supported && enabled,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_daemon_is_reported_as_upgrade_required() {
        // An older daemon cannot parse the request and answers with its untyped malformed-request
        // error; the message is never inspected.
        let rejected = status_from(DaemonResponse::Error {
            message: "anything".into(),
            code: None,
            details: None,
        })
        .unwrap_err();
        assert_eq!(rejected.code, IpcErrorCode::DaemonUpgradeRequired);
        assert_eq!(
            serde_json::to_value(&rejected).unwrap()["code"],
            "DAEMON_UPGRADE_REQUIRED"
        );
        // A current daemon's own failure keeps its typed code instead.
        let failed = status_from(DaemonResponse::Error {
            message: "disk full".into(),
            code: Some(crate::daemon::protocol::AGENT_INTEGRATION_ERROR_CODE.into()),
            details: None,
        })
        .unwrap_err();
        assert_eq!(
            failed.code,
            IpcErrorCode::Custom(crate::daemon::protocol::AGENT_INTEGRATION_ERROR_CODE.into())
        );
    }

    #[test]
    fn the_daemon_reply_maps_to_the_setting() {
        assert_eq!(
            status_from(DaemonResponse::AgentIntegrationOk { enabled: false }).unwrap(),
            AgentIntegrationStatus { enabled: false }
        );
        assert_eq!(
            status_from(DaemonResponse::Pong).unwrap_err().code,
            IpcErrorCode::InternalError
        );
    }
}
