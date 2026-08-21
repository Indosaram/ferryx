use crate::remote::auth::{AuthError, DeviceInfo, DevicePermission};
use crate::remote::protocol::{ClientControlMessage, RemoteTerminalSession};
use crate::remote::state::{RemoteGatewayState, RemoteNetworkMode};
use crate::terminal::{PtySessionState, TerminalSignal};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Deserialize)]
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
                project_id: None,
                worktree_label: None,
                running,
            });
        }
    }

    Ok(Json(out))
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

    // 1. Send initial history buffer as binary frame
    if !history.is_empty() && sender.send(Message::Binary(history.into())).await.is_err() {
        return;
    }

    // 2. Spawn task to stream live broadcast output from OutputHub to WebSocket
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

    // 3. Receive client WebSocket messages (input bytes or JSON control)
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

const REMOTE_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
  <title>rorca Remote Terminal</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/css/xterm.css" />
  <script src="https://cdn.jsdelivr.net/npm/@xterm/xterm@5.5.0/lib/xterm.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/@xterm/addon-fit@0.10.0/lib/addon-fit.js"></script>
  <style>
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { background: #0f1117; color: #e1e4ea; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; display: flex; flex-direction: column; height: 100vh; overflow: hidden; }
    header { background: #181b24; padding: 10px 16px; border-bottom: 1px solid #282c37; display: flex; justify-content: space-between; align-items: center; }
    header h1 { font-size: 15px; font-weight: 600; display: flex; align-items: center; gap: 8px; }
    header span.badge { font-size: 11px; background: #22c55e22; color: #4ade80; padding: 2px 8px; border-radius: 99px; border: 1px solid #22c55e44; }
    #app-container { flex: 1; display: flex; flex-direction: column; position: relative; overflow: hidden; }
    #pairing-view, #session-view, #terminal-view { flex: 1; display: flex; flex-direction: column; }
    .hidden { display: none !important; }
    .card { background: #181b24; border: 1px solid #282c37; border-radius: 8px; padding: 20px; max-width: 400px; margin: 40px auto; width: 90%; }
    .btn { background: #3b82f6; color: white; border: none; padding: 8px 16px; border-radius: 6px; font-size: 14px; font-weight: 500; cursor: pointer; }
    .btn:hover { background: #2563eb; }
    .session-item { background: #1c202c; border: 1px solid #2d3345; border-radius: 6px; padding: 12px 16px; margin-bottom: 10px; cursor: pointer; display: flex; justify-content: space-between; align-items: center; }
    .session-item:hover { border-color: #3b82f6; }
    #terminal-container { flex: 1; background: #000; padding: 4px; overflow: hidden; }
    #touch-bar { background: #181b24; border-top: 1px solid #282c37; display: flex; gap: 6px; padding: 8px; overflow-x: auto; -webkit-overflow-scrolling: touch; }
    .touch-btn { background: #242936; color: #cbd5e1; border: 1px solid #333a4c; padding: 8px 12px; border-radius: 6px; font-size: 13px; font-weight: 600; min-width: 42px; text-align: center; }
    .touch-btn:active { background: #3b82f6; color: white; }
  </style>
</head>
<body>
  <header>
    <h1>🦀 rorca Remote <span class="badge">Live</span></h1>
    <div id="header-actions"></div>
  </header>
  <div id="app-container">
    <div id="pairing-view" class="hidden">
      <div class="card">
        <h2 style="margin-bottom: 12px; font-size: 18px;">Pair Device</h2>
        <p style="color: #94a3b8; font-size: 13px; margin-bottom: 16px;">Enter 6-digit PIN (valid for 1 minute) from rorca Settings:</p>
        <input type="text" id="pair-code-input" placeholder="6-digit PIN" maxlength="6" inputmode="numeric" pattern="[0-9]*" style="width: 100%; padding: 10px; background: #0f1117; border: 1px solid #2d3345; border-radius: 6px; color: white; margin-bottom: 16px; font-family: monospace;" />
        <button class="btn" id="pair-submit-btn" style="width: 100%;">Connect Device</button>
      </div>
    </div>
    <div id="session-view" class="hidden" style="padding: 20px;">
      <h2 style="margin-bottom: 16px; font-size: 16px;">Active Terminals</h2>
      <div id="session-list"></div>
    </div>
    <div id="terminal-view" class="hidden">
      <div id="terminal-container"></div>
      <div id="touch-bar">
        <button class="touch-btn" data-key="ctrl-c">Ctrl-C</button>
        <button class="touch-btn" data-key="tab">Tab</button>
        <button class="touch-btn" data-key="esc">Esc</button>
        <button class="touch-btn" data-key="arrow-up">↑</button>
        <button class="touch-btn" data-key="arrow-down">↓</button>
        <button class="touch-btn" data-key="arrow-left">←</button>
        <button class="touch-btn" data-key="arrow-right">→</button>
        <button class="touch-btn" data-key="ctrl-d">Ctrl-D</button>
      </div>
    </div>
  </div>
  <script>
    const storageTokenKey = "rorca_remote_token";
    let currentWs = null;
    let term = null;
    let fitAddon = null;

    async function init() {
      const hash = window.location.hash;
      if (hash.startsWith("#pair=")) {
        const code = hash.replace("#pair=", "");
        await exchangeCode(code);
        window.location.hash = "";
      }

      const token = localStorage.getItem(storageTokenKey);
      if (!token) {
        showView("pairing-view");
      } else {
        await loadSessions();
      }
    }

    async function exchangeCode(code) {
      try {
        const res = await fetch("/api/v1/pair/exchange", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ code, device_name: navigator.userAgent.includes("Mobile") ? "Mobile Device" : "Browser" })
        });
        if (!res.ok) throw new Error("Pairing failed");
        const data = await res.json();
        localStorage.setItem(storageTokenKey, data.token);
        await loadSessions();
      } catch (err) {
        alert("Pairing error: " + err.message);
        showView("pairing-view");
      }
    }

    async function loadSessions() {
      const token = localStorage.getItem(storageTokenKey);
      try {
        const res = await fetch("/api/v1/sessions?token=" + encodeURIComponent(token));
        if (!res.ok) {
          localStorage.removeItem(storageTokenKey);
          showView("pairing-view");
          return;
        }
        const sessions = await res.json();
        const listEl = document.getElementById("session-list");
        listEl.innerHTML = "";
        if (sessions.length === 0) {
          listEl.innerHTML = "<p style='color:#64748b;'>No active terminals on desktop.</p>";
        } else {
          sessions.forEach(s => {
            const item = document.createElement("div");
            item.className = "session-item";
            item.innerHTML = `<div><strong>Terminal:</strong> <span style="font-family:monospace;">${s.sessionId.substring(0,8)}</span></div><span class="btn" style="padding:4px 10px;font-size:12px;">Attach</span>`;
            item.onclick = () => attachTerminal(s.sessionId);
            listEl.appendChild(item);
          });
        }
        showView("session-view");
      } catch (err) {
        showView("pairing-view");
      }
    }

    function attachTerminal(sessionId) {
      showView("terminal-view");
      const container = document.getElementById("terminal-container");
      container.innerHTML = "";
      term = new Terminal({
        cursorBlink: true,
        fontSize: 14,
        theme: { background: "#000000", foreground: "#ffffff" }
      });
      fitAddon = new FitAddon.FitAddon();
      term.loadAddon(fitAddon);
      term.open(container);
      fitAddon.fit();

      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      const token = localStorage.getItem(storageTokenKey);
      const wsUrl = `${protocol}//${location.host}/api/v1/terminal/${sessionId}?token=${encodeURIComponent(token)}`;
      currentWs = new WebSocket(wsUrl);
      currentWs.binaryType = "arraybuffer";

      currentWs.onmessage = (event) => {
        if (typeof event.data === "string") {
          term.write(event.data);
        } else {
          term.write(new Uint8Array(event.data));
        }
      };

      term.onData((data) => {
        if (currentWs && currentWs.readyState === WebSocket.OPEN) {
          const enc = new TextEncoder();
          currentWs.send(enc.encode(data));
        }
      });

      window.onresize = () => {
        if (fitAddon) {
          fitAddon.fit();
          if (currentWs && currentWs.readyState === WebSocket.OPEN) {
            currentWs.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
          }
        }
      };
    }

    document.querySelectorAll(".touch-btn").forEach(btn => {
      btn.onclick = () => {
        if (!currentWs || currentWs.readyState !== WebSocket.OPEN) return;
        const key = btn.dataset.key;
        const enc = new TextEncoder();
        if (key === "ctrl-c") {
          currentWs.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
        } else if (key === "tab") {
          currentWs.send(enc.encode("	"));
        } else if (key === "esc") {
          currentWs.send(enc.encode(""));
        } else if (key === "arrow-up") {
          currentWs.send(enc.encode("[A"));
        } else if (key === "arrow-down") {
          currentWs.send(enc.encode("[B"));
        } else if (key === "arrow-right") {
          currentWs.send(enc.encode("[C"));
        } else if (key === "arrow-left") {
          currentWs.send(enc.encode("[D"));
        } else if (key === "ctrl-d") {
          currentWs.send(enc.encode(""));
        }
      };
    });

    document.getElementById("pair-submit-btn").onclick = () => {
      const val = document.getElementById("pair-code-input").value.trim();
      if (val) exchangeCode(val);
    };

    function showView(viewId) {
      ["pairing-view", "session-view", "terminal-view"].forEach(id => {
        document.getElementById(id).classList.toggle("hidden", id !== viewId);
      });
    }

    init();
  </script>
</body>
</html>
"##;

async fn index_html() -> Html<&'static str> {
    Html(REMOTE_HTML)
}

pub fn create_remote_router(state: Arc<RemoteGatewayState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(index_html))
        .route("/api/v1/health", get(health_check))
        .route("/api/v1/pair/exchange", post(pair_exchange))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/devices", get(list_devices))
        .route("/api/v1/devices/{id}/revoke", post(revoke_device))
        .route("/api/v1/terminal/{sessionId}", get(ws_terminal_handler))
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
