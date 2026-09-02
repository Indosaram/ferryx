pub mod agent_detect;
pub mod browser;
pub mod daemon;
pub mod dag;
pub mod ipc;
#[cfg(feature = "native-terminal")]
pub mod native_terminal;
pub mod notification;
pub mod remote;
pub mod ssh;
pub mod session;
pub mod terminal;
pub mod util;
pub mod worktree;

use crate::daemon::DaemonClient;
#[cfg(all(target_os = "windows", feature = "native-terminal"))]
use crate::native_terminal::platform::windows_focus::install_windows_terminal_focus_monitor;
#[cfg(feature = "native-terminal")]
use crate::native_terminal::surface_host::NativeTerminalSurfaceHostState;
#[cfg(all(target_os = "macos", feature = "native-terminal"))]
use crate::native_terminal::surface_host::NATIVE_TERMINAL_FOCUS_EVENT;
use ipc::*;
use notification::audio::NotificationAudioPlayer;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use worktree::WorkspaceRegistry;

#[cfg(target_os = "macos")]
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

    let close_window = MenuItemBuilder::with_id("window.close", "Close Window")
        .accelerator("CmdOrCtrl+Shift+W")
        .build(app)?;

    let view_menu = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .item(&close_window)
        .build()?;

    let menu = Menu::new(app.handle())?;
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
pub const ANSI_KEY_CODE_C: u16 = 8;

#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_W: u16 = 13;

#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_V: u16 = 9;

#[cfg(target_os = "macos")]
pub const NATIVE_TERMINAL_COPY_OR_INTERRUPT_EVENT: &str = "native_terminal_copy_or_interrupt";

#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_1: u16 = 18;
#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_5: u16 = 23;
#[cfg(target_os = "macos")]
pub const ANSI_KEY_CODE_9: u16 = 25;

/// macOS keyCodes for the top-row digits 1..9, in order.
#[cfg(target_os = "macos")]
const ANSI_DIGIT_KEY_CODES: [u16; 9] = [18, 19, 20, 21, 23, 22, 26, 28, 25];

/// Returns the pressed digit when the event is an unshifted Cmd+1..9.
///
/// The standard macOS Window menu owns Cmd+digit, so AppKit consumes those
/// events and the webview never receives a `keydown`. The local key monitor uses
/// this to claim them for worktree selection, exactly as it already does for
/// unshifted Cmd+W.
#[cfg(target_os = "macos")]
pub fn unshifted_cmd_digit(
    flags: objc2_app_kit::NSEventModifierFlags,
    characters: Option<&str>,
    key_code: u16,
) -> Option<u8> {
    if !is_unshifted_cmd_w_modifiers(flags) {
        return None;
    }

    if let Some(digit) = characters
        .and_then(|chars| chars.chars().next())
        .and_then(|char| char.to_digit(10))
    {
        if (1..=9).contains(&digit) {
            return Some(digit as u8);
        }
    }

    ANSI_DIGIT_KEY_CODES
        .iter()
        .position(|candidate| *candidate == key_code)
        .map(|index| index as u8 + 1)
}

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
pub fn is_unshifted_cmd_v(
    flags: objc2_app_kit::NSEventModifierFlags,
    characters: Option<&str>,
    key_code: u16,
) -> bool {
    is_unshifted_cmd_w_modifiers(flags)
        && (characters.is_some_and(|chars| chars.eq_ignore_ascii_case("v"))
            || key_code == ANSI_KEY_CODE_V)
}

#[cfg(target_os = "macos")]
pub fn is_unshifted_cmd_c(
    flags: objc2_app_kit::NSEventModifierFlags,
    characters: Option<&str>,
    key_code: u16,
) -> bool {
    is_unshifted_cmd_w_modifiers(flags)
        && (characters.is_some_and(|chars| chars.eq_ignore_ascii_case("c"))
            || key_code == ANSI_KEY_CODE_C)
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTerminalPasteAction {
    EmitAndConsume,
    ConsumeOnly,
    PassThrough,
}

#[cfg(target_os = "macos")]
pub const fn native_key_event_has_characters(event_type: objc2_app_kit::NSEventType) -> bool {
    matches!(
        event_type,
        objc2_app_kit::NSEventType::KeyDown | objc2_app_kit::NSEventType::KeyUp
    )
}

#[cfg(target_os = "macos")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NativeTerminalPasteLatch {
    latched: bool,
}

