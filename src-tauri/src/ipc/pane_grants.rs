//! Settings > Pane access: the owner's cross-pane grants (rex todo 5). The desktop app is a peer
//! outside every pane, so its `PaneGrant` requests are owner requests on the daemon socket.

use super::error::{IpcError, IpcErrorCode};
use super::remote::RemoteGatewayManager;
use crate::daemon::pane_grants::{GrantOp, PaneGrant, PaneGrantAction};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use std::sync::Arc;
use tauri::State;

fn grants_from(response: DaemonResponse) -> Result<Vec<PaneGrant>, IpcError> {
    match response {
        DaemonResponse::PaneGrantsOk { grants, .. } => Ok(grants),
        DaemonResponse::Error { message, code: Some(code), .. } => {
            Err(IpcError::new(IpcErrorCode::from_code_str(&code), message))
        }
        DaemonResponse::Error { .. } => Err(IpcError::new(
            IpcErrorCode::DaemonUpgradeRequired,
            "The running Ferryx daemon predates pane access grants; they appear after the daemon restarts",
        )),
        _ => Err(IpcError::internal("Unexpected daemon pane grant response")),
    }
}

async fn send(manager: &RemoteGatewayManager, request: PaneGrantAction) -> Result<Vec<PaneGrant>, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| {
        IpcError::new(IpcErrorCode::DaemonUpgradeRequired, "Pane access needs the Ferryx daemon")
    })?;
    grants_from(client.send_request(DaemonRequest::PaneGrant { request }).await?)
}

#[tauri::command]
pub async fn cmd_pane_grants_list(
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<Vec<PaneGrant>, IpcError> {
    send(&manager, PaneGrantAction::List).await
}

#[tauri::command]
pub async fn cmd_pane_grants_add(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    from: String,
    to: String,
    ops: Vec<GrantOp>,
) -> Result<Vec<PaneGrant>, IpcError> {
    let grant = PaneGrant { from, to, ops: ops.into_iter().collect() };
    send(&manager, PaneGrantAction::Add { grant }).await
}

#[tauri::command]
pub async fn cmd_pane_grants_remove(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    from: String,
    to: String,
) -> Result<Vec<PaneGrant>, IpcError> {
    send(&manager, PaneGrantAction::Remove { from, to }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untyped_daemon_error_means_the_daemon_is_older() {
        let old = DaemonResponse::Error { message: "Malformed request".into(), code: None, details: None };
        assert_eq!(grants_from(old).unwrap_err().code, IpcErrorCode::DaemonUpgradeRequired);
        let ok = DaemonResponse::PaneGrantsOk { grants: Vec::new(), admin: None };
        assert!(grants_from(ok).unwrap().is_empty());
    }
}
