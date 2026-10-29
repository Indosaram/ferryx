use crate::daemon::{
    protocol::{DaemonRequest, DaemonResponse},
    DaemonClient,
};
use crate::ipc::{run_blocking, IpcError, IpcErrorCode};
use crate::worktree::changes::{GitChangeError, GitDiffScope};
#[cfg(feature = "desktop")]
use crate::worktree::changes::{
    git_commit, git_diff, git_push, git_stage, git_unstage, GitPushResult,
};
use crate::worktree::{
    BranchDeletionPreview, DirtyState, ExternalWorktree, WorkspaceRegistry, Worktree,
    WorktreeIdentity,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(feature = "desktop")]
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

pub const WORKTREE_CHANGED_EVENT: &str = "worktree_changed";

pub(crate) fn worktree_response(response: DaemonResponse) -> Result<DaemonResponse, IpcError> {
    match response {
        DaemonResponse::WorktreeError { error } => Err(error),
        DaemonResponse::Error { message, .. } => Err(IpcError::internal(message)),
        response => Ok(response),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub base_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteWorktreeRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub delete_branch: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeStatusRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeChangeKind {
    Created,
    Deleted,
    DestructivelyDeleted,
    #[serde(rename = "dirtyChanged")]
    DirtyChanged,
    Pruned,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChangedPayload {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub kind: WorktreeChangeKind,
}

#[cfg(feature = "desktop")]
pub(crate) fn emit_worktree_changed<R: Runtime>(
    app: &AppHandle<R>,
    workspace_id: String,
    worktree: WorktreeIdentity,
    kind: WorktreeChangeKind,
) -> Result<(), IpcError> {
    app.emit(
        WORKTREE_CHANGED_EVENT,
        WorktreeChangedPayload {
            workspace_id,
            worktree,
            kind,
        },
    )
    .map_err(|error| IpcError::internal(format!("failed to emit worktree_changed: {error}")))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_list<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    workspace_id: String,
) -> Result<Vec<Worktree>, IpcError> {
    if crate::ssh::projects::is_remote(&workspace_id) {
        let store = crate::ipc::ssh::get_ssh_store_path(&app)?;
        let id = workspace_id.clone();
        let (project, host) =
            run_blocking(move || crate::ssh::projects::resolve(&store, &id)).await?;
        let environment = crate::ssh::runtime::detect_for(&host, project.platform).await?;
        let remote_worktrees =
            crate::ssh::worktree::list_remote(&host, &environment, &project.repo_root).await?;
        let worktrees = remote_worktrees
            .into_iter()
            .map(|wt| Worktree {
                path: std::path::PathBuf::from(wt.path),
                head: wt.head.unwrap_or_default(),
                branch: wt.branch,
                bare: wt.bare,
                detached: wt.detached,
                locked: None,
                prunable: None,
            })
            .collect();
        return Ok(worktrees);
    }
    if workspace_id.starts_with("daemon:") {
        return Ok(Vec::new());
    }

    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry.manager(&workspace_id).map_err(IpcError::from)?;
        if !manager.is_git_backed() {
            return Ok(Vec::new());
        }
        manager.list_worktrees().map_err(IpcError::from)
    })
    .await
}

/// Branch prefix and managed directory new worktrees of a repository use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSettingsResponse {
    pub workspace_id: String,
    /// The prefix new worktree branches get (`<prefix>/<ws>/<slug>`).
    pub branch_prefix: String,
    /// The directory, under the repository root, new worktrees are created in.
    pub worktree_dir: String,
    /// The prefix explicitly stored for this repository, if any.
    pub configured_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeSettingsSetRequest {
    pub workspace_id: String,
    /// `None` clears the stored prefix and restores the default for the repository.
    pub branch_prefix: Option<String>,
}

fn worktree_settings_for(
    workspace_id: &str,
    manager: &crate::worktree::WorktreeManager,
) -> WorktreeSettingsResponse {
    let layout = manager.layout();
    WorktreeSettingsResponse {
        workspace_id: workspace_id.to_string(),
        branch_prefix: layout.branch_prefix,
        worktree_dir: layout.worktree_dir.to_string(),
        configured_prefix: crate::worktree::naming::configured_prefix(
            manager.settings_path(),
            manager.repo_root(),
        ),
    }
}

/// Reads the worktree naming of a local repository. The values live backend-side in
/// `<data>/worktree-settings.json`, so the daemon, SSH helper and CLI read the same setting.
pub fn worktree_settings_get(
    registry: &WorkspaceRegistry,
    workspace_id: &str,
) -> Result<WorktreeSettingsResponse, IpcError> {
    WorkspaceRegistry::validate_workspace_id(workspace_id).map_err(IpcError::from)?;
    let manager = registry.manager(workspace_id).map_err(IpcError::from)?;
    Ok(worktree_settings_for(workspace_id, &manager))
}

/// Stores the branch prefix of a local repository after validating it
/// (`^[a-z0-9][a-z0-9._-]{0,30}$`). An invalid prefix fails with `INVALID_NAMESPACE`.
pub fn worktree_settings_set(
    registry: &WorkspaceRegistry,
    request: &WorktreeSettingsSetRequest,
) -> Result<WorktreeSettingsResponse, IpcError> {
    WorkspaceRegistry::validate_workspace_id(&request.workspace_id).map_err(IpcError::from)?;
    let manager = registry
        .manager(&request.workspace_id)
        .map_err(IpcError::from)?;
    if let Some(prefix) = request.branch_prefix.as_deref() {
        crate::worktree::naming::validate_branch_prefix(prefix).map_err(|error| {
            IpcError::from(crate::worktree::WorktreeError::InvalidNamespace {
                reason: error.reason,
            })
        })?;
    }
    let path = manager.settings_path().ok_or_else(|| {
        IpcError::internal("No Ferryx data directory; set FERRYX_DATA_DIR to store worktree settings")
    })?;
    crate::worktree::naming::write_configured_prefix(
        path,
        manager.repo_root(),
        request.branch_prefix.as_deref(),
    )
    .map_err(|error| IpcError::from(crate::worktree::WorktreeError::Io(error)))?;
    Ok(worktree_settings_for(&request.workspace_id, &manager))
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_settings_get(
    registry: State<'_, WorkspaceRegistry>,
    workspace_id: String,
) -> Result<WorktreeSettingsResponse, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || worktree_settings_get(&registry, &workspace_id)).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_settings_set(
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeSettingsSetRequest,
) -> Result<WorktreeSettingsResponse, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || worktree_settings_set(&registry, &request)).await
}