#[cfg(target_os = "macos")]
impl NativeTerminalPasteLatch {
    pub const fn new() -> Self {
        Self { latched: false }
    }

    pub fn is_latched(&self) -> bool {
        self.latched
    }

    pub fn handle_event(
        &mut self,
        event_type: objc2_app_kit::NSEventType,
        flags: objc2_app_kit::NSEventModifierFlags,
        characters: Option<&str>,
        key_code: u16,
        has_focused_terminal: bool,
    ) -> NativeTerminalPasteAction {
        match event_type {
            objc2_app_kit::NSEventType::KeyDown => {
                if !has_focused_terminal || !is_unshifted_cmd_v(flags, characters, key_code) {
                    NativeTerminalPasteAction::PassThrough
                } else if self.latched {
                    NativeTerminalPasteAction::ConsumeOnly
                } else {
                    self.latched = true;
                    NativeTerminalPasteAction::EmitAndConsume
                }
            }
            objc2_app_kit::NSEventType::KeyUp => {
                if key_code == ANSI_KEY_CODE_V
                    || characters.is_some_and(|chars| chars.eq_ignore_ascii_case("v"))
                {
                    self.latched = false;
                }
                NativeTerminalPasteAction::PassThrough
            }
            objc2_app_kit::NSEventType::FlagsChanged => {
                if !flags.contains(objc2_app_kit::NSEventModifierFlags::Command) {
                    self.latched = false;
                }
                NativeTerminalPasteAction::PassThrough
            }
            _ => NativeTerminalPasteAction::PassThrough,
        }
    }
}

