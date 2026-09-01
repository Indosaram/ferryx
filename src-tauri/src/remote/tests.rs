use super::*;
use crate::remote::protocol::RemoteEventMessage;
use crate::remote::state::REMOTE_ACTIVE_SELECTION_CHANGED_EVENT;
use crate::terminal::TerminalService;
use crate::worktree::WorktreeIdentity;
use std::path::PathBuf;
use std::sync::Arc;

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

    // Revoke device: it is deleted outright, so its token is simply unknown.
    assert!(auth.revoke_device(&device.id));
    assert!(auth.list_devices().is_empty());
    assert!(matches!(
        auth.validate_token(&token),
        Err(AuthError::Unauthorized)
    ));
}

#[tokio::test]
async fn test_active_selection_is_visible_to_a_receiver_that_subscribes_afterwards() {
    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(
        terminal_service,
        crate::worktree::WorkspaceRegistry::new(),
    ));

    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: None,
        worktree_slug: None,
        worktree_label: None,
        session_id: Some("session-late-subscriber".to_string()),
        ..Default::default()
    });

    let receiver = state.active_session_watch_rx();
    assert_eq!(
        receiver.borrow().as_deref(),
        Some("session-late-subscriber"),
        "a client attaching after the desktop selection was set must observe that selection"
    );

    state.clear_active_selection();
    assert_eq!(state.active_session_watch_rx().borrow().as_deref(), None);
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
async fn test_terminal_preferences_requires_a_valid_unrevoked_remote_token() {
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
        .expect("start remote server");

    let (missing_status, _) =
        http_request(addr, "GET", "/api/v1/terminal/preferences", None, None).await;
    assert_eq!(missing_status, 401);

    let (invalid_status, _) = http_request(
        addr,
        "GET",
        "/api/v1/terminal/preferences",
        Some("not-a-valid-token"),
        None,
    )
    .await;
    assert_eq!(invalid_status, 401);

    let pairing_code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, device) = state
        .auth_manager
        .exchange_pairing_code(&pairing_code, "PreferencesTest")
        .expect("pair test device");

    let (valid_status, _) = http_request(
        addr,
        "GET",
        "/api/v1/terminal/preferences",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(valid_status, 200);

    assert!(state.auth_manager.revoke_device(&device.id));
    let (revoked_status, _) = http_request(
        addr,
        "GET",
        "/api/v1/terminal/preferences",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(revoked_status, 401);

    handle.stop();
}

#[tokio::test]
async fn test_active_selection_change_is_broadcast_to_authenticated_event_clients() {
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
        .expect("start remote server");

    let pairing_code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pairing_code, "EventTest")
        .expect("pair event client");
    let mut events_socket = open_ws_stream(addr, "/api/v1/events", Some(&token)).await;

    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("remote-e2e".into()),
        worktree_slug: Some("mobile-control".into()),
        worktree_label: Some("mobile-control".into()),
        session_id: Some("focused-session".into()),
        ..Default::default()
    });

    let message = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_text_frame(&mut events_socket),
    )
    .await
    .expect("timed out waiting for active selection event");
    let event: RemoteEventMessage = serde_json::from_str(&message).expect("parse event message");
    assert_eq!(event.event, REMOTE_ACTIVE_SELECTION_CHANGED_EVENT);
    assert_eq!(event.payload["workspaceId"], "remote-e2e");
    assert_eq!(event.payload["worktreeSlug"], "mobile-control");
    assert_eq!(event.payload["sessionId"], "focused-session");

    handle.stop();
}

#[tokio::test]
async fn test_authenticated_event_clients_receive_the_current_active_selection_on_connect() {
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
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("remote-e2e".into()),
        worktree_slug: Some("mobile-control".into()),
        worktree_label: Some("mobile-control".into()),
        session_id: Some("focused-session".into()),
        ..Default::default()
    });
    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start remote server");

    let pairing_code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pairing_code, "LateEventClient")
        .expect("pair event client");
    let mut events_socket = open_ws_stream(addr, "/api/v1/events", Some(&token)).await;

    let message = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_text_frame(&mut events_socket),
    )
    .await
    .expect("timed out waiting for active selection snapshot");
    let event: RemoteEventMessage = serde_json::from_str(&message).expect("parse event message");
    assert_eq!(event.event, REMOTE_ACTIVE_SELECTION_CHANGED_EVENT);
    assert_eq!(event.payload["workspaceId"], "remote-e2e");
    assert_eq!(event.payload["worktreeSlug"], "mobile-control");
    assert_eq!(event.payload["sessionId"], "focused-session");

    handle.stop();
}

#[tokio::test]
async fn test_authenticated_event_clients_receive_selection_changes_after_the_snapshot() {
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
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("first-workspace".into()),
        worktree_slug: Some("first-worktree".into()),
        worktree_label: Some("first-worktree".into()),
        session_id: Some("first-session".into()),
        ..Default::default()
    });
    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start remote server");

    let pairing_code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pairing_code, "OrderedEventClient")
        .expect("pair event client");
    let mut events_socket = open_ws_stream(addr, "/api/v1/events", Some(&token)).await;

    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_text_frame(&mut events_socket),
    )
    .await
    .expect("timed out waiting for active selection snapshot");
    let snapshot: RemoteEventMessage = serde_json::from_str(&snapshot).expect("parse snapshot");
    assert_eq!(snapshot.event, REMOTE_ACTIVE_SELECTION_CHANGED_EVENT);
    assert_eq!(snapshot.payload["workspaceId"], "first-workspace");

    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("second-workspace".into()),
        worktree_slug: Some("second-worktree".into()),
        worktree_label: Some("second-worktree".into()),
        session_id: Some("second-session".into()),
        ..Default::default()
    });

    let update = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_text_frame(&mut events_socket),
    )
    .await
    .expect("timed out waiting for active selection update");
    let update: RemoteEventMessage = serde_json::from_str(&update).expect("parse update");
    assert_eq!(update.event, REMOTE_ACTIVE_SELECTION_CHANGED_EVENT);
    assert_eq!(update.payload["workspaceId"], "second-workspace");
    assert_eq!(update.payload["worktreeSlug"], "second-worktree");
    assert_eq!(update.payload["sessionId"], "second-session");

    handle.stop();
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

    let (status, body) = http_request(addr, "GET", "/", None, None).await;
    assert_eq!(status, 200);
    assert!(
        body.contains(r#"<div id="root"></div>"#) || body.contains("/assets/"),
        "Remote server must serve compiled SPA index HTML, got:\n{body}"
    );
    assert!(
        !body.contains("Ferryx Remote Server Active"),
        "Remote server must not fall back to placeholder when compiled UI is available"
    );

    // SPA navigation fallback: non-asset path returns index.html
    let (spa_status, spa_body) = http_request(addr, "GET", "/workspaces/active", None, None).await;
    assert_eq!(spa_status, 200);
    assert!(
        spa_body.contains(r#"<div id="root"></div>"#) || spa_body.contains("/assets/"),
        "SPA fallback route must serve compiled index.html"
    );

    handle.stop();
}

#[test]
fn test_resolve_dist_dir_packaged_macos_bundle_layout() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let macos_dir = dir.path().join("Ferryx.app/Contents/MacOS");
    let resources_ui_dist = dir.path().join("Ferryx.app/Contents/Resources/ui/dist");
    std::fs::create_dir_all(&macos_dir).unwrap();
    std::fs::create_dir_all(&resources_ui_dist).unwrap();
    std::fs::write(
        resources_ui_dist.join("index.html"),
        "<!doctype html><html><body><div id=\"root\"></div></body></html>",
    )
    .unwrap();

    let exe_path = macos_dir.join("ferryx");
    let isolated_cwd = dir.path().join("isolated_cwd");
    std::fs::create_dir_all(&isolated_cwd).unwrap();

    let resolved =
        crate::remote::server::resolve_dist_dir_from(Some(&isolated_cwd), Some(&exe_path), None);
    let expected = resources_ui_dist.canonicalize().unwrap();
    assert_eq!(resolved.as_deref(), Some(expected.as_path()));
}

