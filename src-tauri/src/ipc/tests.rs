use crate::ipc::*;
use crate::terminal::PtyManager;
use crate::worktree::WorktreeManager;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::sync::Arc;
use tauri::{Listener, Manager};
use tempfile::tempdir;

#[tokio::test]
async fn test_tauri_mock_app_terminal_events() {
    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(WorktreeManager::new("."))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let pty_manager = app.state::<Arc<PtyManager>>();
    let (tx, mut rx_event) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(10);
    let tx_clone = tx.clone();
    app.listen("terminal_output", move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = tx_clone.try_send(payload);
        }
    });

    let session_id = cmd_terminal_spawn(
        app.handle().clone(),
        pty_manager.clone(),
        Some(SpawnTerminalRequest {
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            command: None,
        }),
    )
    .await
    .expect("cmd_terminal_spawn failed");

    cmd_terminal_write(
        pty_manager.clone(),
        session_id.clone(),
        "echo hello_orca_terminal\n".into(),
    )
    .await
    .expect("write failed");

    let mut collected = Vec::new();
    let timeout = tokio::time::Duration::from_secs(5);
    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(Some(payload)) = tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            rx_event.recv(),
        )
        .await
        {
            assert_eq!(payload.session_id, session_id);
            collected.extend_from_slice(&STANDARD.decode(payload.data).expect("base64 output"));
            if String::from_utf8_lossy(&collected).contains("hello_orca_terminal") {
                break;
            }
        }
    }

    assert!(
        String::from_utf8_lossy(&collected).contains("hello_orca_terminal"),
        "Expected output to contain 'hello_orca_terminal', got: {}",
        String::from_utf8_lossy(&collected)
    );

    cmd_terminal_resize(pty_manager.clone(), session_id.clone(), 120, 40)
        .await
        .expect("resize failed");
    let sessions = cmd_terminal_list(pty_manager.clone())
        .await
        .expect("list failed");
    assert!(sessions.contains(&session_id));

    cmd_terminal_close(pty_manager.clone(), session_id.clone())
        .await
        .expect("close failed");
    cmd_terminal_close(pty_manager.clone(), session_id.clone())
        .await
        .expect("second close must be idempotent");
    let sessions_after = cmd_terminal_list(pty_manager.clone())
        .await
        .expect("list failed");
    assert!(!sessions_after.contains(&session_id));
}

#[tokio::test]
async fn test_tauri_mock_app_worktree_commands() {
    let temp = tempdir().unwrap();
    let repo_path = temp.path().canonicalize().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::fs::write(repo_path.join("README.md"), "initial").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(WorktreeManager::new(&repo_path))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let wt_manager = app.state::<WorktreeManager>();

    let list = cmd_worktree_list(wt_manager.clone(), None)
        .await
        .expect("list failed");
    assert_eq!(list.len(), 1);

    let wt_path = repo_path.join("wt_ipc_test");
    let created = cmd_worktree_create(
        wt_manager.clone(),
        CreateWorktreeRequest {
            repo_root: None,
            ws_id: "ws-ipc".to_string(),
            slug: "task-ipc".to_string(),
            path: wt_path.clone(),
            base_ref: None,
        },
    )
    .await
    .expect("create failed");
    assert_eq!(
        created.path.canonicalize().unwrap(),
        wt_path.canonicalize().unwrap()
    );

    let status = cmd_worktree_status(wt_manager.clone(), wt_path.clone())
        .await
        .expect("status failed");
    assert!(!status.is_dirty);

    cmd_worktree_delete(
        wt_manager.clone(),
        DeleteWorktreeRequest {
            repo_root: None,
            path: wt_path.clone(),
            delete_branch: Some(true),
        },
    )
    .await
    .expect("delete failed");

    let list_after = cmd_worktree_list(wt_manager.clone(), None)
        .await
        .expect("list failed");
    assert_eq!(list_after.len(), 1);
}

