//! Site permission requests: CEF asks, Ferryx decides.
//!
//! Camera, microphone and screen capture arrive as media access requests; notifications,
//! location, clipboard and the rest as permission prompts. Each one is held here and reported to
//! Ferryx as [`Event::PermissionRequest`]; [`Command::Permission`] answers it. A request from a
//! window that is not a Ferryx tab is denied at once, and nothing is granted without Ferryx's
//! answer: when Ferryx quits, the engine quits with the request still unanswered.
//!
//! [`Command::Permission`]: ferryx_browser_engine_protocol::Command::Permission

use cef::*;
use ferryx_browser_engine_protocol::{Event, PermissionKind};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// CEF media access bits (cef_media_access_permission_types_t).
const DEVICE_AUDIO: u32 = 1;
const DEVICE_VIDEO: u32 = 2;
const DESKTOP_AUDIO: u32 = 4;
const DESKTOP_VIDEO: u32 = 8;

/// CEF permission prompt bits (cef_permission_request_types_t) Ferryx names.
const PROMPT_CAMERA: u32 = 4;
const PROMPT_CLIPBOARD: u32 = 16;
const PROMPT_GEOLOCATION: u32 = 256;
const PROMPT_MIC: u32 = 4096;
const PROMPT_NOTIFICATIONS: u32 = 32768;

/// The kinds a media access request asks for.
pub fn media_kinds(bits: u32) -> Vec<PermissionKind> {
    let mut kinds = Vec::new();
    if bits & DEVICE_VIDEO != 0 {
        kinds.push(PermissionKind::Camera);
    }
    if bits & DEVICE_AUDIO != 0 {
        kinds.push(PermissionKind::Microphone);
    }
    if bits & (DESKTOP_VIDEO | DESKTOP_AUDIO) != 0 {
        kinds.push(PermissionKind::Screen);
    }
    kinds
}

/// The kinds a permission prompt asks for; bits Ferryx has no name for are `Other`.
pub fn prompt_kinds(bits: u32) -> Vec<PermissionKind> {
    let named = [
        (PROMPT_CAMERA, PermissionKind::Camera),
        (PROMPT_MIC, PermissionKind::Microphone),
        (PROMPT_NOTIFICATIONS, PermissionKind::Notifications),
        (PROMPT_GEOLOCATION, PermissionKind::Geolocation),
        (PROMPT_CLIPBOARD, PermissionKind::Clipboard),
    ];
    let mut kinds: Vec<PermissionKind> =
        named.iter().filter(|(bit, _)| bits & bit != 0).map(|(_, kind)| *kind).collect();
    let known = named.iter().fold(0, |all, (bit, _)| all | bit);
    if bits & !known != 0 {
        kinds.push(PermissionKind::Other);
    }
    kinds
}

enum Held {
    Media { callback: MediaAccessCallback, requested: u32 },
    Prompt { callback: PermissionPromptCallback, prompt_id: u64 },
}

struct Pending {
    tab: String,
    held: Held,
}

thread_local! {
    static PENDING: RefCell<HashMap<u64, Pending>> = RefCell::new(HashMap::new());
    static NEXT_REQUEST: Cell<u64> = const { Cell::new(1) };
}

fn hold(tab: String, held: Held, kinds: Vec<PermissionKind>, origin: &str) {
    let request = NEXT_REQUEST.get();
    NEXT_REQUEST.set(request + 1);
    PENDING.with_borrow_mut(|pending| pending.insert(request, Pending { tab: tab.clone(), held }));
    super::emit(Event::PermissionRequest { tab, request, kinds, origin: origin.to_string() });
}

fn deny(held: &Held) {
    match held {
        Held::Media { callback, .. } => callback.cancel(),
        Held::Prompt { callback, .. } => callback.cont(PermissionRequestResult::DENY),
    }
}

/// Applies Ferryx's answer; an unknown request (already dismissed) is ignored.
pub fn answer(request: u64, allow: bool) {
    let Some(pending) = PENDING.with_borrow_mut(|pending| pending.remove(&request)) else {
        return;
    };
    match (&pending.held, allow) {
        (Held::Media { callback, requested }, true) => callback.cont(*requested),
        (Held::Prompt { callback, .. }, true) => callback.cont(PermissionRequestResult::ACCEPT),
        (held, false) => deny(held),
    }
}

