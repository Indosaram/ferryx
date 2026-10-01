use super::*;
use crate::paired_host::direct::{DirectConnectionOffer, DirectRole};
use crate::paired_host::direct_wire::offer_session_id;
use crate::remote::auth::load_or_generate_machine_identity;
use crate::terminal::TerminalService;
use ed25519_dalek::SigningKey;
use std::net::{Ipv4Addr, SocketAddrV4};

const CLIENT: &str = "client-machine";
const NOW: u64 = 1_800_000_000_000;
const LIMIT: Duration = Duration::from_secs(10);

fn fixed_now() -> u64 {
    NOW
}

fn skewed_now() -> u64 {
    NOW + 61_000
}

fn client_key() -> SigningKey {
    SigningKey::from_bytes(&[1; 32])
}

struct Fixture {
    _dir: tempfile::TempDir,
    state: Arc<RemoteGatewayState>,
    host_id: String,
}

fn fixture(trust_client: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(RemoteGatewayState::new_with_paths(
        Arc::new(TerminalService::default()),
        crate::worktree::WorkspaceRegistry::new(),
        Some(dir.path().join("remote-config.json")),
        Some(dir.path().join("remote-auth.json")),
    ));
    let host_id = load_or_generate_machine_identity(dir.path()).unwrap().machine_id;
    if trust_client {
        let public = STANDARD.encode(client_key().verifying_key().to_bytes());
        DirectTrustStore::at(dir.path()).provision(CLIENT, &public, false).unwrap();
    }
    Fixture { _dir: dir, state, host_id }
}

fn bearer(state: &RemoteGatewayState, scope: DeviceAccessScope) -> HeaderMap {
    let code = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, scope)
        .unwrap();
    let (token, _) = state.auth_manager.exchange_pairing_code(&code, "direct fixture").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(header::AUTHORIZATION, format!("Bearer {token}").parse().unwrap());
    headers
}

fn offer_body(signer: &SigningKey, target: &str, version: u32) -> Vec<u8> {
    let offer = DirectConnectionOffer::new_at(
        offer_session_id(CLIENT, target),
        SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 40_000),
        vec![7; 32],
        [3; 32],
        DirectRole::Initiator,
        NOW,
        signer,
    );
    serde_json::to_vec(&DirectOfferEnvelope {
        version,
        source_machine_id: CLIENT.into(),
        target_machine_id: target.into(),
        offer,
    })
    .unwrap()
}

fn manager(now_ms: fn() -> u64) -> DirectHostManager {
    DirectHostManager::new(DirectHostConfig { now_ms, ..DirectHostConfig::default() })
}

async fn post(fx: &Fixture, headers: HeaderMap, body: Vec<u8>, now_ms: fn() -> u64) -> (u16, String, usize) {
    let manager = manager(now_ms);
    let response = tokio::time::timeout(LIMIT, handle_offer(Arc::clone(&fx.state), &headers, &body, &manager))
        .await
        .expect("bounded negotiation");
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
    let text = String::from_utf8_lossy(&bytes).into_owned();
    assert_eq!(manager.active_tasks(), 0, "a rejected offer must not spawn a host task");
    (status, text, manager.udp_binds())
}

#[tokio::test]
async fn unauthenticated_offer_is_rejected_before_any_udp() {
    let fx = fixture(true);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, _, binds) = post(&fx, HeaderMap::new(), body, fixed_now).await;
    assert_eq!((status, binds), (401, 0));
}

#[tokio::test]
async fn mirror_scope_token_cannot_negotiate_direct() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Mirror);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, _, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (403, 0));
}

#[tokio::test]
async fn untrusted_client_stays_on_relay() {
    let fx = fixture(false);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (403, 0));
    assert!(text.contains("DIRECT_UNTRUSTED"), "{text}");
}

#[tokio::test]
async fn substituted_client_identity_is_rejected() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&SigningKey::from_bytes(&[66; 32]), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (403, 0));
    assert!(text.contains("DIRECT_BAD_SIGNATURE"), "{text}");
}

#[tokio::test]
async fn replayed_offer_outside_window_is_rejected() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, skewed_now).await;
    assert_eq!((status, binds), (403, 0));
    assert!(text.contains("DIRECT_OFFER_EXPIRED"), "{text}");
}

#[tokio::test]
async fn offer_for_another_machine_is_rejected() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), "other-host", 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (403, 0));
    assert!(text.contains("DIRECT_WRONG_TARGET"), "{text}");
}

#[tokio::test]
async fn unknown_protocol_version_gets_no_direct() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 2);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (404, 0));
    assert!(text.contains("DIRECT_UNSUPPORTED"), "{text}");
}

#[tokio::test]
async fn verified_offer_without_local_gateway_binds_no_udp() {
    let fx = fixture(true);
    let headers = bearer(&fx.state, DeviceAccessScope::Machine);
    let body = offer_body(&client_key(), &fx.host_id, 1);
    let (status, text, binds) = post(&fx, headers, body, fixed_now).await;
    assert_eq!((status, binds), (503, 0));
    assert!(text.contains("DIRECT_UNAVAILABLE"), "{text}");
}

#[test]
fn gateway_target_must_be_loopback_with_a_real_port() {
    let fx = fixture(false);
    for (bound, expected) in [
        (Some("127.0.0.1:8899"), Some("127.0.0.1:8899")),
        (Some("127.0.0.1:0"), None),
        (Some("0.0.0.0:8899"), None),
        (Some("100.64.0.1:8899"), None),
        (None, None),
    ] {
        *fx.state.bound_address.write() = bound.map(str::to_string);
        assert_eq!(local_gateway_addr(&fx.state), expected.map(|a| a.parse().unwrap()), "{bound:?}");
    }
}

/// Exercises the real router: the route exists, rejects without a bearer, and the
/// body limit fires before the handler buffers an oversized offer.
#[tokio::test]
async fn router_mounts_offer_with_bearer_and_body_limit() {
    let fx = fixture(true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = crate::remote::server::create_remote_router(Arc::clone(&fx.state));
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let url = format!("http://{addr}/api/v1/direct/offer");
    let send = |body: Vec<u8>| client.post(&url).body(body).send();
    let small = tokio::time::timeout(LIMIT, send(offer_body(&client_key(), &fx.host_id, 1))).await;
    let large = tokio::time::timeout(LIMIT, send(vec![b' '; MAX_DIRECT_BODY_BYTES + 1])).await;
    server.abort();
    assert_eq!(small.expect("bounded").unwrap().status().as_u16(), 401);
    assert_eq!(large.expect("bounded").unwrap().status().as_u16(), 413);
}

#[path = "direct_lease_tests.rs"]
mod direct_lease_tests;
