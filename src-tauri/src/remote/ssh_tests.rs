use super::*;
use crate::ssh::{projects, SshAuthMethod, SshHost, SshHostSource};

fn fixture() -> (tempfile::TempDir, Arc<RemoteGatewayState>, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("ssh_hosts.json");
    let host = SshHost {
        id: "build".into(), label: "Build".into(), hostname: "build.invalid".into(),
        username: None, port: None, identity_file: None, jump_host: None,
        source: SshHostSource::Manual, auth_method: SshAuthMethod::Agent, disabled: None,
    };
    std::fs::write(&store, serde_json::to_vec(&crate::ipc::ssh::SshHostStore {
        hosts: vec![host], tombstones: vec![],
    }).unwrap()).unwrap();
    let id = projects::identity("build", "/srv/private/repo");
    let project = serde_json::from_value(serde_json::json!({
        "workspaceId": id,
        "hostId": "build",
        "repoRoot": "/srv/private/repo",
        "gitRoot": null,
        "gitRemote": null,
    })).unwrap();
    projects::persist(&store, project).unwrap();
    let service = Arc::new(TerminalService::new(
        Arc::new(crate::terminal::PtyManager::new()),
        Arc::new(crate::terminal::TerminalOutputHub::default()),
    ));
    let state = Arc::new(RemoteGatewayState::new(service, crate::worktree::WorkspaceRegistry::new()));
    *state.ssh_store_path.write() = Some(store);
    (dir, state, id)
}

async fn request(state: Arc<RemoteGatewayState>, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let code = state.auth_manager.create_pairing_code(DevicePermission::Control);
    let (token, _) = state.auth_manager.exchange_pairing_code(&code, "SSH test").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, create_remote_router(state)).await.unwrap() });
    let response = tokio::time::timeout(std::time::Duration::from_secs(5),
        http_request(addr, method, path, Some(&token), body)).await;
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
    assert!(value.projects.iter().any(|project| project.workspace_id == id));
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
        pty.clone(), Arc::new(crate::terminal::TerminalOutputHub::default()),
    ));
    let router = crate::daemon::proxy::SessionRouter::new(service);
    let (id, receiver) = pty.spawn(portable_pty::CommandBuilder::new("/bin/sh"), 80, 24).unwrap();
    router.register_workspace(&id, "ssh:owned", None);
    let details = router.describe_session(&id).await;
    pty.close_session(&id).await.unwrap();
    drop(receiver);
    assert_eq!(details.unwrap().workspace_id.as_deref(), Some("ssh:owned"));
}
