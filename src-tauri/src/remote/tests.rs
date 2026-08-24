use super::*;
use crate::terminal::TerminalService;
use std::path::PathBuf;
use std::sync::Arc;

struct MockTailscaleRunner {
    installed: bool,
    running: bool,
    self_dns: Option<String>,
}

impl CommandRunner for MockTailscaleRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        if program != "tailscale" {
            return Err("not tailscale".into());
        }
        if !self.installed {
            return Err("command not found".into());
        }
        if args.contains(&"status") {
            let json = serde_json::json!({
                "BackendState": if self.running { "Running" } else { "Stopped" },
                "CurrentTailnet": { "Name": "test-tailnet" },
                "Self": { "DNSName": self.self_dns.as_deref().unwrap_or("my-mac.test.ts.net.") }
            });
            Ok(json.to_string())
        } else if args.contains(&"serve") {
            Ok(r#"{"TCP":{"43821":{"HTTPS":true}}}"#.into())
        } else {
            Ok("".into())
        }
    }
}

#[test]
fn test_tailscale_status_parsing() {
    let runner = MockTailscaleRunner {
        installed: true,
        running: true,
        self_dns: Some("my-mac.tailnet.ts.net.".into()),
    };
    let status = check_tailscale_status(&runner);
    assert!(status.installed);
    assert!(status.running);
    assert_eq!(status.self_dns, Some("my-mac.tailnet.ts.net".into()));
    assert_eq!(status.tailnet_name, Some("test-tailnet".into()));
}

#[test]
fn test_auth_manager_pairing_and_revocation() {
    let auth = AuthManager::new();
    let code = auth.create_pairing_code(DevicePermission::Control);

    // Invalid code fails
    assert!(auth
        .exchange_pairing_code("invalid_code", "test-device")
        .is_err());

    // Valid code succeeds
    let (token, device) = auth
        .exchange_pairing_code(&code, "test-device")
        .expect("exchange success");
    assert_eq!(device.permission, DevicePermission::Control);
    assert!(!device.revoked);

    // Code is single use
    assert!(auth.exchange_pairing_code(&code, "test-device-2").is_err());

    // Validate token
    let validated = auth.validate_token(&token).expect("validate success");
    assert_eq!(validated.id, device.id);

    // Revoke device
    assert!(auth.revoke_device(&device.id));
    assert!(matches!(
        auth.validate_token(&token),
        Err(AuthError::RevokedDevice)
    ));
}

#[tokio::test]
async fn test_remote_server_rejects_off_mode_without_binding() {
    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(
        terminal_service,
        crate::worktree::WorkspaceRegistry::new(),
    ));

    assert_eq!(state.config.read().mode, RemoteNetworkMode::Off);
    assert!(start_remote_server(Arc::clone(&state)).await.is_err());
    assert!(!*state.is_running.read());
    assert!(state.bound_address.read().is_none());
}

#[tokio::test]
async fn test_remote_server_health_and_lifecycle() {
    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(
        terminal_service,
        crate::worktree::WorkspaceRegistry::new(),
    ));

    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::Tailscale,
        port: 0, // OS assigns available port
        allow_control: true,
    };

    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start server");
    assert!(addr.ip().is_unspecified() || addr.ip().is_loopback());

    // Health endpoint
    let client = reqwest_like_health(&format!("http://{addr}/api/v1/health")).await;
    assert!(client);

    handle.stop();
}

async fn reqwest_like_health(url: &str) -> bool {
    // simple TCP request
    let parsed: std::net::SocketAddr = url
        .trim_start_matches("http://")
        .trim_end_matches("/api/v1/health")
        .parse()
        .unwrap();
    if let Ok(mut stream) = tokio::net::TcpStream::connect(parsed).await {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let req = b"GET /api/v1/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(req).await;
        let mut buf = [0u8; 1024];
        if let Ok(n) = stream.read(&mut buf).await {
            let res = String::from_utf8_lossy(&buf[..n]);
            return res.contains("200 OK") && res.contains(r#""status":"ok""#);
        }
    }
    false
}

#[tokio::test]
async fn test_remote_server_serves_spa_index_html() {
    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(
        terminal_service,
        crate::worktree::WorkspaceRegistry::new(),
    ));

    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::LocalNetwork,
        port: 0,
        allow_control: true,
    };

    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start server");

    let parsed: std::net::SocketAddr = addr;
    if let Ok(mut stream) = tokio::net::TcpStream::connect(parsed).await {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let req = b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(req).await;
        let mut buf = [0u8; 4096];
        if let Ok(n) = stream.read(&mut buf).await {
            let res = String::from_utf8_lossy(&buf[..n]);
            assert!(res.contains("200 OK"));
            assert!(res.contains("<!doctype html>") || res.contains("<html"));
        }
    }

    handle.stop();
}

