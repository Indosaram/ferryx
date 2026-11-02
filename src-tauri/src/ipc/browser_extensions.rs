//! Extensions of the Chromium engine's browser profiles: what is installed, enabling,
//! disabling, removing and pinning them, and the Chrome pages that manage them, opened as
//! Ferryx Chromium tabs. Changes go through the profile's engine (its control channel), which
//! is started without a tab when it is not running; profile files are only ever read.

use crate::browser::engine_install::{self, EngineStatus};
use crate::browser::extensions::{self, ExtensionPage, InstalledExtension};
use crate::browser::{BrowserManager, BrowserProfileId, BrowserState};
use crate::ipc::browser::{engine_data_dir, ENGINE_EXECUTABLE};
use crate::ipc::error::{IpcError, IpcErrorCode};
use ferryx_browser_engine_protocol::ExtensionCall;
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
    /// The list came from the profile's running engine (pin state known), not its files.
    pub engine_running: bool,
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

/// The profile a change applies to: a private profile keeps no extensions.
fn parse_change_profile(profile: Option<String>) -> Result<BrowserProfileId, IpcError> {
    let profile = parse_profile(profile)?;
    if profile.is_private() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "the private profile has no extensions",
        )
        .with_details(json!({ "reason": "privateProfile" })));
    }
    Ok(profile)
}

fn check_extension_id(id: &str) -> Result<(), IpcError> {
    if extensions::is_valid_extension_id(id) {
        Ok(())
    } else {
        Err(IpcError::new(IpcErrorCode::InvalidArgument, format!("invalid extension id: {id}")))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn engine_not_installed() -> IpcError {
    IpcError::new(IpcErrorCode::UnsupportedCapability, "the Chromium engine is not installed")
        .with_details(json!({ "reason": "engineNotInstalled" }))
}

/// The Chromium data dir the engine uses for `profile`.
fn profile_root(data: &Path, profile: &BrowserProfileId) -> PathBuf {
    data.join("browser-cef").join(profile.as_str())
}

/// A private profile keeps nothing on disk, so it never lists extensions.
fn installed(data: &Path, profile: &BrowserProfileId) -> Result<Vec<InstalledExtension>, IpcError> {
    if profile.is_private() {
        return Ok(Vec::new());
    }
    extensions::list_installed(&profile_root(data, profile))
        .map_err(|error| IpcError::new(IpcErrorCode::IoError, error.to_string()))
}

/// The listing from the profile's files (the engine is not running, or not consulted).
async fn files_listing(data: PathBuf, profile: BrowserProfileId) -> Result<ExtensionsListing, IpcError> {
    crate::ipc::run_blocking(move || {
        let engine_installed = matches!(
            engine_install::status(&data, ENGINE_EXECUTABLE),
            EngineStatus::Installed { .. }
        );
        Ok(ExtensionsListing {
            profile: profile.as_str().to_string(),
            engine_installed,
            engine_running: false,
            supported: engine_install::platform_key().is_some(),
            extensions: installed(&data, &profile)?,
        })
    })
    .await
}

/// The extensions installed in a browser profile: from its running engine when there is one,
/// else from its Chromium data on disk.
#[tauri::command]
pub async fn cmd_browser_extensions_list<R: tauri::Runtime>(
    app: AppHandle<R>,
    profile: Option<String>,
) -> Result<ExtensionsListing, IpcError> {
    let profile = parse_profile(profile)?;
    let data = engine_data_dir(&app)?;
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        if !profile.is_private() && crate::browser::cef_engine::is_running(profile.as_str()) {
            // An engine that cannot answer (it is quitting, its page failed) leaves the file
            // listing.
            match engine_listing(&profile).await {
                Ok(extensions) => return Ok(engine_running_listing(&profile, extensions)),
                Err(error) => tracing::warn!(
                    profile = profile.as_str(),
                    error = %error.message,
                    "engine extension list failed"
                ),
            }
        }
    }
    files_listing(data, profile).await
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
const ENGINE_CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Removing waits for the user to answer Chrome's own confirm dialog.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
const ENGINE_REMOVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
async fn engine_listing(profile: &BrowserProfileId) -> Result<Vec<InstalledExtension>, IpcError> {
    use crate::browser::cef_engine;
    let list = cef_engine::extensions(profile.as_str(), ExtensionCall::List, ENGINE_CALL_TIMEOUT)
        .await
        .map_err(IpcError::from)?;
    Ok(extensions::from_engine_list(&list))
}

/// The listing of a running engine.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn engine_running_listing(profile: &BrowserProfileId, extensions: Vec<InstalledExtension>) -> ExtensionsListing {
    ExtensionsListing {
        profile: profile.as_str().to_string(),
        engine_installed: true,
        engine_running: true,
        supported: true,
        extensions,
    }
}

/// An engine failure as the command's error, with `details.reason`: `cancelled` when the user
/// cancelled Chrome's remove dialog, else `engineCallFailed`.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn change_error(error: crate::browser::BrowserError) -> IpcError {
    use crate::browser::{cef_engine, BrowserError};
    let reason = match &error {
        BrowserError::AutomationFailed(message) if message == cef_engine::EXTENSION_CALL_CANCELLED => "cancelled",
        _ => "engineCallFailed",
    };
    IpcError::from(error).with_details(json!({ "reason": reason }))
}

