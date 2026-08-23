pub mod browser;
pub mod daemon;
pub mod ipc;
pub mod notification;
pub mod remote;
pub mod session;
pub mod terminal;
pub mod worktree;

use crate::daemon::DaemonClient;
use ipc::*;
use notification::audio::NotificationAudioPlayer;
use std::sync::Arc;
use tauri::{Emitter, Manager};
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
    let close_window = MenuItemBuilder::with_id("window.close", "Close Window")
        .accelerator("CmdOrCtrl+Shift+W")
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
        .item(&close_window)
        .build()?;

    let menu = Menu::default(app.handle())?;
    menu.append(&app_menu)?;
    menu.append(&file_menu)?;
    menu.append(&edit_menu)?;
    menu.append(&view_menu)?;
    menu.append(&window_menu)?;
    app.set_menu(menu)?;

    app.on_menu_event(move |app, event| {
        let event_id = event.id().as_ref();
        if let Some(window) = app.get_webview_window("main") {
            match event_id {
                "tab.newTerminal" => {
                    let _ = window.emit("menu_new_terminal_tab", ());
                }
                "tab.close" => {
                    let _ = window.emit("menu_close_tab", ());
                }
                "window.close" => {
                    let _ = window.close();
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

#[cfg(target_os = "macos")]
pub fn is_unshifted_cmd_w_modifiers(flags: objc2_app_kit::NSEventModifierFlags) -> bool {
    use objc2_app_kit::NSEventModifierFlags;
    flags.contains(NSEventModifierFlags::Command)
        && !flags.contains(NSEventModifierFlags::Shift)
        && !flags.contains(NSEventModifierFlags::Control)
        && !flags.contains(NSEventModifierFlags::Option)
}

#[cfg(target_os = "macos")]
pub fn is_unshifted_cmd_w_characters(characters: Option<&str>) -> bool {
    characters.map_or(false, |chars| chars.eq_ignore_ascii_case("w"))
}

#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_W: u16 = 13;

#[cfg(target_os = "macos")]
pub fn is_unshifted_cmd_w(
    flags: objc2_app_kit::NSEventModifierFlags,
    characters: Option<&str>,
    key_code: u16,
) -> bool {
    is_unshifted_cmd_w_modifiers(flags)
        && (is_unshifted_cmd_w_characters(characters) || key_code == ANSI_KEY_CODE_W)
}

#[cfg(target_os = "macos")]
fn install_macos_key_monitor<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask};
    use std::ptr::{self, NonNull};

    let app_handle = app.handle().clone();
    let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        let flags = event.modifierFlags();
        let chars = event.charactersIgnoringModifiers();
        let chars_str = chars.as_deref().map(|s| s.to_string());
        let key_code = event.keyCode();
        if is_unshifted_cmd_w(flags, chars_str.as_deref(), key_code) {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.emit("menu_close_tab", ());
            }
            ptr::null_mut()
        } else {
            event_ptr.as_ptr()
        }
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }

    Ok(())
}

pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let daemon_client = Arc::new(DaemonClient::new());
    let remote_manager = Arc::new(ipc::remote::RemoteGatewayManager::from_daemon(Arc::clone(
        &daemon_client,
    )));
    let workspace_registry = WorkspaceRegistry::new();
    // Lazily initialized: a machine with no audio output device must still launch.
    let notification_audio = Arc::new(NotificationAudioPlayer::new());
    let browser_manager = Arc::new(browser::BrowserManager::new());

    // Exactly one canonical workspace is registered for the startup root; the
    // legacy `default` alias is intentionally not registered, and persisted
    // clients asking for it are resolved to this ID instead.
    if let Err(error) = ipc::project::initial_project(&workspace_registry) {
        tracing::debug!(
            "Current directory is not a registrable ferryx workspace: {}",
            error
        );
    }

    builder
        .setup(move |app| {
            #[cfg(desktop)]
            install_app_menu(app)?;
            #[cfg(target_os = "macos")]
            install_macos_key_monitor(app)?;
            Ok(())
        })
        // Rust-side plugins only. The frontend uses rorca's own typed commands,
        // so no broad JavaScript guest capability needs to be granted.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(daemon_client)
        .manage(remote_manager)
        .manage(workspace_registry)
        .manage(notification_audio)
        .manage(browser_manager)
        .invoke_handler(tauri::generate_handler![
            cmd_terminal_spawn,
            cmd_terminal_attach,
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
            cmd_remote_set_active_selection,
            cmd_remote_get_active_selection,
            cmd_tailscale_status,
            cmd_project_initial,
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
            cmd_notification_set_badge_count,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::DevicePermission;
    use crate::remote::RemoteGatewayState;
    use crate::terminal::TerminalService;
    use std::ffi::OsString;
    use std::sync::Mutex;

    static FERRYX_DATA_DIR_LOCK: Mutex<()> = Mutex::new(());

    struct RestoreFerryxDataDir(Option<OsString>);

    impl Drop for RestoreFerryxDataDir {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("FERRYX_DATA_DIR", value),
                None => std::env::remove_var("FERRYX_DATA_DIR"),
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_macos_cmd_w_shortcut_predicate_boundaries() {
        use objc2_app_kit::NSEventModifierFlags;

        // Accepted: unshifted Cmd+W (lowercase or uppercase)
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("W"),
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
            Some("W"),
            ANSI_KEY_CODE_W
        ));

        // Rejected: Cmd+Shift+W (Close Window shortcut)
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            Some("W"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command
                | NSEventModifierFlags::Shift
                | NSEventModifierFlags::CapsLock,
            Some("w"),
            ANSI_KEY_CODE_W
        ));

        // Rejected: Ctrl or Option variants
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Control,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Option,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command
                | NSEventModifierFlags::Shift
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Control,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Control,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Option,
            Some("w"),
            ANSI_KEY_CODE_W
        ));

        // Rejected: No Command modifier (plain 'w', Shift+W)
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::empty(),
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Shift,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Shift,
            Some("W"),
            ANSI_KEY_CODE_W
        ));

        // Rejected: Other keys with Command (Cmd+T, Cmd+Q, Cmd+B, etc.) with key code 0
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("t"),
            0
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("q"),
            0
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("b"),
            0
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some(""),
            0
        ));

        // Keycode fallback tests:
        // Accepted: key code 13 with None or CJK/Korean characters ("ㅈ" on 2-Set Korean keyboard)
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            None,
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("ㅈ"),
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("w"),
            ANSI_KEY_CODE_W
        ));
        assert!(is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
            None,
            ANSI_KEY_CODE_W
        ));

        // Rejected: key code 13 with Shift, Option, or Control
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            None,
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Option,
            None,
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command | NSEventModifierFlags::Control,
            None,
            ANSI_KEY_CODE_W
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::empty(),
            None,
            ANSI_KEY_CODE_W
        ));

        // Rejected: other keycodes without 'w' char (e.g. key code 0 / 'a', or Korean char with wrong key code)
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("a"),
            0
        ));
        assert!(!is_unshifted_cmd_w(
            NSEventModifierFlags::Command,
            Some("ㅈ"),
            0
        ));
        assert!(!is_unshifted_cmd_w(NSEventModifierFlags::Command, None, 0));
    }

    #[test]
    fn app_remote_state_persists_pairing_but_starts_off() {
        let _lock = FERRYX_DATA_DIR_LOCK.lock().expect("data dir lock");
        let _restore = RestoreFerryxDataDir(std::env::var_os("FERRYX_DATA_DIR"));
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::env::set_var("FERRYX_DATA_DIR", dir.path());
        let registry = WorkspaceRegistry::new();
        let terminal = Arc::new(TerminalService::default());
        let first = RemoteGatewayState::new_persistent(Arc::clone(&terminal), registry.clone());
        let code = first
            .auth_manager
            .create_pairing_code(DevicePermission::Control);
        first
            .auth_manager
            .exchange_pairing_code(&code, "Phone")
            .expect("pair");
        first.persist_config().expect("persist");

        let reopened = RemoteGatewayState::new_persistent(terminal, registry);
        assert_eq!(reopened.config.read().mode, remote::RemoteNetworkMode::Off);
        assert!(!*reopened.is_running.read());
        assert!(reopened.bound_address.read().is_none());
        assert_eq!(reopened.auth_manager.list_devices().len(), 1);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    create_app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