#[cfg(feature = "desktop")]
/// Lists the registered local repository's worktrees that Git knows about but that live outside
/// the project folder (for example Orca's). The UI asks before importing any of them; remote and
/// paired projects have no local Git state to inspect and report none.
#[tauri::command]
pub async fn cmd_worktree_list_external(
    registry: State<'_, WorkspaceRegistry>,
    workspace_id: String,
) -> Result<Vec<ExternalWorktree>, IpcError> {
    if crate::ssh::projects::is_remote(&workspace_id) || workspace_id.starts_with("daemon:") {
        return Ok(Vec::new());
    }
    let registry = (*registry).clone();
    run_blocking(move || {
        let manager = registry.manager(&workspace_id).map_err(IpcError::from)?;
        manager.list_external_worktrees().map_err(IpcError::from)
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_create<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: CreateWorktreeRequest,
) -> Result<Worktree, IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    WorkspaceRegistry::validate_workspace_id(&workspace_id).map_err(IpcError::from)?;
    let created = match worktree_response(
        app.state::<Arc<DaemonClient>>()
            .send_request(DaemonRequest::CreateWorktree {
                workspace_id,
                worktree: identity,
                base_ref: request.base_ref,
            })
            .await?,
    )? {
        DaemonResponse::CreateWorktreeOk { worktree } => worktree,
        _ => return Err(IpcError::internal("Unexpected worktree response")),
    };
    registry.bump_revision();

    emit_worktree_changed(
        &app,
        event_workspace_id,
        event_identity,
        WorktreeChangeKind::Created,
    )?;
    Ok(created)
}

#[cfg(feature = "desktop")]
async fn delete_worktree<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
    destructive: bool,
) -> Result<(), IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let delete_branch = request.delete_branch.unwrap_or(false);
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    WorkspaceRegistry::validate_workspace_id(&workspace_id).map_err(IpcError::from)?;
    // Capture the managed path before the daemon removes the checkout/record.
    let deleted_path = registry
        .manager(&workspace_id)
        .map_err(IpcError::from)?
        .worktree_path_for(&identity.ws_id, &identity.slug)
        .map_err(IpcError::from)?;
    let pruned = match worktree_response(
        app.state::<Arc<DaemonClient>>()
            .send_request(DaemonRequest::DeleteWorktree {
                workspace_id,
                worktree: identity,
                delete_branch,
                destructive,
            })
            .await?,
    )? {
        DaemonResponse::DeleteWorktreeOk { pruned } => pruned,
        _ => return Err(IpcError::internal("Unexpected worktree response")),
    };
    registry.bump_revision();

    // The disk-scan cache is optional app state: a host that never managed it
    // (a test harness, or a future embedder) must still be able to delete a
    // worktree. `state()` panics when unmanaged, so ask for it fallibly the way
    // every other optional-state call site in this crate does.
    if let Some(scans) = app.try_state::<crate::ipc::worktree_disk::WorktreeDiskScans>() {
        if let Some(snapshot) = scans.remove_deleted(&event_workspace_id, &deleted_path) {
            let _ = app.emit(
                crate::ipc::worktree_disk::WORKTREE_DISK_SCAN_PROGRESS_EVENT,
                snapshot,
            );
        }
    }

    emit_worktree_changed(
        &app,
        event_workspace_id.clone(),
        event_identity.clone(),
        if destructive {
            WorktreeChangeKind::DestructivelyDeleted
        } else {
            WorktreeChangeKind::Deleted
        },
    )?;
    if pruned {
        emit_worktree_changed(
            &app,
            event_workspace_id,
            event_identity,
            WorktreeChangeKind::Pruned,
        )?;
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_delete<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, false).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_delete_destructive<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, true).await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_delete_preview(
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<BranchDeletionPreview, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_deletion_worktree(&request.workspace_id, &request.worktree)
            .map_err(IpcError::from)?;
        manager
            .branch_deletion_preview(&worktree.path)
            .map_err(IpcError::from)
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_status<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<DirtyState, IpcError> {
    let registry = (*registry).clone();
    let workspace_id = request.workspace_id.clone();
    let identity = request.worktree.clone();
    let event_workspace_id = workspace_id.clone();
    let event_identity = identity.clone();

    let (status, changed) = run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_worktree(&workspace_id, &identity)
            .map_err(IpcError::from)?;
        manager
            .observe_dirty_state(&worktree.path)
            .map_err(IpcError::from)
    })
    .await?;

    if changed {
        emit_worktree_changed(
            &app,
            event_workspace_id,
            event_identity,
            WorktreeChangeKind::DirtyChanged,
        )?;
    }
    Ok(status)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeDiffRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub scope: GitDiffScope,
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreePathsRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeCommitRequest {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub message: String,
}