#[cfg(target_os = "macos")]
fn install_macos_key_monitor<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventType};
    use std::cell::Cell;
    use std::ptr::{self, NonNull};

    let app_handle = app.handle().clone();
    #[cfg(feature = "native-terminal")]
    let surface_host = app
        .state::<NativeTerminalSurfaceHostState>()
        .inner()
        .clone();
    let paste_latch = Cell::new(NativeTerminalPasteLatch::new());
    let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        let event_type = event.r#type();
        let flags = event.modifierFlags();
        let chars = if native_key_event_has_characters(event_type) {
            event.charactersIgnoringModifiers()
        } else {
            None
        };
        let chars_str = chars.as_deref().map(|s| s.to_string());
        let key_code = event.keyCode();
        let worktree_digit = if event_type == NSEventType::KeyDown {
            unshifted_cmd_digit(flags, chars_str.as_deref(), key_code)
        } else {
            None
        };

        if event_type == NSEventType::KeyDown
            && is_unshifted_cmd_w(flags, chars_str.as_deref(), key_code)
        {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.emit("menu_close_tab", ());
            }
            ptr::null_mut()
        } else if let Some(digit) = worktree_digit {
            if let Some(window) = app_handle.get_webview_window("main") {
                let _ = window.emit("menu_select_worktree", digit);
            }
            ptr::null_mut()
        } else {
            let has_focused_terminal = {
                #[cfg(feature = "native-terminal")]
                {
                    surface_host.has_focused_session()
                }
                #[cfg(not(feature = "native-terminal"))]
                {
                    false
                }
            };
            if event_type == NSEventType::KeyDown
                && is_unshifted_cmd_c(flags, chars_str.as_deref(), key_code)
            {
                if cfg!(debug_assertions)
                    || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1")
                {
                    use std::io::Write;
                    let action_str = if has_focused_terminal {
                        "EmitAndConsume"
                    } else {
                        "PassThrough"
                    };
                    let wall_time_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let record = serde_json::json!({
                        "runId": "rust-monitor",
                        "sequence": 0,
                        "event": "terminal.surface.copy_or_interrupt.monitor",
                        "wallTimeMs": wall_time_ms,
                        "details": {
                            "action": action_str,
                            "hasFocusedTerminal": has_focused_terminal,
                            "keyCode": key_code,
                            "characters": chars_str.as_deref(),
                        }
                    });
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/ferryx-switch-debug.jsonl")
                    {
                        let _ = writeln!(file, "{}", record);
                    }
                }
                if has_focused_terminal {
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit(NATIVE_TERMINAL_COPY_OR_INTERRUPT_EVENT, ());
                    }
                    ptr::null_mut()
                } else {
                    event_ptr.as_ptr()
                }
            } else {
                let mut latch = paste_latch.get();
                let action = latch.handle_event(
                    event_type,
                    flags,
                    chars_str.as_deref(),
                    key_code,
                    has_focused_terminal,
                );
                paste_latch.set(latch);
                if (cfg!(debug_assertions)
                    || std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref() == Some("1"))
                    && event_type == NSEventType::KeyDown
                    && is_unshifted_cmd_v(flags, chars_str.as_deref(), key_code)
                {
                    use std::io::Write;
                    let action_str = match action {
                        NativeTerminalPasteAction::EmitAndConsume => "EmitAndConsume",
                        NativeTerminalPasteAction::ConsumeOnly => "ConsumeOnly",
                        NativeTerminalPasteAction::PassThrough => "PassThrough",
                    };
                    let wall_time_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let record = serde_json::json!({
                        "runId": "rust-monitor",
                        "sequence": 0,
                        "event": "terminal.surface.paste.monitor",
                        "wallTimeMs": wall_time_ms,
                        "details": {
                            "action": action_str,
                            "hasFocusedTerminal": has_focused_terminal,
                            "keyCode": key_code,
                            "characters": chars_str.as_deref(),
                        }
                    });
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/ferryx-switch-debug.jsonl")
                    {
                        let _ = writeln!(file, "{}", record);
                    }
                }
                match action {
                    NativeTerminalPasteAction::EmitAndConsume => {
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let _ = window.emit("native_terminal_paste", ());
                        }
                        ptr::null_mut()
                    }
                    NativeTerminalPasteAction::ConsumeOnly => ptr::null_mut(),
                    NativeTerminalPasteAction::PassThrough => event_ptr.as_ptr(),
                }
            }
        }
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::KeyDown | NSEventMask::KeyUp | NSEventMask::FlagsChanged,
            &block,
        )
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }

    Ok(())
}

#[cfg(all(target_os = "macos", feature = "native-terminal"))]
fn install_macos_terminal_focus_monitor<R: tauri::Runtime>(
    app: &tauri::App<R>,
) -> tauri::Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSWindow};
    use std::ptr::NonNull;

    let app_handle = app.handle().clone();
    let surface_host = app
        .state::<NativeTerminalSurfaceHostState>()
        .inner()
        .clone();
    let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(raw_window) = window.ns_window() {
                if !raw_window.is_null() {
                    let ns_window = unsafe { &*(raw_window as *const NSWindow) };
                    if event.windowNumber() == ns_window.windowNumber() {
                        if let Some(content_view) = ns_window.contentView() {
                            let location = event.locationInWindow();
                            let content_height = content_view.bounds().size.height;
                            let logical_x = location.x;
                            let logical_y = content_height - location.y;
                            if let Some(session_id) =
                                surface_host.session_at_logical_point(logical_x, logical_y)
                            {
                                let _ = window.emit(NATIVE_TERMINAL_FOCUS_EVENT, session_id);
                            }
                        }
                    }
                }
            }
        }
        event_ptr.as_ptr()
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseUp, &block)
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }

    Ok(())
}

