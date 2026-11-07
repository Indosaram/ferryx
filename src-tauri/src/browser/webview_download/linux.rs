//! WebKitGTK: each web context raises `download-started` with the `WebKitDownload`, which can
//! be cancelled and reports progress. WebKitGTK 2.x has no pause or resume, so the control is
//! cancel-only.

use crate::browser::download::{registry, DownloadControl};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use webkit2gtk::glib::{self, ObjectExt, ObjectType, SendWeakRef};
use webkit2gtk::{Download, DownloadExt, URIRequestExt, URIResponseExt, WebContextExt, WebViewExt};

thread_local! {
    /// Web contexts already hooked (tabs of one profile share one). Main thread only.
    static HOOKED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

/// Cancels one download from any thread through the GLib main context.
struct Control {
    download: SendWeakRef<Download>,
}

impl DownloadControl for Control {
    fn cancel(&self) {
        let download = self.download.clone();
        // The weak reference is only dereferenced on the main context, where it was made.
        glib::MainContext::default().invoke(move || {
            if let Some(download) = download.upgrade() {
                download.cancel();
            }
        });
    }

    fn pause(&self) {}

    fn resume(&self) {}
}

fn content_length(download: &Download) -> Option<u64> {
    download.response().map(|response| response.content_length()).filter(|&length| length > 0)
}

/// Takes over a download once wry gave it a destination (wry answers `decide-destination`
/// itself, which stops the signal before any later handler).
fn watch(download: &Download) {
    let id: Rc<RefCell<Option<String>>> = Rc::default();
    let adopted = Rc::clone(&id);
    download.connect_created_destination(move |download, _destination| {
        let Some(url) = download.request().and_then(|request| request.uri()) else { return };
        let control = |_: &str| -> Arc<dyn DownloadControl> { Arc::new(Control { download: download.downgrade().into() }) };
        if let Some(attached) = super::attach(&url, false, control) {
            registry().update_native(&attached, download.received_data_length(), content_length(download), None);
            *adopted.borrow_mut() = Some(attached);
        }
    });
    download.connect_received_data(move |download, _length| {
        if let Some(id) = id.borrow().as_deref() {
            registry().update_native(id, download.received_data_length(), content_length(download), None);
        }
    });
}

/// Hooks the tab's web context once. Main thread, after creation.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    let result = webview.with_webview(|platform| {
        let Some(context) = platform.inner().context() else {
            tracing::warn!("browser tab has no web context: its downloads have no Cancel");
            return;
        };
        let fresh = HOOKED.with_borrow_mut(|hooked| hooked.insert(context.as_ptr() as usize));
        if fresh {
            context.connect_download_started(|_, download| watch(download));
        }
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not reach the browser tab to hook its downloads");
    }
}
