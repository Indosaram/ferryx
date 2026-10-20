use super::*;
use crate::daemon::server::DaemonServer;
use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::machine_protocol::Attached;
use crate::remote::protocol::RemoteGridFrame;
use crate::terminal::machine_output::MachineOutputError;
use crate::terminal::output_hub::{HistoryRange, TerminalOutputHub};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::http::Request;
use tokio_tungstenite::tungstenite::Message;

const DEADLINE: Duration = Duration::from_secs(10);

async fn http_request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(DEADLINE, async {
        let mut stream = tokio::net::TcpStream::connect(addr)
            .await
            .expect("tcp connect");
        let _ = stream.set_nodelay(true);
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
        if let Some(t) = token {
            req.push_str(&format!("Authorization: Bearer {t}\r\n"));
        }
        if let Some(b) = body {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
            req.push_str("\r\n");
            req.push_str(b);
        } else {
            req.push_str("\r\n");
        }
        stream.write_all(req.as_bytes()).await.expect("tcp write");
        let mut resp_buf = Vec::new();
        stream.read_to_end(&mut resp_buf).await.expect("tcp read");
        let resp_str = String::from_utf8_lossy(&resp_buf).into_owned();
        let status_code = resp_str
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = resp_str.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status_code, body)
    })
    .await
    .expect("bounded http_request")
}

async fn ws_status(addr: SocketAddr, path: &str, token: Option<&str>) -> u16 {
    tokio::time::timeout(DEADLINE, async {
        let mut builder = Request::builder()
            .uri(format!("ws://{addr}{path}"))
            .header("Host", addr.to_string())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13");
        if let Some(t) = token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let req = builder.body(()).expect("valid request");
        match tokio_tungstenite::connect_async(req).await {
            Ok((_, resp)) => resp.status().as_u16(),
            Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => resp.status().as_u16(),
            Err(e) => panic!("unexpected ws handshake error: {e:?}"),
        }
    })
    .await
    .expect("bounded ws_status")
}

async fn open_ws(
    addr: SocketAddr,
    path: &str,
    token: Option<&str>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    tokio::time::timeout(DEADLINE, async {
        let mut builder = Request::builder()
            .uri(format!("ws://{addr}{path}"))
            .header("Host", addr.to_string())
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .header("Sec-WebSocket-Version", "13");
        if let Some(t) = token {
            builder = builder.header("Authorization", format!("Bearer {t}"));
        }
        let req = builder.body(()).expect("valid request");
        let (stream, _) = tokio_tungstenite::connect_async(req)
            .await
            .expect("ws handshake success");
        stream
    })
    .await
    .expect("bounded open_ws")
}

async fn read_attached_frame<S>(stream: &mut S) -> Attached
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = stream.next().await {
            let msg = msg.expect("ws stream message");
            if let Message::Text(text) = msg {
                if let Ok(attached) = serde_json::from_str::<Attached>(&text) {
                    return attached;
                }
            }
        }
        panic!("stream ended without Attached frame");
    })
    .await
    .expect("bounded read_attached_frame")
}

async fn read_grid_frame_matching<S, F>(stream: &mut S, predicate: F) -> RemoteGridFrame
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    F: Fn(&RemoteGridFrame) -> bool,
{
    tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = stream.next().await {
            let msg = msg.expect("ws stream message");
            if let Message::Text(text) = msg {
                if let Ok(frame) = serde_json::from_str::<RemoteGridFrame>(&text) {
                    if predicate(&frame) {
                        return frame;
                    }
                }
            }
        }
        panic!("stream ended without matching RemoteGridFrame");
    })
    .await
    .expect("bounded read_grid_frame_matching")
}

async fn read_binary_frame_matching<S, F>(stream: &mut S, predicate: F) -> Vec<u8>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    F: Fn(&[u8]) -> bool,
{
    tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = stream.next().await {
            let msg = msg.expect("ws stream message");
            if let Message::Binary(bytes) = msg {
                if predicate(&bytes) {
                    return bytes.to_vec();
                }
            }
        }
        panic!("stream ended without matching Binary frame");
    })
    .await
    .expect("bounded read_binary_frame_matching")
}

async fn read_error_control_matching<S>(stream: &mut S) -> serde_json::Value
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = stream.next().await {
            let msg = msg.expect("ws stream message");
            if let Message::Text(text) = msg {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if val.get("type").and_then(|t| t.as_str()) == Some("error") {
                        return val;
                    }
                }
            }
        }
        panic!("stream ended without error control");
    })
    .await
    .expect("bounded read_error_control_matching")
}

