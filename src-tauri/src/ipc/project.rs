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
    let canonical = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    if canonical.parent().is_none() {
        return Err(IpcError::from(WorktreeError::InvalidPath {
            path: canonical,
            reason: "filesystem root cannot be registered as a startup workspace".to_string(),
        }));
    }
    register_canonical_project(registry, &canonical, None)
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
    // `try_new` canonicalizes the root (walking up to the Git top level when
    // the path lives inside a repository) and falls back to the folder itself
    // for plain directories, so the uniqueness check below compares
    // canonical roots.
    let manager = WorktreeManager::try_new(repo_path).map_err(IpcError::from)?;
    let git_root = if manager.is_git_backed() {
        Some(manager.repo_root().to_path_buf())
    } else {
        None
    };
    let repo_root = manager.repo_root().to_path_buf();

    if let Some((workspace_id, _)) = registry
        .list()
        .into_iter()
        .find(|(_, manager)| manager.repo_root() == repo_root)
    {
        return Ok(RegisteredProject {
            workspace_id,
            repo_root,
            git_root,
        });
    }

    let requested_id = match preferred_id {
        Some(requested) => requested.to_string(),
        None => derive_workspace_id(&repo_root),
    };
    // Find-or-insert under one registry write lock: two concurrent
    // registrations of the same root must resolve to one workspace ID.
    let workspace_id = registry
        .register_unique_root(&requested_id, &repo_root)
        .map_err(IpcError::from)?;
    let repo_root = registry.repo_root(&workspace_id).map_err(IpcError::from)?;
    Ok(RegisteredProject {
        workspace_id,
        repo_root,
        git_root,
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
    /// Canonical Git root when the workspace is a Git repository; `None` for
    /// plain-folder (terminal-only) workspaces.
    pub git_root: Option<PathBuf>,
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
        if !manager.is_git_backed() {
            return Ok(Vec::new());
        }
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterProjectRequest {
    pub workspace_id: String,
}

#[tauri::command]
pub async fn cmd_project_unregister(
    daemon_client: State<'_, Arc<DaemonClient>>,
    workspace_registry: State<'_, WorkspaceRegistry>,
    request: UnregisterProjectRequest,
) -> Result<(), IpcError> {
    // Revoke the daemon binding first: it backs the remote gateway, so a
    // failure must propagate instead of silently leaving a removed project
    // reachable by paired remote devices. Local registry cleanup runs only
    // after the revocation succeeds, keeping both sides consistent.
    daemon_client
        .unregister_workspace(&request.workspace_id)
        .await?;
    workspace_registry.unregister(&request.workspace_id);
    Ok(())
}

#[tauri::command]
pub async fn cmd_path_reveal(path: String) -> Result<(), IpcError> {
    run_blocking(move || {
        let p = std::path::Path::new(&path);
        if !p.exists() {
            return Err(IpcError::new(
                crate::ipc::error::IpcErrorCode::InvalidPath,
                format!("Path does not exist: {path}"),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("-R")
                .arg(&path)
                .spawn()
                .map_err(|err| {
                    IpcError::internal(format!("Failed to reveal path '{path}': {err}"))
                })?;
        }

        #[cfg(target_os = "windows")]
        {
            // Pass one unquoted argument and let Rust's arg escaping own the
            // quoting; manually embedding "" shifts the argument boundary
            // CreateProcess reconstructs and lets paths with quotes inject
            // extra explorer arguments.
            std::process::Command::new("explorer")
                .arg(format!("/select,{path}"))
                .spawn()
                .map_err(|err| {
                    IpcError::internal(format!("Failed to reveal path '{path}': {err}"))
                })?;
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let target = if p.is_dir() {
                p
            } else {
                p.parent().unwrap_or(p)
            };
            std::process::Command::new("xdg-open")
                .arg(target)
                .spawn()
                .map_err(|err| {
                    IpcError::internal(format!("Failed to reveal path '{path}': {err}"))
                })?;
        }

        Ok(())
    })
    .await
}
