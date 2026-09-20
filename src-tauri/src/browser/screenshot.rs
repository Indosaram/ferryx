use crate::browser::snapshot_source::*;

use crate::ipc::error::{IpcError, IpcErrorCode};
use std::path::PathBuf;
use tauri::AppHandle;

pub fn resolve_screenshot_path(path_str: &str) -> Result<PathBuf, IpcError> {
    let trimmed = path_str.trim();
    if trimmed.is_empty() {
        return Err(IpcError::new(
            IpcErrorCode::InvalidArgument,
            "screenshot output path cannot be empty",
        ));
    }
    let path = if let Some(stripped) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs_home_dir() {
            home.join(stripped)
        } else {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                "failed to expand home directory",
            ));
        }
    } else if trimmed == "~" {
        if let Some(home) = dirs_home_dir() {
            home
        } else {
            return Err(IpcError::new(
                IpcErrorCode::InvalidPath,
                "failed to expand home directory",
            ));
        }
    } else {
        PathBuf::from(trimmed)
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
    let _ = (app, webview_label, out_path);
    Err(IpcError::new(
        IpcErrorCode::Unsupported,
        "screenshots are unavailable on this platform",
    ))
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
                let _ = std::fs::create_dir_all(parent);
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

        let abs = resolve_screenshot_path("/tmp/direct.png").expect("absolute path");
        assert_eq!(abs, PathBuf::from("/tmp/direct.png"));
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
}
