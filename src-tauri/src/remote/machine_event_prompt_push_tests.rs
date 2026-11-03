//! The events socket's `agentPromptChanged` push on a daemon with machine services: the mirror
//! surface never names a machine-only session, and a machine-scope socket gets the push too.
use crate::{
    agent_control::{events::ObservationDraft, AgentControlState, AgentStateSource, AgentTier},
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        machine_protocol::RemoteTerminalTarget,
        server::create_remote_router,
    },
    scoped_contracts::Epoch,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

fn draft(session: &str, prompt: &str) -> ObservationDraft {
    ObservationDraft {
        session_id: session.into(),
        state: AgentControlState::Blocked,
        state_source: AgentStateSource::Screen,
        tier: AgentTier::Screen,
        prompt_id: Some(prompt.into()),
        kind: Some("question".into()),
        screen_revision: Some(format!("rev-{prompt}")),
        workspace_id: None,
    }
}

struct Fixture {
    root: tempfile::TempDir,
    owner: DaemonServer,
    tasks: tokio::task::JoinSet<()>,
    address: std::net::SocketAddr,
}

async fn fixture(scope: DeviceAccessScope) -> (Fixture, String) {
    let (root, owner, token) = crate::ipc::run_blocking(move || {
        let root = tempfile::tempdir().unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let auth = &owner.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, scope)
            .unwrap();
        let (token, _) = auth.exchange_pairing_code(&pin, "prompt-push").unwrap();
        Ok((root, owner, token))
    })
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut tasks = tokio::task::JoinSet::new();
    let router = create_remote_router(owner.remote_state().clone());
    tasks.spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        Fixture {
            root,
            owner,
            tasks,
            address,
        },
        token,
    )
}

impl Fixture {
    async fn events_socket(
        &self,
        token: &str,
    ) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>
    {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let ticket: Value = serde_json::from_str(
            &client
                .post(format!("http://{}/api/v1/socket-ticket", self.address))
                .bearer_auth(token)
                .header("content-type", "application/json")
                .body(json!({"target":"/api/v1/events"}).to_string())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
        )
        .unwrap();
        let (socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://{}/api/v1/events?ticket={}",
            self.address,
            ticket["ticket"].as_str().unwrap()
        ))
        .await
        .unwrap();
        socket
    }

    async fn stop(mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
        let Fixture { root, owner, .. } = self;
        drop(owner);
        crate::ipc::run_blocking(move || {
            root.close().unwrap();
            Ok(())
        })
        .await
        .unwrap();
    }
}

/// The next `agentPromptChanged` frame on the socket, skipping other event types.
async fn next_prompt_frame<S>(socket: &mut S) -> Value
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let Ok(text) = message.to_text() else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if event["type"] == "agentPromptChanged" {
                return event;
            }
        }
    })
    .await
    .expect("an agentPromptChanged frame")
}

#[tokio::test]
async fn a_machine_only_session_prompt_is_never_pushed_to_a_mirror_socket() {
    let (fixture, token) = fixture(DeviceAccessScope::Mirror).await;
    let state = fixture.owner.remote_state().clone();
    let services = state.machine_services.as_ref().unwrap().clone();
    // A session a machine client created: the journal owns it, so the mirror hides it.
    let target = RemoteTerminalTarget {
        machine_id: "machine".into(),
        daemon_epoch: Epoch(1),
        session_id: "machine-only".into(),
    };
    let resource = serde_json::to_string(&target).unwrap();
    let journal_services = services.clone();
    crate::ipc::run_blocking(move || {
        journal_services
            .workspaces
            .journal
            .begin("device", "request", "createSession", "digest", &resource)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .unwrap();
    assert_eq!(
        services.sessions.machine_only_async("machine-only").await,
        Ok(true)
    );
    let mut socket = fixture.events_socket(&token).await;

    state.agent_events.publish(draft("machine-only", "hidden"));
    state.agent_events.publish(draft("desktop", "shown"));
    // Frames are sent in bus order, so the first push being the desktop session's proves the
    // machine-only one was dropped rather than still in flight.
    let pushed = next_prompt_frame(&mut socket).await;
    assert_eq!(pushed["sessionId"], "desktop");
    assert_eq!(pushed["promptId"], "shown");

    drop(socket);
    fixture.stop().await;
}

#[tokio::test]
async fn a_machine_scope_events_socket_receives_the_prompt_changed_frame() {
    let (fixture, token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    let events = state
        .machine_services
        .as_ref()
        .unwrap()
        .workspaces
        .machine_events
        .clone();
    let mut socket = fixture.events_socket(&token).await;
    // Publish only after the socket's first inventory boundary, when it is in its event loop.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if event["type"] == "inventoryInvalidated" {
                break;
            }
        }
    })
    .await
    .expect("the machine socket opens with an inventory boundary");

    state.agent_events.publish(draft("machine-session", "p1"));
    let pushed = next_prompt_frame(&mut socket).await;
    assert_eq!(pushed["sessionId"], "machine-session");
    assert_eq!(pushed["promptId"], "p1");
    assert_eq!(pushed["screenRevision"], "rev-p1");

    socket.close(None).await.unwrap();
    let mut count = events.subscription_count();
    tokio::time::timeout(Duration::from_secs(10), count.wait_for(|n| *n == 0))
        .await
        .unwrap()
        .unwrap();
    fixture.stop().await;
}

