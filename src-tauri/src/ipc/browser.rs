use crate::browser::{BrowserManager, BrowserSessionSummary, BrowserState, CreateBrowserRequest, LogicalRect};
use crate::ipc::error::IpcError;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub async fn cmd_browser_create<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: CreateBrowserRequest,
) -> Result<BrowserState, IpcError> {
    let state = manager.register_session(request.clone())?;

    if let Some(main_window) = app.get_window("main") {
        let label = state.webview_label.clone();
        let target_url = state.url.clone();
        let bounds = request.bounds.clone();
        let visible = state.visible;

        let window_clone = main_window.clone();
        let _ = main_window.run_on_main_thread(move || {
            let parsed_url: tauri::WebviewUrl = if let Ok(u) = target_url.parse() {
                tauri::WebviewUrl::External(u)
            } else {
                tauri::WebviewUrl::App("about:blank".into())
            };

            let builder = tauri::WebviewBuilder::new(label, parsed_url);

            let pos = if let Some(ref b) = bounds {
                tauri::LogicalPosition { x: b.x, y: b.y }
            } else {
                tauri::LogicalPosition { x: 0.0, y: 0.0 }
            };

            let size = if let Some(ref b) = bounds {
                tauri::LogicalSize {
                    width: b.width,
                    height: b.height,
                }
            } else {
                tauri::LogicalSize {
                    width: 800.0,
                    height: 600.0,
                }
            };

            if let Ok(child) = window_clone.add_child(
                builder,
                tauri::LogicalPosition { x: pos.x, y: pos.y },
                tauri::LogicalSize {
                    width: size.width,
                    height: size.height,
                },
            ) {
                if !visible {
                    let _ = child.hide();
                }
            }
        });
    }

    Ok(state)
}

#[tauri::command]
pub async fn cmd_browser_navigate<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    url: String,
) -> Result<(), IpcError> {
    let valid_url = manager.update_url(&browser_id, &url)?;
    let state = manager.get_state(&browser_id)?;

    if let Some(webview) = app.get_webview(&state.webview_label) {
        if let Ok(parsed) = valid_url.parse() {
            let _ = webview.navigate(parsed);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_reload<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.reload();
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_bounds<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    bounds: LogicalRect,
) -> Result<(), IpcError> {
    manager.set_bounds(&browser_id, bounds.clone())?;
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.set_bounds(tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition {
                x: bounds.x,
                y: bounds.y,
            }),
            size: tauri::Size::Logical(tauri::LogicalSize {
                width: bounds.width,
                height: bounds.height,
            }),
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_visible<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    visible: bool,
) -> Result<(), IpcError> {
    manager.set_visible(&browser_id, visible)?;
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        if visible {
            let _ = webview.show();
        } else {
            let _ = webview.hide();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_set_zoom<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    zoom_factor: f64,
) -> Result<f64, IpcError> {
    let clamped = manager.set_zoom(&browser_id, zoom_factor)?;
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.set_zoom(clamped);
    }
    Ok(clamped)
}

#[tauri::command]
pub async fn cmd_browser_focus<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.get_state(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        let _ = webview.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_get_state(
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<BrowserState, IpcError> {
    let state = manager.get_state(&browser_id)?;
    Ok(state)
}

#[tauri::command]
pub async fn cmd_browser_close<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    if let Some(session) = manager.remove_session(&browser_id) {
        if let Some(webview) = app.get_webview(&session.webview_label) {
            let _ = webview.close();
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn cmd_browser_list(
    manager: State<'_, Arc<BrowserManager>>,
) -> Result<Vec<BrowserSessionSummary>, IpcError> {
    Ok(manager.list_sessions())
}

#[tauri::command]
pub async fn cmd_browser_open_external(
    url: String,
) -> Result<(), IpcError> {
    let valid_url = crate::browser::validate_url(&url)?;
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&valid_url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&valid_url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", &valid_url]).spawn();
    }
    Ok(())
}
