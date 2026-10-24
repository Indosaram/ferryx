//! Platform-native cache-bypassing ("hard") reload.
//!
//! Tauri's own `Webview::reload()` takes no cache flag, and wry's implementation calls the plain
//! `WKWebView.reload()`, so a hard reload has to reach the platform binding directly: macOS
//! `WKWebView.reloadFromOrigin` (end-to-end revalidation) and Linux WebKitGTK
//! `WebViewExt::reload_bypass_cache`. This build carries no WebView2 binding, so Windows answers a
//! hard-reload request with a structured `Unsupported` rather than silently degrading to the
//! cached soft reload.

use crate::ipc::error::{IpcError, IpcErrorCode};

/// Which native cache-bypassing reload this build can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardReloadSupport {
    /// macOS WKWebView `reloadFromOrigin`.
    ReloadFromOrigin,
    /// Linux WebKitGTK `WebViewExt::reload_bypass_cache`.
    BypassCache,
    /// No cache-bypassing reload binding on this target.
    Unsupported,
}

#[cfg(target_os = "macos")]
pub const fn hard_reload_support() -> HardReloadSupport {
    HardReloadSupport::ReloadFromOrigin
}

#[cfg(target_os = "linux")]
pub const fn hard_reload_support() -> HardReloadSupport {
    HardReloadSupport::BypassCache
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const fn hard_reload_support() -> HardReloadSupport {
    HardReloadSupport::Unsupported
}

/// Whether this build can perform a cache-bypassing reload at all.
pub const fn hard_reload_supported() -> bool {
    !matches!(hard_reload_support(), HardReloadSupport::Unsupported)
}

#[cfg(feature = "desktop")]
/// Reloads `webview` bypassing the HTTP cache.
///
/// A caller that asked for fresh content must never silently receive cached bytes, so this never
/// falls back to the plain reload: an unavailable platform path is reported as an error.
pub fn hard_reload<R: tauri::Runtime>(webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    match hard_reload_support() {
        HardReloadSupport::ReloadFromOrigin => reload_from_origin(webview),
        HardReloadSupport::BypassCache => reload_bypassing_cache(webview),
        HardReloadSupport::Unsupported => Err(unsupported_error()),
    }
}

fn unsupported_error() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "cache-bypassing reload is unavailable on this platform: this build has no WebView2 binding, so a hard reload cannot be performed (a plain reload would silently serve cached content)",
    )
}

#[cfg(feature = "desktop")]
#[cfg(target_os = "macos")]
fn reload_from_origin<R: tauri::Runtime>(webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    webview
        .with_webview(|platform_webview| unsafe {
            // SAFETY: `with_webview` runs this closure on the app-owned WebView's UI thread, where
            // the platform handle is the WKWebView, exactly as the native history and key-dispatch
            // bridges in `ipc/browser.rs` assume.
            let native: &objc2_web_kit::WKWebView = &*platform_webview.inner().cast();
            // The returned navigation is a handle, not an error channel; like the soft reload path
            // there is nothing to report when the engine declines to start one.
            let _ = native.reloadFromOrigin();
        })
        .map_err(webview_unreachable)
}

#[cfg(not(target_os = "macos"))]
fn reload_from_origin<R: tauri::Runtime>(_webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    Err(unsupported_error())
}

#[cfg(target_os = "linux")]
fn reload_bypassing_cache<R: tauri::Runtime>(webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    use webkit2gtk::WebViewExt;

    webview
        .with_webview(|platform_webview| {
            platform_webview.inner().reload_bypass_cache();
        })
        .map_err(webview_unreachable)
}

#[cfg(feature = "desktop")]
#[cfg(not(target_os = "linux"))]
fn reload_bypassing_cache<R: tauri::Runtime>(_webview: &tauri::Webview<R>) -> Result<(), IpcError> {
    Err(unsupported_error())
}

#[cfg(feature = "desktop")]
fn webview_unreachable(error: tauri::Error) -> IpcError {
    IpcError::new(
        IpcErrorCode::InternalError,
        format!("hard reload could not reach the native webview: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_support() -> HardReloadSupport {
        if cfg!(target_os = "macos") {
            HardReloadSupport::ReloadFromOrigin
        } else if cfg!(target_os = "linux") {
            HardReloadSupport::BypassCache
        } else {
            HardReloadSupport::Unsupported
        }
    }

    #[test]
    fn hard_reload_support_matches_the_compiled_target() {
        assert_eq!(hard_reload_support(), expected_support());
    }

    #[test]
    fn hard_reload_supported_is_false_only_without_a_native_path() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(hard_reload_supported());
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert!(!hard_reload_supported());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hard_reload_support_is_reload_from_origin_on_macos() {
        assert_eq!(hard_reload_support(), HardReloadSupport::ReloadFromOrigin);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn hard_reload_support_is_bypass_cache_on_linux() {
        assert_eq!(hard_reload_support(), HardReloadSupport::BypassCache);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn hard_reload_support_is_unsupported_off_macos_and_linux() {
        assert_eq!(hard_reload_support(), HardReloadSupport::Unsupported);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn unsupported_hard_reload_reports_a_structured_code() {
        assert_eq!(unsupported_error().code, IpcErrorCode::Unsupported);
    }
}
