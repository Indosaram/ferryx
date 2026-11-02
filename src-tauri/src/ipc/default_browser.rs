//! IPC for the default-browser registration, URLs the OS asked Ferryx to open, and the
//! sign-in handoff for identity providers that block the system webview.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::browser::auth_session::{self, AuthSessionOutcome};
use crate::browser::default_browser::{
    self, DefaultBrowserStatus, MakeDefaultOutcome, INBOUND_URL_EVENT,
};
use crate::browser::engine_install;
use crate::browser::sign_in_handoff::{
    self, detect_sign_in_block, handoff_plan, host_platform, oauth_flow, AuthCallback,
    EngineAvailability, HandoffPlan, PendingHandoff, SignInBlock,
};
use crate::browser::{BrowserManager, BrowserProfileId};
use crate::ipc::error::{IpcError, IpcErrorCode};

/// Emitted with `{ profileId, count }` after a Chromium sign-in's cookies were copied into the
/// profile's system-webview tabs.
pub const SIGN_IN_COOKIES_SYNCED_EVENT: &str = "ferryx:browser-sign-in-cookies-synced";

/// Settings > Browser "Default browser" row status.
#[tauri::command]
pub async fn cmd_default_browser_status() -> Result<DefaultBrowserStatus, IpcError> {
    crate::ipc::run_blocking(|| Ok(default_browser::status())).await
}

/// Registers Ferryx with the OS and asks it to become the default browser.
#[tauri::command]
pub async fn cmd_default_browser_make_default() -> Result<MakeDefaultOutcome, IpcError> {
    crate::ipc::run_blocking(|| default_browser::make_default().map_err(IpcError::internal)).await
}

/// Drains the inbound URLs queued since the last call. The UI calls it on start and on every
/// `ferryx:inbound-url` event.
#[tauri::command]
pub async fn cmd_default_browser_take_inbound_urls() -> Result<Vec<String>, IpcError> {
    Ok(default_browser::take_inbound_urls())
}

/// Classifies a page as an identity provider's embedded-webview sign-in block.
#[tauri::command]
pub async fn cmd_browser_detect_sign_in_block(
    url: String,
    title: Option<String>,
) -> Result<SignInBlock, IpcError> {
    Ok(detect_sign_in_block(&url, title.as_deref()))
}

/// The handoff the bar offers for a blocked sign-in page, or `None` when the page is not one.
#[tauri::command]
pub async fn cmd_browser_sign_in_handoff_plan<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    url: String,
    title: Option<String>,
) -> Result<Option<HandoffPlan>, IpcError> {
    let SignInBlock::Blocked {
        provider,
        retry_url,
    } = detect_sign_in_block(&url, title.as_deref())
    else {
        return Ok(None);
    };
    let data_dir = crate::ipc::browser::engine_data_dir(&app)?;
    let inactive = crate::ipc::browser::engine_inactive_reason(&app);
    let status = crate::ipc::run_blocking(move || {
        Ok(engine_install::mark_inactive(
            engine_install::status(&data_dir, crate::ipc::browser::ENGINE_EXECUTABLE),
            inactive,
        ))
    })
    .await?;
    let history = manager.history_snapshot(&browser_id);
    let flow = oauth_flow(&url, &history);
    let auth_session = flow
        .as_ref()
        .is_some_and(|flow| auth_session::supports(&flow.callback));
    Ok(Some(handoff_plan(
        provider,
        &retry_url,
        EngineAvailability::from_status(&status),
        host_platform(),
        flow.as_ref(),
        auth_session,
    )))
}

/// How an OS auth session ended, for the handoff bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AuthSessionResult {
    Completed { callback_url: String },
    Cancelled,
    Unavailable { reason: String },
}

