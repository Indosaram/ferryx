//! WKWebView: wry's download delegate decides the destination of every `WKDownload`. Ferryx
//! wraps that one delegate method (once per process) to keep the download object, which
//! cancels, and pauses by cancelling with resume data that `WKWebView` resumes from
//! (macOS 11.3+, the same releases that have `WKDownload`).

use crate::browser::download::{registry, DownloadControl};
use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, ProtocolObject, Sel};
use objc2::{sel, MainThreadMarker, Message};
use objc2_foundation::{NSData, NSURL};
use objc2_web_kit::{WKDownload, WKDownloadDelegate};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::{Arc, OnceLock};

/// One download Ferryx can steer. Main thread only.
struct Entry {
    download: Retained<WKDownload>,
    /// wry's delegate; a resumed download needs it to report its end.
    delegate: Option<Retained<ProtocolObject<dyn WKDownloadDelegate>>>,
    /// Set while paused: what WebKit resumes from.
    resume_data: Option<Retained<NSData>>,
}

thread_local! {
    /// Live downloads by download id. Main thread only.
    static DOWNLOADS: RefCell<HashMap<String, Entry>> = RefCell::new(HashMap::new());
    static SWIZZLED: Cell<bool> = const { Cell::new(false) };
}

type DecideDestinationImp = unsafe extern "C-unwind" fn(
    *mut AnyObject,
    Sel,
    *mut WKDownload,
    *mut AnyObject,
    *mut AnyObject,
    *mut Block<dyn Fn(*const NSURL)>,
);

/// wry's own implementation, called first by the replacement.
static ORIGINAL: OnceLock<DecideDestinationImp> = OnceLock::new();

/// Steers one download from any thread by posting to the main queue.
struct Control {
    id: String,
}

impl Control {
    fn post(&self, action: fn(&str)) {
        let id = self.id.clone();
        dispatch2::DispatchQueue::main().exec_async(move || action(&id));
    }
}

impl DownloadControl for Control {
    fn cancel(&self) {
        self.post(cancel);
    }

    fn pause(&self) {
        self.post(pause);
    }

    fn resume(&self) {
        self.post(resume);
    }
}

fn cancel(id: &str) {
    let Some(entry) = DOWNLOADS.with_borrow_mut(|downloads| downloads.remove(id)) else { return };
    if entry.resume_data.is_none() {
        // SAFETY: a live WKDownload, on the main thread; no completion block is needed.
        unsafe { entry.download.cancel(None) };
    }
}

fn pause(id: &str) {
    let Some(download) = DOWNLOADS.with_borrow(|downloads| downloads.get(id).map(|entry| entry.download.clone())) else {
        return;
    };
    // Paused first: WebKit reports the cancel to wry as a failure before the block runs.
    registry().set_paused(id, true);
    let paused_id = id.to_string();
    let keep: RcBlock<dyn Fn(*mut NSData)> = RcBlock::new(move |data: *mut NSData| {
        if MainThreadMarker::new().is_none() {
            tracing::warn!("WebKit returned resume data off the main thread; the download cannot resume");
            return;
        }
        // SAFETY: WebKit passes resume data, or null, that is valid for the call.
        match unsafe { Retained::retain(data) } {
            Some(data) => DOWNLOADS.with_borrow_mut(|downloads| {
                if let Some(entry) = downloads.get_mut(&paused_id) {
                    entry.resume_data = Some(data);
                }
            }),
            None => {
                DOWNLOADS.with_borrow_mut(|downloads| downloads.remove(&paused_id));
                registry().set_failed(&paused_id, "the server does not allow resuming this download".into());
            }
        }
    });
    // SAFETY: a live WKDownload, on the main thread, with a block of the declared signature.
    unsafe { download.cancel(Some(&*keep)) };
}

