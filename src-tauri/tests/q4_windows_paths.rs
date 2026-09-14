#![cfg(windows)]
use ferryx_lib::worktree::WorktreeManager;

#[test]
fn git_inventory_and_manager_share_native_identity() {
    let root = tempfile::tempdir().unwrap();
    ferryx_lib::worktree::run_git(root.path(), &["init", "--quiet"]).unwrap();
    ferryx_lib::worktree::run_git(root.path(), &["-c", "user.name=Q4", "-c", "user.email=q4@example.invalid", "commit", "--allow-empty", "-m", "base"]).unwrap();
    let manager = WorktreeManager::new(root.path());
    let rows = ferryx_lib::worktree::git_worktree_list(manager.repo_root()).unwrap();
    assert_eq!(rows[0].path, manager.repo_root(), "native Git inventory identity");
    root.close().unwrap();
}
