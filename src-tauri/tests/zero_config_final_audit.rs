// Audit observations for 30555f2 and focused repair checks for 93dc006.
// An observes_* PASS confirms the named OLD defect; a verifies_* PASS confirms a repair.
// All peers use loopback sockets, synthetic identities/capabilities and temporary stores.
// The exchange responder is synthetic: these tests establish relay admission/forwarding,
// not unauthorized access to a real user's gateway or terminal.
// Old-defect observations (only against the old product revision):
// cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_final_audit observes_ -- --ignored --nocapture --test-threads=1
// Focused current repair checks (93dc006):
// cargo test --manifest-path src-tauri/Cargo.toml --test zero_config_final_audit verifies_ -- --ignored --nocapture --test-threads=1
use std::{net::SocketAddr, time::{Duration, SystemTime, UNIX_EPOCH}};
use axum::{Router, http::StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use ferryx_lib::remote::{
    auth::{MachineIdentity, sign_control_challenge},
    protocol::{ControlAuth, ControlAuthResponse, ControlChallenge, RegisterPairingPin, RegisterPairingPinAck},
    relay_server::{RelayState, relay_router, IncomingSessionNotice},
};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::{TcpListener, TcpStream}, time::timeout};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
const LIMIT: Duration = Duration::from_secs(8);
struct Running(tokio::task::JoinHandle<()>);
impl Drop for Running { fn drop(&mut self) { self.0.abort(); } }

fn identity(seed: u8, machine: &str) -> MachineIdentity {
    let key = SigningKey::from_bytes(&[seed; 32]);
    MachineIdentity {
        machine_id: machine.into(), display_name: "isolated final audit".into(),
        public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        private_key: STANDARD.encode(key.to_bytes()),
    }
}
async fn serve(state: RelayState) -> (String, Running) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router: Router = relay_router(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
    });
    (format!("http://{address}"), Running(task))
}
fn client() -> reqwest::Client {
    // Keep loopback observations independent of workstation/system HTTP proxies.
    reqwest::Client::builder().no_proxy().timeout(LIMIT).redirect(reqwest::redirect::Policy::none()).build().unwrap()
}
async fn json<T: serde::de::DeserializeOwned>(socket: &mut Ws) -> T {
    loop {
        let message = timeout(LIMIT, socket.next()).await.unwrap().unwrap().unwrap();
        match message {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(data) => socket.send(Message::Pong(data)).await.unwrap(),
            other => panic!("Expected JSON, received {other:?}"),
        }
    }
}
async fn authenticate(base: &str, id: &MachineIdentity) -> Ws {
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/control", base.replace("http://", "ws://"))).await.unwrap();
    let challenge: ControlChallenge = json(&mut socket).await;
    let auth = ControlAuth {
        enrollment_token: None, machine_id: id.machine_id.clone(),
        display_name: id.display_name.clone(), public_key: id.public_key.clone(),
        signature: sign_control_challenge(id, "relay", &challenge.nonce, challenge.timestamp).unwrap(),
        timestamp: challenge.timestamp,
    };
    socket.send(Message::Text(serde_json::to_string(&auth).unwrap().into())).await.unwrap();
    let reply: ControlAuthResponse = json(&mut socket).await;
    assert!(reply.success, "Synthetic owner failed authentication: {:?}", reply.error);
    socket
}
fn registration(machine: &str, generation: Option<u64>, pin: &str) -> RegisterPairingPin {
    RegisterPairingPin {
        generation, machine_id: machine.into(), pin: pin.into(),
        pairing_token: format!("audit-capability-{pin}-synthetic"),
        expires_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 60,
    }
}
async fn register(socket: &mut Ws, request: &RegisterPairingPin) {
    socket.send(Message::Text(serde_json::to_string(request).unwrap().into())).await.unwrap();
    let ack: RegisterPairingPinAck = json(socket).await;
    assert_eq!(ack.status, "ready");
    assert_eq!(ack.pin, request.pin);
    assert_eq!(ack.generation, request.generation);
}
async fn exchange(base: &str, pin: &str) -> reqwest::Response {
    client().post(format!("{base}/api/v1/pair/exchange"))
        .header("content-type", "application/json")
        .body(serde_json::json!({"pin":pin,"deviceName":"audit browser"}).to_string())
        .send().await.unwrap()
}
// A 409 from the synthetic responder makes it unambiguous that the relay accepted
// the claim and forwarded its secret. It is NOT a successful real device exchange.
async fn observe_forwarded_claim(base: &str, owner: &mut Ws, request: &RegisterPairingPin) {
    let base_copy = base.to_owned();
    let pin = request.pin.clone();
    let http = tokio::spawn(async move { exchange(&base_copy, &pin).await });
    let notice: IncomingSessionNotice = json(owner).await;
    let (mut data, _) = tokio_tungstenite::connect_async(format!("{}/tunnel/data/{}", base.replace("http://", "ws://"), notice.session_id)).await.unwrap();
    let wire = timeout(LIMIT, data.next()).await.unwrap().unwrap().unwrap();
    let bytes = wire.into_data();
    let raw = std::str::from_utf8(&bytes).unwrap();
    assert!(raw.starts_with("POST /api/v1/pair/exchange HTTP/1.1\r\n"));
    assert!(raw.contains(&request.pairing_token), "Claim must actually forward the old capability");
    data.send(Message::Binary(b"HTTP/1.1 409 Conflict\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec().into())).await.unwrap();
    let result = timeout(LIMIT, http).await.unwrap().unwrap();
    assert_eq!(result.status(), StatusCode::CONFLICT, "Expected the responder's status, not relay rejection");
}

#[tokio::test]
#[ignore = "Historical audit observation: run only against product revision 30555f2"]
async fn observes_unrelated_connection_blocks_a_second_machines_first_pairing() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let mut unrelated = authenticate(&base, &identity(91, "unrelated")).await;
    register(&mut unrelated, &registration("unrelated", Some(1), "110001")).await;
    let mut victim = authenticate(&base, &identity(92, "second-machine")).await;
    // The actual PairingCoordinator starts its own counter at zero and sends Some(1)
    // for its first pairing. This is not the relay-wide control generation (now 2).
    let request = registration("second-machine", Some(1), "110002");
    register(&mut victim, &request).await;
    assert_eq!(exchange(&base, &request.pin).await.status(), StatusCode::NOT_FOUND);
    println!("OBSERVED: second machine received ready ACK for local generation 1, then relay exchange returned 404");
}

#[tokio::test]
#[ignore = "Historical audit observation: run only against product revision 30555f2"]
async fn observes_absent_generation_claim_forwarded_after_control_replacement() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let id = identity(93, "optional-generation");
    let mut original = authenticate(&base, &id).await;
    let request = registration(&id.machine_id, None, "220001");
    register(&mut original, &request).await;
    let mut replacement = authenticate(&base, &id).await;
    // ACK of a DIFFERENT registration proves the replacement is live without
    // re-registering, renewing, or reauthorizing the old PIN under test.
    register(&mut replacement, &registration(&id.machine_id, None, "220002")).await;
    observe_forwarded_claim(&base, &mut replacement, &request).await;
    println!("OBSERVED: omitted generation allowed the old, never re-registered claim to reach replacement control");
}