#[tokio::test]
async fn test_ipc_terminal_lifecycle() {
    let pty_manager = Arc::new(PtyManager::new());
    assert_eq!(pty_manager.list_sessions().len(), 0);

    let (session_id, _rx) = pty_manager.spawn_shell(80, 24).unwrap();
    assert!(pty_manager.has_session(&session_id));
    assert_eq!(pty_manager.list_sessions().len(), 1);

    pty_manager.write_input(&session_id, b"echo test\n").unwrap();
    pty_manager.resize(&session_id, 100, 30).unwrap();
    let session = pty_manager.get_session(&session_id).unwrap();
    assert_eq!(session.get_size(), (100, 30));

    pty_manager.close_session(&session_id).await.unwrap();
    pty_manager.close_session(&session_id).await.unwrap();
    assert!(!pty_manager.has_session(&session_id));
}

#[tokio::test]
async fn test_ipc_worktree_lifecycle() {
    let temp = tempdir().unwrap();
    let repo_path = temp.path().canonicalize().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::fs::write(repo_path.join("README.md"), "initial").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let manager = WorktreeManager::new(&repo_path);
    let initial_list = manager.list_worktrees().unwrap();
    assert_eq!(initial_list.len(), 1);

    let wt_path = repo_path.join("wt_feat");
    let opts = crate::worktree::model::CreateWorktreeOptions::new("ws1", "feat", &wt_path);
    let created = manager.create_worktree(opts).unwrap();
    assert_eq!(
        created.path.canonicalize().unwrap(),
        wt_path.canonicalize().unwrap()
    );

    let list_after = manager.list_worktrees().unwrap();
    assert_eq!(list_after.len(), 2);

    manager.delete_worktree_and_branch(&wt_path, true).unwrap();
    let list_final = manager.list_worktrees().unwrap();
    assert_eq!(list_final.len(), 1);
}

#[tokio::test]
async fn terminal_global_events_preserve_raw_bytes_and_lifecycle() {
    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(WorktreeManager::new("."))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let pty_manager = app.state::<Arc<PtyManager>>();
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(32);
    let (lifecycle_tx, mut lifecycle_rx) =
        tokio::sync::mpsc::channel::<TerminalLifecyclePayload>(8);

    app.listen("terminal_output", move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = output_tx.try_send(payload);
        }
    });
    app.listen("terminal_lifecycle", move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalLifecyclePayload>(event.payload()) {
            let _ = lifecycle_tx.try_send(payload);
        }
    });

    let session_id = cmd_terminal_spawn(
        app.handle().clone(),
        pty_manager.clone(),
        Some(SpawnTerminalRequest {
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            command: None,
        }),
    )
    .await
    .expect("spawn");

    let started = tokio::time::timeout(tokio::time::Duration::from_secs(2), lifecycle_rx.recv())
        .await
        .expect("started lifecycle timeout")
        .expect("started lifecycle channel");
    assert_eq!(started.session_id, session_id);
    assert_eq!(started.state, TerminalLifecycleState::Started);
    assert_eq!(started.exit_code, None);
    assert_eq!(started.reason, None);

    cmd_terminal_write(
        pty_manager.clone(),
        session_id.clone(),
        "printf '\\377\\376\\n'\n".into(),
    )
    .await
    .expect("write raw-byte command");

    let mut raw = Vec::new();
    tokio::time::timeout(tokio::time::Duration::from_secs(5), async {
        while !raw.windows(2).any(|window| window == [0xff, 0xfe]) {
            let payload = output_rx.recv().await.expect("terminal_output channel");
            assert_eq!(payload.session_id, session_id);
            raw.extend_from_slice(&STANDARD.decode(payload.data).expect("base64 payload"));
        }
    })
    .await
    .expect("raw terminal output timeout");

    cmd_terminal_close(pty_manager.clone(), session_id.clone())
        .await
        .expect("close");

    let exited = tokio::time::timeout(tokio::time::Duration::from_secs(2), lifecycle_rx.recv())
        .await
        .expect("exited lifecycle timeout")
        .expect("exited lifecycle channel");
    assert_eq!(exited.session_id, session_id);
    assert_eq!(exited.state, TerminalLifecycleState::Exited);
    assert!(exited.reason.is_none());
}