/// The extension id a change names; it is checked before anything runs.
fn call_extension(call: &ExtensionCall) -> Option<&str> {
    match call {
        ExtensionCall::List => None,
        ExtensionCall::SetEnabled { extension, .. }
        | ExtensionCall::Remove { extension }
        | ExtensionCall::Pin { extension, .. } => Some(extension),
    }
}

/// Checks a change's profile and extension id before anything starts.
fn check_change(profile: Option<String>, call: &ExtensionCall) -> Result<BrowserProfileId, IpcError> {
    let profile = parse_change_profile(profile)?;
    check_extension_id(call_extension(call).unwrap_or_default())?;
    Ok(profile)
}

/// Applies `call` through the profile's engine (starting it without a tab when needed) and
/// returns the fresh listing.
async fn change_extension<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: &Arc<BrowserManager>,
    profile: Option<String>,
    call: ExtensionCall,
) -> Result<ExtensionsListing, IpcError> {
    let profile = check_change(profile, &call)?;
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        use crate::browser::cef_engine;
        let data = engine_data_dir(&app)?;
        let engine_dir = crate::ipc::run_blocking(move || {
            cef_engine::engine_dir(&data).ok_or_else(engine_not_installed)
        })
        .await?;
        crate::ipc::browser::ensure_cef_engine(&app, manager, &profile, engine_dir).await?;
        let timeout = if matches!(call, ExtensionCall::Remove { .. }) {
            ENGINE_REMOVE_TIMEOUT
        } else {
            ENGINE_CALL_TIMEOUT
        };
        cef_engine::extensions(profile.as_str(), call, timeout).await.map_err(change_error)?;
        let extensions = engine_listing(&profile).await?;
        Ok(engine_running_listing(&profile, extensions))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, manager, profile, call);
        Err(platform_unsupported())
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_unsupported() -> IpcError {
    IpcError::new(
        IpcErrorCode::UnsupportedCapability,
        "the Chromium engine is unavailable on this platform",
    )
    .with_details(json!({ "reason": "platformUnsupported" }))
}

/// Enables or disables an extension in the profile's engine.
#[tauri::command]
pub async fn cmd_browser_extensions_set_enabled<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, Arc<BrowserManager>>,
    profile: Option<String>,
    extension_id: String,
    enabled: bool,
) -> Result<ExtensionsListing, IpcError> {
    change_extension(app, manager.inner(), profile, ExtensionCall::SetEnabled { extension: extension_id, enabled })
        .await
}

