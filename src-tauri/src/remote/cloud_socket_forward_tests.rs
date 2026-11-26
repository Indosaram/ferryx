//! End-to-end proof of the cloud socket-forward route.
//!
//! The relay router here is the unchanged one with end-to-end required: nothing in this file
//! calls `set_require_e2e(false)`, which the older DAG and PIN fixtures do. So the round-trip
//! below can only work if the bytes really travel inside an attach-encrypted session.
//!
//! The topology is the production one, minus the network:
//!
//! ```text
//!   test client --ws--> relay /tunnel/opaque/{sid} --+
//!                                                    relay (ciphertext only)
//!   machine (attach responder) --ws--> /tunnel/data/{sid}
//!        |
//!        +-- plaintext --> gateway router (`create_remote_router`)
//!                             |
//!                             +--> /api/v1/cloud/socket --> test unix listener
//! ```
//!
//! `proxy_secure_to_gateway` is the same splice the running daemon uses, so a byte that comes
//! back out of the unix listener has crossed: the client's websocket, its attach handshake, the
//! relay's ciphertext tunnel, the machine's attach responder, the gateway route, and the fixed
//! host-side socket path.

#![cfg(unix)]

use crate::remote::attach_client::{
    attach_opaque_session, load_or_generate_client_attach_identity,
};
use crate::remote::attach_crypto::{WebSocketByteStream, MAX_ATTACH_FRAME};
use crate::remote::attach_identity::AttachIdentity;
use crate::remote::machine_input_fixture::Fixture;
use crate::remote::relay_server::{relay_router, RelayState};
use crate::remote::session_transport::{
    establish_session, proxy_secure_to_gateway, SessionTransport,
};
use crate::remote::{DeviceAccessScope, DevicePermission};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt as _, StreamExt as _};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error as WsError, Message};
use tokio_tungstenite::MaybeTlsStream;

const DEADLINE: Duration = Duration::from_secs(20);
/// The machine id both halves bind into the attach prologue.
const MACHINE_ID: &str = "cloud-socket-machine";
const ENROLLMENT_EPOCH: &str = "4";
const TARGET: &str = "/api/v1/cloud/socket";

/// A unix listener standing in for `$HOME/.ferryx-cloud/rpc.sock`.
///
/// It echoes, so a byte coming back proves the path moved it both ways. It also counts accepted
/// connections, which is what lets the refusal test prove *nothing* was opened rather than
/// merely that an answer was 403.
struct TestRpcSocket {
    path: std::path::PathBuf,
    accepts: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl TestRpcSocket {
    fn start() -> Self {
        let dir = tempfile::tempdir().expect("socket dir");
        let path = dir.path().join("rpc.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind test rpc socket");
        let accepts = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&accepts);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16 * 1024];
                    loop {
                        match stream.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => {
                                if stream.write_all(&buffer[..read]).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                });
            }
        });
        Self {
            path,
            accepts,
            task,
            _dir: dir,
        }
    }

    fn accepts(&self) -> usize {
        self.accepts.load(Ordering::SeqCst)
    }
}

