pub mod ipc;
pub mod terminal;
pub mod worktree;

use ipc::*;
use std::sync::Arc;
use terminal::PtyManager;
use worktree::WorkspaceRegistry;

pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let pty_manager = Arc::new(PtyManager::new());
    let workspace_registry = WorkspaceRegistry::new();

    if let Ok(repo_root) = std::env::current_dir() {
        if let Err(error) = workspace_registry.register("default", &repo_root) {
            tracing::debug!(
                "Current directory is not a registrable Orca workspace ({}): {}",
                repo_root.display(),
                error
            );
        }
    }

    builder
        .manage(pty_manager)
        .manage(workspace_registry)
        .invoke_handler(tauri::generate_handler![
            cmd_terminal_spawn,
            cmd_terminal_write,
            cmd_terminal_resize,
            cmd_terminal_signal,
            cmd_terminal_close,
            cmd_terminal_list,
            cmd_worktree_list,
            cmd_worktree_create,
            cmd_worktree_delete,
            cmd_worktree_delete_destructive,
            cmd_worktree_delete_preview,
            cmd_worktree_status,
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    create_app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
