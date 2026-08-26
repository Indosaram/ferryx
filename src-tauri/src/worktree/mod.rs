pub mod git;
pub mod manager;
pub mod model;
pub mod registry;

pub use git::*;
pub use manager::*;
pub use model::*;
pub use registry::*;

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, WorktreeManager) {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let repo_path = temp_dir.path();
        git::run_git(repo_path, &["init"]).expect("git init failed");
        git::run_git(repo_path, &["config", "user.name", "Orca Test"])
            .expect("git config user.name failed");
        git::run_git(repo_path, &["config", "user.email", "test@orca.dev"])
            .expect("git config user.email failed");
        fs::write(repo_path.join("README.md"), "# Test Repo\n").expect("write README");
        git::run_git(repo_path, &["add", "README.md"]).expect("git add failed");
        git::run_git(repo_path, &["commit", "-m", "Initial commit"]).expect("git commit failed");
        let manager = WorktreeManager::new(repo_path);
        (temp_dir, manager)
    }

    fn path_for(manager: &WorktreeManager, name: &str) -> std::path::PathBuf {
        manager.repo_root().join(format!("wt-{name}"))
    }

    #[test]
    fn plain_folder_without_git_registers_and_guards_worktrees() {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let folder = temp_dir.path();
        fs::create_dir_all(folder.join("nested")).expect("mkdir failed");

        let manager = WorktreeManager::try_new(folder).expect("plain folder registers");
        assert_eq!(manager.repo_root(), fs::canonicalize(folder).unwrap());
        assert!(!manager.is_git_backed());

        // Worktree inspection is empty; mutations are rejected with a clear error.
        assert!(manager.list_worktrees().unwrap().is_empty());
        let err = manager
            .create_worktree(CreateWorktreeOptions::new(
                "ws-1",
                "task",
                folder.join("wt-task"),
            ))
            .unwrap_err();
        assert!(matches!(err, WorktreeError::NotAGitRepository { .. }));
    }

    #[test]
    fn branch_namespace_formatting_and_validation() {
        assert_eq!(
            WorktreeManager::format_branch_name("ws-123", "task-a").unwrap(),
            "orca/ws-123/task-a"
        );
        assert_eq!(
            WorktreeManager::format_branch_name("session_1", "feature/subtask").unwrap(),
            "orca/session_1/feature/subtask"
        );
        assert!(WorktreeManager::format_branch_name("", "slug").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "").is_err());
        assert!(WorktreeManager::format_branch_name("ws..1", "slug").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug with space").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug:name").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug~1").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "slug.lock").is_err());
        assert!(WorktreeManager::format_branch_name("ws/extra", "slug").is_err());
        assert!(WorktreeManager::format_branch_name("ws", "nested/-slug").is_err());

        let info = WorktreeManager::parse_branch_name("refs/heads/orca/ws-abc/my-task").unwrap();
        assert_eq!(info.ws_id, "ws-abc");
        assert_eq!(info.slug, "my-task");
        assert!(WorktreeManager::parse_branch_name("refs/heads/main").is_none());
    }

    #[test]
    fn parse_worktree_list_porcelain() {
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
        assert_eq!(worktrees[1].branch_short_name(), Some("orca/ws-1/task-1"));
        assert_eq!(
            worktrees[2].locked.as_deref(),
            Some("maintenance in progress")
        );
        assert!(worktrees[3].detached);
        assert_eq!(worktrees[4].prunable.as_deref(), Some("reason gitdir gone"));
    }

    #[test]
    fn parse_status_porcelain_v1_and_v2() {
        let clean = git::parse_status_porcelain("");
        assert!(!clean.is_dirty);

        let v1 = git::parse_status_porcelain(" M src/lib.rs\n?? new_file.txt\nD  deleted.rs\n");
        assert!(v1.is_dirty);
        assert_eq!(v1.files.len(), 3);
        assert_eq!(v1.files[0].path, "src/lib.rs");

        let v2 = git::parse_status_porcelain(
            "1 .M N... 100644 100644 100644 123 123 src/main.rs\n? untracked.json\n",
        );
        assert!(v2.is_dirty);
        assert_eq!(v2.files.len(), 2);
        assert_eq!(v2.files[0].status_code, ".M");
    }

    #[test]
    fn create_find_and_list_worktree() {
        let (_temp, manager) = setup_test_repo();
        let path = manager.worktree_path_for("ws-alpha", "feat-login").unwrap();
        let created = manager
            .create_worktree(CreateWorktreeOptions::new("ws-alpha", "feat-login", &path))
            .expect("create worktree");
        assert_eq!(
            created.branch_short_name(),
            Some("orca/ws-alpha/feat-login")
        );
        assert!(path.join("README.md").exists());
        assert_eq!(manager.list_worktrees().unwrap().len(), 2);
        assert!(manager.find_worktree(&path).unwrap().is_some());
        assert!(manager
            .find_worktree_by_slug("ws-alpha", "feat-login")
            .unwrap()
            .is_some());
        assert!(!manager.is_dirty(&path).unwrap());
    }

    #[test]
    fn safe_delete_clean_worktree() {
        let (_temp, manager) = setup_test_repo();
        let path = path_for(&manager, "clean");
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-safe", "task-clean", &path))
            .unwrap();
        manager.safe_delete(&path).expect("safe delete");
        assert!(!path.exists());
        assert_eq!(manager.list_worktrees().unwrap().len(), 1);
    }

    #[test]
    fn dirty_worktree_is_never_deleted_even_with_force_remove() {
        let (_temp, manager) = setup_test_repo();
        let path = path_for(&manager, "dirty");
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-dirty", "task-dirty", &path))
            .unwrap();
        let scratch = path.join("scratchpad.txt");
        fs::write(&scratch, "temporary notes").unwrap();
        assert!(manager.is_dirty(&path).unwrap());
        assert!(matches!(
            manager.safe_delete(&path),
            Err(WorktreeError::DirtyWorktree { .. })
        ));
        assert!(matches!(
            manager.remove_worktree(&path, true),
            Err(WorktreeError::DirtyWorktree { .. })
        ));
        assert!(scratch.exists());
    }

    #[test]
    fn safe_branch_delete_removes_merged_branch() {
        let (_temp, manager) = setup_test_repo();
        let path = path_for(&manager, "branch-delete");
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-branch", "task-del", &path))
            .unwrap();
        manager
            .delete_worktree_and_branch(&path, true)
            .expect("delete merged branch");
        assert!(!path.exists());
        let branches = git::run_git(manager.repo_root(), &["branch", "--list"]).unwrap();
        assert!(!branches.contains("orca/ws-branch/task-del"));
    }

    #[test]
    fn existing_target_and_missing_status_are_rejected() {
        let (_temp, manager) = setup_test_repo();
        let existing = path_for(&manager, "existing");
        fs::create_dir_all(&existing).unwrap();
        assert!(matches!(
            manager.create_worktree(CreateWorktreeOptions::new("ws", "existing", &existing)),
            Err(WorktreeError::WorktreeAlreadyExists { .. })
        ));

        let missing = path_for(&manager, "missing");
        assert!(matches!(
            manager.check_dirty(&missing),
            Err(WorktreeError::WorktreeNotFound { .. })
        ));
    }

    #[test]
    fn multiple_worktrees_remain_isolated() {
        let (_temp, manager) = setup_test_repo();
        let wt1 = path_for(&manager, "one");
        let wt2 = path_for(&manager, "two");
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-1", "worker-1", &wt1))
            .unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("ws-2", "worker-2", &wt2))
            .unwrap();
        fs::write(wt1.join("worker1.log"), "working").unwrap();
        assert!(manager.is_dirty(&wt1).unwrap());
        assert!(!manager.is_dirty(&wt2).unwrap());
        assert!(manager.safe_delete(&wt1).is_err());
        assert!(manager.safe_delete(&wt2).is_ok());
    }

    #[test]
    fn registry_resolves_only_registered_workspace_identity() {
        let (_temp, manager) = setup_test_repo();
        let registry = WorkspaceRegistry::new();
        registry
            .register("workspace-a", manager.repo_root())
            .unwrap();
        let identity = WorktreeIdentity {
            ws_id: "ws".into(),
            slug: "task".into(),
        };
        let target = registry.target_path("workspace-a", &identity).unwrap();
        assert!(target.starts_with(manager.repo_root()));
        assert!(matches!(
            registry.manager("missing"),
            Err(WorktreeError::WorkspaceNotFound { .. })
        ));
    }

    #[test]
    fn worktree_manager_resolves_nested_git_directory_to_canonical_root() {
        let (_temp, manager) = setup_test_repo();
        let nested_directory = manager.repo_root().join("src-tauri");
        fs::create_dir_all(&nested_directory).expect("create nested launch directory");

        let resolved = WorktreeManager::try_new(&nested_directory)
            .expect("nested Git directory must resolve to its repository root");

        assert_eq!(resolved.repo_root(), manager.repo_root());
    }

    #[test]
    fn git_merge_base_is_ancestor_and_branch_merge_detection() {
        let (_temp, manager) = setup_test_repo();
        let wt_path = manager.worktree_path_for("ws-merge", "task-merge").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new(
                "ws-merge",
                "task-merge",
                &wt_path,
            ))
            .expect("create worktree");

        // Newly branched from HEAD is an ancestor of HEAD
        let branch = "orca/ws-merge/task-merge";
        assert!(git::git_merge_base_is_ancestor(manager.repo_root(), branch, "HEAD").unwrap());
        assert!(manager.branch_is_merged(branch).unwrap());

        // Add a commit to the worktree branch -> no longer an ancestor of HEAD
        fs::write(wt_path.join("new_feature.txt"), "feature data").unwrap();
        git::run_git(&wt_path, &["add", "new_feature.txt"]).unwrap();
        git::run_git(&wt_path, &["commit", "-m", "add feature"]).unwrap();

        assert!(!git::git_merge_base_is_ancestor(manager.repo_root(), branch, "HEAD").unwrap());
        assert!(!manager.branch_is_merged(branch).unwrap());

        // Merge the branch into HEAD (main/master) -> becomes an ancestor of HEAD again
        git::run_git(manager.repo_root(), &["merge", branch]).unwrap();
        assert!(git::git_merge_base_is_ancestor(manager.repo_root(), branch, "HEAD").unwrap());
        assert!(manager.branch_is_merged(branch).unwrap());
    }

    #[test]
    fn resolve_worktree_by_path_and_identity() {
        let (_temp, manager) = setup_test_repo();
        let registry = WorkspaceRegistry::new();
        registry
            .register("workspace-test", manager.repo_root())
            .unwrap();

        let identity = WorktreeIdentity {
            ws_id: "ws-direct".into(),
            slug: "direct-task".into(),
        };

        // Before creation, resolve fails with WorktreeIdentityNotFound
        assert!(matches!(
            registry.resolve_worktree("workspace-test", &identity),
            Err(WorktreeError::WorktreeIdentityNotFound { .. })
        ));

        let wt_path = manager
            .worktree_path_for(&identity.ws_id, &identity.slug)
            .unwrap();
        let created = manager
            .create_worktree(CreateWorktreeOptions::new(
                &identity.ws_id,
                &identity.slug,
                &wt_path,
            ))
            .expect("create worktree");
        assert_eq!(
            created.branch_short_name(),
            Some("orca/ws-direct/direct-task")
        );

        // After creation, resolve succeeds and returns matching canonical path
        let (resolved_mgr, resolved_wt) = registry
            .resolve_worktree("workspace-test", &identity)
            .expect("resolve worktree");
        assert_eq!(resolved_wt.path, created.path);
        assert_eq!(resolved_mgr.repo_root(), manager.repo_root());
    }
}
