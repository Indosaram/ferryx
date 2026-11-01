//! macOS system webview: camera and microphone requests from WKWebView.
//!
//! wry's `WryWebViewUIDelegate` answers `webView:requestMediaCapturePermissionForOrigin:...`
//! with Grant for every page. Ferryx replaces that one method on wry's class (once, on the main
//! thread) with [`media_capture_request`], which asks Ferryx for a browser tab's webview and
//! leaves WebKit's own prompt in place for any other webview. WKWebView on macOS has no web
//! notification or screen capture permission API, and it denies geolocation without a
//! CoreLocation delegate, so camera and microphone are the requests to route.

use super::{Answer, PermissionKind};
use block2::{Block, RcBlock};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::sel;
use objc2_web_kit::{WKMediaCaptureType, WKPermissionDecision, WKSecurityOrigin};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::Once;

type DecisionHandler = RcBlock<dyn Fn(WKPermissionDecision)>;

thread_local! {
    /// Browser tab webviews (WKWebView pointer to browser id). Main thread only.
    static TAB_VIEWS: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
    /// Decision handlers waiting for the user. Main thread only.
    static HANDLERS: RefCell<HashMap<u64, DecisionHandler>> = RefCell::new(HashMap::new());
    static NEXT_HANDLER: Cell<u64> = const { Cell::new(1) };
}

static SWIZZLE: Once = Once::new();

type MediaCaptureImp = unsafe extern "C-unwind" fn(
    *mut AnyObject,
    Sel,
    *mut AnyObject,
    *mut WKSecurityOrigin,
    *mut AnyObject,
    WKMediaCaptureType,
    *mut Block<dyn Fn(WKPermissionDecision)>,
);

/// The kinds a WebKit media capture type asks for.
pub fn capture_kinds(capture: WKMediaCaptureType) -> Vec<PermissionKind> {
    match capture {
        WKMediaCaptureType::Camera => vec![PermissionKind::Camera],
        WKMediaCaptureType::Microphone => vec![PermissionKind::Microphone],
        WKMediaCaptureType::CameraAndMicrophone => vec![PermissionKind::Camera, PermissionKind::Microphone],
        _ => vec![PermissionKind::Other],
    }
}

/// `protocol://host[:port]` of a WebKit security origin (port 0 means the default).
pub fn origin_string(protocol: &str, host: &str, port: isize) -> String {
    if port > 0 {
        format!("{protocol}://{host}:{port}")
    } else {
        format!("{protocol}://{host}")
    }
}

fn swizzle() {
    SWIZZLE.call_once(|| {
        let selector = sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:);
        let Some(class) = AnyClass::get(c"WryWebViewUIDelegate") else {
            tracing::warn!("wry's UI delegate class is missing: browser media requests keep WebKit's default prompt");
            return;
        };
        let Some(method) = class.instance_method(selector) else {
            tracing::warn!("wry's UI delegate has no media capture method: WebKit's default prompt applies");
            return;
        };
        let replacement: MediaCaptureImp = media_capture_request;
        // SAFETY: the replacement has exactly the method's Objective-C signature
        // (self, _cmd, WKWebView*, WKSecurityOrigin*, WKFrameInfo*, NSInteger, block), which
        // objc2-web-kit declares for this WKUIDelegate selector.
        unsafe {
            method.set_implementation(std::mem::transmute::<MediaCaptureImp, Imp>(replacement));
        }
    });
}

/// Routes a browser tab's media requests through Ferryx. Main thread, after creation.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>, browser_id: &str) {
    let browser_id = browser_id.to_string();
    let result = webview.with_webview(move |platform| {
        swizzle();
        let view = platform.inner() as usize;
        TAB_VIEWS.with_borrow_mut(|views| views.insert(view, browser_id));
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not route the browser tab's media requests; WebKit prompts instead");
    }
}

/// Forgets a closed tab's webview. Main thread.
pub fn forget_browser(browser_id: &str) {
    TAB_VIEWS.with_borrow_mut(|views| views.retain(|_, id| id != browser_id));
}

fn decide(token: u64, allow: bool) {
    if let Some(handler) = HANDLERS.with_borrow_mut(|handlers| handlers.remove(&token)) {
        handler.call((if allow { WKPermissionDecision::Grant } else { WKPermissionDecision::Deny },));
    }
}

unsafe extern "C-unwind" fn media_capture_request(
    _this: *mut AnyObject,
    _cmd: Sel,
    webview: *mut AnyObject,
    origin: *mut WKSecurityOrigin,
    _frame: *mut AnyObject,
    capture: WKMediaCaptureType,
    handler: *mut Block<dyn Fn(WKPermissionDecision)>,
) {
    // SAFETY: WebKit passes a live decision block for the duration of the call.
    let Some(handler) = (unsafe { handler.as_ref() }) else { return };
    let browser_id = TAB_VIEWS.with_borrow(|views| views.get(&(webview as usize)).cloned());
    let Some(browser_id) = browser_id else {
        // Not a browser tab (Ferryx's own UI): WebKit's own prompt, never a silent grant.
        handler.call((WKPermissionDecision::Prompt,));
        return;
    };
    // SAFETY: WebKit passes a live security origin for the duration of the call.
    let origin = unsafe { origin.as_ref() }.map(|origin| unsafe {
        origin_string(&origin.protocol().to_string(), &origin.host().to_string(), origin.port())
    });
    let token = NEXT_HANDLER.get();
    NEXT_HANDLER.set(token + 1);
    HANDLERS.with_borrow_mut(|handlers| handlers.insert(token, handler.copy()));
    let answer = Answer::new(Box::new(move |allow| {
        super::run_on_main(Box::new(move || decide(token, allow)));
    }));
    super::request_for_browser(&browser_id, origin, capture_kinds(capture), answer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_types_name_camera_and_microphone() {
        assert_eq!(capture_kinds(WKMediaCaptureType::Camera), vec![PermissionKind::Camera]);
        assert_eq!(capture_kinds(WKMediaCaptureType::Microphone), vec![PermissionKind::Microphone]);
        assert_eq!(
            capture_kinds(WKMediaCaptureType::CameraAndMicrophone),
            vec![PermissionKind::Camera, PermissionKind::Microphone]
        );
    }

    #[test]
    fn origins_keep_a_non_default_port() {
        assert_eq!(origin_string("https", "meet.test", 0), "https://meet.test");
        assert_eq!(origin_string("http", "localhost", 3000), "http://localhost:3000");
    }
}
