//! Extensions of the Chromium engine's browser profiles: what is installed, and the Chrome
//! pages that manage them, opened as Ferryx Chromium tabs.

use crate::browser::engine_install::{self, EngineStatus};
use crate::browser::extensions::{self, ExtensionPage, InstalledExtension};
use crate::browser::{BrowserManager, BrowserProfileId, BrowserState};
use crate::ipc::browser::{engine_data_dir, ENGINE_EXECUTABLE};
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::AppHandle;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionsListing {
    pub profile: String,
    pub engine_installed: bool,
    pub supported: bool,
    pub extensions: Vec<InstalledExtension>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnginePaths {
    pub install_dir: String,
    pub profiles_dir: String,
}

fn parse_profile(profile: Option<String>) -> Result<BrowserProfileId, IpcError> {
    let raw = profile.unwrap_or_else(|| "default".to_string());
    BrowserProfileId::from_id(&raw).ok_or_else(|| {
        IpcError::new(IpcErrorCode::InvalidArgument, format!("invalid browser profile: {raw}"))
    })
}

/// The Chromium data dir the engine uses for `profile`.
fn profile_root(data: &Path, profile: &BrowserProfileId) -> PathBuf {
    crate::browser::data_paths::cef_profile_dir(data, profile.as_str())
}

/// A private profile keeps nothing on disk, so it never lists extensions.
fn installed(data: &Path, profile: &BrowserProfileId) -> Result<Vec<InstalledExtension>, IpcError> {
    if profile.is_private() {
        return Ok(Vec::new());
    }
    extensions::list_installed(&profile_root(data, profile))
        .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
}

/// The extensions installed in a browser profile's Chromium engine data.
#[tauri::command]
pub async fn cmd_browser_extensions_list<R: tauri::Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
) -> Result<ExtensionsListing, IpcError> {
    let profile = parse_profile(profile)?;
    let data = engine_data_dir(&app)?;
    crate::ipc::run_blocking(move || {
        let engine_installed = matches!(
            engine_install::status(&data, ENGINE_EXECUTABLE),
            EngineStatus::Installed { .. }
        );
        Ok(ExtensionsListing {
            profile: profile.as_str().to_string(),
            engine_installed,
            supported: engine_install::platform_key().is_some(),
            extensions: installed(&data, &profile)?,
        })
    })
    .await
}

/// Opens Chrome's extensions page (or an extension's options page) in a new Chromium tab of
/// the profile. The tab reaches the UI through the session-created event.
#[tauri::command]
pub async fn cmd_browser_extensions_open_page<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, Arc<BrowserManager>>,
    profile: Option<String>,
    page: ExtensionPage,
) -> Result<BrowserState, IpcError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        open_page(&app, manager.inner(), profile, page).await
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, manager, profile, page);
        Err(IpcError::new(
            IpcErrorCode::UnsupportedCapability,
            "the Chromium engine is unavailable on this platform",
        )
        .with_details(json!({ "reason": "platformUnsupported" })))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
async fn open_page<R: tauri::Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    profile: Option<String>,
    page: ExtensionPage,
) -> Result<BrowserState, IpcError> {
    use crate::browser::CreateBrowserRequest;
    use tauri::Emitter;

    let profile = parse_profile(profile)?;
    let data = engine_data_dir(app)?;
    let lookup_data = data.clone();
    let lookup_profile = profile.clone();
    let (engine_dir, url) = crate::ipc::run_blocking(move || {
        let Some(engine_dir) = crate::browser::cef_engine::engine_dir(&lookup_data) else {
            return Err(IpcError::new(
                IpcErrorCode::UnsupportedCapability,
                "the Chromium engine is not installed",
            )
            .with_details(json!({ "reason": "engineNotInstalled" })));
        };
        let options_url = match &page {
            ExtensionPage::Options(id) => installed(&lookup_data, &lookup_profile)?
                .into_iter()
                .find(|extension| &extension.id == id)
                .and_then(|extension| extension.options_url),
            _ => None,
        };
        let url = extensions::page_url(&page, options_url.as_deref())
            .map_err(|message| IpcError::new(IpcErrorCode::InvalidArgument, message))?;
        Ok((engine_dir, url))
    })
    .await?;

    let request = CreateBrowserRequest {
        browser_id: None,
        workspace_id: None,
        worktree_path: None,
        // The engine opens the chrome:// page; its address event brings the real URL.
        url: "about:blank".to_string(),
        profile: Some(profile),
        zoom_factor: None,
        bounds: None,
        visible: Some(false),
    };
    let state = manager.register_session(request).map_err(IpcError::from)?;
    crate::ipc::browser::record_session_created(&state.browser_id);
    let state =
        crate::ipc::browser::create_cef_browser_session(app, manager, state, engine_dir, Some(url))
            .await?;
    let _ = app.emit(
        crate::browser::guest::BROWSER_SESSION_CREATED_EVENT,
        crate::browser::model::BrowserSessionCreatedPayload {
            browser: state.clone(),
            workspace_id: None,
        },
    );
    Ok(state)
}

/// Where the engine is installed and where its profiles keep their Chromium data.
#[tauri::command]
pub async fn cmd_browser_engine_paths<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<EnginePaths, IpcError> {
    let data = engine_data_dir(&app)?;
    Ok(EnginePaths {
        install_dir: engine_install::install_dir(&data).to_string_lossy().into_owned(),
        profiles_dir: data.join("browser-cef").to_string_lossy().into_owned(),
    })
}
