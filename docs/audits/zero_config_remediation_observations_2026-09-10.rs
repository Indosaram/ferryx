//! Audit observations of 496cad7, not a security-acceptance suite.
//! PASS means the explicitly named remaining behavior was observed.
//! All listeners are ephemeral loopback; identities and credentials are synthetic.
//! Copy to src-tauri/tests/zero_config_remediation_observations.rs and run:
//! cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_remediation_observations
use std::{net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use axum::{Router, http::StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::remote::{
    auth::{MachineIdentity, sign_control_challenge, load_or_generate_machine_identity},
    protocol::{ControlChallenge, ControlAuth, ControlAuthResponse, RegisterPairingPin, RegisterPairingPinAck},
    relay_server::{RelayState, relay_router, IncomingSessionNotice},
    server::create_remote_router,
    state::RemoteGatewayState,
};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
const LIMIT: Duration = Duration::from_secs(5);
struct Running { task: tokio::task::JoinHandle<()>, storage: Option<tempfile::TempDir> }
impl Drop for Running { fn drop(&mut self) { self.task.abort(); } }
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
    (format!("http://{addr}"), Running { task, storage: None })
}
async fn relay() -> (String, Running) {
    let storage = tempfile::tempdir().unwrap();
    let state = RelayState::new_with_key_store(vec![], storage.path().join("machine_keys.json")).unwrap();
    let (base, mut running) = serve(relay_router(state)).await;
    running.storage = Some(storage);
    (base, running)
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
    assert!(result.success, "synthetic owner authentication must succeed: {:?}", result.error);
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

#[tokio::test]
async fn observes_unissued_bearer_receives_ticket_and_scoped_session_is_rejected() {
    let (base, _relay) = relay().await;
    let mut control = auth(&base, &key(1, "audit-ticket")).await;
    allocate(&mut control).await; // Real control-registration barrier.
    let http = client();
    let url = format!("{base}/host/audit-ticket/api/v1/socket-ticket");
    let body = r#"{"target":"/api/v1/events"}"#;
    let missing = http.post(&url).header("content-type", "application/json").body(body).send().await.unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED, "missing header fix is present");
    // No gateway or paired device exists in this fixture; this bearer was never issued.
    let accepted = http.post(&url).header("authorization", "Bearer audit-unissued")
        .header("origin", "https://audit.invalid").header("content-type", "application/json")
        .body(body).send().await.unwrap();
    assert_eq!(accepted.status(), StatusCode::OK, "issuance validates syntax, not a device");
    let value: serde_json::Value = serde_json::from_slice(&accepted.bytes().await.unwrap()).unwrap();
    assert!(value["ticket"].as_str().is_some());
    let scoped = http.post(&url).header("authorization", "Bearer audit-unissued")
        .header("content-type", "application/json")
        .body(r#"{"target":"/api/v1/terminal/host-a::session-a"}"#).send().await.unwrap();
    assert_eq!(scoped.status(), StatusCode::BAD_REQUEST, "gateway-supported scoped IDs remain excluded");
}

#[tokio::test]
async fn observes_old_control_generation_raw_halves_still_attach() {
    let identity = key(2, "audit-generation");
    let (base, _relay) = relay().await;
    let mut original = auth(&base, &identity).await;
    let old = allocate(&mut original).await;
    let mut replacement = auth(&base, &identity).await;
    allocate(&mut replacement).await; // Replacement is definitely installed.
    // Knowledge of an issued ID is the prerequisite; no guessing claim is made.
    let mut daemon_half = data(&base, &old.session_id).await;
    let (mut browser_half, _) = tokio_tungstenite::connect_async(format!(
        "{}/tunnel/client/{}", base.replace("http://", "ws://"), old.session_id)).await.unwrap();
    daemon_half.send(Message::Text("inert audit frame".into())).await.unwrap();
    let frame = timeout(LIMIT, browser_half.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(frame.to_text().unwrap(), "inert audit frame");
}

#[tokio::test]
async fn observes_one_character_day_long_registration_and_nonidempotent_ack() {
    let (base, _relay) = relay().await;
    let mut control = auth(&base, &key(3, "audit-registration")).await;
    let registration = RegisterPairingPin { generation: Some(1), pin: "456789".into(),
        pairing_token: "x".into(), machine_id: "audit-registration".into(),
        expires_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 86400 };
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    let first: RegisterPairingPinAck = json(&mut control).await;
    assert_eq!(first.status, "ready");
    control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
    let replay: RegisterPairingPinAck = json(&mut control).await;
    assert_ne!(replay.status, "ready", "same immutable request is not acknowledged idempotently");
}

#[test]
fn observes_inconsistent_persisted_identity_is_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let mut inconsistent = key(4, "audit-identity");
    inconsistent.public_key = key(5, "unused").public_key;
    std::fs::write(directory.path().join("identity.json"), serde_json::to_vec(&inconsistent).unwrap()).unwrap();
    let loaded = load_or_generate_machine_identity(directory.path()).unwrap();
    assert_eq!(loaded.public_key, inconsistent.public_key);
    assert_eq!(loaded.private_key, inconsistent.private_key);
    // Acceptance here does not mean the mismatched key can authenticate.
}

#[tokio::test]
async fn observes_independent_relay_instances_overwrite_shared_key_store() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("machine_keys.json");
    // Both processes/instances take their startup snapshot before either enrolls.
    let first = RelayState::new_with_key_store(vec![], &path).unwrap();
    let second = RelayState::new_with_key_store(vec![], &path).unwrap();
    let (a, _a) = serve(relay_router(first)).await;
    let (b, _b) = serve(relay_router(second)).await;
    let _owner_a = auth(&a, &key(6, "audit-owner-a")).await;
    let _owner_b = auth(&b, &key(7, "audit-owner-b")).await;
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(saved.get("audit-owner-a").is_none(), "later startup snapshot overwrites earlier enrollment");
    assert!(saved.get("audit-owner-b").is_some());
}

#[tokio::test]
async fn observes_response_contract_gaps_while_cookie_and_csp_hardening_is_present() {
    let (base, _relay) = relay().await;
    let mut control = auth(&base, &key(8, "audit-response")).await;
    allocate(&mut control).await;
    let http = client();
    let request = http.delete(format!("{base}/host/audit-response/api/v1/workspace/audit.js")).send();
    let responder = async {
        let notice: IncomingSessionNotice = json(&mut control).await;
        let mut channel = data(&base, &notice.session_id).await;
        let request = timeout(LIMIT, channel.next()).await.unwrap().unwrap().unwrap();
        assert!(String::from_utf8_lossy(&request.into_data()).starts_with("DELETE /api/v1/workspace/audit.js "));
        let body = b"/* inert audit fixture; no execution */";
        let mut response = format!("HTTP/1.1 302 Found\r\nContent-Type: application/javascript\r\nLocation: https://audit.invalid/\r\nCache-Control: public, max-age=86400\r\nSet-Cookie: audit=fixture; Path=/\r\nService-Worker-Allowed: /\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        response.extend_from_slice(body);
        channel.send(Message::Binary(response.into())).await.unwrap();
        let _ = timeout(LIMIT, channel.next()).await;
    };
    let (response, ()) = timeout(LIMIT, async { tokio::join!(request, responder) }).await.unwrap();
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(response.headers()["content-type"], "application/javascript");
    assert_eq!(response.headers()["location"], "https://audit.invalid/");
    assert_eq!(response.headers()["cache-control"], "public, max-age=86400");
    assert_eq!(response.headers()["content-security-policy"], "default-src 'none'");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(!response.headers().contains_key("set-cookie"));
    assert!(!response.headers().contains_key("service-worker-allowed"));
}

#[tokio::test]
async fn observes_real_gateway_health_has_no_machine_identity() {
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::new(ferryx_lib::terminal::TerminalService::default()),
        ferryx_lib::worktree::WorkspaceRegistry::new(), None, None));
    let (base, _gateway) = serve(create_remote_router(state)).await;
    let response = client().get(format!("{base}/api/v1/health")).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body.get("machineId").is_none());
}
