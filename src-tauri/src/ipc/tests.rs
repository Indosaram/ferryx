// allow: SIZE_OK — Tauri IPC integration tests for terminal, worktree, and daemon commands
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

// A real login shell must start before it can echo, so this bounds the await on the
// expected output rather than the shell's startup cost, which varies with machine load.
const REAL_SHELL_OUTPUT_DEADLINE: tokio::time::Duration = tokio::time::Duration::from_secs(30);

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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
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
    tokio::time::timeout(REAL_SHELL_OUTPUT_DEADLINE, async {
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
        .any(|session| session.session_id == spawned.session_id && session.running));

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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
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
    tokio::time::timeout(REAL_SHELL_OUTPUT_DEADLINE, async {
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
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register daemon workspace");
    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let registry_state = app.state::<WorkspaceRegistry>();
    let identity = WorktreeIdentity {
        ws_id: "ws-ipc".into(),
        slug: "task-ipc".into(),
    };

    let initial = cmd_worktree_list(
        app.handle().clone(),
        registry_state.clone(),
        "workspace-test".into(),
    )
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

    let final_list = cmd_worktree_list(
        app.handle().clone(),
        registry_state,
        "workspace-test".into(),
    )
    .await
    .expect("final list");
    assert_eq!(final_list.len(), 1);
    server_task.abort();
    assert!(server_task
        .await
        .expect_err("daemon listener aborted")
        .is_cancelled());
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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
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
    tokio::time::timeout(REAL_SHELL_OUTPUT_DEADLINE, async {
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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
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
    tokio::time::timeout(REAL_SHELL_OUTPUT_DEADLINE, async {
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
    assert_eq!(status.port, crate::remote::state::REMOTE_GATEWAY_PORT);
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
async fn cmd_project_unregister_removes_registry_entry() {
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    let (_repo, registry) = setup_workspace();
    assert!(registry.contains("workspace-test"));

    let app = tauri::test::mock_builder()
        .manage(daemon_client)
        .manage(registry.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    cmd_project_unregister(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        UnregisterProjectRequest {
            workspace_id: "workspace-test".into(),
        },
    )
    .await
    .expect("unregister succeeds");

    assert!(!registry.contains("workspace-test"));

    // Unregistering an unknown workspace is an acknowledged no-op, not an error:
    // the frontend catalog is the source of truth and may already be gone.
    cmd_project_unregister(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        UnregisterProjectRequest {
            workspace_id: "never-registered".into(),
        },
    )
    .await
    .expect("unregister unknown id is a no-op");

    server_task.abort();
}

#[tokio::test]
async fn cmd_path_reveal_rejects_nonexistent_path_with_invalid_path_code() {
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let err = cmd_path_reveal(
        app.handle().clone(),
        "/definitely/not/a/real/path/orca-lite-probe".to_string(),
        None,
    )
    .await
    .expect_err("nonexistent path must be rejected before any spawn");
    assert_eq!(err.code, IpcErrorCode::InvalidPath);
}

#[tokio::test]
async fn cmd_project_unregister_serializes_camel_case_request() {
    let request = UnregisterProjectRequest {
        workspace_id: "ws".into(),
    };
    let value = serde_json::to_value(&request).expect("serialize");
    assert_eq!(value, serde_json::json!({ "workspaceId": "ws" }));
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
            shell: None,
            startup: None,
            inherit_from_session_id: None,
        },
    )
    .await
    .expect("terminal spawn on daemon must succeed after GUI project registration");

    cmd_terminal_close(app.state::<Arc<DaemonClient>>(), spawned.session_id)
        .await
        .expect("close");

    server_task.abort();
}

#[tokio::test]
async fn agent_resume_startup_validation_failure_before_pty_spawn() {
    use crate::daemon::protocol::{AgentProviderSession, AgentProviderSessionKey, TerminalStartup};
    use crate::ipc::terminal::cmd_terminal_spawn;

    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client.clone())
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let client_state = app.state::<Arc<DaemonClient>>();
    let registry_state = app.state::<WorkspaceRegistry>();

    // 1. Send invalid session ID starting with dash ("--new")
    let invalid_id_req = SpawnTerminalRequest {
        workspace_id: "workspace-test".into(),
        worktree: None,
        cwd: None,
        cols: Some(80),
        rows: Some(24),
        client_request_id: Some("req-invalid-id".into()),
        inherit_from_session_id: None,
        shell: None,
        startup: Some(TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "--new".to_string(),
                transcript_path: None,
            },
        }),
    };

    let err_id = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state.clone(),
        invalid_id_req,
    )
    .await
    .expect_err("must reject leading dash session ID before PTY spawn");

    assert_eq!(err_id.code, crate::ipc::IpcErrorCode::AgentResumeInvalid);
    assert!(
        err_id.details.as_ref().is_some_and(|details| {
            details
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|reason| reason.contains("id cannot start with a dash"))
        }),
        "Unexpected error: {err_id:?}"
    );

    // 2. Send wrong provider session key (Claude with conversation_id)
    let wrong_key_req = SpawnTerminalRequest {
        workspace_id: "workspace-test".into(),
        worktree: None,
        cwd: None,
        cols: Some(80),
        rows: Some(24),
        client_request_id: Some("req-wrong-key".into()),
        inherit_from_session_id: None,
        shell: None,
        startup: Some(TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: AgentProviderSession {
                key: AgentProviderSessionKey::ConversationId,
                id: "sess-valid-id".to_string(),
                transcript_path: None,
            },
        }),
    };

    let err_key = cmd_terminal_spawn(
        app.handle().clone(),
        client_state.clone(),
        registry_state.clone(),
        wrong_key_req,
    )
    .await
    .expect_err("must reject wrong provider key for Claude before PTY spawn");

    assert_eq!(err_key.code, crate::ipc::IpcErrorCode::AgentResumeInvalid);
    assert!(
        err_key.details.as_ref().is_some_and(|details| {
            details
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|reason| reason.contains("expected SessionId"))
        }),
        "Unexpected error: {err_key:?}"
    );

    // Verify no PTY sessions were spawned on the daemon
    let active_sessions = daemon_client.list_sessions().await.expect("list sessions");
    assert_eq!(
        active_sessions.len(),
        0,
        "No PTY sessions should be created on validation failure"
    );

    server_task.abort();
}

#[tokio::test]
async fn agent_resume_startup_cwd_jail_enforcement() {
    use crate::daemon::protocol::{AgentProviderSession, AgentProviderSessionKey, TerminalStartup};
    use crate::ipc::terminal::cmd_terminal_spawn;

    let (repo, registry) = setup_workspace();
    let (_dir, daemon_client, server_task) = setup_test_daemon().await;
    daemon_client
        .register_workspace("workspace-test", &repo.path().to_string_lossy())
        .await
        .expect("register workspace on daemon");

    let app = tauri::test::mock_builder()
        .manage(daemon_client.clone())
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let outside_dir = tempfile::tempdir().expect("outside tempdir");
    let outside_path = outside_dir.path().to_path_buf();

    let request_outside_cwd = SpawnTerminalRequest {
        workspace_id: "workspace-test".into(),
        worktree: None,
        cwd: Some(outside_path),
        cols: Some(80),
        rows: Some(24),
        client_request_id: Some("req-outside-cwd".into()),
        inherit_from_session_id: None,
        shell: None,
        startup: Some(TerminalStartup::AgentResume {
            agent_type: "claude".to_string(),
            provider_session: AgentProviderSession {
                key: AgentProviderSessionKey::SessionId,
                id: "sess-valid-id".to_string(),
                transcript_path: None,
            },
        }),
    };

    let err = cmd_terminal_spawn(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        request_outside_cwd,
    )
    .await
    .expect_err("must enforce CWD jail for agent resume startup");

    assert_eq!(err.code, IpcErrorCode::PathOutsideWorkspace);

    server_task.abort();
}

