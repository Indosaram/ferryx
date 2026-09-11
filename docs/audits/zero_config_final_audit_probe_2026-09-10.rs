//! Audit-only observations of revision 818b68d; PASS confirms the reported defect,
//! not security approval. All listeners are ephemeral loopback, all keys synthetic.
//! Temporarily place this file at src-tauri/tests/zero_config_final_audit_probe.rs
//! and run: cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_final_audit_probe
use std::{net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use axum::{Router, http::StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::remote::{
    auth::{MachineIdentity, sign_challenge, DevicePermission},
    protocol::{ControlChallenge, ControlAuth, ControlAuthResponse, RegisterPairingPin, RegisterPairingPinAck},
    relay_client::RelayClient,
    relay_server::{RelayState, relay_router, IncomingSessionNotice},
    server::create_remote_router,
    state::RemoteGatewayState,
};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::{Message, client::IntoClientRequest}};
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
const LIMIT: Duration = Duration::from_secs(5);
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running { fn drop(&mut self) { self.0.abort(); } }
fn key(seed: u8, id: &str) -> MachineIdentity {
    let signing = SigningKey::from_bytes(&[seed; 32]);
    MachineIdentity { machine_id: id.into(), display_name: "audit fixture".into(),
        public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        private_key: STANDARD.encode(signing.to_bytes()) }
}
async fn serve(router: Router) -> (String, SocketAddr, Running) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    (format!("http://{addr}"), addr, Running(task))
}
async fn relay(tokens: Vec<String>) -> (String, Running) {
    let (base, _, task) = serve(relay_router(RelayState::new(tokens))).await;
    (base, task)
}
async fn json<T: serde::de::DeserializeOwned>(ws: &mut Ws) -> T {
    let message = timeout(LIMIT, ws.next()).await.unwrap().unwrap().unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}