impl From<GitChangeError> for IpcError {
    fn from(error: GitChangeError) -> Self {
        let message = error.to_string();
        match error {
            GitChangeError::NothingToCommit => Self::new(IpcErrorCode::GitNothingToCommit, message),
            GitChangeError::PushRejected { stderr } => {
                Self::new(IpcErrorCode::GitPushRejected, message)
                    .with_details(serde_json::json!({ "stderr": stderr }))
            }
            GitChangeError::NoRemote => Self::new(IpcErrorCode::GitNoRemote, message),
            GitChangeError::EmptyMessage => Self::new(IpcErrorCode::InvalidArgument, message)
                .with_details(serde_json::json!({ "reason": "empty_message" })),
            GitChangeError::DetachedHead => Self::new(IpcErrorCode::InvalidArgument, message)
                .with_details(serde_json::json!({ "reason": "detached_head" })),
            GitChangeError::InvalidCommit { sha } => {
                Self::new(IpcErrorCode::InvalidArgument, message)
                    .with_details(serde_json::json!({ "reason": "invalid_commit", "sha": sha }))
            }
            GitChangeError::Worktree(error) => error.into(),
        }
    }
}

/// Resolves a managed worktree to its jailed canonical path and runs `work` there on a
/// blocking thread.
#[cfg(feature = "desktop")]
async fn with_worktree_path<T: Send + 'static>(
    registry: WorkspaceRegistry,
    workspace_id: String,
    identity: WorktreeIdentity,
    work: impl FnOnce(&std::path::Path) -> Result<T, GitChangeError> + Send + 'static,
) -> Result<T, IpcError> {
    run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_worktree(&workspace_id, &identity)
            .map_err(IpcError::from)?;
        let path = manager
            .canonical_allowed_path(&worktree.path)
            .map_err(IpcError::from)?;
        work(&path).map_err(IpcError::from)
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_diff(
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeDiffRequest,
) -> Result<String, IpcError> {
    let WorktreeDiffRequest { workspace_id, worktree, scope, file } = request;
    with_worktree_path((*registry).clone(), workspace_id, worktree, move |path| {
        git_diff(path, &scope, file.as_deref())
    })
    .await
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_stage<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreePathsRequest,
) -> Result<(), IpcError> {
    let WorktreePathsRequest { workspace_id, worktree, paths } = request;
    let event = (workspace_id.clone(), worktree.clone());
    with_worktree_path((*registry).clone(), workspace_id, worktree, move |path| {
        git_stage(path, &paths)
    })
    .await?;
    emit_worktree_changed(&app, event.0, event.1, WorktreeChangeKind::DirtyChanged)
}

#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_unstage<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreePathsRequest,
) -> Result<(), IpcError> {
    let WorktreePathsRequest { workspace_id, worktree, paths } = request;
    let event = (workspace_id.clone(), worktree.clone());
    with_worktree_path((*registry).clone(), workspace_id, worktree, move |path| {
        git_unstage(path, &paths)
    })
    .await?;
    emit_worktree_changed(&app, event.0, event.1, WorktreeChangeKind::DirtyChanged)
}

