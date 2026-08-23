use crate::remote::auth::{AuthError, DeviceInfo, DevicePermission};
use crate::remote::protocol::{
    ClientControlMessage, RemoteCreateWorktreeRequest, RemoteDeleteWorktreeRequest,
    RemoteProjectInfo, RemoteSelectWorkspaceRequest, RemoteSelectionRequestPayload,
    RemoteTerminalSession, RemoteWorkspaceState,
};
use crate::remote::state::{RemoteGatewayState, RemoteNetworkMode};
use crate::terminal::{PtySessionState, TerminalSignal};
use crate::worktree::{CreateWorktreeOptions, WorktreeIdentity};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path as AxumPath, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

pub const REMOTE_SELECTION_REQUEST_EVENT: &str = "remote_selection_requested";

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairExchangeRequest {
    code: String,
    device_name: String,
}

#[derive(Serialize)]
struct PairExchangeResponse {
    token: String,
    device: DeviceInfo,
}

#[derive(Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

fn extract_token(headers: &HeaderMap, query: Option<&AuthQuery>) -> Option<String> {
    if let Some(auth_header) = headers.get("authorization").and_then(|h| h.to_str().ok()) {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            return Some(token.trim().to_string());
        }
    }
    query.and_then(|q| q.token.clone())
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: "0.1.0",
    })
}

async fn pair_exchange(
    State(state): State<Arc<RemoteGatewayState>>,
    Json(payload): Json<PairExchangeRequest>,
) -> Result<Json<PairExchangeResponse>, (StatusCode, String)> {
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code(&payload.code, &payload.device_name)
        .map_err(|e| match e {
            AuthError::InvalidPairingCode => {
                (StatusCode::BAD_REQUEST, "Invalid pairing code".into())
            }
            AuthError::ExpiredPairingCode => {
                (StatusCode::UNAUTHORIZED, "Pairing code expired".into())
            }
            _ => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
        })?;

    Ok(Json(PairExchangeResponse { token, device }))
}

/// Canonicalize a filesystem path for comparison purposes. When the path itself
/// doesn't exist (e.g. a session whose worktree was deleted after the session was
/// spawned), canonicalizes the nearest existing ancestor instead and re-appends
/// the missing tail, so the result still resolves symlinks (notably macOS's
/// `/var` -> `/private/var`) in the part of the path that *does* exist. This
/// keeps `starts_with` comparisons against a canonical repo root correct even for
/// non-existent paths, rather than silently falling back to a raw path that may
/// use a different symlink alias than the canonical root it's compared against.
fn canonicalize_or_raw(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let mut missing_tail = Vec::new();
    let mut ancestor = path;
    loop {
        match ancestor.parent() {
            Some(parent) => {
                missing_tail.push(ancestor.file_name());
                ancestor = parent;
            }
            None => break,
        }
        if let Ok(canonical_ancestor) = std::fs::canonicalize(ancestor) {
            let mut resolved = canonical_ancestor;
            for component in missing_tail.into_iter().rev().flatten() {
                resolved.push(component);
            }
            return resolved;
        }
    }
    path.to_path_buf()
}

/// Snapshot of one registered workspace, captured once per request so repeated
/// session lookups don't each re-list git worktrees from disk.
struct WorkspaceSnapshot {
    workspace_id: String,
    /// Canonicalized repo root (the manager's root is already canonical at
    /// construction time, but we re-derive defensively in case the directory
    /// was removed or replaced by a symlink after registration).
    root: PathBuf,
    repo_root_display: String,
    worktrees: Vec<crate::worktree::Worktree>,
}

/// Request-scoped cache of the workspace registry's contents. Building this once
/// per request (rather than once per session) avoids repeated `git worktree list`
/// subprocess invocations when a request needs metadata for many sessions.
pub(crate) struct WorkspaceSnapshotCache {
    workspaces: Vec<WorkspaceSnapshot>,
}

