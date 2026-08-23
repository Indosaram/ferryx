use ferryx_lib::terminal::PtyManager;
use ferryx_lib::worktree::{CreateWorktreeOptions, WorktreeError, WorktreeManager};
use portable_pty::CommandBuilder;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

fn setup_test_repo() -> (TempDir, WorktreeManager) {
    let temp_dir = TempDir::new().expect("temp dir");
    let repo_path = temp_dir.path();
    ferryx_lib::worktree::run_git(repo_path, &["init"]).expect("git init");
    ferryx_lib::worktree::run_git(repo_path, &["config", "user.name", "Orca Test"])
        .expect("git user name");
    ferryx_lib::worktree::run_git(repo_path, &["config", "user.email", "test@orca.dev"])
        .expect("git user email");
    fs::write(repo_path.join("README.md"), "# base\n").expect("write base");
    ferryx_lib::worktree::run_git(repo_path, &["add", "README.md"]).expect("git add");
    ferryx_lib::worktree::run_git(repo_path, &["commit", "-m", "base"]).expect("git commit");
    let manager = WorktreeManager::new(repo_path);
    (temp_dir, manager)
}

fn create_worktree(manager: &WorktreeManager, slug: &str) -> std::path::PathBuf {
    let path = manager.repo_root().join(format!("wt-{slug}"));
    manager
        .create_worktree(CreateWorktreeOptions::new("ws-safety", slug, &path))
        .expect("create worktree");
    path
}

#[test]
fn second_writer_is_rejected_and_release_allows_reacquire() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "lease");

    manager
        .acquire_writer(&wt, "owner-a")
        .expect("first writer");
    assert_eq!(
        manager.writer_owner(&wt).expect("writer owner"),
        Some("owner-a".into())
    );

    let err = manager.acquire_writer(&wt, "owner-b").unwrap_err();
    assert!(matches!(err, WorktreeError::WriterAlreadyActive { .. }));

    manager
        .release_writer(&wt, "owner-a")
        .expect("release owner-a");
    manager
        .acquire_writer(&wt, "owner-b")
        .expect("owner-b reacquire");
    assert_eq!(
        manager.writer_owner(&wt).expect("writer owner"),
        Some("owner-b".into())
    );
    manager
        .release_writer(&wt, "owner-b")
        .expect("release owner-b");
}

#[test]
fn active_writer_blocks_delete() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "active-writer");

    manager
        .acquire_writer(&wt, "agent-1")
        .expect("writer acquire");
    let err = manager.safe_delete(&wt).unwrap_err();
    assert!(matches!(err, WorktreeError::WriterAlreadyActive { .. }));
    assert!(wt.exists(), "active writer worktree must remain");
    manager
        .release_writer(&wt, "agent-1")
        .expect("writer release");
}

#[test]
fn default_delete_rejects_clean_unmerged_branch() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "unmerged-default");

    fs::write(wt.join("feature.txt"), "unmerged work\n").expect("write feature");
    ferryx_lib::worktree::run_git(&wt, &["add", "feature.txt"]).expect("git add");
    ferryx_lib::worktree::run_git(&wt, &["commit", "-m", "unmerged feature"])
        .expect("feature commit");
    assert!(!manager.is_dirty(&wt).expect("clean status"));

    let err = manager.delete_worktree_and_branch(&wt, true).unwrap_err();
    assert!(matches!(err, WorktreeError::UnmergedBranch { .. }));
    assert!(
        wt.exists(),
        "default delete must preserve unmerged worktree"
    );
    let branches = ferryx_lib::worktree::run_git(manager.repo_root(), &["branch", "--list"])
        .expect("list branches");
    assert!(branches.contains("orca/ws-safety/unmerged-default"));
}

#[test]
fn explicit_destructive_delete_removes_clean_unmerged_branch() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "unmerged-force");

    fs::write(wt.join("feature.txt"), "unmerged work\n").expect("write feature");
    ferryx_lib::worktree::run_git(&wt, &["add", "feature.txt"]).expect("git add");
    ferryx_lib::worktree::run_git(&wt, &["commit", "-m", "unmerged feature"])
        .expect("feature commit");

    let preview = manager
        .branch_deletion_preview(&wt)
        .expect("destructive preview");
    assert_eq!(preview.branch, "orca/ws-safety/unmerged-force");
    assert!(!preview.head.is_empty());
    assert!(!preview.merged);

    manager
        .delete_worktree_and_branch_destructive(&wt, true)
        .expect("explicit destructive delete");
    assert!(!wt.exists());
    let branches = ferryx_lib::worktree::run_git(manager.repo_root(), &["branch", "--list"])
        .expect("list branches");
    assert!(!branches.contains("orca/ws-safety/unmerged-force"));
}

