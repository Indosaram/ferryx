// Generation 2 audit observations of 28032e3. These are NOT security-acceptance tests.
// `observes_*` PASS confirms the explicitly named residual behavior.
// `verifies_*` PASS confirms a narrow repair. All peers/keys/tokens are synthetic.
// Run as src-tauri/tests/zero_config_gen2_audit.rs with:
// cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_gen2_audit
use std::{net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use axum::{Router, http::StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::remote::{
    auth::{MachineIdentity, DevicePermission, sign_control_challenge, load_or_generate_machine_identity},
    protocol::{ControlChallenge, ControlAuth, ControlAuthResponse, RegisterPairingPin, RegisterPairingPinAck},
    relay_client::RelayClient,
    relay_server::{RelayState, relay_router, IncomingSessionNotice},
    server::create_remote_router,
    state::RemoteGatewayState,
};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
const LIMIT: Duration = Duration::from_secs(8);
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running { fn drop(&mut self) { self.0.abort(); } }
fn key(seed: u8, id: &str) -> MachineIdentity {
    let signing = SigningKey::from_bytes(&[seed; 32]);
    MachineIdentity { machine_id: id.into(), display_name: "audit fixture".into(),
        public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        private_key: STANDARD.encode(signing.to_bytes()) }
}
async fn serve(router: Router) -> (String, Running) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    (format!("http://{addr}"), Running(task))
}
fn relay_state(path: &std::path::Path) -> RelayState {
    RelayState::new_with_key_store(vec![], path).unwrap()
}
async fn json<T: serde::de::DeserializeOwned>(ws: &mut Ws) -> T {
    let message = timeout(LIMIT, ws.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}
async fn auth(base: &str, identity: &MachineIdentity) -> Ws {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/control", base.replace("http://", "ws://"))).await.unwrap();
    let challenge: ControlChallenge = json(&mut ws).await;
    let response = ControlAuth { enrollment_token: None, machine_id: identity.machine_id.clone(),
        display_name: identity.display_name.clone(), public_key: identity.public_key.clone(),
        timestamp: challenge.timestamp,
        signature: sign_control_challenge(identity, "relay", &challenge.nonce, challenge.timestamp).unwrap() };
    ws.send(Message::Text(serde_json::to_string(&response).unwrap().into())).await.unwrap();
    let result: ControlAuthResponse = json(&mut ws).await;
    assert!(result.success, "owner auth failed: {:?}", result.error);
    ws
}
async fn allocate(ws: &mut Ws) -> IncomingSessionNotice {
    ws.send(Message::Text(r#"{"type":"AllocateSession"}"#.into())).await.unwrap();
    json(ws).await
}
fn client() -> reqwest::Client {
    reqwest::Client::builder().timeout(LIMIT).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}
async fn data(base: &str, session: &str) -> Ws {
    tokio_tungstenite::connect_async(format!("{}/tunnel/data/{session}", base.replace("http://", "ws://"))).await.unwrap().0
}
async fn ticket(base: &str, machine: &str, token: Option<&str>, target: &str) -> reqwest::Response {
    let mut request = client().post(format!("{base}/host/{machine}/api/v1/socket-ticket"))
        .header("content-type", "application/json").header("origin", "https://audit.invalid")
        .body(serde_json::json!({"target":target}).to_string());
    if let Some(token) = token { request = request.bearer_auth(token); }
    request.send().await.unwrap()
}
fn gateway_state() -> Arc<RemoteGatewayState> {
    Arc::new(RemoteGatewayState::new_with_paths(
        Arc::new(ferryx_lib::terminal::TerminalService::default()),
        ferryx_lib::worktree::WorkspaceRegistry::new(), None, None))
}

#[tokio::test]
async fn observes_unissued_ticket_before_pairing_and_again_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys.json");
    let state = relay_state(&path);
    let identity = key(1, "ticket-machine");
    let (base, server) = serve(relay_router(state.clone())).await;
    let mut control = auth(&base, &identity).await;
    allocate(&mut control).await;
    assert_eq!(ticket(&base, "ticket-machine", None, "/api/v1/events").await.status(), StatusCode::UNAUTHORIZED);
    let response = ticket(&base, "ticket-machine", Some("never-issued"), "/api/v1/events").await;
    assert_eq!(response.status(), StatusCode::OK, "empty token cache fails open");
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert!(body["ticket"].as_str().is_some());
    // Trusted in-process cache injection isolates cache semantics; not an E2E exchange claim.
    state.register_device_token("ticket-machine", "synthetic-issued");
    assert_eq!(ticket(&base, "ticket-machine", Some("never-issued"), "/api/v1/events").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ticket(&base, "ticket-machine", Some("synthetic-issued"), "/api/v1/events").await.status(), StatusCode::OK);
    drop(control); drop(server); drop(state);
    let restarted = relay_state(&path);
    let (base, _server) = serve(relay_router(restarted)).await;
    let mut control = auth(&base, &identity).await;
    allocate(&mut control).await;
    assert_eq!(ticket(&base, "ticket-machine", Some("never-issued"), "/api/v1/events").await.status(), StatusCode::OK,
        "restart retains owner keys but loses authorization cache");
}

#[tokio::test]
async fn verifies_double_colon_target_is_now_accepted_for_cached_device() {
    let dir = tempfile::tempdir().unwrap();
    let state = relay_state(&dir.path().join("keys.json"));
    state.register_device_token("scoped", "synthetic-issued");
    let (base, _server) = serve(relay_router(state)).await;
    let mut control = auth(&base, &key(2, "scoped")).await;
    allocate(&mut control).await;
    assert_eq!(ticket(&base, "scoped", Some("synthetic-issued"), "/api/v1/terminal/host-a::session-a").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn observes_revoked_cached_device_still_gets_ticket_while_gateway_rejects_it() {
    let gateway = gateway_state();
    let pin = gateway.auth_manager.create_pairing_code(DevicePermission::View);
    let (credential, device) = gateway.auth_manager.exchange_pairing_code(&pin, "audit-view").unwrap();
    let (gateway_base, _gateway) = serve(create_remote_router(gateway.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let relay = relay_state(&dir.path().join("keys.json"));
    relay.register_device_token("revocation", &credential);
    let (base, _server) = serve(relay_router(relay)).await;
    let mut control = auth(&base, &key(3, "revocation")).await;
    allocate(&mut control).await;
    assert!(gateway.auth_manager.revoke_device(&device.id));
    assert!(gateway.auth_manager.validate_token(&credential).is_err());
    assert_eq!(client().get(format!("{gateway_base}/api/v1/workspace/state")).bearer_auth(&credential).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ticket(&base, "revocation", Some(&credential), "/api/v1/events").await.status(), StatusCode::OK);
    // This is relay admission staleness, NOT a gateway revocation bypass.
}

#[tokio::test]
async fn observes_valid_older_gateway_device_rejected_after_another_token_is_cached() {
    let gateway = gateway_state();
    let pin = gateway.auth_manager.create_pairing_code(DevicePermission::View);
    let (older, _) = gateway.auth_manager.exchange_pairing_code(&pin, "older").unwrap();
    let pin = gateway.auth_manager.create_pairing_code(DevicePermission::View);
    let (newer, _) = gateway.auth_manager.exchange_pairing_code(&pin, "newer").unwrap();
    assert!(gateway.auth_manager.validate_token(&older).is_ok());
    let dir = tempfile::tempdir().unwrap();
    let state = relay_state(&dir.path().join("keys.json"));
    let (base, _server) = serve(relay_router(state.clone())).await;
    let mut control = auth(&base, &key(4, "migration")).await;
    allocate(&mut control).await;
    assert_eq!(ticket(&base, "migration", Some(&older), "/api/v1/events").await.status(), StatusCode::OK);
    state.register_device_token("migration", &newer);
    assert_eq!(ticket(&base, "migration", Some(&older), "/api/v1/events").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(ticket(&base, "migration", Some(&newer), "/api/v1/events").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn observes_day_long_lease_replay_rejection_and_nonmonotonic_registration_generation() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _server) = serve(relay_router(relay_state(&dir.path().join("keys.json")))).await;
    let mut control = auth(&base, &key(5, "lease")).await;
    let mut registration = RegisterPairingPin { generation: Some(2), pin: "456789".into(),
        pairing_token: "x".into(), machine_id: "lease".into(),
        expires_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 86400 };
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    assert_ne!(json::<RegisterPairingPinAck>(&mut control).await.status, "ready", "short capability repair");
    registration.pairing_token = "0123456789abcdef0123456789abcdef".into();
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    assert_eq!(json::<RegisterPairingPinAck>(&mut control).await.status, "ready", "day-long lease still accepted");
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    assert_ne!(json::<RegisterPairingPinAck>(&mut control).await.status, "ready", "identical retry is not idempotent");
    registration.generation = Some(1); registration.pin = "567890".into();
    registration.pairing_token = "abcdef0123456789abcdef0123456789".into();
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    let ack: RegisterPairingPinAck = json(&mut control).await;
    assert_eq!(ack.status, "ready"); assert_eq!(ack.generation, Some(1));
}

#[tokio::test]
async fn observes_old_control_generation_raw_halves_still_attach() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _server) = serve(relay_router(relay_state(&dir.path().join("keys.json")))).await;
    let identity = key(6, "generation");
    let mut original = auth(&base, &identity).await;
    let old = allocate(&mut original).await;
    let mut replacement = auth(&base, &identity).await;
    allocate(&mut replacement).await;
    let mut daemon_half = data(&base, &old.session_id).await;
    let (mut browser_half, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/client/{}", base.replace("http://", "ws://"), old.session_id)).await.unwrap();
    daemon_half.send(Message::Text("inert audit frame".into())).await.unwrap();
    assert_eq!(timeout(LIMIT, browser_half.next()).await.unwrap().unwrap().unwrap().to_text().unwrap(), "inert audit frame");
}

#[tokio::test]
async fn observes_independent_key_store_writers_still_lose_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keys.json");
    let first = relay_state(&path); let second = relay_state(&path);
    let (a, _a) = serve(relay_router(first)).await;
    let (b, _b) = serve(relay_router(second)).await;
    let _owner_a = auth(&a, &key(7, "owner-a")).await;
    let _owner_b = auth(&b, &key(8, "owner-b")).await;
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(saved.get("owner-a").is_none()); assert!(saved.get("owner-b").is_some());
}

#[test]
fn observes_inconsistent_persisted_identity_is_still_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let mut invalid = key(9, "inconsistent"); invalid.public_key = key(10, "unused").public_key;
    std::fs::write(dir.path().join("identity.json"), serde_json::to_vec(&invalid).unwrap()).unwrap();
    let loaded = load_or_generate_machine_identity(dir.path()).unwrap();
    assert_eq!(loaded.public_key, invalid.public_key); assert_eq!(loaded.private_key, invalid.private_key);
}

#[tokio::test]
async fn observes_public_health_ignores_bearer_and_has_no_machine_id() {
    let (base, _gateway) = serve(create_remote_router(gateway_state())).await;
    for token in [None, Some("unissued-health-token")] {
        let mut request = client().get(format!("{base}/api/v1/health"));
        if let Some(token) = token { request = request.bearer_auth(token); }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
        assert_eq!(body["status"], "ok"); assert!(body.get("machineId").is_none());
    }
}

#[tokio::test]
async fn observes_redirect_and_type_gaps_but_verifies_cache_and_origin_headers_fixed() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _server) = serve(relay_router(relay_state(&dir.path().join("keys.json")))).await;
    let mut control = auth(&base, &key(11, "response")).await;
    allocate(&mut control).await;
    let http = client();
    let request = http.delete(format!("{base}/host/response/api/v1/workspace/audit.js")).send();
    let responder = async {
        let notice: IncomingSessionNotice = json(&mut control).await;
        let mut channel = data(&base, &notice.session_id).await;
        let raw = timeout(LIMIT, channel.next()).await.unwrap().unwrap().unwrap();
        assert!(String::from_utf8_lossy(&raw.into_data()).starts_with("DELETE /api/v1/workspace/audit.js "));
        channel.send(Message::Binary(b"HTTP/1.1 302 Found\r\nContent-Type: application/javascript\r\nLocation: https://audit.invalid/\r\nCache-Control: public\r\nClear-Site-Data: *\r\nSet-Cookie: audit=fixture; Path=/\r\nService-Worker-Allowed: /\r\nContent-Length: 0\r\n\r\n".to_vec().into())).await.unwrap();
        let _ = timeout(LIMIT, channel.next()).await;
    };
    let (response, ()) = timeout(LIMIT, async { tokio::join!(request, responder) }).await.unwrap();
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["content-type"], "application/javascript");
    assert_eq!(response.headers()["location"], "https://audit.invalid/");
    assert_eq!(response.headers()["cache-control"], "no-store, private");
    assert_eq!(response.headers()["pragma"], "no-cache");
    assert_eq!(response.headers()["content-security-policy"], "default-src 'none'");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    for header in ["set-cookie", "service-worker-allowed", "clear-site-data"] {
        assert!(!response.headers().contains_key(header));
    }
}

#[tokio::test]
async fn verifies_real_gateway_relay_events_and_active_revocation() {
    let gateway = gateway_state();
    let pin = gateway.auth_manager.create_pairing_code(DevicePermission::View);
    let (credential, device) = gateway.auth_manager.exchange_pairing_code(&pin, "view").unwrap();
    let (gateway_base, _gateway) = serve(create_remote_router(gateway.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let relay = relay_state(&dir.path().join("keys.json"));
    relay.register_device_token("real-events", &credential);
    let (base, _server) = serve(relay_router(relay.clone())).await;
    let daemon = RelayClient::with_identity(&base, key(12, "real-events"), gateway_base.strip_prefix("http://").unwrap())
        .with_auth_manager((*gateway.auth_manager).clone());
    // Real authenticated connection, rather than accepting an echo gateway.
    let _daemon = Running(tokio::spawn(async move { daemon.run().await }));
    let issued = timeout(LIMIT, async {
        loop {
            let response = ticket(&base, "real-events", Some(&credential), "/api/v1/events").await;
            if response.status() == StatusCode::OK { break response; }
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&issued.bytes().await.unwrap()).unwrap();
    let url = format!("{}/host/real-events/api/v1/events?ticket={}", base.replace("http://", "ws://"), body["ticket"].as_str().unwrap());
    let (mut browser, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    // Backend event subscription is the readiness barrier, not a fixed sleep.
    timeout(LIMIT, async {
        while gateway.event_tx.receiver_count() == 0 { tokio::time::sleep(Duration::from_millis(10)).await; }
    }).await.unwrap();
    gateway.event_tx.send("audit-real-event".into()).unwrap();
    assert_eq!(timeout(LIMIT, browser.next()).await.unwrap().unwrap().unwrap().to_text().unwrap(), "audit-real-event");
    assert!(gateway.auth_manager.revoke_device(&device.id));
    assert!(matches!(timeout(LIMIT, browser.next()).await.unwrap(), None | Some(Err(_)) | Some(Ok(Message::Close(_)))));
    assert_eq!(ticket(&base, "real-events", Some(&credential), "/api/v1/events").await.status(), StatusCode::OK,
        "active stream revocation works, but admission cache remains stale");
}