#[test]
fn test_derive_session_metadata_matches_workspace() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let repo_root = dir.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    crate::worktree::run_git(&repo_root, &["init"]).expect("git init");
    let wt = repo_root.join("wt-a");
    std::fs::create_dir_all(&wt).unwrap();

    let registry = crate::worktree::WorkspaceRegistry::new();
    registry.register("ws1", &repo_root).expect("register");
    let cache = crate::remote::server::WorkspaceSnapshotCache::build(&registry);

    // Path inside a registered workspace but not a listed git worktree (e.g. an
    // ad-hoc subdirectory terminal): workspace matches, label falls back to the
    // path relative to the workspace root.
    let (ws_id, label) = cache.derive_session_metadata(Some(&wt));
    assert_eq!(ws_id.as_deref(), Some("ws1"));
    assert_eq!(label.as_deref(), Some("wt-a"));

    // Nested ad-hoc subdirectory: label is the full relative path, not just the
    // leaf directory name, so it stays informative and collision-resistant.
    let nested = wt.join("src").join("feature");
    std::fs::create_dir_all(&nested).unwrap();
    let (ws_id, label) = cache.derive_session_metadata(Some(&nested));
    assert_eq!(ws_id.as_deref(), Some("ws1"));
    assert_eq!(label.as_deref(), Some("wt-a/src/feature"));

    // Path outside any registered workspace: no workspace id, best-effort label.
    let other = dir.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let (ws_id, label) = cache.derive_session_metadata(Some(&other));
    assert_eq!(ws_id, None);
    assert_eq!(label.as_deref(), Some("other"));

    // No owning worktree path at all.
    assert_eq!(cache.derive_session_metadata(None), (None, None));

    // The repo root itself (a session opened directly at the workspace root, not
    // in any subdirectory): must not panic, and should yield a sensible, non-empty
    // label. The root is git's "main" worktree entry, so it resolves via the
    // checked-out branch name rather than the relative-path fallback.
    let (ws_id, label) = cache.derive_session_metadata(Some(&repo_root));
    assert_eq!(ws_id.as_deref(), Some("ws1"));
    assert!(label.as_deref().is_some_and(|l| !l.is_empty()));

    // Nonexistent path (e.g. a worktree deleted after the session was spawned)
    // inside the registered root: canonicalize() fails and falls back to the raw
    // path without panicking.
    let missing = repo_root.join("does-not-exist");
    let (ws_id, label) = cache.derive_session_metadata(Some(&missing));
    assert_eq!(ws_id.as_deref(), Some("ws1"));
    assert_eq!(label.as_deref(), Some("does-not-exist"));
}

async fn http_request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
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
}

async fn ws_handshake_status(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");
    let auth_header = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{auth_header}\r\n"
    );
    stream.write_all(req.as_bytes()).await.expect("tcp write");
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await.expect("tcp read");
    let resp_str = String::from_utf8_lossy(&buf[..n]);
    resp_str
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0)
}

