//! `sessionAttention` privacy: a mirror-scoped phone is never told about a machine-only session or
//! a private (machine-catalog, non-mirror) project, at publish time or at read time.
use crate::{
    daemon::server::DaemonServer,
    remote::{
        auth::{DeviceAccessScope, DevicePermission},
        machine_protocol::RemoteTerminalTarget,
        protocol::{RemoteActiveDesktopSelection, RemoteSessionAttention},
        server::create_remote_router,
        state::RemoteGatewayState,
    },
    scoped_contracts::Epoch,
};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

fn entry(session: &str, workspace: &str) -> RemoteSessionAttention {
    RemoteSessionAttention {
        backend_session_id: session.into(),
        workspace_id: workspace.into(),
        worktree_slug: None,
        state: "waiting".into(),
        agent_type: Some("claude".into()),
    }
}

async fn fixture() -> (tempfile::TempDir, DaemonServer, String, String) {
    crate::ipc::run_blocking(|| {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("private-repo");
        std::fs::create_dir(&repo).unwrap();
        crate::worktree::run_git(&repo, &["init", "--quiet"]).unwrap();
        crate::worktree::run_git(
            &repo,
            &["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "--allow-empty", "-m", "init"],
        )
        .unwrap();
        let owner = DaemonServer::new_with_paths(
            Some(root.path().join("config")),
            Some(root.path().join("auth")),
        );
        let auth = &owner.remote_state().auth_manager;
        let pin = auth
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Mirror)
            .unwrap();
        let (token, _) = auth.exchange_pairing_code(&pin, "phone").unwrap();
        let private_ws = owner
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces
            .register_machine(repo.to_str().unwrap())
            .unwrap();
        Ok((root, owner, token, private_ws))
    })
    .await
    .unwrap()
}

async fn own_session(state: &Arc<RemoteGatewayState>, id: &str) {
    let services = state.machine_services.as_ref().unwrap().clone();
    let resource = serde_json::to_string(&RemoteTerminalTarget {
        machine_id: "machine".into(),
        daemon_epoch: Epoch(1),
        session_id: id.into(),
    })
    .unwrap();
    let journal = services.clone();
    let request = format!("req-{id}");
    crate::ipc::run_blocking(move || {
        journal
            .workspaces
            .journal
            .begin("device", &request, "createSession", "digest", &resource)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .unwrap();
    assert_eq!(services.sessions.machine_only_async(id).await, Ok(true));
}

async fn workspace_state(state: Arc<RemoteGatewayState>, token: &str) -> Value {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = create_remote_router(state);
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let response = client
        .get(format!("http://{address}/api/v1/workspace/state"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let text = response.text().await.unwrap();
    task.abort();
    let _ = task.await;
    assert_eq!(status, reqwest::StatusCode::OK, "{text}");
    serde_json::from_str(&text).unwrap()
}

fn ids(body: &Value) -> Vec<String> {
    body["sessionAttention"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item["backendSessionId"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

async fn teardown(root: tempfile::TempDir, owner: DaemonServer) {
    drop(owner);
    crate::ipc::run_blocking(move || {
        root.close().unwrap();
        Ok(())
    })
    .await
    .unwrap();
}

/// Publish-time filter (state.rs): a session already owned by a machine client and a private
/// project's session never enter the stored selection.
#[tokio::test]
async fn publish_filter_drops_machine_only_and_private_project_attention() {
    let (root, owner, _token, private_ws) = fixture().await;
    let state = owner.remote_state().clone();
    own_session(&state, "machine-1").await;
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("ws-desk".into()),
        session_attention: vec![
            entry("desk-1", "ws-desk"),
            entry("machine-1", "ws-desk"),
            entry("private-1", &private_ws),
        ],
        ..Default::default()
    });
    let stored: Vec<String> = state
        .active_selection
        .read()
        .as_ref()
        .unwrap()
        .session_attention
        .iter()
        .map(|item| item.backend_session_id.clone())
        .collect();
    assert_eq!(stored, ["desk-1"]);
    teardown(root, owner).await;
}

/// Read-time filter (server.rs): a session that became machine-only after the desktop published
/// it is still hidden from the mirror phone's workspace state.
#[tokio::test]
async fn read_filter_hides_session_owned_after_publication() {
    let (root, owner, token, _private_ws) = fixture().await;
    let state = owner.remote_state().clone();
    state.set_active_selection(RemoteActiveDesktopSelection {
        workspace_id: Some("ws-desk".into()),
        session_attention: vec![entry("desk-1", "ws-desk"), entry("machine-1", "ws-desk")],
        ..Default::default()
    });
    assert_eq!(
        state.active_selection.read().as_ref().unwrap().session_attention.len(),
        2,
        "published before ownership, so only the read-time filter can hide it"
    );
    own_session(&state, "machine-1").await;
    let body = workspace_state(state.clone(), &token).await;
    assert_eq!(ids(&body), ["desk-1"], "{body}");
    assert!(!body.to_string().contains("machine-1"), "{body}");
    assert!(body["activeContext"].get("sessionAttention").is_none(), "{body}");
    teardown(root, owner).await;
}

/// Read-time catalog filter (server.rs): a machine-catalog project's session that bypassed the
/// publish filter is still hidden from the mirror phone.
#[tokio::test]
async fn read_filter_hides_private_catalog_project_attention() {
    let (root, owner, token, private_ws) = fixture().await;
    let state = owner.remote_state().clone();
    *state.active_selection.write() = Some(RemoteActiveDesktopSelection {
        workspace_id: Some("ws-desk".into()),
        session_attention: vec![entry("desk-1", "ws-desk"), entry("private-1", &private_ws)],
        ..Default::default()
    });
    let body = workspace_state(state.clone(), &token).await;
    assert_eq!(ids(&body), ["desk-1"], "{body}");
    assert!(!body.to_string().contains("private-1"), "{body}");
    teardown(root, owner).await;
}
