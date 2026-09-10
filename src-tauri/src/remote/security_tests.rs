use super::*;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

#[path = "security_socket_tests.rs"]
mod sockets;

struct SecurityServer {
    addr: std::net::SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl SecurityServer {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        Self::start_router(create_remote_router(state)).await
    }

    async fn start_router(router: axum::Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { addr, tasks }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
    ) -> (u16, String) {
        tokio::time::timeout(DEADLINE, http_request(self.addr, method, path, token, body))
            .await
            .expect("bounded HTTP response")
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

fn state() -> Arc<RemoteGatewayState> {
    Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        crate::worktree::WorkspaceRegistry::new(),
    ))
}

fn pair(
    state: &RemoteGatewayState,
    permission: DevicePermission,
) -> (String, crate::remote::auth::DeviceInfo) {
    let code = state.auth_manager.create_pairing_code(permission);
    state
        .auth_manager
        .exchange_pairing_code(&code, "Security fixture")
        .unwrap()
}

#[tokio::test]
async fn static_raw_traversal_is_rejected() {
    // Given an actual dist directory and a private sibling file, not a system secret.
    let dist = crate::remote::server::resolve_dist_dir();
    let outside = tempfile::tempdir_in(dist.parent().unwrap()).unwrap();
    std::fs::write(outside.path().join("secret.txt"), "PRIVATE_SENTINEL").unwrap();
    let server = SecurityServer::start(state()).await;
    let outside_name = outside.path().file_name().unwrap().to_str().unwrap();
    // When raw HTTP bypasses URL client normalization.
    let (status, body) = server
        .request("GET", &format!("/../{outside_name}/secret.txt"), None, None)
        .await;
    server.stop().await;
    // Then no bytes outside the asset root can be returned.
    assert_eq!(status, 400, "traversal returned {body}");
    assert!(!body.contains("PRIVATE_SENTINEL"));
}

#[cfg(unix)]
#[tokio::test]
async fn static_symlinks_cannot_escape_dist() {
    let dist = crate::remote::server::resolve_dist_dir();
    let assets = tempfile::tempdir_in(&dist).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "SYMLINK_SENTINEL").unwrap();
    std::os::unix::fs::symlink(outside.path(), assets.path().join("escape")).unwrap();
    let server = SecurityServer::start(state()).await;
    let assets_name = assets.path().file_name().unwrap().to_str().unwrap();
    let (status, body) = server
        .request(
            "GET",
            &format!("/{assets_name}/escape/secret.txt"),
            None,
            None,
        )
        .await;
    server.stop().await;
    assert_eq!(status, 404, "symlink escape returned {body}");
    assert!(!body.contains("SYMLINK_SENTINEL"));
}

#[tokio::test]
async fn static_portable_paths_and_assets() {
    let dist = crate::remote::server::resolve_dist_dir();
    let assets = tempfile::tempdir_in(&dist).unwrap();
    std::fs::write(assets.path().join("hello world.js"), "ASSET_SENTINEL").unwrap();
    let server = SecurityServer::start(state()).await;
    for path in [
        "/%2e%2e/secret",
        "/..%2fsecret",
        "/%252e%252e/secret",
        "/..\\secret",
        "/%5c%5cserver/share",
        "/C:/secret",
        "/C:secret",
        "//server/share",
        "/%00",
        "/%zz",
    ] {
        let (status, _) = server.request("GET", path, None, None).await;
        assert_eq!(status, 400, "unsafe portable path {path}");
    }
    let assets_name = assets.path().file_name().unwrap().to_str().unwrap();
    let response = server
        .request(
            "GET",
            &format!("/{assets_name}/hello%20world.js"),
            None,
            None,
        )
        .await;
    assert_eq!(response, (200, "ASSET_SENTINEL".into()));
    let (_, index) = server.request("GET", "/", None, None).await;
    let fallback = server
        .request("GET", "/workspaces/not-an-asset", None, None)
        .await;
    assert_eq!(fallback, (200, index));
    server.stop().await;
}

#[tokio::test]
async fn view_cannot_revoke_another_device() {
    let state = state();
    let (view_token, _) = pair(&state, DevicePermission::View);
    let (control_token, control) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    let (status, _) = server
        .request(
            "POST",
            &format!("/api/v1/devices/{}/revoke", control.id),
            Some(&view_token),
            None,
        )
        .await;
    server.stop().await;
    assert_eq!(status, 403);
    assert!(state.auth_manager.validate_token(&control_token).is_ok());
}

#[tokio::test]
async fn revoke_self_and_control_management_keep_status_contract() {
    let state = state();
    let (view_token, view) = pair(&state, DevicePermission::View);
    let (control_token, _) = pair(&state, DevicePermission::Control);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/devices/unknown/revoke",
                Some(&view_token),
                None
            )
            .await
            .0,
        403
    );
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/devices/unknown/revoke",
                Some(&control_token),
                None
            )
            .await
            .0,
        404
    );
    assert_eq!(
        server
            .request(
                "POST",
                &format!("/api/v1/devices/{}/revoke", view.id),
                Some(&view_token),
                None
            )
            .await
            .0,
        204
    );
    assert!(state.auth_manager.validate_token(&view_token).is_err());
    let (_, target) = pair(&state, DevicePermission::View);
    assert_eq!(
        server
            .request(
                "POST",
                &format!("/api/v1/devices/{}/revoke", target.id),
                Some(&control_token),
                None
            )
            .await
            .0,
        204
    );
    server.stop().await;
}

#[tokio::test]
async fn pairing_budget_is_global_atomic_and_rotation_cannot_reset_it() {
    let state = state();
    let code = state
        .auth_manager
        .create_pairing_code(DevicePermission::View);
    let server = SecurityServer::start(Arc::clone(&state)).await;
    // All requests run concurrently through the real unauthenticated route.
    let responses = futures_util::future::join_all((0..20).map(|_| {
        server.request(
            "POST",
            "/api/v1/pair/exchange",
            None,
            Some(r#"{"code":"000000","deviceName":"guess"}"#),
        )
    }))
    .await;
    assert_eq!(responses.iter().filter(|(s, _)| *s == 400).count(), 5);
    assert_eq!(responses.iter().filter(|(s, _)| *s == 429).count(), 15);
    for (_, body) in responses.iter().filter(|(s, _)| *s == 429) {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap()["code"],
            "pairing_rate_limited"
        );
    }
    let rotated = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    for pin in [code, rotated] {
        let body = serde_json::json!({"code":pin,"deviceName":"after exhaustion"}).to_string();
        assert_eq!(
            server
                .request("POST", "/api/v1/pair/exchange", None, Some(&body))
                .await
                .0,
            429
        );
    }
    assert!(state.auth_manager.list_devices().is_empty());
    server.stop().await;
}
