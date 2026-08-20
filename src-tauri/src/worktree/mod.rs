pub mod git;
pub mod manager;
pub mod model;

pub use git::*;
pub use manager::*;
pub use model::*;

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// Helper to initialize a clean git repository for testing.
    fn setup_test_repo() -> (TempDir, WorktreeManager) {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let repo_path = temp_dir.path();

        // Initialize git repo
        git::run_git(repo_path, &["init"]).expect("git init failed");
        git::run_git(repo_path, &["config", "user.name", "Orca Test"])
            .expect("git config user.name failed");
        git::run_git(repo_path, &["config", "user.email", "test@orca.dev"])
            .expect("git config user.email failed");

        // Create initial commit
        let init_file = repo_path.join("README.md");
        fs::write(&init_file, "# Test Repo\n").expect("failed to write README.md");
        git::run_git(repo_path, &["add", "README.md"]).expect("git add failed");
        git::run_git(repo_path, &["commit", "-m", "Initial commit"]).expect("git commit failed");

        let manager = WorktreeManager::new(repo_path);
        (temp_dir, manager)
    }

    #[test]
    fn test_branch_namespace_formatting_and_validation() {
        // Valid namespaces
        assert_eq!(
            WorktreeManager::format_branch_name("ws-123", "task-a").unwrap(),
            "orca/ws-123/task-a"
        );
        assert_eq!(
            WorktreeManager::format_branch_name("session_1", "feature/subtask").unwrap(),
            "orca/session_1/feature/subtask"
        );

        // Invalid namespaces
        assert!(WorktreeManager::format_branch_name("", "slug").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "").is_err());
        assert!(WorktreeManager::format_branch_name("ws..1", "slug").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug with space").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug:name").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug~1").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug.lock").is_err());

        // Parse branch name
        let info = WorktreeManager::parse_branch_name("refs/heads/orca/ws-abc/my-task").unwrap();
        assert_eq!(info.ws_id, "ws-abc");
        assert_eq!(info.slug, "my-task");

        let info2 = WorktreeManager::parse_branch_name("orca/ws-456/nested/task").unwrap();
        assert_eq!(info2.ws_id, "ws-456");
        assert_eq!(info2.slug, "nested/task");

        assert!(WorktreeManager::parse_branch_name("refs/heads/main").is_none());
    }

    #[test]
    fn test_parse_worktree_list_porcelain() {
        let sample = r#"worktree /Users/orca/code/main
HEAD 0123456789abcdef0123456789abcdef01234567
branch refs/heads/master

worktree /Users/orca/code/wt-1
HEAD 1123456789abcdef0123456789abcdef01234567
branch refs/heads/orca/ws-1/task-1

worktree /Users/orca/code/wt-locked
HEAD 2123456789abcdef0123456789abcdef01234567
branch refs/heads/orca/ws-1/locked-task
locked maintenance in progress

worktree /Users/orca/code/wt-detached
HEAD 3123456789abcdef0123456789abcdef01234567
detached

worktree /Users/orca/code/wt-prunable
HEAD 4123456789abcdef0123456789abcdef01234567
prunable reason gitdir gone
"#;

        let worktrees = git::parse_worktree_list_porcelain(sample).expect("parse failed");
        assert_eq!(worktrees.len(), 5);

        assert_eq!(worktrees[0].path, Path::new("/Users/orca/code/main"));
        assert_eq!(
            worktrees[0].head,
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(
            worktrees[0].branch.as_deref(),
            Some("refs/heads/master")
        );
        assert_eq!(worktrees[0].branch_short_name(), Some("master"));

        assert_eq!(worktrees[1].path, Path::new("/Users/orca/code/wt-1"));
        let orca_info = worktrees[1].orca_info().unwrap();
        assert_eq!(orca_info.ws_id, "ws-1");
        assert_eq!(orca_info.slug, "task-1");

        assert_eq!(
            worktrees[2].locked.as_deref(),
            Some("maintenance in progress")
        );
        assert!(worktrees[3].detached);
        assert_eq!(
            worktrees[4].prunable.as_deref(),
            Some("reason gitdir gone")
        );
    }

    #[test]
    fn test_parse_status_porcelain_v1_and_v2() {
        // Clean
        let clean = git::parse_status_porcelain("");
        assert!(!clean.is_dirty);
        assert!(clean.files.is_empty());

        // Porcelain v1
        let v1_dirty = " M src/lib.rs\n?? new_file.txt\nD  deleted.rs\n";
        let state1 = git::parse_status_porcelain(v1_dirty);
        assert!(state1.is_dirty);
        assert_eq!(state1.files.len(), 3);
        assert_eq!(state1.files[0].path, "src/lib.rs");
        assert_eq!(state1.files[1].path, "new_file.txt");
        assert_eq!(state1.files[2].path, "deleted.rs");

        // Porcelain v2
        let v2_dirty = "1 .M N... 100644 100644 100644 123 123 src/main.rs\n? untracked.json\n# branch.head master\n";
        let state2 = git::parse_status_porcelain(v2_dirty);
        assert!(state2.is_dirty);
        assert_eq!(state2.files.len(), 2);
        assert_eq!(state2.files[0].path, "src/main.rs");
        assert_eq!(state2.files[1].path, "untracked.json");
    }

    #[test]
    fn test_create_and_list_worktree_on_fixture() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt_path = wt_parent.path().join("wt-feature");

        let wt = manager
            .create_worktree(CreateWorktreeOptions::new("ws-alpha", "feat-login", &wt_path))
            .expect("create worktree failed");

        assert_eq!(
            wt.branch_short_name(),
            Some("orca/ws-alpha/feat-login")
        );
        assert!(wt_path.join("README.md").exists());

        // Check list_worktrees
        let list = manager.list_worktrees().expect("list failed");
        assert_eq!(list.len(), 2);

        // Find worktree by path and slug
        let found = manager.find_worktree(&wt_path).expect("find failed").expect("should be found");
        assert_eq!(found.branch_short_name(), Some("orca/ws-alpha/feat-login"));

        let found_by_slug = manager
            .find_worktree_by_slug("ws-alpha", "feat-login")
            .expect("find by slug failed")
            .expect("should find by slug");
        assert_eq!(found_by_slug.path, found.path);

        // Initial state is clean
        assert!(!manager.is_dirty(&wt_path).expect("is_dirty failed"));
    }

    #[test]
    fn test_safe_delete_clean_worktree() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt_path = wt_parent.path().join("wt-clean");

        manager
            .create_worktree(CreateWorktreeOptions::new("ws-safe", "task-clean", &wt_path))
            .expect("create failed");
        assert!(wt_path.exists());

        // Safe delete clean worktree
        manager.safe_delete(&wt_path).expect("safe delete failed");
        assert!(!wt_path.exists());

        // Ensure removed from git worktree list
        let list = manager.list_worktrees().expect("list failed");
        assert_eq!(list.len(), 1);
        assert!(manager.find_worktree(&wt_path).unwrap().is_none());
    }

    #[test]
    fn test_dirty_rejection_on_untracked_and_modified_files() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt_path = wt_parent.path().join("wt-dirty");

        manager
            .create_worktree(CreateWorktreeOptions::new("ws-dirty", "task-dirty", &wt_path))
            .expect("create failed");

        // 1. Create untracked file
        let dirty_file = wt_path.join("scratchpad.txt");
        fs::write(&dirty_file, "temporary notes").expect("write scratchpad");

        // Verify detected as dirty
        let dirty_state = manager.check_dirty(&wt_path).expect("check_dirty failed");
        assert!(dirty_state.is_dirty);
        assert!(manager.is_dirty(&wt_path).expect("is_dirty failed"));

        // 2. Safe deletion MUST fail
        let err = manager.safe_delete(&wt_path).unwrap_err();
        match err {
            WorktreeError::DirtyWorktree { path, count, files } => {
                assert_eq!(path, wt_path);
                assert!(count >= 1);
                assert!(files.iter().any(|f| f.contains("scratchpad.txt")));
            }
            other => panic!("Expected DirtyWorktree error, got: {other:?}"),
        }

        // 3. Forced deletion MUST ALSO FAIL for dirty worktrees (1-writer-1-worktree isolation)
        let force_err = manager.remove_worktree(&wt_path, true).unwrap_err();
        match force_err {
            WorktreeError::DirtyWorktree { path, .. } => {
                assert_eq!(path, wt_path);
            }
            other => panic!("Expected DirtyWorktree error on force remove, got: {other:?}"),
        }

        // Worktree and untracked file must remain intact
        assert!(wt_path.exists());
        assert!(dirty_file.exists());

        // 4. Remove untracked file and modify tracked file
        fs::remove_file(&dirty_file).expect("remove scratchpad");
        let readme = wt_path.join("README.md");
        fs::write(&readme, "# Modified Content\n").expect("modify readme");

        assert!(manager.is_dirty(&wt_path).expect("is_dirty modified"));
        let err2 = manager.safe_delete(&wt_path).unwrap_err();
        match err2 {
            WorktreeError::DirtyWorktree { .. } => {}
            other => panic!("Expected DirtyWorktree error for modified file, got: {other:?}"),
        }

        // 5. Restore file back to clean state
        git::run_git(&wt_path, &["checkout", "--", "README.md"]).expect("restore file");
        assert!(!manager.is_dirty(&wt_path).expect("should be clean now"));

        // Safe delete now succeeds
        manager.safe_delete(&wt_path).expect("safe delete clean worktree");
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_delete_worktree_and_branch() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt_path = wt_parent.path().join("wt-branch-del");

        manager
            .create_worktree(CreateWorktreeOptions::new("ws-branch", "task-del", &wt_path))
            .expect("create failed");

        // Verify branch exists in repo
        let branches = git::run_git(manager.repo_root(), &["branch", "--list"]).expect("list branches");
        assert!(branches.contains("orca/ws-branch/task-del"));

        // Delete worktree and its branch
        manager
            .delete_worktree_and_branch(&wt_path, true)
            .expect("delete worktree and branch failed");

        assert!(!wt_path.exists());
        let branches_after = git::run_git(manager.repo_root(), &["branch", "--list"]).expect("list branches");
        assert!(!branches_after.contains("orca/ws-branch/task-del"));
    }

    #[test]
    fn test_create_worktree_already_exists_rejection() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt_path = wt_parent.path().join("existing-dir");
        fs::create_dir_all(&wt_path).expect("create dir");

        let err = manager
            .create_worktree(CreateWorktreeOptions::new("ws-1", "task-exist", &wt_path))
            .unwrap_err();

        match err {
            WorktreeError::WorktreeAlreadyExists { path } => {
                assert_eq!(path, wt_path);
            }
            other => panic!("Expected WorktreeAlreadyExists, got: {other:?}"),
        }
    }

    #[test]
    fn test_check_dirty_nonexistent_path() {
        let (_temp, manager) = setup_test_repo();
        let fake_path = Path::new("/path/that/definitely/does/not/exist");
        let err = manager.check_dirty(fake_path).unwrap_err();
        match err {
            WorktreeError::WorktreeNotFound { path } => {
                assert_eq!(path, fake_path);
            }
            other => panic!("Expected WorktreeNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn test_multiple_isolated_worktrees() {
        let (_temp, manager) = setup_test_repo();
        let wt_parent = TempDir::new().expect("tempdir");
        let wt1 = wt_parent.path().join("wt-1");
        let wt2 = wt_parent.path().join("wt-2");

        manager
            .create_worktree(CreateWorktreeOptions::new("ws-1", "worker-1", &wt1))
            .expect("create wt1");
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-2", "worker-2", &wt2))
            .expect("create wt2");

        // Mutate wt1
        fs::write(wt1.join("worker1.log"), "working").expect("write worker1");
        assert!(manager.is_dirty(&wt1).unwrap());
        assert!(!manager.is_dirty(&wt2).unwrap());

        // wt1 rejection, wt2 safe deletion
        assert!(manager.safe_delete(&wt1).is_err());
        assert!(manager.safe_delete(&wt2).is_ok());
    }
}