#[test]
fn test_spawn_terminal_request_serde_camelcase_roundtrip() {
    use crate::daemon::protocol::{AgentProviderSessionKey, TerminalStartup};

    let req_json = r#"{
        "workspaceId": "ws-1",
        "worktree": null,
        "cwd": "/repo/path",
        "cols": 120,
        "rows": 40,
        "clientRequestId": "req-123",
        "shell": null,
        "startup": {
            "kind": "agentResume",
            "agentType": "omo",
            "providerSession": {
                "key": "session_id",
                "id": "omo-session-456",
                "transcriptPath": null
            }
        }
    }"#;

    let req: SpawnTerminalRequest =
        serde_json::from_str(req_json).expect("deserialize SpawnTerminalRequest");
    assert_eq!(req.workspace_id, "ws-1");
    assert_eq!(req.cwd, Some(PathBuf::from("/repo/path")));
    assert_eq!(req.cols, Some(120));
    assert_eq!(req.rows, Some(40));
    assert_eq!(req.client_request_id, Some("req-123".to_string()));
    assert_eq!(req.shell, None);

    match req.startup.as_ref() {
        Some(TerminalStartup::AgentResume {
            agent_type,
            provider_session,
        }) => {
            assert_eq!(agent_type, "omo");
            assert_eq!(provider_session.key, AgentProviderSessionKey::SessionId);
            assert_eq!(provider_session.id, "omo-session-456");
            assert_eq!(provider_session.transcript_path, None);
        }
        _ => panic!("Expected AgentResume variant"),
    }

    // Verify serialization roundtrip preserves camelCase
    let serialized = serde_json::to_string(&req).expect("serialize");
    assert!(serialized.contains(r#""workspaceId":"ws-1""#));
    assert!(serialized.contains(r#""clientRequestId":"req-123""#));
    assert!(serialized.contains(r#""kind":"agentResume""#));
    assert!(serialized.contains(r#""agentType":"omo""#));
    assert!(serialized.contains(r#""sessionId""#) || serialized.contains(r#""session_id""#));
}

#[tokio::test]
async fn remote_terminal_spawn_forwards_worktree_and_cwd_to_daemon() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DaemonSessionDetails};
    use crate::ipc::terminal::cmd_terminal_spawn;
    use crate::worktree::WorktreeIdentity;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("mock_daemon.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind unix listener");

    let (req_tx, mut req_rx) = tokio::sync::mpsc::channel::<DaemonRequest>(8);

    let server_task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let req_tx = req_tx.clone();
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut line = String::new();
                while let Ok(n) = reader.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    if let Ok(req) = serde_json::from_str::<DaemonRequest>(line.trim()) {
                        let _ = req_tx.send(req.clone()).await;
                        let resp = match req {
                            DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                                version: crate::daemon::protocol::DAEMON_PROTOCOL_VERSION,
                                pid: std::process::id(),
                                epoch: 1,
                                binary_path: None,
                                binary_mtime_ms: None,
                                daemon_version: None,
                            },
                            DaemonRequest::Spawn { .. } => DaemonResponse::SpawnOk {
                                session_id: "remote-mock-session".into(),
                                epoch: 1,
                                session: DaemonSessionDetails {
                                    session_id: "remote-mock-session".into(),
                                    workspace_id: None,
                                    worktree: None,
                                    cwd: None,
                                    cols: 90,
                                    rows: 30,
                                    running: true,
                                    start_sequence: None,
                                    end_sequence: None,
                                },
                            },
                            DaemonRequest::Attach { .. } => DaemonResponse::AttachOk {
                                epoch: 1,
                                session_id: "remote-mock-session".into(),
                                start_sequence: None,
                                end_sequence: None,
                                gap: None,
                                history: bytes::Bytes::new(),
                                pty_cols: Some(90),
                                pty_rows: Some(30),
                                history_segments: vec![],
                                remote_generation: None,
                            },
                            _ => DaemonResponse::Pong,
                        };
                        let mut resp_json = serde_json::to_string(&resp).unwrap();
                        resp_json.push('\n');
                        let _ = write_half.write_all(resp_json.as_bytes()).await;
                        let _ = write_half.flush().await;
                    }
                    line.clear();
                }
            });
        }
    });

    let daemon_client = Arc::new(DaemonClient::new_with_socket(socket_path));
    let registry = WorkspaceRegistry::new();
    let app = tauri::test::mock_builder()
        .manage(daemon_client.clone())
        .manage(registry)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let store_path = crate::ipc::ssh::get_ssh_store_path(&app.handle()).expect("store path");
    std::fs::create_dir_all(store_path.parent().unwrap()).unwrap();

    let host = crate::ssh::SshHost {
        id: "mock-host".into(),
        label: "Mock Host".into(),
        hostname: "127.0.0.1".into(),
        username: Some("mock-user".into()),
        port: Some(22),
        identity_file: None,
        jump_host: None,
        source: crate::ssh::SshHostSource::Config,
        auth_method: crate::ssh::SshAuthMethod::Agent,
        disabled: None,
    };
    std::fs::write(
        &store_path,
        serde_json::to_vec(&crate::ipc::ssh::SshHostStore {
            hosts: vec![host.clone()],
            tombstones: vec![],
        })
        .unwrap(),
    )
    .unwrap();

    let ws_id = crate::ssh::projects::identity(&host.id, "/srv/project");
    let project = crate::ssh::projects::RemoteProject {
        workspace_id: ws_id.clone(),
        host_id: host.id.clone(),
        repo_root: "/srv/project".into(),
        git_root: None,
        git_remote: None,
        git_branch: None,
        git_head: None,
        platform: Some(crate::ssh::runtime::RemotePlatform::Posix),
    };
    let mut projects_map = std::collections::BTreeMap::new();
    projects_map.insert(ws_id.clone(), project);
    std::fs::write(
        crate::ssh::projects::store_path(&store_path),
        serde_json::to_vec(&projects_map).unwrap(),
    )
    .unwrap();

    let wt_ident = WorktreeIdentity {
        ws_id: "agent-1".into(),
        slug: "feat-remote".into(),
    };
    let wt_path = PathBuf::from("/srv/project/.orca-worktrees/wt-feat-remote");

    let request = SpawnTerminalRequest {
        workspace_id: ws_id.clone(),
        worktree: Some(wt_ident.clone()),
        cwd: Some(wt_path.clone()),
        cols: Some(90),
        rows: Some(30),
        client_request_id: Some("req-forward-wt".into()),
        inherit_from_session_id: None,
        shell: None,
        startup: None,
    };

    let spawned = cmd_terminal_spawn(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        request,
    )
    .await
    .expect("remote terminal spawn should forward worktree and cwd to daemon");

    assert_eq!(spawned.session_id, "remote-mock-session");

    // Verify DaemonRequest::Spawn captured worktree and cwd
    let mut received_spawn = None;
    while let Ok(req) = req_rx.try_recv() {
        if let DaemonRequest::Spawn { .. } = req {
            received_spawn = Some(req);
            break;
        }
    }
    let Some(DaemonRequest::Spawn {
        workspace_id,
        worktree,
        cwd,
        cols,
        rows,
        startup,
        ..
    }) = received_spawn
    else {
        panic!("expected DaemonRequest::Spawn to be received by mock daemon");
    };

    assert_eq!(workspace_id, ws_id);
    assert_eq!(worktree, Some(wt_ident));
    assert_eq!(cwd, Some(wt_path.to_string_lossy().to_string()));
    assert_eq!(cols, 90);
    assert_eq!(rows, 30);
    assert_eq!(
        startup,
        Some(crate::daemon::protocol::TerminalStartup::RemoteSsh {
            host_store_path: store_path
        })
    );

    server_task.abort();
}