impl WorkspaceSnapshotCache {
    pub(crate) fn build(registry: &crate::worktree::WorkspaceRegistry) -> Self {
        let mut entries = registry.list();
        // Deterministic order: `WorkspaceRegistry::list` iterates a `HashMap`, whose
        // order is unspecified and can vary between calls.
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        let workspaces = entries
            .into_iter()
            .map(|(workspace_id, mgr)| WorkspaceSnapshot {
                workspace_id,
                root: canonicalize_or_raw(mgr.repo_root()),
                repo_root_display: mgr.repo_root().to_string_lossy().into_owned(),
                worktrees: mgr.list_worktrees().unwrap_or_default(),
            })
            .collect();
        Self { workspaces }
    }

    /// Workspace ids and repo roots in deterministic order, for building the
    /// `projects` list without a second registry/disk round trip.
    fn projects(&self) -> Vec<RemoteProjectInfo> {
        self.workspaces
            .iter()
            .map(|w| RemoteProjectInfo {
                workspace_id: w.workspace_id.clone(),
                repo_root: w.repo_root_display.clone(),
            })
            .collect()
    }

    /// Previously listed worktrees for `workspace_id`, if registered. Reuses the
    /// snapshot captured at cache-build time instead of re-listing from disk.
    fn worktrees_for(&self, workspace_id: &str) -> Vec<crate::worktree::Worktree> {
        self.workspaces
            .iter()
            .find(|w| w.workspace_id == workspace_id)
            .map(|w| w.worktrees.clone())
            .unwrap_or_default()
    }

    /// Resolve `(workspace_id, worktree_label)` for a terminal session from the
    /// worktree path that owns it. Degrades gracefully: unmatched paths yield a
    /// best-effort label derived from the path itself, missing paths yield `None`.
    pub(crate) fn derive_session_metadata(
        &self,
        worktree_path: Option<&Path>,
    ) -> (Option<String>, Option<String>) {
        let Some(path) = worktree_path else {
            return (None, None);
        };
        let canonical = canonicalize_or_raw(path);
        for workspace in &self.workspaces {
            if !canonical.starts_with(&workspace.root) {
                continue;
            }
            let label = workspace
                .worktrees
                .iter()
                .find(|wt| wt.path == path || wt.path == canonical)
                .and_then(|wt| wt.branch_short_name().map(str::to_string))
                .or_else(|| relative_label(&workspace.root, &canonical));
            return (Some(workspace.workspace_id.clone()), label);
        }
        (
            None,
            path.file_name().map(|f| f.to_string_lossy().into_owned()),
        )
    }
}

/// Best-effort, non-panicking label for a path that lives under `root` but isn't a
/// listed git worktree (e.g. an ad-hoc subdirectory terminal). Prefers the path
/// relative to the workspace root so nested/ad-hoc sessions get an informative,
/// collision-resistant label instead of a bare directory name; falls back to the
/// root's own directory name when the path *is* the root.
fn relative_label(root: &Path, canonical: &Path) -> Option<String> {
    if let Ok(rel) = canonical.strip_prefix(root) {
        if !rel.as_os_str().is_empty() {
            return Some(
                rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
        }
    }
    canonical
        .file_name()
        .or_else(|| root.file_name())
        .map(|f| f.to_string_lossy().into_owned())
}

fn get_active_running_sessions(
    state: &RemoteGatewayState,
    cache: &WorkspaceSnapshotCache,
) -> Vec<RemoteTerminalSession> {
    let active = state.active_selection.read().clone();
    let Some(active_sel) = active else {
        return Vec::new();
    };
    let Some(session_id) = active_sel.session_id.as_deref() else {
        return Vec::new();
    };
    let Some(session) = state.terminal_service.get_session(session_id) else {
        return Vec::new();
    };
    let running = matches!(
        session.state(),
        PtySessionState::Running | PtySessionState::Starting
    );
    if !running {
        return Vec::new();
    }
    let (derived_ws, derived_label) =
        cache.derive_session_metadata(session.worktree_path().as_deref());
    let workspace_id = active_sel.workspace_id.clone().or(derived_ws);
    let worktree_label = active_sel
        .worktree_slug
        .clone()
        .or_else(|| active_sel.worktree_label.clone())
        .or(derived_label);

    vec![RemoteTerminalSession {
        session_id: session.id().to_string(),
        title: None,
        workspace_id,
        worktree_label,
        running,
    }]
}

async fn list_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<Vec<RemoteTerminalSession>>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let cache = WorkspaceSnapshotCache::build(&state.workspace_registry);
    let sessions = get_active_running_sessions(&state, &cache);

    Ok(Json(sessions))
}

