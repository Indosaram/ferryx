use crate::browser::snapshot_source::*;

use crate::browser::remote_service::BrowserRemoteService;
use crate::ipc::error::{IpcError, IpcErrorCode};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Expands a leading `~`, `~/` (and `~\` where backslash is a separator) against `home`.
/// `Ok(None)` means the path has no home prefix. A prefix that names only the home directory
/// is refused: a screenshot needs a file, and writing to a directory can only fail later.
fn expand_home_prefix(
    trimmed: &str,
    home: Option<PathBuf>,
    backslash_is_separator: bool,
) -> Result<Option<PathBuf>, IpcError> {
    let is_separator = |c: char| c == '/' || (backslash_is_separator && c == '\\');
    let rest = if trimmed == "~" {
        ""
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        rest
    } else if let Some(rest) = backslash_is_separator
        .then(|| trimmed.strip_prefix("~\\"))
        .flatten()
    {
        rest
    } else {
        return Ok(None);
    };
    if rest.trim_matches(is_separator).is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "screenshot output path must name a file, not the home directory",
        ));
    }
    let home = home.ok_or_else(|| {
        IpcError::new(IpcErrorCode::InvalidPath, "failed to expand home directory")
    })?;
    Ok(Some(home.join(rest)))
}

pub fn resolve_screenshot_path(path_str: &str) -> Result<PathBuf, IpcError> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "screenshot output path cannot be empty",
        ));
    }
    let path = match expand_home_prefix(trimmed, dirs_home_dir(), cfg!(windows))? {
        Some(expanded) => expanded,
        None => PathBuf::from(trimmed),
    };

    let path = if path.is_relative() {
        std::env::current_dir()
            .map_err(|e| IpcError::new(IpcErrorCode::IoError, e.to_string()))?
            .join(path)
    } else {
        path
    };

    Ok(path)
}

fn dirs_home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            if !profile.is_empty() {
                return Some(PathBuf::from(profile));
            }
        }
    }
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

pub fn unsupported_screenshot_error() -> IpcError {
    IpcError::new(
        IpcErrorCode::Unsupported,
        "screenshots are unavailable on this platform: native webview capture is macOS-only in this build",
    )
}

#[cfg(target_os = "macos")]
pub async fn take_browser_screenshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    out_path: &str,
) -> Result<String, IpcError> {
    let target_path = resolve_screenshot_path(out_path)?;
    let source = TauriBrowserSnapshotSource::new(app.clone());
    take_browser_screenshot_with_source(&source, webview_label, target_path).await
}

#[cfg(not(target_os = "macos"))]
pub async fn take_browser_screenshot<R: tauri::Runtime>(
    app: &AppHandle<R>,
    webview_label: &str,
    out_path: &str,
) -> Result<String, IpcError> {
    use tauri::Manager;

    // Reject an unusable output path BEFORE reporting the platform limitation, so a
    // caller that asked for an impossible path gets that specific error instead of a
    // blanket `Unsupported`. Nothing is written here, so the resolved path is
    // deliberately dropped.
    resolve_screenshot_path(out_path).map(|_resolved_path| ())?;
    let _ = app
        .get_webview(webview_label)
        .ok_or_else(|| crate::browser::BrowserError::WebviewNotFound(webview_label.to_string()))?;
    Err(unsupported_screenshot_error())
}

pub async fn take_browser_screenshot_with_source<S: BrowserSnapshotSource + ?Sized>(
    source: &S,
    webview_label: &str,
    target_path: PathBuf,
) -> Result<String, IpcError> {
    let snapshot = source
        .capture_snapshot(webview_label, SnapshotOptions::png())
        .await?;

    let path_to_write = target_path.clone();
    crate::ipc::run_blocking(move || {
        if let Some(parent) = path_to_write.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    IpcError::new(
                        IpcErrorCode::BrowserScreenshotFailed,
                        format!(
                            "failed to create parent directory {}: {e}",
                            parent.display()
                        ),
                    )
                })?;
            }
        }
        std::fs::write(&path_to_write, snapshot.bytes).map_err(|e| {
            IpcError::new(
                IpcErrorCode::BrowserScreenshotFailed,
                format!(
                    "failed to write screenshot to {}: {e}",
                    path_to_write.display()
                ),
            )
        })?;
        Ok(())
    })
    .await?;

    Ok(target_path.to_string_lossy().to_string())
}

/// Capability descriptor for native browser snapshots. The frontend uses this to
/// disable platform-limited affordances (element picking, screenshot capture)
/// instead of offering a control that can never complete.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSnapshotCapability {
    pub supported: bool,
    pub formats: Vec<String>,
    /// Element picking needs both a native snapshot and the tab's isolated-world bridge:
    /// without the world, a pick cannot be told apart from page-forged input (design 3.6).
    pub pick: bool,
}

/// `pick` is true only when snapshots work and this tab's trusted bridge is isolated.
pub fn element_pick_capability(
    snapshot_supported: bool,
    bridge: Option<crate::browser::bridge::TrustedBridgeKind>,
) -> bool {
    snapshot_supported && bridge == Some(crate::browser::bridge::TrustedBridgeKind::Isolated)
}

/// Reports whether native webview snapshot capture is available in this build and
/// which encodings it supports. Non-macOS builds report the platform limitation here
/// instead of failing silently when a snapshot is requested. With `browser_id`, `pick`
/// also reflects that tab's trusted bridge; without it `pick` is false.
#[tauri::command]
pub fn cmd_browser_snapshot_capability(
    service: State<'_, Arc<BrowserRemoteService>>,
    manager: State<'_, Arc<crate::browser::BrowserManager>>,
    browser_id: Option<String>,
) -> BrowserSnapshotCapability {
    let source = service.snapshot_source();
    let supported = service.is_snapshot_supported();
    let bridge = browser_id
        .as_deref()
        .and_then(|id| manager.get_state(id).ok())
        .map(|state| state.trusted_bridge);
    BrowserSnapshotCapability {
        supported,
        formats: source.supported_formats(),
        pick: element_pick_capability(supported, bridge),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_screenshot_path() {
        assert!(resolve_screenshot_path("").is_err());
        assert!(resolve_screenshot_path("   ").is_err());

        let res = resolve_screenshot_path("~/my-shot.png").expect("expand tilde");
        assert!(res.to_str().unwrap().ends_with("my-shot.png"));
        assert!(!res.to_str().unwrap().starts_with('~'));

        let absolute_input = if cfg!(windows) { r"C:\tmp\direct.png" } else { "/tmp/direct.png" };
        let abs = resolve_screenshot_path(absolute_input).expect("absolute path");
        assert_eq!(abs, PathBuf::from(absolute_input));

        let rel = resolve_screenshot_path("shots/rel.png").expect("relative path");
        assert!(rel.is_absolute(), "relative input must be anchored to the current dir");
        assert!(rel.ends_with("shots/rel.png"));
    }

    #[test]
    fn expand_home_prefix_handles_windows_backslash_and_bare_tilde() {
        let home = || Some(PathBuf::from("/home/tester"));
        // The Windows `~\` form expands only where backslash is a separator.
        assert_eq!(
            expand_home_prefix(r"~\shots\a.png", home(), true).unwrap(),
            Some(PathBuf::from("/home/tester").join(r"shots\a.png"))
        );
        assert_eq!(expand_home_prefix(r"~\a.png", home(), false).unwrap(), None);
        assert_eq!(
            expand_home_prefix("~/a.png", home(), false).unwrap(),
            Some(PathBuf::from("/home/tester/a.png"))
        );
        // A path naming only the home directory is not a file.
        for (bare, windows) in [("~", false), ("~/", false), ("~//", false), (r"~\", true)] {
            let err = expand_home_prefix(bare, home(), windows)
                .expect_err("home directory is not a screenshot file");
            assert_eq!(err.code, IpcErrorCode::InvalidArgument, "{bare:?}");
        }
        let err = resolve_screenshot_path("~").expect_err("bare tilde must be refused");
        assert_eq!(err.code, IpcErrorCode::InvalidArgument);
        // Without a home directory the expansion reports InvalidPath.
        let err = expand_home_prefix("~/a.png", None, false).expect_err("no home");
        assert_eq!(err.code, IpcErrorCode::InvalidPath);
    }

    #[tokio::test]
    async fn test_take_browser_screenshot_with_source_png_file_contract() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let file_path = temp_dir.path().join("shot.png");
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
            width: 120,
            height: 90,
        });

        let saved_path =
            take_browser_screenshot_with_source(&source, "main-view", file_path.clone())
                .await
                .expect("should save PNG file");

        assert_eq!(saved_path, file_path.to_string_lossy().to_string());
        let read_bytes = std::fs::read(&file_path).expect("file should exist");
        assert!(read_bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[tokio::test]
    async fn test_take_browser_screenshot_with_source_unsupported_propagates() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let file_path = temp_dir.path().join("shot.png");
        let source = UnsupportedSnapshotSource;

        let err = take_browser_screenshot_with_source(&source, "main-view", file_path)
            .await
            .expect_err("should propagate unsupported error");

        assert_eq!(err.code, IpcErrorCode::Unsupported);
    }

    #[test]
    fn test_unsupported_screenshot_error_contract() {
        let err = unsupported_screenshot_error();
        assert_eq!(err.code, IpcErrorCode::Unsupported);
    }

    #[test]
    fn element_pick_capability_requires_snapshot_and_isolated_bridge() {
        use crate::browser::bridge::TrustedBridgeKind;
        assert!(element_pick_capability(true, Some(TrustedBridgeKind::Isolated)));
        assert!(!element_pick_capability(true, Some(TrustedBridgeKind::Unavailable)));
        assert!(!element_pick_capability(true, None));
        assert!(!element_pick_capability(false, Some(TrustedBridgeKind::Isolated)));
    }

    #[test]
    fn test_unsupported_screenshot_error_contract_details() {
        let err = unsupported_screenshot_error();
        assert_eq!(err.code, IpcErrorCode::Unsupported);
        let code_str = serde_json::to_value(&err.code)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap();
        assert_eq!(code_str, "UNSUPPORTED");
        assert!(err.message.contains("macOS-only"));
    }

    // A-CORE-10: drive the real `take_browser_screenshot` (whichever platform variant
    // this build compiles) with no such webview. An unusable path must be reported as
    // InvalidArgument before capture, webview lookup or the non-macOS Unsupported error.
    #[tokio::test]
    async fn take_browser_screenshot_rejects_empty_path_before_capture_or_unsupported() {
        let app = tauri::test::mock_app();
        for out_path in ["", "   "] {
            let err = take_browser_screenshot(app.handle(), "browser-missing", out_path)
                .await
                .expect_err("empty path must fail");
            assert_eq!(err.code, IpcErrorCode::InvalidArgument, "{out_path:?}: {err:?}");
        }
    }

    #[tokio::test]
    async fn take_browser_screenshot_reports_parent_directory_creation_failure() {
        // A-CORE-12: a parent that cannot be created must surface its own error.
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let blocker = temp_dir.path().join("blocker");
        std::fs::write(&blocker, b"not a directory").expect("write blocker file");
        let parent = blocker.join("nested");
        let source = FakeBrowserSnapshotSource::new(FakeSnapshotBehavior::Auto {
            width: 4,
            height: 4,
        });

        let err = take_browser_screenshot_with_source(&source, "main-view", parent.join("shot.png"))
            .await
            .expect_err("parent under a regular file cannot be created");

        assert_eq!(err.code, IpcErrorCode::BrowserScreenshotFailed);
        assert!(
            err.message.contains(&format!(
                "failed to create parent directory {}",
                parent.display()
            )),
            "message must name the parent: {}",
            err.message
        );
    }
}
