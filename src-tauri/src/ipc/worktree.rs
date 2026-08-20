use crate::worktree::model::{CreateWorktreeOptions, DirtyState, Worktree};
use crate::worktree::WorktreeManager;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateWorktreeRequest {
    pub repo_root: Option<PathBuf>,
    pub ws_id: String,
    pub slug: String,
    pub path: PathBuf,
    pub base_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteWorktreeRequest {
    pub repo_root: Option<PathBuf>,
    pub path: PathBuf,
    pub delete_branch: Option<bool>,
}

#[tauri::command]
pub async fn cmd_worktree_list(
    default_manager: State<'_, WorktreeManager>,
    repo_root: Option<PathBuf>,
) -> Result<Vec<Worktree>, String> {
    let manager = if let Some(root) = repo_root {
        WorktreeManager::new(root)
    } else {
        (*default_manager).clone()
    };

    manager.list_worktrees().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_worktree_create(
    default_manager: State<'_, WorktreeManager>,
    request: CreateWorktreeRequest,
) -> Result<Worktree, String> {
    let manager = if let Some(root) = request.repo_root {
        WorktreeManager::new(root)
    } else {
        (*default_manager).clone()
    };

    let mut opts = CreateWorktreeOptions::new(request.ws_id, request.slug, request.path);
    if let Some(base) = request.base_ref {
        opts = opts.with_base_ref(base);
    }

    manager.create_worktree(opts).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_worktree_delete(
    default_manager: State<'_, WorktreeManager>,
    request: DeleteWorktreeRequest,
) -> Result<(), String> {
    let manager = if let Some(root) = request.repo_root {
        WorktreeManager::new(root)
    } else {
        (*default_manager).clone()
    };

    let delete_branch = request.delete_branch.unwrap_or(false);
    manager
        .delete_worktree_and_branch(&request.path, delete_branch)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_worktree_status(
    default_manager: State<'_, WorktreeManager>,
    path: PathBuf,
) -> Result<DirtyState, String> {
    default_manager.check_dirty(&path).map_err(|e| e.to_string())
}
