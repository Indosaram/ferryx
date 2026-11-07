//! Where every browser data root lives.
//!
//! A run with `FERRYX_DATA_DIR` set (QA, tests, a second isolated instance) keeps all browser
//! data under that directory: the app and page webviews' data folders, named profiles, the
//! Chromium engine's profiles, the library and the window state. Without it nothing moves:
//! roots stay at the app data dir, and the webviews keep the platform default folder
//! (`%LOCALAPPDATA%\<identifier>` for WebView2, the identifier's dir for WebKitGTK).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The isolation override, ignored when empty.
pub fn isolated_root(env: Option<&OsStr>) -> Option<PathBuf> {
    env.filter(|dir| !dir.is_empty()).map(PathBuf::from)
}

/// The browser data root: the isolation override, else the app data dir.
pub fn data_root(env: Option<&OsStr>, app_data_dir: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    isolated_root(env).or_else(app_data_dir)
}

/// A named profile's system-webview data folder.
pub fn named_profile_dir(root: &Path, profile_id: &str) -> PathBuf {
    root.join("browser-profiles").join(profile_id)
}

/// A profile's Chromium engine user data.
pub fn cef_profile_dir(root: &Path, profile_id: &str) -> PathBuf {
    root.join("browser-cef").join(profile_id)
}

/// The data folder of the app webview and of the default and private page webviews. `None`
/// (no isolation) keeps the platform default so existing installs keep their data.
pub fn shared_webview_dir(env: Option<&OsStr>) -> Option<PathBuf> {
    isolated_root(env).map(|root| root.join("webview"))
}

/// The window-state file. `None` keeps the plugin's default under the app config dir.
pub fn window_state_file(env: Option<&OsStr>) -> Option<PathBuf> {
    isolated_root(env).map(|root| root.join(".window-state.json"))
}

/// A WKWebsiteDataStore identifier for an isolated folder (macOS has no data folder option):
/// the same folder always maps to the same store.
pub fn data_store_identifier(dir: &Path) -> [u8; 16] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(dir.to_string_lossy().as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    // RFC 4122 version 4 / variant bits, so the bytes are a well-formed UUID.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// The browser data root for a running app.
#[cfg(feature = "desktop")]
pub fn app_data_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    let mut failure = None;
    data_root(std::env::var_os("FERRYX_DATA_DIR").as_deref(), || {
        app.path().app_data_dir().map_err(|error| failure = Some(error.to_string())).ok()
    })
    .ok_or_else(|| failure.unwrap_or_else(|| "no app data dir".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn env(value: &str) -> Option<OsString> {
        Some(OsString::from(value))
    }

    #[test]
    fn the_override_roots_every_browser_dir() {
        let root = std::env::temp_dir().join("ferryx-isolated");
        let value = env(root.to_str().expect("utf-8 temp dir"));
        let resolved = data_root(value.as_deref(), || panic!("the app data dir must not be consulted"))
            .expect("root");
        assert_eq!(resolved, root);
        assert_eq!(named_profile_dir(&resolved, "work"), root.join("browser-profiles").join("work"));
        assert_eq!(cef_profile_dir(&resolved, "default"), root.join("browser-cef").join("default"));
        assert_eq!(shared_webview_dir(value.as_deref()), Some(root.join("webview")));
        assert_eq!(window_state_file(value.as_deref()), Some(root.join(".window-state.json")));
        for dir in [
            named_profile_dir(&resolved, "work"),
            cef_profile_dir(&resolved, "default"),
            shared_webview_dir(value.as_deref()).expect("webview dir"),
        ] {
            assert!(dir.starts_with(&root), "{} escapes the isolated root", dir.display());
        }
    }

    #[test]
    fn without_the_override_existing_locations_are_kept() {
        let app_data = PathBuf::from("app-data");
        for unset in [None, env("")] {
            assert_eq!(data_root(unset.as_deref(), || Some(app_data.clone())), Some(app_data.clone()));
            assert_eq!(shared_webview_dir(unset.as_deref()), None);
            assert_eq!(window_state_file(unset.as_deref()), None);
            assert_eq!(isolated_root(unset.as_deref()), None);
        }
        assert_eq!(named_profile_dir(&app_data, "work"), app_data.join("browser-profiles").join("work"));
        assert_eq!(cef_profile_dir(&app_data, "work"), app_data.join("browser-cef").join("work"));
    }

    #[test]
    fn a_store_identifier_is_stable_per_folder_and_a_v4_uuid() {
        let a = data_store_identifier(Path::new("/qa/a/webview"));
        assert_eq!(a, data_store_identifier(Path::new("/qa/a/webview")));
        assert_ne!(a, data_store_identifier(Path::new("/qa/b/webview")));
        assert_eq!(a[6] >> 4, 4);
        assert_eq!(a[8] >> 6, 0b10);
    }
}