struct TestServer {
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl TestServer {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, create_remote_router(state)).await.unwrap();
        });
        Self { addr, tasks }
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

struct MachineTestFixture {
    _root: tempfile::TempDir,
    owner: Arc<DaemonServer>,
    server: TestServer,
    token: String,
    device_id: String,
    other_token: String,
    session_id: String,
    epoch: String,
}

impl MachineTestFixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let (owner, token, device, other_token, workspace) = crate::ipc::run_blocking({
            let root_path = root.path().to_path_buf();
            move || {
                let project = root_path.join("project");
                std::fs::create_dir_all(&project)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                crate::worktree::run_git(&project, &["init", "--quiet"])
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                crate::worktree::run_git(
                    &project,
                    &[
                        "-c",
                        "user.name=GridTester",
                        "-c",
                        "user.email=grid@example.invalid",
                        "commit",
                        "--allow-empty",
                        "-m",
                        "init",
                    ],
                )
                .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;

                let owner = Arc::new(DaemonServer::new_with_paths(
                    Some(root_path.join("config")),
                    Some(root_path.join("auth")),
                ));
                let state = owner.remote_state().clone();
                let services = state.machine_services.as_ref().unwrap().clone();
                let workspace = services
                    .workspaces
                    .register_machine(project.to_str().unwrap())
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;

                let expected_root = std::fs::canonicalize(&project)
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                let catalog = services
                    .workspaces
                    .catalog()
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                let row = catalog
                    .workspaces
                    .get(&workspace)
                    .ok_or_else(|| crate::ipc::IpcError::internal("workspace row missing"))?;
                assert_eq!(
                    row.repo_root,
                    expected_root,
                    "registered workspace repo_root must match isolated test project directory"
                );

                let pin = state
                    .auth_manager
                    .create_scoped_pairing_code(
                        DevicePermission::Control,
                        DeviceAccessScope::Machine,
                    )
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                let (token, device) = state
                    .auth_manager
                    .exchange_pairing_code(&pin, "grid-tester")
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;

                let other_pin = state
                    .auth_manager
                    .create_scoped_pairing_code(
                        DevicePermission::Control,
                        DeviceAccessScope::Machine,
                    )
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;
                let (other_token, _other_device) = state
                    .auth_manager
                    .exchange_pairing_code(&other_pin, "other-tester")
                    .map_err(|e| crate::ipc::IpcError::internal(e.to_string()))?;

                Ok((owner, token, device, other_token, workspace))
            }
        })
        .await
        .expect("run_blocking fixture setup");

        let state = owner.remote_state().clone();
        let server = TestServer::start(state.clone()).await;

        let request = serde_json::json!({
            "requestId": uuid::Uuid::new_v4().to_string(),
            "workspaceId": workspace,
            "worktree": null,
            "inheritFromSessionId": null,
            "cwdRelative": null,
            "cols": 80,
            "rows": 24,
            "startup": { "kind": "shell" }
        });
        let (status, body) = http_request(
            server.addr,
            "POST",
            "/api/v1/sessions",
            Some(&token),
            Some(&request.to_string()),
        )
        .await;
        assert_eq!(status, 201, "failed to create machine session: {body}");
        let created: serde_json::Value = serde_json::from_str(&body).unwrap();
        let session_id = created["target"]["sessionId"].as_str().unwrap().to_string();
        let epoch = created["target"]["daemonEpoch"].as_str().unwrap().to_string();

        Self {
            _root: root,
            owner,
            server,
            token,
            device_id: device.id,
            other_token,
            session_id,
            epoch,
        }
    }

    async fn stop(self) {
        self.owner
            .terminal_service()
            .close_session(&self.session_id)
            .await
            .expect("close fixture session");
        self.server.stop().await;
    }
}

fn frame_contains_text(frame: &RemoteGridFrame, expected: &str) -> bool {
    let lines = match frame {
        RemoteGridFrame::Grid { lines, .. } | RemoteGridFrame::GridDiff { lines, .. } => lines,
    };
    for line in lines {
        for run in &line.runs {
            if run.text.contains(expected) {
                return true;
            }
        }
    }
    false
}