/// The tab's browser closed: its requests are denied and their prompts go.
pub fn forget_tab(tab: &str) {
    let dropped: Vec<(u64, Pending)> = PENDING.with_borrow_mut(|pending| {
        let ids: Vec<u64> = pending.iter().filter(|(_, held)| held.tab == tab).map(|(id, _)| *id).collect();
        ids.into_iter().filter_map(|id| pending.remove(&id).map(|held| (id, held))).collect()
    });
    for (request, held) in dropped {
        deny(&held.held);
        super::emit(Event::PermissionDismissed { tab: tab.to_string(), request });
    }
}

fn tab_of(browser: &mut Browser) -> Option<String> {
    match super::key_of(browser) {
        Some(super::Key::Tab(tab)) => Some(tab),
        _ => None,
    }
}

wrap_permission_handler! {
    pub struct EnginePermissionHandler;

    impl PermissionHandler {
        fn on_request_media_access_permission(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut MediaAccessCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback.cloned() else { return 0 };
            let kinds = media_kinds(requested_permissions);
            let tab = browser.and_then(tab_of);
            let origin = requesting_origin.map(CefString::to_string).unwrap_or_default();
            match tab {
                Some(tab) if !kinds.is_empty() => {
                    hold(tab, Held::Media { callback, requested: requested_permissions }, kinds, &origin);
                }
                // Not a Ferryx tab, or nothing Ferryx can name: denied, never granted by default.
                _ => callback.cancel(),
            }
            1
        }

        fn on_show_permission_prompt(
            &self,
            browser: Option<&mut Browser>,
            prompt_id: u64,
            requesting_origin: Option<&CefString>,
            requested_permissions: u32,
            callback: Option<&mut PermissionPromptCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback.cloned() else { return 0 };
            let kinds = prompt_kinds(requested_permissions);
            let tab = browser.and_then(tab_of);
            let origin = requesting_origin.map(CefString::to_string).unwrap_or_default();
            match tab {
                Some(tab) if !kinds.is_empty() => {
                    hold(tab, Held::Prompt { callback, prompt_id }, kinds, &origin);
                }
                _ => callback.cont(PermissionRequestResult::DENY),
            }
            1
        }

        fn on_dismiss_permission_prompt(
            &self,
            _browser: Option<&mut Browser>,
            prompt_id: u64,
            _result: PermissionRequestResult,
        ) {
            // Chromium dropped the prompt itself (navigation, the tab closed): Ferryx's prompt
            // goes too, and the request stays denied.
            let dismissed = PENDING.with_borrow_mut(|pending| {
                let id = pending
                    .iter()
                    .find(|(_, held)| matches!(held.held, Held::Prompt { prompt_id: id, .. } if id == prompt_id))
                    .map(|(id, _)| *id)?;
                pending.remove(&id).map(|held| (id, held.tab))
            });
            if let Some((request, tab)) = dismissed {
                super::emit(Event::PermissionDismissed { tab, request });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_bits_name_camera_microphone_and_screen() {
        assert_eq!(media_kinds(DEVICE_VIDEO | DEVICE_AUDIO), vec![PermissionKind::Camera, PermissionKind::Microphone]);
        assert_eq!(media_kinds(DESKTOP_VIDEO | DESKTOP_AUDIO), vec![PermissionKind::Screen]);
        assert!(media_kinds(0).is_empty());
    }

    #[test]
    fn prompt_bits_name_what_ferryx_knows_and_the_rest_is_other() {
        assert_eq!(prompt_kinds(PROMPT_NOTIFICATIONS), vec![PermissionKind::Notifications]);
        assert_eq!(
            prompt_kinds(PROMPT_GEOLOCATION | PROMPT_CLIPBOARD),
            vec![PermissionKind::Geolocation, PermissionKind::Clipboard]
        );
        // MIDI sysex (8192) has no Ferryx name.
        assert_eq!(prompt_kinds(PROMPT_CAMERA | 8192), vec![PermissionKind::Camera, PermissionKind::Other]);
        assert!(prompt_kinds(0).is_empty());
    }
}
