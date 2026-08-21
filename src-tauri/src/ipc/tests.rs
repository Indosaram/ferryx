use crate::ipc::*;
use crate::terminal::TerminalService;
use crate::worktree::{run_git, WorkspaceRegistry, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::Arc;
use tauri::{Listener, Manager};
use tempfile::TempDir;

fn setup_workspace() -> (TempDir, WorkspaceRegistry) {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.email", "test@example.com"]).expect("email");
    run_git(repo.path(), &["config", "user.name", "Test User"]).expect("name");
    std::fs::write(repo.path().join("README.md"), "initial\n").expect("README");
    run_git(repo.path(), &["add", "README.md"]).expect("add");
    run_git(repo.path(), &["commit", "-m", "initial commit"]).expect("commit");

    let registry = WorkspaceRegistry::new();
    registry.register("workspace-test", repo.path()).expect("register");
    (repo, registry)
}

#[tokio::test]
async fn tauri_mock_terminal_events_use_registered_workspace() {
    let (_repo, registry) = setup_workspace();
    let app = tauri::test::mock_builder()
        .manage(Arc::new(TerminalService::default()))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let terminal_service = app.state::<Arc<TerminalService>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let (tx, mut rx_event) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(16);
    app.listen(TERMINAL_OUTPUT_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = tx.try_send(payload);
        }
    });

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        terminal_service.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cols: Some(80),
            rows: Some(24),
        },
    )
    .await
    .expect("spawn");

    cmd_terminal_write(
        terminal_service.clone(),
        spawned.session_id.clone(),
        "echo hello_orca_terminal\n".into(),
    )
    .await
    .expect("write");

    let mut collected = Vec::new();
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&collected).contains("hello_orca_terminal") {
            let payload = rx_event.recv().await.expect("terminal output");
            assert_eq!(payload.session_id, spawned.session_id);
            collected.extend_from_slice(&STANDARD.decode(payload.data).expect("base64"));
        }
    })
    .await
    .expect("terminal output timeout");

    cmd_terminal_resize(
        terminal_service.clone(),
        spawned.session_id.clone(),
        120,
        40,
    )
    .await
    .expect("resize");
    let sessions = cmd_terminal_list(terminal_service.clone()).await.expect("list");
    assert!(sessions
        .iter()
        .any(|session| session.session_id == spawned.session_id));

    cmd_terminal_close(terminal_service.clone(), spawned.session_id.clone())
        .await
        .expect("close");
    cmd_terminal_close(terminal_service.clone(), spawned.session_id.clone())
        .await
        .expect("idempotent close");
    let sessions = cmd_terminal_list(terminal_service).await.expect("list after");
    assert!(!sessions
        .iter()
        .any(|session| session.session_id == spawned.session_id));
}

#[tokio::test]
async fn tauri_mock_worktree_commands_use_identity_contract() {
    let (_repo, registry) = setup_workspace();
    let app = tauri::test::mock_builder()
        .manage(Arc::new(TerminalService::default()))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let identity = WorktreeIdentity {
        ws_id: "ws-ipc".into(),
        slug: "task-ipc".into(),
    };

    let initial = cmd_worktree_list(registry_state.clone(), "workspace-test".into())
        .await
        .expect("initial list");
    assert_eq!(initial.len(), 1);

    let created = cmd_worktree_create(
        app.handle().clone(),
        registry_state.clone(),
        CreateWorktreeRequest {
            workspace_id: "workspace-test".into(),
            worktree: identity.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("create");
    assert!(created.path.exists());

    let status = cmd_worktree_status(
        app.handle().clone(),
        registry_state.clone(),
        WorktreeStatusRequest {
            workspace_id: "workspace-test".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("status");
    assert!(!status.is_dirty);

    cmd_worktree_delete(
        app.handle().clone(),
        registry_state.clone(),
        DeleteWorktreeRequest {
            workspace_id: "workspace-test".into(),
            worktree: identity,
            delete_branch: Some(true),
        },
    )
    .await
    .expect("delete");

    let final_list = cmd_worktree_list(registry_state, "workspace-test".into())
        .await
        .expect("final list");
    assert_eq!(final_list.len(), 1);
}

#[tokio::test]
async fn terminal_global_events_preserve_raw_bytes_and_lifecycle() {
    let (_repo, registry) = setup_workspace();
    let app = tauri::test::mock_builder()
        .manage(Arc::new(TerminalService::default()))
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let terminal_service = app.state::<Arc<TerminalService>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(32);
    let (lifecycle_tx, mut lifecycle_rx) =
        tokio::sync::mpsc::channel::<TerminalLifecyclePayload>(8);

    app.listen(TERMINAL_OUTPUT_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = output_tx.try_send(payload);
        }
    });
    app.listen(TERMINAL_LIFECYCLE_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalLifecyclePayload>(event.payload()) {
            let _ = lifecycle_tx.try_send(payload);
        }
    });

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        terminal_service.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cols: Some(80),
            rows: Some(24),
        },
    )
    .await
    .expect("spawn");

    let started = tokio::time::timeout(tokio::time::Duration::from_secs(2), lifecycle_rx.recv())
        .await
        .expect("started timeout")
        .expect("started event");
    assert_eq!(started.session_id, spawned.session_id);
    assert_eq!(started.state, TerminalLifecycleState::Started);

    cmd_terminal_write(
        terminal_service.clone(),
        spawned.session_id.clone(),
        "printf '\\377\\376\\n'\n".into(),
    )
    .await
    .expect("write raw bytes");

    let mut raw = Vec::new();
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        while !raw.windows(2).any(|window| window == [0xff, 0xfe]) {
            let payload = output_rx.recv().await.expect("output event");
            assert_eq!(payload.session_id, spawned.session_id);
            raw.extend_from_slice(&STANDARD.decode(payload.data).expect("base64 payload"));
        }
    })
    .await
    .expect("raw output timeout");

    cmd_terminal_close(terminal_service, spawned.session_id.clone())
        .await
        .expect("close");

    let exited = tokio::time::timeout(tokio::time::Duration::from_secs(2), lifecycle_rx.recv())
        .await
        .expect("exited timeout")
        .expect("exited event");
    assert_eq!(exited.session_id, spawned.session_id);
    assert_eq!(exited.state, TerminalLifecycleState::Exited);
    assert!(exited.reason.is_none());
}