#[tokio::test]
async fn machine_terminal_rejects_malformed_render_and_geometry() {
    let fixture = MachineTestFixture::new().await;
    let base_path = format!("/api/v1/terminal/{}", fixture.session_id);

    let status = ws_status(
        fixture.server.addr,
        &format!("{base_path}?daemonEpoch={}&render=invalid", fixture.epoch),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, 400);

    let status = ws_status(
        fixture.server.addr,
        &format!("{base_path}?daemonEpoch={}&render=grid&cols=100&rows=40", fixture.epoch),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, 400);

    let status = ws_status(
        fixture.server.addr,
        &format!("{base_path}?daemonEpoch={}&cols=80", fixture.epoch),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, 400);

    let status = ws_status(
        fixture.server.addr,
        &format!("{base_path}?daemonEpoch={}&rows=24", fixture.epoch),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, 400);

    let status = ws_status(
        fixture.server.addr,
        &format!("/api/v1/terminal/peer::{}?daemonEpoch={}&render=grid", fixture.session_id, fixture.epoch),
        Some(&fixture.token),
    )
    .await;
    assert_eq!(status, 400);

    fixture.stop().await;
}

#[tokio::test]
async fn machine_terminal_grid_emits_initial_replay_and_live_grid() {
    let fixture = MachineTestFixture::new().await;
    fixture
        .owner
        .terminal_service()
        .output_hub()
        .publish(&fixture.session_id, b"\x1b[32mINITIAL_REPLAY_GRID\x1b[0m\r\n".to_vec())
        .expect("publish replay output");

    let mut socket = open_ws(
        fixture.server.addr,
        &format!("/api/v1/terminal/{}?daemonEpoch={}&render=grid", fixture.session_id, fixture.epoch),
        Some(&fixture.token),
    )
    .await;

    let attached = read_attached_frame(&mut socket).await;
    assert!(matches!(attached, Attached::Attached { cols: 80, rows: 24, .. }));

    let initial_frame = read_grid_frame_matching(&mut socket, |f| {
        matches!(f, RemoteGridFrame::Grid { cols: 80, rows: 24, .. })
            && frame_contains_text(f, "INITIAL_REPLAY_GRID")
    })
    .await;
    assert!(frame_contains_text(&initial_frame, "INITIAL_REPLAY_GRID"));

    fixture
        .owner
        .terminal_service()
        .output_hub()
        .publish(&fixture.session_id, b"\x1b[31mLIVE_GRID_OUTPUT\x1b[0m\r\n".to_vec())
        .expect("publish live output");

    let live_frame = read_grid_frame_matching(&mut socket, |f| {
        frame_contains_text(f, "LIVE_GRID_OUTPUT")
    })
    .await;
    assert!(frame_contains_text(&live_frame, "LIVE_GRID_OUTPUT"));

    let _ = socket.close(None).await;
    fixture.stop().await;
}

#[tokio::test]
async fn machine_terminal_preserves_raw_mode_and_rejects_scroll() {
    let fixture = MachineTestFixture::new().await;
    fixture
        .owner
        .terminal_service()
        .output_hub()
        .publish(&fixture.session_id, b"\x1b[34mRAW_MODE_REPLAY\x1b[0m\r\n".to_vec())
        .expect("publish replay output");

    let mut socket = open_ws(
        fixture.server.addr,
        &format!("/api/v1/terminal/{}?daemonEpoch={}", fixture.session_id, fixture.epoch),
        Some(&fixture.token),
    )
    .await;

    let attached = read_attached_frame(&mut socket).await;
    let Attached::Attached { generation, .. } = attached else {
        panic!("unexpected attached variant");
    };

    let raw_bytes = read_binary_frame_matching(&mut socket, |b| {
        String::from_utf8_lossy(b).contains("RAW_MODE_REPLAY")
    })
    .await;
    assert!(String::from_utf8_lossy(&raw_bytes).contains("RAW_MODE_REPLAY"));

    let scroll_control = serde_json::json!({
        "type": "scroll",
        "generation": generation.0.to_string(),
        "rows": -5
    });
    socket
        .send(Message::Text(scroll_control.to_string().into()))
        .await
        .expect("send scroll control");

    let error = read_error_control_matching(&mut socket).await;
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "INVALID_CONTROL_OR_GENERATION");

    let _ = socket.close(None).await;
    fixture.stop().await;
}

