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
        // Agent control is off on a fresh daemon (the owner opts in); every route here is an
        // agent-control route, so the fixture is the owner who turned it on. The off state is
        // pinned by `with_agent_control_off_a_machine_control_device_cannot_mutate`.
        daemon.remote_state().agent_control_policy.set_enabled(true);
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
        self.post_to("/api/v1/supervisor/pause", token, body).await
    }

    async fn post_to(&self, path: &str, token: &str, body: &str) -> (u16, Option<String>, serde_json::Value) {
        self.send(
            client()
                .post(format!("http://{}{path}", self.addr))
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
    // Agent control stays on; only the allowlist leaves this device out.
    assert!(gateway.state().agent_control_policy.enabled());
    gateway.state().agent_control_policy.restrict_to(["another-device"]);

    let (status, _, body) = gateway.post(&token, PAUSE).await;

    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    gateway.stop().await;
}

#[tokio::test]
async fn with_agent_control_off_a_machine_control_device_cannot_mutate() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().agent_control_policy.set_enabled(false);

    let (status, _, body) = gateway.post(&token, PAUSE).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    let request = r#"{"requestId":"off-1","action":"message","payload":{"text":"hi"}}"#;
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/request", &token, request)
        .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "FORBIDDEN");
    gateway.stop().await;
}

#[tokio::test]
async fn with_agent_control_off_reads_and_malformed_bodies_are_forbidden() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().agent_control_policy.set_enabled(false);

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
async fn a_start_without_an_open_hub_is_supervisor_unavailable() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    let (status, _, body) = gateway.post_to("/api/v1/supervisor/start", &token, "").await;

    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], SUPERVISOR_UNAVAILABLE_CODE);
    gateway.stop().await;
}

#[tokio::test]
async fn only_an_admitted_machine_control_device_may_start_the_hub() {
    let gateway = Gateway::start().await;
    let mirror = gateway.token(DevicePermission::Control, DeviceAccessScope::Mirror);
    let outsider = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().set_agent_control(true, Vec::new()).expect("enable policy to exercise denials");
    gateway.state().agent_control_policy.restrict_to(["another-device"]);

    for token in [&mirror, &outsider] {
        let (status, _, body) = gateway.post_to("/api/v1/supervisor/start", token, "").await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], "FORBIDDEN");
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

// ---------------------------------------------------------------------------------------------
// The unified supervisor API (todo 13) over the real router: a populated hub for the authorized
// 200 / stale 409 / malformed 400 paths, mirror 403, and a closed hub's 503.

const OWNER: &str = "desktop:qa-owner";

/// A hub with one owner, one project and one admitted live goal; returns (goal id, project id).
async fn populate(gateway: &Gateway) -> (ferryx_hub::goals::SqlitePool, String, String) {
    use ferryx_hub::supervisor as sv;
    let pool = ferryx_hub::storage::memory().await.expect("hub db");
    let (identity, project, _) = sv::provision_owner(
        &pool,
        OWNER,
        "qa-supervisor",
        "github.com/example/qa",
        "local",
        "/qa/repo/.git",
    )
    .await
    .expect("owner setup");
    let request = sv::AdmitRequest {
        supervisor_id: identity.supervisor_id.clone(),
        owner_principal: OWNER.into(),
        source_adapter: sv::ChannelProvider::Desktop,
        source_message_id: "qa-fixture-1".into(),
        body: "Create result.txt containing READY.".into(),
        project_id: project.project_id.clone(),
        channel_binding_id: None,
        target: sv::ExecutionTarget {
            kind: sv::ExecutionKind::Pty,
            host_id: "local".into(),
        },
        title: "Write result.txt".into(),
        cwd: "/qa/repo".into(),
        instruction: "Create result.txt containing READY.".into(),
        requirements: vec![ferryx_hub::goals::contract::RequirementSpec {
            id: "r1".into(),
            text: "result.txt contains READY".into(),
            verifier: "verify.sh exits 0".into(),
        }],
        mode: ferryx_hub::goals::GoalMode::Full,
        turn_limit: sv::DEFAULT_TURN_LIMIT,
        operation_grant: Vec::new(),
    };
    let goal_id = sv::admit_request(&pool, &request)
        .await
        .expect("admit")
        .receipt
        .goal_id;
    *gateway.state().supervisor_test_hub.write() = Some((pool.clone(), OWNER.into()));
    (pool, goal_id, project.project_id)
}

