use crate::daemon::client::DaemonClient;
use crate::daemon::server::DaemonServer;
use crate::ipc::terminal::{get_cached_cwd, process_cwd};
use crate::ipc::*;
use crate::remote::{RemoteNetworkMode, RemoteRestartPolicy};
use crate::worktree::{run_git, WorkspaceRegistry, WorktreeIdentity};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Listener, Manager};
use tempfile::TempDir;
use tokio::net::UnixListener;

fn setup_workspace() -> (TempDir, WorkspaceRegistry) {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.email", "test@example.com"]).expect("email");
    run_git(repo.path(), &["config", "user.name", "Test User"]).expect("name");
    std::fs::write(repo.path().join("README.md"), "initial\n").expect("README");
    run_git(repo.path(), &["add", "README.md"]).expect("add");
    run_git(repo.path(), &["commit", "-m", "initial commit"]).expect("commit");

    let registry = WorkspaceRegistry::new();
    registry
        .register("workspace-test", repo.path())
        .expect("register");
    (repo, registry)
}

async fn setup_test_daemon_with_remote_paths(
    config_path: Option<PathBuf>,
    auth_path: Option<PathBuf>,
) -> (TempDir, Arc<DaemonClient>, tokio::task::JoinHandle<()>) {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("test_daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind unix listener");
    let server = Arc::new(DaemonServer::new_with_paths(config_path, auth_path));
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
async fn tauri_mock_terminal_events_use_registered_workspace() {
    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let (tx, mut rx_event) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(16);
    app.listen(TERMINAL_OUTPUT_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = tx.try_send(payload);
        }
    });

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("spawn");

    cmd_terminal_write(
        client_state.clone(),
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

    cmd_terminal_resize(client_state.clone(), spawned.session_id.clone(), 120, 40)
        .await
        .expect("resize");
    let sessions = cmd_terminal_list(client_state.clone()).await.expect("list");
    assert!(sessions
        .iter()
        .any(|session| session.session_id == spawned.session_id));

    cmd_terminal_close(client_state.clone(), spawned.session_id.clone())
        .await
        .expect("close");
    cmd_terminal_close(client_state.clone(), spawned.session_id.clone())
        .await
        .expect("idempotent close");
    let sessions = cmd_terminal_list(client_state).await.expect("list after");
    assert!(!sessions
        .iter()
        .any(|session| session.session_id == spawned.session_id));

    server_task.abort();
}

#[tokio::test]
async fn tauri_mock_terminal_attach_returns_base64_history_and_decimal_sequences() {
    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let (tx, mut rx_event) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(16);
    app.listen(TERMINAL_OUTPUT_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = tx.try_send(payload);
        }
    });

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("spawn");

    cmd_terminal_write(
        client_state.clone(),
        spawned.session_id.clone(),
        "echo initial_attach_test\n".into(),
    )
    .await
    .expect("write");

    let mut collected = Vec::new();
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&collected).contains("initial_attach_test") {
            let payload = rx_event.recv().await.expect("output event");
            assert_eq!(payload.session_id, spawned.session_id);
            collected.extend_from_slice(&STANDARD.decode(payload.data).expect("base64"));
        }
    })
    .await
    .expect("initial output timeout");

    let attach_res = cmd_terminal_attach(
        app.handle().clone(),
        client_state.clone(),
        spawned.session_id.clone(),
        None,
    )
    .await
    .expect("attach");

    assert_eq!(attach_res.session_id, spawned.session_id);
    assert!(attach_res.daemon_epoch.is_some());
    let epoch_num: u64 = attach_res
        .daemon_epoch
        .as_ref()
        .unwrap()
        .parse()
        .expect("daemon epoch is decimal string");
    assert!(epoch_num > 0);

    let decoded_history = STANDARD
        .decode(&attach_res.history)
        .expect("valid base64 history");
    let history_str = String::from_utf8_lossy(&decoded_history);
    assert!(history_str.contains("initial_attach_test"));

    if let Some(start_seq) = attach_res.history_start_sequence {
        assert!(
            start_seq.parse::<u64>().is_ok(),
            "start sequence is decimal string"
        );
    }
    if let Some(end_seq) = attach_res.history_end_sequence {
        assert!(
            end_seq.parse::<u64>().is_ok(),
            "end sequence is decimal string"
        );
    }

    cmd_terminal_close(client_state, spawned.session_id)
        .await
        .expect("close");

    server_task.abort();
}

#[tokio::test]
async fn tauri_mock_worktree_commands_use_identity_contract() {
    let (repo, registry) = setup_workspace();
    let app = tauri::test::mock_builder()
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
            worktree: identity.clone(),
            delete_branch: Some(true),
        },
    )
    .await
    .expect("delete");

    assert!(!created.path.exists());
    let branches = run_git(repo.path(), &["branch", "--list"]).expect("list branches");
    assert!(!branches.contains("orca/ws-ipc/task-ipc"));

    let final_list = cmd_worktree_list(registry_state, "workspace-test".into())
        .await
        .expect("final list");
    assert_eq!(final_list.len(), 1);
}

#[tokio::test]
async fn terminal_global_events_preserve_raw_bytes_and_lifecycle() {
    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
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
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
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
        client_state.clone(),
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

    cmd_terminal_close(client_state, spawned.session_id.clone())
        .await
        .expect("close");

    let exited = tokio::time::timeout(tokio::time::Duration::from_secs(2), lifecycle_rx.recv())
        .await
        .expect("exited timeout")
        .expect("exited event");
    assert_eq!(exited.session_id, spawned.session_id);
    assert_eq!(exited.state, TerminalLifecycleState::Exited);
    assert!(exited.reason.is_none());

    server_task.abort();
}

