//! The desktop chat lens over one pane. The daemon owns the panes and the reference-chat input
//! queue. Reads reuse its connection; mutations use an independent one and need no network listener.

use std::sync::Arc;

use tauri::State;

use crate::daemon::protocol::{
    DaemonRequest, DaemonResponse, DesktopReferenceChatIdentity, DesktopReferenceChatReply,
    DesktopReferenceChatRequest,
};
use crate::daemon::DaemonClient;
use crate::ipc::{IpcError, IpcErrorCode};

fn daemon_failure(response: DaemonResponse) -> IpcError {
    match response {
        DaemonResponse::Error { message, code: Some(code), details } => {
            let error = IpcError::new(IpcErrorCode::from_code_str(&code), message);
            match details {
                Some(details) => error.with_details(details),
                None => error,
            }
        }
        DaemonResponse::Error { message, .. } => IpcError::internal(message),
        _ => IpcError::internal("Unexpected daemon reference-chat response"),
    }
}

pub(crate) fn identity_from(response: DaemonResponse) -> Result<DesktopReferenceChatIdentity, IpcError> {
    match response {
        DaemonResponse::ReferenceChatIdentityOk { identity } => Ok(identity),
        other => Err(daemon_failure(other)),
    }
}

pub(crate) fn reply_from(response: DaemonResponse) -> Result<DesktopReferenceChatReply, IpcError> {
    match response {
        DaemonResponse::ReferenceChatOk { reply } => Ok(reply),
        other => Err(daemon_failure(other)),
    }
}

#[tauri::command]
pub async fn cmd_desktop_reference_chat_identity(
    daemon: State<'_, Arc<DaemonClient>>,
) -> Result<DesktopReferenceChatIdentity, IpcError> {
    identity_from(daemon.send_request_one_shot(DaemonRequest::ReferenceChatIdentity).await?)
}

#[tauri::command]
pub async fn cmd_desktop_reference_chat(
    daemon: State<'_, Arc<DaemonClient>>,
    request: DesktopReferenceChatRequest,
) -> Result<DesktopReferenceChatReply, IpcError> {
    let is_read = request.kind.is_read();
    let request = DaemonRequest::ReferenceChat { request };
    reply_from(if is_read {
        daemon.send_request(request).await?
    } else {
        daemon.send_request_one_shot(request).await?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_map_and_untyped_errors_preserve_the_daemon_failure() {
        let reply = DesktopReferenceChatReply { status: 200, body: serde_json::json!({"ok": true}) };
        assert_eq!(reply_from(DaemonResponse::ReferenceChatOk { reply: reply.clone() }).unwrap(), reply);
        let old = reply_from(DaemonResponse::Error {
            message: "Malformed request: unknown variant".into(),
            code: None,
            details: None,
        })
        .unwrap_err();
        assert_eq!(old.code, IpcErrorCode::InternalError);
        assert_eq!(old.message, "Malformed request: unknown variant");
        let identity = DesktopReferenceChatIdentity {
            host_id: "local".into(),
            owner_id: "o".into(),
            epoch: "7".into(),
        };
        assert_eq!(
            identity_from(DaemonResponse::ReferenceChatIdentityOk { identity: identity.clone() }).unwrap(),
            identity
        );
    }
}