async fn goal_row(pool: &ferryx_hub::goals::SqlitePool, goal_id: &str) -> (String, i64) {
    let goal = ferryx_hub::supervisor::goal_of(pool, goal_id)
        .await
        .expect("goal")
        .expect("goal row");
    (goal.run_state.as_str().to_owned(), goal.revision)
}

#[tokio::test]
async fn an_authorized_device_pauses_a_live_goal_and_a_stale_revision_is_409() {
    let gateway = Gateway::start().await;
    let (pool, goal_id, project_id) = populate(&gateway).await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);

    let (status, cache, snapshot) = gateway.get("/api/v1/supervisor", &token).await;
    assert_eq!(status, 200, "{snapshot}");
    assert_eq!(cache.as_deref(), Some("no-store"));
    let goals = snapshot["supervisor"]["goals"].as_array().expect("goals");
    assert_eq!(goals.len(), 1, "{snapshot}");
    assert_eq!(goals[0]["goalId"], goal_id.as_str());
    assert_eq!(goals[0]["runState"], "ready");
    assert_eq!(goals[0]["revision"], 1);
    assert_eq!(snapshot["supervisor"]["identity"]["ownerPrincipal"], OWNER);

    let pause = serde_json::json!({"requestId": "qa-pause-1", "projectId": project_id, "goalId": goal_id,
        "expectedRevision": 1, "action": "pause", "payload": {"reason": "phone"}});
    let (status, cache, body) = gateway
        .post_to("/api/v1/supervisor/request", &token, &pause.to_string())
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(cache.as_deref(), Some("no-store"));
    assert_eq!(body["runState"], "paused");
    assert_eq!(body["revision"], 2);
    assert_eq!(goal_row(&pool, &goal_id).await, ("paused".into(), 2));

    // The same decision replayed at the old revision: 409, nothing changes.
    let resume = serde_json::json!({"requestId": "qa-resume-1", "goalId": goal_id,
        "expectedRevision": 1, "action": "resume", "payload": {}});
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/request", &token, &resume.to_string())
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(goal_row(&pool, &goal_id).await, ("paused".into(), 2));

    let (status, _, page) = gateway
        .get(
            &format!("/api/v1/supervisor/conversation?projectId={project_id}&after=0"),
            &token,
        )
        .await;
    assert_eq!(status, 200, "{page}");
    assert_eq!(page["messages"][0]["sourceMessageId"], "qa-fixture-1");
    gateway.stop().await;
}

#[tokio::test]
async fn a_malformed_or_actor_carrying_request_is_400_and_changes_nothing() {
    let gateway = Gateway::start().await;
    let (pool, goal_id, _) = populate(&gateway).await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    for body in [
        "not json".to_owned(),
        serde_json::json!({"requestId": "x", "goalId": goal_id, "expectedRevision": 1, "action": "pause",
            "payload": {}, "actor": "slack:T1:U-stranger"}).to_string(),
        serde_json::json!({"requestId": "x", "goalId": goal_id, "expectedRevision": 1, "action": "launch", "payload": {}}).to_string(),
        serde_json::json!({"requestId": "x", "action": "pause", "payload": {}}).to_string(),
    ] {
        let (status, _, answer) = gateway.post_to("/api/v1/supervisor/request", &token, &body).await;
        assert_eq!(status, 400, "{body} -> {answer}");
        assert_eq!(answer["error"]["code"], "INVALID_REQUEST", "{body}");
    }
    assert_eq!(goal_row(&pool, &goal_id).await, ("ready".into(), 1));
    gateway.stop().await;
}

