//! Authenticated machine-scope DAG streaming.
//!
//! The journal lives on the machine that runs the agent, so the scan, the watch and
//! the parse happen here and the desktop receives push frames over an already
//! authenticated connection. No client-supplied filesystem path is ever accepted:
//! roots come from this daemon's own workspace catalog.
use super::{
    auth::{DeviceAccessScope, DevicePermission},
    state::RemoteGatewayState,
};
use crate::dag::journal::DagRunSnapshot;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Socket route a DAG ticket may be minted against.
pub const DAG_SOCKET_TARGET: &str = "/api/v1/workspace/dag";
/// Capability a host advertises once it can stream DAG frames.
pub const DAG_STREAM_CAPABILITY: &str = "dagStreamingV1";
/// Per-frame ceiling. An oversized snapshot is reported and the stream closed,
/// never truncated: a trimmed graph is indistinguishable from a corrupt one.
pub const MAX_DAG_FRAME_BYTES: usize = 512 * 1024;
/// Byte budget one inventory frame's snapshots may occupy. Fixed-count batching
/// cannot hold a frame under the ceiling, because snapshot size varies with node
/// prompts and diagnostics; a batch that overran it would fail every reconnect
/// identically and strand the workspace.
const INVENTORY_BATCH_BYTES: usize = MAX_DAG_FRAME_BYTES / 2;

/// Frames a DAG subscriber receives. `projectPath` is the root this machine
/// resolved, never a path the client asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DagFrame {
    #[serde(rename_all = "camelCase")]
    DagInventory {
        workspace_id: String,
        project_path: String,
        runs: Vec<DagRunSnapshot>,
    },
    #[serde(rename_all = "camelCase")]
    DagRunUpdated {
        workspace_id: String,
        project_path: String,
        snapshot: Box<DagRunSnapshot>,
    },
    #[serde(rename_all = "camelCase")]
    DagError { code: String },
}

/// Live host-side DAG streams; teardown is observable without polling.
fn live_streams() -> &'static tokio::sync::watch::Sender<usize> {
    static LIVE: std::sync::OnceLock<tokio::sync::watch::Sender<usize>> =
        std::sync::OnceLock::new();
    LIVE.get_or_init(|| tokio::sync::watch::channel(0).0)
}

/// Observes how many DAG streams this machine is serving.
pub fn active_stream_count() -> tokio::sync::watch::Receiver<usize> {
    live_streams().subscribe()
}

struct LiveStream;
impl LiveStream {
    fn enter() -> Self {
        live_streams().send_modify(|count| *count += 1);
        Self
    }
}
impl Drop for LiveStream {
    fn drop(&mut self) {
        live_streams().send_modify(|count| *count -= 1);
    }
}

/// Aborts the journal watcher however this stream ends, including when the future
/// is dropped by device revocation rather than returning.
struct WatcherGuard(tauri::async_runtime::JoinHandle<()>);
impl Drop for WatcherGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DagSocketQuery {
    workspace_id: String,
    ticket: Option<String>,
}

/// Upgrades an authenticated machine-scope client to a DAG stream. A ticket must
/// name this exact route; mirror or view-only devices are refused before lookup.
pub(super) async fn ws_dag_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<DagSocketQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = match query.ticket.as_deref() {
        Some(ticket) => super::server::consume_socket_ticket(&state, ticket, DAG_SOCKET_TARGET),
        None => super::server::extract_token(&headers),
    }
    .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    if device.access_scope != DeviceAccessScope::Machine
        || device.permission != DevicePermission::Control
    {
        return Err((StatusCode::FORBIDDEN, "MACHINE_ACCESS_REQUIRED".into()));
    }
    let mut revocation = state
        .auth_manager
        .device_revocation(&device.id)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;
    let workspace_id = query.workspace_id;
    Ok(ws.on_upgrade(move |socket| async move {
        tokio::select! {
            biased;
            _ = revocation.wait_for(|revoked| *revoked) => {},
            _ = serve(socket, state, workspace_id) => {},
        }
    }))
}

