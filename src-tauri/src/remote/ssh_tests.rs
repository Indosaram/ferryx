use super::*;
use crate::terminal::{SessionAttachment, TerminalSignal};
use crate::ssh::{projects, SshAuthMethod, SshHost, SshHostSource};

fn fixture() -> (tempfile::TempDir, Arc<RemoteGatewayState>, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("ssh_hosts.json");
    let host = SshHost {
        id: "build".into(),
        label: "Build".into(),
        hostname: "build.invalid".into(),
        username: None,
        port: None,
        identity_file: None,
        jump_host: None,
        source: SshHostSource::Manual,
        auth_method: SshAuthMethod::Agent,
        disabled: None,
    };
    std::fs::write(
        &store,
        serde_json::to_vec(&crate::ipc::ssh::SshHostStore {
            hosts: vec![host],
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();
    let id = projects::identity("build", "/srv/private/repo");
    let project = serde_json::from_value(serde_json::json!({
        "workspaceId": id,
        "hostId": "build",
        "repoRoot": "/srv/private/repo",
        "gitRoot": null,
        "gitRemote": null,
    }))
    .unwrap();
    projects::persist(&store, project).unwrap();
    let service = Arc::new(TerminalService::new(
        Arc::new(crate::terminal::PtyManager::new()),
        Arc::new(crate::terminal::TerminalOutputHub::default()),
    ));
    let state = Arc::new(RemoteGatewayState::new(
        service,
        crate::worktree::WorkspaceRegistry::new(),
    ));
    *state.ssh_store_path.write() = Some(store);
    (dir, state, id)
}

async fn request(
    state: Arc<RemoteGatewayState>,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> (u16, String) {
    let code = state
        .auth_manager
        .create_pairing_code(DevicePermission::Control);
    let (token, _) = state
        .auth_manager
        .exchange_pairing_code(&code, "SSH test")
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, create_remote_router(state))
            .await
            .unwrap()
    });
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        http_request(addr, method, path, Some(&token), body),
    )
    .await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    response.unwrap()
}

#[tokio::test]
async fn registered_ssh_project_is_listed_without_a_focused_terminal() {
    let (_dir, state, id) = fixture();
    let (status, body) = request(state, "GET", "/api/v1/workspace/state", None).await;
    assert_eq!(status, 200);
    let value: RemoteWorkspaceState = serde_json::from_str(&body).unwrap();
    assert!(value
        .projects
        .iter()
        .any(|project| project.workspace_id == id));
    assert!(!body.contains("/srv/private"));
    assert!(!body.contains("build.invalid"));
}

#[tokio::test]
async fn registered_ssh_project_can_be_selected_without_local_registration() {
    let (_dir, state, id) = fixture();
    let mut events = state.event_tx.subscribe();
    let body = serde_json::json!({ "workspaceId": id }).to_string();
    let (status, _) = request(state, "POST", "/api/v1/workspace/select", Some(&body)).await;
    assert_eq!(status, 200);
    let event: RemoteEventMessage = serde_json::from_str(&events.try_recv().unwrap()).unwrap();
    assert_eq!(event.payload["workspaceId"], id);
}

#[tokio::test]
async fn ssh_session_retains_its_workspace_in_remote_routing() {
    let pty = Arc::new(crate::terminal::PtyManager::new());
    let service = Arc::new(TerminalService::new(
        pty.clone(),
        Arc::new(crate::terminal::TerminalOutputHub::default()),
    ));
    let router = crate::daemon::proxy::SessionRouter::new(service);
    let (id, receiver) = pty
        .spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24)
        .unwrap();
    router.register_workspace(&id, "ssh:owned", None);
    let details = router.describe_session(&id).await;
    pty.close_session(&id).await.unwrap();
    drop(receiver);
    assert_eq!(details.unwrap().workspace_id.as_deref(), Some("ssh:owned"));
}

struct MockSshResizeBackend {
    applied_resize: Arc<parking_lot::Mutex<Option<(u64, u16, u16)>>>,
}

impl RemoteSessionBackend for MockSshResizeBackend {
    fn recovery<'a>(
        &'a self,
        _id: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<Option<RecoveryStream>, String>> {
        let stream = futures_util::stream::iter(vec![RemoteRecoveryStatus {
            state: crate::terminal::remote::RemoteConnectionState::Connected,
            generation: 42,
        }]);
        Box::pin(async move { Ok(Some(Box::pin(stream) as RecoveryStream)) })
    }

    fn resize_generation<'a>(
        &'a self,
        _id: &'a str,
        generation: u64,
        cols: u16,
        rows: u16,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if generation == 42 {
                *self.applied_resize.lock() = Some((generation, cols, rows));
                Ok(())
            } else {
                Err("stale generation".into())
            }
        })
    }

    fn list_sessions(&self) -> futures_util::future::BoxFuture<'_, Vec<String>> {
        Box::pin(async { vec![] })
    }

    fn describe_session<'a>(
        &'a self,
        _session_id: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<RemoteSessionDetails, String>> {
        Box::pin(async { Err("unimplemented".into()) })
    }

    fn attach_with_sequence<'a>(
        &'a self,
        _session_id: &'a str,
        _after_sequence: Option<u64>,
    ) -> futures_util::future::BoxFuture<'a, Result<SessionAttachment, String>> {
        Box::pin(async { Err("unimplemented".into()) })
    }

    fn write_input<'a>(
        &'a self,
        _session_id: &'a str,
        _data: &'a [u8],
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn resize<'a>(
        &'a self,
        _session_id: &'a str,
        _cols: u16,
        _rows: u16,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn signal<'a>(
        &'a self,
        _session_id: &'a str,
        _signal: TerminalSignal,
    ) -> futures_util::future::BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn ssh_control_handles_generation_fenced_remote_resize_and_status() {
    let applied = Arc::new(parking_lot::Mutex::new(None));
    let backend: Arc<dyn RemoteSessionBackend> = Arc::new(MockSshResizeBackend {
        applied_resize: Arc::clone(&applied),
    });

    // 1. RemoteStatus serialization contract
    let status = ServerControlMessage::RemoteStatus {
        state: crate::terminal::remote::RemoteConnectionState::Connected,
        generation: "42".into(),
    };
    let status_json = serde_json::to_string(&status).unwrap();
    assert!(status_json.contains(r#""type":"remoteStatus""#));
    assert!(status_json.contains(r#""generation":"42""#));

    // 2. RemoteResize deserialization contract
    let wire_resize = r#"{"type":"remoteResize","generation":"42","cols":110,"rows":35}"#;
    let ctrl: ClientControlMessage = serde_json::from_str(wire_resize).unwrap();
    assert_eq!(
        ctrl,
        ClientControlMessage::RemoteResize {
            generation: "42".into(),
            cols: 110,
            rows: 35,
        }
    );

    // 3. View-only permission rejected
    let rejected_perm = ssh_control(&backend, "sess-1", &ctrl, false).await;
    assert!(!rejected_perm);
    assert_eq!(*applied.lock(), None);

    // 4. Stale generation rejected
    let stale_ctrl = ClientControlMessage::RemoteResize {
        generation: "41".into(),
        cols: 110,
        rows: 35,
    };
    let rejected_gen = ssh_control(&backend, "sess-1", &stale_ctrl, true).await;
    assert!(!rejected_gen);
    assert_eq!(*applied.lock(), None);

    // 5. Valid generation applied
    let ok = ssh_control(&backend, "sess-1", &ctrl, true).await;
    assert!(ok);
    assert_eq!(*applied.lock(), Some((42, 110, 35)));
}