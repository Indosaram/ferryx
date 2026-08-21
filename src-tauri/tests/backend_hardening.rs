use ferryx_lib::ipc::{CreateWorktreeRequest, SpawnTerminalRequest};
use ferryx_lib::worktree::{
    run_git, CreateWorktreeOptions, DirtyState, WorktreeError, WorktreeManager,
};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn setup_repo() -> (TempDir, WorktreeManager) {
    let repo = TempDir::new().expect("repo tempdir");
    run_git(repo.path(), &["init"]).expect("git init");
    run_git(repo.path(), &["config", "user.name", "Orca Test"]).expect("git user name");
    run_git(repo.path(), &["config", "user.email", "test@orca.dev"]).expect("git user email");
    fs::write(repo.path().join("README.md"), "# base\n").expect("write README");
    run_git(repo.path(), &["add", "README.md"]).expect("git add");
    run_git(repo.path(), &["commit", "-m", "base"]).expect("git commit");
    let manager = WorktreeManager::new(repo.path());
    (repo, manager)
}

#[test]
fn worktree_creation_rejects_parent_traversal_outside_registered_root() {
    let (repo, manager) = setup_repo();
    let outside = repo.path().parent().expect("repo parent").join(format!(
        "orca-outside-{}",
        std::process::id()
    ));
    let result = manager.create_worktree(CreateWorktreeOptions::new(
        "ws-hardening",
        "traversal",
        &outside,
    ));
    assert!(result.is_err(), "../ traversal target must be rejected before Git runs");
}

#[cfg(unix)]
#[test]
fn worktree_creation_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let (repo, manager) = setup_repo();
    let outside = TempDir::new().expect("outside tempdir");
    let link = repo.path().join("escape-link");
    symlink(outside.path(), &link).expect("create symlink");

    let escaped_target = link.join("nested-worktree");
    let result = manager.create_worktree(CreateWorktreeOptions::new(
        "ws-hardening",
        "symlink",
        &escaped_target,
    ));
    assert!(result.is_err(), "symlink target outside allowed root must be rejected");
}

#[test]
fn leading_dash_namespace_component_is_rejected() {
    assert!(
        WorktreeManager::format_branch_name("-workspace", "task").is_err(),
        "leading dash workspace IDs must be rejected before reaching Git option parsing"
    );
    assert!(
        WorktreeManager::format_branch_name("workspace", "-task").is_err(),
        "leading dash slugs must be rejected before reaching Git option parsing"
    );
}

#[test]
fn ipc_request_contract_rejects_raw_paths_and_shell_commands() {
    let raw_worktree = json!({
        "repoRoot": "/tmp/attacker-repo",
        "wsId": "ws",
        "slug": "task",
        "path": "/tmp/attacker-worktree",
        "baseRef": null
    });
    assert!(
        serde_json::from_value::<CreateWorktreeRequest>(raw_worktree).is_err(),
        "worktree IPC must not accept raw repoRoot/path overrides"
    );

    let raw_terminal = json!({
        "cwd": "/tmp/attacker-cwd",
        "cols": 80,
        "rows": 24,
        "command": "touch /tmp/raw-shell-surface"
    });
    assert!(
        serde_json::from_value::<SpawnTerminalRequest>(raw_terminal).is_err(),
        "terminal IPC must not accept arbitrary cwd or raw shell command strings"
    );
}

#[test]
fn serialized_dirty_state_uses_explicit_camel_case_fields() {
    let value = serde_json::to_value(DirtyState::clean()).expect("serialize dirty state");
    assert_eq!(value.get("isDirty"), Some(&json!(false)));
    assert!(value.get("is_dirty").is_none(), "snake_case IPC fields must not leak");
}

#[test]
fn csp_is_active_and_restrictive() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse tauri.conf.json");
    let csp = config["app"]["security"]["csp"]
        .as_str()
        .expect("CSP must be an active string policy");

    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(!csp.contains("script-src *"));
}

#[test]
fn git_error_command_log_escapes_control_characters() {
    let temp = TempDir::new().expect("tempdir");
    let err = run_git(temp.path(), &["rev-parse", "bad\nref"]).expect_err("command must fail");
    let WorktreeError::GitError { command, .. } = err else {
        panic!("expected GitError");
    };
    assert!(
        !command.contains('\n'),
        "Git error command metadata must escape literal control characters"
    );
}

#[test]
fn worktree_manager_validates_repo_root_during_construction() {
    let non_repo = TempDir::new().expect("non-repo tempdir");
    let result = std::panic::catch_unwind(|| WorktreeManager::new(non_repo.path()));
    assert!(result.is_err(), "invalid/non-Git repo roots must be rejected immediately");
}