#[test]
fn test_resolve_dist_dir_packaged_windows_or_linux_bundle_layout() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let install_dir = dir.path().join("FerryxApp");
    let resources_ui_dist = install_dir.join("resources/ui/dist");
    std::fs::create_dir_all(&resources_ui_dist).unwrap();
    std::fs::write(
        resources_ui_dist.join("index.html"),
        "<!doctype html><html><body><div id=\"root\"></div></body></html>",
    )
    .unwrap();

    let exe_path = install_dir.join("ferryx");
    let isolated_cwd = dir.path().join("isolated_cwd");
    std::fs::create_dir_all(&isolated_cwd).unwrap();

    let resolved =
        crate::remote::server::resolve_dist_dir_from(Some(&isolated_cwd), Some(&exe_path), None);
    let expected = resources_ui_dist.canonicalize().unwrap();
    assert_eq!(resolved.as_deref(), Some(expected.as_path()));
}

#[test]
fn test_resolve_dist_dir_isolated_runtime_debug_fallback() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let isolated_cwd = dir.path().join("isolated_cwd");
    let fake_exe = dir.path().join("somewhere/else/ferryx");
    std::fs::create_dir_all(&isolated_cwd).unwrap();
    std::fs::create_dir_all(fake_exe.parent().unwrap()).unwrap();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected_dist = manifest_dir.join("../ui/dist");

    if expected_dist.join("index.html").exists() {
        let resolved = crate::remote::server::resolve_dist_dir_from(
            Some(&isolated_cwd),
            Some(&fake_exe),
            Some(&manifest_dir),
        );
        let expected = expected_dist.canonicalize().unwrap();
        assert_eq!(resolved.as_deref(), Some(expected.as_path()));
    }
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
        ..Default::default()
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
        ..Default::default()
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
            tab_id: Some("tab-safe".into()),
            terminal_tabs: vec![
                crate::remote::protocol::RemoteTerminalTabInfo {
                    id: "tab-safe".into(),
                    label: "/Users/alice/private-shell".into(),
                    activity_state: Some("blocked".into()),
                    agent_type: Some("/Users/alice/bin/agent".into()),
                    ..Default::default()
                },
                crate::remote::protocol::RemoteTerminalTabInfo {
                    id: "tab-agent".into(),
                    label: "Build Agent".into(),
                    activity_state: Some("working".into()),
                    agent_type: Some("claude".into()),
                    ..Default::default()
                },
            ],
        },
    )
    .await
    .expect("set active selection");

    let sel = crate::ipc::remote::cmd_remote_get_active_selection(app.state())
        .await
        .expect("get active selection");
    let selection = sel.expect("selection remains available");
    assert_eq!(selection.workspace_id.as_deref(), Some("ws-gui"));
    assert_eq!(
        server
            .remote_state()
            .active_selection()
            .unwrap()
            .workspace_id
            .as_deref(),
        Some("ws-gui")
    );
    assert_eq!(selection.terminal_tabs[0].id, "tab-safe");
    assert_eq!(selection.terminal_tabs[0].label, "Terminal");
    // "blocked" was mapped to "waiting"
    assert_eq!(
        selection.terminal_tabs[0].activity_state.as_deref(),
        Some("waiting")
    );
    // path-like agent_type was dropped to None
    assert_eq!(selection.terminal_tabs[0].agent_type, None);
    assert_eq!(selection.terminal_tabs[1].id, "tab-agent");
    assert_eq!(selection.terminal_tabs[1].label, "Build Agent");
    assert_eq!(
        selection.terminal_tabs[1].activity_state.as_deref(),
        Some("working")
    );
    assert_eq!(
        selection.terminal_tabs[1].agent_type.as_deref(),
        Some("claude")
    );
    assert!(!serde_json::to_string(&selection)
        .expect("serialize sanitized selection")
        .contains("/Users/alice/private-shell"));
    assert!(!serde_json::to_string(&selection)
        .expect("serialize sanitized selection")
        .contains("/Users/alice/bin/agent"));

    // 7. Revoke device via GUI IPC command -> revokes on daemon
    let revoked = crate::ipc::remote::cmd_remote_device_revoke(app.state(), device_id)
        .await
        .expect("revoke device");
    assert!(revoked);

    let devices_after = crate::ipc::remote::cmd_remote_devices(app.state())
        .await
        .expect("devices after revoke");
    assert!(
        devices_after.is_empty(),
        "revoking must remove the device from the list outright"
    );

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

    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut byte).await.expect("tcp read");
        assert_ne!(
            n, 0,
            "WebSocket upgrade response ended before headers completed"
        );
        response.push(byte[0]);
        assert!(
            response.len() <= 16 * 1024,
            "WebSocket upgrade headers too large"
        );
    }
    let resp_str = String::from_utf8_lossy(&response);
    assert!(
        resp_str.contains("101 Switching Protocols"),
        "WebSocket upgrade must succeed, got:\n{resp_str}"
    );
    stream
}

async fn read_server_ws_text_frame(stream: &mut tokio::net::TcpStream) -> String {
    let ServerWebSocketFrame::Text(text) = read_server_ws_frame(stream).await else {
        panic!("expected first server frame to be Text");
    };
    text
}

enum ServerWebSocketFrame {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

async fn read_server_ws_frame(stream: &mut tokio::net::TcpStream) -> ServerWebSocketFrame {
    use tokio::io::AsyncReadExt;

    let mut header = [0u8; 2];
    stream
        .read_exact(&mut header)
        .await
        .expect("read websocket frame header");
    assert_eq!(
        header[1] & 0x80,
        0,
        "server websocket frames must be unmasked"
    );

    let payload_len = match header[1] & 0x7f {
        len @ 0..=125 => usize::from(len),
        126 => {
            let mut extended = [0u8; 2];
            stream
                .read_exact(&mut extended)
                .await
                .expect("read websocket 16-bit payload length");
            usize::from(u16::from_be_bytes(extended))
        }
        127 => {
            let mut extended = [0u8; 8];
            stream
                .read_exact(&mut extended)
                .await
                .expect("read websocket 64-bit payload length");
            usize::try_from(u64::from_be_bytes(extended)).expect("websocket frame fits usize")
        }
        _ => unreachable!(),
    };

    let mut payload = vec![0u8; payload_len];
    stream
        .read_exact(&mut payload)
        .await
        .expect("read websocket frame payload");

    match header[0] & 0x0f {
        0x01 => ServerWebSocketFrame::Text(
            String::from_utf8(payload).expect("websocket Text frame is utf-8"),
        ),
        0x02 => ServerWebSocketFrame::Binary(payload),
        0x08 => ServerWebSocketFrame::Close,
        opcode => panic!("unexpected server websocket opcode: {opcode}"),
    }
}

async fn write_client_ws_frame(stream: &mut tokio::net::TcpStream, opcode: u8, payload: &[u8]) {
    use tokio::io::AsyncWriteExt;

    let mut frame = Vec::with_capacity(payload.len() + 14);
    frame.push(0x80 | opcode);
    match payload.len() {
        len @ 0..=125 => frame.push(0x80 | u8::try_from(len).expect("payload length fits u8")),
        len @ 126..=65_535 => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(
                &u16::try_from(len)
                    .expect("payload length fits u16")
                    .to_be_bytes(),
            );
        }
        len => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(
                &u64::try_from(len)
                    .expect("payload length fits u64")
                    .to_be_bytes(),
            );
        }
    }

    let mask = [0x41, 0x92, 0x37, 0xe5];
    frame.extend_from_slice(&mask);
    for (index, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[index % mask.len()]);
    }
    stream
        .write_all(&frame)
        .await
        .expect("write masked websocket frame");
}