#[cfg(all(target_os = "macos", feature = "native-terminal"))]
fn install_macos_terminal_scroll_monitor<R: tauri::Runtime>(
    app: &tauri::App<R>,
) -> tauri::Result<()> {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSWindow};
    use std::ptr::{self, NonNull};

    let app_handle = app.handle().clone();
    let surface_host = app
        .state::<NativeTerminalSurfaceHostState>()
        .inner()
        .clone();
    let block = RcBlock::new(move |event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        if let Some(window) = app_handle.get_webview_window("main") {
            if let Ok(raw_window) = window.ns_window() {
                if !raw_window.is_null() {
                    let ns_window = unsafe { &*(raw_window as *const NSWindow) };
                    if event.windowNumber() == ns_window.windowNumber() {
                        if let Some(content_view) = ns_window.contentView() {
                            let location = event.locationInWindow();
                            let content_height = content_view.bounds().size.height;
                            let logical_x = location.x;
                            let logical_y = content_height - location.y;
                            if let Some(session_id) =
                                surface_host.session_at_logical_point(logical_x, logical_y)
                            {
                                let delta_y = event.scrollingDeltaY();
                                let has_precise = event.hasPreciseScrollingDeltas();
                                let rows = crate::native_terminal::macos_wheel_scroll_rows(
                                    delta_y,
                                    has_precise,
                                );
                                if rows != 0 {
                                    if ipc::native_terminal::scroll_attached_native_terminal(
                                        &surface_host,
                                        &session_id,
                                        crate::native_terminal::ScrollViewport::Delta(rows as isize),
                                    )
                                    .is_ok()
                                    {
                                        let surface_window = window.clone();
                                        let state_inner = surface_host.clone();
                                        let session_id_clone = session_id.clone();
                                        let bounds =
                                            surface_host.session_logical_bounds(&session_id);
                                        let _ = window.run_on_main_thread(move || {
                                            match bounds {
                                                Some(logical_bounds) => {
                                                    let _ = state_inner.render(
                                                        &surface_window,
                                                        crate::native_terminal::surface_host::NativeTerminalBoundsRequest {
                                                            session_id: session_id_clone,
                                                            bounds: logical_bounds,
                                                        },
                                                    );
                                                }
                                                None => {
                                                    let _ = state_inner.get_receipt(
                                                        &surface_window,
                                                        &session_id_clone,
                                                    );
                                                }
                                            }
                                        });
                                    }
                                }
                                return ptr::null_mut();
                            }
                        }
                    }
                }
            }
        }
        event_ptr.as_ptr()
    });

    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::ScrollWheel, &block)
    };
    if let Some(monitor) = monitor {
        std::mem::forget(monitor);
    }

    Ok(())
}

/// Relays desktop-directed remote gateway events from the daemon (which owns the
/// gateway) into Tauri events the frontend already listens for. Without this the
/// remote client's selection requests never reach the desktop.
fn start_remote_event_bridge<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon_client: Arc<DaemonClient>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            match daemon_client.subscribe_remote_events().await {
                Ok(mut events) => {
                    while let Some(event) = events.recv().await {
                        if let Err(error) = app.emit(event.event.as_str(), event.payload) {
                            tracing::debug!("Failed to relay remote event: {error}");
                        }
                    }
                }
                Err(error) => {
                    tracing::debug!("Remote event subscription unavailable: {error}");
                }
            }
            // The daemon restarts independently of the GUI; resubscribe instead
            // of leaving remote selections permanently unroutable.
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });
}