async fn get_workspace_state(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<RemoteWorkspaceState>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    // Build the request-scoped snapshot once: it drives `projects`, the active
    // workspace's `worktrees`, and every session's derived metadata below, so a
    // request with many sessions only lists each workspace's git worktrees once.
    let cache = WorkspaceSnapshotCache::build(&state.workspace_registry);

    let projects = cache.projects();
    let active_ws = state
        .active_selection
        .read()
        .as_ref()
        .and_then(|sel| sel.workspace_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| projects.first().map(|p| p.workspace_id.clone()))
        .unwrap_or_else(|| "default".into());
    let worktrees = cache.worktrees_for(&active_ws);
    let sessions = get_active_running_sessions(&state, &cache);

    Ok(Json(RemoteWorkspaceState {
        projects,
        active_workspace_id: active_ws,
        worktrees,
        sessions,
    }))
}

async fn select_workspace(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    Json(payload): Json<RemoteSelectWorkspaceRequest>,
) -> Result<Json<RemoteSelectionRequestPayload>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot request workspace selection".into(),
        ));
    }

    let _mgr = state
        .workspace_registry
        .manager(&payload.workspace_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let (worktree_identity, worktree_slug, worktree_label) = if let Some(ref wt) = payload.worktree
    {
        let (_, resolved_wt) = state
            .workspace_registry
            .resolve_worktree(&payload.workspace_id, wt)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let label = resolved_wt
            .branch_short_name()
            .map(str::to_string)
            .or_else(|| payload.worktree_label.clone());
        (Some(wt.clone()), Some(wt.slug.clone()), label)
    } else if let Some(ref slug) = payload.worktree_slug {
        let ident = WorktreeIdentity {
            ws_id: payload.workspace_id.clone(),
            slug: slug.clone(),
        };
        let (_, resolved_wt) = state
            .workspace_registry
            .resolve_worktree(&payload.workspace_id, &ident)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let label = resolved_wt
            .branch_short_name()
            .map(str::to_string)
            .or_else(|| payload.worktree_label.clone());
        (Some(ident), Some(slug.clone()), label)
    } else {
        (None, None, payload.worktree_label.clone())
    };

    let event_payload = RemoteSelectionRequestPayload {
        workspace_id: payload.workspace_id,
        worktree: worktree_identity,
        worktree_slug,
        worktree_label,
        session_id: payload.session_id,
    };

    state.emit_desktop_event(
        REMOTE_SELECTION_REQUEST_EVENT,
        serde_json::to_value(&event_payload).unwrap_or(serde_json::Value::Null),
    );

    Ok(Json(event_payload))
}

async fn create_worktree(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    Json(payload): Json<RemoteCreateWorktreeRequest>,
) -> Result<Json<crate::worktree::Worktree>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot create worktrees".into(),
        ));
    }

    let mgr = state
        .workspace_registry
        .manager(&payload.workspace_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let path = mgr
        .worktree_path_for(&payload.worktree.ws_id, &payload.worktree.slug)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let options = CreateWorktreeOptions {
        ws_id: payload.worktree.ws_id,
        slug: payload.worktree.slug,
        path,
        base_ref: payload.base_ref,
    };

    let created = mgr
        .create_worktree(options)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(created))
}