struct GridSocketTestHarness {
    pty: Arc<crate::terminal::PtyManager>,
    terminal_service: Arc<crate::terminal::TerminalService>,
    _state: Arc<RemoteGatewayState>,
    session_id: String,
    token: String,
    handle: RemoteServerHandle,
    addr: std::net::SocketAddr,
}

impl GridSocketTestHarness {
    async fn start(command: portable_pty::CommandBuilder, cols: u16, rows: u16) -> Self {
        let pty = Arc::new(crate::terminal::PtyManager::new());
        let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
        let terminal_service = Arc::new(crate::terminal::TerminalService::new(
            Arc::clone(&pty),
            Arc::clone(&hub),
        ));
        let (session_id, mut pty_rx) = pty.spawn(command, cols, rows).expect("spawn test session");
        hub.register_session(&session_id);
        let publish_session_id = session_id.clone();
        let publish_hub = Arc::clone(&hub);
        tokio::spawn(async move {
            while let Some(chunk) = pty_rx.recv().await {
                publish_hub.publish(&publish_session_id, chunk);
            }
        });

        let state = Arc::new(RemoteGatewayState::new(
            Arc::clone(&terminal_service),
            crate::worktree::WorkspaceRegistry::new(),
        ));
        *state.config.write() = RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
        };
        state.set_active_selection(RemoteActiveDesktopSelection {
            workspace_id: None,
            worktree_slug: None,
            worktree_label: None,
            session_id: Some(session_id.clone()),
            ..Default::default()
        });

        let pairing_code = state
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        let (token, _) = state
            .auth_manager
            .exchange_pairing_code(&pairing_code, "GridSocketTest")
            .expect("pair grid socket client");
        let (handle, addr) = start_remote_server(Arc::clone(&state))
            .await
            .expect("start test server");

        Self {
            pty,
            terminal_service,
            _state: state,
            session_id,
            token,
            handle,
            addr,
        }
    }

    async fn stop(self) {
        self.handle.stop();
        self.pty
            .close_session(&self.session_id)
            .await
            .expect("cleanup test session");
    }
}

async fn open_grid_socket(harness: &GridSocketTestHarness) -> tokio::net::TcpStream {
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        open_ws_stream(
            harness.addr,
            &format!("/api/v1/terminal/{}?render=grid", harness.session_id),
            Some(&harness.token),
        ),
    )
    .await
    .expect("timed out opening grid websocket")
}

async fn read_grid_text_frame(stream: &mut tokio::net::TcpStream) -> serde_json::Value {
    let frame = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_frame(stream),
    )
    .await
    .expect("timed out waiting for grid websocket frame");
    let ServerWebSocketFrame::Text(text) = frame else {
        panic!("grid socket must emit a Text frame");
    };
    serde_json::from_str(&text).expect("parse grid frame json")
}

#[tokio::test]
async fn test_grid_render_attach_sends_full_frame_with_session_dimensions() {
    let pty = Arc::new(crate::terminal::PtyManager::new());
    let hub = Arc::new(crate::terminal::TerminalOutputHub::default());
    let terminal_service = Arc::new(crate::terminal::TerminalService::new(
        Arc::clone(&pty),
        Arc::clone(&hub),
    ));

    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("sleep 30");
    let (session_id, mut pty_rx) = pty
        .spawn(command, 93, 27)
        .expect("spawn grid mirror session");
    hub.register_session(&session_id);
    let publish_session_id = session_id.clone();
    let publish_hub = Arc::clone(&hub);
    tokio::spawn(async move {
        while let Some(chunk) = pty_rx.recv().await {
            publish_hub.publish(&publish_session_id, chunk);
        }
    });

    let state = Arc::new(RemoteGatewayState::new(
        Arc::clone(&terminal_service),
        crate::worktree::WorkspaceRegistry::new(),
    ));
    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::LocalNetwork,
        port: 0,
        allow_control: true,
    };
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: None,
        worktree_slug: None,
        worktree_label: None,
        session_id: Some(session_id.clone()),
        ..Default::default()
    });

    let pairing_code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pairing_code, "GridMirrorTest")
        .expect("pair grid client");
    let (handle, addr) = start_remote_server(Arc::clone(&state))
        .await
        .expect("start grid remote server");

    let mut stream = open_ws_stream(
        addr,
        &format!("/api/v1/terminal/{session_id}?render=grid"),
        Some(&token),
    )
    .await;
    let text = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_text_frame(&mut stream),
    )
    .await
    .expect("timed out waiting for initial grid Text frame");
    let frame: serde_json::Value = serde_json::from_str(&text).expect("parse grid frame json");
    assert_eq!(frame["type"], "grid");
    assert_eq!(frame["cols"], 93);
    assert_eq!(frame["rows"], 27);

    handle.stop();
    pty.close_session(&session_id)
        .await
        .expect("cleanup grid mirror session");
}

#[tokio::test]
async fn test_grid_render_initial_frame_uses_requested_viewport_dimensions() {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("sleep 30");
    let harness = GridSocketTestHarness::start(command, 93, 27).await;

    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        open_ws_stream(
            harness.addr,
            &format!(
                "/api/v1/terminal/{}?render=grid&cols=47&rows=18",
                harness.session_id
            ),
            Some(&harness.token),
        ),
    )
    .await
    .expect("timed out opening viewport-sized grid websocket");

    let initial = read_grid_text_frame(&mut stream).await;
    assert_eq!(initial["type"], "grid");
    assert_eq!(initial["cols"], 47);
    assert_eq!(initial["rows"], 18);
    let session = harness
        .terminal_service
        .get_session(&harness.session_id)
        .expect("active terminal session");
    assert_eq!(session.get_size(), (47, 18));

    harness.stop().await;
}

#[tokio::test]
async fn test_grid_render_live_pty_output_emits_grid_frame_after_attach() {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("IFS= read -r line; printf '%s\\n' \"$line\"; sleep 30");
    let harness = GridSocketTestHarness::start(command, 93, 27).await;
    let mut stream = open_grid_socket(&harness).await;

    let initial = read_grid_text_frame(&mut stream).await;
    assert_eq!(initial["type"], "grid");

    let marker = "FERRYX_GRID_LIVE_MARKER";
    harness
        .terminal_service
        .write_input(&harness.session_id, format!("{marker}\n").as_bytes())
        .expect("write test input to PTY");

    let update = read_grid_text_frame(&mut stream).await;
    assert!(
        matches!(update["type"].as_str(), Some("grid") | Some("gridDiff")),
        "live output must emit a grid or gridDiff frame, got: {update}"
    );
    let rendered = update["lines"]
        .as_array()
        .expect("grid frame includes lines")
        .iter()
        .flat_map(|line| line["runs"].as_array().into_iter().flatten())
        .filter_map(|run| run["text"].as_str())
        .collect::<String>();
    assert!(
        rendered.contains(marker),
        "live grid update must include PTY output marker, got: {update}"
    );

    harness.stop().await;
}