/// The next frame of `kind` on the socket, skipping other event types.
async fn next_frame_of<S>(socket: &mut S, kind: &str) -> Value
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let Ok(text) = message.to_text() else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(text) else {
                continue;
            };
            if event["type"] == kind {
                return event;
            }
        }
    })
    .await
    .expect("a frame of the wanted type")
}

#[tokio::test]
async fn a_non_owner_machine_socket_gets_the_panel_transition_without_a_prompt_body() {
    // Agent control is off by default, so this Control device is not an owner subscriber.
    let (fixture, token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    assert!(!state.agent_control_policy.enabled());
    let mut socket = fixture.events_socket(&token).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if event["type"] == "inventoryInvalidated" {
                break;
            }
        }
    })
    .await
    .expect("the machine socket opens with an inventory boundary");

    state.agent_events.publish(draft("machine-session", "p1"));
    let frame = next_frame_of(&mut socket, "panel.session").await;
    assert_eq!(frame["state"], "blocked");
    assert_eq!(frame["promptId"], "p1");
    assert!(frame["sessionRef"].as_str().unwrap().ends_with(":machine-session"));
    assert!(frame.get("session").is_none(), "{frame}");
    assert!(frame.get("question").is_none() && frame.get("body").is_none(), "{frame}");

    drop(socket);
    fixture.stop().await;
}

/// Waits for the machine socket's first inventory boundary, when it is in its event loop.
async fn wait_for_boundary<S>(socket: &mut S)
where
    S: futures_util::Stream<
            Item = Result<
                tokio_tungstenite::tungstenite::Message,
                tokio_tungstenite::tungstenite::Error,
            >,
        > + Unpin,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            if event["type"] == "inventoryInvalidated" {
                break;
            }
        }
    })
    .await
    .expect("the machine socket opens with an inventory boundary");
}

/// The panel channel is process-wide, so the tests that publish on it run one at a time.
static PANEL_CHANNEL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn escalation() -> ferryx_hub::supervisor::panel_contract::Escalation {
    use ferryx_hub::supervisor::panel_contract::{Escalation, PendingPrompt, SessionRef};
    Escalation {
        session_ref: SessionRef::new("mac", "s1"),
        prompt: PendingPrompt {
            prompt_id: "p1".into(),
            kind: "approval".into(),
            question: "Run the migration?".into(),
            command: Some("sqlx migrate run".into()),
            options: vec!["Allow".into(), "Deny".into()],
            screen_revision: Some("r1".into()),
        },
        recommended_index: 1,
        reason: "Not a local database".into(),
        created_at_ms: 1,
    }
}

