//! Windows system webview: WebView2 `PermissionRequested`.
//!
//! Every request a browser tab's WebView2 raises (camera, microphone, notifications, location,
//! clipboard reading and the rest) is deferred and asked about in Ferryx. The decision is not
//! saved in the WebView2 profile (`SavesInProfile` off), so revoking in Ferryx takes effect on
//! the next request. A request is denied when its answer never comes. Screen capture
//! (getDisplayMedia) is not a WebView2 permission kind: WebView2 shows its own source picker.

use super::{Answer, PermissionKind};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Deferral, ICoreWebView2PermissionRequestedEventArgs,
    ICoreWebView2PermissionRequestedEventArgs3, COREWEBVIEW2_PERMISSION_KIND,
    COREWEBVIEW2_PERMISSION_KIND_CAMERA, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
    COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION, COREWEBVIEW2_PERMISSION_KIND_MICROPHONE,
    COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    COREWEBVIEW2_PERMISSION_STATE_DENY,
};
use webview2_com::{take_pwstr, PermissionRequestedEventHandler};
use windows::core::{Interface, PWSTR};

struct Held {
    args: ICoreWebView2PermissionRequestedEventArgs,
    deferral: ICoreWebView2Deferral,
}

thread_local! {
    /// Deferred requests waiting for the user. UI thread only (COM objects).
    static HELD: RefCell<HashMap<u64, Held>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// The Ferryx kind of a WebView2 permission kind.
pub fn permission_kind(kind: COREWEBVIEW2_PERMISSION_KIND) -> PermissionKind {
    match kind {
        COREWEBVIEW2_PERMISSION_KIND_CAMERA => PermissionKind::Camera,
        COREWEBVIEW2_PERMISSION_KIND_MICROPHONE => PermissionKind::Microphone,
        COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS => PermissionKind::Notifications,
        COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION => PermissionKind::Geolocation,
        COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ => PermissionKind::Clipboard,
        _ => PermissionKind::Other,
    }
}

fn complete(token: u64, allow: bool) {
    let Some(held) = HELD.with_borrow_mut(|held| held.remove(&token)) else { return };
    let state = if allow { COREWEBVIEW2_PERMISSION_STATE_ALLOW } else { COREWEBVIEW2_PERMISSION_STATE_DENY };
    if let Err(error) = unsafe { held.args.SetState(state) } {
        tracing::warn!(%error, "could not set a WebView2 permission state");
    }
    if let Err(error) = unsafe { held.deferral.Complete() } {
        tracing::warn!(%error, "could not complete a WebView2 permission deferral");
    }
}

fn on_request(browser_id: &str, args: ICoreWebView2PermissionRequestedEventArgs) -> windows::core::Result<()> {
    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
    unsafe { args.PermissionKind(&mut kind) }?;
    let mut uri = PWSTR::null();
    unsafe { args.Uri(&mut uri) }?;
    let origin = take_pwstr(uri);
    // Ferryx remembers decisions itself; WebView2 must not answer the next request on its own.
    if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
        unsafe { args3.SetSavesInProfile(false) }?;
    }
    // Until the deferral completes the request is pending; set Deny first so a request whose
    // deferral is dropped is never granted by default.
    unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY) }?;
    let deferral = unsafe { args.GetDeferral() }?;
    let token = NEXT.get();
    NEXT.set(token + 1);
    HELD.with_borrow_mut(|held| held.insert(token, Held { args, deferral }));
    let answer = Answer::new(Box::new(move |allow| {
        super::run_on_main(Box::new(move || complete(token, allow)));
    }));
    super::request_for_browser(browser_id, Some(origin), vec![permission_kind(kind)], answer);
    Ok(())
}

/// Routes a browser tab's permission requests through Ferryx. UI thread, after creation.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>, browser_id: &str) {
    let browser_id = browser_id.to_string();
    let result = webview.with_webview(move |platform| {
        let core = match unsafe { platform.controller().CoreWebView2() } {
            Ok(core) => core,
            Err(error) => {
                tracing::warn!(%error, "no CoreWebView2 for site permissions");
                return;
            }
        };
        let handler = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            on_request(&browser_id, args)
        }));
        let mut token = 0i64;
        if let Err(error) = unsafe { core.add_PermissionRequested(&handler, &mut token) } {
            tracing::warn!(%error, "could not route the browser tab's permission requests");
        }
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not reach the browser tab to route its permission requests");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview2_kinds_map_to_ferryx_kinds() {
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND_CAMERA), PermissionKind::Camera);
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND_MICROPHONE), PermissionKind::Microphone);
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS), PermissionKind::Notifications);
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND_GEOLOCATION), PermissionKind::Geolocation);
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ), PermissionKind::Clipboard);
        assert_eq!(permission_kind(COREWEBVIEW2_PERMISSION_KIND::default()), PermissionKind::Other);
    }
}