#[tokio::test]
async fn remote_terminal_spawn_rejects_explicit_startup() {
    use crate::ipc::terminal::cmd_terminal_spawn;

    let app = tauri::test::mock_builder()
        .manage(Arc::new(DaemonClient::new_with_socket(PathBuf::from(
            "/unused",
        ))))
        .manage(WorkspaceRegistry::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    let request = SpawnTerminalRequest {
        workspace_id: "ssh:0123456789abcdef".into(),
        worktree: None,
        cwd: None,
        cols: Some(80),
        rows: Some(24),
        client_request_id: None,
        inherit_from_session_id: None,
        shell: None,
        startup: Some(crate::daemon::protocol::TerminalStartup::AgentResume {
            agent_type: "claude".into(),
            provider_session: crate::daemon::protocol::AgentProviderSession {
                key: crate::daemon::protocol::AgentProviderSessionKey::SessionId,
                id: "sess-1".into(),
                transcript_path: None,
            },
        }),
    };

    let err = cmd_terminal_spawn(
        app.handle().clone(),
        app.state::<Arc<DaemonClient>>(),
        app.state::<WorkspaceRegistry>(),
        request,
    )
    .await
    .unwrap_err();

    assert_eq!(err.code, IpcErrorCode::Unsupported);
}

#[test]
fn paired_machine_worktree_spawn_target_resolution() {
    use crate::ipc::terminal::{
        effective_paired_repo_root, infer_worktree_slug, resolve_paired_spawn_target,
    };
    use crate::worktree::WorktreeIdentity;
    use std::path::Path;

    let repo_root = Path::new("/srv/repo");
    let wt_cwd = Path::new("/srv/repo/.orca-worktrees/wt-my-feature");
    let sub_cwd = Path::new("/srv/repo/.orca-worktrees/wt-my-feature/src/lib");
    let explicit_wt = WorktreeIdentity {
        ws_id: "remote-ws".into(),
        slug: "my-feature".into(),
    };

    // 1. Explicit worktree + worktree root cwd: cwd_relative MUST be None (never .orca-worktrees/wt-...)
    let slug = infer_worktree_slug(Some(&explicit_wt), Some(wt_cwd));
    assert_eq!(slug.as_deref(), Some("my-feature"));
    let (ws, wt_ident, cwd_rel) =
        resolve_paired_spawn_target("remote-ws", repo_root, Some(wt_cwd), slug.as_deref());
    assert_eq!(ws, "remote-ws");
    assert_eq!(
        wt_ident.as_ref().map(|w| w.slug.as_str()),
        Some("my-feature")
    );
    assert_eq!(
        cwd_rel, None,
        "cwd_relative must be None when cwd points to worktree root"
    );

    // 2. Explicit worktree + subdirectory cwd: cwd_relative is relative to the worktree root
    let (ws, wt_ident, cwd_rel) =
        resolve_paired_spawn_target("remote-ws", repo_root, Some(sub_cwd), slug.as_deref());
    assert_eq!(ws, "remote-ws");
    assert_eq!(
        wt_ident.as_ref().map(|w| w.slug.as_str()),
        Some("my-feature")
    );
    assert_eq!(cwd_rel.as_deref(), Some("src/lib"));

    // 3. Inferred slug from cwd path when request.worktree is None
    let inferred = infer_worktree_slug(None, Some(wt_cwd));
    assert_eq!(inferred.as_deref(), Some("my-feature"));
    let (_ws, wt_ident, cwd_rel) =
        resolve_paired_spawn_target("remote-ws", repo_root, Some(wt_cwd), inferred.as_deref());
    assert_eq!(
        wt_ident.as_ref().map(|w| w.slug.as_str()),
        Some("my-feature")
    );
    assert_eq!(cwd_rel, None);

    // 4. Effective repo root inference when stored repo root was empty
    let empty_root = Path::new("");
    let recovered = effective_paired_repo_root(empty_root, Some(wt_cwd));
    assert_eq!(recovered, Path::new("/srv/repo"));

    // 5. Effective repo root recovery when stored repo root was corrupted with worktree path
    let corrupted_root = Path::new("/srv/repo/.orca-worktrees/wt-old-worktree");
    let recovered_corr = effective_paired_repo_root(corrupted_root, Some(wt_cwd));
    assert_eq!(recovered_corr, Path::new("/srv/repo"));

    // 6. Windows backslash path support
    let win_cwd = Path::new(r"C:\Users\repo\.orca-worktrees\wt-win-feature\sub");
    let win_root = Path::new(r"C:\Users\repo");
    let win_slug = infer_worktree_slug(None, Some(win_cwd));
    assert_eq!(win_slug.as_deref(), Some("win-feature"));
    let (ws, wt_ident, cwd_rel) =
        resolve_paired_spawn_target("remote-ws", win_root, Some(win_cwd), win_slug.as_deref());
    assert_eq!(ws, "remote-ws");
    assert_eq!(
        wt_ident.as_ref().map(|w| w.slug.as_str()),
        Some("win-feature")
    );
    assert_eq!(cwd_rel.as_deref(), Some("sub"));
}

#[tokio::test]
async fn test_p10_ambiguous_create_session_reconciles_via_journal() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::reconcile_ambiguous_create;
    use crate::paired_host::client::{Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p10.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let fixture_session = m::Session {
        title: None,
        agent_type: None,
        provider_session: None,
        workspace_id: "ws-1".into(),
        worktree: None,
        target: m::RemoteTerminalTarget {
            machine_id: "m-1".into(),
            session_id: "remote-session-p10".into(),
            daemon_epoch: Epoch(1),
        },
        cols: 80,
        rows: 24,
        running: true,
        start_sequence: Epoch(0),
        end_sequence: Epoch(0),
        cwd: "/remote/dir".into(),
    };

    let fixture_for_server = fixture_session.clone();
    let close_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let close_called_clone = close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let fixture = fixture_for_server.clone();
            let close_flag = close_called_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::Operation { request_id } => {
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::Operation(
                                            m::Operation::Completed {
                                                request_id,
                                                outcome: m::OperationOutcome::Session {
                                                    session: fixture.clone(),
                                                },
                                            },
                                        ),
                                    },
                                }
                            }
                            Operation::CloseSession { .. } => {
                                close_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::CloseSession(()),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);

    let req_id_1 = "a0000000-0000-0000-0000-000000000001";
    let recovered = reconcile_ambiguous_create(&client, "host-1", Epoch(1), req_id_1, false)
        .await
        .expect("reconcile_ambiguous_create must succeed for Completed session outcome");
    let session =
        recovered.expect("recovered session must be returned from journal Completed outcome");
    assert_eq!(session.target.session_id, "remote-session-p10");

    let req_id_2 = "a0000000-0000-0000-0000-000000000002";
    let cancelled = reconcile_ambiguous_create(&client, "host-1", Epoch(1), req_id_2, true)
        .await
        .expect("reconcile_ambiguous_create with cancelled=true must succeed");
    assert!(
        cancelled.is_none(),
        "cancelled reconciliation must return None"
    );
    assert!(
        close_called.load(std::sync::atomic::Ordering::SeqCst),
        "cancelled reconciliation must explicitly close the remote target"
    );

    server.abort();
}