#[tokio::test]
async fn test_active_desktop_terminal_contract_and_safe_selection_bridge() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let repo_root = dir.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    crate::worktree::run_git(&repo_root, &["init"]).expect("git init");
    crate::worktree::run_git(
        &repo_root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .expect("git commit");

    let registry = crate::worktree::WorkspaceRegistry::new();
    registry.register("ws1", &repo_root).expect("register");

    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let terminal_service = Arc::new(crate::terminal::TerminalService::new(
        Arc::clone(&pty),
        Arc::clone(&hub),
    ));

    let (s1, mut rx1) = pty
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn s1");
    hub.register_session(&s1);
    let s1_clone = s1.clone();
    let hub_clone = Arc::clone(&hub);
    tokio::spawn(async move {
        while let Some(chunk) = rx1.recv().await {
            hub_clone.publish(&s1_clone, chunk);
        }
    });

    let (s2, mut rx2) = pty
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn s2");
    hub.register_session(&s2);
    let s2_clone = s2.clone();
    let hub_clone2 = Arc::clone(&hub);
    tokio::spawn(async move {
        while let Some(chunk) = rx2.recv().await {
            hub_clone2.publish(&s2_clone, chunk);
        }
    });

    let state = Arc::new(RemoteGatewayState::new(
        Arc::clone(&terminal_service),
        registry.clone(),
    ));

    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::LocalNetwork,
        port: 0,
        allow_control: true,
    };

    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start server");

    let code_ctrl = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token_ctrl, _) = state
        .auth_manager
        .exchange_pairing_code(&code_ctrl, "ControlDevice")
        .expect("pair ctrl");

    let code_view = state
        .auth_manager
        .create_pairing_code(DevicePermission::View);
    let (token_view, _) = state
        .auth_manager
        .exchange_pairing_code(&code_view, "ViewDevice")
        .expect("pair view");

    // 1. Without active selection set:
    // GET /api/v1/sessions must return empty vec (no background sessions exposed)
    let (status, body) =
        http_request(addr, "GET", "/api/v1/sessions", Some(&token_ctrl), None).await;
    assert_eq!(status, 200);
    let sessions: Vec<RemoteTerminalSession> = serde_json::from_str(&body).expect("parse sessions");
    assert!(
        sessions.is_empty(),
        "Must not expose background sessions when no active desktop selection is declared, got: {:?}",
        sessions
    );

    // GET /api/v1/workspace/state must also have empty sessions
    let (status, body) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let ws_state: RemoteWorkspaceState = serde_json::from_str(&body).expect("parse ws state");
    assert!(
        ws_state.sessions.is_empty(),
        "Workspace state must not expose background sessions, got: {:?}",
        ws_state.sessions
    );

    // Attach to s1 or s2 without active selection must return 403 Forbidden
    let ws_status =
        ws_handshake_status(addr, &format!("/api/v1/terminal/{s1}"), Some(&token_ctrl)).await;
    assert_eq!(
        ws_status, 403,
        "Attach to session without declared active selection must return 403 Forbidden"
    );

    // 2. Set active selection to s1 (safe IDs only)
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("ws1".into()),
        worktree_slug: None,
        worktree_label: Some("main".into()),
        session_id: Some(s1.clone()),
    });

    // GET /api/v1/sessions must return ONLY s1, s2 is excluded
    let (status, body) =
        http_request(addr, "GET", "/api/v1/sessions", Some(&token_ctrl), None).await;
    assert_eq!(status, 200);
    let sessions: Vec<RemoteTerminalSession> = serde_json::from_str(&body).expect("parse sessions");
    assert_eq!(
        sessions.len(),
        1,
        "Only declared active session should be returned"
    );
    assert_eq!(sessions[0].session_id, s1);

    // GET /api/v1/workspace/state sessions must return ONLY s1
    let (status, body) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let ws_state: RemoteWorkspaceState = serde_json::from_str(&body).expect("parse ws state");
    assert_eq!(ws_state.sessions.len(), 1);
    assert_eq!(ws_state.sessions[0].session_id, s1);

    // Attach to s2 (background session) MUST return 403 Forbidden even though PTY exists
    let ws_status_s2 =
        ws_handshake_status(addr, &format!("/api/v1/terminal/{s2}"), Some(&token_ctrl)).await;
    assert_eq!(
        ws_status_s2, 403,
        "Attach to background session s2 must return 403 Forbidden"
    );

    // Attach to s1 (active session) succeeds (101 Switching Protocols)
    let ws_status_s1 =
        ws_handshake_status(addr, &format!("/api/v1/terminal/{s1}"), Some(&token_ctrl)).await;
    assert_eq!(
        ws_status_s1, 101,
        "Attach to active session s1 must succeed with 101 Switching Protocols"
    );

    // 3. Mobile selection request route
    let event_received = Arc::new(parking_lot::Mutex::new(None));
    let event_received_clone = Arc::clone(&event_received);
    state.set_desktop_event_sink(Arc::new(move |event, payload| {
        *event_received_clone.lock() = Some((event.to_string(), payload));
    }));

    // View device cannot select workspace -> 403 Forbidden
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_view),
        Some(r#"{"workspaceId":"ws1"}"#),
    )
    .await;
    assert_eq!(
        status, 403,
        "View-only device must be rejected with 403 Forbidden on workspace selection"
    );

    // Unregistered workspace -> 400 Bad Request
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(r#"{"workspaceId":"nonexistent-ws"}"#),
    )
    .await;
    assert_eq!(
        status, 400,
        "Unregistered workspace selection must return 400 Bad Request"
    );

    // Control device selects registered workspace -> 200 OK and emits safe desktop event
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(r#"{"workspaceId":"ws1"}"#),
    )
    .await;
    assert_eq!(status, 200);

    let received = event_received.lock().take().expect("event was emitted");
    assert_eq!(received.0, "remote_selection_requested");
    assert_eq!(received.1["workspaceId"], "ws1");
    // Verify NEVER a local absolute path, cwd, or repoRoot in payload
    let serialized_payload = received.1.to_string();
    assert!(
        !serialized_payload.contains(repo_root.to_str().unwrap()),
        "Event payload must never expose repo root or local absolute paths"
    );

    // Create a worktree to test worktree selection
    let mgr = registry.manager("ws1").unwrap();
    let wt_path = mgr.worktree_path_for("ws1", "feat").expect("wt path");
    let _wt = mgr
        .create_worktree(crate::worktree::CreateWorktreeOptions {
            ws_id: "ws1".into(),
            slug: "feat".into(),
            path: wt_path.clone(),
            base_ref: None,
        })
        .expect("create worktree");

    // Select valid worktree
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(r#"{"workspaceId":"ws1","worktreeSlug":"feat"}"#),
    )
    .await;
    assert_eq!(status, 200);
    let received_wt = event_received.lock().take().expect("wt event emitted");
    assert_eq!(received_wt.1["workspaceId"], "ws1");
    assert_eq!(received_wt.1["worktreeSlug"], "feat");
    assert_eq!(received_wt.1["worktree"]["wsId"], "ws1");
    assert_eq!(received_wt.1["worktree"]["slug"], "feat");
    assert!(
        !received_wt
            .1
            .to_string()
            .contains(wt_path.to_str().unwrap()),
        "Worktree event payload must not leak local path"
    );

    // Select invalid worktree slug -> 400 Bad Request
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(r#"{"workspaceId":"ws1","worktreeSlug":"nonexistent"}"#),
    )
    .await;
    assert_eq!(status, 400);

    // 4. Test active session termination: when active session exits, sessions list becomes empty
    pty.close_session(&s1).await.unwrap();
    let (status, body) =
        http_request(addr, "GET", "/api/v1/sessions", Some(&token_ctrl), None).await;
    assert_eq!(status, 200);
    let sessions_after_close: Vec<RemoteTerminalSession> =
        serde_json::from_str(&body).expect("parse sessions");
    assert!(
        sessions_after_close.is_empty(),
        "Terminated active session must not be exposed"
    );

    handle.stop();
    pty.close_session(&s2).await.unwrap();
}

#[tokio::test]
async fn test_desktop_ipc_active_selection_lifecycle() {
    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let terminal_service = Arc::new(crate::terminal::TerminalService::new(pty, hub));
    let registry = crate::worktree::WorkspaceRegistry::new();
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
    let _manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::new(state.clone()));

    // Initially None
    assert!(state.active_selection().is_none());

    // Update active selection
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("my-ws".into()),
        worktree_slug: Some("my-feature".into()),
        worktree_label: Some("orca/my-ws/my-feature".into()),
        session_id: Some("term-uuid-456".into()),
    });

    let current = state.active_selection().expect("selection set");
    assert_eq!(current.workspace_id.as_deref(), Some("my-ws"));
    assert_eq!(current.worktree_slug.as_deref(), Some("my-feature"));
    assert_eq!(
        current.worktree_label.as_deref(),
        Some("orca/my-ws/my-feature")
    );
    assert_eq!(current.session_id.as_deref(), Some("term-uuid-456"));

    // Clear active selection
    state.clear_active_selection();
    assert!(state.active_selection().is_none());
}