#[tokio::test]
async fn an_owner_socket_gets_decision_action_and_rules_frames_within_two_seconds() {
    use ferryx_hub::supervisor::panel_contract::{ActionEntry, ActionKind, SessionRef};
    let _channel = PANEL_CHANNEL.lock().await;
    let (fixture, token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    state.agent_control_policy.set_enabled(true);
    let mut socket = fixture.events_socket(&token).await;
    wait_for_boundary(&mut socket).await;

    let decision = escalation();
    crate::hub_runtime::publish_panel_decision(&decision.session_ref, Some(&decision));
    let frame = tokio::time::timeout(Duration::from_secs(2), next_frame_of(&mut socket, "panel.decision"))
        .await
        .expect("the decision frame arrives within two seconds");
    assert_eq!(frame["sessionRef"], "mac:s1");
    assert_eq!(frame["decision"]["prompt"]["question"], "Run the migration?");
    assert_eq!(frame["decision"]["recommendedIndex"], 1);

    crate::hub_runtime::publish_panel_decision(&SessionRef::new("mac", "s1"), None);
    let resolved = next_frame_of(&mut socket, "panel.decision").await;
    assert!(resolved["decision"].is_null(), "{resolved}");

    let action = ActionEntry {
        id: 3,
        incident_key: "proxy-502".into(),
        session_ref: None,
        kind: ActionKind::Retried,
        summary: "Retried".into(),
        basis: "default rule".into(),
        evidence: Vec::new(),
        undo: None,
        live: true,
        created_at_ms: 1,
        updated_at_ms: 2,
    };
    crate::hub_runtime::publish_panel_action(&action);
    let frame = next_frame_of(&mut socket, "panel.action").await;
    assert_eq!(frame["action"]["incidentKey"], "proxy-502");

    crate::hub_runtime::publish_panel_rules(&ferryx_hub::supervisor::panel_contract::default_rules());
    let frame = next_frame_of(&mut socket, "panel.rules").await;
    assert_eq!(frame["rules"].as_array().unwrap().len(), 4);

    drop(socket);
    fixture.stop().await;
}

#[tokio::test]
async fn a_non_owner_socket_never_gets_a_decision_frame_with_its_question() {
    let _channel = PANEL_CHANNEL.lock().await;
    let (fixture, owner_token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    let other_token = {
        let auth = &state.auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        auth.exchange_pairing_code(&pin, "other-device").unwrap().0
    };
    let other_id = state.auth_manager.validate_token(&other_token).unwrap().id;
    // Agent control admits the first device only; the second is a machine device it excludes.
    state.agent_control_policy.set_enabled(true);
    state.agent_control_policy.restrict_to([state.auth_manager.validate_token(&owner_token).unwrap().id]);
    assert!(!state.agent_control_policy.permits(&other_id));
    let mut owner = fixture.events_socket(&owner_token).await;
    let mut other = fixture.events_socket(&other_token).await;
    wait_for_boundary(&mut owner).await;
    wait_for_boundary(&mut other).await;

    let decision = escalation();
    crate::hub_runtime::publish_panel_decision(&decision.session_ref, Some(&decision));
    // The owner socket proves the frame was on the channel before anything below is published.
    next_frame_of(&mut owner, "panel.decision").await;
    state.agent_events.publish(draft("machine-session", "p1"));
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = other.next().await.unwrap().unwrap();
            let Ok(text) = message.to_text() else { continue };
            let Ok(event) = serde_json::from_str::<Value>(text) else { continue };
            assert!(!text.contains("Run the migration?"), "{text}");
            let kind = event["type"].as_str().unwrap_or_default().to_owned();
            seen.push(kind.clone());
            if kind == "panel.session" {
                break;
            }
        }
    })
    .await
    .expect("the non-owner socket still gets the slim panel.session frame");
    assert!(!seen.iter().any(|kind| kind == "panel.decision"), "{seen:?}");

    drop((owner, other));
    fixture.stop().await;
}

#[tokio::test]
async fn disabling_agent_control_stops_owner_frames_on_an_open_socket_at_once() {
    let _channel = PANEL_CHANNEL.lock().await;
    let (fixture, token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    state.agent_control_policy.set_enabled(true);
    let mut socket = fixture.events_socket(&token).await;
    wait_for_boundary(&mut socket).await;

    let decision = escalation();
    crate::hub_runtime::publish_panel_decision(&decision.session_ref, Some(&decision));
    next_frame_of(&mut socket, "panel.decision").await;

    // The socket was opened as an owner; the policy is asked again for every frame.
    state.agent_control_policy.set_enabled(false);
    crate::hub_runtime::publish_panel_decision(&decision.session_ref, Some(&decision));
    state.agent_events.publish(draft("machine-session", "p1"));
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let Ok(text) = message.to_text() else { continue };
            let Ok(event) = serde_json::from_str::<Value>(text) else { continue };
            assert!(!text.contains("Run the migration?"), "{text}");
            let kind = event["type"].as_str().unwrap_or_default().to_owned();
            seen.push(kind.clone());
            if kind == "panel.session" {
                assert!(event.get("session").is_none(), "{event}");
                break;
            }
        }
    })
    .await
    .expect("the demoted socket still gets the slim panel.session frame");
    assert!(!seen.iter().any(|kind| kind == "panel.decision"), "{seen:?}");

    drop(socket);
    fixture.stop().await;
}

#[tokio::test]
async fn an_owner_transition_whose_session_cannot_be_read_is_a_resync_not_a_session_less_frame() {
    let _channel = PANEL_CHANNEL.lock().await;
    let (fixture, token) = fixture(DeviceAccessScope::Machine).await;
    let state = fixture.owner.remote_state().clone();
    state.agent_control_policy.set_enabled(true);
    let mut socket = fixture.events_socket(&token).await;
    wait_for_boundary(&mut socket).await;

    // No pane backs this session, so the owner's fleet read fails.
    state.agent_events.publish(draft("ghost-session", "p1"));
    let frame = tokio::time::timeout(Duration::from_secs(2), next_frame_of(&mut socket, "panel.resync"))
        .await
        .expect("the resync arrives within two seconds");
    assert_eq!(frame["reason"], "sessionUnavailable");
    assert!(frame["sessionRef"].as_str().unwrap().ends_with(":ghost-session"));

    drop(socket);
    fixture.stop().await;
}