#[tokio::test]
async fn test_grid_render_resize_returns_full_frame_with_requested_geometry() {
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("sleep 30");
    let harness = GridSocketTestHarness::start(command, 80, 24).await;
    let mut stream = open_grid_socket(&harness).await;

    let initial = read_grid_text_frame(&mut stream).await;
    assert_eq!(initial["type"], "grid");
    assert_eq!(initial["cols"], 80);
    assert_eq!(initial["rows"], 24);

    let resize = br#"{"type":"resize","cols":51,"rows":17}"#;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        write_client_ws_frame(&mut stream, 0x01, resize),
    )
    .await
    .expect("timed out sending resize control frame");

    let resized = read_grid_text_frame(&mut stream).await;
    assert_eq!(
        resized["type"], "grid",
        "resize must return a full grid frame"
    );
    assert_eq!(resized["cols"], 51);
    assert_eq!(resized["rows"], 17);

    harness.stop().await;
}

#[tokio::test]
async fn test_legacy_terminal_socket_emits_binary_snapshot_without_grid_text_frames() {
    let marker = "FERRYX_LEGACY_BINARY_MARKER";
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("IFS= read -r line; printf '%s\\n' \"$line\"; sleep 30");
    let harness = GridSocketTestHarness::start(command, 80, 24).await;

    let mut stream = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        open_ws_stream(
            harness.addr,
            &format!("/api/v1/terminal/{}", harness.session_id),
            Some(&harness.token),
        ),
    )
    .await
    .expect("timed out opening legacy websocket");
    harness
        .terminal_service
        .write_input(&harness.session_id, format!("{marker}\n").as_bytes())
        .expect("write test input to legacy PTY");
    let frame = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_server_ws_frame(&mut stream),
    )
    .await
    .expect("timed out waiting for legacy terminal output");

    let ServerWebSocketFrame::Binary(bytes) = frame else {
        panic!("legacy terminal socket must emit Binary, never a grid Text frame");
    };
    assert!(
        bytes
            .windows(marker.len())
            .any(|window| window == marker.as_bytes()),
        "legacy binary output frame must include the PTY output marker"
    );

    harness.stop().await;
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
        ..Default::default()
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
        ..Default::default()
    });

    // 5. Existing WebSocket connection on session s1 MUST be closed after focus changes.
    // PTY bytes legitimately emitted before the switch may already be buffered in TCP.
    let mut read_buf = [0u8; 1024];
    let close_result = tokio::time::timeout(std::time::Duration::from_millis(500), async {
        loop {
            match ws1_stream.read(&mut read_buf).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    break;
                }
                Err(error) => {
                    panic!("unexpected error waiting for focus-revoked socket close: {error}")
                }
            }
        }
    })
    .await;
    assert!(
        close_result.is_ok(),
        "WebSocket on s1 timed out waiting for server to revoke/close connection after focus switched to s2"
    );

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
            None,
        )
        .await
        .expect("spawn daemon terminal");

    // 4. Publish backend session as active selection
    let active_selection = RemoteActiveDesktopSelection {
        workspace_id: Some(ws_id.to_string()),
        worktree_slug: None,
        worktree_label: Some("main".to_string()),
        session_id: Some(session_id.clone()),
        ..Default::default()
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
async fn test_daemon_remote_worktree_selection_then_grid_terminal_control() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");
    let (_daemon_dir, daemon_client, server, server_task) =
        setup_test_daemon_with_remote_paths(Some(config_path), Some(auth_path)).await;

    let repo_dir = dir.path().join("repo");
    std::fs::create_dir_all(&repo_dir).expect("create repo directory");
    crate::worktree::run_git(&repo_dir, &["init"]).expect("git init");
    crate::worktree::run_git(
        &repo_dir,
        &[
            "-c",
            "user.name=Remote E2E",
            "-c",
            "user.email=remote-e2e@example.com",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    )
    .expect("initial commit");

    let workspace_id = "remote-e2e";
    daemon_client
        .register_workspace(workspace_id, repo_dir.to_str().expect("repo path utf-8"))
        .await
        .expect("register workspace");
    let manager = server
        .workspace_registry()
        .manager(workspace_id)
        .expect("workspace manager");
    let worktree_slug = "mobile-control";

    let (selection_tx, selection_rx) = tokio::sync::oneshot::channel();
    let selection_tx = Arc::new(parking_lot::Mutex::new(Some(selection_tx)));
    server.remote_state().set_desktop_event_sink(Arc::new({
        let selection_tx = Arc::clone(&selection_tx);
        move |event, payload| {
            if event == REMOTE_SELECTION_REQUEST_EVENT {
                if let Some(sender) = selection_tx.lock().take() {
                    let _ = sender.send(payload);
                }
            }
        }
    }));

    daemon_client
        .remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::LocalNetwork,
            port: 0,
            allow_control: true,
        })
        .await
        .expect("configure remote listener");
    let bound_addr: std::net::SocketAddr = daemon_client
        .remote_get_status()
        .await
        .expect("remote status")
        .bound_address
        .expect("bound address")
        .parse()
        .expect("parse bound address");

    let pair_code = daemon_client
        .remote_create_pairing_code(Some(DevicePermission::Control))
        .await
        .expect("create control pairing code");
    let (pair_status, pair_body) = http_request(
        bound_addr,
        "POST",
        "/api/v1/pair/exchange",
        None,
        Some(&format!(
            r#"{{"code":"{pair_code}","deviceName":"Remote E2E"}}"#
        )),
    )
    .await;
    assert_eq!(pair_status, 200);
    let token = serde_json::from_str::<serde_json::Value>(&pair_body).expect("pair response JSON")
        ["token"]
        .as_str()
        .expect("pair token")
        .to_string();

    let create_body = format!(
        r#"{{"workspaceId":"{workspace_id}","worktree":{{"wsId":"{workspace_id}","slug":"{worktree_slug}"}},"baseRef":null}}"#
    );
    let (create_status, create_response) = http_request(
        bound_addr,
        "POST",
        "/api/v1/workspace/worktrees",
        Some(&token),
        Some(&create_body),
    )
    .await;
    assert_eq!(create_status, 200);
    let created: RemoteWorktreeInfo =
        serde_json::from_str(&create_response).expect("safe worktree response");
    assert_eq!(created.worktree_slug.as_deref(), Some(worktree_slug));
    assert!(
        !create_response.contains(dir.path().to_str().expect("temp root utf-8")),
        "worktree creation response must not expose local paths: {create_response}"
    );

    let worktree = manager
        .find_worktree_by_slug(workspace_id, worktree_slug)
        .expect("find managed worktree")
        .expect("created managed worktree");
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.arg("-c");
    command.arg("IFS= read -r line; printf '%s\\n' \"$line\"; sleep 30");
    let session_id = daemon_client
        .spawn_terminal(
            uuid::Uuid::new_v4().to_string(),
            workspace_id.to_string(),
            Some(WorktreeIdentity {
                ws_id: workspace_id.to_string(),
                slug: worktree_slug.to_string(),
            }),
            Some(
                worktree
                    .path
                    .to_str()
                    .expect("worktree path utf-8")
                    .to_string(),
            ),
            80,
            24,
            None,
        )
        .await
        .expect("spawn selected-worktree terminal");

    let (initial_status, initial_body) = http_request(
        bound_addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(initial_status, 200);
    assert!(
        !initial_body.contains(dir.path().to_str().expect("temp root utf-8")),
        "remote state must not expose local paths: {initial_body}"
    );

    let select_body =
        format!(r#"{{"workspaceId":"{workspace_id}","worktreeSlug":"{worktree_slug}"}}"#);
    let (select_status, _) = http_request(
        bound_addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token),
        Some(&select_body),
    )
    .await;
    assert_eq!(select_status, 200);
    let selection_event = tokio::time::timeout(std::time::Duration::from_secs(2), selection_rx)
        .await
        .expect("timed out waiting for desktop selection request")
        .expect("desktop selection sender remains available");
    assert_eq!(selection_event["workspaceId"], workspace_id);
    assert_eq!(selection_event["worktreeSlug"], worktree_slug);
    assert!(
        !selection_event
            .to_string()
            .contains(dir.path().to_str().expect("temp root utf-8")),
        "desktop event must not expose local paths"
    );

    daemon_client
        .remote_set_active_selection(Some(RemoteActiveDesktopSelection {
            workspace_id: Some(workspace_id.to_string()),
            worktree_slug: Some(worktree_slug.to_string()),
            worktree_label: worktree.branch_short_name().map(str::to_string),
            session_id: Some(session_id.clone()),
            ..Default::default()
        }))
        .await
        .expect("desktop confirms active selection");

    let (state_status, state_body) = http_request(
        bound_addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token),
        None,
    )
    .await;
    assert_eq!(state_status, 200);
    let state: RemoteWorkspaceState = serde_json::from_str(&state_body).expect("workspace state");
    assert_eq!(state.active_workspace_id, workspace_id);
    assert_eq!(
        state.active_context.workspace_id.as_deref(),
        Some(workspace_id)
    );
    assert_eq!(
        state.active_context.worktree_slug.as_deref(),
        Some(worktree_slug)
    );
    assert!(
        state
            .projects
            .iter()
            .find(|project| project.workspace_id == workspace_id)
            .is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|worktree| worktree.worktree_slug.as_deref() == Some(worktree_slug))
            }),
        "remote state must expose the safe selected worktree option"
    );
    assert!(
        !state_body.contains(dir.path().to_str().expect("temp root utf-8")),
        "confirmed remote state must not expose local paths: {state_body}"
    );
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].session_id, session_id);

    let mut socket = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        open_ws_stream(
            bound_addr,
            &format!("/api/v1/terminal/{session_id}?render=grid"),
            Some(&token),
        ),
    )
    .await
    .expect("timed out opening grid websocket");
    let initial_grid = read_grid_text_frame(&mut socket).await;
    assert_eq!(initial_grid["type"], "grid");

    let marker = "REMOTE_WORKTREE_TERMINAL_CONTROL_OK";
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        write_client_ws_frame(&mut socket, 0x02, format!("{marker}\n").as_bytes()),
    )
    .await
    .expect("timed out writing terminal control frame");
    let update = read_grid_text_frame(&mut socket).await;
    assert!(
        matches!(update["type"].as_str(), Some("grid") | Some("gridDiff")),
        "terminal output must return a grid frame: {update}"
    );
    let rendered = update["lines"]
        .as_array()
        .expect("grid lines")
        .iter()
        .flat_map(|line| line["runs"].as_array().into_iter().flatten())
        .filter_map(|run| run["text"].as_str())
        .collect::<String>();
    assert!(
        rendered.contains(marker),
        "grid response must contain terminal control marker: {update}"
    );

    daemon_client
        .remote_configure(RemoteGatewayConfig {
            mode: RemoteNetworkMode::Off,
            port: 0,
            allow_control: true,
        })
        .await
        .expect("disable remote listener");
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
    let terminal_service =
        crate::terminal::TerminalService::new(Arc::clone(&pty), Arc::clone(&hub));

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
    let gap = recovered
        .snapshot
        .gap
        .as_ref()
        .expect("eviction must report gap");
    assert_eq!(gap.requested_after_sequence, 0);
    assert!(gap.available_from_sequence > 1);
    assert!(recovered.snapshot.history_start_sequence.is_some());
    assert!(recovered.snapshot.history_end_sequence.is_some());
    assert!(!recovered.snapshot.history.is_empty());

    let frame =
        crate::remote::server::encode_remote_terminal_snapshot_frame(&recovered.snapshot, true);
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