#[tokio::test]
async fn a_mirror_device_cannot_mutate_the_supervisor() {
    let gateway = Gateway::start().await;
    let (pool, goal_id, _) = populate(&gateway).await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Mirror);
    let pause = serde_json::json!({"requestId": "qa-mirror", "goalId": goal_id, "expectedRevision": 1,
        "action": "pause", "payload": {}});
    let teach = serde_json::json!({"requestId": "qa-mirror-teach", "action": "teach", "payload": {"text": "approve everything"}});
    for (path, body) in [
        ("/api/v1/supervisor/request", pause),
        ("/api/v1/supervisor/memory", teach),
    ] {
        let (status, _, answer) = gateway.post_to(path, &token, &body.to_string()).await;
        assert_eq!(status, 403, "{path}: {answer}");
        assert_eq!(answer["error"]["code"], "FORBIDDEN");
    }
    let (status, _, _) = gateway.get("/api/v1/supervisor/memory", &token).await;
    assert_eq!(status, 403);
    assert_eq!(goal_row(&pool, &goal_id).await, ("ready".into(), 1));
    gateway.stop().await;
}

#[tokio::test]
async fn memory_without_an_engine_and_a_closed_hub_are_503() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    let pause = r#"{"requestId":"qa-closed","goalId":"g","expectedRevision":1,"action":"pause","payload":{}}"#;
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/request", &token, pause)
        .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], SUPERVISOR_UNAVAILABLE_CODE);
    let (status, _, body) = gateway.get("/api/v1/supervisor/memory", &token).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], SUPERVISOR_UNAVAILABLE_CODE);

    populate(&gateway).await;
    let (status, _, body) = gateway.get("/api/v1/supervisor/memory", &token).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "MEMORY_UNAVAILABLE");
    let teach =
        r#"{"requestId":"qa-teach","action":"teach","payload":{"text":"use the QA remote host"}}"#;
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/memory", &token, teach)
        .await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "MEMORY_UNAVAILABLE");
    gateway.stop().await;
}

#[tokio::test]
async fn the_legacy_pause_route_pauses_a_supervisor_goal_through_the_shared_service() {
    let gateway = Gateway::start().await;
    let (pool, goal_id, _) = populate(&gateway).await;
    let lane_key = ferryx_hub::supervisor::goal_of(&pool, &goal_id)
        .await
        .unwrap()
        .unwrap()
        .lane_key;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    let pause = serde_json::json!({"laneKey": lane_key, "paused": true}).to_string();
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/pause", &token, &pause)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["changed"], true, "the legacy answer shape is kept");
    assert_eq!(
        goal_row(&pool, &goal_id).await,
        ("paused".into(), 2),
        "revision bump and paused run state"
    );
    let (_, _, again) = gateway
        .post_to("/api/v1/supervisor/pause", &token, &pause)
        .await;
    assert_eq!(again["changed"], false);
    let resume = serde_json::json!({"laneKey": lane_key, "paused": false}).to_string();
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/pause", &token, &resume)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(goal_row(&pool, &goal_id).await, ("ready".into(), 2));
    gateway.stop().await;
}

// ---------------------------------------------------------------------------------------------
// T9 direct controls: the session stop, the decision tap, the global pause and the undo, over the
// real router. Auth runs before routing and before the hub, so every refusal class is 403 with
// nothing typed and nothing changed.

const SESSION_STOP: &str = "/api/v1/sessions/local:no-such-pane/stop";
const DECISION_ANSWER: &str = "/api/v1/supervisor/decisions/local:no-such-pane/answer";
const ACTION_UNDO: &str = "/api/v1/supervisor/actions/1/undo";
const TAP: &str = r#"{"promptId":"p","screenRevision":"r","optionIndex":0}"#;
const GLOBAL_PAUSE: &str = r#"{"paused":true}"#;

#[tokio::test]
async fn every_direct_control_route_refuses_a_mirror_device_and_one_outside_the_allowlist() {
    let gateway = Gateway::start().await;
    let mirror = gateway.token(DevicePermission::Control, DeviceAccessScope::Mirror);
    let outsider = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().agent_control_policy.restrict_to(["another-device"]);

    for token in [&mirror, &outsider] {
        for (path, body) in [
            (SESSION_STOP, ""),
            (DECISION_ANSWER, TAP),
            (ACTION_UNDO, ""),
            ("/api/v1/supervisor/pause", GLOBAL_PAUSE),
        ] {
            let (status, _, answer) = gateway.post_to(path, token, body).await;
            assert_eq!(status, 403, "{path} -> {answer}");
            assert_eq!(answer["error"]["code"], "FORBIDDEN", "{path}");
        }
    }
    gateway.stop().await;
}