async fn auth(base: &str, identity: &MachineIdentity) -> (Ws, ControlAuthResponse) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/control", base.replace("http://", "ws://"))).await.unwrap();
    let challenge: ControlChallenge = json(&mut ws).await;
    let response = ControlAuth { machine_id: identity.machine_id.clone(), display_name: identity.display_name.clone(),
        public_key: identity.public_key.clone(), timestamp: challenge.timestamp,
        signature: sign_challenge(identity, &challenge.nonce, challenge.timestamp).unwrap() };
    ws.send(Message::Text(serde_json::to_string(&response).unwrap().into())).await.unwrap();
    let result = json(&mut ws).await;
    (ws, result)
}
async fn allocate(ws: &mut Ws) -> IncomingSessionNotice {
    ws.send(Message::Text(r#"{"type":"AllocateSession"}"#.into())).await.unwrap();
    json(ws).await
}
fn client() -> reqwest::Client { reqwest::Client::builder().timeout(LIMIT).build().unwrap() }
async fn post(base: &str, path: &str, value: serde_json::Value) -> reqwest::Response {
    client().post(format!("{base}{path}")).header("content-type", "application/json")
        .body(value.to_string()).send().await.unwrap()
}
async fn ticket(base: &str, machine: &str, target: &str) -> String {
    let response = post(base, &format!("/host/{machine}/api/v1/socket-ticket"), serde_json::json!({"target": target})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    value["ticket"].as_str().unwrap().into()
}
async fn data(base: &str, session: &str) -> Ws {
    tokio_tungstenite::connect_async(format!("{}/tunnel/data/{session}", base.replace("http://", "ws://"))).await.unwrap().0
}
fn gateway_state() -> Arc<RemoteGatewayState> {
    Arc::new(RemoteGatewayState::new(Arc::new(ferryx_lib::terminal::TerminalService::default()), ferryx_lib::worktree::WorkspaceRegistry::new()))
}

#[tokio::test]
async fn observes_restart_loses_machine_owner_binding() {
    let owner = key(1, "audit-owner");
    let impostor = key(2, "audit-owner");
    let (base, running) = relay(vec![]).await;
    let (_owner_ws, accepted) = auth(&base, &owner).await;
    assert!(accepted.success);
    let (_, rejected) = auth(&base, &impostor).await;
    assert!(!rejected.success);
    drop(running);
    let (restarted, _running) = relay(vec![]).await;
    let (_, reclaimed) = auth(&restarted, &impostor).await;
    assert!(reclaimed.success, "new RelayState forgets the previously enrolled owner");
}

#[tokio::test]
async fn observes_private_allowlist_does_not_gate_public_key_enrollment() {
    let (base, _running) = relay(vec!["synthetic-private-token".into()]).await;
    let (_, accepted) = auth(&base, &key(3, "not-on-the-allowlist")).await;
    assert!(accepted.success, "an unlisted key enrolls even with a nonempty private allowlist");
}

#[tokio::test]
async fn observes_unauthenticated_ticket_and_terminal_geometry_rejection() {
    let (base, _running) = relay(vec![]).await;
    let (mut control, accepted) = auth(&base, &key(4, "audit-socket")).await;
    assert!(accepted.success);
    allocate(&mut control).await;
    // No device is paired and no Authorization header is supplied.
    let value = ticket(&base, "audit-socket", "/api/v1/terminal/t1").await;
    let url = format!("{}/host/audit-socket/api/v1/terminal/t1?ticket={value}&render=grid&cols=80&rows=24", base.replace("http://", "ws://"));
    let error = tokio_tungstenite::connect_async(url).await.unwrap_err();
    assert!(matches!(error, tokio_tungstenite::tungstenite::Error::Http(r) if r.status() == StatusCode::UNAUTHORIZED));
}

#[tokio::test]
async fn observes_coordinator_secret_cannot_pair_real_gateway() {
    let state = gateway_state();
    let (_, address, _gateway) = serve(create_remote_router(state.clone())).await;
    let (base, _relay) = relay(vec![]).await;
    let daemon = RelayClient::with_identity(&base, key(5, "audit-real-pair"), address.to_string());
    let coordinator = daemon.pairing_coordinator();
    let _daemon = Running(tokio::spawn(async move { daemon.run().await }));
    let registration = timeout(LIMIT, coordinator.generate_pairing(Duration::from_secs(60))).await.unwrap().unwrap();
    let browser_shape = post(&base, "/api/v1/pair/exchange", serde_json::json!({"code": registration.pin, "deviceName":"audit"})).await;
    assert_eq!(browser_shape.status(), StatusCode::NOT_FOUND);
    let response = post(&base, "/api/v1/pair/exchange", serde_json::json!({"pin": registration.pin, "deviceName":"audit"})).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.text().await.unwrap(), "Invalid pairing code");
    assert!(state.auth_manager.list_devices().is_empty());
    assert!(coordinator.generate_pairing(Duration::from_secs(60)).await.is_err());
}

#[tokio::test]
async fn observes_backend_ws_missing_auth_despite_valid_device() {
    let state = gateway_state();
    let pin = state.auth_manager.create_pairing_code(DevicePermission::View);
    let (credential, _) = state.auth_manager.exchange_pairing_code(&pin, "audit-view").unwrap();
    let (gateway_base, address, _gateway) = serve(create_remote_router(state)).await;
    // Prove this very same gateway accepts an authenticated event upgrade.
    let mut direct = format!("{}/api/v1/events", gateway_base.replace("http://", "ws://")).into_client_request().unwrap();
    direct.headers_mut().insert("authorization", format!("Bearer {credential}").parse().unwrap());
    let (mut direct_ws, _) = tokio_tungstenite::connect_async(direct).await.unwrap();
    direct_ws.close(None).await.unwrap();
    let (base, _relay) = relay(vec![]).await;
    let daemon = RelayClient::with_identity(&base, key(6, "audit-real-ws"), address.to_string());
    let coordinator = daemon.pairing_coordinator();
    let _daemon = Running(tokio::spawn(async move { daemon.run().await }));
    timeout(LIMIT, coordinator.generate_pairing(Duration::from_secs(60))).await.unwrap().unwrap();
    let response = client().post(format!("{base}/host/audit-real-ws/api/v1/socket-ticket"))
        .header("authorization", format!("Bearer {credential}")).header("content-type", "application/json")
        .body(r#"{"target":"/api/v1/events"}"#).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
    let url = format!("{}/host/audit-real-ws/api/v1/events?ticket={}", base.replace("http://", "ws://"), body["ticket"].as_str().unwrap());
    let (mut browser, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let frame = timeout(LIMIT, browser.next()).await.unwrap();
    assert!(matches!(frame, None | Some(Err(_)) | Some(Ok(Message::Close(_)))), "relay cannot open the authenticated backend event stream");
}

#[tokio::test]
async fn observes_untrusted_html_and_cookie_forwarded_on_shared_origin() {
    let (base, _relay) = relay(vec![]).await;
    let (mut control, accepted) = auth(&base, &key(7, "audit-content")).await;
    assert!(accepted.success);
    allocate(&mut control).await;
    let http = client();
    let request = http.get(format!("{base}/host/audit-content/api/v1/audit-page")).send();
    let responder = async {
        let notice: IncomingSessionNotice = json(&mut control).await;
        let mut channel = data(&base, &notice.session_id).await;
        timeout(LIMIT, channel.next()).await.unwrap().unwrap().unwrap();
        let body = b"<html>inert audit fixture</html>";
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nSet-Cookie: audit=fixture; Path=/\r\nService-Worker-Allowed: /\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        response.extend_from_slice(body);
        channel.send(Message::Binary(response.into())).await.unwrap();
        let _ = timeout(LIMIT, channel.next()).await;
    };
    let (response, ()) = timeout(LIMIT, async { tokio::join!(request, responder) }).await.unwrap();
    let response = response.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/html");
    assert_eq!(response.headers()["set-cookie"], "audit=fixture; Path=/");
    assert_eq!(response.headers()["service-worker-allowed"], "/");
    assert!(!response.headers().contains_key("content-security-policy"));
    assert_eq!(response.text().await.unwrap(), "<html>inert audit fixture</html>");
}

#[tokio::test]
async fn observes_old_generation_data_and_client_roles_still_attach() {
    let identity = key(8, "audit-generations");
    let (base, _relay) = relay(vec![]).await;
    let (mut first, _) = auth(&base, &identity).await;
    let old = allocate(&mut first).await;
    let (mut replacement, accepted) = auth(&base, &identity).await;
    assert!(accepted.success);
    allocate(&mut replacement).await;
    // A caller who knows the old stream ID presents neither role proof nor key.
    let mut daemon_role = data(&base, &old.session_id).await;
    let (mut client_role, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/client/{}", base.replace("http://", "ws://"), old.session_id)).await.unwrap();
    daemon_role.send(Message::Text("audit-role-observation".into())).await.unwrap();
    let frame = timeout(LIMIT, client_role.next()).await.unwrap().unwrap().unwrap();
    assert_eq!(frame.to_text().unwrap(), "audit-role-observation");
}

#[tokio::test]
async fn observes_unbounded_registration_lifetime_and_nonidempotent_ack() {
    let (base, _relay) = relay(vec![]).await;
    let (mut control, accepted) = auth(&base, &key(9, "audit-lease")).await;
    assert!(accepted.success);
    let registration = RegisterPairingPin { pin: "456789".into(), pairing_token: "x".into(), machine_id: "audit-lease".into(),
        expires_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 86400 };
    for expected in ["ready", "rejected"] {
        control.send(Message::Text(serde_json::to_string(&registration).unwrap().into())).await.unwrap();
        let ack: RegisterPairingPinAck = json(&mut control).await;
        assert_eq!(ack.status, expected);
    }
}