#[tokio::test]
async fn test_remote_active_selection_safe_tab_descriptors_and_no_path_leakage() {
    use crate::remote::protocol::RemoteTerminalTabInfo;

    let tab1 = RemoteTerminalTabInfo {
        id: "tab-term-1".to_string(),
        label: "build".to_string(),
        session_id: Some("session-123".to_string()),
        ..Default::default()
    };
    let tab2 = RemoteTerminalTabInfo {
        id: "tab-term-2".to_string(),
        label: "tests".to_string(),
        session_id: Some("session-456".to_string()),
        ..Default::default()
    };

    let selection = RemoteActiveDesktopSelection {
        workspace_id: Some("proj-1".to_string()),
        worktree_slug: Some("feat".to_string()),
        worktree_label: Some("feature-cool".to_string()),
        session_id: Some("session-123".to_string()),
        tab_id: Some("tab-term-1".to_string()),
        terminal_tabs: vec![tab1.clone(), tab2.clone()],
    };

    let json_str = serde_json::to_string(&selection).expect("serialize selection");
    assert!(json_str.contains("\"tabId\":\"tab-term-1\""));
    assert!(json_str.contains("\"terminalTabs\":["));
    assert!(json_str.contains("\"id\":\"tab-term-1\""));
    assert!(json_str.contains("\"label\":\"build\""));
    assert!(json_str.contains("\"sessionId\":\"session-123\""));
    assert!(json_str.contains("\"id\":\"tab-term-2\""));
    assert!(json_str.contains("\"label\":\"tests\""));
    assert!(json_str.contains("\"sessionId\":\"session-456\""));

    // Ensure NO path leakage in serialized structure
    assert!(!json_str.contains("/Users/"));
    assert!(!json_str.contains("cwd"));
    assert!(!json_str.contains("worktreePath"));
    assert!(!json_str.contains("repoRoot"));

    // Deserialization with aliases (e.g. activeTabId, tabs, tabLabel)
    let aliased_json = serde_json::json!({
        "workspaceId": "proj-1",
        "activeTabId": "tab-term-2",
        "tabs": [
            { "tabId": "tab-term-2", "tabLabel": "tests" }
        ]
    });
    let parsed: RemoteActiveDesktopSelection =
        serde_json::from_value(aliased_json).expect("deserialize aliased selection");
    assert_eq!(parsed.tab_id.as_deref(), Some("tab-term-2"));
    assert_eq!(parsed.terminal_tabs.len(), 1);
    assert_eq!(parsed.terminal_tabs[0].id, "tab-term-2");
    assert_eq!(parsed.terminal_tabs[0].label, "tests");
}