#[tokio::test]
async fn test_remote_listener_persistence_and_startup_restoration_lifecycle() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");

    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let terminal_service = Arc::new(crate::terminal::TerminalService::new(pty, hub));
    let registry = crate::worktree::WorkspaceRegistry::new();

    // 1. Initial process: pair a device and enable LocalNetwork gateway
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::clone(&terminal_service),
        registry.clone(),
        Some(config_path.clone()),
        Some(auth_path.clone()),
    ));
    let _manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::new(Arc::clone(
        &state,
    )));

    let code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code(&code, "TestTablet")
        .expect("pair");

    {
        let mut cfg = state.config.write();
        cfg.mode = RemoteNetworkMode::LocalNetwork;
        cfg.port = 0;
        cfg.allow_control = true;
    }
    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start initial server");
    assert!(reqwest_like_health(&format!("http://{addr}/api/v1/health")).await);
    state.persist_config().expect("persist enabled config");

    // Simulate Desktop shutdown / restart: stop listener handle
    handle.stop();

    // 2. Relaunch: load persisted state and run startup restoration hook
    let reopened_state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::clone(&terminal_service),
        registry.clone(),
        Some(config_path.clone()),
        Some(auth_path.clone()),
    ));
    let reopened_manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::new(Arc::clone(
        &reopened_state,
    )));

    // Execute the exact startup restoration hook
    let restored = reopened_manager
        .restore_persisted_listener()
        .await
        .expect("restoration call succeeds");
    assert!(
        restored,
        "Restoration hook must report listener was restored"
    );

    // Verify state & live listener health
    assert_eq!(
        reopened_state.config.read().mode,
        RemoteNetworkMode::LocalNetwork
    );
    assert!(
        *reopened_state.is_running.read(),
        "Restored state must report is_running = true"
    );
    let bound_addr = reopened_state
        .bound_address
        .read()
        .clone()
        .expect("bound address must be present");
    assert!(
        reqwest_like_health(&format!("http://{bound_addr}/api/v1/health")).await,
        "Restored listener must respond to live HTTP health check"
    );

    // Verify pairing auth token survived restart
    let validated = reopened_state
        .auth_manager
        .validate_token(&token)
        .expect("token should be valid");
    assert_eq!(validated.id, device.id);

    // 3. User explicitly disables Remote listener
    reopened_manager.stop();
    {
        let mut cfg = reopened_state.config.write();
        cfg.mode = RemoteNetworkMode::Off;
        *reopened_state.is_running.write() = false;
        *reopened_state.bound_address.write() = None;
    }
    reopened_state.persist_config().expect("persist off config");

    // 4. Second Relaunch: state must remain Off and startup hook must not start a listener
    let second_state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::clone(&terminal_service),
        registry.clone(),
        Some(config_path.clone()),
        Some(auth_path.clone()),
    ));
    let second_manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::new(Arc::clone(
        &second_state,
    )));

    let second_restored = second_manager
        .restore_persisted_listener()
        .await
        .expect("second restoration call succeeds");
    assert!(
        !second_restored,
        "Startup hook must not restore listener when mode is Off"
    );
    assert_eq!(second_state.config.read().mode, RemoteNetworkMode::Off);
    assert!(
        !*second_state.is_running.read(),
        "Listener must remain not running"
    );
    assert!(
        second_state.bound_address.read().is_none(),
        "Bound address must remain None"
    );
}

async fn setup_test_daemon_with_remote_paths(
    config_path: Option<PathBuf>,
    auth_path: Option<PathBuf>,
) -> (
    tempfile::TempDir,
    Arc<crate::daemon::client::DaemonClient>,
    Arc<crate::daemon::server::DaemonServer>,
    tokio::task::JoinHandle<()>,
) {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("daemon.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
    let server = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
        config_path,
        auth_path,
    ));
    let server_clone = Arc::clone(&server);
    let server_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let s = Arc::clone(&server_clone);
                    tokio::spawn(async move {
                        s.handle_client(stream).await;
                    });
                }
                Err(_) => break,
            }
        }
    });
    let client = Arc::new(crate::daemon::client::DaemonClient::new_with_socket(
        socket_path,
    ));
    (dir, client, server, server_task)
}