#[tokio::test]
async fn test_p10_background_reconciler_adopts_delayed_completed_session() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{
        clear_pending_creates_for_test, get_pending_create, reconcile_ambiguous_create,
        PendingCreateStatus,
    };
    use crate::paired_host::client::{Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_creates_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p10_adopt.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let poll_count = Arc::new(AtomicUsize::new(0));
    let poll_count_clone = poll_count.clone();

    let fixture_session = m::Session {
        title: None,
        agent_type: None,
        provider_session: None,
        workspace_id: "ws-p10".into(),
        worktree: None,
        target: m::RemoteTerminalTarget {
            machine_id: "m-1".into(),
            session_id: "remote-p10-delayed".into(),
            daemon_epoch: Epoch(1),
        },
        cols: 80,
        rows: 24,
        running: true,
        start_sequence: Epoch(0),
        end_sequence: Epoch(0),
        cwd: "/remote/dir".into(),
    };
    let fixture_for_server = fixture_session.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let polls = poll_count_clone.clone();
            let fixture = fixture_for_server.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => {
                            match request.operation {
                                Operation::Operation { request_id } => {
                                    let current = polls.fetch_add(1, Ordering::SeqCst);
                                    if current < 5 {
                                        // Return Pending for the first 5 polls (exceeding 3 inline attempts)
                                        DaemonResponse::PairedHostOperationOk {
                                            response: OperationResponse {
                                                host_id: "host-1".into(),
                                                generation: Epoch(1),
                                                result: OperationResult::Operation(
                                                    m::Operation::Pending { request_id },
                                                ),
                                            },
                                        }
                                    } else {
                                        // Resolves Completed on subsequent poll
                                        DaemonResponse::PairedHostOperationOk {
                                            response: OperationResponse {
                                                host_id: "host-1".into(),
                                                generation: Epoch(1),
                                                result: OperationResult::Operation(
                                                    m::Operation::Completed {
                                                        request_id,
                                                        outcome: m::OperationOutcome::Session {
                                                            session: fixture.clone(),
                                                        },
                                                    },
                                                ),
                                            },
                                        }
                                    }
                                }
                                _ => DaemonResponse::Error {
                                    message: "unexpected op".into(),
                                    code: None,
                                    details: None,
                                },
                            }
                        }
                        DaemonRequest::PairedTerminalReattach { .. } => {
                            DaemonResponse::PairedTerminalReattachOk {
                                session_id: "proxy-p10-adopted".into(),
                                generation: Epoch(1),
                            }
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let req_id = "a0000000-0000-0000-0000-000000000099";

    let inline_res = reconcile_ambiguous_create(&client, "host-1", Epoch(1), req_id, false).await;
    assert!(
        inline_res.is_err(),
        "Inline reconcile should exhaust attempts and return Err"
    );

    // Wait for background reconciler to adopt session
    let mut adopted = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Some(record) = get_pending_create(req_id) {
            if let PendingCreateStatus::Adopted { proxy_session_id } = record.status {
                assert_eq!(proxy_session_id, "proxy-p10-adopted");
                adopted = true;
                break;
            }
        }
    }
    assert!(
        adopted,
        "Delayed Completed create must be adopted by background reconciler"
    );

    server.abort();
}

#[tokio::test]
async fn test_p10_background_reconciler_closes_cancelled_delayed_completed_session() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{clear_pending_creates_for_test, reconcile_ambiguous_create};
    use crate::paired_host::client::{Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_creates_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p10_cancel.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let poll_count = Arc::new(AtomicUsize::new(0));
    let poll_count_clone = poll_count.clone();
    let close_called = Arc::new(AtomicBool::new(false));
    let close_called_clone = close_called.clone();

    let fixture_session = m::Session {
        title: None,
        agent_type: None,
        provider_session: None,
        workspace_id: "ws-p10-cancel".into(),
        worktree: None,
        target: m::RemoteTerminalTarget {
            machine_id: "m-1".into(),
            session_id: "remote-p10-to-close".into(),
            daemon_epoch: Epoch(1),
        },
        cols: 80,
        rows: 24,
        running: true,
        start_sequence: Epoch(0),
        end_sequence: Epoch(0),
        cwd: "/remote/dir".into(),
    };
    let fixture_for_server = fixture_session.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let polls = poll_count_clone.clone();
            let fixture = fixture_for_server.clone();
            let close_flag = close_called_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::Operation { request_id } => {
                                let current = polls.fetch_add(1, Ordering::SeqCst);
                                if current < 5 {
                                    DaemonResponse::PairedHostOperationOk {
                                        response: OperationResponse {
                                            host_id: "host-1".into(),
                                            generation: Epoch(1),
                                            result: OperationResult::Operation(
                                                m::Operation::Pending { request_id },
                                            ),
                                        },
                                    }
                                } else {
                                    DaemonResponse::PairedHostOperationOk {
                                        response: OperationResponse {
                                            host_id: "host-1".into(),
                                            generation: Epoch(1),
                                            result: OperationResult::Operation(
                                                m::Operation::Completed {
                                                    request_id,
                                                    outcome: m::OperationOutcome::Session {
                                                        session: fixture.clone(),
                                                    },
                                                },
                                            ),
                                        },
                                    }
                                }
                            }
                            Operation::CloseSession { .. } => {
                                close_flag.store(true, Ordering::SeqCst);
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::CloseSession(()),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let req_id = "a0000000-0000-0000-0000-000000000098";

    let inline_res = reconcile_ambiguous_create(&client, "host-1", Epoch(1), req_id, true).await;
    assert!(
        inline_res.is_err(),
        "Inline reconcile should exhaust attempts and return Err"
    );

    // Wait for background reconciler to close the session
    let mut closed = false;
    for _ in 0..40 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if close_called.load(Ordering::SeqCst) {
            closed = true;
            break;
        }
    }
    assert!(
        closed,
        "Cancelled delayed Completed create must be closed by background reconciler"
    );

    server.abort();
}

