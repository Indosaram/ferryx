use crate::ipc::{run_blocking, IpcError};
use crate::worktree::{
    BranchDeletionPreview, CreateWorktreeOptions, DirtyState, WorkspaceRegistry, Worktree,
    WorktreeIdentity,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime, State};

pub const WORKTREE_CHANGED_EVENT: &str = "worktree_changed";

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChangedPayload {
    pub workspace_id: String,
    pub worktree: WorktreeIdentity,
    pub kind: WorktreeChangeKind,
}

fn emit_worktree_changed<R: Runtime>(
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

#[tauri::command]
pub async fn cmd_worktree_list(
    registry: State<'_, WorkspaceRegistry>,
    workspace_id: String,
) -> Result<Vec<Worktree>, IpcError> {
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

    let created = run_blocking(move || {
        let manager = registry.manager(&workspace_id).map_err(IpcError::from)?;
        let target = manager
            .worktree_path_for(&identity.ws_id, &identity.slug)
            .map_err(IpcError::from)?;
        let mut options =
            CreateWorktreeOptions::new(identity.ws_id.clone(), identity.slug.clone(), target);
        if let Some(base_ref) = request.base_ref {
            options = options.with_base_ref(base_ref);
        }
        manager.create_worktree(options).map_err(IpcError::from)
    })
    .await?;

    emit_worktree_changed(
        &app,
        event_workspace_id,
        event_identity,
        WorktreeChangeKind::Created,
    )?;
    Ok(created)
}

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

    let pruned = run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_worktree(&workspace_id, &identity)
            .map_err(IpcError::from)?;
        manager
            .delete_worktree_and_branch_with_prune_status(
                &worktree.path,
                delete_branch,
                destructive,
            )
            .map_err(IpcError::from)
    })
    .await?;

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

#[tauri::command]
pub async fn cmd_worktree_delete<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, false).await
}

#[tauri::command]
pub async fn cmd_worktree_delete_destructive<R: Runtime>(
    app: AppHandle<R>,
    registry: State<'_, WorkspaceRegistry>,
    request: DeleteWorktreeRequest,
) -> Result<(), IpcError> {
    delete_worktree(app, registry, request, true).await
}

#[tauri::command]
pub async fn cmd_worktree_delete_preview(
    registry: State<'_, WorkspaceRegistry>,
    request: WorktreeStatusRequest,
) -> Result<BranchDeletionPreview, IpcError> {
    let registry = (*registry).clone();
    run_blocking(move || {
        let (manager, worktree) = registry
            .resolve_worktree(&request.workspace_id, &request.worktree)
            .map_err(IpcError::from)?;
        manager
            .branch_deletion_preview(&worktree.path)
            .map_err(IpcError::from)
    })
    .await
}

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
