#![cfg(unix)]
use ferryx_lib::daemon::{DaemonClient, DaemonServer};
use ferryx_lib::ipc::{
    cmd_terminal_close, cmd_terminal_spawn, cmd_worktree_create, cmd_worktree_delete,
    cmd_worktree_status, CreateWorktreeRequest, DeleteWorktreeRequest, IpcErrorCode,
    SpawnTerminalRequest, TerminalLifecycleState, WorktreeChangeKind, WorktreeChangedPayload,
    WorktreeStatusRequest, WORKTREE_CHANGED_EVENT,
};
use ferryx_lib::worktree::{run_git, WorkspaceRegistry, WorktreeIdentity};
use std::fs;
use std::sync::Arc;
use tauri::{Listener, Manager};
use tempfile::TempDir;
use tokio::net::UnixListener;

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

async fn setup_test_daemon() -> (TempDir, Arc<DaemonClient>, tokio::task::JoinHandle<()>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("test_daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind unix listener");
    let server = Arc::new(DaemonServer::new());
    let server_clone = Arc::clone(&server);
    let server_task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let s = Arc::clone(&server_clone);
                    tokio::spawn(async move {
                        s.handle_client(stream).await;
                    });
                }
                Err(_) => break,
            }
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket_path));
    (dir, client, server_task)
}

#[tokio::test]
async fn identity_based_ipc_resolves_registered_worktree_and_emits_mutation_events() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");

    let (_daemon_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-a", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon_client))
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
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        SpawnTerminalRequest {
            workspace_id: "workspace-a".into(),
            worktree: Some(identity.clone()),
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("identity terminal spawn");

    let details = daemon_client
        .describe_session(&spawned.session_id)
        .await
        .expect("session details");
    assert_eq!(
        details.cwd.as_deref().map(std::path::Path::new),
        Some(canonical_created.as_path())
    );
    assert_eq!(details.worktree.as_ref(), Some(&identity));
    let (worktree_manager, _) = registry
        .resolve_terminal_target("workspace-a", Some(&identity))
        .expect("resolve worktree manager");
    assert_eq!(
        worktree_manager
            .writer_owner(&canonical_created)
            .expect("writer owner query"),
        None,
        "interactive terminals must not consume the exclusive writer lease"
    );

    cmd_terminal_close(app.state::<Arc<DaemonClient>>(), spawned.session_id)
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

    let pruned_event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .expect("pruned event timeout")
        .expect("pruned event");
    assert_eq!(pruned_event.workspace_id, "workspace-a");
    assert_eq!(pruned_event.worktree, identity);
    assert_eq!(pruned_event.kind, WorktreeChangeKind::Pruned);

    server_task.abort();
}

// TEMPORARY-GIT-FIXTURE: protects identity deletion from stale checked-out branches.
#[tokio::test]
async fn swapped_checked_out_branches_cannot_delete_a_stale_identity_slot() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");
    let app = tauri::test::mock_builder()
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let identity_a = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "slot-a".into(),
    };
    let identity_b = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "slot-b".into(),
    };
    let created_a = cmd_worktree_create(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        CreateWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity_a.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("create slot A");
    let created_b = cmd_worktree_create(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        CreateWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity_b.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("create slot B");

    run_git(&created_a.path, &["checkout", "--detach"]).expect("detach slot A");
    run_git(&created_b.path, &["checkout", "--detach"]).expect("detach slot B");
    run_git(&created_a.path, &["checkout", "orca/agent/slot-b"]).expect("swap branch into slot A");
    run_git(&created_b.path, &["checkout", "orca/agent/slot-a"]).expect("swap branch into slot B");

    let err = cmd_worktree_delete(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        DeleteWorktreeRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity_a,
            delete_branch: Some(true),
        },
    )
    .await
    .expect_err("stale identity must fail closed");
    assert_eq!(err.code, IpcErrorCode::WorktreeNotFound);
    assert!(created_a.path.exists(), "slot A must survive");
    assert!(created_b.path.exists(), "slot B must survive");
    let branches = run_git(repo.path(), &["branch", "--list"]).expect("list managed branches");
    assert!(
        branches.contains("orca/agent/slot-a"),
        "branch A must survive"
    );
    assert!(
        branches.contains("orca/agent/slot-b"),
        "branch B must survive"
    );
}