fn resume(id: &str) {
    let found = DOWNLOADS.with_borrow_mut(|downloads| {
        let entry = downloads.get_mut(id)?;
        Some((entry.resume_data.take()?, entry.download.clone(), entry.delegate.clone()))
    });
    let Some((data, old, delegate)) = found else { return };
    // SAFETY: a WKDownload, read on the main thread.
    let Some(webview) = (unsafe { old.webView() }) else {
        tracing::warn!("the paused download's tab is gone; it cannot resume");
        return;
    };
    let resumed_id = id.to_string();
    let adopt_resumed: RcBlock<dyn Fn(NonNull<WKDownload>)> = RcBlock::new(move |download: NonNull<WKDownload>| {
        // SAFETY: WebKit passes a live WKDownload for the call.
        let Some(download) = (unsafe { Retained::retain(download.as_ptr()) }) else { return };
        // SAFETY: wry's delegate, set on the main thread before WebKit reports anything else.
        unsafe { download.setDelegate(delegate.as_deref()) };
        DOWNLOADS.with_borrow_mut(|downloads| {
            if let Some(entry) = downloads.get_mut(&resumed_id) {
                entry.download = download;
            }
        });
        registry().set_paused(&resumed_id, false);
    });
    // SAFETY: WebKit's own resume data, on the main thread, with a block of the declared
    // signature. `WKDownload` exists, so this macOS 11.3 method does too.
    unsafe { webview.resumeDownloadFromResumeData_completionHandler(&data, &adopt_resumed) };
}

/// Keeps a download wry just gave a destination, if a browser tab registered it.
fn adopt(download: &WKDownload) {
    // SAFETY: a live WKDownload, read on the main thread.
    let request = unsafe { download.originalRequest() };
    let Some(url) = request.and_then(|request| request.URL()).and_then(|url| url.absoluteString()) else { return };
    let control = |id: &str| -> Arc<dyn DownloadControl> { Arc::new(Control { id: id.to_string() }) };
    let Some(id) = super::attach(&url.to_string(), true, control) else { return };
    // SAFETY: a live WKDownload, read on the main thread.
    let delegate = unsafe { download.delegate() };
    let entry = Entry { download: download.retain(), delegate, resume_data: None };
    DOWNLOADS.with_borrow_mut(|downloads| downloads.insert(id, entry));
}

unsafe extern "C-unwind" fn decide_destination(
    this: *mut AnyObject,
    cmd: Sel,
    download: *mut WKDownload,
    response: *mut AnyObject,
    filename: *mut AnyObject,
    handler: *mut Block<dyn Fn(*const NSURL)>,
) {
    match ORIGINAL.get().copied() {
        // SAFETY: WebKit's own arguments, passed on to the implementation they were meant for.
        Some(original) => unsafe { original(this, cmd, download, response, filename, handler) },
        None => {
            // SAFETY: WebKit passes a live destination block; null declines the download.
            if let Some(handler) = unsafe { handler.as_ref() } {
                handler.call((std::ptr::null(),));
            }
            return;
        }
    }
    // SAFETY: WebKit passes a live WKDownload for the duration of the call.
    if let Some(download) = unsafe { download.as_ref() } {
        adopt(download);
    }
}

/// wry's download delegate class. Its runtime name is generated from wry's module path and
/// version, so it is found by suffix.
fn wry_download_delegate() -> Option<&'static AnyClass> {
    AnyClass::get(c"WryDownloadDelegate").or_else(|| {
        AnyClass::classes().iter().copied().find(|class| {
            class.name().to_str().is_ok_and(|name| name.contains("::WryDownloadDelegate"))
        })
    })
}

/// Wraps wry's destination method once wry registered its class (it does when it creates the
/// first webview with a download handler).
fn swizzle() {
    if SWIZZLED.get() {
        return;
    }
    let selector = sel!(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:);
    let Some(method) = wry_download_delegate().and_then(|class| class.instance_method(selector)) else {
        tracing::warn!("wry's download delegate is missing: browser downloads have no Cancel or Pause");
        return;
    };
    SWIZZLED.set(true);
    // SAFETY: the method's current implementation has exactly the DecideDestinationImp
    // signature (self, _cmd, WKDownload*, NSURLResponse*, NSString*, block), which
    // objc2-web-kit declares for this WKDownloadDelegate selector.
    let original = unsafe { std::mem::transmute::<Imp, DecideDestinationImp>(method.implementation()) };
    if ORIGINAL.set(original).is_err() {
        return;
    }
    let replacement: DecideDestinationImp = decide_destination;
    // SAFETY: the replacement has the same signature as the method it replaces (see above).
    unsafe {
        method.set_implementation(std::mem::transmute::<DecideDestinationImp, Imp>(replacement));
    }
}

/// Drops a finished download's object. Main thread.
pub fn forget(id: &str) {
    DOWNLOADS.with_borrow_mut(|downloads| downloads.remove(id));
}

/// Wraps wry's download delegate. Main thread, after the tab was created.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    if let Err(error) = webview.with_webview(|_| swizzle()) {
        tracing::warn!(%error, "could not reach the browser tab to hook its downloads");
    }
}