pub fn create_app<R: tauri::Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    let daemon_client = Arc::new(DaemonClient::new());
    let bridge_daemon_client = Arc::clone(&daemon_client);
    let remote_manager = Arc::new(ipc::remote::RemoteGatewayManager::from_daemon(Arc::clone(
        &daemon_client,
    )));
    let workspace_registry = WorkspaceRegistry::new();
    // Lazily initialized: a machine with no audio output device must still launch.
    let notification_audio = Arc::new(NotificationAudioPlayer::new());
    let browser_manager = Arc::new(browser::BrowserManager::new());
    let browser_cli_manager = Arc::clone(&browser_manager);
    #[cfg(feature = "native-terminal")]
    let native_terminal_surface_host = NativeTerminalSurfaceHostState::default();

    // Exactly one canonical workspace is registered for the startup root; the
    // legacy `default` alias is intentionally not registered, and persisted
    // clients asking for it are resolved to this ID instead.
    if let Err(error) = ipc::project::initial_project(&workspace_registry) {
        tracing::debug!(
            "Current directory is not a registrable ferryx workspace: {}",
            error
        );
    }

    let builder = builder
        .on_window_event(|window, event| {
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::DragDrop(drag_event) = event {
                if crate::ipc::debug::switch_debug_sink_enabled(
                    cfg!(debug_assertions),
                    std::env::var("FERRYX_SWITCH_DEBUG").ok().as_deref(),
                ) {
                    let wall_time_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    let details = match drag_event {
                        tauri::DragDropEvent::Enter { paths, position } => serde_json::json!({
                            "label": window.label(),
                            "kind": "enter",
                            "pathCount": paths.len(),
                            "paths": paths,
                            "position": { "x": position.x, "y": position.y },
                        }),
                        tauri::DragDropEvent::Over { position } => serde_json::json!({
                            "label": window.label(),
                            "kind": "over",
                            "position": { "x": position.x, "y": position.y },
                        }),
                        tauri::DragDropEvent::Drop { paths, position } => serde_json::json!({
                            "label": window.label(),
                            "kind": "drop",
                            "pathCount": paths.len(),
                            "paths": paths,
                            "position": { "x": position.x, "y": position.y },
                        }),
                        tauri::DragDropEvent::Leave => serde_json::json!({
                            "label": window.label(),
                            "kind": "leave",
                        }),
                        _ => serde_json::json!({
                            "label": window.label(),
                            "kind": "unknown",
                        }),
                    };
                    let record = serde_json::json!({
                        "runId": "rust-monitor",
                        "sequence": 0,
                        "event": "terminal.surface.dragdrop.window_event",
                        "wallTimeMs": wall_time_ms,
                        "details": details
                    });
                    let _ = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/tmp/ferryx-switch-debug.jsonl")
                        .and_then(|mut file| std::io::Write::write_all(&mut file, format!("{record}\n").as_bytes()));
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            install_app_menu(app)?;
            #[cfg(target_os = "macos")]
            install_macos_key_monitor(app)?;
            #[cfg(all(target_os = "macos", feature = "native-terminal"))]
            install_macos_terminal_focus_monitor(app)?;
            #[cfg(all(target_os = "macos", feature = "native-terminal"))]
            install_macos_terminal_scroll_monitor(app)?;
            #[cfg(all(target_os = "windows", feature = "native-terminal"))]
            install_windows_terminal_focus_monitor(app)?;
            ipc::browser_cli::start_browser_cli_server(
                app.handle().clone(),
                Arc::clone(&browser_cli_manager),
            )?;
            start_remote_event_bridge(app.handle().clone(), Arc::clone(&bridge_daemon_client));
            Ok(())
        })
        // Rust-side plugins only. The frontend uses rorca's own typed commands,
        // so no broad JavaScript guest capability needs to be granted.
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(daemon_client)
        .manage(remote_manager)
        .manage(workspace_registry)
        .manage(notification_audio)
        .manage(browser_manager);

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());

    #[cfg(feature = "native-terminal")]
    let builder = builder.manage(native_terminal_surface_host);

    builder.invoke_handler(tauri::generate_handler![
        cmd_switch_debug_log,
        cmd_terminal_output_channel,
        cmd_terminal_spawn,
        cmd_terminal_attach,
        cmd_terminal_get_cwd,
        cmd_terminal_write,
        cmd_terminal_resize,
        cmd_terminal_signal,
        cmd_terminal_close,
        cmd_terminal_list,
        cmd_terminal_preferences,
        cmd_terminal_apply_overrides,
        cmd_native_terminal_attach,
        cmd_native_terminal_detach,
        cmd_native_terminal_close,
        cmd_native_terminal_set_bounds,
        cmd_native_terminal_set_focus,
        cmd_native_terminal_set_preedit,
        cmd_native_terminal_send_input,
        cmd_native_terminal_scroll,
        cmd_native_terminal_scrollbar,
        cmd_native_terminal_set_scrollbar_overlay,
        cmd_native_terminal_select,
        cmd_native_terminal_copy_selection,
        cmd_native_terminal_paste,
        cmd_native_terminal_clipboard_content,
        cmd_native_terminal_mouse,
        cmd_native_terminal_search,
        cmd_remote_status,
        cmd_remote_enable,
        cmd_remote_disable,
        cmd_remote_pairing_create,
        cmd_remote_devices,
        cmd_remote_device_revoke,
        cmd_remote_set_active_selection,
        cmd_remote_get_active_selection,
        cmd_project_initial,
        cmd_boot_trace,
        cmd_project_register,
        ipc::ssh::cmd_ssh_list_hosts,
        ipc::ssh::cmd_ssh_import_config,
        ipc::ssh::cmd_ssh_update_host,
        ipc::ssh::cmd_ssh_delete_host,
        ipc::ssh::cmd_ssh_test_connection,
        ipc::ssh::cmd_ssh_list_remote_worktrees,
        ipc::ssh::cmd_ssh_create_remote_worktree,
        ipc::ssh::cmd_ssh_delete_remote_worktree,
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
        cmd_cli_launcher_status,
        cmd_cli_launcher_install,
        cmd_agents_detect,
        cmd_agent_session_discover,
        cmd_browser_create,
        cmd_browser_navigate,
        cmd_browser_go_back,
        cmd_browser_go_forward,
        cmd_browser_import_cookies,
        cmd_browser_reload,
        cmd_browser_set_bounds,
        cmd_browser_set_visible,
        cmd_browser_set_zoom,
        cmd_browser_focus,
        cmd_browser_get_state,
        cmd_browser_find,
        cmd_browser_clear_find,
        cmd_browser_download,
        cmd_browser_automation_snapshot,
        cmd_browser_automation_act,
        cmd_browser_close,
        cmd_browser_list,
        cmd_browser_open_external,
        dag_list_runs,
        dag_get_run,
        dag_watch_project,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::client::DaemonClient;
    use crate::remote::auth::DevicePermission;
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
    fn cmd_digit_is_claimed_for_worktree_selection() {
        use objc2_app_kit::NSEventModifierFlags;

        // The standard macOS Window menu claims Cmd+1..9, so AppKit swallows the
        // keystroke before the webview sees a keydown. The local monitor must
        // recognize it and hand it to the frontend instead.
        for (digit, key_code) in [
            (1u8, ANSI_KEY_CODE_1),
            (5u8, ANSI_KEY_CODE_5),
            (9u8, ANSI_KEY_CODE_9),
        ] {
            let label = digit.to_string();
            assert_eq!(
                unshifted_cmd_digit(
                    NSEventModifierFlags::Command,
                    Some(label.as_str()),
                    key_code,
                ),
                Some(digit),
                "Cmd+{digit} must map to worktree index {digit}"
            );
        }

        // Rejected: any extra modifier, or a non-digit key.
        assert_eq!(
            unshifted_cmd_digit(
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
                Some("1"),
                ANSI_KEY_CODE_1,
            ),
            None,
        );
        assert_eq!(
            unshifted_cmd_digit(NSEventModifierFlags::Command, Some("w"), ANSI_KEY_CODE_W),
            None,
        );
        // Rejected: no Command modifier at all (plain "1" typed into a terminal).
        assert_eq!(
            unshifted_cmd_digit(NSEventModifierFlags::empty(), Some("1"), ANSI_KEY_CODE_1),
            None,
        );
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
    #[cfg(target_os = "macos")]
    fn test_macos_cmd_c_shortcut_predicate_boundaries() {
        use objc2_app_kit::NSEventModifierFlags;

        // Accepted: unshifted Cmd+C (lowercase or uppercase, with or without caps lock)
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("c"),
            ANSI_KEY_CODE_C
        ));
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("C"),
            ANSI_KEY_CODE_C
        ));
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
            Some("c"),
            ANSI_KEY_CODE_C
        ));
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
            Some("C"),
            ANSI_KEY_CODE_C
        ));

        // Rejected: Cmd+Shift+C
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            Some("c"),
            ANSI_KEY_CODE_C
        ));
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            Some("C"),
            ANSI_KEY_CODE_C
        ));

        // Rejected: Cmd+V
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("v"),
            ANSI_KEY_CODE_V
        ));

        // Rejected: plain c (no Command modifier)
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::empty(),
            Some("c"),
            ANSI_KEY_CODE_C
        ));

        // Rejected: Ctrl+C
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Control,
            Some("c"),
            ANSI_KEY_CODE_C
        ));
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command | NSEventModifierFlags::Control,
            Some("c"),
            ANSI_KEY_CODE_C
        ));

        // Keycode fallback tests:
        // Accepted: key code 8 with None or Korean character ("ㅊ" on 2-Set Korean keyboard)
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            None,
            ANSI_KEY_CODE_C
        ));
        assert!(is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("ㅊ"),
            ANSI_KEY_CODE_C
        ));

        // Rejected: other keycode without 'c' char
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("a"),
            0
        ));
        assert!(!is_unshifted_cmd_c(
            NSEventModifierFlags::Command,
            Some("ㅊ"),
            0
        ));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn korean_cmd_v_multiple_keydowns_emit_only_once_until_keyup() {
        use objc2_app_kit::{NSEventModifierFlags, NSEventType};

        let mut latch = NativeTerminalPasteLatch::new();

        // 1. First physical Cmd+V KeyDown (Korean IME produces first event with isARepeat=false)
        let action1 = latch.handle_event(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            Some("ㅍ"),
            ANSI_KEY_CODE_V,
            true,
        );
        assert_eq!(action1, NativeTerminalPasteAction::EmitAndConsume);

        // 2. Second duplicate KeyDown during the same physical gesture (also isARepeat=false from AppKit/IME)
        let action2 = latch.handle_event(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            Some("v"),
            ANSI_KEY_CODE_V,
            true,
        );
        assert_eq!(
            action2,
            NativeTerminalPasteAction::ConsumeOnly,
            "Duplicate KeyDown within same Cmd+V gesture must be consumed without emit"
        );

        // 3. Third duplicate KeyDown during the same physical gesture
        let action3 = latch.handle_event(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            None,
            ANSI_KEY_CODE_V,
            true,
        );
        assert_eq!(action3, NativeTerminalPasteAction::ConsumeOnly);

        let unrelated_key_up = latch.handle_event(
            NSEventType::KeyUp,
            NSEventModifierFlags::Command,
            Some("a"),
            0,
            true,
        );
        assert_eq!(unrelated_key_up, NativeTerminalPasteAction::PassThrough);
        assert!(latch.is_latched());

        // 4. KeyUp for 'V' re-arms the latch
        let action_up = latch.handle_event(
            NSEventType::KeyUp,
            NSEventModifierFlags::empty(),
            Some("v"),
            ANSI_KEY_CODE_V,
            true,
        );
        assert_eq!(action_up, NativeTerminalPasteAction::PassThrough);

        // 5. Subsequent Cmd+V KeyDown in a new gesture emits again
        let action4 = latch.handle_event(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            Some("ㅍ"),
            ANSI_KEY_CODE_V,
            true,
        );
        assert_eq!(action4, NativeTerminalPasteAction::EmitAndConsume);

        let command_up = latch.handle_event(
            NSEventType::FlagsChanged,
            NSEventModifierFlags::empty(),
            None,
            0,
            true,
        );
        assert_eq!(command_up, NativeTerminalPasteAction::PassThrough);
        assert!(!latch.is_latched());

        // 6. When terminal is not focused, Cmd+V passes through unchanged
        let mut unfocused_latch = NativeTerminalPasteLatch::new();
        let action_unfocused = unfocused_latch.handle_event(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            Some("v"),
            ANSI_KEY_CODE_V,
            false,
        );
        assert_eq!(action_unfocused, NativeTerminalPasteAction::PassThrough);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn modifier_events_never_read_key_characters() {
        use objc2_app_kit::NSEventType;

        assert!(native_key_event_has_characters(NSEventType::KeyDown));
        assert!(native_key_event_has_characters(NSEventType::KeyUp));
        assert!(!native_key_event_has_characters(NSEventType::FlagsChanged));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_cmd_v_uses_physical_keycode_under_korean_input() {
        use objc2_app_kit::{NSEventModifierFlags, NSEventType};

        assert!(is_unshifted_cmd_v(
            NSEventModifierFlags::Command,
            Some("ㅍ"),
            ANSI_KEY_CODE_V,
        ));
        assert!(is_unshifted_cmd_v(
            NSEventModifierFlags::Command,
            None,
            ANSI_KEY_CODE_V,
        ));
        assert!(!is_unshifted_cmd_v(
            NSEventModifierFlags::Command | NSEventModifierFlags::Control,
            Some("ㅍ"),
            ANSI_KEY_CODE_V,
        ));
        assert!(!is_unshifted_cmd_v(
            NSEventModifierFlags::Control,
            Some("v"),
            ANSI_KEY_CODE_V,
        ));

        let mut latch = NativeTerminalPasteLatch::new();
        assert_eq!(
            latch.handle_event(
                NSEventType::KeyDown,
                NSEventModifierFlags::Command,
                Some("ㅍ"),
                ANSI_KEY_CODE_V,
                true,
            ),
            NativeTerminalPasteAction::EmitAndConsume,
        );

        let mut unfocused_latch = NativeTerminalPasteLatch::new();
        assert_eq!(
            unfocused_latch.handle_event(
                NSEventType::KeyDown,
                NSEventModifierFlags::Command,
                Some("ㅍ"),
                ANSI_KEY_CODE_V,
                false,
            ),
            NativeTerminalPasteAction::PassThrough,
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn app_remote_state_persists_pairing_but_starts_off() {
        let _lock = FERRYX_DATA_DIR_LOCK.lock().expect("data dir lock");
        let _restore = RestoreFerryxDataDir(std::env::var_os("FERRYX_DATA_DIR"));
        let dir = tempfile::TempDir::new().expect("tempdir");
        std::env::set_var("FERRYX_DATA_DIR", dir.path());

        let config_path = dir.path().join("remote-config.json");
        let auth_path = dir.path().join("remote-auth.json");

        let socket_path = dir.path().join("daemon1.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).expect("bind 1");
        let server = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(config_path.clone()),
            Some(auth_path.clone()),
        ));
        let server_clone = Arc::clone(&server);
        let server_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&server_clone);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client = Arc::new(DaemonClient::new_with_socket(socket_path));
        let code = client
            .remote_create_pairing_code(Some(DevicePermission::Control))
            .await
            .expect("create code");
        assert_eq!(code.len(), 6);
        let devices = client.remote_list_devices().await.expect("list devices");
        assert_eq!(devices.len(), 0);
        server_task.abort();

        let socket_path2 = dir.path().join("daemon2.sock");
        let listener2 = tokio::net::UnixListener::bind(&socket_path2).expect("bind 2");
        let server2 = Arc::new(crate::daemon::server::DaemonServer::new_with_paths(
            Some(config_path),
            Some(auth_path),
        ));
        let server_clone2 = Arc::clone(&server2);
        let server_task2 = tokio::spawn(async move {
            loop {
                match listener2.accept().await {
                    Ok((stream, _)) => {
                        let s = Arc::clone(&server_clone2);
                        tokio::spawn(async move {
                            s.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let client2 = Arc::new(DaemonClient::new_with_socket(socket_path2));
        let status = client2.remote_get_status().await.expect("status");
        assert_eq!(status.mode, remote::RemoteNetworkMode::Off);
        assert!(!status.is_running);
        assert!(status.bound_address.is_none());
        server_task2.abort();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .try_init();

    create_app(tauri::Builder::default())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