#[tokio::test]
async fn with_agent_control_off_the_direct_controls_are_forbidden_before_their_body_is_read() {
    let gateway = Gateway::start().await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    gateway.state().agent_control_policy.set_enabled(false);

    for (path, body) in [
        (SESSION_STOP, "not json"),
        (DECISION_ANSWER, "not json"),
        (ACTION_UNDO, ""),
        ("/api/v1/supervisor/pause", "not json"),
    ] {
        let (status, _, answer) = gateway.post_to(path, &token, body).await;
        assert_eq!(status, 403, "{path} -> {answer}");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn a_missing_token_is_unauthorized_on_the_direct_controls() {
    let gateway = Gateway::start().await;
    for path in [SESSION_STOP, DECISION_ANSWER, ACTION_UNDO] {
        let (status, _, answer) = gateway
            .send(client().post(format!("http://{}{path}", gateway.addr)))
            .await;
        assert_eq!(status, 401, "{path} -> {answer}");
        assert_eq!(answer["error"]["code"], "UNAUTHORIZED", "{path}");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn a_stop_routes_by_session_ref_and_stays_on_this_computer() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    // This computer, no such pane: the route reached agent control (typed NOT_FOUND), no hub needed.
    let (status, _, body) = gateway.post_to(SESSION_STOP, &token, "").await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    let audit = gateway.state().agent_control_audit.latest(1);
    assert_eq!(audit[0].op, "stop", "the stop went through agent control and was audited");
    assert_eq!(audit[0].code, "NOT_FOUND");
    let audited = gateway.state().agent_control_audit.recent().len();

    // Another computer's ref never reaches a local pane: refused before agent control runs.
    let (status, _, body) = gateway
        .post_to("/api/v1/sessions/otherhost:no-such-pane/stop", &token, "")
        .await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    assert_eq!(gateway.state().agent_control_audit.recent().len(), audited, "nothing was attempted locally");

    // A handle with no host is malformed.
    let (status, _, body) = gateway.post_to("/api/v1/sessions/nohost/stop", &token, "").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "INVALID_REQUEST");
    gateway.stop().await;
}

#[tokio::test]
async fn a_decision_tap_needs_its_exact_body_and_is_audited_as_an_answer() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    for body in [r#"{"promptId":"p"}"#, r#"{"promptId":"p","screenRevision":"r","optionIndex":"0"}"#, "not json"] {
        let (status, _, answer) = gateway.post_to(DECISION_ANSWER, &token, body).await;
        assert_eq!(status, 400, "{body} -> {answer}");
        assert_eq!(answer["error"]["code"], "INVALID_REQUEST", "{body}");
    }
    let (status, _, body) = gateway.post_to(DECISION_ANSWER, &token, TAP).await;
    assert_eq!(status, 404, "{body}");
    let audit = gateway.state().agent_control_audit.latest(1);
    assert_eq!((audit[0].op.as_str(), audit[0].code.as_str()), ("answer", "NOT_FOUND"));
    gateway.stop().await;
}

#[tokio::test]
async fn the_global_pause_without_a_hub_is_unavailable_and_a_malformed_body_is_invalid() {
    let gateway = Gateway::start().await;
    let token = gateway.authorized_machine_token();

    let (status, _, body) = gateway.post_to("/api/v1/supervisor/pause", &token, GLOBAL_PAUSE).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], SUPERVISOR_UNAVAILABLE_CODE);
    for body in [r#"{}"#, r#"{"paused":"yes"}"#, r#"{"paused":true,"extra":1}"#] {
        let (status, _, answer) = gateway.post_to("/api/v1/supervisor/pause", &token, body).await;
        assert_eq!(status, 400, "{body} -> {answer}");
    }
    gateway.stop().await;
}