#[tokio::test]
async fn same_worktree_supports_multiple_interactive_pty_sessions() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "multi-pty");
    let canonical = fs::canonicalize(&wt).expect("canonical worktree");
    let pty = PtyManager::new();

    let mut first_shell = CommandBuilder::new("/bin/sh");
    first_shell.cwd(&wt);
    let (first_id, _first_rx) = pty
        .spawn_in_worktree(first_shell, 80, 24, &manager, &wt)
        .expect("spawn first terminal");

    let mut second_shell = CommandBuilder::new("/bin/sh");
    second_shell.cwd(&wt);
    let (second_id, _second_rx) = pty
        .spawn_in_worktree(second_shell, 80, 24, &manager, &wt)
        .expect("spawn second terminal in same worktree");

    assert_ne!(first_id, second_id);
    assert_eq!(pty.session_count(), 2);
    assert_eq!(manager.writer_owner(&wt).expect("writer owner"), None);
    assert_eq!(
        pty.get_session(&first_id)
            .and_then(|session| session.worktree_path()),
        Some(canonical.clone())
    );
    assert_eq!(
        pty.get_session(&second_id)
            .and_then(|session| session.worktree_path()),
        Some(canonical)
    );

    pty.close_session(&first_id)
        .await
        .expect("close first terminal");
    assert!(
        pty.has_session(&second_id),
        "closing one split PTY must not close its sibling"
    );
    pty.close_session(&second_id)
        .await
        .expect("close second terminal");
    assert_eq!(pty.session_count(), 0);
}

#[tokio::test]
async fn pty_worktree_ownership_clears_on_close_and_natural_exit() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "pty-ownership");
    let pty = PtyManager::new();

    let mut shell = CommandBuilder::new("/bin/sh");
    shell.cwd(&wt);
    let (session_id, _rx) = pty
        .spawn_in_worktree(shell, 80, 24, &manager, &wt)
        .expect("spawn terminal");
    assert!(pty
        .get_session(&session_id)
        .and_then(|session| session.worktree_path())
        .is_some());
    assert_eq!(manager.writer_owner(&wt).expect("writer owner"), None);
    pty.close_session(&session_id)
        .await
        .expect("close terminal");
    assert!(!pty.has_session(&session_id));

    let mut one_shot = CommandBuilder::new("/bin/sh");
    one_shot.arg("-c");
    one_shot.arg("exit 0");
    one_shot.cwd(&wt);
    let (natural_id, mut output) = pty
        .spawn_in_worktree(one_shot, 80, 24, &manager, &wt)
        .expect("spawn one-shot terminal");

    tokio::time::timeout(Duration::from_secs(5), async {
        while output.recv().await.is_some() {}
    })
    .await
    .expect("natural session output closure");
    assert!(!pty.has_session(&natural_id));
    assert_eq!(manager.writer_owner(&wt).expect("writer owner"), None);
}

#[test]
fn pty_spawn_failure_does_not_claim_exclusive_writer() {
    let (_repo, manager) = setup_test_repo();
    let wt = create_worktree(&manager, "pty-spawn-failure");
    let pty = PtyManager::new();

    let mut missing = CommandBuilder::new("/definitely/missing/orca-command");
    missing.cwd(&wt);
    let error = pty
        .spawn_in_worktree(missing, 80, 24, &manager, &wt)
        .expect_err("missing program must fail to spawn");
    assert!(error.to_string().contains("spawn"));
    assert_eq!(manager.writer_owner(&wt).expect("writer owner"), None);
}

#[tokio::test]
async fn five_concurrent_worktree_terminal_lifecycles_remain_isolated() {
    let (_repo, manager) = setup_test_repo();
    let pty = PtyManager::new();
    let worktrees = (0..5)
        .map(|index| create_worktree(&manager, &format!("concurrent-{index}")))
        .collect::<Vec<_>>();
    let mut session_ids = Vec::with_capacity(worktrees.len());
    let mut receivers = Vec::with_capacity(worktrees.len());

    for worktree in &worktrees {
        let mut shell = CommandBuilder::new("/bin/sh");
        shell.cwd(worktree);
        let (session_id, receiver) = pty
            .spawn_in_worktree(shell, 80, 24, &manager, worktree)
            .expect("spawn terminal");
        assert_eq!(manager.writer_owner(worktree).expect("writer owner"), None);
        session_ids.push(session_id);
        receivers.push(receiver);
    }

    assert_eq!(pty.session_count(), worktrees.len());

    for session_id in &session_ids {
        pty.close_session(session_id).await.expect("close terminal");
    }

    assert_eq!(pty.session_count(), 0);
    for worktree in &worktrees {
        assert_eq!(manager.writer_owner(worktree).expect("writer owner"), None);
        manager
            .delete_worktree_and_branch(worktree, true)
            .expect("delete clean worktree and merged branch");
    }

    assert_eq!(
        manager.list_worktrees().expect("remaining worktrees").len(),
        1
    );
    drop(receivers);
}
