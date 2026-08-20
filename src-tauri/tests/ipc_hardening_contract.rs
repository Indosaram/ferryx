use orca_lite_lib::ipc::{
    cmd_terminal_close, cmd_terminal_spawn, cmd_worktree_create, cmd_worktree_delete,
    CreateWorktreeRequest, DeleteWorktreeRequest, IpcErrorCode, SpawnTerminalRequest,
    TerminalLifecycleState, WorktreeChangeKind, WorktreeChangedPayload, WORKTREE_CHANGED_EVENT,
};
use orca_lite_lib::terminal::PtyManager;
use orca_lite_lib::worktree::{run_git, WorkspaceRegistry, WorktreeIdentity};
use std::fs;
use std::sync::Arc;
use tauri::{Listener, Manager};
use tempfile::TempDir;

fn setup_repo() -> TempDir {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.name", "Orca Test"]).expect("git user name");
    run_git(repo.path(), &["config", "user.email", "test@orca.dev"]).expect("git user email");
    fs::write(repo.path().join("README.md"), "# base\n").expect("write README");
    run_git(repo.path(), &["add", "README.md"]).expect("git add");
    run_git(repo.path(), &["commit", "-m", "base"]).expect("git commit");
    repo
}

#[tokio::test]
async fn identity_based_ipc_resolves_registered_worktree_and_emits_mutation_events() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");

    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<WorktreeChangedPayload>(8);
    app.listen(WORKTREE_CHANGED_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<WorktreeChangedPayload>(event.payload()) {
            let _ = event_tx.try_send(payload);
        }
    });

    let identity = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "task".into(),
    };
    let created = cmd_worktree_create(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        CreateWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("identity create");

    let canonical_root = repo.path().canonicalize().expect("canonical root");
    let canonical_created = created.path.canonicalize().expect("canonical worktree");
    assert!(canonical_created.starts_with(&canonical_root));

    let created_event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .expect("created event timeout")
        .expect("created event");
    assert_eq!(created_event.workspace_id, "workspace-a");
    assert_eq!(created_event.worktree, identity);
    assert_eq!(created_event.kind, WorktreeChangeKind::Created);

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        app.state::<Arc<PtyManager>>(),
        app.state::<WorkspaceRegistry>(),
        SpawnTerminalRequest {
            workspace_id: "workspace-a".into(),
            worktree: Some(identity.clone()),
            cols: Some(80),
            rows: Some(24),
        },
    )
    .await
    .expect("identity terminal spawn");

    let session = app
        .state::<Arc<PtyManager>>()
        .get_session(&spawned.session_id)
        .expect("session");
    assert_eq!(session.writer_worktree(), Some(canonical_created.clone()));
    assert_eq!(session.writer_owner_id(), Some(spawned.session_id.clone()));

    cmd_terminal_close(app.state::<Arc<PtyManager>>(), spawned.session_id)
        .await
        .expect("close terminal");

    cmd_worktree_delete(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        DeleteWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
            delete_branch: Some(true),
        },
    )
    .await
    .expect("safe delete");

    let deleted_event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .expect("deleted event timeout")
        .expect("deleted event");
    assert_eq!(deleted_event.kind, WorktreeChangeKind::Deleted);
    assert_eq!(deleted_event.worktree, identity);
}

#[tokio::test]
async fn dirty_delete_returns_structured_error_code() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");
    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let identity = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "dirty".into(),
    };
    let created = cmd_worktree_create(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        CreateWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("create dirty worktree");
    fs::write(created.path.join("dirty.txt"), "dirty\n").expect("dirty file");

    let err = cmd_worktree_delete(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        DeleteWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity,
            delete_branch: Some(false),
        },
    )
    .await
    .expect_err("dirty delete must fail");

    assert_eq!(err.code, IpcErrorCode::DirtyWorktree);
    let json = serde_json::to_value(err).expect("serialize IPC error");
    assert_eq!(json["code"], "DIRTY_WORKTREE");
    assert!(json.get("details").is_some());
}

#[test]
fn terminal_lifecycle_state_serializes_stably() {
    assert_eq!(
        serde_json::to_value(TerminalLifecycleState::Exited).expect("serialize lifecycle"),
        serde_json::json!("exited")
    );
}