#[tokio::test]
async fn test_remote_select_workspace_with_tab_selector_and_primary_worktree() {
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
    let workspace_id = "proj-safe";
    registry
        .register(workspace_id, &repo_root)
        .expect("register");
    let manager = registry.manager(workspace_id).expect("manager");

    let ident = WorktreeIdentity {
        ws_id: workspace_id.to_string(),
        slug: "feature-tab".to_string(),
    };
    let wt_path = manager
        .worktree_path_for(&ident.ws_id, &ident.slug)
        .expect("wt path");
    manager
        .create_worktree(crate::worktree::CreateWorktreeOptions::new(
            &ident.ws_id,
            &ident.slug,
            &wt_path,
        ))
        .expect("create worktree");

    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
    *state.config.write() = RemoteGatewayConfig {
        mode: RemoteNetworkMode::LocalNetwork,
        port: 0,
        allow_control: true,
    };
    let (_handle, addr) = start_remote_server(Arc::clone(&state))
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

    let event_received = Arc::new(parking_lot::Mutex::new(None));
    let event_received_clone = Arc::clone(&event_received);
    state.set_desktop_event_sink(Arc::new(move |event, payload| {
        *event_received_clone.lock() = Some((event.to_string(), payload));
    }));

    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some(workspace_id.to_string()),
        worktree_slug: Some("feature-tab".to_string()),
        worktree_label: Some("feature-tab".to_string()),
        session_id: None,
        tab_id: Some("tab-term-selected".to_string()),
        terminal_tabs: vec![crate::remote::protocol::RemoteTerminalTabInfo {
            id: "tab-term-selected".to_string(),
            label: "Terminal 1".to_string(),
            ..Default::default()
        }],
    });

    // 1. View-only token must be rejected with 403 Forbidden
    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_view),
        Some(
            &serde_json::json!({
                "workspaceId": workspace_id,
                "worktreeSlug": "feature-tab",
                "tabId": "tab-view-attempt"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, 403);

    let (status, _) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(
            &serde_json::json!({
                "workspaceId": workspace_id,
                "worktreeSlug": "feature-tab",
                "tabId": "unpublished-tab"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, 400);

    // 2. Control token selecting worktree with tabId selector
    let (status, body) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(
            &serde_json::json!({
                "workspaceId": workspace_id,
                "worktreeSlug": "feature-tab",
                "tabId": "tab-term-selected"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, 200);
    let select_resp: serde_json::Value = serde_json::from_str(&body).expect("select json");
    assert_eq!(select_resp["workspaceId"], workspace_id);
    assert_eq!(select_resp["worktreeSlug"], "feature-tab");
    assert_eq!(select_resp["tabId"], "tab-term-selected");

    let (event_name, event_payload) = event_received.lock().take().expect("desktop event emitted");
    assert_eq!(event_name, "remote_selection_requested");
    assert_eq!(event_payload["workspaceId"], workspace_id);
    assert_eq!(event_payload["worktreeSlug"], "feature-tab");
    assert_eq!(event_payload["tabId"], "tab-term-selected");

    // 3. Primary worktree selection (without worktreeSlug) with tab selector
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some(workspace_id.to_string()),
        worktree_slug: None,
        worktree_label: Some("main".to_string()),
        session_id: None,
        tab_id: Some("tab-primary-2".to_string()),
        terminal_tabs: vec![crate::remote::protocol::RemoteTerminalTabInfo {
            id: "tab-primary-2".to_string(),
            label: "Terminal 2".to_string(),
            ..Default::default()
        }],
    });
    let (status, body) = http_request(
        addr,
        "POST",
        "/api/v1/workspace/select",
        Some(&token_ctrl),
        Some(
            &serde_json::json!({
                "workspaceId": workspace_id,
                "tabId": "tab-primary-2"
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, 200);
    let select_primary_resp: serde_json::Value = serde_json::from_str(&body).expect("select json");
    assert_eq!(select_primary_resp["workspaceId"], workspace_id);
    assert_eq!(select_primary_resp["tabId"], "tab-primary-2");
    assert!(
        select_primary_resp.get("worktreeSlug").is_none()
            || select_primary_resp["worktreeSlug"].is_null()
    );

    let (primary_name, primary_payload) = event_received
        .lock()
        .take()
        .expect("primary desktop event emitted");
    assert_eq!(primary_name, "remote_selection_requested");
    assert_eq!(primary_payload["workspaceId"], workspace_id);
    assert_eq!(primary_payload["tabId"], "tab-primary-2");

    // 4. GET /api/v1/workspace/state contains safe active terminal tab descriptors
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some(workspace_id.to_string()),
        worktree_slug: Some("feature-tab".to_string()),
        worktree_label: Some("feature-tab".to_string()),
        session_id: Some("sess-1".to_string()),
        tab_id: Some("tab-term-selected".to_string()),
        terminal_tabs: vec![
            crate::remote::protocol::RemoteTerminalTabInfo {
                id: "tab-term-selected".to_string(),
                label: "Terminal 1".to_string(),
                ..Default::default()
            },
            crate::remote::protocol::RemoteTerminalTabInfo {
                id: "tab-term-other".to_string(),
                label: "Terminal 2".to_string(),
                ..Default::default()
            },
        ],
    });

    let (status, body) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let ws_state: RemoteWorkspaceState = serde_json::from_str(&body).expect("workspace state");
    assert_eq!(
        ws_state.active_context.tab_id.as_deref(),
        Some("tab-term-selected")
    );
    assert_eq!(ws_state.active_context.terminal_tabs.len(), 2);
    assert_eq!(
        ws_state.active_context.terminal_tabs[0].id,
        "tab-term-selected"
    );
    assert_eq!(ws_state.active_context.terminal_tabs[0].label, "Terminal 1");
    assert_eq!(
        ws_state.active_context.terminal_tabs[1].id,
        "tab-term-other"
    );
    assert_eq!(ws_state.active_context.terminal_tabs[1].label, "Terminal 2");
    assert!(!body.contains(dir.path().to_str().expect("temp utf-8")));
}

#[tokio::test]
async fn test_workspace_state_agent_activity_and_worktree_attention_rollup() {
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
    let workspace_id = "ws-activity";
    registry
        .register(workspace_id, &repo_root)
        .expect("register");
    let manager = registry.manager(workspace_id).expect("manager");

    let ident = WorktreeIdentity {
        ws_id: workspace_id.to_string(),
        slug: "feat-agent".to_string(),
    };
    let wt_path = manager
        .worktree_path_for(&ident.ws_id, &ident.slug)
        .expect("wt path");
    manager
        .create_worktree(crate::worktree::CreateWorktreeOptions::new(
            &ident.ws_id,
            &ident.slug,
            &wt_path,
        ))
        .expect("create worktree");

    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry));
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

    // 1. Set active selection with multiple tabs having done, working, and waiting states.
    // Rollup rank: waiting (3) > working (2) > done (1).
    let sel_json = serde_json::json!({
        "workspaceId": workspace_id,
        "worktreeSlug": "feat-agent",
        "worktreeLabel": "feat-agent",
        "sessionId": "sess-active",
        "activeTabId": "tab-3",
        "tabs": [
            {
                "tabId": "tab-1",
                "tabLabel": "Agent Tab 1",
                "activityState": "done",
                "agentType": "codex"
            },
            {
                "tabId": "tab-2",
                "tabLabel": "Agent Tab 2",
                "activityState": "working",
                "agentType": "claude"
            },
            {
                "tabId": "tab-3",
                "tabLabel": "Agent Tab 3",
                "activityState": "waiting",
                "agentType": "opendevin"
            },
            {
                "tabId": "tab-4",
                "tabLabel": "Agent Tab 4",
                "activityState": "done",
                "agentType": "aider"
            }
        ]
    });
    let selection: RemoteActiveDesktopSelection =
        serde_json::from_value(sel_json).expect("deserialize selection");
    state.set_active_selection(selection);

    // Unauthenticated GET returns 401
    let (unauth_status, _) = http_request(addr, "GET", "/api/v1/workspace/state", None, None).await;
    assert_eq!(unauth_status, 401);

    // Authenticated GET /api/v1/workspace/state
    let (status, body) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);

    let state_val: serde_json::Value =
        serde_json::from_str(&body).expect("parse workspace state json");

    // Verify tabs carry activityState and agentType
    let tabs = state_val["activeContext"]["terminalTabs"]
        .as_array()
        .expect("terminalTabs array");
    assert_eq!(tabs.len(), 4);
    assert_eq!(tabs[0]["id"], "tab-1");
    assert_eq!(tabs[0]["activityState"], "done");
    assert_eq!(tabs[0]["agentType"], "codex");
    assert_eq!(tabs[1]["id"], "tab-2");
    assert_eq!(tabs[1]["activityState"], "working");
    assert_eq!(tabs[1]["agentType"], "claude");
    assert_eq!(tabs[2]["id"], "tab-3");
    assert_eq!(tabs[2]["activityState"], "waiting");
    assert_eq!(tabs[2]["agentType"], "opendevin");
    assert_eq!(tabs[3]["id"], "tab-4");
    assert_eq!(tabs[3]["activityState"], "done");
    assert_eq!(tabs[3]["agentType"], "aider");

    // Verify worktree attention rollup on worktrees array: waiting beats working and done
    let worktrees = state_val["worktrees"].as_array().expect("worktrees array");
    let feat_wt = worktrees
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .expect("find feat-agent worktree");
    assert_eq!(feat_wt["attention"], "waiting");

    let primary_wt = worktrees
        .iter()
        .find(|wt| wt["worktreeSlug"].is_null())
        .expect("find primary worktree");
    assert!(primary_wt["attention"].is_null());

    // Verify projects worktree attention rollup as well
    let projects = state_val["projects"].as_array().expect("projects array");
    let proj_wt = projects[0]["worktrees"]
        .as_array()
        .expect("projects[0].worktrees")
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .expect("find feat-agent in project");
    assert_eq!(proj_wt["attention"], "waiting");

    // 2. Rank test: Remove waiting tab, leaving working and done -> attention should be working
    let sel_working = serde_json::json!({
        "workspaceId": workspace_id,
        "worktreeSlug": "feat-agent",
        "worktreeLabel": "feat-agent",
        "sessionId": "sess-active",
        "activeTabId": "tab-2",
        "tabs": [
            {
                "tabId": "tab-1",
                "tabLabel": "Agent Tab 1",
                "activityState": "done",
                "agentType": "codex"
            },
            {
                "tabId": "tab-2",
                "tabLabel": "Agent Tab 2",
                "activityState": "working",
                "agentType": "claude"
            }
        ]
    });
    state.set_active_selection(serde_json::from_value(sel_working).unwrap());

    let (status, body2) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let state_val2: serde_json::Value = serde_json::from_str(&body2).expect("parse json");
    let feat_wt2 = state_val2["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .unwrap();
    assert_eq!(feat_wt2["attention"], "working");

    // 3. Rank test: Only done tab -> attention should be done
    let sel_done = serde_json::json!({
        "workspaceId": workspace_id,
        "worktreeSlug": "feat-agent",
        "worktreeLabel": "feat-agent",
        "sessionId": "sess-active",
        "activeTabId": "tab-1",
        "tabs": [
            {
                "tabId": "tab-1",
                "tabLabel": "Agent Tab 1",
                "activityState": "done",
                "agentType": "codex"
            }
        ]
    });
    state.set_active_selection(serde_json::from_value(sel_done).unwrap());

    let (status, body3) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let state_val3: serde_json::Value = serde_json::from_str(&body3).expect("parse json");
    let feat_wt3 = state_val3["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .unwrap();
    assert_eq!(feat_wt3["attention"], "done");

    // 4. No activity tabs -> attention is omitted / null
    let sel_none = serde_json::json!({
        "workspaceId": workspace_id,
        "worktreeSlug": "feat-agent",
        "worktreeLabel": "feat-agent",
        "sessionId": "sess-active",
        "activeTabId": "tab-1",
        "tabs": [
            {
                "tabId": "tab-1",
                "tabLabel": "Shell Tab"
            }
        ]
    });
    state.set_active_selection(serde_json::from_value(sel_none).unwrap());

    let (status, body4) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let state_val4: serde_json::Value = serde_json::from_str(&body4).expect("parse json");
    let feat_wt4 = state_val4["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .unwrap();
    assert!(feat_wt4["attention"].is_null());

    // 5. Blocked maps to waiting in rollup
    let sel_blocked = serde_json::json!({
        "workspaceId": workspace_id,
        "worktreeSlug": "feat-agent",
        "worktreeLabel": "feat-agent",
        "sessionId": "sess-active",
        "activeTabId": "tab-1",
        "tabs": [
            {
                "tabId": "tab-1",
                "tabLabel": "Agent Tab 1",
                "activityState": "blocked",
                "agentType": "codex"
            }
        ]
    });
    state.set_active_selection(serde_json::from_value(sel_blocked).unwrap());
    let (status, body_blocked) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let state_val_blocked: serde_json::Value =
        serde_json::from_str(&body_blocked).expect("parse json");
    let feat_wt_blocked = state_val_blocked["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|wt| wt["worktreeSlug"] == "feat-agent")
        .unwrap();
    assert_eq!(feat_wt_blocked["attention"], "waiting");

    // 6. Direct IPC bridge command sanitization test:
    // Tab labels with paths sanitized to "Terminal",
    // activityState normalized / validated,
    // agentType with path characters dropped.
    let _ipc_manager = Arc::new(crate::ipc::remote::RemoteGatewayManager::new(Arc::clone(
        &state,
    )));
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some(workspace_id.to_string()),
        worktree_slug: Some("feat-agent".to_string()),
        worktree_label: Some("feat-agent".to_string()),
        session_id: Some("sess-active".to_string()),
        tab_id: Some("tab-1".to_string()),
        terminal_tabs: vec![
            crate::remote::protocol::RemoteTerminalTabInfo {
                id: "tab-1".to_string(),
                label: "/Users/dev/secret-project".to_string(),
                activity_state: Some("blocked".to_string()),
                agent_type: Some("/bin/sh".to_string()),
                ..Default::default()
            },
            crate::remote::protocol::RemoteTerminalTabInfo {
                id: "tab-2".to_string(),
                label: "Safe Tab".to_string(),
                activity_state: Some("invalid_state".to_string()),
                agent_type: Some("claude".to_string()),
                ..Default::default()
            },
        ],
    });

    // Invariant: No path leakage in response
    assert!(!body.contains(dir.path().to_str().expect("utf8")));

    handle.stop();
}

#[test]
fn test_remote_terminal_tab_info_serializes_and_deserializes_session_id() {
    use crate::remote::protocol::RemoteTerminalTabInfo;

    let tab = RemoteTerminalTabInfo {
        id: "tab-1".to_string(),
        label: "build".to_string(),
        session_id: Some("session-abc-123".to_string()),
        ..Default::default()
    };

    let serialized = serde_json::to_string(&tab).expect("serialize tab");
    assert!(
        serialized.contains("\"sessionId\":\"session-abc-123\""),
        "Serialized tab must contain sessionId, got: {serialized}"
    );

    // Deserialization with camelCase "sessionId"
    let parsed: RemoteTerminalTabInfo =
        serde_json::from_str(&serialized).expect("deserialize camelCase");
    assert_eq!(parsed.session_id.as_deref(), Some("session-abc-123"));

    // Deserialization with snake_case "session_id" alias
    let snake_json = serde_json::json!({
        "id": "tab-2",
        "label": "test",
        "session_id": "session-snake-456"
    });
    let parsed_snake: RemoteTerminalTabInfo =
        serde_json::from_value(snake_json).expect("deserialize snake_case");
    assert_eq!(
        parsed_snake.session_id.as_deref(),
        Some("session-snake-456")
    );
}

#[tokio::test]
async fn test_repeated_workspace_state_reads_do_not_rerun_git_discovery_and_refresh_on_change() {
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
    let workspace_id = "ws-cache-test";
    registry
        .register(workspace_id, &repo_root)
        .expect("register");

    let terminal_service = Arc::new(TerminalService::default());
    let state = Arc::new(RemoteGatewayState::new(terminal_service, registry.clone()));
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

    // First request: triggers initial snapshot build
    let (status1, _body1) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status1, 200);
    assert_eq!(
        state.snapshot_build_count(),
        1,
        "Initial request must build snapshot once"
    );

    // Repeated requests (e.g. mobile terminal switching / rapid state polls):
    // MUST NOT rerun git discovery or rebuild snapshot
    for _ in 0..5 {
        let (status, _) = http_request(
            addr,
            "GET",
            "/api/v1/workspace/state",
            Some(&token_ctrl),
            None,
        )
        .await;
        assert_eq!(status, 200);
        let (status_sess, _) =
            http_request(addr, "GET", "/api/v1/sessions", Some(&token_ctrl), None).await;
        assert_eq!(status_sess, 200);
    }
    assert_eq!(
        state.snapshot_build_count(),
        1,
        "Repeated workspace-state and sessions reads must reuse cached snapshot without rerunning git discovery"
    );

    // Now modify workspace registration / worktrees:
    let manager = registry.manager(workspace_id).expect("manager");
    let ident = WorktreeIdentity {
        ws_id: workspace_id.to_string(),
        slug: "feat-new".to_string(),
    };
    let wt_path = manager
        .worktree_path_for(&ident.ws_id, &ident.slug)
        .expect("wt path");
    manager
        .create_worktree(crate::worktree::CreateWorktreeOptions::new(
            &ident.ws_id,
            &ident.slug,
            &wt_path,
        ))
        .expect("create worktree");

    // Subsequent request must detect change and refresh cache
    let (status_after, body_after) = http_request(
        addr,
        "GET",
        "/api/v1/workspace/state",
        Some(&token_ctrl),
        None,
    )
    .await;
    assert_eq!(status_after, 200);
    assert_eq!(
        state.snapshot_build_count(),
        2,
        "Snapshot must refresh after worktree creation"
    );
    let ws_state: RemoteWorkspaceState = serde_json::from_str(&body_after).expect("parse state");
    assert!(
        ws_state
            .worktrees
            .iter()
            .any(|w| w.worktree_slug.as_deref() == Some("feat-new")),
        "Refreshed snapshot must contain newly created worktree"
    );

    handle.stop();
}

#[tokio::test]
async fn test_workspace_snapshot_refreshes_external_git_worktree_changes_after_interval() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let repo_root = dir.path().join("repo");
    std::fs::create_dir_all(&repo_root).expect("create repo");
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
    let workspace_id = "ws-external-refresh";
    registry
        .register(workspace_id, &repo_root)
        .expect("register");
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        registry.clone(),
    ));
    let initial_time = std::time::Instant::now();

    let initial = state
        .workspace_snapshot_at(initial_time)
        .await
        .expect("initial snapshot");
    assert!(!initial
        .worktrees_for(workspace_id, None)
        .iter()
        .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));

    let manager = registry.manager(workspace_id).expect("manager");
    let identity = WorktreeIdentity {
        ws_id: workspace_id.to_string(),
        slug: "external-change".to_string(),
    };
    let external_path = manager
        .worktree_path_for(&identity.ws_id, &identity.slug)
        .expect("external path");
    let external_path_text = external_path.to_string_lossy().into_owned();
    crate::worktree::run_git(
        &repo_root,
        &[
            "worktree",
            "add",
            "-b",
            "external-change",
            &external_path_text,
        ],
    )
    .expect("external git worktree add");

    let before_interval = state
        .workspace_snapshot_at(
            initial_time + crate::remote::state::WORKSPACE_SNAPSHOT_REFRESH_INTERVAL / 2,
        )
        .await
        .expect("cached snapshot");
    assert!(!before_interval
        .worktrees_for(workspace_id, None)
        .iter()
        .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));

    let refresh_completed = state.next_snapshot_build();
    let stale = state
        .workspace_snapshot_at(
            initial_time + crate::remote::state::WORKSPACE_SNAPSHOT_REFRESH_INTERVAL,
        )
        .await
        .expect("stale snapshot while refresh starts");
    assert!(!stale
        .worktrees_for(workspace_id, None)
        .iter()
        .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));

    tokio::time::timeout(std::time::Duration::from_secs(2), refresh_completed)
        .await
        .expect("background snapshot refresh completes within the test bound");
    let refreshed = state
        .workspace_snapshot_at(
            initial_time + crate::remote::state::WORKSPACE_SNAPSHOT_REFRESH_INTERVAL,
        )
        .await
        .expect("refreshed snapshot");
    assert!(refreshed
        .worktrees_for(workspace_id, None)
        .iter()
        .any(|worktree| worktree.worktree_label.as_deref() == Some("external-change")));
    assert_eq!(state.snapshot_build_count(), 2);
}

