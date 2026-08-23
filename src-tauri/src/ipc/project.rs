use crate::daemon::DaemonClient;
use crate::ipc::{run_blocking, IpcError};
use crate::worktree::{run_git, WorkspaceRegistry, WorktreeError, WorktreeManager};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

/// Workspace ID that shipped as a hard-coded alias for the startup repository.
/// It is no longer registered. Clients that persisted it migrate by calling
/// [`cmd_project_initial`] on startup and adopting the canonical ID it returns.
pub const LEGACY_DEFAULT_WORKSPACE_ID: &str = "default";

/// Fallback ID for a root whose folder name carries no usable characters.
const FALLBACK_WORKSPACE_ID: &str = "project";

/// Derives the canonical workspace ID for a repository root from its folder
/// name, restricted to characters `WorkspaceRegistry::validate_workspace_id`
/// accepts (no separators, whitespace, control characters, or leading `-`).
pub fn derive_workspace_id(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let slug: String = name
                .chars()
                .map(|ch| {
                    if ch.is_alphanumeric() || ch == '_' || ch == '-' {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect();
            let trimmed = slug.trim_matches('-');
            if trimmed.is_empty() {
                FALLBACK_WORKSPACE_ID.to_string()
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_else(|| FALLBACK_WORKSPACE_ID.to_string())
}

/// Registers (idempotently) and returns the single canonical project for the
/// startup repository.
///
/// The ID is derived from the repository root reported by Git, not from the
/// process working directory, so launching from a subdirectory (`src-tauri/`,
/// a worktree checkout, a test harness) still yields one stable project ID.
pub fn initial_project(registry: &WorkspaceRegistry) -> Result<RegisteredProject, IpcError> {
    let cwd = std::env::current_dir().map_err(|error| {
        IpcError::from(WorktreeError::InvalidPath {
            path: PathBuf::from("."),
            reason: format!("the startup working directory is unavailable: {error}"),
        })
    })?;
    register_canonical_project(registry, &cwd, None)
}

/// Registers a repository root under exactly one workspace ID.
///
/// Canonical-root uniqueness: a root already registered under some ID resolves
/// to that existing project instead of gaining a second alias, so the registry
/// holds at most one ID per repository root. When `preferred_id` is `None` the
/// ID is derived from the root's folder name.
fn register_canonical_project(
    registry: &WorkspaceRegistry,
    repo_path: &Path,
    preferred_id: Option<&str>,
) -> Result<RegisteredProject, IpcError> {
    // `try_new` walks up to the Git top level, canonicalizes it, and rejects
    // non-Git paths, so the uniqueness check below compares canonical roots.
    let repo_root = WorktreeManager::try_new(repo_path)
        .map_err(IpcError::from)?
        .repo_root()
        .to_path_buf();

    if let Some((workspace_id, _)) = registry
        .list()
        .into_iter()
        .find(|(_, manager)| manager.repo_root() == repo_root)
    {
        return Ok(RegisteredProject {
            workspace_id,
            repo_root,
        });
    }

    let workspace_id = match preferred_id {
        Some(requested) => requested.to_string(),
        None => derive_workspace_id(&repo_root),
    };
    registry
        .register(&workspace_id, &repo_root)
        .map_err(IpcError::from)?;
    let repo_root = registry.repo_root(&workspace_id).map_err(IpcError::from)?;
    Ok(RegisteredProject {
        workspace_id,
        repo_root,
    })
}

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

/// Registers a project, enforcing one workspace ID per canonical repository
/// root. Requesting a new ID for an already-registered root is not an error:
/// the existing project is returned, so a client holding a stale ID (such as
/// the legacy `default`) migrates onto the canonical one instead of creating a
/// duplicate alias. Callers must adopt the returned `workspaceId`.
#[tauri::command]
pub async fn cmd_project_register(
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
    request: RegisterProjectRequest,
) -> Result<RegisteredProject, IpcError> {
    let registry = (*registry).clone();
    let client = daemon_client.inner().clone();
    let registered = run_blocking(move || {
        register_canonical_project(
            &registry,
            &request.repo_path,
            Some(request.workspace_id.as_str()),
        )
    })
    .await?;

    client
        .register_workspace(
            &registered.workspace_id,
            &registered.repo_root.to_string_lossy(),
        )
        .await?;

    Ok(registered)
}

/// Returns the one canonical project for the repository the app was started
/// from, registering it on first call. Replaces the previous startup behaviour
/// of registering both a derived ID and a `default` alias for the same root.
#[tauri::command]
pub async fn cmd_project_initial(
    daemon_client: State<'_, Arc<DaemonClient>>,
    registry: State<'_, WorkspaceRegistry>,
) -> Result<RegisteredProject, IpcError> {
    let registry = (*registry).clone();
    let client = daemon_client.inner().clone();
    let registered = run_blocking(move || initial_project(&registry)).await?;

    client
        .register_workspace(
            &registered.workspace_id,
            &registered.repo_root.to_string_lossy(),
        )
        .await?;

    Ok(registered)
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
