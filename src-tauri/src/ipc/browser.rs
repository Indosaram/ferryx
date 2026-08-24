use crate::browser::{
    cookie_from_imported, parse_cookie_file, BrowserError, BrowserManager, BrowserProfileId,
    BrowserSessionSummary, BrowserState, BrowserStateChangedPayload, CreateBrowserRequest,
    ImportBrowserCookiesRequest, ImportBrowserCookiesResult, LogicalRect,
};
use crate::ipc::error::IpcError;
use std::sync::Arc;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, State};

pub const BROWSER_STATE_CHANGED_EVENT: &str = "browser_state_changed";

fn emit_browser_state<R: tauri::Runtime>(webview: &tauri::Webview<R>, state: &BrowserState) {
    let _ = webview.app_handle().emit(
        BROWSER_STATE_CHANGED_EVENT,
        BrowserStateChangedPayload::from(state),
    );
}

fn update_webview_state<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    manager: Arc<BrowserManager>,
    browser_id: String,
    url: Option<String>,
    title: Option<String>,
    loading: Option<bool>,
) {
    if let Ok(state) =
        manager.update_navigation_state(&browser_id, url, title, loading, None, None, None)
    {
        emit_browser_state(webview, &state);
    }

    #[cfg(target_os = "macos")]
    {
        let manager = Arc::clone(&manager);
        let browser_id = browser_id.clone();
        let webview_for_emit = webview.clone();
        let _ = webview.with_webview(move |platform_webview| unsafe {
            let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
            if let Ok(state) = manager.update_navigation_state(
                &browser_id,
                None,
                None,
                None,
                Some(native.canGoBack()),
                Some(native.canGoForward()),
                None,
            ) {
                emit_browser_state(&webview_for_emit, &state);
            }
        });
    }
}

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
        let browser_id = state.browser_id.clone();
        let incognito = matches!(&state.profile_id, BrowserProfileId::Private);
        let page_manager = Arc::clone(manager.inner());
        let title_manager = Arc::clone(manager.inner());
        let creation_manager = Arc::clone(manager.inner());
        let page_browser_id = browser_id.clone();
        let title_browser_id = browser_id.clone();

        let window_clone = main_window.clone();
        let _ = main_window.run_on_main_thread(move || {
            let parsed_url: tauri::WebviewUrl = if let Ok(u) = target_url.parse() {
                tauri::WebviewUrl::External(u)
            } else {
                tauri::WebviewUrl::App("about:blank".into())
            };

            let builder = tauri::WebviewBuilder::new(label, parsed_url)
                .incognito(incognito)
                .on_page_load(move |webview, payload| {
                    let loading = matches!(payload.event(), PageLoadEvent::Started);
                    update_webview_state(
                        &webview,
                        Arc::clone(&page_manager),
                        page_browser_id.clone(),
                        Some(payload.url().to_string()),
                        None,
                        Some(loading),
                    );
                })
                .on_document_title_changed(move |webview, title| {
                    update_webview_state(
                        &webview,
                        Arc::clone(&title_manager),
                        title_browser_id.clone(),
                        None,
                        Some(title),
                        None,
                    );
                });

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
                if let Ok(Some(current_bounds)) = creation_manager.get_bounds(&browser_id) {
                    let _ = child.set_bounds(tauri::Rect {
                        position: tauri::Position::Logical(tauri::LogicalPosition {
                            x: current_bounds.x,
                            y: current_bounds.y,
                        }),
                        size: tauri::Size::Logical(tauri::LogicalSize {
                            width: current_bounds.width,
                            height: current_bounds.height,
                        }),
                    });
                }
                let is_visible = creation_manager
                    .is_visible(&browser_id)
                    .unwrap_or(visible);
                if !is_visible {
                    let _ = child.hide();
                } else {
                    let _ = child.show();
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
        emit_browser_state(&webview, &state);
        let parsed = valid_url.parse().map_err(|error| {
            BrowserError::NavigationFailed(format!("invalid target URL: {error}"))
        })?;
        webview
            .navigate(parsed)
            .map_err(|error| BrowserError::NavigationFailed(error.to_string()))?;
    }
    Ok(())
}

fn history_navigation<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    browser_id: &str,
    forward: bool,
) -> Result<(), IpcError> {
    let state = manager.begin_history_navigation(browser_id)?;
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::NotFound(browser_id.to_string()))?;
    emit_browser_state(&webview, &state);

    #[cfg(target_os = "macos")]
    {
        let manager = Arc::clone(manager);
        let browser_id = browser_id.to_string();
        let webview_for_emit = webview.clone();
        webview
            .with_webview(move |platform_webview| unsafe {
                let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
                let can_navigate = if forward {
                    native.canGoForward()
                } else {
                    native.canGoBack()
                };

                if can_navigate {
                    if forward {
                        let _ = native.goForward();
                    } else {
                        let _ = native.goBack();
                    }
                }

                if let Ok(next_state) = manager.update_navigation_state(
                    &browser_id,
                    None,
                    None,
                    Some(can_navigate),
                    Some(native.canGoBack()),
                    Some(native.canGoForward()),
                    None,
                ) {
                    emit_browser_state(&webview_for_emit, &next_state);
                }
            })
            .map_err(|error| BrowserError::HistoryFailed(error.to_string()))?;
        return Ok(());
    }

    #[cfg(not(target_os = "macos"))]
    {
        let script = if forward {
            "history.forward()"
        } else {
            "history.back()"
        };
        webview
            .eval(script)
            .map_err(|error| BrowserError::HistoryFailed(error.to_string()))?;
        Ok(())
    }
}