#[tokio::test]
async fn test_p11_reattach_failure_cleanup_reconciles_and_reaps_unknown() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{
        clear_pending_cleanups_for_test, execute_cleanup_close, get_pending_cleanups,
        reap_cleanup_unknowns, CleanupOutcome,
    };
    use crate::paired_host::client::{ClientError, Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_cleanups_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p11.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let journal_queries = Arc::new(AtomicUsize::new(0));
    let journal_queries_clone = journal_queries.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let j_queries = journal_queries_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { .. } => {
                                DaemonResponse::PairedHostOperationError {
                                    error: ClientError {
                                        code: "TIMEOUT".into(),
                                        machine_error: None,
                                        ambiguous: true,
                                        request_id: Some(
                                            "a0000000-0000-0000-0000-000000000011".into(),
                                        ),
                                    },
                                }
                            }
                            Operation::Operation { request_id } => {
                                let q = j_queries.fetch_add(1, Ordering::SeqCst);
                                if q == 0 {
                                    DaemonResponse::PairedHostOperationOk {
                                        response: OperationResponse {
                                            host_id: "host-1".into(),
                                            generation: Epoch(1),
                                            result: OperationResult::Operation(
                                                m::Operation::Pending { request_id },
                                            ),
                                        },
                                    }
                                } else {
                                    DaemonResponse::PairedHostOperationOk {
                                        response: OperationResponse {
                                            host_id: "host-1".into(),
                                            generation: Epoch(1),
                                            result: OperationResult::Operation(
                                                m::Operation::Completed {
                                                    request_id,
                                                    outcome: m::OperationOutcome::NoContent,
                                                },
                                            ),
                                        },
                                    }
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let cleanup_req_id = "a0000000-0000-0000-0000-000000000011".to_string();

    let outcome = execute_cleanup_close(
        &client,
        "host-1",
        Epoch(1),
        "remote-s1",
        Epoch(1),
        cleanup_req_id.clone(),
    )
    .await;

    match outcome {
        CleanupOutcome::Unknown {
            ref cleanup_request_id,
            ..
        } => {
            assert_eq!(cleanup_request_id, &cleanup_req_id);
        }
        other => panic!("Expected CleanupOutcome::Unknown, got {other:?}"),
    }

    assert_eq!(
        get_pending_cleanups().len(),
        1,
        "Unknown cleanup must be recorded in pending cleanups"
    );

    let resolved = reap_cleanup_unknowns(&client).await;
    assert_eq!(
        resolved, 1,
        "Reaper must resolve exactly 1 pending unknown cleanup"
    );
    assert!(
        get_pending_cleanups().is_empty(),
        "Pending cleanups must be empty after resolution"
    );

    server.abort();
}

#[tokio::test]
async fn test_p12_close_terminates_remote_session_while_detach_preserves_it() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::paired_host::client::{Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p12.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let remote_close_called = Arc::new(AtomicBool::new(false));
    let detach_called = Arc::new(AtomicBool::new(false));
    let local_close_called = Arc::new(AtomicBool::new(false));

    let remote_close_clone = remote_close_called.clone();
    let detach_clone = detach_called.clone();
    let local_close_clone = local_close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let remote_close = remote_close_clone.clone();
            let detach = detach_clone.clone();
            let local_close = local_close_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { session_id } => {
                            if session_id.starts_with("daemon-session:") {
                                DaemonResponse::PairedTerminalDescriptorOk {
                                    descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        target: m::RemoteTerminalTarget {
                                            machine_id: "m-1".into(),
                                            daemon_epoch: Epoch(1),
                                            session_id: "remote-pty-p12".into(),
                                        },
                                        after_sequence: None,
                                    }),
                                }
                            } else {
                                DaemonResponse::PairedTerminalDescriptorOk { descriptor: None }
                            }
                        }
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { session_id, .. } => {
                                assert_eq!(session_id, "remote-pty-p12");
                                remote_close.store(true, Ordering::SeqCst);
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::CloseSession(()),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        DaemonRequest::PairedTerminalDetach { .. } => {
                            detach.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        DaemonRequest::Close { .. } => {
                            local_close.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);

    // 1. Detach on paired session: detaches proxy locally but preserves remote PTY session
    client
        .detach_terminal("daemon-session:paired-1")
        .await
        .expect("detach_terminal must succeed for paired session");
    assert!(
        detach_called.load(Ordering::SeqCst),
        "detach_terminal must call PairedTerminalDetach"
    );
    assert!(
        !remote_close_called.load(Ordering::SeqCst),
        "detach_terminal must NOT terminate remote PTY"
    );

    // 2. Detach on local session: must fail with invalid input
    let local_detach = client.detach_terminal("local-pane-1").await;
    assert!(
        local_detach.is_err(),
        "detach_terminal must be rejected for local sessions"
    );

    // 3. Close on paired session: terminates remote PTY session with CloseSession and closes local proxy
    client
        .close_terminal("daemon-session:paired-1")
        .await
        .expect("close_terminal must succeed for paired session");
    assert!(
        remote_close_called.load(Ordering::SeqCst),
        "close_terminal must terminate remote PTY with CloseSession"
    );
    assert!(
        local_close_called.load(Ordering::SeqCst),
        "close_terminal must close local session descriptor/proxy"
    );

    // 4. Close on local session: closes local session directly, no remote CloseSession
    remote_close_called.store(false, Ordering::SeqCst);
    local_close_called.store(false, Ordering::SeqCst);
    client
        .close_terminal("local-pane-1")
        .await
        .expect("close_terminal must succeed for local session");
    assert!(
        !remote_close_called.load(Ordering::SeqCst),
        "close_terminal for local session must NOT call remote CloseSession"
    );
    assert!(
        local_close_called.load(Ordering::SeqCst),
        "close_terminal for local session must close local PTY"
    );

    server.abort();
}

#[tokio::test]
async fn test_p11_reaper_retains_exhausted_records_in_dead_letter_list() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{
        clear_pending_cleanups_for_test, execute_cleanup_close, get_exhausted_cleanups,
        get_pending_cleanups, reap_cleanup_unknowns, CleanupOutcome,
    };
    use crate::paired_host::client::{ClientError, Operation};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_cleanups_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p11_dead_letter.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { .. } => {
                                DaemonResponse::PairedHostOperationError {
                                    error: ClientError {
                                        code: "TIMEOUT".into(),
                                        machine_error: None,
                                        ambiguous: true,
                                        request_id: Some("req-p11-dead".into()),
                                    },
                                }
                            }
                            Operation::Operation { request_id } => {
                                DaemonResponse::PairedHostOperationOk {
                                    response: crate::paired_host::client::OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result:
                                            crate::paired_host::client::OperationResult::Operation(
                                                m::Operation::Pending { request_id },
                                            ),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let cleanup_req_id = "req-p11-dead".to_string();

    let outcome = execute_cleanup_close(
        &client,
        "host-1",
        Epoch(1),
        "remote-s-dead",
        Epoch(1),
        cleanup_req_id.clone(),
    )
    .await;

    assert!(matches!(outcome, CleanupOutcome::Unknown { .. }));
    assert_eq!(get_pending_cleanups().len(), 1);

    // Reap attempt 1 (item.attempts becomes 2)
    let _ = reap_cleanup_unknowns(&client).await;
    assert_eq!(get_pending_cleanups().len(), 1);

    // Reap attempt 2 (item.attempts becomes 3)
    let _ = reap_cleanup_unknowns(&client).await;
    assert_eq!(get_pending_cleanups().len(), 1);

    // Reap attempt 3 (reaches MAX_REAP_ATTEMPTS = 3; must move to exhausted dead-letter list)
    let _ = reap_cleanup_unknowns(&client).await;
    assert_eq!(
        get_pending_cleanups().len(),
        0,
        "Pending cleanups must be empty after exhausting attempts"
    );
    assert_eq!(
        get_exhausted_cleanups().len(),
        1,
        "Exhausted cleanups must be retained in dead-letter list"
    );
    assert_eq!(
        get_exhausted_cleanups()[0].cleanup_request_id,
        cleanup_req_id
    );

    server.abort();
}

#[tokio::test]
async fn test_p10_pending_create_reaper_resolves_still_pending_records() {
    // P10 (round 2): the long-lived reaper must revisit still-Pending records
    // after the bounded background reconciler gives up (including records
    // loaded from disk after a restart) and adopt/close a late terminal
    // journal outcome.
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{
        clear_pending_creates_for_test, get_pending_create, register_pending_create,
        start_pending_create_reaper, PendingCreateStatus,
    };
    use crate::paired_host::client::{Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_creates_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p10_reaper.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let fixture_session = m::Session {
        title: None,
        agent_type: None,
        provider_session: None,
        workspace_id: "ws-p10-reaper".into(),
        worktree: None,
        target: m::RemoteTerminalTarget {
            machine_id: "m-1".into(),
            session_id: "remote-p10-reaper".into(),
            daemon_epoch: Epoch(1),
        },
        cols: 80,
        rows: 24,
        running: true,
        start_sequence: Epoch(0),
        end_sequence: Epoch(0),
        cwd: "/remote/dir".into(),
    };

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let fixture = fixture_session.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::Operation { request_id } => {
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::Operation(
                                            m::Operation::Completed {
                                                request_id,
                                                outcome: m::OperationOutcome::Session {
                                                    session: fixture.clone(),
                                                },
                                            },
                                        ),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        DaemonRequest::PairedTerminalReattach { .. } => {
                            DaemonResponse::PairedTerminalReattachOk {
                                session_id: "proxy-p10-reaper".into(),
                                generation: Epoch(1),
                            }
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    // Operation journal request ids are hyphenated UUIDs on the wire.
    let reaper_request_id = uuid::Uuid::new_v4().to_string();
    register_pending_create("host-1", Epoch(1), &reaper_request_id, false);
    assert_eq!(
        get_pending_create(&reaper_request_id).map(|r| r.status),
        Some(PendingCreateStatus::Pending)
    );

    // Production lifecycle entry point: first pass runs immediately.
    start_pending_create_reaper(std::sync::Arc::new(client.clone())).await;

    let mut adopted = false;
    for _ in 0..50 {
        // The PENDING_CREATES static is shared with parallel p10/p11 tests that
        // clear it; re-register if another test wiped the record so the
        // assertion tests reaper behavior, not static-clear race timing.
        if let Some(record) = get_pending_create(&reaper_request_id) {
            if !matches!(record.status, PendingCreateStatus::Pending) {
                adopted = matches!(record.status, PendingCreateStatus::Adopted { .. });
                break;
            }
        } else {
            register_pending_create("host-1", Epoch(1), &reaper_request_id, false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        adopted,
        "Reaper must adopt a late terminal session outcome; final={:?}",
        get_pending_create(&reaper_request_id)
    );

    server.abort();
}

#[tokio::test]
async fn test_p11_start_cleanup_reaper_schedules_background_resolution() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::terminal::{
        clear_pending_cleanups_for_test, execute_cleanup_close, get_pending_cleanups,
        start_cleanup_reaper,
    };
    use crate::paired_host::client::{ClientError, Operation};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    clear_pending_cleanups_for_test();

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p11_reaper_lifecycle.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { .. } => {
                                DaemonResponse::PairedHostOperationError {
                                    error: ClientError {
                                        code: "TIMEOUT".into(),
                                        machine_error: None,
                                        ambiguous: true,
                                        request_id: Some("req-p11-bg".into()),
                                    },
                                }
                            }
                            Operation::Operation { request_id } => {
                                DaemonResponse::PairedHostOperationOk {
                                    response: crate::paired_host::client::OperationResponse {
                                        host_id: "host-1".into(),
                                        generation: Epoch(1),
                                        result:
                                            crate::paired_host::client::OperationResult::Operation(
                                                m::Operation::Completed {
                                                    request_id,
                                                    outcome: m::OperationOutcome::NoContent,
                                                },
                                            ),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let cleanup_req_id = "req-p11-bg".to_string();

    let _ = execute_cleanup_close(
        &client,
        "host-1",
        Epoch(1),
        "remote-s-bg",
        Epoch(1),
        cleanup_req_id,
    )
    .await;

    assert_eq!(get_pending_cleanups().len(), 1);

    start_cleanup_reaper(client.clone()).await;

    // PairedHostOperation cycles against the in-process UDS fixture can take
    // tens of seconds each (same budget class as the other paired tests in
    // this file); give the reaper's first cycle a matching bounded window.
    let mut resolved = false;
    for _ in 0..3000 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if get_pending_cleanups().is_empty() {
            resolved = true;
            break;
        }
    }
    assert!(resolved, "Background reaper must resolve pending cleanup");

    server.abort();
}

#[tokio::test]
async fn test_p12_close_definitive_remote_error_aborts_local_close() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use crate::paired_host::client::{ClientError, Operation};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p12_def.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let local_close_called = Arc::new(AtomicBool::new(false));
    let local_close_clone = local_close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let local_close = local_close_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { .. } => {
                            DaemonResponse::PairedTerminalDescriptorOk {
                                descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                    host_id: "host-p12".into(),
                                    generation: Epoch(1),
                                    target: m::RemoteTerminalTarget {
                                        machine_id: "m-1".into(),
                                        daemon_epoch: Epoch(1),
                                        session_id: "remote-pty-def".into(),
                                    },
                                    after_sequence: None,
                                }),
                            }
                        }
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { .. } => {
                                DaemonResponse::PairedHostOperationError {
                                    error: ClientError {
                                        code: "PERMISSION_DENIED".into(),
                                        machine_error: None,
                                        ambiguous: false,
                                        request_id: Some("req-p12-def".into()),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        DaemonRequest::Close { .. } => {
                            local_close.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let err = client
        .close_terminal("daemon-session:paired-p12-def")
        .await
        .expect_err("Definitive remote error must cause close_terminal to fail");

    assert_eq!(err.code, IpcErrorCode::from_code_str("REMOTE_CLOSE_FAILED"));
    let details = err.details.expect("Error details must be present");
    assert_eq!(
        details.get("cause").and_then(|v| v.as_str()),
        Some("PERMISSION_DENIED")
    );
    assert_eq!(
        details.get("hostId").and_then(|v| v.as_str()),
        Some("host-p12")
    );
    assert_eq!(
        details.get("remoteSessionId").and_then(|v| v.as_str()),
        Some("remote-pty-def")
    );
    assert!(
        !local_close_called.load(Ordering::SeqCst),
        "Local close must NOT proceed on definitive remote close failure"
    );

    server.abort();
}

#[tokio::test]
async fn test_p12_close_descriptor_lookup_failure_is_uncertain() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p12_lookup.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let local_close_called = Arc::new(AtomicBool::new(false));
    let local_close_clone = local_close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let local_close = local_close_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        // P12 (round 2): a failing descriptor lookup must NOT
                        // silently skip the remote close.
                        DaemonRequest::PairedTerminalDescriptor { .. } => DaemonResponse::Error {
                            message: "descriptor store unavailable".into(),
                            code: Some("DESCRIPTOR_IO".into()),
                            details: None,
                        },
                        DaemonRequest::Close { .. } => {
                            local_close.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let err = client
        .close_terminal("daemon-session:paired-p12-lookup")
        .await
        .expect_err("Descriptor lookup failure must surface uncertainty");

    assert_eq!(
        err.code,
        IpcErrorCode::from_code_str("REMOTE_CLOSE_UNCERTAIN")
    );
    let details = err.details.expect("details present");
    assert_eq!(
        details.get("cause").and_then(|v| v.as_str()),
        Some("DESCRIPTOR_IO")
    );
    assert_eq!(
        details.get("remoteCloseUnknown"),
        Some(&serde_json::json!(true))
    );
    assert!(
        !local_close_called.load(Ordering::SeqCst),
        "Local close must NOT be acknowledged after an unprovable remote close"
    );

    server.abort();
}

#[tokio::test]
async fn test_p12_close_operation_not_found_journal_stays_uncertain() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use crate::paired_host::client::{ClientError, Operation};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p12_onf.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let local_close_called = Arc::new(AtomicBool::new(false));
    let local_close_clone = local_close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let local_close = local_close_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { .. } => {
                            DaemonResponse::PairedTerminalDescriptorOk {
                                descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                    host_id: "host-p12-onf".into(),
                                    generation: Epoch(1),
                                    target: m::RemoteTerminalTarget {
                                        machine_id: "m-1".into(),
                                        daemon_epoch: Epoch(1),
                                        session_id: "remote-pty-onf".into(),
                                    },
                                    after_sequence: None,
                                }),
                            }
                        }
                        DaemonRequest::PairedHostOperation { request } => {
                            match request.operation {
                                // Ambiguous close: journal must be consulted.
                                Operation::CloseSession { .. } => {
                                    DaemonResponse::PairedHostOperationError {
                                        error: ClientError {
                                            code: "TIMEOUT".into(),
                                            machine_error: None,
                                            ambiguous: true,
                                            request_id: Some("req-p12-onf".into()),
                                        },
                                    }
                                }
                                // P12 (round 2): journal OPERATION_NOT_FOUND means the
                                // close request never committed remotely — the remote
                                // session may still be running. NOT proof of closure.
                                Operation::Operation { .. } => {
                                    DaemonResponse::PairedHostOperationError {
                                        error: ClientError {
                                            code: "OPERATION_NOT_FOUND".into(),
                                            machine_error: None,
                                            ambiguous: false,
                                            request_id: Some("req-p12-onf".into()),
                                        },
                                    }
                                }
                                _ => DaemonResponse::Error {
                                    message: "unexpected op".into(),
                                    code: None,
                                    details: None,
                                },
                            }
                        }
                        DaemonRequest::Close { .. } => {
                            local_close.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let err = client
        .close_terminal("daemon-session:paired-p12-onf")
        .await
        .expect_err("An uncommitted close must stay uncertain");

    assert_eq!(
        err.code,
        IpcErrorCode::from_code_str("REMOTE_CLOSE_UNCERTAIN")
    );
    let details = err.details.expect("details present");
    assert_eq!(
        details.get("remoteSessionId").and_then(|v| v.as_str()),
        Some("remote-pty-onf")
    );
    assert!(
        !local_close_called.load(Ordering::SeqCst),
        "Journal OPERATION_NOT_FOUND must not mask a possibly-live remote PTY"
    );

    server.abort();
}

#[tokio::test]
async fn test_p12_close_ambiguous_remote_error_exhausts_and_aborts_local_close() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use crate::paired_host::client::{ClientError, Operation, OperationResponse, OperationResult};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p12_amb.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let local_close_called = Arc::new(AtomicBool::new(false));
    let local_close_clone = local_close_called.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let local_close = local_close_clone.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { .. } => {
                            DaemonResponse::PairedTerminalDescriptorOk {
                                descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                    host_id: "host-p12".into(),
                                    generation: Epoch(1),
                                    target: m::RemoteTerminalTarget {
                                        machine_id: "m-1".into(),
                                        daemon_epoch: Epoch(1),
                                        session_id: "remote-pty-amb".into(),
                                    },
                                    after_sequence: None,
                                }),
                            }
                        }
                        DaemonRequest::PairedHostOperation { request } => match request.operation {
                            Operation::CloseSession { .. } => {
                                DaemonResponse::PairedHostOperationError {
                                    error: ClientError {
                                        code: "TIMEOUT".into(),
                                        machine_error: None,
                                        ambiguous: true,
                                        request_id: Some("req-p12-amb".into()),
                                    },
                                }
                            }
                            Operation::Operation { request_id } => {
                                DaemonResponse::PairedHostOperationOk {
                                    response: OperationResponse {
                                        host_id: "host-p12".into(),
                                        generation: Epoch(1),
                                        result: OperationResult::Operation(m::Operation::Pending {
                                            request_id,
                                        }),
                                    },
                                }
                            }
                            _ => DaemonResponse::Error {
                                message: "unexpected op".into(),
                                code: None,
                                details: None,
                            },
                        },
                        DaemonRequest::Close { .. } => {
                            local_close.store(true, Ordering::SeqCst);
                            DaemonResponse::CloseOk
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = DaemonClient::new_with_socket(socket);
    let err = client
        .close_terminal("daemon-session:paired-p12-amb")
        .await
        .expect_err(
            "Ambiguous remote close with pending journal must cause close_terminal to fail",
        );

    assert_eq!(
        err.code,
        IpcErrorCode::from_code_str("REMOTE_CLOSE_UNCERTAIN")
    );
    let details = err.details.expect("Error details must be present");
    assert_eq!(
        details.get("remoteCloseUnknown").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        details.get("hostId").and_then(|v| v.as_str()),
        Some("host-p12")
    );
    assert_eq!(
        details.get("remoteSessionId").and_then(|v| v.as_str()),
        Some("remote-pty-amb")
    );
    assert!(details.get("cleanupRequestId").is_some());
    assert!(
        !local_close_called.load(Ordering::SeqCst),
        "Local close must NOT proceed on uncertain remote close"
    );

    server.abort();
}

#[tokio::test]
async fn test_p13_attach_routes_through_descriptor_and_reinstalls_proxy() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p13_reattach.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let descriptor_queried = Arc::new(AtomicBool::new(false));
    let reattach_called = Arc::new(AtomicBool::new(false));
    let proxy_attached = Arc::new(AtomicBool::new(false));

    let descriptor_clone = descriptor_queried.clone();
    let reattach_clone = reattach_called.clone();
    let proxy_attached_clone = proxy_attached.clone();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let descriptor_queried = descriptor_clone.clone();
            let reattach_called = reattach_clone.clone();
            let proxy_attached = proxy_attached_clone.clone();

            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { session_id } => {
                            if session_id == "daemon-session:orig-p13" {
                                descriptor_queried.store(true, Ordering::SeqCst);
                                DaemonResponse::PairedTerminalDescriptorOk {
                                    descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                        host_id: "host-p13".into(),
                                        generation: Epoch(1),
                                        target: m::RemoteTerminalTarget {
                                            machine_id: "m-p13".into(),
                                            daemon_epoch: Epoch(1),
                                            session_id: "remote-pty-p13".into(),
                                        },
                                        after_sequence: None,
                                    }),
                                }
                            } else {
                                DaemonResponse::PairedTerminalDescriptorOk { descriptor: None }
                            }
                        }
                        DaemonRequest::PairedTerminalReattach { descriptor } => {
                            assert_eq!(descriptor.host_id, "host-p13");
                            assert_eq!(descriptor.after_sequence, Some(Epoch(42)));
                            reattach_called.store(true, Ordering::SeqCst);
                            DaemonResponse::PairedTerminalReattachOk {
                                session_id: "daemon-session:reinstalled-p13-proxy".into(),
                                generation: Epoch(2),
                            }
                        }
                        DaemonRequest::Attach {
                            session_id,
                            after_sequence,
                        } => {
                            if session_id == "daemon-session:reinstalled-p13-proxy" {
                                assert_eq!(after_sequence, Some(42));
                                proxy_attached.store(true, Ordering::SeqCst);
                                DaemonResponse::AttachOk {
                                    epoch: 2,
                                    session_id: "daemon-session:reinstalled-p13-proxy".into(),
                                    start_sequence: Some(42),
                                    end_sequence: Some(43),
                                    gap: None,
                                    history: bytes::Bytes::from(b"p13 history line\n".to_vec()),
                                    pty_cols: Some(80),
                                    pty_rows: Some(24),
                                    history_segments: vec![],
                                    remote_generation: None,
                                }
                            } else {
                                DaemonResponse::Error {
                                    message: format!("Session '{session_id}' not found"),
                                    code: Some("SESSION_NOT_FOUND".into()),
                                    details: None,
                                }
                            }
                        }
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let app = tauri::test::mock_builder()
        .manage(client.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let client_state = app.state::<Arc<DaemonClient>>();

    let attach_res = cmd_terminal_attach(
        app.handle().clone(),
        client_state,
        "daemon-session:orig-p13".into(),
        Some("42".into()),
    )
    .await
    .expect("attach of paired session must succeed through reinstalled proxy");

    assert_eq!(
        attach_res.session_id,
        "daemon-session:reinstalled-p13-proxy"
    );
    assert!(
        descriptor_queried.load(Ordering::SeqCst),
        "descriptor must be queried"
    );
    assert!(
        reattach_called.load(Ordering::SeqCst),
        "reattach must be called"
    );
    assert!(
        proxy_attached.load(Ordering::SeqCst),
        "proxy session must be attached"
    );

    server.abort();
}

#[tokio::test]
async fn test_p13_attach_preserves_ambiguous_reattach_failure() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use crate::paired_host::client::ClientError;
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p13_amb.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { session_id } => {
                            if session_id == "daemon-session:orig-p13-amb" {
                                DaemonResponse::PairedTerminalDescriptorOk {
                                    descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                        host_id: "host-p13".into(),
                                        generation: Epoch(1),
                                        target: m::RemoteTerminalTarget {
                                            machine_id: "m-p13".into(),
                                            daemon_epoch: Epoch(1),
                                            session_id: "remote-pty-amb".into(),
                                        },
                                        after_sequence: None,
                                    }),
                                }
                            } else {
                                DaemonResponse::PairedTerminalDescriptorOk { descriptor: None }
                            }
                        }
                        DaemonRequest::PairedTerminalReattach { .. } => {
                            DaemonResponse::PairedHostOperationError {
                                error: ClientError {
                                    code: "TIMEOUT".into(),
                                    machine_error: None,
                                    request_id: Some("req-p13-amb".into()),
                                    ambiguous: true,
                                },
                            }
                        }
                        DaemonRequest::Attach { session_id, .. } => DaemonResponse::Error {
                            message: format!("Session '{session_id}' not found"),
                            code: Some("SESSION_NOT_FOUND".into()),
                            details: None,
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let app = tauri::test::mock_builder()
        .manage(client.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let client_state = app.state::<Arc<DaemonClient>>();

    let err = cmd_terminal_attach(
        app.handle().clone(),
        client_state,
        "daemon-session:orig-p13-amb".into(),
        None,
    )
    .await
    .expect_err("ambiguous reattach must fail");

    assert_ne!(
        err.code,
        IpcErrorCode::SessionNotFound,
        "Ambiguous reattach must NOT report remote death (SESSION_NOT_FOUND)"
    );
    assert_eq!(err.code, IpcErrorCode::Timeout);
    let details = err
        .details
        .expect("Structured error details must be present");
    assert_eq!(
        details.get("ambiguous").and_then(|v| v.as_bool()),
        Some(true)
    );

    server.abort();
}

#[tokio::test]
async fn test_p13_attach_preserves_not_found_when_descriptor_none() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p13_none.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { .. } => {
                            DaemonResponse::PairedTerminalDescriptorOk { descriptor: None }
                        }
                        DaemonRequest::Attach { session_id, .. } => DaemonResponse::Error {
                            message: format!("Session '{session_id}' not found"),
                            code: Some("SESSION_NOT_FOUND".into()),
                            details: None,
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let app = tauri::test::mock_builder()
        .manage(client.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let client_state = app.state::<Arc<DaemonClient>>();

    let err = cmd_terminal_attach(
        app.handle().clone(),
        client_state,
        "daemon-session:unknown-p13".into(),
        None,
    )
    .await
    .expect_err("descriptor None must preserve not-found behavior");

    assert_eq!(err.code, IpcErrorCode::SessionNotFound);

    server.abort();
}

#[tokio::test]
async fn test_p13_attach_hub_absence_returns_proxy_pending_unknown() {
    use crate::daemon::protocol::{DaemonRequest, DaemonResponse, DAEMON_PROTOCOL_VERSION};
    use crate::ipc::IpcErrorCode;
    use crate::remote::machine_protocol as m;
    use crate::scoped_contracts::Epoch;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("p13_hub_absent.sock");
    let listener = UnixListener::bind(&socket).unwrap();

    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let req: DaemonRequest = match serde_json::from_str(line.trim()) {
                        Ok(r) => r,
                        Err(_) => {
                            line.clear();
                            continue;
                        }
                    };
                    line.clear();
                    let resp = match req {
                        DaemonRequest::Handshake { .. } => DaemonResponse::HandshakeOk {
                            version: DAEMON_PROTOCOL_VERSION,
                            pid: std::process::id(),
                            epoch: 1,
                            binary_path: None,
                            binary_mtime_ms: None,
                            daemon_version: None,
                        },
                        DaemonRequest::GetCapabilities => DaemonResponse::CapabilitiesOk {
                            capabilities: vec!["pairedHostInventoryV1".into()],
                        },
                        DaemonRequest::PairedTerminalDescriptor { session_id } => {
                            if session_id == "daemon-session:orig-hub-absent" {
                                DaemonResponse::PairedTerminalDescriptorOk {
                                    descriptor: Some(crate::terminal::paired_daemon::Descriptor {
                                        host_id: "host-p13".into(),
                                        generation: Epoch(1),
                                        target: m::RemoteTerminalTarget {
                                            machine_id: "m-p13".into(),
                                            daemon_epoch: Epoch(1),
                                            session_id: "remote-pty-hub-absent".into(),
                                        },
                                        after_sequence: None,
                                    }),
                                }
                            } else {
                                DaemonResponse::PairedTerminalDescriptorOk { descriptor: None }
                            }
                        }
                        DaemonRequest::PairedTerminalReattach { .. } => {
                            DaemonResponse::PairedTerminalReattachOk {
                                session_id: "daemon-session:proxy-hub-absent".into(),
                                generation: Epoch(1),
                            }
                        }
                        DaemonRequest::Attach { session_id, .. } => DaemonResponse::Error {
                            message: format!("Session '{session_id}' not found"),
                            code: Some("SESSION_NOT_FOUND".into()),
                            details: None,
                        },
                        _ => DaemonResponse::Error {
                            message: "unexpected req".into(),
                            code: None,
                            details: None,
                        },
                    };
                    let bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write.write_all(&bytes).await;
                    let _ = write.write_all(b"\n").await;
                    let _ = write.flush().await;
                }
            });
        }
    });

    let client = Arc::new(DaemonClient::new_with_socket(socket));
    let app = tauri::test::mock_builder()
        .manage(client.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let client_state = app.state::<Arc<DaemonClient>>();

    std::env::set_var("FERRYX_TEST_HUB_ATTACH_TIMEOUT_MS", "200");

    let err = cmd_terminal_attach(
        app.handle().clone(),
        client_state,
        "daemon-session:orig-hub-absent".into(),
        None,
    )
    .await
    .expect_err("hub absence must return non-terminal error");

    assert_ne!(
        err.code,
        IpcErrorCode::SessionNotFound,
        "Hub absence must NOT return SESSION_NOT_FOUND"
    );
    let details = err.details.expect("Error details must be present");
    assert_eq!(
        details.get("pairedProxyPending").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        details.get("sessionId").and_then(|v| v.as_str()),
        Some("daemon-session:orig-hub-absent")
    );

    server.abort();
}
