//! Linux system webview: WebKitGTK `permission-request`.
//!
//! WebKitGTK raises one signal for every permission: camera and microphone (and screen capture,
//! a display device) as user media requests, notifications, location, and the rest. Ferryx
//! handles the signal for browser tabs, holds the request (a GObject that stays pending while
//! referenced) and allows or denies it with the user's answer. A request whose answer never
//! comes is denied when it is dropped, never granted.

use super::{Answer, PermissionKind};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use webkit2gtk::glib::object::{Cast, ObjectType};
use webkit2gtk::{
    GeolocationPermissionRequest, NotificationPermissionRequest, PermissionRequest, PermissionRequestExt,
    UserMediaPermissionRequest, UserMediaPermissionRequestExt, WebViewExt,
};

thread_local! {
    /// Requests waiting for the user. Main thread only (GObjects).
    static HELD: RefCell<HashMap<u64, PermissionRequest>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// What a WebKitGTK request asks for.
fn request_kinds(request: &PermissionRequest) -> Vec<PermissionKind> {
    if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
        // SAFETY: a live WebKitUserMediaPermissionRequest pointer for the call.
        let display = unsafe {
            webkit2gtk::ffi::webkit_user_media_permission_is_for_display_device(media.as_ptr()) != 0
        };
        return media_kinds(media.is_for_video_device(), media.is_for_audio_device(), display);
    }
    if request.is::<NotificationPermissionRequest>() {
        return vec![PermissionKind::Notifications];
    }
    if request.is::<GeolocationPermissionRequest>() {
        return vec![PermissionKind::Geolocation];
    }
    vec![PermissionKind::Other]
}

/// The kinds a user media request asks for; a display device is screen capture.
pub fn media_kinds(video: bool, audio: bool, display: bool) -> Vec<PermissionKind> {
    let mut kinds = Vec::new();
    if display {
        kinds.push(PermissionKind::Screen);
    } else if video {
        kinds.push(PermissionKind::Camera);
    }
    if audio {
        kinds.push(PermissionKind::Microphone);
    }
    if kinds.is_empty() {
        kinds.push(PermissionKind::Other);
    }
    kinds
}

fn complete(token: u64, allow: bool) {
    if let Some(request) = HELD.with_borrow_mut(|held| held.remove(&token)) {
        if allow {
            request.allow();
        } else {
            request.deny();
        }
    }
}

/// Routes a browser tab's permission requests through Ferryx. Main thread, after creation.
pub fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>, browser_id: &str) {
    let browser_id = browser_id.to_string();
    let result = webview.with_webview(move |platform| {
        platform.inner().connect_permission_request(move |view, request| {
            let token = NEXT.get();
            NEXT.set(token + 1);
            let kinds = request_kinds(request);
            HELD.with_borrow_mut(|held| held.insert(token, request.clone()));
            let answer = Answer::new(Box::new(move |allow| {
                super::run_on_main(Box::new(move || complete(token, allow)));
            }));
            let origin = view.uri().map(|uri| uri.to_string());
            super::request_for_browser(&browser_id, origin, kinds, answer);
            // Handled: WebKitGTK's default (deny) does not run.
            true
        });
    });
    if let Err(error) = result {
        tracing::warn!(%error, "could not route the browser tab's permission requests");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_media_requests_name_camera_microphone_and_screen() {
        assert_eq!(media_kinds(true, true, false), vec![PermissionKind::Camera, PermissionKind::Microphone]);
        assert_eq!(media_kinds(true, false, true), vec![PermissionKind::Screen]);
        assert_eq!(media_kinds(false, true, false), vec![PermissionKind::Microphone]);
        assert_eq!(media_kinds(false, false, false), vec![PermissionKind::Other]);
    }
}
