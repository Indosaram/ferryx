use crate::terminal::PtyManager;
use crate::worktree::WorktreeManager;
use crate::ipc::*;
use std::sync::Arc;
use tempfile::tempdir;
use tauri::{Listener, Manager};

#[tokio::test]
async fn test_tauri_mock_app_terminal_events() {
    let app = tauri::test::mock_builder()
        .manage(Arc::new(PtyManager::new()))
        .manage(WorktreeManager::new("."))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let pty_manager = app.state::<Arc<PtyManager>>();
    let (tx, mut rx_event) = tokio::sync::mpsc::channel::<TerminalOutputPayload>(10);

    let session_id = cmd_terminal_spawn(
        app.handle().clone(),
        pty_manager.clone(),
        Some(SpawnTerminalRequest {
            cwd: None,
            cols: Some(80),
            rows: Some(24),
            command: Some("echo hello_orca_terminal".into()),
        }),
    )
    .await
    .expect("cmd_terminal_spawn failed");

    let event_name = format!("terminal_output:{}", session_id);
    let tx_clone = tx.clone();
    app.listen(&event_name, move |event: tauri::Event| {
        if let Ok(payload) = serde_json::from_str::<TerminalOutputPayload>(event.payload()) {
            let _ = tx_clone.try_send(payload);
        }
    });

    // Wait for the event output
    let mut collected = String::new();
    let timeout = tokio::time::Duration::from_secs(5);
    let start = tokio::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(Some(payload)) = tokio::time::timeout(tokio::time::Duration::from_millis(500), rx_event.recv()).await {
            collected.push_str(&payload.data);
            if collected.contains("hello_orca_terminal") {
                break;
            }
        }
    }

    assert!(
        collected.contains("hello_orca_terminal"),
        "Expected output to contain 'hello_orca_terminal', got: {}",
        collected
    );

    // Test resize, write, list, close
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

    // Test cmd_worktree_list
    let list = cmd_worktree_list(wt_manager.clone(), None)
        .await
        .expect("list failed");
    assert_eq!(list.len(), 1);

    // Test cmd_worktree_create
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
    assert_eq!(created.path.canonicalize().unwrap(), wt_path.canonicalize().unwrap());

    // Test cmd_worktree_status
    let status = cmd_worktree_status(wt_manager.clone(), wt_path.clone())
        .await
        .expect("status failed");
    assert!(!status.is_dirty);

    // Test cmd_worktree_delete
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

    pty_manager.kill(&session_id).unwrap();
    pty_manager.remove_session(&session_id);
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
    assert_eq!(created.path.canonicalize().unwrap(), wt_path.canonicalize().unwrap());

    let list_after = manager.list_worktrees().unwrap();
    assert_eq!(list_after.len(), 2);

    manager.delete_worktree_and_branch(&wt_path, true).unwrap();
    let list_final = manager.list_worktrees().unwrap();
    assert_eq!(list_final.len(), 1);
}