/// Runs the OS web-authentication session for `authorize_url`. An http(s) callback loads in
/// the original tab, so the site sets its session cookies in that tab's own profile.
#[tauri::command]
pub async fn cmd_browser_sign_in_auth_session<R: Runtime>(
    app: AppHandle<R>,
    manager: State<'_, Arc<BrowserManager>>,
    browser_id: String,
    authorize_url: String,
    callback: AuthCallback,
) -> Result<AuthSessionResult, IpcError> {
    if !url::Url::parse(&authorize_url).is_ok_and(|parsed| parsed.scheme() == "https") {
        return Err(IpcError::new(
            IpcErrorCode::BrowserUrlInvalid,
            "the sign-in authorize URL must be https",
        ));
    }
    manager.get_state(&browser_id)?;
    match auth_session::run(&app, authorize_url, callback).await {
        AuthSessionOutcome::Callback(callback_url) => {
            let loads_in_tab = url::Url::parse(&callback_url)
                .is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"));
            if loads_in_tab {
                crate::ipc::browser::navigate_browser_session(
                    &app,
                    manager.inner(),
                    &browser_id,
                    &callback_url,
                )
                .await?;
            }
            Ok(AuthSessionResult::Completed { callback_url })
        }
        AuthSessionOutcome::Cancelled => Ok(AuthSessionResult::Cancelled),
        AuthSessionOutcome::Unavailable(reason) => Ok(AuthSessionResult::Unavailable { reason }),
    }
}

/// Remembers a Chromium tab opened for a sign-in; once it leaves `idp_host`, the cookies of the
/// signed-in site are copied into the profile's system-webview tabs.
#[tauri::command]
pub async fn cmd_browser_sign_in_track(
    manager: State<'_, Arc<BrowserManager>>,
    chromium_browser_id: String,
    profile_id: String,
    target_url: String,
    idp_host: String,
) -> Result<(), IpcError> {
    let profile = BrowserProfileId::from_id(&profile_id).ok_or_else(|| {
        IpcError::new(
            IpcErrorCode::InvalidArgument,
            format!("unknown browser profile: {profile_id}"),
        )
    })?;
    let idp_host = idp_host.trim().to_ascii_lowercase();
    // `cef_engine::open` adds the tab to the engine's tabs before the create command resolves,
    // so the UI's track call after `createBrowserTab` sees it. The engine tab's profile is the
    // session's `profile_id`, so the session's profile is the one to compare.
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    let chromium_tab = crate::browser::cef_engine::is_cef_tab(&chromium_browser_id);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let chromium_tab = false;
    let tab_profile = manager
        .get_state(&chromium_browser_id)
        .ok()
        .map(|state| state.profile_id);
    check_sign_in_track(
        &idp_host,
        &target_url,
        chromium_tab,
        tab_profile.as_ref(),
        &profile,
    )?;
    sign_in_handoff::track(PendingHandoff {
        chromium_browser_id,
        profile_id: profile.as_str().to_string(),
        target_url,
        idp_host,
    });
    Ok(())
}

/// A tracked handoff needs a known identity provider host, an https target and a live Chromium
/// tab (`chromium_tab`) whose session belongs to `profile`.
fn check_sign_in_track(
    idp_host: &str,
    target_url: &str,
    chromium_tab: bool,
    tab_profile: Option<&BrowserProfileId>,
    profile: &BrowserProfileId,
) -> Result<(), IpcError> {
    let invalid = |message: &str| -> Result<(), IpcError> {
        Err(IpcError::new(IpcErrorCode::InvalidArgument, message))
    };
    if !sign_in_handoff::is_identity_provider_host(idp_host) {
        return invalid("the host is not a known identity provider");
    }
    if !url::Url::parse(target_url).is_ok_and(|parsed| parsed.scheme() == "https") {
        return invalid("the sign-in target URL must be https");
    }
    if !chromium_tab || tab_profile != Some(profile) {
        return invalid("the sign-in tab is not a live Chromium tab of the profile");
    }
    Ok(())
}

/// Called with every address a Chromium tab reports: a tracked sign-in tab that left its
/// identity provider gets its cookies copied in the background.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn on_cef_address<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    tab: &str,
    url: &str,
) {
    let Some(handoff) = sign_in_handoff::take_completed(tab, url) else {
        return;
    };
    let app = app.clone();
    let manager = Arc::clone(manager);
    tauri::async_runtime::spawn(async move {
        let profile_id = handoff.profile_id.clone();
        match sync_sign_in_cookies(&app, &manager, &handoff).await {
            Ok(count) => {
                let _ = app.emit(
                    SIGN_IN_COOKIES_SYNCED_EVENT,
                    serde_json::json!({ "profileId": profile_id, "count": count }),
                );
            }
            Err(error) => {
                tracing::warn!(
                    tab = %handoff.chromium_browser_id,
                    "could not copy sign-in cookies to the profile's tabs: {error}"
                );
            }
        }
    });
}