#[tokio::test]
async fn test_gui_remote_forwarding_and_no_gui_gateway_ownership() {
    use tauri::Manager;

    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");

    let (_daemon_dir, daemon_client, server, server_task) =
        setup_test_daemon_with_remote_paths(Some(config_path.clone()), Some(auth_path.clone()))
            .await;

    let remote_manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::from_daemon(
        Arc::clone(&daemon_client),
    ));
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon_client))
        .manage(remote_manager)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    // 1. Initial status via GUI IPC command -> forwarded to daemon
    let status = crate::ipc::remote::cmd_remote_status(app.state())
        .await
        .expect("cmd_remote_status");
    assert!(!status.enabled);
    assert_eq!(status.mode, RemoteNetworkMode::Off);

    // 2. Enable remote gateway via GUI IPC command -> forwarded to daemon
    let enable_resp = crate::ipc::remote::cmd_remote_enable(
        app.state(),
        crate::ipc::remote::EnableRemoteGatewayRequest {
            mode: RemoteNetworkMode::LocalNetwork,
            port: Some(0),
            allow_control: Some(true),
        },
    )
    .await
    .expect("cmd_remote_enable");

    assert!(enable_resp.enabled);
    assert_eq!(enable_resp.mode, RemoteNetworkMode::LocalNetwork);
    assert!(enable_resp.bound_address.is_some());
    // Verify daemon is the authority holding the bound state
    assert!(*server.remote_state().is_running.read());

    // 3. Create pairing code via GUI IPC command -> generated on daemon
    let pair_resp =
        crate::ipc::remote::cmd_remote_pairing_create(app.state(), Some(DevicePermission::Control))
            .await
            .expect("cmd_remote_pairing_create");
    assert_eq!(pair_resp.code.len(), 6);

    // 4. Exchange pairing code against daemon HTTP server
    let bound_addr: std::net::SocketAddr = enable_resp.bound_address.unwrap().parse().unwrap();
    let (pair_status, pair_body) = http_request(
        bound_addr,
        "POST",
        "/api/v1/pair/exchange",
        None,
        Some(&format!(
            r#"{{"code":"{}","deviceName":"ForwardDevice"}}"#,
            pair_resp.code
        )),
    )
    .await;
    assert_eq!(pair_status, 200);
    let pair_parsed: serde_json::Value = serde_json::from_str(&pair_body).expect("parse json");
    let device_id = pair_parsed["device"]["id"].as_str().unwrap().to_string();

    // 5. List devices via GUI IPC command -> lists devices from daemon
    let devices = crate::ipc::remote::cmd_remote_devices(app.state())
        .await
        .expect("cmd_remote_devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, device_id);

    // 6. Active selection set & get via GUI IPC command -> updates daemon
    crate::ipc::remote::cmd_remote_set_active_selection(
        app.state(),
        crate::ipc::remote::SetActiveDesktopSelectionRequest {
            workspace_id: Some("ws-gui".into()),
            worktree_slug: None,
            worktree_label: Some("main".into()),
            session_id: Some("session-gui".into()),
        },
    )
    .await
    .expect("set active selection");

    let sel = crate::ipc::remote::cmd_remote_get_active_selection(app.state())
        .await
        .expect("get active selection");
    assert_eq!(sel.unwrap().workspace_id.as_deref(), Some("ws-gui"));
    assert_eq!(
        server
            .remote_state()
            .active_selection()
            .unwrap()
            .workspace_id
            .as_deref(),
        Some("ws-gui")
    );

    // 7. Revoke device via GUI IPC command -> revokes on daemon
    let revoked = crate::ipc::remote::cmd_remote_device_revoke(app.state(), device_id)
        .await
        .expect("revoke device");
    assert!(revoked);

    let devices_after = crate::ipc::remote::cmd_remote_devices(app.state())
        .await
        .expect("devices after revoke");
    assert!(devices_after[0].revoked);

    // 8. Disable via GUI IPC command -> stops daemon listener and sets Off
    let disable_resp = crate::ipc::remote::cmd_remote_disable(app.state())
        .await
        .expect("cmd_remote_disable");
    assert!(!disable_resp.enabled);
    assert_eq!(disable_resp.mode, RemoteNetworkMode::Off);
    assert!(!*server.remote_state().is_running.read());

    server_task.abort();
}

