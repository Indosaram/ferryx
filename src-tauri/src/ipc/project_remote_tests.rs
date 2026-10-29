use super::*;
use crate::ipc::*;
use crate::worktree::{WorkspaceRegistry, WorktreeIdentity};
use tauri::Manager;

#[test]
fn remote_registration_wire_request_rejects_extra_execution_fields() {
    let request: RegisterRemoteProjectRequest = serde_json::from_value(serde_json::json!({
        "workspaceId": "project", "hostId": "host", "repoPath": "/srv/project"
    }))
    .unwrap();
    assert_eq!(request.repo_path, "/srv/project");
    assert!(
        serde_json::from_value::<RegisterRemoteProjectRequest>(serde_json::json!({
            "workspaceId": "project", "hostId": "host", "repoPath": "/srv/project", "program": "sh"
        }))
        .is_err()
    );
}

#[tokio::test]
async fn all_local_git_boundaries_reject_remote_namespace_even_without_loaded_metadata() {
    let app = tauri::test::mock_builder()
        .manage(WorkspaceRegistry::new())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let id = "ssh:stored-remote-identity".to_string();
    let worktree = WorktreeIdentity {
        ws_id: "ws".into(),
        slug: "feature".into(),
    };
    assert_eq!(
        cmd_worktree_list(app.handle().clone(), app.state(), id.clone())
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::WorkspaceNotFound
    );
    assert_eq!(
        cmd_project_branches(
            app.state(),
            ProjectBranchesRequest {
                workspace_id: id.clone()
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_worktree_create(
            app.handle().clone(),
            app.state(),
            CreateWorktreeRequest {
                workspace_id: id.clone(),
                worktree: worktree.clone(),
                base_ref: None,
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_worktree_delete(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: id.clone(),
                worktree: worktree.clone(),
                delete_branch: Some(true),
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_worktree_delete_destructive(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: id.clone(),
                worktree: worktree.clone(),
                delete_branch: Some(true),
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_worktree_status(
            app.handle().clone(),
            app.state(),
            WorktreeStatusRequest {
                workspace_id: id.clone(),
                worktree: worktree.clone(),
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_worktree_delete_preview(
            app.state(),
            WorktreeStatusRequest {
                workspace_id: id.clone(),
                worktree,
            }
        )
        .await
        .unwrap_err()
        .code,
        IpcErrorCode::Unsupported
    );
    assert_eq!(
        cmd_path_reveal(app.handle().clone(), "/".into(), Some(id.clone()))
            .await
            .unwrap_err()
            .code,
        IpcErrorCode::Unsupported
    );
}

#[tokio::test]
async fn local_registration_never_canonicalizes_a_remote_id_into_a_local_project() {
    let root = tempfile::tempdir().unwrap();
    let registry = WorkspaceRegistry::new();
    registry.register("local-project", root.path()).unwrap();
    let app = tauri::test::mock_builder()
        .manage(registry.clone())
        .manage(Arc::new(DaemonClient::new_with_socket(
            root.path().join("unused.sock"),
        )))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let error = cmd_project_register(
        app.state(),
        app.state(),
        RegisterProjectRequest {
            workspace_id: "ssh:remote".into(),
            repo_path: root.path().to_path_buf(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, IpcErrorCode::Unsupported);
    assert_eq!(registry.list().len(), 1);
}

#[test]
fn remote_startup_cannot_use_local_shell_resolver() {
    let startup = crate::daemon::protocol::TerminalStartup::RemoteSsh {
        host_store_path: "/private/ssh_hosts.json".into(),
    };
    assert_eq!(
        crate::terminal::shell::resolve_startup_command_pure(
            Some("/bin/sh"),
            Some(&startup),
            crate::terminal::shell::TargetPlatform::CURRENT,
            |_| true,
            |_| None,
        )
        .unwrap_err(),
        crate::terminal::shell::AgentResumeError::RemoteSshRequiresWorkspace
    );
}