/// Commits the staged changes and returns the new commit sha.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_commit<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeCommitRequest,
) -> Result<String, IpcError> {
    let WorktreeCommitRequest { workspace_id, worktree, message } = request;
    let event = (workspace_id.clone(), worktree.clone());
    let sha = with_worktree_path((*registry).clone(), workspace_id, worktree, move |path| {
        git_commit(path, &message)
    })
    .await?;
    emit_worktree_changed(&app, event.0, event.1, WorktreeChangeKind::Updated)?;
    Ok(sha)
}

/// Pushes the worktree branch to its upstream (or `origin` with `-u`). Never forces.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn cmd_worktree_push<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<GitPushResult, IpcError> {
    let WorktreeStatusRequest { workspace_id, worktree } = request;
    let event = (workspace_id.clone(), worktree.clone());
    let result = with_worktree_path((*registry).clone(), workspace_id, worktree, git_push).await?;
    emit_worktree_changed(&app, event.0, event.1, WorktreeChangeKind::Updated)?;
    Ok(result)
}

#[cfg(test)]
mod git_change_error_tests {
    use super::*;

    #[test]
    fn git_change_errors_map_to_structured_codes() {
        let code = |error: GitChangeError| serde_json::to_value(IpcError::from(error).code).unwrap();
        assert_eq!(code(GitChangeError::NothingToCommit), "GIT_NOTHING_TO_COMMIT");
        assert_eq!(code(GitChangeError::NoRemote), "GIT_NO_REMOTE");
        let rejected = IpcError::from(GitChangeError::PushRejected { stderr: "! [rejected]".into() });
        assert_eq!(serde_json::to_value(&rejected.code).unwrap(), "GIT_PUSH_REJECTED");
        assert_eq!(rejected.details.unwrap()["stderr"], "! [rejected]");
        assert_eq!(code(GitChangeError::EmptyMessage), "INVALID_ARGUMENT");
        let outside = GitChangeError::Worktree(crate::worktree::WorktreeError::InvalidPath {
            path: "../x".into(),
            reason: "parent traversal".into(),
        });
        assert_eq!(code(outside), "INVALID_PATH");
        for wire in ["GIT_NOTHING_TO_COMMIT", "GIT_PUSH_REJECTED", "GIT_NO_REMOTE"] {
            assert_eq!(serde_json::to_value(IpcErrorCode::from_code_str(wire)).unwrap(), wire);
        }
    }

