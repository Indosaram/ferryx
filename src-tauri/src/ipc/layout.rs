//! Desktop bridge for the additive layout protocol. Conflicts carry the tree.
use super::error::IpcError;
use super::remote::RemoteGatewayManager;
use crate::daemon::layout::{Intent, Layout, View};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use serde::Serialize;
use std::sync::Arc;
use tauri::{State, WebviewWindow};

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LayoutReply {
    Ready { view: View },
    Conflict { code: String, revision: u64, layout: Layout },
    Unsupported,
}

fn reply(response: DaemonResponse) -> Result<LayoutReply, IpcError> {
    match response {
        DaemonResponse::LayoutOk { view } => Ok(LayoutReply::Ready { view }),
        DaemonResponse::LayoutConflict { code, revision, layout } => {
            Ok(LayoutReply::Conflict { code, revision, layout })
        }
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        _ => Err(IpcError::internal("Unexpected daemon layout response")),
    }
}

#[tauri::command]
pub async fn cmd_layout_get(
    window: WebviewWindow,
    manager: State<'_, Arc<RemoteGatewayManager>>,
) -> Result<LayoutReply, IpcError> {
    let Some(client) = manager.daemon_client() else { return Ok(LayoutReply::Unsupported); };
    match client.send_request(DaemonRequest::GetCapabilities).await? {
        DaemonResponse::CapabilitiesOk { capabilities }
            if capabilities.iter().any(|capability| capability == "daemonLayoutV1") => {}
        DaemonResponse::CapabilitiesOk { .. } => return Ok(LayoutReply::Unsupported),
        DaemonResponse::Error { code: None, .. } => return Ok(LayoutReply::Unsupported),
        response => return reply(response),
    }
    reply(client.send_request(DaemonRequest::LayoutGet { window: window.label().into() }).await?)
}

#[tauri::command]
pub async fn cmd_layout_apply(
    window: WebviewWindow,
    manager: State<'_, Arc<RemoteGatewayManager>>,
    base_revision: u64,
    intent: Intent,
) -> Result<LayoutReply, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Layout daemon unavailable"))?;
    reply(client.send_request(DaemonRequest::LayoutApply {
        window: window.label().into(), base_revision, intent,
    }).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflict_remains_structured_with_its_authoritative_layout() {
        let response = reply(DaemonResponse::LayoutConflict {
            code: "LAYOUT_CONFLICT".into(), revision: 7, layout: Layout::default(),
        }).unwrap();
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["type"], "conflict");
        assert_eq!(json["code"], "LAYOUT_CONFLICT");
        assert_eq!(json["revision"], 7);
        assert!(json["layout"]["tabs"].is_array());
    }
}
