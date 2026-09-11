// Permanent relay pairing regressions over REAL loopback relay sockets.
//
// These lock in the repairs for the defects the Gen3 audit reproduced at 30555f2:
// a second machine's first pairing was denied because the relay compared the
// client's pairing-attempt counter with its own control generation, and a stale
// registration stayed claimable across control replacement. The harness is taken
// from the audit's own reproduction so the coverage matches how it was found.
//
// Unlike the audit's observation file these are NOT #[ignore]d: they must pass on
// every run, and a PASS here means the defect is absent.
// All peers use loopback sockets, synthetic identities/capabilities and temporary stores.
// The exchange responder is synthetic: these tests establish relay admission/forwarding,
// not unauthorized access to a real user's gateway or terminal.
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

/// The exact case the audit reproduced: an unrelated machine connects first and
/// advances the relay-wide counter, then a second machine performs its very first
/// pairing, which the real PairingCoordinator sends as attempt 1. That must pair.
#[tokio::test]
async fn second_machine_first_pairing_is_not_blocked_by_an_unrelated_connection() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
    let (base, _server) = serve(state).await;

    let mut unrelated = authenticate(&base, &identity(91, "unrelated")).await;
    register(&mut unrelated, &registration("unrelated", Some(1), "110001")).await;

    let mut second = authenticate(&base, &identity(92, "second-machine")).await;
    let request = registration("second-machine", Some(1), "110002");
    register(&mut second, &request).await;

    // The relay must accept the claim and forward it to the owning daemon. The
    // synthetic responder's 409 proves forwarding happened rather than rejection.
    observe_forwarded_claim(&base, &mut second, &request).await;
}

/// A registration whose owner's control channel has been replaced must stop being
/// claimable, whether the client sent no attempt number or an arbitrary one.
#[tokio::test]
async fn stale_registration_stops_being_claimable_after_control_replacement() {
    for (seed, machine, generation, pin) in [
        (93u8, "optional-generation", None, "220001"),
        (94u8, "chosen-generation", Some(2), "230001"),
    ] {
        let dir = tempfile::tempdir_in(".").unwrap();
        let state = RelayState::new_with_key_store(vec![], dir.path().join("keys.json")).unwrap();
        let (base, _server) = serve(state).await;

        let id = identity(seed, machine);
        let mut original = authenticate(&base, &id).await;
        let request = registration(&id.machine_id, generation, pin);
        register(&mut original, &request).await;

        // Replace the owner's control channel. ACKing a DIFFERENT registration proves
        // the replacement is live without touching the PIN under test.
        let mut replacement = authenticate(&base, &id).await;
        let other = format!("{}9", &pin[..5]);
        register(&mut replacement, &registration(&id.machine_id, generation, &other)).await;

        assert_eq!(
            exchange(&base, &request.pin).await.status(),
            StatusCode::NOT_FOUND,
            "a registration from a superseded control generation must not be claimable ({machine})"
        );
    }
}