#[tokio::test]
async fn test_daemon_pairing_and_revocation_authority() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");

    let (_daemon_dir, daemon_client, _server, server_task) =
        setup_test_daemon_with_remote_paths(Some(config_path), Some(auth_path)).await;

    // Configure daemon to listen on local network port 0
    daemon_client
        .remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
        })
        .await
        .expect("configure daemon");

    let status = daemon_client.remote_get_status().await.expect("status");
    assert!(status.is_running);
    let bound_addr: std::net::SocketAddr = status.bound_address.unwrap().parse().unwrap();

    // Create pairing code via daemon authority
    let code = daemon_client
        .remote_create_pairing_code(Some(DevicePermission::Control))
        .await
        .expect("create pairing code");

    // Exchange pairing code on daemon's HTTP listener
    let (pair_status, pair_body) = http_request(
        bound_addr,
        "POST",
        "/api/v1/pair/exchange",
        None,
        Some(&format!(r#"{{"code":"{code}","deviceName":"AuthDevice"}}"#)),
    )
    .await;
    assert_eq!(pair_status, 200);
    let pair_json: serde_json::Value = serde_json::from_str(&pair_body).expect("parse");
    let token = pair_json["token"].as_str().unwrap();
    let device_id = pair_json["device"]["id"].as_str().unwrap();

    // Verify authenticated request succeeds
    let (devices_status, _) =
        http_request(bound_addr, "GET", "/api/v1/devices", Some(token), None).await;
    assert_eq!(devices_status, 200);

    // List devices via daemon client
    let devices = daemon_client
        .remote_list_devices()
        .await
        .expect("list devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, device_id);
    assert!(!devices[0].revoked);

    // Revoke device via daemon authority
    daemon_client
        .remote_revoke_device(device_id)
        .await
        .expect("revoke device");

    // Verify subsequent authenticated requests on daemon HTTP listener are rejected with 401 Unauthorized
    let (revoked_status, _) =
        http_request(bound_addr, "GET", "/api/v1/devices", Some(token), None).await;
    assert_eq!(
        revoked_status, 401,
        "Revoked token must be rejected with 401"
    );

    // Verify WebSocket connection with revoked token is rejected
    let ws_status = ws_handshake_status(bound_addr, "/api/v1/events", Some(token)).await;
    assert_eq!(
        ws_status, 401,
        "WebSocket with revoked token must return 401"
    );

    server_task.abort();
}

#[tokio::test]
async fn test_successful_enable_restores_and_disable_remains_off() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");

    // Phase 1: Server 1 enables Remote listener on port 0 and persists
    {
        let server1 = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(config_path.clone()),
            Some(auth_path.clone()),
        ));
        server1
            .handle_remote_configure(RemoteGatewayConfig {
                mode: RemoteNetworkMode::LocalNetwork,
                port: 0,
                allow_control: true,
            })
            .await
            .expect("enable server 1");

        assert!(server1.remote_state().bound_address.read().is_some());
        assert_eq!(
            server1.remote_state().config.read().mode,
            RemoteNetworkMode::LocalNetwork
        );
    }

    // Phase 2: Server 2 reopens with persisted enabled config
    {
        let server2 = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(config_path.clone()),
            Some(auth_path.clone()),
        ));
        // Verify persisted config was loaded as LocalNetwork
        assert_eq!(
            server2.remote_state().config.read().mode,
            RemoteNetworkMode::LocalNetwork
        );
        // Initially unbound
        assert!(!*server2.remote_state().is_running.read());

        // Restore listener on startup
        let persisted_cfg = server2.remote_state().config.read().clone();
        server2
            .handle_remote_configure(persisted_cfg)
            .await
            .expect("restore listener on startup");

        assert!(*server2.remote_state().is_running.read());
        let bound_addr = server2.remote_state().bound_address.read().clone().unwrap();
        assert!(reqwest_like_health(&format!("http://{bound_addr}/api/v1/health")).await);

        // Explicitly disable
        server2
            .handle_remote_configure(RemoteGatewayConfig {
                mode: RemoteNetworkMode::Off,
                port: 0,
                allow_control: true,
            })
            .await
            .expect("disable server 2");

        assert!(!*server2.remote_state().is_running.read());
        assert_eq!(
            server2.remote_state().config.read().mode,
            RemoteNetworkMode::Off
        );
    }

    // Phase 3: Server 3 reopens after explicit disable -> remains Off and does not restore listener
    {
        let server3 = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(config_path.clone()),
            Some(auth_path.clone()),
        ));
        assert_eq!(
            server3.remote_state().config.read().mode,
            RemoteNetworkMode::Off
        );
        assert!(!*server3.remote_state().is_running.read());
        assert!(server3.remote_state().bound_address.read().is_none());
    }
}

#[tokio::test]
async fn test_occupied_port_enable_does_not_persist_enabled_intent() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");

    let server = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
        Some(config_path.clone()),
        Some(auth_path.clone()),
    ));

    // Bind a separate TCP listener on 0.0.0.0 to occupy a port
    let occupied_listener = tokio::net::TcpListener::bind("0.0.0.0:0")
        .await
        .expect("bind occupied port");
    let occupied_port = occupied_listener.local_addr().unwrap().port();

    // Given: server starts with default Off configuration
    assert_eq!(
        server.remote_state().config.read().mode,
        RemoteNetworkMode::Off
    );

    // When: attempting to enable on the occupied port
    let enable_result = server
        .handle_remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: occupied_port,
            allow_control: true,
        })
        .await;

    // Then: enable fails
    assert!(
        enable_result.is_err(),
        "Configuring on an occupied port must fail"
    );

    // In-memory config must not remain in enabled mode
    assert_eq!(
        server.remote_state().config.read().mode,
        RemoteNetworkMode::Off,
        "In-memory config must roll back to Off after failed enable"
    );
    assert!(!*server.remote_state().is_running.read());
    assert!(server.remote_state().bound_address.read().is_none());

    // Persisted config on disk must NOT contain enabled mode
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).expect("read config file");
        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("parse persisted config");
        assert_eq!(
            parsed["mode"], "off",
            "Persisted config mode must remain 'off' when bind fails, but found: {content}"
        );
    }
}

async fn open_ws_stream(
    addr: std::net::SocketAddr,
    path: &str,
    token: Option<&str>,
) -> tokio::net::TcpStream {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .expect("tcp connect");
    let auth_header = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{auth_header}\r\n"
    );
    stream.write_all(req.as_bytes()).await.expect("tcp write");
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await.expect("tcp read");
    let resp_str = String::from_utf8_lossy(&buf[..n]);
    assert!(
        resp_str.contains("101 Switching Protocols"),
        "WebSocket upgrade must succeed, got:\n{resp_str}"
    );
    stream
}