async fn delete_worktree(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    Json(payload): Json<RemoteDeleteWorktreeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((
            StatusCode::FORBIDDEN,
            "View-only device cannot delete worktrees".into(),
        ));
    }

    let (mgr, worktree) = state
        .workspace_registry
        .resolve_worktree(&payload.workspace_id, &payload.worktree)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    mgr.delete_worktree_and_branch(&worktree.path, payload.delete_branch.unwrap_or(false))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_devices(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<Vec<DeviceInfo>>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    Ok(Json(state.auth_manager.list_devices()))
}

async fn revoke_device(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    AxumPath(device_id): AxumPath<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if state.auth_manager.revoke_device(&device_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Device not found".into()))
    }
}

async fn ws_events_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let rx = state.event_tx.subscribe();
    Ok(ws.on_upgrade(move |socket| handle_events_socket(socket, rx)))
}

async fn handle_events_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<String>) {
    while let Ok(msg) = rx.recv().await {
        if socket.send(Message::Text(msg.into())).await.is_err() {
            break;
        }
    }
}

async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let is_declared_active = {
        let active = state.active_selection.read();
        active
            .as_ref()
            .and_then(|a| a.session_id.as_deref())
            .map(|id| id == session_id.as_str())
            .unwrap_or(false)
    };

    if !is_declared_active {
        return Err((
            StatusCode::FORBIDDEN,
            "Forbidden: session is not the active desktop session".into(),
        ));
    }

    let (history, rx) = state
        .terminal_service
        .attach(&session_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Session not found".into()))?;

    Ok(ws.on_upgrade(move |socket| {
        handle_terminal_socket(socket, session_id, history, rx, device, state)
    }))
}

