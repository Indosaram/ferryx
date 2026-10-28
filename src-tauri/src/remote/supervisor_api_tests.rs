//! The Supervisor routes over the real router, against a daemon that never opened the hub.
//! The hub's process-global view is not set in unit tests, so these pin the hub-absent answers,
//! the access rules, and the machine event the phone refetches on.

use super::*;
use crate::agent_control::AgentControlOp;
use std::net::SocketAddr;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(10);

struct Gateway {
    _root: tempfile::TempDir,
    daemon: crate::daemon::server::DaemonServer,
    addr: SocketAddr,
    tasks: tokio::task::JoinSet<()>,
}

impl Gateway {
    async fn start() -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let daemon = crate::daemon::server::DaemonServer::new_with_paths(
            Some(root.path().join("data/config")),
            Some(root.path().join("data/auth")),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let router = create_remote_router(daemon.remote_state().clone());
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move { axum::serve(listener, router).await.expect("serve") });
        Self { _root: root, daemon, addr, tasks }
    }

    fn state(&self) -> &Arc<RemoteGatewayState> {
        self.daemon.remote_state()
    }

    fn token(&self, permission: DevicePermission, scope: DeviceAccessScope) -> String {
        let auth = &self.state().auth_manager;
        let pin = auth.create_scoped_pairing_code(permission, scope).expect("pin");
        auth.exchange_pairing_code(&pin, "supervisor fixture").expect("token").0
    }

    fn authorized_machine_token(&self) -> String {
        let token = self.token(DevicePermission::Control, DeviceAccessScope::Machine);
        let device = self.state().auth_manager.validate_token(&token).expect("fixture device");
        // Pairing a fresh machine does not opt it into agent control. Explicitly grant
        // only this fixture device access before testing the downstream hub/body contract.
        self.state().set_agent_control(true, Vec::new()).expect("enable fixture control");
        self.state().agent_control_policy.restrict_to([device.id]);
        token
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> (u16, Option<String>, serde_json::Value) {
        let response = tokio::time::timeout(LIMIT, request.send()).await.expect("bounded").expect("response");
        let status = response.status().as_u16();
        let cache = response
            .headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.json().await.unwrap_or(serde_json::Value::Null);
        (status, cache, body)
    }

    async fn get(&self, path: &str, token: &str) -> (u16, Option<String>, serde_json::Value) {
        self.send(client().get(format!("http://{}{path}", self.addr)).bearer_auth(token)).await
    }

    async fn post(&self, token: &str, body: &str) -> (u16, Option<String>, serde_json::Value) {
        self.send(
            client()
                .post(format!("http://{}/api/v1/supervisor/pause", self.addr))
                .bearer_auth(token)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.to_owned()),
        )
        .await
    }

    async fn stop(mut self) {
        self.tasks.shutdown().await;
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().timeout(LIMIT).build().expect("client")
}

const PAUSE: &str = r#"{"laneKey":"ledger","paused":true}"#;

#[tokio::test]
async fn a_mirror_device_cannot_read_the_supervisor() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Mirror);
    gateway.state().set_agent_control(true, Vec::new()).expect("enable policy to exercise scope denial");

    let (status, _, body) = gateway.get("/api/v1/supervisor", &token).await;

    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    gateway.stop().await;
}

#[tokio::test]
async fn a_device_outside_the_agent_control_allowlist_cannot_pause_a_lane() {
    // A view-only machine grant cannot be issued, so the refused writer is a machine-control
    // device that the agent-control policy no longer admits.
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().set_agent_control(true, Vec::new()).expect("enable policy to exercise allowlist denial");
    gateway.state().agent_control_policy.restrict_to(["another-device"]);

    let (status, _, body) = gateway.post(&token, PAUSE).await;

    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    gateway.stop().await;
}

#[tokio::test]
async fn a_fresh_machine_control_device_is_denied_until_agent_control_is_enabled() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    assert!(!gateway.state().agent_control_policy.enabled(), "fresh agent control stays off");

    let (status, _, body) = gateway.get("/api/v1/supervisor", &token).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    // Authorization remains ahead of both valid hub operations and malformed body handling.
    for request in [PAUSE, "not json"] {
        let (status, _, body) = gateway.post(&token, request).await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], "FORBIDDEN");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn a_machine_control_device_reads_the_snapshot_shape_without_a_hub() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    let (status, cache, body) = gateway.get("/api/v1/supervisor", &token).await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(cache.as_deref(), Some("no-store"));
    assert_eq!(body["hub"]["available"], false);
    assert_eq!(body["hub"]["running"], false);
    assert!(body["hub"]["error"].is_null(), "{body}");
    for list in ["lanes", "watched", "actions"] {
        assert!(body[list].is_array(), "{list} in {body}");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn a_pause_without_an_open_hub_is_supervisor_unavailable() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    let (status, _, body) = gateway.post(&token, PAUSE).await;

    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], SUPERVISOR_UNAVAILABLE_CODE);
    gateway.stop().await;
}

#[tokio::test]
async fn a_malformed_pause_body_is_an_invalid_request() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    for body in [r#"{"laneKey":"ledger"}"#, r#"{"laneKey":"ledger","paused":"yes"}"#, "not json"] {
        let (status, _, answer) = gateway.post(&token, body).await;

        assert_eq!(status, 400, "{body} -> {answer}");
        assert_eq!(answer["error"]["code"], "INVALID_REQUEST", "{body}");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn the_capability_is_absent_while_the_hub_is_closed() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);

    let (status, _, body) = gateway.get("/api/v1/capabilities", &token).await;

    assert_eq!(status, 200, "{body}");
    let capabilities = body["capabilities"].as_array().expect("capabilities");
    assert!(
        capabilities.iter().any(|value| value == crate::agent_control::AGENT_CONTROL_CAPABILITY),
        "a machine Control device sees agent control: {body}"
    );
    assert!(!capabilities.iter().any(|value| value == SUPERVISOR_CAPABILITY), "{body}");
    gateway.stop().await;
}

#[tokio::test]
async fn supervisor_input_is_published_as_a_machine_event() {
    let gateway = Gateway::start().await;
    let services = gateway.state().machine_services.as_ref().expect("machine services");
    let mut events = services.workspaces.machine_events.subscribe();

    // A supervisor send, even a refused one, is audited input the Supervisor view lists.
    let _refused = agent_control_service::handle_as(
        gateway.state(),
        AgentControlOp::Send { session_id: "missing-pane".into(), text: "go".into(), request_id: None },
        "owner",
        None,
    )
    .await;

    let event = tokio::time::timeout(LIMIT, async {
        loop {
            let event = events.recv().await.expect("machine event");
            if event["type"] == agent_control_service::SUPERVISOR_CHANGED_MACHINE_EVENT {
                return event;
            }
        }
    })
    .await
    .expect("supervisorChanged is published");
    assert_eq!(event["payload"]["sessionId"], "missing-pane");
    gateway.stop().await;
}
