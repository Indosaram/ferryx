//! Regression source for the malformed-read-query refusal on the reference-chat read routes.
//!
//! The five read handlers bind the query extractor's OUTCOME (`Result<Query<...>, QueryRejection>`)
//! and answer a rejection with the frozen machine envelope, so a client that parses the envelope
//! reads `INVALID_REQUEST` instead of meeting axum's own plain-text rejection. This case drives the
//! PRODUCTION router over real HTTP with a real Control token, so it fails again if a read handler
//! ever goes back to binding `Query` directly: the status is 400 either way, but only the envelope
//! parses.
//!
//! Deferred execution: authored with the route, run at the merge barrier
//! (`cargo test --manifest-path src-tauri/Cargo.toml --lib reference_chat_query_tests`).
//!
//! Environment safety: nothing here writes the process environment. The gateway gets its own
//! private data directories under a temporary root, so this module shares no state with the tests
//! beside it.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::remote::auth::{DeviceAccessScope, DevicePermission};
use crate::remote::machine_protocol::ErrorEnvelope;
use crate::remote::reference_chat::REFERENCE_CHAT_ROUTE_PREFIX;
use crate::remote::server::create_remote_router;
use crate::remote::state::RemoteGatewayState;

/// One bound gateway serving the production router on a loopback port.
struct ReadQueryServer {
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl ReadQueryServer {
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

    /// One authenticated GET, as a remote client sends it.
    async fn get(&self, path: &str, token: &str) -> (u16, String) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("client");
        let response = client
            .get(format!("http://{}{path}", self.addr))
            .bearer_auth(token)
            .send()
            .await
            .expect("a response");
        (
            response.status().as_u16(),
            response.text().await.unwrap_or_default(),
        )
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

/// A gateway with its own private data directories and a Control machine token.
async fn fixture() -> (tempfile::TempDir, ReadQueryServer, String) {
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
        .exchange_pairing_code(&pin, "reference-read-query")
        .expect("token");
    let server = ReadQueryServer::start(state).await;
    (root, server, token)
}

/// A malformed read query answers the frozen machine envelope, not axum's own rejection.
///
/// `cursor` is `Option<u64>` in the frozen read query, so `not-a-cursor` cannot be parsed into
/// it and the extractor rejects before the handler runs. Binding the extractor's outcome is what
/// turns that rejection into the same `INVALID_REQUEST` envelope every other refusal on this
/// surface carries. The assertion parses the envelope, so axum's plain-text body fails it even
/// though the status is 400 either way.
#[tokio::test]
async fn a_malformed_read_query_answers_the_machine_envelope() {
    let (_root, server, token) = fixture().await;
    let session = uuid::Uuid::new_v4().to_string();
    let path = format!("{REFERENCE_CHAT_ROUTE_PREFIX}/{session}/history?cursor=not-a-cursor");

    let (status, body) = server.get(&path, &token).await;
    server.stop().await;

    assert_eq!(
        status, 400,
        "a malformed read query is a bad request: {body}"
    );
    let envelope: ErrorEnvelope = serde_json::from_str(&body).unwrap_or_else(|error| {
        panic!(
            "the refusal must be the frozen machine envelope, not axum's own rejection: {error}: {body}"
        )
    });
    assert_eq!(envelope.error.code, "INVALID_REQUEST", "{body}");
    assert!(
        !envelope.error.retryable,
        "a malformed query is not retryable: {body}"
    );
    assert!(!envelope.error.message.is_empty(), "{body}");
    assert!(!envelope.error.request_id.is_empty(), "{body}");
}