/// Sends one frame. An oversized frame is refused, not trimmed, and ends the stream.
async fn send(socket: &mut WebSocket, frame: &DagFrame) -> bool {
    let Ok(text) = serde_json::to_string(frame) else {
        let _ = fail(socket, "MACHINE_SERVICE_UNAVAILABLE").await;
        return false;
    };
    if text.len() > MAX_DAG_FRAME_BYTES {
        let _ = fail(socket, "PAYLOAD_TOO_LARGE").await;
        return false;
    }
    socket.send(Message::Text(text.into())).await.is_ok()
}

/// Reports a terminal condition and closes, so a client never mistakes a failed
/// hydration for an empty journal.
async fn fail(socket: &mut WebSocket, code: &str) {
    let frame = serde_json::to_string(&DagFrame::DagError { code: code.into() })
        .expect("error frame serializes");
    let _ = socket.send(Message::Text(frame.into())).await;
    let _ = socket.send(Message::Close(None)).await;
}

/// Splits the inventory into frames that each stay inside the byte budget.
///
/// A single snapshot larger than the budget gets its own frame: `send` then refuses
/// it explicitly if it also exceeds the hard ceiling, which is a reported error
/// rather than a silent omission.
fn inventory_batches(runs: Vec<DagRunSnapshot>) -> Vec<Vec<DagRunSnapshot>> {
    let mut batches: Vec<Vec<DagRunSnapshot>> = Vec::new();
    let mut current: Vec<DagRunSnapshot> = Vec::new();
    let mut used = 0usize;
    for run in runs {
        let size = serde_json::to_vec(&run)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        if !current.is_empty() && used + size > INVENTORY_BATCH_BYTES {
            batches.push(std::mem::take(&mut current));
            used = 0;
        }
        used += size;
        current.push(run);
    }
    // An empty journal still yields one frame, so the desktop can tell "hydrated,
    // nothing running" from "never hydrated".
    batches.push(current);
    batches
}

/// Streams one workspace's journal: inventory first, then live updates.
///
/// The inventory is resent on every subscription because the watcher only emits
/// snapshots it saw change; a reconnecting desktop would otherwise see nothing
/// until the next journal write.
pub(super) async fn serve(
    mut socket: WebSocket,
    state: Arc<RemoteGatewayState>,
    workspace_id: String,
) {
    use futures_util::StreamExt;

    let Some(services) = state.machine_services.as_ref() else {
        fail(&mut socket, "MACHINE_SERVICE_UNAVAILABLE").await;
        return;
    };
    let workspaces = services.workspaces.clone();
    let lookup = workspace_id.clone();
    let resolved = crate::ipc::run_blocking(move || {
        Ok(workspaces.catalog().map(|catalog| {
            catalog
                .workspaces
                .get(&lookup)
                .map(|row| row.repo_root.clone())
        }))
    })
    .await;
    let root = match resolved {
        Ok(Ok(Some(root))) => root,
        Ok(Ok(None)) => {
            fail(&mut socket, "PROJECT_NOT_FOUND").await;
            return;
        }
        _ => {
            fail(&mut socket, "MACHINE_SERVICE_UNAVAILABLE").await;
            return;
        }
    };

    let _live = LiveStream::enter();
    let project_path = root.to_string_lossy().into_owned();
    let scan_root = root.clone();
    let Ok(runs) = crate::ipc::run_blocking(move || {
        Ok(crate::daemon::dag_service::scan_project_inventory(
            &scan_root,
        ))
    })
    .await
    else {
        fail(&mut socket, "MACHINE_SERVICE_UNAVAILABLE").await;
        return;
    };

    for batch in inventory_batches(runs) {
        if !send(
            &mut socket,
            &DagFrame::DagInventory {
                workspace_id: workspace_id.clone(),
                project_path: project_path.clone(),
                runs: batch,
            },
        )
        .await
        {
            return;
        }
    }

    let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(64);
    let _watcher = WatcherGuard(crate::dag::watcher::spawn_dag_watcher(root, events_tx));
    loop {
        tokio::select! {
            incoming = socket.next() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
            event = events_rx.recv() => match event {
                Some((_, snapshot)) => {
                    if !send(
                        &mut socket,
                        &DagFrame::DagRunUpdated {
                            workspace_id: workspace_id.clone(),
                            project_path: project_path.clone(),
                            snapshot: Box::new(snapshot),
                        },
                    )
                    .await
                    {
                        break;
                    }
                }
                None => break,
            },
        }
    }
}