#[tokio::test]
async fn the_same_pause_path_pauses_the_whole_supervisor_or_one_lane_by_body_shape() {
    let gateway = Gateway::start().await;
    let (pool, goal_id, _) = populate(&gateway).await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    assert!(!ferryx_hub::control::is_paused(&pool).await.unwrap());

    let (status, cache, body) = gateway.post_to("/api/v1/supervisor/pause", &token, GLOBAL_PAUSE).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(cache.as_deref(), Some("no-store"));
    assert_eq!((body["paused"].as_bool(), body["changed"].as_bool()), (Some(true), Some(true)));
    assert!(body.get("laneKey").is_none(), "the global form names no lane");
    assert!(ferryx_hub::control::is_paused(&pool).await.unwrap(), "the flag the hub reads is set");
    assert_eq!(goal_row(&pool, &goal_id).await, ("ready".into(), 1), "no lane or goal was touched");

    let (_, _, again) = gateway.post_to("/api/v1/supervisor/pause", &token, GLOBAL_PAUSE).await;
    assert_eq!(again["changed"], false, "pausing twice changes nothing");
    let (status, _, body) = gateway
        .post_to("/api/v1/supervisor/pause", &token, r#"{"paused":false}"#)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!ferryx_hub::control::is_paused(&pool).await.unwrap(), "resume clears the flag");

    // The lane form on the same path still pauses that one goal and leaves the global flag alone.
    let lane_key = ferryx_hub::supervisor::goal_of(&pool, &goal_id).await.unwrap().unwrap().lane_key;
    let lane = serde_json::json!({"laneKey": lane_key, "paused": true}).to_string();
    let (status, _, body) = gateway.post_to("/api/v1/supervisor/pause", &token, &lane).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["laneKey"], lane_key.as_str());
    assert_eq!(goal_row(&pool, &goal_id).await, ("paused".into(), 2));
    assert!(!ferryx_hub::control::is_paused(&pool).await.unwrap(), "a lane pause is not the global pause");
    gateway.stop().await;
}

#[tokio::test]
async fn an_action_undo_restores_a_rule_once_then_has_nothing_left() {
    use ferryx_hub::supervisor::activity::SqliteActionLog;
    use ferryx_hub::supervisor::panel_contract::{ActionEntry, ActionKind, ActionLog, RuleStore, UndoHandle};
    use ferryx_hub::supervisor::rules::SqliteRuleStore;
    let gateway = Gateway::start().await;
    let (pool, _, _) = populate(&gateway).await;
    let token = gateway.token(DevicePermission::Control, DeviceAccessScope::Machine);
    let rules = SqliteRuleStore::open(pool.clone(), 1).await.unwrap();
    let before = rules.list().await.unwrap()[0].clone();
    let mut changed = before.clone();
    changed.params = serde_json::json!({"backoffSecs": [300], "thenEverySecs": 300});
    rules.upsert(changed).await.unwrap();
    let stored = SqliteActionLog::new(pool.clone())
        .upsert_incident(ActionEntry {
            id: 0,
            incident_key: "rule-1".into(),
            session_ref: None,
            kind: ActionKind::RuleChanged,
            summary: "Changed the retry interval".into(),
            basis: "you asked".into(),
            evidence: Vec::new(),
            undo: Some(UndoHandle {
                kind: "restoreRule".into(),
                payload: serde_json::json!({ "rule": before.clone() }),
            }),
            live: false,
            created_at_ms: 10,
            updated_at_ms: 10,
        })
        .await
        .unwrap();

    let path = format!("/api/v1/supervisor/actions/{}/undo", stored.id);
    let (status, _, body) = gateway.post_to(&path, &token, "").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["undone"], stored.id);
    let restored = rules.list().await.unwrap().into_iter().find(|rule| rule.id == before.id).unwrap();
    assert_eq!(restored.params, before.params, "the previous rule is back");

    let (status, _, body) = gateway.post_to(&path, &token, "").await;
    assert_eq!(status, 404, "a spent handle has nothing left: {body}");
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    let (status, _, body) = gateway.post_to("/api/v1/supervisor/actions/x/undo", &token, "").await;
    assert_eq!(status, 400, "{body}");
    gateway.stop().await;
}
