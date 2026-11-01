//! Settings commands for the daemon-owned remote desktop host. The daemon holds the host, its
//! pairings and its consent queue; these commands only relay to it. Consent prompts arrive as the
//! daemon remote event `rd-host-consent` `{requestId, clientName}`, which the GUI's remote event
//! bridge re-emits as a Tauri event of the same name.

use super::error::{IpcError, IpcErrorCode};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse, RdHostState};
use std::sync::Arc;
use tauri::State;

/// Maps a host reply. A daemon that knows these requests types every failure, so an untyped
/// error comes from a daemon that predates remote desktop.
fn state_from(response: DaemonResponse) -> Result<RdHostState, IpcError> {
    match response {
        DaemonResponse::RemoteDesktopHostState { state } => Ok(state),
        DaemonResponse::Error {
            message,
            code: Some(code),
            details,
        } => {
            let error = IpcError::new(IpcErrorCode::from_code_str(&code), message);
            Err(match details {
                Some(details) => error.with_details(details),
                None => error,
            })
        }
        DaemonResponse::Error { .. } => Err(IpcError::new(
            IpcErrorCode::Custom("RD_UNSUPPORTED".into()),
            "The running Ferryx daemon predates remote desktop; it appears after the daemon restarts",
        )),
        _ => Err(IpcError::internal("Unexpected daemon remote desktop response")),
    }
}

#[tauri::command]
pub async fn cmd_rd_host_state(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<RdHostState, IpcError> {
    state_from(daemon.send_request(DaemonRequest::RemoteDesktopHostGet).await?)
}

#[tauri::command]
pub async fn cmd_rd_host_set(
    daemon: State<'_, Arc<DaemonClient>>,
    enabled: bool,
    allow_input: bool,
    allow_clipboard: bool,
) -> Result<RdHostState, IpcError> {
    state_from(
        daemon
            .send_request(DaemonRequest::RemoteDesktopHostSet {
                enabled,
                allow_input,
                allow_clipboard,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn cmd_rd_host_respond(
    daemon: State<'_, Arc<DaemonClient>>,
    request_id: String,
    approve: bool,
) -> Result<(), IpcError> {
    state_from(
        daemon
            .send_request(DaemonRequest::RemoteDesktopHostRespond {
                request_id,
                approve,
            })
            .await?,
    )
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_replies_map_to_state_or_typed_errors() {
        let state = RdHostState {
            enabled: true,
            running: true,
            allow_input: true,
            allow_clipboard: false,
            sessions: Vec::new(),
            pending: Vec::new(),
            error: None,
        };
        let ok = DaemonResponse::RemoteDesktopHostState { state: state.clone() };
        assert_eq!(state_from(ok).unwrap(), state);
        let typed = DaemonResponse::Error {
            message: "off".into(),
            code: Some("RD_HOST_DISABLED".into()),
            details: None,
        };
        assert_eq!(
            state_from(typed).unwrap_err().code,
            IpcErrorCode::Custom("RD_HOST_DISABLED".into())
        );
        let old = DaemonResponse::Error {
            message: "Malformed request".into(),
            code: None,
            details: None,
        };
        assert_eq!(
            state_from(old).unwrap_err().code,
            IpcErrorCode::Custom("RD_UNSUPPORTED".into())
        );
    }

    #[test]
    fn host_state_serializes_camel_case() {
        let state = RdHostState {
            enabled: false,
            running: false,
            allow_input: true,
            allow_clipboard: true,
            sessions: vec![crate::daemon::protocol::RdHostSession {
                session_id: "s".into(),
                client_name: "Laptop".into(),
            }],
            pending: vec![crate::daemon::protocol::RdHostPending {
                request_id: "r".into(),
                client_name: "Phone".into(),
            }],
            error: None,
        };
        let value = serde_json::to_value(state).unwrap();
        assert_eq!(value["allowInput"], true);
        assert_eq!(value["sessions"][0]["sessionId"], "s");
        assert_eq!(value["pending"][0]["requestId"], "r");
        assert!(value["error"].is_null());
    }
}