#[tokio::test]
#[ignore = "Historical audit observation: run only against product revision 30555f2"]
async fn observes_caller_selected_future_generation_becomes_claimable_on_replacement() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let id = identity(94, "future-generation");
    let mut original = authenticate(&base, &id).await;
    // Registration made under server control generation 1 claims to be generation 2.
    let request = registration(&id.machine_id, Some(2), "330001");
    register(&mut original, &request).await;
    assert_eq!(exchange(&base, &request.pin).await.status(), StatusCode::NOT_FOUND);
    let mut replacement = authenticate(&base, &id).await;
    // Synchronize with a different PIN: the old registration stays entirely untouched.
    register(&mut replacement, &registration(&id.machine_id, None, "330002")).await;
    observe_forwarded_claim(&base, &mut replacement, &request).await;
    println!("OBSERVED: caller value 2 accepted under control 1 became claimable after replacement without re-registration");
}

#[tokio::test]
#[ignore = "Audit positive control for the F08b repair"]
async fn verifies_raw_half_from_superseded_control_is_rejected() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let id = identity(95, "raw-half-control");
    let mut original = authenticate(&base, &id).await;
    original.send(Message::Text(r#"{"type":"AllocateSession"}"#.into())).await.unwrap();
    let old: IncomingSessionNotice = json(&mut original).await;
    let mut replacement = authenticate(&base, &id).await;
    register(&mut replacement, &registration(&id.machine_id, None, "440001")).await;
    let result = tokio_tungstenite::connect_async(format!("{}/tunnel/data/{}", base.replace("http://", "ws://"), old.session_id)).await;
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => assert_eq!(response.status(), StatusCode::GONE),
        other => panic!("Expected 410 for the old raw half, received {other:?}"),
    }
    println!("VERIFIED: old raw data half received HTTP 410 after control replacement");
}

#[tokio::test]
#[ignore = "Focused post-remediation check for 93dc006"]
async fn verifies_second_machines_first_pairing_reaches_its_own_control() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let mut first = authenticate(&base, &identity(96, "first-current")).await;
    register(&mut first, &registration("first-current", Some(1), "550001")).await;
    let mut second = authenticate(&base, &identity(97, "second-current")).await;
    let request = registration("second-current", Some(1), "550002");
    register(&mut second, &request).await;
    observe_forwarded_claim(&base, &mut second, &request).await;
    println!("VERIFIED: second machine attempt 1 reached its own control channel and returned synthetic gateway 409");
}

#[tokio::test]
#[ignore = "Focused post-remediation check for 93dc006"]
async fn verifies_omitted_generation_cannot_redeem_after_control_replacement() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let id = identity(98, "null-current");
    let mut original = authenticate(&base, &id).await;
    let request = registration(&id.machine_id, None, "660001");
    register(&mut original, &request).await;
    let mut replacement = authenticate(&base, &id).await;
    register(&mut replacement, &registration(&id.machine_id, None, "660002")).await;
    assert_eq!(exchange(&base, &request.pin).await.status(), StatusCode::NOT_FOUND);
    println!("VERIFIED: old omitted-generation registration returned 404 after replacement without re-registration");
}

#[tokio::test]
#[ignore = "Focused post-remediation check for 93dc006"]
async fn verifies_caller_attempt_cannot_select_a_control_generation() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;
    let id = identity(99, "attempt-current");
    let mut original = authenticate(&base, &id).await;
    // Arbitrary client attempt values are ACK metadata, not control authorization.
    let current = registration(&id.machine_id, Some(999), "770001");
    register(&mut original, &current).await;
    observe_forwarded_claim(&base, &mut original, &current).await;
    // The successful forwarding advanced the server counter. Choose the exact future
    // control value (3) that replacement will receive; it must not revive this record.
    let stale = registration(&id.machine_id, Some(3), "770002");
    register(&mut original, &stale).await;
    let mut replacement = authenticate(&base, &id).await;
    register(&mut replacement, &registration(&id.machine_id, None, "770003")).await;
    assert_eq!(exchange(&base, &stale.pin).await.status(), StatusCode::NOT_FOUND);
    println!("VERIFIED: arbitrary attempt accepted on live owner; caller-selected future attempt did not revive old registration");
}
