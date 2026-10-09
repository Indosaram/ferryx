use super::*;
use crate::{
    daemon::server::DaemonServer,
    remote::{server::create_remote_router, state::RemoteGatewayState},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};

pub const DEADLINE: Duration = Duration::from_secs(5);
pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);

impl<T> AbortOnDrop<T> {
    fn new(handle: tokio::task::JoinHandle<T>) -> Self {
        Self(Some(handle))
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

pub struct TwoConsumerFixture {
    pub root: tempfile::TempDir,
    pub owner: DaemonServer,
    pub state: Arc<RemoteGatewayState>,
    pub base: String,
    pub controller_token: String,
    pub controller_device: String,
    pub viewer_token: String,
    pub viewer_device: String,
    tasks: tokio::task::JoinSet<()>,
}

impl TwoConsumerFixture {
    pub async fn new() -> Self {
        let (root, owner) = tokio::task::spawn_blocking(|| {
            let root = tempfile::tempdir().unwrap();
            let owner = DaemonServer::new_with_paths(
                Some(root.path().join("config")),
                Some(root.path().join("auth")),
            );
            (root, owner)
        })
        .await
        .unwrap();

        let state = owner.remote_state().clone();

        let ctrl_pin = state
            .auth_manager
            .create_scoped_pairing_code(
                crate::remote::DevicePermission::Control,
                crate::remote::DeviceAccessScope::Machine,
            )
            .unwrap();
        let (controller_token, ctrl_device) = state
            .auth_manager
            .exchange_pairing_code(&ctrl_pin, "controller-device")
            .unwrap();

        let view_pin = state
            .auth_manager
            .create_pairing_code(crate::remote::DevicePermission::View)
            .unwrap();
        let (viewer_token, view_device) = state
            .auth_manager
            .exchange_pairing_code(&view_pin, "viewer-device")
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = create_remote_router(state.clone());
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        Self {
            root,
            owner,
            state,
            base,
            controller_token,
            controller_device: ctrl_device.id,
            viewer_token,
            viewer_device: view_device.id,
            tasks,
        }
    }

    pub async fn create_session(&self) -> Value {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let text = client
            .post(format!("{}/api/v1/workspace/projects", self.base))
            .bearer_auth(&self.controller_token)
            .body(json!({"requestId": uuid::Uuid::new_v4(), "repoPath": self.root.path()}).to_string())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        let project: Value = serde_json::from_str(&text).unwrap();

        let text = client
            .post(format!("{}/api/v1/sessions", self.base))
            .bearer_auth(&self.controller_token)
            .body(json!({
                "requestId": uuid::Uuid::new_v4(),
                "workspaceId": project["workspaceId"],
                "worktree": null,
                "inheritFromSessionId": null,
                "cwdRelative": null,
                "cols": 80,
                "rows": 24,
                "startup": {"kind": "shell"}
            }).to_string())
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    pub async fn attach_controller(&self, session: &Value) -> Socket {
        self.attach_with_token(session, &self.controller_token, None).await
    }

    pub async fn attach_viewer(&self, session: &Value, after_sequence: Option<u64>) -> Socket {
        self.attach_with_token(session, &self.viewer_token, after_sequence).await
    }

    pub async fn attach_with_token(
        &self,
        session: &Value,
        token: &str,
        after_sequence: Option<u64>,
    ) -> Socket {
        let target = &session["target"];
        let mut url = format!(
            "{}/api/v1/terminal/{}?daemonEpoch={}",
            self.base.replace("http:", "ws:"),
            target["sessionId"].as_str().unwrap(),
            target["daemonEpoch"].as_str().unwrap()
        );
        if let Some(seq) = after_sequence {
            url.push_str(&format!("&afterSequence={seq}"));
        }
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", token).parse().unwrap(),
        );
        let (mut socket, _) = tokio::time::timeout(DEADLINE, connect_async(request))
            .await
            .expect("Connection to terminal socket must succeed within deadline")
            .unwrap();
        let first = tokio::time::timeout(DEADLINE, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(first, Message::Text(_)));
        socket
    }

    pub async fn cleanup(mut self) {
        self.state.auth_manager.revoke_device(&self.controller_device);
        self.state.auth_manager.revoke_device(&self.viewer_device);
        let backend = self.owner.terminal_service();
        for id in backend.list_sessions() {
            let pty = backend.get_session(&id);
            if pty.is_some() {
                let _ = backend.close_session(&id).await;
            }
            if let Some(services) = &self.state.machine_services {
                let _ = services.sessions.wait_machine_lifecycle(&id).await;
            }
        }
        self.tasks.shutdown().await;
        drop(self.owner);
        drop(self.state);
        let _ = tokio::task::spawn_blocking(move || self.root.close()).await;
    }
}

#[tokio::test]
async fn two_consumer_gateway_healthy_controller_progresses_while_viewer_stalls_and_drops() {
    let fixture = TwoConsumerFixture::new().await;
    let session = fixture.create_session().await;

    let mut controller = fixture.attach_controller(&session).await;
    let mut viewer = fixture.attach_viewer(&session, None).await;

    let target_session_id = session["target"]["sessionId"].as_str().unwrap();

    let writer_grant_msg = Message::Text(json!({
        "type": "writerGrant",
        "sessionId": target_session_id,
        "generation": 1
    }).to_string());
    controller.send(writer_grant_msg).await.unwrap();

    let test_input = b"echo 'TWO_CONSUMER_ACTIVE'\r";
    controller.send(Message::Binary(test_input.to_vec().into())).await.unwrap();

    let mut controller_echoed = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(Some(Ok(Message::Binary(bytes)))) = tokio::time::timeout(Duration::from_millis(500), controller.next()).await {
            if String::from_utf8_lossy(&bytes).contains("TWO_CONSUMER_ACTIVE") {
                controller_echoed = true;
                break;
            }
        }
    }
    assert!(controller_echoed, "Controller must receive PTY echo output");

    let first_viewer_chunk = tokio::time::timeout(Duration::from_secs(3), viewer.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(first_viewer_chunk, Message::Binary(_)));

    let (burst_done_tx, mut burst_done_rx) = tokio::sync::oneshot::channel::<()>();
    let hub = fixture.owner.terminal_service().output_hub().clone();
    let session_id_owned = target_session_id.to_string();

    let burst_payload = vec![0x44; 64 * 1024];
    let burst_task = tokio::spawn(async move {
        for _ in 0..25 {
            hub.publish(&session_id_owned, burst_payload.clone());
            tokio::task::yield_now().await;
        }
        let _ = burst_done_tx.send(());
    });
    let _burst_guard = AbortOnDrop::new(burst_task);

    burst_done_rx.await.unwrap();

    let viewer_outcome = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match viewer.next().await {
                Some(Ok(Message::Binary(_))) => {},
                Some(Ok(Message::Close(_))) => return Ok("CLOSED"),
                None => return Ok("DISCONNECTED"),
                Some(Err(_)) => return Ok("ERROR"),
                _ => {}
            }
        }
    }).await;
    assert!(viewer_outcome.is_ok(), "Stalled viewer must terminate due to overflow drop");

    let final_check = b"echo 'CONTROLLER_SURVIVES'\r";
    controller.send(Message::Binary(final_check.to_vec().into())).await.unwrap();

    let mut controller_survived = false;
    let deadline2 = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline2 {
        if let Ok(Some(Ok(Message::Binary(bytes)))) = tokio::time::timeout(Duration::from_millis(500), controller.next()).await {
            if String::from_utf8_lossy(&bytes).contains("CONTROLLER_SURVIVES") {
                controller_survived = true;
                break;
            }
        }
    }
    assert!(controller_survived, "Healthy controller must continue progressing");

    drop(viewer);
    let mut reconnected = fixture.attach_viewer(&session, Some(1)).await;
    let reconnect_live = tokio::time::timeout(Duration::from_secs(3), reconnected.next()).await;
    assert!(reconnect_live.is_ok(), "Reconnected viewer must successfully receive frames");

    drop(controller);
    drop(reconnected);
    fixture.cleanup().await;
}

#[tokio::test]
async fn two_consumer_controller_exclusivity_second_controller_conflicts() {
    let fixture = TwoConsumerFixture::new().await;
    let session = fixture.create_session().await;

    let ctrl1 = fixture.attach_controller(&session).await;

    let pin2 = fixture.state
        .auth_manager
        .create_scoped_pairing_code(
            crate::remote::DevicePermission::Control,
            crate::remote::DeviceAccessScope::Machine,
        )
        .unwrap();
    let (token2, _) = fixture.state
        .auth_manager
        .exchange_pairing_code(&pin2, "second-controller-device")
        .unwrap();

    let target = &session["target"];
    let url = format!(
        "{}/api/v1/terminal/{}?daemonEpoch={}",
        fixture.base.replace("http:", "ws:"),
        target["sessionId"].as_str().unwrap(),
        target["daemonEpoch"].as_str().unwrap()
    );
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", token2).parse().unwrap(),
    );

    let second_connect_res = connect_async(request).await;
    assert!(second_connect_res.is_err(), "Second controller with different device must be rejected with 409 CONTROL_CONFLICT");

    drop(ctrl1);
    fixture.cleanup().await;
}
