//! GAP-6: the gateway answers cross-origin `/api/v1/*` calls only for allowlisted origins.
use super::*;
use crate::terminal::TerminalService;
use crate::worktree::WorkspaceRegistry;

async fn preflight(
    http: &reqwest::Client,
    addr: SocketAddr,
    path: &str,
    origin: &str,
) -> Option<String> {
    let response = http
        .request(reqwest::Method::OPTIONS, format!("http://{addr}{path}"))
        .header("Origin", origin)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "authorization")
        .send()
        .await
        .expect("preflight reaches the gateway");
    response
        .headers()
        .get("access-control-allow-origin")
        .map(|value| value.to_str().unwrap().to_string())
}

#[tokio::test]
async fn cors_rejects_an_unlisted_origin_and_allows_the_relay_origin() {
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        WorkspaceRegistry::new(),
    ));
    state.config.write().relay_url = Some("https://relay.selfhost.example/".to_string());
    *state.cors_saved_host_origins.write() = vec!["https://saved.example".to_string()];
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state)).await.unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = "/api/v1/capabilities";

    // An unlisted origin gets no allow-origin header, so the browser blocks the read.
    assert_eq!(preflight(&http, addr, api, "https://evil.example").await, None);

    // The default relay, the configured relay override, a saved host and the gateway's own
    // origin are echoed back exactly (never `*`).
    for allowed in [
        crate::remote::state::DEFAULT_RELAY_URL.to_string(),
        "https://relay.selfhost.example".to_string(),
        "https://saved.example".to_string(),
        format!("http://{addr}"),
    ] {
        assert_eq!(
            preflight(&http, addr, api, &allowed).await.as_deref(),
            Some(allowed.as_str()),
            "{allowed} must be allowed on {api}"
        );
    }

    // The phone's "Send test" is an authenticated /api/v1 call like the rest, so the relay origin
    // may preflight it; only registration below stays same-origin.
    assert_eq!(
        preflight(&http, addr, "/api/v1/push/test", crate::remote::state::DEFAULT_RELAY_URL)
            .await
            .as_deref(),
        Some(crate::remote::state::DEFAULT_RELAY_URL)
    );
    assert_eq!(preflight(&http, addr, "/api/v1/push/test", "https://evil.example").await, None);

    // Health, push registration and anything outside /api/v1 stay same-origin, even for the relay.
    for same_origin_only in [
        "/api/v1/health",
        "/index.html",
        "/api/push/subscribe",
        "/api/v1/push/vapid-public-key",
        "/api/v1/push/subscribe",
        "/api/v1/push/unsubscribe",
        "/api/v1/push/preferences",
    ] {
        assert_eq!(
            preflight(&http, addr, same_origin_only, crate::remote::state::DEFAULT_RELAY_URL)
                .await,
            None,
            "{same_origin_only} must not answer cross-origin"
        );
    }

    // A simple cross-origin GET from an unlisted origin carries no allow-origin either.
    let response = http
        .get(format!("http://{addr}{api}"))
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert!(response.headers().get("access-control-allow-origin").is_none());
    assert!(response.headers().get("access-control-allow-credentials").is_none());

    server.abort();
}