/// Removes an extension from the profile. Chrome asks the user to confirm in its own dialog.
#[tauri::command]
pub async fn cmd_browser_extensions_remove<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, Arc<BrowserManager>>,
    profile: Option<String>,
    extension_id: String,
) -> Result<ExtensionsListing, IpcError> {
    change_extension(app, manager.inner(), profile, ExtensionCall::Remove { extension: extension_id }).await
}

/// Pins or unpins an extension's toolbar button in the profile's engine.
#[tauri::command]
pub async fn cmd_browser_extensions_set_pinned<R: tauri::Runtime>(
    app: AppHandle<R>,
    manager: tauri::State<'_, Arc<BrowserManager>>,
    profile: Option<String>,
    extension_id: String,
    pinned: bool,
) -> Result<ExtensionsListing, IpcError> {
    change_extension(app, manager.inner(), profile, ExtensionCall::Pin { extension: extension_id, pinned }).await
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
        Err(platform_unsupported())
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
            return Err(engine_not_installed());
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

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "cjpalhdlnbpafiamejdnhcphjbkeiagm";

    fn reason(error: &IpcError) -> Option<&str> {
        error.details.as_ref()?.get("reason")?.as_str()
    }

    #[test]
    fn a_change_needs_a_stored_profile_and_a_valid_extension_id() {
        let call = ExtensionCall::Remove { extension: ID.into() };
        assert_eq!(check_change(None, &call).unwrap(), BrowserProfileId::Default);
        assert_eq!(
            check_change(Some("work".into()), &call).unwrap(),
            BrowserProfileId::Named("work".into())
        );

        let private = check_change(Some("private".into()), &call).unwrap_err();
        assert_eq!(private.code, IpcErrorCode::InvalidArgument);
        assert_eq!(reason(&private), Some("privateProfile"));

        let bad_profile = check_change(Some("../x".into()), &call).unwrap_err();
        assert_eq!(bad_profile.code, IpcErrorCode::InvalidArgument);

        let upper = ID.to_uppercase();
        let bad_ids: [&str; 5] = ["", "../../etc", &upper, &ID[..31], "qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"];
        for bad in bad_ids {
            let calls = [
                ExtensionCall::SetEnabled { extension: bad.to_string(), enabled: true },
                ExtensionCall::Remove { extension: bad.to_string() },
                ExtensionCall::Pin { extension: bad.to_string(), pinned: false },
            ];
            for call in calls {
                let error = check_change(None, &call).unwrap_err();
                assert_eq!(error.code, IpcErrorCode::InvalidArgument, "{bad}");
            }
        }
        assert_eq!(check_change(None, &ExtensionCall::List).unwrap_err().code, IpcErrorCode::InvalidArgument);
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn engine_failures_map_to_structured_errors() {
        use crate::browser::{cef_engine, BrowserError};
        let missing = engine_not_installed();
        assert_eq!(missing.code, IpcErrorCode::UnsupportedCapability);
        assert_eq!(reason(&missing), Some("engineNotInstalled"));

        let cancelled = change_error(BrowserError::AutomationFailed(cef_engine::EXTENSION_CALL_CANCELLED.into()));
        assert_eq!(cancelled.code, IpcErrorCode::BrowserAutomationFailed);
        assert_eq!(reason(&cancelled), Some("cancelled"));

        let failed = change_error(BrowserError::AutomationFailed("the Chromium engine stopped".into()));
        assert_eq!(failed.code, IpcErrorCode::BrowserAutomationFailed);
        assert_eq!(reason(&failed), Some("engineCallFailed"));
    }

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    #[test]
    fn a_running_engine_listing_says_so() {
        let listing = engine_running_listing(&BrowserProfileId::Default, Vec::new());
        let value = serde_json::to_value(&listing).unwrap();
        assert_eq!(value["engineRunning"], json!(true));
        assert_eq!(value["engineInstalled"], json!(true));
        assert_eq!(value["profile"], json!("default"));
    }
}