/// Reads the signed-in site's and the identity provider's cookies from the Chromium tab and
/// sets them in every system-webview tab of the same profile. Returns how many were read.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
async fn sync_sign_in_cookies<R: Runtime>(
    app: &AppHandle<R>,
    manager: &Arc<BrowserManager>,
    handoff: &PendingHandoff,
) -> Result<usize, String> {
    let profile = BrowserProfileId::from_id(&handoff.profile_id)
        .ok_or_else(|| format!("unknown browser profile {}", handoff.profile_id))?;
    let result = crate::browser::cef_engine::devtools(
        &handoff.chromium_browser_id,
        "Network.getCookies",
        serde_json::json!({
            "urls": [handoff.target_url, format!("https://{}/", handoff.idp_host)]
        }),
        std::time::Duration::from_secs(5),
    )
    .await
    .map_err(|error| error.to_string())?;
    let cookies = sign_in_handoff::cdp_cookies(&result)
        .into_iter()
        .filter_map(|cookie| crate::browser::cookie_from_imported(cookie).ok())
        .collect::<Vec<_>>();
    let targets = manager
        .list_sessions()
        .into_iter()
        .filter(|session| session.profile_id == profile)
        .filter_map(|session| manager.get_state(&session.browser_id).ok())
        .filter(|state| state.engine == crate::browser::BrowserEngine::System)
        .filter_map(|state| app.get_webview(&state.webview_label))
        .collect::<Vec<_>>();
    for target in targets {
        for cookie in &cookies {
            if let Err(error) = target.set_cookie(cookie.clone()) {
                tracing::warn!(cookie = %cookie.name(), "could not set a sign-in cookie: {error}");
            }
        }
    }
    Ok(cookies.len())
}

/// Queues `url` for the UI and brings the main window forward. Non-http(s) URLs are dropped.
pub fn deliver_inbound_url<R: Runtime>(app: &AppHandle<R>, url: &str) -> bool {
    let Some(url) = default_browser::classify_inbound_url(url) else {
        tracing::warn!("ignored an inbound URL that is not http(s)");
        return false;
    };
    default_browser::queue_inbound_url(url);
    if let Err(error) = app.emit(INBOUND_URL_EVENT, ()) {
        tracing::warn!(%error, "could not notify the UI of an inbound URL; it stays queued");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "https://mail.google.com/mail/";

    fn rejected(result: Result<(), IpcError>) -> bool {
        result.is_err_and(|error| error.code == IpcErrorCode::InvalidArgument)
    }

    #[test]
    fn sign_in_track_accepts_a_live_chromium_tab_of_the_profile() {
        let profile = BrowserProfileId::Default;
        assert!(check_sign_in_track("accounts.google.com", TARGET, true, Some(&profile), &profile).is_ok());
    }

    #[test]
    fn sign_in_track_rejects_bad_hosts_targets_and_tabs() {
        let profile = BrowserProfileId::Default;
        let other = BrowserProfileId::Named("work".into());
        let host = "accounts.google.com";
        assert!(rejected(check_sign_in_track("evil.test", TARGET, true, Some(&profile), &profile)));
        assert!(rejected(check_sign_in_track("", TARGET, true, Some(&profile), &profile)));
        assert!(rejected(check_sign_in_track(host, "http://mail.google.com/", true, Some(&profile), &profile)));
        assert!(rejected(check_sign_in_track(host, "not a url", true, Some(&profile), &profile)));
        // An unknown or empty tab id has no session and no engine tab.
        assert!(rejected(check_sign_in_track(host, TARGET, false, None, &profile)));
        // A system-webview tab of the profile.
        assert!(rejected(check_sign_in_track(host, TARGET, false, Some(&profile), &profile)));
        assert!(rejected(check_sign_in_track(host, TARGET, true, Some(&other), &profile)));
    }
}
