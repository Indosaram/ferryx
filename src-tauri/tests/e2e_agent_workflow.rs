use ferryx_lib::terminal::PtyManager;
use ferryx_lib::worktree::model::CreateWorktreeOptions;
use ferryx_lib::worktree::WorktreeManager;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::time::Duration;

#[tokio::test]
async fn test_e2e_agent_worktree_and_terminal_lifecycle() {
    let temp = tempdir().unwrap();
    let repo_path = temp.path().canonicalize().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::process::Command::new("git")
        .args(["config", "user.email", "agent@orca.local"])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Orca Agent"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    std::fs::write(repo_path.join("main.txt"), "base repository content\n").unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&repo_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "chore: base commit"])
        .current_dir(&repo_path)
        .output()
        .unwrap();

    let worktree_mgr = WorktreeManager::new(&repo_path);
    let pty_mgr = Arc::new(PtyManager::new());

    let agent_wt_path = repo_path.join("wt_agent_task");
    let opts = CreateWorktreeOptions::new("agent1", "task-code", &agent_wt_path);
    let created_wt = worktree_mgr.create_worktree(opts).unwrap();
    assert_eq!(
        created_wt.path.canonicalize().unwrap(),
        agent_wt_path.canonicalize().unwrap()
    );

    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.cwd(&agent_wt_path);

    let (session_id, mut rx) = pty_mgr
        .spawn_in_worktree(cmd, 80, 24, &worktree_mgr, &agent_wt_path)
        .unwrap();
    assert!(pty_mgr.has_session(&session_id));

    pty_mgr
        .write_input(
            &session_id,
            b"echo 'agent work done' > task.out; printf '__ORCA_AGENT_%s__\\n' DONE\n",
        )
        .unwrap();

    let mut captured_output = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&captured_output).contains("__ORCA_AGENT_DONE__") {
            let chunk = rx
                .recv()
                .await
                .expect("terminal output before completion marker");
            captured_output.extend_from_slice(&chunk);
        }
    })
    .await
    .expect("agent completion marker timeout");

    assert!(
        agent_wt_path.join("task.out").exists(),
        "Agent task file must exist"
    );
    let content = std::fs::read_to_string(agent_wt_path.join("task.out")).unwrap();
    assert!(content.contains("agent work done"));

    let dirty = worktree_mgr.check_dirty(&agent_wt_path).unwrap();
    assert!(
        dirty.is_dirty,
        "Untracked task.out must make worktree dirty"
    );

    let delete_dirty_err = worktree_mgr.remove_worktree(&agent_wt_path, false);
    assert!(
        delete_dirty_err.is_err(),
        "Must refuse to delete dirty worktree"
    );

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&agent_wt_path)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "feat: complete agent task"])
        .current_dir(&agent_wt_path)
        .output()
        .unwrap();

    let dirty_after = worktree_mgr.check_dirty(&agent_wt_path).unwrap();
    assert!(!dirty_after.is_dirty, "Worktree must be clean after commit");

    pty_mgr.close_session(&session_id).await.unwrap();
    assert!(!pty_mgr.has_session(&session_id));

    worktree_mgr
        .delete_worktree_and_branch_destructive(&agent_wt_path, true)
        .unwrap();

    let remaining_wts = worktree_mgr.list_worktrees().unwrap();
    assert_eq!(remaining_wts.len(), 1, "Only main worktree should remain");
}