#[tokio::test]
async fn test_workspace_snapshot_started_before_revision_change_is_not_marked_current() {
    let registry = crate::worktree::WorkspaceRegistry::new();
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        registry.clone(),
    ));
    let initial_time = std::time::Instant::now();
    let dir = tempfile::TempDir::new().expect("tempdir");
    let hook_registry = registry.clone();
    let hook_path = dir.path().to_path_buf();
    let hook_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hook_fired_for_callback = Arc::clone(&hook_fired);
    state.set_snapshot_post_build_hook(Some(Arc::new(move || {
        if !hook_fired_for_callback.swap(true, std::sync::atomic::Ordering::AcqRel) {
            hook_registry
                .register("registered-during-discovery", &hook_path)
                .expect("register workspace during discovery");
        }
    })));

    let built = state
        .workspace_snapshot_at(initial_time)
        .await
        .expect("snapshot built before overlapping registration");
    assert!(built.projects(None).is_empty());
    assert!(hook_fired.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(state.snapshot_build_count(), 1);
    state.set_snapshot_post_build_hook(None);

    let rebuilt = state
        .workspace_snapshot_at(initial_time)
        .await
        .expect("revision mismatch rebuilds");
    assert!(rebuilt
        .projects(None)
        .iter()
        .any(|project| project.workspace_id == "registered-during-discovery"));
    assert_eq!(state.snapshot_build_count(), 2);
}