#[tokio::test]
async fn test_connected_terminal_websocket_closed_when_active_selection_changes() {
    use tokio::io::AsyncReadExt;

    let dir = tempfile::TempDir::new().expect("tempdir");
    let repo_root = dir.path().join("repo");
    std::fs::create_dir_all(&repo_root).unwrap();
    crate::worktree::run_git(&repo_root, &["init"]).expect("git init");
    crate::worktree::run_git(
        &repo_root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .expect("git commit");

    let registry = crate::worktree::WorkspaceRegistry::new();
    registry.register("ws1", &repo_root).expect("register");

    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let terminal_service = Arc::new(crate::terminal::TerminalService::new(
        Arc::clone(&pty),
        Arc::clone(&hub),
    ));

    let (s1, mut rx1) = pty
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn s1");
    hub.register_session(&s1);
    let s1_clone = s1.clone();
    let hub_clone = Arc::clone(&hub);
    tokio::spawn(async move {
        while let Some(chunk) = rx1.recv().await {
            hub_clone.publish(&s1_clone, chunk);
        }
    });

    let (s2, mut rx2) = pty
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .expect("spawn s2");
    hub.register_session(&s2);
    let s2_clone = s2.clone();
    let hub_clone2 = Arc::clone(&hub);
    tokio::spawn(async move {
        while let Some(chunk) = rx2.recv().await {
            hub_clone2.publish(&s2_clone, chunk);
        }
    });

    let state = Arc::new(RemoteGatewayState::new(
        Arc::clone(&terminal_service),
        registry.clone(),
    ));

    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::LocalNetwork,
        port: 0,
        allow_control: true,
    };

    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start server");

    let code_ctrl = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token_ctrl, _) = state
        .auth_manager
        .exchange_pairing_code(&code_ctrl, "ControlDevice")
        .expect("pair ctrl");

    // 1. Declare active selection as session s1
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("ws1".into()),
        worktree_slug: None,
        worktree_label: Some("main".into()),
        session_id: Some(s1.clone()),
    });

    // 2. Connect WebSocket to active session s1 -> succeeds
    let mut ws1_stream =
        open_ws_stream(addr, &format!("/api/v1/terminal/{s1}"), Some(&token_ctrl)).await;

    // 3. Connect WebSocket to background session s2 -> fails with 403
    let s2_status =
        ws_handshake_status(addr, &format!("/api/v1/terminal/{s2}"), Some(&token_ctrl)).await;
    assert_eq!(s2_status, 403);

    // 4. Change active selection from s1 to s2
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("ws1".into()),
        worktree_slug: None,
        worktree_label: Some("main".into()),
        session_id: Some(s2.clone()),
    });

    // 5. Existing WebSocket connection on session s1 MUST be closed by server immediately
    let mut read_buf = [0u8; 1024];
    let read_result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        ws1_stream.read(&mut read_buf),
    )
    .await;

    match read_result {
        Ok(Ok(n)) => {
            // Either EOF (0 bytes) or WebSocket Close frame received
            assert!(
                n == 0 || (n >= 2 && (read_buf[0] & 0x0f == 0x08)),
                "Expected EOF (0) or WebSocket Close frame opcode 8, got {n} bytes: {:?}",
                &read_buf[..n]
            );
        }
        Ok(Err(e)) => {
            // Connection reset is also acceptable disconnect
            assert!(matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ));
        }
        Err(_) => {
            panic!("WebSocket on s1 timed out waiting for server to revoke/close connection after focus switched to s2");
        }
    }

    handle.stop();
    pty.close_session(&s1).await.unwrap();
    pty.close_session(&s2).await.unwrap();
}

#[tokio::test]
async fn test_daemon_owned_remote_chain_end_to_end() {
    // 1. Create test daemon with isolated paths
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");
    let (_daemon_dir, daemon_client, _server, server_task) =
        setup_test_daemon_with_remote_paths(Some(config_path), Some(auth_path)).await;

    // 2. Register temporary workspace repository
    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    crate::worktree::run_git(&repo_dir, &["init"]).expect("git init");
    crate::worktree::run_git(
        &repo_dir,
        &[
            "-c",
            "user.name=Test User",
            "-c",
            "user.email=test@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial commit",
        ],
    )
    .expect("git commit");

    let ws_id = "ws-e2e-chain";
    daemon_client
        .register_workspace(ws_id, repo_dir.to_str().unwrap())
        .await
        .expect("register workspace");

    // 3. Spawn real daemon PTY terminal
    let client_req_id = uuid::Uuid::new_v4().to_string();
    let session_id = daemon_client
        .spawn_terminal(
            client_req_id,
            ws_id.to_string(),
            None,
            Some(repo_dir.to_str().unwrap().to_string()),
            80,
            24,
        )
        .await
        .expect("spawn daemon terminal");

    // 4. Publish backend session as active selection
    let active_selection = RemoteActiveDesktopSelection {
        workspace_id: Some(ws_id.to_string()),
        worktree_slug: None,
        worktree_label: Some("main".to_string()),
        session_id: Some(session_id.clone()),
    };
    daemon_client
        .remote_set_active_selection(Some(active_selection))
        .await
        .expect("set active selection");

    // 5. Daemon-configure ephemeral Remote listener (port 0)
    daemon_client
        .remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
        })
        .await
        .expect("configure remote listener");

    let status = daemon_client
        .remote_get_status()
        .await
        .expect("remote status");
    assert!(status.is_running);
    assert_eq!(status.mode, RemoteNetworkMode::LocalNetwork);
    let bound_addr: std::net::SocketAddr = status
        .bound_address
        .expect("bound address")
        .parse()
        .unwrap();

    // 6. Create & exchange Control pairing code
    let pair_code = daemon_client
        .remote_create_pairing_code(Some(DevicePermission::Control))
        .await
        .expect("create pairing code");

    let (pair_status, pair_body) = http_request(
        bound_addr,
        "POST",
        "/api/v1/pair/exchange",
        None,
        Some(&format!(
            r#"{{"code":"{pair_code}","deviceName":"E2EControlDevice"}}"#
        )),
    )
    .await;
    assert_eq!(pair_status, 200);
    let pair_parsed: serde_json::Value = serde_json::from_str(&pair_body).expect("parse pair resp");
    let token = pair_parsed["token"].as_str().expect("token").to_string();

    // 7. Verify authenticated HTTP health & state exposes exactly that focused session
    let (health_status, health_body) =
        http_request(bound_addr, "GET", "/api/v1/health", None, None).await;
    assert_eq!(health_status, 200);
    assert!(health_body.contains(r#""status":"ok""#));

    let (sessions_status, sessions_body) =
        http_request(bound_addr, "GET", "/api/v1/sessions", Some(&token), None).await;
    assert_eq!(sessions_status, 200);
    let sessions: Vec<RemoteTerminalSession> =
        serde_json::from_str(&sessions_body).expect("parse sessions");
    assert_eq!(sessions.len(), 1, "Exactly one active session exposed");
    assert_eq!(sessions[0].session_id, session_id);
    assert_eq!(sessions[0].workspace_id.as_deref(), Some(ws_id));

    let (ws_state_status, ws_state_body) = http_request(
        bound_addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(ws_state_status, 200);
    let ws_state: RemoteWorkspaceState =
        serde_json::from_str(&ws_state_body).expect("parse ws state");
    assert_eq!(ws_state.sessions.len(), 1);
    assert_eq!(ws_state.sessions[0].session_id, session_id);

    // 8. Perform authenticated terminal WebSocket handshake / attach
    let ws_status = ws_handshake_status(
        bound_addr,
        &format!("/api/v1/terminal/{session_id}"),
        Some(&token),
    )
    .await;
    assert_eq!(
        ws_status, 101,
        "WebSocket attach to active terminal must upgrade with 101"
    );

    // 9. Cleanly disable & abort test daemon
    daemon_client
        .remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::Off,
            port: 0,
            allow_control: true,
        })
        .await
        .expect("disable remote listener");

    let disabled_status = daemon_client
        .remote_get_status()
        .await
        .expect("disabled status");
    assert!(!disabled_status.is_running);
    assert_eq!(disabled_status.mode, RemoteNetworkMode::Off);

    daemon_client
        .close_terminal(&session_id)
        .await
        .expect("close terminal");

    server_task.abort();
}

