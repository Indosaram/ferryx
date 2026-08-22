pub mod daemon;
pub mod browser;
pub mod ipc;
pub mod notification;
pub mod remote;
pub mod session;
pub mod terminal;
pub mod worktree;

use crate::ipc::remote::RemoteGatewayManager;
use ipc::*;
use notification::audio::NotificationAudioPlayer;
use remote::RemoteGatewayState;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use terminal::{PtyManager, TerminalOutputHub, TerminalService};
use worktree::WorkspaceRegistry;

#[cfg(desktop)]
fn install_app_menu<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

    let app_menu = SubmenuBuilder::new(app, "Ferryx")
        .about(Some(tauri::menu::AboutMetadata {
            name: Some("Ferryx".to_string()),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            ..Default::default()
        }))
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let new_terminal = MenuItemBuilder::with_id("tab.newTerminal", "New Terminal Tab")
        .accelerator("CmdOrCtrl+T")
        .build(app)?;
    let close_tab = MenuItemBuilder::with_id("tab.close", "Close Tab")
        .accelerator("CmdOrCtrl+W")
        .build(app)?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&new_terminal)
        .separator()
        .item(&close_tab)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let toggle_sidebar = MenuItemBuilder::with_id("sidebar.left.toggle", "Toggle Sidebar")
        .accelerator("CmdOrCtrl+B")
        .build(app)?;
    let command_palette = MenuItemBuilder::with_id("commandPalette.open", "Command Palette...")
        .accelerator("CmdOrCtrl+K")
        .build(app)?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&toggle_sidebar)
        .item(&command_palette)
        .separator()
        .fullscreen()
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .close_window()
        .build()?;

    let menu = Menu::default(app.handle())?;
    menu.append(&app_menu)?;
    menu.append(&file_menu)?;
    menu.append(&edit_menu)?;
    menu.append(&view_menu)?;
    menu.append(&window_menu)?;
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        let event_id = event.id().as_ref();
        if let Some(window) = app.get_webview_window("main") {
            match event_id {
                "tab.newTerminal" => {
                    let _ = window.emit("menu_new_terminal_tab", ());
                }
                "tab.close" => {
                    let _ = window.emit("menu_close_tab", ());
                }
                "sidebar.left.toggle" => {
                    let _ = window.emit("menu_toggle_sidebar", ());
                }
                "commandPalette.open" => {
                    let _ = window.emit("menu_command_palette", ());
                }
                _ => {}
            }
        }
    });
    Ok(())
}

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
                "Current directory is not a registrable ferryx workspace ({}): {}",
                repo_root.display(),
                error
            );
        }
    }

    builder
        .setup(|app| {
            #[cfg(desktop)]
            install_app_menu(app)?;
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
            cmd_terminal_get_cwd,
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