#[tokio::test]
async fn machine_grid_controller_fencing_and_revocation() {
    let fixture = MachineTestFixture::new().await;
    let path = format!(
        "/api/v1/terminal/{}?daemonEpoch={}&render=grid",
        fixture.session_id, fixture.epoch
    );

    let mut socket_a = open_ws(fixture.server.addr, &path, Some(&fixture.token)).await;

    let attached = read_attached_frame(&mut socket_a).await;
    let Attached::Attached { generation, .. } = attached else {
        panic!("unexpected attached variant");
    };
    let gen_str = generation.0.to_string();

    let _ = read_grid_frame_matching(&mut socket_a, |f| matches!(f, RemoteGridFrame::Grid { .. })).await;

    let status_b = ws_status(fixture.server.addr, &path, Some(&fixture.other_token)).await;
    assert_eq!(status_b, 409);

    let stale_scroll = serde_json::json!({
        "type": "scroll",
        "generation": "999999",
        "rows": -1
    });
    socket_a
        .send(Message::Text(stale_scroll.to_string().into()))
        .await
        .expect("send stale scroll");

    let error = read_error_control_matching(&mut socket_a).await;
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "INVALID_CONTROL_OR_GENERATION");

    let scroll_control = serde_json::json!({
        "type": "scroll",
        "generation": gen_str,
        "rows": -1
    });
    socket_a
        .send(Message::Text(scroll_control.to_string().into()))
        .await
        .expect("send valid scroll");

    let _ = read_grid_frame_matching(&mut socket_a, |f| matches!(f, RemoteGridFrame::Grid { .. })).await;

    let state = fixture.owner.remote_state();
    state.auth_manager.revoke_device(&fixture.device_id).expect("revoke device");

    let close_observed = tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = socket_a.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => return true,
                _ => {}
            }
        }
        true
    })
    .await
    .expect("bounded socket close on revocation");
    assert!(close_observed);

    fixture.stop().await;
}

#[tokio::test]
async fn machine_output_try_charge_retains_permits_until_drop() {
    let hub = TerminalOutputHub::new(1024);
    let session_id = "unit-budget-session";
    hub.register_session(session_id);

    let attachment = hub
        .subscribe_machine(session_id, None)
        .expect("session exists")
        .expect("subscription succeeds");

    drop(attachment.snapshot);

    let initial_pending = attachment.receiver.pending_bytes();
    let available = 1024 * 1024 - initial_pending;

    let permit = attachment
        .receiver
        .try_charge(available)
        .expect("acquires up to remaining budget");
    assert_eq!(attachment.receiver.pending_bytes(), 1024 * 1024);

    let overflow = attachment.receiver.try_charge(1);
    assert_eq!(overflow.unwrap_err(), MachineOutputError::Overflow);

    drop(permit);
    assert_eq!(attachment.receiver.pending_bytes(), initial_pending);

    let reacquired = attachment
        .receiver
        .try_charge(available)
        .expect("re-acquires freed permits");
    drop(reacquired);
}

