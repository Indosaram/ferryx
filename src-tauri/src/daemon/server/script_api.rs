//! Owner-script adapter. Reuses public owner requests so every mutation crosses the pane gate.
use super::*;
use crate::daemon::{client::DaemonClient, protocol::{DaemonRequest as Q, DaemonResponse as R}};
use serde_json::{json, Value};

impl DaemonServer {
    pub(super) fn script_host(self: &Arc<Self>) -> crate::script::control::Host {
        let server = Arc::downgrade(self);
        let runtime = tokio::runtime::Handle::current();
        Arc::new(move |name, value| {
            let server = server.upgrade().ok_or("Daemon unavailable")?;
            runtime.block_on(server.script_effect(name, value))
        })
    }

    async fn script_effect(&self, name: &str, value: Value) -> Result<Value, String> {
        let client = DaemonClient::existing_only(std::time::Duration::from_secs(2));
        let session = value["session"].as_str().unwrap_or_default();
        match name {
            "send" => {
                client.write_terminal(session, value["text"].as_str().ok_or("BAD_ARGUMENT")?.as_bytes().to_vec()).await.map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "read" => {
                let screen = self.terminal_service.output_hub().visible_screen_text(session).ok_or("SESSION_NOT_FOUND")?;
                let count = value["lines"].as_u64().ok_or("BAD_ARGUMENT")? as usize;
                let lines: Vec<_> = screen.text.lines().collect();
                Ok(json!(lines[lines.len().saturating_sub(count)..].join("\n")))
            }
            "notify" => {
                self.remote_event_tx.send(DaemonRemoteEvent { event: "ScriptNotification".into(), payload: value })
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "layout" => {
                let intent = serde_json::from_value(value["intent"].clone()).map_err(|e| e.to_string())?;
                response(client.send_request(Q::LayoutApply { window: value["window"].as_str().ok_or("BAD_ARGUMENT")?.into(),
                    base_revision: value["revision"].as_u64().ok_or("BAD_ARGUMENT")?, intent }).await.map_err(|e| e.to_string())?)
            }
            "exec" => {
                let cwd = match client.describe_session(session).await.map_err(|e| e.to_string())? {
                    details => details.cwd.ok_or("Session has no local cwd")?,
                };
                let argv: Vec<String> = serde_json::from_value(value["argv"].clone()).map_err(|e| e.to_string())?;
                crate::ipc::run_blocking(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(crate::ipc::IpcError::internal)?;
                    runtime.block_on(async move {
                        let mut command = tokio::process::Command::new(&argv[0]);
                        command.args(&argv[1..]).current_dir(cwd).kill_on_drop(true);
                        let output = tokio::time::timeout(std::time::Duration::from_secs(30), command.output()).await
                            .map_err(crate::ipc::IpcError::internal)?.map_err(crate::ipc::IpcError::internal)?;
                        Ok(json!({"code":output.status.code(),"stdout":String::from_utf8_lossy(&output.stdout),"stderr":String::from_utf8_lossy(&output.stderr)}))
                    })
                }).await.map_err(|e| e.to_string())
            }
            "split" => {
                let details = client.describe_session(session).await.map_err(|e| e.to_string())?;
                let workspace_id = details.workspace_id.ok_or("Session has no workspace")?;
                let created = client.send_request(Q::Spawn { client_request_id: uuid::Uuid::new_v4().to_string(), workspace_id,
                    worktree: details.worktree, cwd: details.cwd, cols: details.cols, rows: details.rows,
                    shell: None, startup: None, local_split: None, session_id: None }).await.map_err(|e| e.to_string())?;
                let R::SpawnOk { session_id: new_session, .. } = created else { return response(created); };
                let target = {
                    let store = self.layouts.lock();
                    store.layouts.iter().find_map(|(window, layout)| layout.tabs.iter().find_map(|tab|
                        find(&tab.pane_tree, session).map(|node| (window.clone(), layout.revision, tab.tab_id.clone(), node))))
                }.ok_or("Session is not attached to a window")?;
                let direction = value["direction"].as_str().ok_or("BAD_ARGUMENT")?;
                let intent = crate::daemon::layout::Intent::SplitPane { tab_id: target.2, target_node_id: target.3,
                    dir: if ["right", "left"].contains(&direction) { crate::daemon::layout::Direction::Horizontal } else { crate::daemon::layout::Direction::Vertical },
                    first: ["left", "up"].contains(&direction), content: crate::daemon::layout::Content::Terminal { backend_session_id: new_session.clone() } };
                response(client.send_request(Q::LayoutApply { window: target.0, base_revision: target.1, intent }).await.map_err(|e| e.to_string())?)?;
                Ok(json!(new_session))
            }
            _ => Err("BAD_ARGUMENT".into()),
        }
    }
}
fn find(node: &crate::daemon::layout::Node, session: &str) -> Option<u64> {
    use crate::daemon::layout::{Node, Content};
    match node {
        Node::Leaf { node_id, content: Content::Terminal { backend_session_id } } if backend_session_id == session => Some(*node_id),
        Node::Split { a, b, .. } => find(a, session).or_else(|| find(b, session)),
        _ => None,
    }
}
fn response(value: R) -> Result<Value, String> {
    match value { R::Error { message, .. } => Err(message), other => serde_json::to_value(other).map_err(|e| e.to_string()) }
}
