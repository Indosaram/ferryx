use crate::ipc::{run_blocking, IpcError};
use crate::worktree::{run_git, WorkspaceRegistry};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterProjectRequest {
    pub workspace_id: String,
    pub repo_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredProject {
    pub workspace_id: String,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectBranchesRequest {
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBranch {
    pub name: String,
    pub is_current: bool,
}

#[tauri::command]
pub async fn cmd_project_register(
    registry: State<'_, WorkspaceRegistry>,
    request: RegisterProjectRequest,
) -> Result<RegisteredProject, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        registry
            .register(&request.workspace_id, &request.repo_path)
            .map_err(IpcError::from)?;
        let repo_root = registry
            .repo_root(&request.workspace_id)
            .map_err(IpcError::from)?;
        Ok(RegisteredProject {
            workspace_id: request.workspace_id,
            repo_root,
        })
    })
    .await
}

#[tauri::command]
pub async fn cmd_project_branches(
    registry: State<'_, WorkspaceRegistry>,
    request: ProjectBranchesRequest,
) -> Result<Vec<LocalBranch>, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry
            .manager(&request.workspace_id)
            .map_err(IpcError::from)?;
        let branch_output = run_git(
            manager.repo_root(),
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(refname:short)",
                "refs/heads/",
            ],
        )
        .map_err(IpcError::from)?;
        let current_output =
            run_git(manager.repo_root(), &["branch", "--show-current"]).map_err(IpcError::from)?;
        let current = current_output.trim();

        let mut branches = branch_output
            .lines()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| LocalBranch {
                name: name.to_string(),
                is_current: !current.is_empty() && name == current,
            })
            .collect::<Vec<_>>();
        branches.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(branches)
    })
    .await
}