#[tokio::test]
async fn machine_grid_rejects_oversized_resize_and_allows_raw_resize() {
    let fixture = MachineTestFixture::new().await;
    let path_grid = format!(
        "/api/v1/terminal/{}?daemonEpoch={}&render=grid",
        fixture.session_id, fixture.epoch
    );
    let mut socket_grid = open_ws(fixture.server.addr, &path_grid, Some(&fixture.token)).await;
    let attached = read_attached_frame(&mut socket_grid).await;
    let Attached::Attached { generation, .. } = attached else {
        panic!("unexpected attached variant");
    };

    let oversized_resize = serde_json::json!({
        "type": "resize",
        "generation": generation.0.to_string(),
        "cols": 600,
        "rows": 100
    });
    socket_grid
        .send(Message::Text(oversized_resize.to_string().into()))
        .await
        .expect("send oversized grid resize");

    let err = read_error_control_matching(&mut socket_grid).await;
    assert_eq!(err["type"], "error");
    assert_eq!(err["code"], "INVALID_CONTROL_OR_GENERATION");
    let _ = socket_grid.close(None).await;

    let path_raw = format!(
        "/api/v1/terminal/{}?daemonEpoch={}",
        fixture.session_id, fixture.epoch
    );
    let mut socket_raw = open_ws(fixture.server.addr, &path_raw, Some(&fixture.token)).await;
    let attached_raw = read_attached_frame(&mut socket_raw).await;
    let Attached::Attached { generation: raw_gen, .. } = attached_raw else {
        panic!("unexpected attached variant");
    };

    let raw_resize = serde_json::json!({
        "type": "resize",
        "generation": raw_gen.0.to_string(),
        "cols": 800,
        "rows": 100
    });
    socket_raw
        .send(Message::Text(raw_resize.to_string().into()))
        .await
        .expect("send raw resize");

    let ping = serde_json::json!({ "type": "ping" });
    socket_raw
        .send(Message::Text(ping.to_string().into()))
        .await
        .expect("send ping");

    let pong = tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = socket_raw.next().await {
            if let Ok(Message::Text(t)) = msg {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&t) {
                    if val.get("type").and_then(|s| s.as_str()) == Some("error") {
                        panic!("unexpected error on raw resize: {val:?}");
                    }
                    if val.get("type").and_then(|s| s.as_str()) == Some("pong") {
                        return val;
                    }
                }
            }
        }
        panic!("ping not acknowledged");
    })
    .await
    .expect("bounded pong wait");
    assert_eq!(pong["type"], "pong");

    let session = fixture
        .owner
        .terminal_service()
        .get_session(&fixture.session_id)
        .expect("session exists");
    assert_eq!(session.get_size(), (800, 100));

    let _ = socket_raw.close(None).await;
    fixture.stop().await;
}

#[tokio::test]
async fn machine_grid_rejects_historical_invalid_geometry_segments() {
    let fixture = MachineTestFixture::new().await;

    fixture
        .owner
        .terminal_service()
        .output_hub()
        .record_resize(&fixture.session_id, 1000, 1000)
        .expect("record resize");

    fixture
        .owner
        .terminal_service()
        .output_hub()
        .publish(
            &fixture.session_id,
            b"HISTORICAL_OVERSIZED_CHUNK\r\n".to_vec(),
        )
        .expect("publish bytes under 1000x1000");

    let pty = fixture
        .owner
        .terminal_service()
        .get_session(&fixture.session_id)
        .expect("session exists");
    assert_eq!(
        pty.get_size(),
        (80, 24),
        "PTY must remain at current 80x24 size, isolating historical vs current guard"
    );

    let services = fixture.owner.remote_state().machine_services.as_ref().unwrap();
    let attachment = services
        .sessions
        .attach_machine_output(&fixture.session_id, None)
        .expect("session exists")
        .expect("attachment succeeds");
    let has_oversized_range = attachment
        .history_ranges
        .iter()
        .any(|range| range.cols == Some(1000) && range.rows == Some(1000));
    assert!(
        has_oversized_range,
        "history_ranges must contain range with cols=1000, rows=1000"
    );
    drop(attachment);

    let path_grid = format!(
        "/api/v1/terminal/{}?daemonEpoch={}&render=grid",
        fixture.session_id, fixture.epoch
    );
    let mut socket_grid = open_ws(fixture.server.addr, &path_grid, Some(&fixture.token)).await;

    let closed = tokio::time::timeout(DEADLINE, async {
        while let Some(msg) = socket_grid.next().await {
            match msg {
                Ok(Message::Text(t)) => {
                    if let Ok(frame) = serde_json::from_str::<RemoteGridFrame>(&t) {
                        panic!(
                            "unexpected grid frame emitted before close on invalid historical geometry: {frame:?}"
                        );
                    }
                }
                Ok(Message::Close(_)) | Err(_) => return true,
                _ => {}
            }
        }
        true
    })
    .await
    .expect("bounded socket close on invalid historical geometry");
    assert!(closed);

    let path_raw = format!(
        "/api/v1/terminal/{}?daemonEpoch={}",
        fixture.session_id, fixture.epoch
    );
    let mut socket_raw = open_ws(fixture.server.addr, &path_raw, Some(&fixture.token)).await;
    let attached = read_attached_frame(&mut socket_raw).await;
    assert!(matches!(attached, Attached::Attached { .. }));
    let _ = socket_raw.close(None).await;

    fixture.stop().await;
}