    #[test]
    fn diff_scope_wire_shape() {
        let scope: GitDiffScope =
            serde_json::from_value(serde_json::json!({ "kind": "commit", "sha": "abc1234" })).unwrap();
        assert_eq!(scope, GitDiffScope::Commit { sha: "abc1234".into() });
        let staged: GitDiffScope = serde_json::from_value(serde_json::json!({ "kind": "staged" })).unwrap();
        assert_eq!(staged, GitDiffScope::Staged);
        let working: GitDiffScope =
            serde_json::from_value(serde_json::json!({ "kind": "workingTree" })).unwrap();
        assert_eq!(working, GitDiffScope::WorkingTree);
    }
}

#[cfg(all(test, unix))]
mod deletion_repair_tests {
    use super::*;
    use crate::ipc::worktree_disk::WorktreeDiskScans;
    use crate::worktree::{run_git, CreateWorktreeOptions};
    use tauri::{Listener, Manager};

    // Copy existing objects; never create commits or mutate the source repository.
    fn fixture() -> (
        tempfile::TempDir,
        WorkspaceRegistry,
        WorktreeIdentity,
        Worktree,
    ) {
        let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        let dir = std::env::var("TMPDIR")
            .ok()
            .and_then(|t| tempfile::tempdir_in(t).ok())
            .or_else(|| tempfile::tempdir_in(&target_dir).ok())
            .unwrap_or_else(|| tempfile::tempdir().unwrap());
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "--quiet"]).unwrap();
        run_git(&repo, &["config", "user.name", "Deletion fixture"]).unwrap();
        run_git(&repo, &["config", "user.email", "fixture@example.invalid"]).unwrap();
        run_git(&repo, &["commit", "--allow-empty", "-m", "base"]).unwrap();
        std::fs::write(repo.join("target.txt"), b"unmerged change").unwrap();
        run_git(&repo, &["add", "target.txt"]).unwrap();
        run_git(&repo, &["commit", "-m", "target"]).unwrap();
        let tip = run_git(&repo, &["rev-parse", "HEAD"]).unwrap();
        run_git(&repo, &["checkout", "--detach", "HEAD~1"]).unwrap();
        let registry = WorkspaceRegistry::new();
        registry.register("repair", &repo).unwrap();
        let manager = registry.manager("repair").unwrap();
        let identity = WorktreeIdentity {
            ws_id: "repair".into(),
            slug: "target".into(),
        };
        let path = manager
            .worktree_path_for(&identity.ws_id, &identity.slug)
            .unwrap();
        let wt = manager
            .create_worktree(
                CreateWorktreeOptions::new("repair", "target", path).with_base_ref(tip.trim()),
            )
            .unwrap();
        (dir, registry, identity, wt)
    }

    async fn deletion_daemon(
        root: &std::path::Path,
        registry: &WorkspaceRegistry,
    ) -> (
        Arc<DaemonClient>,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let owner = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(root.join("config.json")),
            Some(root.join("auth.json")),
        ));
        owner
            .remote_state()
            .machine_services
            .as_ref()
            .unwrap()
            .workspaces
            .register(
                "repair",
                registry
                    .manager("repair")
                    .unwrap()
                    .repo_root()
                    .to_str()
                    .unwrap(),
            )
            .unwrap();
        let socket = root.join("repair.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        clients.spawn(owner.clone().handle_client(stream));
                    }
                }
            }
            clients.shutdown().await;
        });
        (Arc::new(DaemonClient::new_with_socket(socket)), stop, task)
    }

    async fn stop_deletion_daemon(
        stop: tokio::sync::oneshot::Sender<()>,
        mut task: tokio::task::JoinHandle<()>,
    ) {
        stop.send(()).unwrap();
        match tokio::time::timeout(std::time::Duration::from_secs(10), &mut task).await {
            Ok(result) => result.unwrap(),
            Err(_) => {
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
                panic!("deletion daemon failed to stop");
            }
        }
    }

    #[tokio::test]
    async fn deletion_repair_preview_reports_current_dirty_and_unmerged_loss() {
        let (_dir, registry, identity, wt) = fixture();
        let app = tauri::test::mock_builder()
            .manage(registry)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let request = WorktreeStatusRequest {
            workspace_id: "repair".into(),
            worktree: identity,
        };
        let clean = cmd_worktree_delete_preview(app.state(), request.clone())
            .await
            .unwrap();
        assert!(!clean.merged);
        let clean = serde_json::to_value(clean).unwrap();
        assert_eq!(
            clean["dirtyState"]["isDirty"], false,
            "preview must include current dirty state"
        );
        assert_eq!(clean["missing"], false);
        std::fs::write(wt.path.join("repair-untracked.txt"), b"current loss").unwrap();
        let dirty = serde_json::to_value(
            cmd_worktree_delete_preview(app.state(), request)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(dirty["merged"], false);
        assert_eq!(dirty["dirtyState"]["isDirty"], true);
        assert_eq!(
            dirty["dirtyState"]["files"],
            serde_json::json!([{ "statusCode": "??", "path": "repair-untracked.txt" }])
        );
    }

    #[tokio::test]
    async fn deletion_repair_missing_record_preview_and_targeted_cleanup() {
        let (_dir, registry, identity, wt) = fixture();
        let manager = registry.manager("repair").unwrap();
        let other = manager.worktree_path_for("repair", "other").unwrap();
        manager
            .create_worktree(CreateWorktreeOptions::new("repair", "other", &other))
            .unwrap();
        let outside = manager.repo_root().parent().unwrap().join("outside");
        run_git(
            manager.repo_root(),
            &[
                "worktree",
                "add",
                "--detach",
                outside.to_str().unwrap(),
                "HEAD",
            ],
        )
        .unwrap();
        for path in [&wt.path, &other, &outside] {
            std::fs::remove_dir_all(path).unwrap();
        }
        let (client, stop, daemon) = deletion_daemon(_dir.path(), &registry).await;
        let app = tauri::test::mock_builder()
            .manage(client)
            .manage(registry)
            .manage(WorktreeDiskScans::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let preview = cmd_worktree_delete_preview(
            app.state(),
            WorktreeStatusRequest {
                workspace_id: "repair".into(),
                worktree: identity.clone(),
            },
        )
        .await;
        assert!(
            preview.is_ok(),
            "missing managed Git record must resolve for preview: {preview:?}"
        );
        assert_eq!(
            serde_json::to_value(preview.unwrap()).unwrap()["missing"],
            true
        );
        cmd_worktree_delete_destructive(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: "repair".into(),
                worktree: identity,
                delete_branch: Some(true),
            },
        )
        .await
        .unwrap();
        let records = crate::worktree::git_worktree_list(manager.repo_root()).unwrap();
        assert!(!records.iter().any(|r| r.path == wt.path));
        assert!(records.iter().any(|r| r.path == other));
        assert!(records.iter().any(|r| r.path == outside));
        assert!(manager.canonical_allowed_path(&outside).is_err());
        assert!(manager
            .find_worktree_by_slug("repair", "other")
            .unwrap()
            .is_none());
        stop_deletion_daemon(stop, daemon).await;
    }

    #[tokio::test]
    async fn deletion_repair_success_removes_cached_row_and_blocks_stale_worker() {
        let (_dir, registry, identity, wt) = fixture();
        let scans = WorktreeDiskScans::default();
        let row = crate::worktree::disk::WorktreeDiskRow {
            worktree: wt.clone(),
            size_bytes: Some(1),
            last_commit_at: None,
            is_dirty: Some(false),
            dirty_files: vec![],
            error: None,
        };
        let (initial, _) = scans.begin("repair", false);
        scans.finish("repair", &initial.scan_id, Ok(vec![row.clone()]));
        let (worker, _) = scans.begin("repair", true);
        let (client, stop, daemon) = deletion_daemon(_dir.path(), &registry).await;
        let app = tauri::test::mock_builder()
            .manage(client)
            .manage(registry)
            .manage(scans.clone())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let (scan_event_tx, mut scan_event_rx) = tokio::sync::mpsc::unbounded_channel();
        app.listen(
            crate::ipc::worktree_disk::WORKTREE_DISK_SCAN_PROGRESS_EVENT,
            move |event: tauri::Event| {
                let snapshot: crate::ipc::worktree_disk::DiskScanSnapshot =
                    serde_json::from_str(event.payload()).unwrap();
                scan_event_tx.send(snapshot).unwrap();
            },
        );
        cmd_worktree_delete_destructive(
            app.handle().clone(),
            app.state(),
            DeleteWorktreeRequest {
                workspace_id: "repair".into(),
                worktree: identity,
                delete_branch: Some(true),
            },
        )
        .await
        .unwrap();
        let cancelled_event =
            tokio::time::timeout(std::time::Duration::from_secs(5), scan_event_rx.recv())
                .await
                .expect("delete must emit scan cancellation event")
                .expect("event received");
        assert_eq!(
            cancelled_event.status,
            crate::ipc::worktree_disk::DiskScanStatus::Cancelled
        );
        assert_eq!(cancelled_event.scan_id, worker.scan_id);
        assert!(!wt.path.exists());
        assert!(
            scans
                .finish("repair", &worker.scan_id, Ok(vec![row]))
                .is_none(),
            "pre-delete worker must not resurrect deleted row"
        );
        assert!(scans.result("repair").unwrap().rows.is_empty());
        let (cached, _) = scans.begin("repair", false);
        assert!(cached.rows.is_empty(), "reopen must not return deleted row");
        stop_deletion_daemon(stop, daemon).await;
    }
}

