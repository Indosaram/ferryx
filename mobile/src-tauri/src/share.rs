//! The one place the mobile shell hands a file to the user: `share_text_file` stages text as a
//! file and opens the iOS share sheet (Save to Files, AirDrop, Mail). The shell has no browser
//! download handler, so this is the supervisor transcript's export route on the iPhone.
//! Resolves `true` only when the user completed a share or save; a dismissed sheet is `false`.
use std::path::{Path, PathBuf};

/// A plain file name: no path parts, no leading dot, short, ASCII-safe.
pub fn plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Writes `text` to `<dir>/<file_name>` for the share sheet. The directory is private to this
/// call; the caller removes it afterwards. A failed write leaves nothing behind.
pub fn stage(dir: &Path, file_name: &str, text: &str) -> Result<PathBuf, String> {
    if !plain_file_name(file_name) {
        return Err("the export file name must be a plain file name".into());
    }
    std::fs::create_dir_all(dir).map_err(|error| format!("the export file was not staged: {error}"))?;
    let path = dir.join(file_name);
    if let Err(error) = std::fs::write(&path, text.as_bytes()) {
        let _ = std::fs::remove_file(&path);
        return Err(format!("the export file was not staged: {error}"));
    }
    Ok(path)
}

#[cfg(target_os = "ios")]
fn present(window: &tauri::WebviewWindow, path: PathBuf) -> Result<tauri::async_runtime::Receiver<bool>, String> {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSArray, NSError, NSString, NSURL};
    use objc2_ui_kit::{UIActivityType, UIActivityViewController, UIViewController};

    let (sender, receiver) = tauri::async_runtime::channel::<bool>(1);
    window
        .with_webview(move |webview| {
            // `with_webview` runs on the main thread.
            let Some(main) = MainThreadMarker::new() else {
                let _ = sender.try_send(false);
                return;
            };
            let controller = webview.view_controller();
            if controller.is_null() {
                let _ = sender.try_send(false);
                return;
            }
            // SAFETY: the pointer is the live UIViewController Tauri owns for this window.
            let controller: &UIViewController = unsafe { &*(controller as *const UIViewController) };
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            let item: Retained<AnyObject> = unsafe { Retained::cast_unchecked(url) };
            let items = NSArray::from_retained_slice(&[item]);
            // SAFETY: the items are NSURLs, which UIActivityViewController accepts.
            let sheet = unsafe {
                UIActivityViewController::initWithActivityItems_applicationActivities(main.alloc(), &items, None)
            };
            let done = sender.clone();
            let handler = RcBlock::new(
                move |_kind: *mut UIActivityType, completed: Bool, _items: *mut NSArray, _error: *mut NSError| {
                    let _ = done.try_send(completed.as_bool());
                },
            );
            // SAFETY: the block matches UIActivityViewControllerCompletionWithItemsHandler and is copied.
            unsafe { sheet.setCompletionWithItemsHandler(&*handler as *const _ as *mut _) };
            controller.presentViewController_animated_completion(&sheet, true, None);
        })
        .map_err(|error| format!("the share sheet did not open: {error}"))?;
    Ok(receiver)
}

/// Stages the text as a file and opens the share sheet over the app. `true`: shared or saved;
/// `false`: the sheet was dismissed, nothing was exported.
#[tauri::command]
pub async fn share_text_file(
    window: tauri::WebviewWindow,
    file_name: String,
    text: String,
) -> Result<bool, String> {
    let dir = std::env::temp_dir().join(format!("ferryx-export-{}", std::process::id()));
    let path = stage(&dir, &file_name, &text)?;
    let outcome = share_staged(&window, path).await;
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

#[cfg(target_os = "ios")]
async fn share_staged(window: &tauri::WebviewWindow, path: PathBuf) -> Result<bool, String> {
    let mut receiver = present(window, path)?;
    Ok(receiver.recv().await.unwrap_or(false))
}

#[cfg(not(target_os = "ios"))]
async fn share_staged(_window: &tauri::WebviewWindow, _path: PathBuf) -> Result<bool, String> {
    Err("the share sheet is available only on iPhone and iPad".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_names_are_staged_and_the_text_is_the_file() {
        let dir = tempdir();
        let path = stage(&dir, "supervisor-transcript.txt", "Owner prompt\nSupervisor reply\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Owner prompt\nSupervisor reply\n");
        for bad in ["", "../x.txt", "a/b.txt", ".hidden", "a b.txt"] {
            assert!(stage(&dir, bad, "x").is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferryx-share-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}
