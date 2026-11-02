//! Native controls for downloads the system webview runs (WebView2, WKWebView, WebKitGTK).
//!
//! wry reports a download only by URL: once when it starts and once when it ends. The download
//! handler registers the record and marks its URL as waiting here; each OS hook then finds the
//! engine's own download object for that URL and hands the registry a control for it.

use crate::browser::download::{registry, DownloadControl};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Whether this OS's engine can pause and resume a download. WebKitGTK 2.x has no pause.
pub const CAN_PAUSE: bool = cfg!(any(target_os = "windows", target_os = "macos"));
/// Whether this OS's engine can cancel a download through the hook.
pub const CAN_CANCEL: bool = cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux"));

/// Download ids waiting for their engine download object, by URL.
static PENDING: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

/// The same URL as wry hands the download handler (parsed and serialized), so the engine's raw
/// string and the handler's string meet.
fn key(url: &str) -> String {
    url::Url::parse(url).map(|url| url.to_string()).unwrap_or_else(|_| url.to_string())
}

/// Marks the registered download `id` for `url` as waiting for its engine control.
pub fn expect(url: &str, id: &str) {
    PENDING.lock().insert(key(url), id.to_string());
}

/// Gives the download waiting for `url` the control `make` builds for its id. Returns the id
/// when the record took the control.
pub fn attach(
    url: &str,
    can_pause: bool,
    make: impl FnOnce(&str) -> Arc<dyn DownloadControl>,
) -> Option<String> {
    if !CAN_CANCEL {
        return None;
    }
    let id = PENDING.lock().remove(&key(url))?;
    let control = make(&id);
    registry().attach_control(&id, control, can_pause && CAN_PAUSE).then_some(id)
}

/// True when the engine stopped download `id` in a state it can resume from (WebView2 reports
/// an interrupted, resumable download as finished). Main thread.
#[cfg(target_os = "windows")]
pub use self::windows::engine_paused;

/// Only WebView2 reports a resumable stop as a finish.
#[cfg(not(target_os = "windows"))]
pub fn engine_paused(_id: &str) -> bool {
    false
}

#[cfg(target_os = "windows")]
use self::windows::forget;
#[cfg(target_os = "macos")]
use self::macos::forget;

/// WebKitGTK's control holds only a weak reference; nothing to drop.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn forget(_id: &str) {}

/// Drops what the hooks hold for a download that ended. Main thread.
pub fn finished(url: &str, id: &str) {
    PENDING.lock().remove(&key(url));
    forget(id);
}

/// Hooks a browser tab's system webview so its downloads get engine controls. Once per tab,
/// after creation.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    #[cfg(target_os = "windows")]
    self::windows::install(webview);
    #[cfg(target_os = "macos")]
    self::macos::install(webview);
    #[cfg(target_os = "linux")]
    self::linux::install(webview);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let _ = webview;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::download::DownloadStatus;

    struct NoopControl;

    impl DownloadControl for NoopControl {
        fn cancel(&self) {}
        fn pause(&self) {}
        fn resume(&self) {}
    }

    fn register(id: &str, url: &str) {
        registry().register_native(id.into(), url.into(), "/tmp/f".into(), None, None, false);
        expect(url, id);
    }

    #[test]
    fn platform_capabilities_match_the_engines() {
        assert_eq!(CAN_PAUSE, cfg!(any(target_os = "windows", target_os = "macos")));
        assert_eq!(CAN_CANCEL, cfg!(any(target_os = "windows", target_os = "macos", target_os = "linux")));
    }

    #[test]
    fn the_engine_download_for_a_url_gets_the_control_once() {
        register("wv-test-attach", "https://a.test/file.zip");
        let mut made_for = None;
        let attached = attach("https://a.test/file.zip", true, |id| {
            made_for = Some(id.to_string());
            Arc::new(NoopControl)
        });
        assert_eq!(attached.as_deref(), CAN_CANCEL.then_some("wv-test-attach"));
        let record = registry().get("wv-test-attach").unwrap();
        assert_eq!(record.can_cancel, CAN_CANCEL);
        assert_eq!(record.can_pause, CAN_PAUSE);
        if CAN_CANCEL {
            assert_eq!(made_for.as_deref(), Some("wv-test-attach"));
        }
        assert!(attach("https://a.test/file.zip", true, |_| Arc::new(NoopControl)).is_none());
    }

    #[test]
    fn an_engine_url_matches_the_handler_url_after_parsing() {
        register("wv-test-key", "https://A.test/a b");
        let attached = attach("https://a.test/a%20b", false, |_| Arc::new(NoopControl));
        assert_eq!(attached.is_some(), CAN_CANCEL);
        assert!(!registry().get("wv-test-key").unwrap().can_pause);
    }

    #[test]
    fn an_unknown_or_finished_download_gets_no_control() {
        assert!(attach("https://a.test/never", true, |_| Arc::new(NoopControl)).is_none());
        register("wv-test-done", "https://a.test/done");
        registry().set_committed("wv-test-done");
        assert!(attach("https://a.test/done", true, |_| Arc::new(NoopControl)).is_none());
        assert_eq!(registry().get("wv-test-done").unwrap().status, DownloadStatus::Completed);
        register("wv-test-ended", "https://a.test/ended");
        finished("https://a.test/ended", "wv-test-ended");
        assert!(attach("https://a.test/ended", true, |_| Arc::new(NoopControl)).is_none());
    }
}
