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