#[tokio::test]
async fn test_remote_terminal_forced_lag_replays_with_explicit_gap_and_sequence_metadata() {
    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::new(64));
    let terminal_service = crate::terminal::TerminalService::new(Arc::clone(&pty), Arc::clone(&hub));

    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("sleep 30");
    let (session_id, _pty_rx) = pty
        .spawn(command, 80, 24)
        .expect("spawn forced-lag session");
    hub.register_session_with_sequence(&session_id);

    let mut initial = terminal_service
        .attach_with_sequence(&session_id, None)
        .expect("initial sequenced attach");

    for index in 0..1_100u64 {
        hub.publish(&session_id, format!("chunk-{index:04};").into_bytes())
            .expect("publish output");
    }

    let lag = initial.receiver.recv().await;
    assert!(matches!(
        lag,
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
    ));

    let recovered = crate::remote::server::recover_remote_terminal_attachment(
        &terminal_service,
        &session_id,
        None,
    )
    .expect("remote lag recovery reattach");
    let gap = recovered.snapshot.gap.as_ref().expect("eviction must report gap");
    assert_eq!(gap.requested_after_sequence, 0);
    assert!(gap.available_from_sequence > 1);
    assert!(recovered.snapshot.history_start_sequence.is_some());
    assert!(recovered.snapshot.history_end_sequence.is_some());
    assert!(!recovered.snapshot.history.is_empty());

    let frame = crate::remote::server::encode_remote_terminal_snapshot_frame(
        &recovered.snapshot,
        true,
    );
    let prefix = b"\x1b]777;ferryx;";
    assert!(frame.starts_with(prefix));
    let metadata_end = frame[prefix.len()..]
        .iter()
        .position(|byte| *byte == 0x07)
        .map(|offset| prefix.len() + offset)
        .expect("OSC metadata terminator");
    let metadata: crate::remote::server::RemoteTerminalFrameMetadata =
        serde_json::from_slice(&frame[prefix.len()..metadata_end]).expect("frame metadata json");
    assert_eq!(metadata.kind, "replayGap");
    assert_eq!(metadata.requested_after_sequence.as_deref(), Some("0"));
    let expected_available = gap.available_from_sequence.to_string();
    assert_eq!(
        metadata.available_from_sequence.as_deref(),
        Some(expected_available.as_str())
    );
    let expected_end = recovered
        .snapshot
        .history_end_sequence
        .map(|sequence| sequence.to_string());
    assert_eq!(metadata.end_sequence.as_deref(), expected_end.as_deref());

    let frame_payload = &frame[metadata_end + 1..];
    assert!(frame_payload.starts_with(b"\x1bc"));
    assert_eq!(&frame_payload[2..], recovered.snapshot.history.as_slice());

    let live = hub
        .publish(&session_id, b"live-after-gap".to_vec())
        .expect("publish live output");
    let live_frame = crate::remote::server::encode_remote_terminal_output_frame(&live);
    let live_metadata_end = live_frame[prefix.len()..]
        .iter()
        .position(|byte| *byte == 0x07)
        .map(|offset| prefix.len() + offset)
        .expect("live metadata terminator");
    let live_metadata: crate::remote::server::RemoteTerminalFrameMetadata =
        serde_json::from_slice(&live_frame[prefix.len()..live_metadata_end])
            .expect("live frame metadata json");
    assert_eq!(live_metadata.kind, "output");
    let expected_live_sequence = live.sequence.to_string();
    assert_eq!(
        live_metadata.sequence.as_deref(),
        Some(expected_live_sequence.as_str())
    );
    assert_eq!(&live_frame[live_metadata_end + 1..], b"live-after-gap");

    pty.close_session(&session_id)
        .await
        .expect("cleanup forced-lag session");
}
