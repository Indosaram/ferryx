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

    // Health and anything outside /api/v1 stay same-origin, even for the relay.
    for same_origin_only in ["/api/v1/health", "/index.html", "/api/push/subscribe"] {
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

/// D2: a relay the daemon resolved from FERRYX_RELAY_URL or its enrollment (not saved in the
/// config) is admitted too, and only while it is the recorded resolution.
#[tokio::test]
async fn cors_allows_the_relay_the_daemon_actually_resolved() {
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        WorkspaceRegistry::new(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = Arc::clone(&state);
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(served)).await.unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = "/api/v1/capabilities";
    let env_relay = "https://relay.from-env.example";

    assert_eq!(preflight(&http, addr, api, env_relay).await, None, "not resolved yet");
    *state.resolved_relay.write() = Some((
        format!("{env_relay}/"),
        crate::remote::state::RelayUrlSource::Env,
    ));
    assert_eq!(preflight(&http, addr, api, env_relay).await.as_deref(), Some(env_relay));
    *state.resolved_relay.write() = None;
    assert_eq!(preflight(&http, addr, api, env_relay).await, None, "cleared with the relay");
    server.abort();
}

/// D26-5: after the relay client fails over, CORS admits the fallback it now uses (and still the
/// resolved primary); a fallback that is configured but not active stays refused.
#[tokio::test]
async fn cors_allows_the_fallback_relay_after_a_failover() {
    let state = Arc::new(RemoteGatewayState::new(
        Arc::new(TerminalService::default()),
        WorkspaceRegistry::new(),
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = Arc::clone(&state);
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(served)).await.unwrap();
    });
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = "/api/v1/capabilities";
    let (primary, standby) = ("https://primary.selfhost.example", "https://standby.selfhost.example");
    let client = crate::remote::relay_client::RelayClient::new(primary, "tok")
        .with_fallback_urls([standby])
        .with_resolved_source(crate::remote::state::RelayUrlSource::Config);
    *state.resolved_relay.write() =
        Some((primary.to_string(), crate::remote::state::RelayUrlSource::Config));
    *state.relay_client.write() = Some(client.clone());

    assert_eq!(preflight(&http, addr, api, standby).await, None, "standby is not active yet");
    client.fail_over_for_test();
    assert_eq!(preflight(&http, addr, api, standby).await.as_deref(), Some(standby));
    assert_eq!(preflight(&http, addr, api, primary).await.as_deref(), Some(primary));
    server.abort();
}
