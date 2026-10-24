//! Bearer-token absolute lifetime and transparent rotation, exercised through the real router on
//! a private loopback listener with the auth manager's injected clock. No sleeps: time moves only
//! by `set_test_clock`.
use super::*;

const DAY: u64 = 24 * 60 * 60;
/// A fixed epoch for the injected clock, far from zero (zero means "real clock").
const T0: u64 = 1_900_000_000;

struct Gateway {
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl Gateway {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, create_remote_router(state)).await.unwrap();
        });
        Self { addr, tasks }
    }

    /// The reconnect call every client makes before opening its event socket.
    async fn reconnect(&self, token: &str) -> (u16, Option<String>, String) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap();
        let response = client
            .post(format!("http://{}/api/v1/socket-ticket", self.addr))
            .bearer_auth(token)
            .json(&serde_json::json!({ "target": "/api/v1/events" }))
            .send()
            .await
            .expect("a response");
        let status = response.status().as_u16();
        let rotated = response
            .headers()
            .get(ROTATED_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        (status, rotated, response.text().await.unwrap_or_default())
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

fn isolated_state() -> Arc<RemoteGatewayState> {
    Arc::new(RemoteGatewayState::new(
        Arc::new(crate::terminal::TerminalService::default()),
        crate::worktree::WorkspaceRegistry::new(),
    ))
}

/// Pairs at `T0` and keeps the device active (inside the 30-day idle window) up to `day`.
fn pair_and_age(state: &RemoteGatewayState, day: u64) -> String {
    let auth = &state.auth_manager;
    auth.set_test_clock(T0);
    let code = auth.create_pairing_code(DevicePermission::Control);
    let (token, _) = auth.exchange_pairing_code(&code, "lifetime fixture").unwrap();
    let mut at = 0;
    while at + 25 < day {
        at += 25;
        auth.set_test_clock(T0 + at * DAY);
        auth.validate_token(&token).expect("still within lifetime");
    }
    auth.set_test_clock(T0 + day * DAY);
    token
}

#[tokio::test]
async fn a_token_older_than_90_days_is_refused() {
    let state = isolated_state();
    let token = pair_and_age(&state, 91);
    assert_eq!(
        state.auth_manager.validate_token(&token).unwrap_err(),
        crate::remote::auth::AuthError::TokenExpired
    );
    let gateway = Gateway::start(Arc::clone(&state)).await;
    let (status, rotated, body) = gateway.reconnect(&token).await;
    gateway.stop().await;
    assert_eq!(status, 401);
    assert_eq!(rotated, None, "an expired token is never rotated");
    let envelope: crate::remote::machine_protocol::ErrorEnvelope =
        serde_json::from_str(&body).expect("typed error envelope");
    assert_eq!(envelope.error.code, "TOKEN_EXPIRED");
}

#[tokio::test]
async fn a_token_near_expiry_rotates_on_reconnect() {
    let state = isolated_state();
    let token = pair_and_age(&state, 89);
    let gateway = Gateway::start(Arc::clone(&state)).await;

    let (status, rotated, _) = gateway.reconnect(&token).await;
    assert_eq!(status, 200);
    let fresh = rotated.expect("an 89-day token rotates on reconnect");
    assert_ne!(fresh, token);
    // A retry with the old token answers the same successor instead of minting another.
    let (status, again, _) = gateway.reconnect(&token).await;
    assert_eq!(status, 200);
    assert_eq!(again.as_deref(), Some(fresh.as_str()));

    // The successor works, starts a new lifetime, and is not rotated again.
    let (status, none, _) = gateway.reconnect(&fresh).await;
    assert_eq!(status, 200);
    assert_eq!(none, None);

    // Past day 90 the old token is refused while the rotated one keeps working.
    state.auth_manager.set_test_clock(T0 + 91 * DAY);
    let (status, _, body) = gateway.reconnect(&token).await;
    assert_eq!(status, 401);
    assert!(body.contains("TOKEN_EXPIRED"), "{body}");
    let (status, _, _) = gateway.reconnect(&fresh).await;
    assert_eq!(status, 200);
    gateway.stop().await;

    // A token outside the last week is never rotated.
    let young = isolated_state();
    let token = pair_and_age(&young, 60);
    let gateway = Gateway::start(Arc::clone(&young)).await;
    let (status, rotated, _) = gateway.reconnect(&token).await;
    gateway.stop().await;
    assert_eq!(status, 200);
    assert_eq!(rotated, None);
}

/// Stores written before tokens carried an issue time must keep working after the upgrade.
#[test]
fn a_legacy_token_without_issue_time_starts_its_lifetime_on_first_use() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote-auth.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    std::fs::write(
        &path,
        serde_json::json!({
            "devices": { "dev-1": {
                "id": "dev-1", "name": "Old phone", "permission": "control",
                "createdAt": now - 200 * DAY, "lastSeenAt": now - DAY
            }},
            "tokens": { "legacy-token": "dev-1" }
        })
        .to_string(),
    )
    .unwrap();
    let auth = crate::remote::auth::AuthManager::with_persistence(Some(path.clone()));
    assert_eq!(auth.validate_token("legacy-token").unwrap().id, "dev-1");
    assert!(!auth.token_near_expiry("legacy-token"));
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(
        stored["tokenMeta"]["legacy-token"]["issuedAt"].as_u64().is_some(),
        "the first use durably stamps the legacy token: {stored}"
    );
}