#[tokio::test]
async fn worktree_status_emits_dirty_changed_on_clean_to_dirty_transition() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");
    let app = tauri::test::mock_builder()
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let identity = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "dirty-transition".into(),
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
    .expect("create worktree");

    let initial = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("initial clean status");
    assert!(!initial.is_dirty);

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<WorktreeChangedPayload>(4);
    app.listen(WORKTREE_CHANGED_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<WorktreeChangedPayload>(event.payload()) {
            let _ = event_tx.try_send(payload);
        }
    });

    fs::write(created.path.join("dirty.txt"), "dirty\n").expect("dirty file");
    let dirty = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("dirty status");
    assert!(dirty.is_dirty);

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .expect("dirtyChanged event timeout")
        .expect("dirtyChanged event");
    assert_eq!(event.workspace_id, "workspace-a");
    assert_eq!(event.worktree, identity);
    assert_eq!(event.kind, WorktreeChangeKind::DirtyChanged);
    assert_eq!(
        serde_json::to_value(event.kind).expect("serialize dirtyChanged kind"),
        serde_json::json!("dirtyChanged")
    );
}

#[tokio::test]
async fn worktree_status_emits_dirty_changed_on_dirty_to_clean_transition() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");
    let app = tauri::test::mock_builder()
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let identity = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "clean-transition".into(),
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
    .expect("create worktree");
    let dirty_file = created.path.join("dirty.txt");
    fs::write(&dirty_file, "dirty\n").expect("dirty file");

    let initial = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("initial dirty status");
    assert!(initial.is_dirty);

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<WorktreeChangedPayload>(4);
    app.listen(WORKTREE_CHANGED_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<WorktreeChangedPayload>(event.payload()) {
            let _ = event_tx.try_send(payload);
        }
    });

    fs::remove_file(dirty_file).expect("clean dirty file");
    let clean = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("clean status");
    assert!(!clean.is_dirty);

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), event_rx.recv())
        .await
        .expect("dirtyChanged event timeout")
        .expect("dirtyChanged event");
    assert_eq!(event.workspace_id, "workspace-a");
    assert_eq!(event.worktree, identity);
    assert_eq!(event.kind, WorktreeChangeKind::DirtyChanged);
}

#[tokio::test]
async fn worktree_status_does_not_emit_when_dirty_state_is_unchanged() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");
    let app = tauri::test::mock_builder()
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let identity = WorktreeIdentity {
        ws_id: "agent".into(),
        slug: "unchanged-dirty".into(),
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
    .expect("create worktree");
    fs::write(created.path.join("dirty.txt"), "dirty\n").expect("dirty file");

    let initial = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity.clone(),
        },
    )
    .await
    .expect("initial dirty status");
    assert!(initial.is_dirty);

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<WorktreeChangedPayload>(4);
    app.listen(WORKTREE_CHANGED_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<WorktreeChangedPayload>(event.payload()) {
            let _ = event_tx.try_send(payload);
        }
    });

    let unchanged = cmd_worktree_status(
        app.handle().clone(),
        app.state::<WorkspaceRegistry>(),
        WorktreeStatusRequest {
            workspace_id: "workspace-a".into(),
            worktree: identity,
        },
    )
    .await
    .expect("unchanged dirty status");
    assert!(unchanged.is_dirty);

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), event_rx.recv())
            .await
            .is_err(),
        "unchanged dirty state must not emit worktree_changed"
    );
}

#[tokio::test]
async fn dirty_delete_returns_structured_error_code() {
    let repo = setup_repo();
    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-a", repo.path())
        .expect("register workspace");

    let app = tauri::test::mock_builder()
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
