use crate::ipc::error::IpcError;

pub async fn print<R: tauri::Runtime>(webview: tauri::Webview<R>) -> Result<bool, IpcError> {
    let (send, receive) = tokio::sync::oneshot::channel();
    webview.with_webview(move |platform| {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: Tauri supplies its live WKWebView on the UI thread; the borrow
            // stays inside this closure and the print operation retains its view.
            let native: &objc2_web_kit::WKWebView = unsafe { &*platform.inner().cast() };
            let info = objc2_app_kit::NSPrintInfo::sharedPrintInfo();
            // SAFETY: info is a retained NSPrintInfo and native remains live on its UI thread.
            let operation = unsafe { native.printOperationWithPrintInfo(&info) };
            operation.setCanSpawnSeparateThread(false);
            operation.runOperation();
            let _ = send.send(Ok(true));
        }
        #[cfg(target_os = "linux")]
        {
            use std::cell::RefCell;
            use std::rc::Rc;
            use webkit2gtk::PrintOperationExt;
            let operation = webkit2gtk::PrintOperation::new(&platform.inner());
            let completion = Rc::new(RefCell::new(Some(send)));
            let finished = Rc::clone(&completion);
            operation.connect_finished(move |_| {
                if let Some(send) = finished.borrow_mut().take() {
                    let _ = send.send(Ok(true));
                }
            });
            let failed = Rc::clone(&completion);
            operation.connect_failed(move |_, error| {
                if let Some(send) = failed.borrow_mut().take() {
                    let _ = send.send(Err(IpcError::internal(error.to_string())));
                }
            });
            if operation.run_dialog(None::<&gtk::Window>) == webkit2gtk::PrintOperationResponse::Cancel {
                if let Some(send) = completion.borrow_mut().take() {
                    let _ = send.send(Ok(true));
                }
            }
        }
    }).map_err(|error| IpcError::internal(error.to_string()))?;
    receive.await.map_err(|error| IpcError::internal(error.to_string()))?
}