#[tokio::test]
async fn machine_output_history_ranges_charged_within_budget() {
    let hub = TerminalOutputHub::new(1024);
    let session_id = "range-budget-session";
    hub.register_session(session_id);

    hub.publish(session_id, b"INITIAL_CHUNK\r\n".to_vec())
        .expect("publish 1");
    hub.record_resize(session_id, 120, 40)
        .expect("record resize");
    hub.publish(session_id, b"RESIZED_CHUNK\r\n".to_vec())
        .expect("publish 2");

    let attachment = hub
        .subscribe_machine(session_id, None)
        .expect("session exists")
        .expect("subscription succeeds");

    assert!(
        attachment.history_ranges.len() >= 2,
        "attachment must have multiple history ranges after resize"
    );
    let last_range = attachment.history_ranges.last().unwrap();
    assert_eq!(last_range.cols, Some(120));
    assert_eq!(last_range.rows, Some(40));

    // Expected exact accounting:
    // replay_bytes: 15 (INITIAL) + 15 (RESIZED) = 30
    // range_capacity: (1 ledger + 1).next_power_of_two().max(4) = 4
    // max_range_bytes: 4 * size_of::<HistoryRange>()
    // snapshot permit: 30 + 8 (header) + 20 (MACHINE_FRAME_OVERHEAD) + max_range_bytes
    // controls: 16 * 1024 = 16384
    let range_capacity = 4;
    let max_range_bytes = range_capacity * std::mem::size_of::<HistoryRange>();
    let expected_pending = crate::terminal::machine_output::MACHINE_CONTROL_BYTES
        + 30
        + 8
        + crate::terminal::machine_output::MACHINE_FRAME_OVERHEAD
        + max_range_bytes;
    assert_eq!(
        attachment.receiver.pending_bytes(),
        expected_pending,
        "pending_bytes must reflect exact controls + snapshot + range capacity charge"
    );

    let remaining = 1024 * 1024 - expected_pending;
    let full_permit = attachment
        .receiver
        .try_charge(remaining)
        .expect("acquires exact remaining permits up to 1MiB");
    assert_eq!(attachment.receiver.pending_bytes(), 1024 * 1024);
    assert_eq!(
        attachment.receiver.try_charge(1).unwrap_err(),
        MachineOutputError::Overflow,
        "over-budget allocation must overflow"
    );
    drop(full_permit);
    assert_eq!(attachment.receiver.pending_bytes(), expected_pending);

    drop(attachment);
}

#[tokio::test]
async fn machine_output_gap_replays_and_charges_full_history() {
    let hub = TerminalOutputHub::new(20);
    let session_id = "gap-charge-session";
    hub.register_session(session_id);

    hub.publish(session_id, b"\x1b[?2004hxy".to_vec())
        .expect("publish 1");
    hub.publish(session_id, b"abcdefghij".to_vec()).expect("publish 2");
    hub.publish(session_id, b"klmnopqrst".to_vec()).expect("publish 3");
    hub.publish(session_id, b"uvwxyz1234".to_vec()).expect("publish 4");

    // Requesting after seq 1 (evicted) triggers gap and forces full history replay
    let attachment = hub
        .subscribe_machine(session_id, Some(1))
        .expect("session exists")
        .expect("subscription succeeds");

    assert!(
        attachment.snapshot.value.gap.is_some(),
        "gap must be detected on evicted sequence"
    );
    assert_eq!(
        attachment.snapshot.value.history,
        b"\x1b[?2004hklmnopqrstuvwxyz1234"
    );

    let flat_len = attachment.snapshot.value.history.len();
    let range_capacity = 4;
    let max_range_bytes = range_capacity * std::mem::size_of::<HistoryRange>();
    let expected_pending = crate::terminal::machine_output::MACHINE_CONTROL_BYTES
        + flat_len
        + 8
        + crate::terminal::machine_output::MACHINE_FRAME_OVERHEAD
        + max_range_bytes;

    assert_eq!(
        attachment.receiver.pending_bytes(),
        expected_pending,
        "gap replay must charge full surviving history length without undercharging"
    );
    assert!(attachment.receiver.pending_bytes() >= 16384 + flat_len + max_range_bytes);

    let remaining = 1024 * 1024 - expected_pending;
    let full_permit = attachment
        .receiver
        .try_charge(remaining)
        .expect("acquires exact remaining budget");
    assert_eq!(attachment.receiver.pending_bytes(), 1024 * 1024);
    assert_eq!(
        attachment.receiver.try_charge(1).unwrap_err(),
        MachineOutputError::Overflow
    );
    drop(full_permit);

    drop(attachment);
}