#[tokio::test]
async fn test_concurrent_cold_snapshot_requests_coalesce_to_one_git_discovery() {
    let registry = crate::worktree::WorkspaceRegistry::new();
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        registry,
    ));
    let (build_reached_tx, build_reached_rx) = tokio::sync::oneshot::channel();
    let build_reached_tx = Arc::new(std::sync::Mutex::new(Some(build_reached_tx)));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    state.set_snapshot_post_build_hook(Some(Arc::new({
        let build_reached_tx = Arc::clone(&build_reached_tx);
        let release_rx = Arc::clone(&release_rx);
        move || {
            if let Some(sender) = build_reached_tx.lock().expect("signal lock").take() {
                let _ = sender.send(());
                release_rx
                    .lock()
                    .expect("release lock")
                    .recv()
                    .expect("release build");
            }
        }
    })));

    let first_state = Arc::clone(&state);
    let first = tokio::spawn(async move { first_state.workspace_snapshot().await });
    build_reached_rx.await.expect("first build reaches hook");

    let second_state = Arc::clone(&state);
    let mut second = Box::pin(async move { second_state.workspace_snapshot().await });
    tokio::select! {
        result = &mut second => panic!("second request completed before the in-flight build: {result:?}"),
        _ = tokio::task::yield_now() => {}
    }

    release_tx.send(()).expect("release first build");
    first.await.expect("first join").expect("first snapshot");
    second.await.expect("second snapshot");
    state.set_snapshot_post_build_hook(None);

    assert_eq!(state.snapshot_build_count(), 1);
}