async fn handle_terminal_socket(
    socket: WebSocket,
    session_id: String,
    history: Vec<u8>,
    mut rx: broadcast::Receiver<Vec<u8>>,
    device: DeviceInfo,
    state: Arc<RemoteGatewayState>,
) {
    let (mut sender, mut receiver) = socket.split();

    if !history.is_empty() && sender.send(Message::Binary(history.into())).await.is_err() {
        return;
    }

    let mut send_task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(chunk) => {
                    if sender.send(Message::Binary(chunk.into())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let term_service = Arc::clone(&state.terminal_service);
    let session_id_clone = session_id.clone();
    let can_control = device.permission == DevicePermission::Control;

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Binary(bytes) => {
                    if can_control {
                        let _ = term_service.write_input(&session_id_clone, &bytes);
                    }
                }
                Message::Text(text) => {
                    if let Ok(ctrl) = serde_json::from_str::<ClientControlMessage>(&text) {
                        match ctrl {
                            ClientControlMessage::Resize { cols, rows } => {
                                let _ = term_service.resize(&session_id_clone, cols, rows);
                            }
                            ClientControlMessage::Signal { signal } => {
                                if can_control && signal == "interrupt" {
                                    let _ = term_service
                                        .signal(&session_id_clone, TerminalSignal::Interrupt);
                                }
                            }
                            ClientControlMessage::Ping => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let mut active_session_rx = state.active_session_watch_rx();
    let target_session_id = session_id.clone();
    let mut focus_watcher = tokio::spawn(async move {
        if active_session_rx.borrow().as_deref() != Some(target_session_id.as_str()) {
            return;
        }
        while active_session_rx.changed().await.is_ok() {
            let current = active_session_rx.borrow().clone();
            if current.as_deref() != Some(target_session_id.as_str()) {
                break;
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
            focus_watcher.abort();
        }
        _ = (&mut recv_task) => {
            send_task.abort();
            focus_watcher.abort();
        }
        _ = (&mut focus_watcher) => {
            send_task.abort();
            recv_task.abort();
        }
    };
}

fn resolve_dist_dir() -> PathBuf {
    let mut candidates = vec![
        PathBuf::from("ui/dist"),
        PathBuf::from("../ui/dist"),
        PathBuf::from("../../ui/dist"),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("ui/dist"));
        candidates.push(cwd.join("dist"));
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe;
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                candidates.push(parent.join("ui/dist"));
                candidates.push(parent.join("Resources/ui/dist"));
                candidates.push(parent.join("../Resources/ui/dist"));
                candidates.push(parent.join("../Resources"));
                cur = parent.to_path_buf();
            }
        }
    }
    for c in &candidates {
        if c.exists() && c.is_dir() && c.join("index.html").exists() {
            return c.clone();
        }
    }
    PathBuf::from("ui/dist")
}

async fn serve_static_or_index(uri: axum::http::Uri) -> Response {
    let dist_dir = resolve_dist_dir();
    let path = uri.path().trim_start_matches('/');
    let file_path = dist_dir.join(path);

    if !path.is_empty() && file_path.exists() && file_path.is_file() {
        if let Ok(bytes) = tokio::fs::read(&file_path).await {
            let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], bytes).into_response();
        }
    }

    // SPA fallback: serve index.html from dist_dir
    let index_file = dist_dir.join("index.html");
    if index_file.exists() && index_file.is_file() {
        if let Ok(html) = tokio::fs::read_to_string(&index_file).await {
            return Html(html).into_response();
        }
    }

    Html(EMBEDDED_FALLBACK_HTML).into_response()
}

const EMBEDDED_FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"/><title>Ferryx Remote</title></head>
<body style="background:#09090b;color:#fafafa;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;">
  <div style="text-align:center;">
    <h2>Ferryx Remote Server Active</h2>
    <p style="color:#a1a1aa;font-size:14px;">Building the UI bundle or connect through the Ferryx desktop app.</p>
  </div>
</body>
</html>"#;

async fn get_terminal_preferences(
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<crate::terminal::TerminalPreferences>, (StatusCode, String)> {
    let _token = extract_token(&headers, Some(&query))
        .ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    Ok(Json(crate::terminal::load_terminal_preferences()))
}

pub fn create_remote_router(state: Arc<RemoteGatewayState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/pair/exchange", post(pair_exchange))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/workspace/state", get(get_workspace_state))
        .route("/api/v1/workspace/select", post(select_workspace))
        .route("/api/v1/workspace/selection", post(select_workspace))
        .route(
            "/api/v1/terminal/preferences",
            get(get_terminal_preferences),
        )
        .route(
            "/api/v1/workspace/worktrees",
            post(create_worktree).delete(delete_worktree),
        )
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}/revoke", post(revoke_device))
        .route("/api/v1/events", get(ws_events_handler))
        .route("/api/v1/terminal/{sessionId}", get(ws_terminal_handler))
        .fallback(get(serve_static_or_index))
        .layer(cors)
        .with_state(state)
}

pub struct RemoteServerHandle {
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl RemoteServerHandle {
    pub fn stop(self) {
        let _ = self.shutdown_tx.send(());
    }
}

pub async fn start_remote_server(
    state: Arc<RemoteGatewayState>,
) -> Result<(RemoteServerHandle, SocketAddr), String> {
    let config = state.config.read().clone();
    let bind_host = match config.mode {
        RemoteNetworkMode::Off => return Err("Remote gateway is OFF".into()),
        _ => "0.0.0.0",
    };

    let bind_addr: SocketAddr = format!("{bind_host}:{}", config.port)
        .parse()
        .map_err(|e| format!("Invalid bind address: {e}"))?;

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind to {bind_addr}: {e}"))?;

    let local_addr = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local address: {e}"))?;

    let router = create_remote_router(Arc::clone(&state));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    *state.is_running.write() = true;
    *state.bound_address.write() = Some(local_addr.to_string());

    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();

        *state_clone.is_running.write() = false;
        *state_clone.bound_address.write() = None;
    });

    Ok((RemoteServerHandle { shutdown_tx }, local_addr))
}
