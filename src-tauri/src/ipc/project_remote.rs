use crate::daemon::DaemonClient;
use crate::ipc::{run_blocking, IpcError};
use crate::ssh::{direct, projects};
use crate::worktree::WorkspaceRegistry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Runtime, State};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRemoteProjectRequest {
    pub workspace_id: String,
    pub host_id: String,
    pub repo_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredRemoteProject {
    pub workspace_id: String,
    pub repo_root: String,
    pub git_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,
    pub host_id: String,
    pub host_label: String,
}

/// Validate a saved SSH host and remote directory, then persist their canonical
/// project identity. This is the exact core used by `cmd_project_register_remote`;
/// standalone callers supply an existing `ssh_hosts.json` inventory path and a
/// Tokio runtime, without constructing a Tauri AppHandle. No executable or trust
/// policy is accepted in the request. The desktop command additionally checks
/// local daemon availability before calling this function.
///
/// `workspace_id` is advisory: both an initial local-style slug and a returned
/// `ssh:<hash>` ID are accepted. Callers must adopt the returned canonical ID.
pub async fn register_remote_project(
    host_store: PathBuf,
    request: RegisterRemoteProjectRequest,
) -> Result<RegisteredRemoteProject, IpcError> {
    // The requested slug is advisory, as in local canonical registration. Identity
    // comes from the canonical host/path, not the user-provided display slug.
    if !projects::is_remote(&request.workspace_id) {
        WorkspaceRegistry::validate_workspace_id(&request.workspace_id).map_err(IpcError::from)?;
    }
    direct::validate_remote_path(&request.repo_path)?;
    let lookup = host_store.clone();
    let host = run_blocking(move || projects::enabled_host(&lookup, &request.host_id)).await?;
    let (repo_root, git_root, git_remote) = direct::probe(&host, &request.repo_path).await?;
    let project = projects::RemoteProject {
        workspace_id: projects::identity(&host.id, &repo_root),
        host_id: host.id.clone(),
        repo_root,
        git_root,
        git_remote,
    };
    let probed_host = host.clone();
    let project = run_blocking(move || {
        if projects::enabled_host(&host_store, &project.host_id)? != probed_host {
            return Err(IpcError::new(
                crate::ipc::IpcErrorCode::InvalidPath,
                "SSH host changed during registration; validate the project again",
            ));
        }
        projects::persist(&host_store, project)
    })
    .await?;
    Ok(RegisteredRemoteProject {
        workspace_id: project.workspace_id,
        repo_root: project.repo_root,
        git_root: project.git_root,
        git_remote: project.git_remote,
        host_id: project.host_id,
        host_label: host.label,
    })
}

#[tauri::command]
pub async fn cmd_project_register_remote<R: Runtime>(
    app: AppHandle<R>,
    daemon_client: State<'_, Arc<DaemonClient>>,
    request: RegisterRemoteProjectRequest,
) -> Result<RegisteredRemoteProject, IpcError> {
    let path = super::ssh::get_ssh_store_path(&app)?;
    // Fail before creating durable state if the local PTY daemon is unavailable.
    daemon_client.ping().await?;
    register_remote_project(path, request).await
}

#[cfg(test)]
#[path = "project_remote_tests.rs"]
mod tests;