#[tokio::test]
async fn terminal_cwd_cache_and_resolution_contract() {
    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("spawn");

    let cwd_res = cmd_terminal_get_cwd(client_state.clone(), spawned.session_id.clone())
        .await
        .expect("get cwd");
    assert!(cwd_res.cwd.exists());

    let cached = get_cached_cwd(&spawned.session_id);
    assert_eq!(cached, Some(cwd_res.cwd.clone()));

    cmd_terminal_close(client_state.clone(), spawned.session_id.clone())
        .await
        .expect("close");
    assert_eq!(get_cached_cwd(&spawned.session_id), None);

    server_task.abort();
}

#[tokio::test]
async fn terminal_output_batching_coalesces_rapid_bursts() {
    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(100);

    app.listen(TERMINAL_OUTPUT_EVENT, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = output_tx.try_send(payload);
        }
    });

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state,
        SpawnTerminalRequest {
            workspace_id: "workspace-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("spawn");

    cmd_terminal_write(
        client_state.clone(),
        spawned.session_id.clone(),
        "for i in $(seq 1 20); do echo \"LINE_$i\"; done\n".into(),
    )
    .await
    .expect("write burst");

    let mut event_count = 0;
    let mut collected = Vec::new();
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&collected).contains("LINE_20") {
            let payload = output_rx.recv().await.expect("output event");
            assert_eq!(payload.session_id, spawned.session_id);
            event_count += 1;
            collected.extend_from_slice(&STANDARD.decode(payload.data).expect("base64 payload"));
        }
    })
    .await
    .expect("burst output timeout");

    let output_str = String::from_utf8_lossy(&collected);
    assert!(output_str.contains("LINE_1"));
    assert!(output_str.contains("LINE_20"));
    assert!(
        event_count < 15,
        "expected batched events < 15, got {event_count}"
    );

    cmd_terminal_close(client_state, spawned.session_id)
        .await
        .expect("close");

    server_task.abort();
}

#[tokio::test]
async fn remote_status_after_reopen_persists_config_mode_until_started() {
    let dir = TempDir::new().expect("tempdir");
    let config_path = dir.path().join("remote-config.json");
    let auth_path = dir.path().join("remote-auth.json");
    std::fs::write(
        &config_path,
        r#"{"mode":"localNetwork","port":45678,"allowControl":false}"#,
    )
    .expect("write stale enabled config");

    let (_daemon_dir, daemon_client, server_task) =
        setup_test_daemon_with_remote_paths(Some(config_path), Some(auth_path)).await;

    let remote_manager = Arc::new(RemoteGatewayManager::from_daemon(Arc::clone(
        &daemon_client,
    )));
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&daemon_client))
        .manage(remote_manager)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let status = cmd_remote_status(app.state()).await.expect("status");
    assert!(
        !status.enabled,
        "relaunch must not report an active listener"
    );
    assert_eq!(status.mode, RemoteNetworkMode::LocalNetwork);
    assert_eq!(status.port, 45678);
    assert!(status.bound_address.is_none());
    assert_eq!(status.restart_policy, RemoteRestartPolicy::RestoreListener);

    server_task.abort();
}

#[test]
fn terminal_process_cwd_resolves_accurately() {
    let current_pid = std::process::id();
    let resolved = process_cwd(current_pid);
    assert!(resolved.is_some(), "process_cwd should resolve current pid");
    let resolved_path = resolved.expect("resolved path");
    assert!(resolved_path.exists());
    let current_dir = std::env::current_dir().expect("current dir");
    assert_eq!(
        resolved_path.canonicalize().expect("canonicalize resolved"),
        current_dir
            .canonicalize()
            .expect("canonicalize current dir")
    );
}

#[tokio::test]
async fn test_project_registration_then_daemon_spawn() {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.email", "test@example.com"]).expect("email");
    run_git(repo.path(), &["config", "user.name", "Test User"]).expect("name");
    std::fs::write(repo.path().join("README.md"), "initial\n").expect("README");
    run_git(repo.path(), &["add", "README.md"]).expect("add");
    run_git(repo.path(), &["commit", "-m", "initial commit"]).expect("commit");

    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    let local_registry = WorkspaceRegistry::new();

    let app = tauri::test::mock_builder()
        .manage(daemon_client.clone())
        .manage(local_registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let registered = cmd_project_register(
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        RegisterProjectRequest {
            workspace_id: "workspace-reg-test".into(),
            repo_path: repo.path().to_path_buf(),
        },
    )
    .await
    .expect("cmd_project_register succeeds");

    assert_eq!(registered.workspace_id, "workspace-reg-test");

    // Attempting terminal spawn for the registered workspace.
    // RED EXPECTATION: Fails because daemon workspace registry was not updated by cmd_project_register.
    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        SpawnTerminalRequest {
            workspace_id: "workspace-reg-test".into(),
            worktree: None,
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            client_request_id: None,
        },
    )
    .await
    .expect("terminal spawn on daemon must succeed after GUI project registration");

    cmd_terminal_close(app.state::<Arc<DaemonClient>>(), spawned.session_id)
        .await
        .expect("close");

    server_task.abort();
}
