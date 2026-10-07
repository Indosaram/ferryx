//! Regression source for the reference-chat host identity the capability document publishes.
//!
//! Task 12's mutation target names a host id, and this gateway refuses any host id that is not
//! its own `reference_host_id()` with a typed FORBIDDEN. The capability document is therefore the
//! ONLY place a client can learn which host id to name, and `machineId` — the host's pairing
//! identity — is a different value that must never stand in for it.
//!
//! Deferred execution: authored with the route, run only at the complete-code merge barrier
//! (`cargo test --manifest-path src-tauri/Cargo.toml --lib capability_tests`).
//!
//! Environment safety: nothing here writes the process environment. The identity mapping is
//! asserted through the pure builder the handler calls, and the one environment-dependent value
//! (the host id) is READ and compared against the same environment, so this file cannot race a
//! neighbouring test the way a process-wide set_var would.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::reference_chat::files::{
    reference_host_id, REFERENCE_HOST_ID_ENV, REFERENCE_LOCAL_HOST_ID,
};
use crate::remote::server::{create_remote_router, reference_chat_capability_identity};
use crate::remote::state::RemoteGatewayState;

/// The capability route this contract is published on.
const CAPABILITIES_PATH: &str = "/api/v1/capabilities";

/// One bound gateway serving the production router.
struct CapabilityServer {
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl CapabilityServer {
    async fn start(state: Arc<RemoteGatewayState>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, create_remote_router(state))
                .await
                .expect("serve");
        });
        Self { addr, tasks }
    }

    async fn capabilities(&self, token: Option<&str>) -> (u16, String) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("client");
        let mut request = client.get(format!("http://{}{CAPABILITIES_PATH}", self.addr));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.expect("a response");
        let status = response.status().as_u16();
        (status, response.text().await.unwrap_or_default())
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

/// A gateway with its own private identity directory and a Control machine token.
async fn capability_fixture() -> (tempfile::TempDir, CapabilityServer, String) {
    let root = tempfile::tempdir().expect("tempdir");
    let daemon = crate::daemon::server::DaemonServer::new_with_paths(
        Some(root.path().join("data/config")),
        Some(root.path().join("data/auth")),
    );
    let state = daemon.remote_state().clone();
    let pin = state
        .auth_manager
        .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
        .expect("pairing code");
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&pin, "capability-reader")
        .expect("token");
    let server = CapabilityServer::start(state).await;
    (root, server, token)
}

/// Each id lands in its own field: the reference host id is not derived from the machine id.
#[test]
fn the_capability_identity_publishes_each_id_in_its_own_field() {
    let fields = reference_chat_capability_identity("machine-abc", "renamed-host", "owner-1");
    assert_eq!(
        fields.get("machineId").and_then(|value| value.as_str()),
        Some("machine-abc")
    );
    assert_eq!(
        fields.get("referenceHostId").and_then(|value| value.as_str()),
        Some("renamed-host")
    );
    assert_eq!(
        fields.get("referenceOwnerId").and_then(|value| value.as_str()),
        Some("owner-1")
    );
    // Exactly the three fields: no alias a client could read one identity out of.
    let names: Vec<&str> = fields.keys().map(String::as_str).collect();
    assert_eq!(names, vec!["machineId", "referenceHostId", "referenceOwnerId"]);
}

/// The two ids are independent inputs, not one value published twice.
#[test]
fn the_capability_identity_never_derives_one_id_from_the_other() {
    let distinct = reference_chat_capability_identity("machine-abc", "renamed-host", "owner-1");
    assert_ne!(
        distinct.get("machineId").and_then(|value| value.as_str()),
        distinct.get("referenceHostId").and_then(|value| value.as_str()),
        "distinct inputs must stay distinct; a shared field would let machineId stand in"
    );

    // The same input for both is copied into both: the function invents neither.
    let identical = reference_chat_capability_identity("same-value", "same-value", "same-value");
    assert_eq!(
        identical.get("machineId").and_then(|value| value.as_str()),
        Some("same-value")
    );
    assert_eq!(
        identical.get("referenceHostId").and_then(|value| value.as_str()),
        Some("same-value")
    );
}

/// The resolver's documented contract, read without writing the environment.
#[test]
fn the_reference_host_id_contract_names_its_environment_and_default() {
    assert_eq!(REFERENCE_HOST_ID_ENV, "FERRYX_HOST_ID");
    assert_eq!(REFERENCE_LOCAL_HOST_ID, "local");

    let resolved = reference_host_id();
    let from_environment = std::env::var(REFERENCE_HOST_ID_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    match from_environment {
        // A deployment that renames its host publishes that name, verbatim.
        Some(named) => assert_eq!(resolved, named),
        // Otherwise the documented default, never the machine identity.
        None => assert_eq!(resolved, REFERENCE_LOCAL_HOST_ID),
    }
    assert!(
        !resolved.trim().is_empty(),
        "a host id a client must name can never be empty"
    );
}

/// Authentication precedes the identity document.
#[tokio::test]
async fn capabilities_require_authentication_before_publishing_any_identity() {
    let (_root, server, _token) = capability_fixture().await;
    let (status, body) = server.capabilities(None).await;
    assert_eq!(status, 401, "an unauthenticated capability read: {body}");
    assert!(
        !body.contains("referenceHostId"),
        "no identity may be published before authentication: {body}"
    );
    server.stop().await;
}

/// The authenticated document publishes the gateway's own reference host id, beside — never as —
/// the machine id.
#[tokio::test]
async fn capabilities_publish_the_gateway_reference_host_id() {
    let (_root, server, token) = capability_fixture().await;
    let (status, body) = server.capabilities(Some(&token)).await;
    assert_eq!(status, 200, "{body}");
    let document: serde_json::Value = serde_json::from_str(&body).expect("a capability document");

    let published_host = document["referenceHostId"]
        .as_str()
        .expect("the document must publish a reference host id string");
    assert_eq!(
        published_host,
        reference_host_id(),
        "the published host id is the resolver's own value"
    );
    assert!(
        !published_host.trim().is_empty(),
        "a client cannot name an empty host id: {body}"
    );

    let machine_id = document["machineId"]
        .as_str()
        .expect("the machine identity is still published");
    assert!(!machine_id.trim().is_empty(), "{body}");
    assert_ne!(
        document["referenceHostId"], document["machineId"],
        "machineId must not be published as the reference host id: {body}"
    );

    let epoch = document["daemonEpoch"]
        .as_str()
        .expect("the incarnation a target is compared against");
    assert!(
        epoch == "0" || (!epoch.starts_with('0') && epoch.chars().all(|c| c.is_ascii_digit())),
        "daemonEpoch must be canonical decimal: {epoch}"
    );

    server.stop().await;
}
