use crate::remote::auth::{AuthError, DeviceInfo, DevicePermission};
use crate::remote::protocol::{
    ClientControlMessage, RemoteCreateWorktreeRequest, RemoteDeleteWorktreeRequest,
    RemoteProjectInfo, RemoteTerminalSession, RemoteWorkspaceState,
};
use crate::remote::state::{RemoteGatewayState, RemoteNetworkMode};
use crate::terminal::{PtySessionState, TerminalSignal};
use crate::worktree::CreateWorktreeOptions;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

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
            AuthError::InvalidPairingCode => (StatusCode::BAD_REQUEST, "Invalid pairing code".into()),
            AuthError::ExpiredPairingCode => (StatusCode::UNAUTHORIZED, "Pairing code expired".into()),
            _ => (StatusCode::UNAUTHORIZED, "Unauthorized".into()),
        })?;

    Ok(Json(PairExchangeResponse { token, device }))
}

async fn list_sessions(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<Vec<RemoteTerminalSession>>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let session_ids = state.terminal_service.list_sessions();
    let mut out = Vec::new();
    for id in session_ids {
        if let Some(session) = state.terminal_service.get_session(&id) {
            let running = matches!(session.state(), PtySessionState::Running | PtySessionState::Starting);
            out.push(RemoteTerminalSession {
                session_id: id,
                title: None,
                workspace_id: None,
                worktree_label: None,
                running,
            });
        }
    }

    Ok(Json(out))
}

async fn get_workspace_state(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> Result<Json<RemoteWorkspaceState>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let _device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let projects_raw = state.workspace_registry.list();
    let projects: Vec<RemoteProjectInfo> = projects_raw
        .into_iter()
        .map(|(ws_id, mgr)| RemoteProjectInfo {
            workspace_id: ws_id,
            repo_root: mgr.repo_root().to_string_lossy().to_string(),
        })
        .collect();

    let active_ws = projects.first().map(|p| p.workspace_id.clone()).unwrap_or_else(|| "default".into());
    let worktrees = state
        .workspace_registry
        .manager(&active_ws)
        .map(|m| m.list_worktrees().unwrap_or_default())
        .unwrap_or_default();

    let session_ids = state.terminal_service.list_sessions();
    let mut sessions = Vec::new();
    for id in session_ids {
        if let Some(session) = state.terminal_service.get_session(&id) {
            let running = matches!(session.state(), PtySessionState::Running | PtySessionState::Starting);
            sessions.push(RemoteTerminalSession {
                session_id: id,
                title: None,
                workspace_id: Some(active_ws.clone()),
                worktree_label: None,
                running,
            });
        }
    }

    Ok(Json(RemoteWorkspaceState {
        projects,
        active_workspace_id: active_ws,
        worktrees,
        sessions,
    }))
}

async fn create_worktree(
    State(state): State<Arc<RemoteGatewayState>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
    Json(payload): Json<RemoteCreateWorktreeRequest>,
) -> Result<Json<crate::worktree::Worktree>, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((StatusCode::FORBIDDEN, "View-only device cannot create worktrees".into()));
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
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    if device.permission != DevicePermission::Control {
        return Err((StatusCode::FORBIDDEN, "View-only device cannot delete worktrees".into()));
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
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    Path(device_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
    Path(session_id): Path<String>,
    Query(query): Query<AuthQuery>,
    headers: HeaderMap,
    State(state): State<Arc<RemoteGatewayState>>,
) -> Result<Response, (StatusCode, String)> {
    let token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
    let device = state
        .auth_manager
        .validate_token(&token)
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or revoked token".into()))?;

    let (history, rx) = state
        .terminal_service
        .attach(&session_id)
        .map_err(|_| (StatusCode::NOT_FOUND, "Session not found".into()))?;

    Ok(ws.on_upgrade(move |socket| handle_terminal_socket(socket, session_id, history, rx, device, state)))
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
                                    let _ = term_service.signal(&session_id_clone, TerminalSignal::Interrupt);
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

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
}

fn resolve_dist_dir() -> PathBuf {
    let candidates = [
        PathBuf::from("ui/dist"),
        PathBuf::from("../ui/dist"),
        PathBuf::from("../../ui/dist"),
    ];
    for c in &candidates {
        if c.exists() && c.is_dir() {
            return c.clone();
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe;
        for _ in 0..6 {
            if let Some(parent) = cur.parent() {
                let candidate = parent.join("ui/dist");
                if candidate.exists() && candidate.is_dir() {
                    return candidate;
                }
                cur = parent.to_path_buf();
            }
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
    let _token = extract_token(&headers, Some(&query)).ok_or((StatusCode::UNAUTHORIZED, "Missing auth token".into()))?;
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
        .route("/api/v1/terminal/preferences", get(get_terminal_preferences))
        .route("/api/v1/workspace/worktrees", post(create_worktree).delete(delete_worktree))
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
