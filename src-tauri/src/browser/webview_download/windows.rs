//! WebView2: `DownloadStarting` hands over the `ICoreWebView2DownloadOperation`, which cancels,
//! pauses, resumes and reports progress. COM objects stay on the UI thread; the control only
//! posts work there.

use crate::browser::download::{registry, DownloadControl};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2DownloadOperation, ICoreWebView2_4, COREWEBVIEW2_DOWNLOAD_STATE,
    COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED, COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS,
};
use webview2_com::{take_pwstr, BytesReceivedChangedEventHandler, DownloadStartingEventHandler, StateChangedEventHandler};
use windows::core::{Interface, BOOL, PWSTR};

thread_local! {
    /// Live download operations by download id. UI thread only.
    static OPERATIONS: RefCell<HashMap<String, ICoreWebView2DownloadOperation>> = RefCell::new(HashMap::new());
}

/// Steers one download from any thread by posting to the UI thread.
struct Control<R: Runtime> {
    id: String,
    app: AppHandle<R>,
}

impl<R: Runtime> Control<R> {
    fn post(&self, action: fn(&str, &ICoreWebView2DownloadOperation)) {
        let id = self.id.clone();
        let posted = self.app.run_on_main_thread(move || {
            if let Some(operation) = OPERATIONS.with_borrow(|operations| operations.get(&id).cloned()) {
                action(&id, &operation);
            }
        });
        if let Err(error) = posted {
            tracing::warn!(%error, download_id = %self.id, "could not reach the download on the UI thread");
        }
    }
}

impl<R: Runtime> DownloadControl for Control<R> {
    fn cancel(&self) {
        self.post(|id, operation| {
            // SAFETY: a live download operation, used on the UI thread that owns it.
            if let Err(error) = unsafe { operation.Cancel() } {
                tracing::warn!(%error, "WebView2 did not cancel the download");
            }
            // A paused (interrupted) download may report no further state change.
            forget(id);
        });
    }

    fn pause(&self) {
        self.post(|id, operation| {
            // Paused first: WebView2 reports the stop to wry as a failed finish right away.
            registry().set_paused(id, true);
            // SAFETY: a live download operation, used on the UI thread that owns it.
            if let Err(error) = unsafe { operation.Pause() } {
                tracing::warn!(%error, "WebView2 did not pause the download");
                registry().set_paused(id, false);
            }
        });
    }

    fn resume(&self) {
        self.post(|_, operation| {
            // SAFETY: a live download operation, used on the UI thread that owns it.
            if let Err(error) = unsafe { operation.Resume() } {
                tracing::warn!(%error, "WebView2 did not resume the download");
            }
        });
    }
}

/// Interrupted in a state WebView2 can resume from (a user pause, or a dropped connection).
fn resumable_stop(operation: &ICoreWebView2DownloadOperation) -> bool {
    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
    let mut can_resume = BOOL::default();
    // SAFETY: a live download operation and valid out pointers, on the UI thread.
    let read = unsafe { operation.State(&mut state).and_then(|()| operation.CanResume(&mut can_resume)) };
    read.is_ok() && state == COREWEBVIEW2_DOWNLOAD_STATE_INTERRUPTED && can_resume.as_bool()
}

/// See [`super::engine_paused`]. UI thread.
pub fn engine_paused(id: &str) -> bool {
    OPERATIONS.with_borrow(|operations| operations.get(id).is_some_and(resumable_stop))
}

/// Drops a finished download's operation. UI thread.
pub fn forget(id: &str) {
    OPERATIONS.with_borrow_mut(|operations| operations.remove(id));
}

fn report_progress(id: &str, operation: &ICoreWebView2DownloadOperation) -> windows::core::Result<()> {
    let (mut received, mut total) = (0i64, 0i64);
    // SAFETY: a live download operation and valid out pointers, on the UI thread.
    unsafe {
        operation.BytesReceived(&mut received)?;
        operation.TotalBytesToReceive(&mut total)?;
    }
    let total = u64::try_from(total).ok().filter(|&total| total > 0);
    registry().update_native(id, u64::try_from(received).unwrap_or(0), total, None);
    Ok(())
}

fn report_state(id: &str, operation: &ICoreWebView2DownloadOperation) -> windows::core::Result<()> {
    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
    // SAFETY: a live download operation and a valid out pointer, on the UI thread.
    unsafe { operation.State(&mut state)? };
    if state == COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
        registry().set_paused(id, false);
    } else if resumable_stop(operation) {
        registry().set_paused(id, true);
    }
    Ok(())
}

/// Takes over a download wry just let start. Runs after wry's own `DownloadStarting` handler,
/// so the download handler has registered the record by now.
fn adopt<R: Runtime>(app: &AppHandle<R>, operation: ICoreWebView2DownloadOperation) -> windows::core::Result<()> {
    let url = {
        let mut uri = PWSTR::null();
        // SAFETY: a live download operation and a valid out pointer, on the UI thread.
        unsafe { operation.Uri(&mut uri)? };
        take_pwstr(uri)
    };
    let control = |id: &str| -> Arc<dyn DownloadControl> { Arc::new(Control { id: id.to_string(), app: app.clone() }) };
    let Some(id) = super::attach(&url, true, control) else { return Ok(()) };
    OPERATIONS.with_borrow_mut(|operations| operations.insert(id.clone(), operation.clone()));

    let progress_id = id.clone();
    let progress = BytesReceivedChangedEventHandler::create(Box::new(move |operation, _| match operation {
        Some(operation) => report_progress(&progress_id, &operation),
        None => Ok(()),
    }));
    let state = StateChangedEventHandler::create(Box::new(move |operation, _| match operation {
        Some(operation) => report_state(&id, &operation),
        None => Ok(()),
    }));
    let mut token = 0i64;
    // SAFETY: a live download operation and valid handlers, on the UI thread.
    unsafe {
        operation.add_BytesReceivedChanged(&progress, &mut token)?;
        operation.add_StateChanged(&state, &mut token)?;
    }
    Ok(())
}

/// Hooks the tab's `DownloadStarting`. UI thread, after creation.
pub fn install<R: Runtime>(webview: &tauri::Webview<R>) {
    let app = webview.app_handle().clone();
    let result = webview.with_webview(move |platform| {
        // SAFETY: the tab's live controller, on the UI thread that owns it.
        let core = match unsafe { platform.controller().CoreWebView2() }.and_then(|core| core.cast::<ICoreWebView2_4>()) {
            Ok(core) => core,
            Err(error) => {
                tracing::warn!(%error, "no ICoreWebView2_4: browser downloads have no Cancel or Pause");
                return;
            }
        };
        let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            // SAFETY: WebView2 passes live event args for the duration of the call.
            let operation = unsafe { args.DownloadOperation() }?;
            if let Err(error) = adopt(&app, operation) {
                tracing::warn!(%error, "could not take over a browser download");
            }
            Ok(())
        }));
        let mut token = 0i64;
        // SAFETY: a live ICoreWebView2_4 and a valid handler, on the UI thread.
        if let Err(error) = unsafe { core.add_DownloadStarting(&handler, &mut token) } {
            tracing::warn!(%error, "could not hook browser downloads");
        }
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not reach the browser tab to hook its downloads");
    }
}
