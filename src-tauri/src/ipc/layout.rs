//! Desktop bridge for the additive layout protocol. Conflicts carry the tree.
use super::error::IpcError;
use super::remote::RemoteGatewayManager;
use crate::daemon::layout::{Intent, Layout, View};
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use serde::Serialize;
use std::sync::Arc;
use tauri::{State, WebviewWindow};

#[tauri::command]
pub async fn cmd_script(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    action: crate::script::control::Action,
) -> Result<serde_json::Value, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Daemon unavailable"))?;
    match client.send_request(DaemonRequest::Script { action }).await? {
        DaemonResponse::ScriptOk { value } => Ok(value),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        response => Err(IpcError::internal(format!("Unexpected script response: {response:?}"))),
    }
}

#[tauri::command]
pub async fn cmd_plugins(
    manager: State<'_, Arc<RemoteGatewayManager>>,
    name: Option<String>,
    enabled: Option<bool>,
) -> Result<Vec<crate::daemon::plugins::registry::Status>, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Daemon unavailable"))?;
    let request = match (name, enabled) {
        (Some(name), Some(enabled)) => DaemonRequest::PluginSetEnabled { name, enabled },
        (None, None) => DaemonRequest::PluginsList,
        _ => return Err(IpcError::internal("Name and enabled must be supplied together")),
    };
    match client.send_request(request).await? {
        DaemonResponse::PluginsOk { plugins } => Ok(plugins),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        response => Err(IpcError::internal(format!("Unexpected plugin response: {response:?}"))),
    }
}

#[tauri::command]
pub async fn cmd_plugin_view(
    manager: State<'_, Arc<RemoteGatewayManager>>, plugin: String, view: String,
) -> Result<String, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Daemon unavailable"))?;
    match client.send_request(DaemonRequest::PluginView { plugin, view }).await? {
        DaemonResponse::PluginViewOk { document } => Ok(document),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        response => Err(IpcError::internal(format!("Unexpected view response: {response:?}"))),
    }
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LayoutReply {
    Ready { view: View },
    Conflict { code: String, revision: u64, layout: Layout },
    Unsupported,
}

#[tauri::command]
pub async fn cmd_panes(
    window: WebviewWindow,
    manager: State<'_, Arc<RemoteGatewayManager>>,
    request: crate::daemon::layout::PaneAction,
    scope: Option<String>,
) -> Result<crate::daemon::layout::DetachedRegistry, IpcError> {
    let request = match request {
        crate::daemon::layout::PaneAction::Attach { session, window: None, split } => crate::daemon::layout::PaneAction::Attach { session, window: Some(window.label().into()), split },
        request => request,
    };
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Layout daemon unavailable"))?;
    if let (Some(scope), crate::daemon::layout::PaneAction::Attach { session, split, .. }) = (scope, &request) {
        let current = client.send_request(DaemonRequest::LayoutGet { window: window.label().into() }).await?;
        let DaemonResponse::LayoutOk { view } = current else { return Err(IpcError::internal("Layout unavailable")); };
        match client.send_request(DaemonRequest::LayoutApply {
            window: window.label().into(), base_revision: view.revision,
            intent: Intent::Scoped { scope, intent: Box::new(Intent::AttachDetached { backend_session_id: session.clone(), target: crate::daemon::layout::AttachTarget { split: *split, ..Default::default() } }) },
        }).await? {
            DaemonResponse::LayoutOk { .. } => {},
            DaemonResponse::Error { message, .. } => return Err(IpcError::internal(message)),
            response => return Err(IpcError::internal(format!("Attach rejected: {response:?}"))),
        }
        return match client.send_request(DaemonRequest::Panes { request: crate::daemon::layout::PaneAction::List }).await? {
            DaemonResponse::PanesOk { detached } => Ok(detached),
            _ => Err(IpcError::internal("Inventory unavailable")),
        };
    }
    match client.send_request(DaemonRequest::Panes { request }).await? {
        DaemonResponse::PanesOk { detached } => Ok(detached),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        _ => Err(IpcError::internal("Unexpected pane response")),
    }
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
    scope: Option<String>,
) -> Result<LayoutReply, IpcError> {
    let Some(client) = manager.daemon_client() else { return Ok(LayoutReply::Unsupported); };
    match client.send_request(DaemonRequest::GetCapabilities).await? {
        DaemonResponse::CapabilitiesOk { capabilities }
            if capabilities.iter().any(|capability| capability == "daemonLayoutV1") => {}
        DaemonResponse::CapabilitiesOk { .. } => return Ok(LayoutReply::Unsupported),
        DaemonResponse::Error { code: None, .. } => return Ok(LayoutReply::Unsupported),
        response => return reply(response),
    }
    let request = match scope {
        Some(scope) => DaemonRequest::LayoutGetScoped { window: window.label().into(), scope },
        None => DaemonRequest::LayoutGet { window: window.label().into() },
    };
    reply(client.send_request(request).await?)
}

#[tauri::command]
pub async fn cmd_layout_import(
    app: tauri::AppHandle,
    window: WebviewWindow,
    manager: State<'_, Arc<RemoteGatewayManager>>,
    machine_id: String,
    revision: u64,
) -> Result<String, IpcError> {
    let client = manager.daemon_client().ok_or_else(|| IpcError::internal("Daemon unavailable"))?;
    match client.send_request(DaemonRequest::LayoutImport { window: window.label().into(), machine_id, revision }).await? {
        DaemonResponse::LayoutImported { window, .. } => super::window::build_app_window(&app, &window, None).await,
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        response => Err(IpcError::internal(format!("Unexpected layout import response: {response:?}"))),
    }
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
