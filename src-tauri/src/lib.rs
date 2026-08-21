pub mod browser;
pub mod ipc;
pub mod notification;
pub mod remote;
pub mod session;
pub mod terminal;
pub mod worktree;

use ipc::*;
use crate::ipc::remote::RemoteGatewayManager;
use remote::RemoteGatewayState;
use std::sync::Arc;
use terminal::{PtyManager, TerminalOutputHub, TerminalService};
use worktree::WorkspaceRegistry;
use notification::audio::NotificationAudioPlayer;

#[allow(dependency_on_unit_never_type_fallback)]
pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let pty_manager = Arc::new(PtyManager::new());
    let output_hub = Arc::new(TerminalOutputHub::default());
    let terminal_service = Arc::new(TerminalService::new(Arc::clone(&pty_manager), Arc::clone(&output_hub)));
    let workspace_registry = WorkspaceRegistry::new();
    let remote_state = Arc::new(RemoteGatewayState::new(Arc::clone(&terminal_service), workspace_registry.clone()));
    let remote_manager = Arc::new(RemoteGatewayManager::new(Arc::clone(&remote_state)));
    // Lazily initialized: a machine with no audio output device must still launch.
    let notification_audio = Arc::new(NotificationAudioPlayer::new());
    let browser_manager = Arc::new(browser::BrowserManager::new());

    if let Ok(repo_root) = std::env::current_dir() {
        if let Err(error) = workspace_registry.register("default", &repo_root) {
            tracing::debug!(
                "Current directory is not a registrable rorca workspace ({}): {}",
                repo_root.display(),
                error
            );
        }
    }

    builder
        .setup(|_app| {
            Ok(())
        })
        // Rust-side plugins only. The frontend uses rorca's own typed commands,
        // so no broad JavaScript guest capability needs to be granted.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pty_manager)
        .manage(output_hub)
        .manage(terminal_service)
        .manage(remote_manager)
        .manage(workspace_registry)
        .manage(notification_audio)
        .manage(browser_manager)
        .invoke_handler(tauri::generate_handler![
            cmd_terminal_spawn,
            cmd_terminal_write,
            cmd_terminal_resize,
            cmd_terminal_signal,
            cmd_terminal_close,
            cmd_terminal_list,
            cmd_terminal_preferences,
            cmd_remote_status,
            cmd_remote_enable,
            cmd_remote_disable,
            cmd_remote_pairing_create,
            cmd_remote_devices,
            cmd_remote_device_revoke,
            cmd_tailscale_status,
            cmd_project_register,
            cmd_project_branches,
            cmd_worktree_list,
            cmd_worktree_create,
            cmd_worktree_delete,
            cmd_worktree_delete_destructive,
            cmd_worktree_delete_preview,
            cmd_worktree_status,
            cmd_notification_dispatch,
            cmd_notification_get_permission_status,
            cmd_notification_request_permission,
            cmd_notification_probe_delivery,
            cmd_notification_open_system_settings,
            cmd_notification_play_sound,
            cmd_notification_pick_audio,
            cmd_session_save,
            cmd_session_load,
            cmd_session_clear,
            cmd_browser_create,
            cmd_browser_navigate,
            cmd_browser_reload,
            cmd_browser_set_bounds,
            cmd_browser_set_visible,
            cmd_browser_set_zoom,
            cmd_browser_focus,
            cmd_browser_get_state,
            cmd_browser_close,
            cmd_browser_list,
            cmd_browser_open_external,
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    create_app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