#[cfg(all(test, feature = "desktop"))]
mod change_event_tests {
    use super::*;
    use crate::worktree::{run_git, CreateWorktreeOptions};
    use tauri::Listener;

    #[tokio::test]
    async fn stage_unstage_and_push_emit_worktree_changed() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        run_git(&repo, &["init", "-q", "-b", "main"]).unwrap();
        run_git(&repo, &["config", "user.name", "Event fixture"]).unwrap();
        run_git(&repo, &["config", "user.email", "fixture@example.invalid"]).unwrap();
        run_git(&repo, &["config", "commit.gpgsign", "false"]).unwrap();
        run_git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]).unwrap();
        run_git(dir.path(), &["init", "-q", "--bare", "remote.git"]).unwrap();
        let bare = dir.path().join("remote.git");
        run_git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]).unwrap();
        let registry = WorkspaceRegistry::new();
        registry.register("events", &repo).unwrap();
        let manager = registry.manager("events").unwrap();
        let identity = WorktreeIdentity { ws_id: "events".into(), slug: "wt".into() };
        let path = manager.worktree_path_for("events", "wt").unwrap();
        let wt = manager
            .create_worktree(CreateWorktreeOptions::new("events", "wt", path))
            .unwrap();
        std::fs::write(wt.path.join("a.txt"), b"a").unwrap();

        let app = tauri::test::mock_builder()
            .manage(registry)
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.listen(WORKTREE_CHANGED_EVENT, move |event: tauri::Event| {
            let payload: WorktreeChangedPayload = serde_json::from_str(event.payload()).unwrap();
            tx.send(payload).unwrap();
        });
        let mut next = || {
            rx.try_recv().expect("command must emit worktree_changed before returning")
        };
        let paths = WorktreePathsRequest {
            workspace_id: "events".into(),
            worktree: identity.clone(),
            paths: vec!["a.txt".into()],
        };

        cmd_worktree_stage(app.handle().clone(), app.state(), paths.clone()).await.unwrap();
        let staged = next();
        assert_eq!(staged.kind, WorktreeChangeKind::DirtyChanged);
        assert_eq!(staged.workspace_id, "events");
        assert_eq!(staged.worktree, identity);

        cmd_worktree_unstage(app.handle().clone(), app.state(), paths.clone()).await.unwrap();
        assert_eq!(next().kind, WorktreeChangeKind::DirtyChanged);

        cmd_worktree_stage(app.handle().clone(), app.state(), paths).await.unwrap();
        next();
        cmd_worktree_commit(
            app.handle().clone(),
            app.state(),
            WorktreeCommitRequest {
                workspace_id: "events".into(),
                worktree: identity.clone(),
                message: "a".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(next().kind, WorktreeChangeKind::Updated);

        cmd_worktree_push(
            app.handle().clone(),
            app.state(),
            WorktreeStatusRequest { workspace_id: "events".into(), worktree: identity },
        )
        .await
        .unwrap();
        assert_eq!(next().kind, WorktreeChangeKind::Updated);
    }
}
