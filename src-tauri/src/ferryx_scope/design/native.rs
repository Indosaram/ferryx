use super::{DesignError, Result};

/// Captures the actual visible child WebView, never a DOM reconstruction or desktop.
/// Caller removes the guest overlay and checks its operation fence before and after await.
pub async fn capture_viewport(webview: &tauri::Webview) -> Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        webview.with_webview(move |platform| {
            // Tauri supplies this WKWebView on its owning main thread. ObjC selectors
            // avoid requiring additional generated bindings for snapshot/image classes.
            unsafe {
                use objc2::{class, msg_send, runtime::AnyObject};
                use objc2::rc::Retained;
                let view: &AnyObject = &*platform.inner().cast();
                let tx = std::sync::Mutex::new(Some(tx));
                let callback = block2::RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
                    let result = if !error.is_null() || image.is_null() { Err(DesignError::Capture("WKWebView snapshot failed".into())) } else {
                        let tiff: Option<Retained<AnyObject>> = msg_send![image, TIFFRepresentation];
                        tiff.ok_or_else(|| DesignError::Capture("snapshot has no image representation".into())).and_then(|tiff| {
                            let bitmap: Option<Retained<AnyObject>> = msg_send![class!(NSBitmapImageRep), imageRepWithData: &*tiff];
                            let bitmap = bitmap.ok_or_else(|| DesignError::Capture("cannot decode native image".into()))?;
                            let properties: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
                            let data: Option<Retained<AnyObject>> = msg_send![&*bitmap, representationUsingType: 4usize, properties: &*properties];
                            let data = data.ok_or_else(|| DesignError::Capture("cannot encode native PNG".into()))?;
                            let length: usize = msg_send![&*data, length];
                            if length == 0 || length > 10 * 1024 * 1024 { return Err(DesignError::Capture("native PNG exceeds byte budget".into())); }
                            let pointer: *const u8 = msg_send![&*data, bytes];
                            Ok(std::slice::from_raw_parts(pointer, length).to_vec())
                        })
                    };
                    if let Some(tx) = tx.lock().expect("snapshot callback lock").take() { let _ = tx.send(result); }
                });
                let _: () = msg_send![view, takeSnapshotWithConfiguration: std::ptr::null::<AnyObject>(), completionHandler: &*callback];
            }
        }).map_err(|e| DesignError::Capture(e.to_string()))?;
        return tokio::time::timeout(std::time::Duration::from_secs(5), rx).await.map_err(|_| DesignError::Timeout)?
            .map_err(|_| DesignError::Capture("native callback abandoned".into()))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
        Err(DesignError::Unsupported(if cfg!(target_os = "windows") {
            "Requires direct webview2-com 0.38.2 and windows 0.61 Win32_System_Com + Win32_UI_Shell; use ICoreWebView2::CapturePreview PNG into retained IStream"
        } else {
            "Requires direct webkit2gtk 2.0.2 v2_6 and cairo-rs 0.18 png; use WebViewExt::snapshot Visible/NONE on owning GLib context"
        }.into()))
    }
}