impl Drop for TestRpcSocket {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A relay, a gateway router, and the machine half of one opaque session, wired as production
/// wires them.
struct Harness {
    fixture: Fixture,
    rpc: TestRpcSocket,
    relay_origin: String,
    session: String,
    client_identity: AttachIdentity,
    machine_attach: AttachIdentity,
    /// Held so the client identity directory outlives the handshake that reads from it.
    _client_dir: tempfile::TempDir,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Harness {
    async fn start() -> Self {
        let fixture = Fixture::new().await;
        let rpc = TestRpcSocket::start();
        // Point the route at this test's listener. Production has no such override: it always
        // resolves `$HOME/.ferryx-cloud/rpc.sock`, and no caller can influence it.
        *fixture.state.cloud_socket_target.write() = Some(rpc.path.clone());

        let relay = RelayState::new(vec![]);
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind relay");
        let relay_origin = format!("http://{}", relay_listener.local_addr().unwrap());
        let router = relay_router(relay.clone());
        let mut tasks = vec![tokio::spawn(async move {
            axum::serve(
                relay_listener,
                router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
            )
            .await
            .unwrap();
        })];

        let client_dir = tempfile::tempdir().expect("client identity dir");
        let client_identity =
            load_or_generate_client_attach_identity(client_dir.path()).expect("client identity");
        let expected_key = STANDARD
            .decode(&client_identity.public_key)
            .expect("client key bytes");
        let machine_attach = {
            let secret = x25519_dalek::StaticSecret::random_from_rng(rand::rngs::OsRng);
            AttachIdentity {
                public_key: STANDARD.encode(x25519_dalek::PublicKey::from(&secret).as_bytes()),
                private_key: STANDARD.encode(secret.to_bytes()),
            }
        };

        // One opaque session for this harness. `opaque = true` is what makes the relay carry
        // ciphertext and nothing else.
        let (generation, _notices, _grants) = relay.register_control_channel(MACHINE_ID.into());
        let session = format!("cloud-socket-{}", uuid::Uuid::new_v4());
        assert!(
            relay.issue_session(MACHINE_ID, &session, Some(generation), true),
            "an opaque session must be issuable for the registered machine"
        );

        let gateway_addr = fixture
            .base
            .trim_start_matches("http://")
            .to_owned();
        let ws_origin = relay_origin.replace("http://", "ws://");
        tasks.push(tokio::spawn({
            let session = session.clone();
            let machine_attach = machine_attach.clone();
            async move {
                let (ws, _) = tokio_tungstenite::connect_async(format!(
                    "{ws_origin}/tunnel/data/{session}"
                ))
                .await
                .expect("machine data half");
                let transport = establish_session(
                    WebSocketByteStream::new(ws),
                    true,
                    Some(&machine_attach),
                    MACHINE_ID,
                    &session,
                    ENROLLMENT_EPOCH,
                    move |key: &[u8; 32]| key.as_slice() == expected_key.as_slice(),
                )
                .await
                .expect("machine attach handshake");
                let SessionTransport::Attached(secure) = transport else {
                    panic!("the machine side must end up attached, never plaintext");
                };
                let gateway = tokio::net::TcpStream::connect(&gateway_addr)
                    .await
                    .expect("gateway connection");
                // Ends when either side closes, exactly as the daemon's proxy does.
                let _ = proxy_secure_to_gateway(secure, gateway).await;
            }
        }));

        Self {
            fixture,
            rpc,
            relay_origin,
            session,
            client_identity,
            machine_attach,
            _client_dir: client_dir,
            tasks,
        }
    }

    /// Dials `target` on the gateway through this harness's encrypted relay session.
    ///
    /// This is `open_secure_websocket`'s shape with the bearer supplied by the caller, so a
    /// refusal can be observed without first minting a ticket.
    async fn connect(
        &self,
        token: &str,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<MaybeTlsStream<crate::paired_host::secure_http::PairedStream>>,
        WsError,
    > {
        let ws_origin = self.relay_origin.replace("http://", "ws://");
        let (ws, _) = tokio_tungstenite::connect_async(format!(
            "{ws_origin}/tunnel/opaque/{}",
            self.session
        ))
        .await
        .expect("client data half");

        let secure = attach_opaque_session(
            ws,
            &self.client_identity,
            &self.machine_attach.public_key,
            MACHINE_ID,
            &self.session,
            ENROLLMENT_EPOCH,
        )
        .await
        .expect("client attach handshake");

        let (local, gateway) = tokio::io::duplex(256 * 1024);
        tokio::spawn(async move {
            let _ = proxy_secure_to_gateway(secure, gateway).await;
        });

        let mut request = format!("ws://localhost{TARGET}")
            .into_client_request()
            .expect("websocket request");
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("bearer header"),
        );
        let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
            .max_message_size(Some(MAX_ATTACH_FRAME))
            .max_frame_size(Some(MAX_ATTACH_FRAME));
        timeout(
            DEADLINE,
            tokio_tungstenite::client_async_with_config(
                request,
                MaybeTlsStream::Plain(crate::paired_host::secure_http::PairedStream::Tunnel(local)),
                Some(config),
            ),
        )
        .await
        .expect("bounded handshake")
        .map(|(socket, _)| socket)
    }

    /// A device token for `scope`, minted through the real pairing path.
    fn token(&self, scope: DeviceAccessScope) -> String {
        let pin = self
            .fixture
            .state
            .auth_manager
            .create_scoped_pairing_code(DevicePermission::Control, scope)
            .expect("scoped pairing code");
        let (token, device) = self
            .fixture
            .state
            .auth_manager
            .exchange_pairing_code(&pin, "forward test device")
            .expect("pairing token");
        assert_eq!(device.access_scope, scope);
        token
    }

    async fn cleanup(mut self) {
        for task in self.tasks.drain(..) {
            task.abort();
        }
        self.fixture.cleanup().await;
    }
}

#[tokio::test]
async fn bytes_round_trip_through_the_forwarded_socket() {
    let harness = Harness::start().await;
    let payload = b"{\"type\":\"get_protocol_info\"}\n".to_vec();

    let mut socket = harness
        .connect(&harness.fixture.token)
        .await
        .expect("a machine/control device must be admitted");
    socket
        .send(Message::Binary(payload.clone().into()))
        .await
        .expect("send");

    let echoed = timeout(DEADLINE, socket.next())
        .await
        .expect("bounded reply")
        .expect("a frame")
        .expect("a healthy frame");
    assert_eq!(
        echoed,
        Message::Binary(payload.into()),
        "the host socket must see and echo the bytes"
    );
    assert_eq!(
        harness.rpc.accepts(),
        1,
        "exactly one host-socket connection per forwarded websocket"
    );
    harness.cleanup().await;
}

#[tokio::test]
async fn a_mirror_grant_is_refused_without_opening_the_socket() {
    let harness = Harness::start().await;
    let mirror = harness.token(DeviceAccessScope::Mirror);

    // Matched rather than `expect_err`, which would require the admitted stream to be Debug for
    // a refusal path that never produces one.
    let refusal = match harness.connect(&mirror).await {
        Ok(_) => panic!("a mirror grant must not be admitted"),
        Err(error) => error,
    };
    let WsError::Http(response) = refusal else {
        panic!("expected an HTTP refusal, got {refusal:?}");
    };
    assert_eq!(response.status().as_u16(), 403);
    let body = String::from_utf8_lossy(response.body().as_deref().unwrap_or_default()).into_owned();
    assert_eq!(
        body.lines().next(),
        Some("MACHINE_ACCESS_REQUIRED"),
        "the refusal must be for scope, not for a missing socket: {body:?}"
    );
    assert_eq!(
        harness.rpc.accepts(),
        0,
        "a refused grant must not connect to the host socket at all"
    );
    harness.cleanup().await;
}

#[tokio::test]
async fn the_plaintext_relay_path_still_refuses_this_route() {
    let harness = Harness::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    // The relay serves plaintext routes only behind a ticket, and refuses every one of them
    // with 426 before it looks at the target. This route must stay outside that set entirely.
    let ticket = client
        .post(format!(
            "{}/host/{MACHINE_ID}/api/v1/socket-ticket",
            harness.relay_origin
        ))
        .bearer_auth(&harness.fixture.token)
        .json(&serde_json::json!({ "target": TARGET }))
        .send()
        .await
        .expect("relay reachable");
    assert_eq!(
        ticket.status().as_u16(),
        426,
        "the relay must answer E2E_REQUIRED before considering any target"
    );
    let body = ticket.text().await.unwrap();
    assert!(
        body.contains("E2E_REQUIRED"),
        "the refusal must name the code, got {body:?}"
    );

    // The plaintext socket route does not exist either, so a hand-rolled upgrade cannot slip
    // past the ticket check.
    let upgrade = client
        .get(format!(
            "{}/host/{MACHINE_ID}{TARGET}",
            harness.relay_origin
        ))
        .bearer_auth(&harness.fixture.token)
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .expect("relay reachable");
    assert_ne!(
        upgrade.status().as_u16(),
        101,
        "the relay must never upgrade a plaintext cloud socket"
    );
    assert_eq!(
        harness.rpc.accepts(),
        0,
        "a refused plaintext attempt must not reach the host socket"
    );
    harness.cleanup().await;
}

#[tokio::test]
async fn the_gateway_ticket_allow_list_names_this_exact_route() {
    let fixture = Fixture::new().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let ticket = |target: &'static str| {
        let client = client.clone();
        let base = fixture.base.clone();
        let token = fixture.token.clone();
        async move {
            client
                .post(format!("{base}/api/v1/socket-ticket"))
                .bearer_auth(token)
                .json(&serde_json::json!({ "target": target }))
                .send()
                .await
                .expect("gateway reachable")
                .status()
                .as_u16()
        }
    };
    assert_eq!(
        ticket(TARGET).await,
        200,
        "the gateway's own allow-list must admit the cloud socket target"
    );
    // An adjacent path is refused, so the entry is the exact route rather than a prefix.
    assert_eq!(ticket("/api/v1/cloud/socket/../../etc/passwd").await, 400);
    assert_eq!(ticket("/api/v1/cloud/socketish").await, 400);
    fixture.cleanup().await;
}

#[tokio::test]
async fn a_machine_control_host_advertises_the_forward() {
    let fixture = Fixture::new().await;
    let capabilities: serde_json::Value = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{}/api/v1/capabilities", fixture.base))
        .bearer_auth(&fixture.token)
        .send()
        .await
        .expect("gateway reachable")
        .json()
        .await
        .expect("capability document");
    let names = capabilities["capabilities"]
        .as_array()
        .expect("capability array");
    assert!(
        names
            .iter()
            .any(|value| value == crate::remote::cloud_socket::SOCKET_FORWARD_CAPABILITY),
        "a machine/control host must advertise the forward, got {names:?}"
    );
    fixture.cleanup().await;
}