#[tauri::command]
pub async fn cmd_browser_go_back<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    history_navigation(&app, manager.inner(), &browser_id, false)
}

#[tauri::command]
pub async fn cmd_browser_go_forward<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    history_navigation(&app, manager.inner(), &browser_id, true)
}

#[tauri::command]
pub async fn cmd_browser_import_cookies<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    request: ImportBrowserCookiesRequest,
) -> Result<ImportBrowserCookiesResult, IpcError> {
    let profile_id = BrowserProfileId::from_id(&request.profile_id)
        .ok_or_else(|| BrowserError::UnsupportedProfile(request.profile_id.clone()))?;
    let source = tokio::fs::read_to_string(&request.file_path)
        .await
        .map_err(|error| {
            BrowserError::CookieImport(format!("failed to read cookie file: {error}"))
        })?;
    let cookies = parse_cookie_file(&source)?
        .into_iter()
        .map(cookie_from_imported)
        .collect::<Result<Vec<_>, _>>()?;

    let mut targets = manager
        .webview_labels_for_profile(&profile_id)
        .into_iter()
        .filter_map(|label| app.get_webview(&label))
        .collect::<Vec<_>>();

    if targets.is_empty() && matches!(profile_id, BrowserProfileId::Default) {
        if let Some(main_webview) = app.get_webview("main") {
            targets.push(main_webview);
        }
    }

    if targets.is_empty() {
        return Err(BrowserError::CookieImport(format!(
            "open a browser tab using the {} profile before importing cookies",
            profile_id.as_str()
        ))
        .into());
    }

    for target in targets {
        for cookie in &cookies {
            target
                .set_cookie(cookie.clone())
                .map_err(|error| BrowserError::CookieImport(error.to_string()))?;
        }
    }

    Ok(ImportBrowserCookiesResult {
        imported_count: cookies.len(),
    })
}

#[tauri::command]
pub async fn cmd_browser_reload<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
) -> Result<(), IpcError> {
    let state = manager.begin_history_navigation(&browser_id)?;
    if let Some(webview) = app.get_webview(&state.webview_label) {
        emit_browser_state(&webview, &state);
        webview
            .reload()
            .map_err(|error| BrowserError::NavigationFailed(error.to_string()))?;
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
    let webview = app
        .get_webview(&state.webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(state.webview_label.clone()))?;
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
    // TEMP-DIAG: remove after toolbar-overlap investigation
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/ferryx_browser_bounds.log") {
        use std::io::Write;
        let native = webview.bounds();
        let _ = writeln!(
            log,
            "set_bounds id={} sent=({:.1},{:.1} {:.1}x{:.1}) native={:?}",
            browser_id, bounds.x, bounds.y, bounds.width, bounds.height,
            native.map(|b| (b.position, b.size))
        );
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
pub async fn cmd_browser_open_external(url: String) -> Result<(), IpcError> {
    let valid_url = crate::browser::validate_url(&url)?;
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&valid_url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&valid_url)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &valid_url])
            .spawn();
    }
    Ok(())
}
