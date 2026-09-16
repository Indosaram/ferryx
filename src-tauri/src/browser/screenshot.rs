use crate::browser::BrowserError;
use crate::ipc::error::{IpcError, IpcErrorCode};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

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
    let webview = app
        .get_webview(webview_label)
        .ok_or_else(|| BrowserError::WebviewNotFound(webview_label.to_string()))?;

    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, IpcError>>(1);
    let tx_mutex = std::sync::Mutex::new(Some(tx));

    webview
        .with_webview(move |platform| unsafe {
            use objc2::rc::Retained;
            use objc2::{class, msg_send, runtime::AnyObject};

            let view: &AnyObject = &*platform.inner().cast();
            let tx_mutex = tx_mutex;
            let callback =
                block2::RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
                    let result = if !error.is_null() || image.is_null() {
                        Err(IpcError::new(
                            IpcErrorCode::BrowserScreenshotFailed,
                            "WKWebView snapshot failed",
                        ))
                    } else {
                        let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
                        tiff.ok_or_else(|| {
                            IpcError::new(
                                IpcErrorCode::BrowserScreenshotFailed,
                                "snapshot has no TIFF representation",
                            )
                        })
                        .and_then(|tiff| {
                            let bitmap: Option<Retained<AnyObject>> =
                                msg_send![class!(NSBitmapImageRep), imageRepWithData: &*tiff];
                            let bitmap = bitmap.ok_or_else(|| {
                                IpcError::new(
                                    IpcErrorCode::BrowserScreenshotFailed,
                                    "cannot decode snapshot bitmap image",
                                )
                            })?;
                            let properties: Retained<AnyObject> =
                                msg_send![class!(NSDictionary), dictionary];
                            // 4 is NSBitmapImageFileTypePNG
                            let data: Option<Retained<AnyObject>> = msg_send![
                                &*bitmap,
                                representationUsingType: 4usize,
                                properties: &*properties
                            ];
                            let data = data.ok_or_else(|| {
                                IpcError::new(
                                    IpcErrorCode::BrowserScreenshotFailed,
                                    "cannot encode snapshot to PNG",
                                )
                            })?;
                            let length: usize = msg_send![&*data, length];
                            if length == 0 {
                                return Err(IpcError::new(
                                    IpcErrorCode::BrowserScreenshotFailed,
                                    "empty PNG snapshot generated",
                                ));
                            }
                            let pointer: *const u8 = msg_send![&*data, bytes];
                            Ok(std::slice::from_raw_parts(pointer, length).to_vec())
                        })
                    };
                    if let Ok(mut guard) = tx_mutex.lock() {
                        if let Some(sender) = guard.take() {
                            let _ = sender.send(result);
                        }
                    }
                });

            let _: () = msg_send![
                view,
                takeSnapshotWithConfiguration: std::ptr::null::<AnyObject>(),
                completionHandler: &*callback
            ];
        })
        .map_err(|e| {
            IpcError::new(
                IpcErrorCode::BrowserScreenshotFailed,
                format!("failed to attach webview snapshot handler: {e}"),
            )
        })?;

    let (tx_async, rx_async) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let res = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|e| {
                IpcError::new(
                    IpcErrorCode::BrowserScreenshotFailed,
                    format!("screenshot timed out: {e}"),
                )
            })
            .and_then(|r| r);
        let _ = tx_async.send(res);
    });

    let png_bytes = rx_async.await.map_err(|_| {
        IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            "snapshot task channel closed",
        )
    })??;

    if let Some(parent) = target_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    std::fs::write(&target_path, png_bytes).map_err(|e| {
        IpcError::new(
            IpcErrorCode::BrowserScreenshotFailed,
            format!("failed to write screenshot to {}: {e}", target_path.display()),
        )
    })?;

    Ok(target_path.to_string_lossy().to_string())
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
}
