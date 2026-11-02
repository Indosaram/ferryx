//! Extension management for Ferryx's settings, through Chrome's own `chrome://extensions` page.
//!
//! CEF has no extension C API in the Chrome runtime, but `chrome://extensions` is a WebUI with
//! `chrome.developerPrivate` and `chrome.management`. The engine keeps one hidden browser on
//! that page (created on the first call, never a Ferryx tab or popup) and runs each
//! `Command::Extensions` there with DevTools `Runtime.evaluate` over CEF's in-process DevTools
//! message API. Calls wait until the page has loaded; each answer is `Event::Extensions`.
//!
//! Removing: Chromium always shows its own confirm dialog when a WebUI page uninstalls an
//! extension (`management_api.cc`: `show_confirm_dialog |= !self_uninstall`), so the page's
//! window comes up on top while that call runs and hides again once it is answered. A
//! cancelled dialog answers with `{"message":"cancelled"}`.

use super::{emit, same, EngineBrowserViewDelegate, EngineClient, EngineWindowDelegate, Key, QUITTING};
use cef::*;
use ferryx_browser_engine_protocol::{is_extension_id, json_string, Event, ExtensionCall};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

const PAGE_URL: &str = "chrome://extensions/";
/// Icons larger than this are left out of the list.
const MAX_ICON_BYTES: usize = 64 * 1024;
/// DevTools message ids of these calls stay below the vault's own ids (`vault.rs`), which
/// share this browser's DevTools channel.
const MAX_MESSAGE_ID: i32 = 999_999;

const LIST_BODY: &str = r#"(async () => {
  const icon = async (url) => {
    if (typeof url !== 'string' || !url) return null;
    try {
      const response = await fetch(url);
      if (!response.ok) return null;
      const blob = await response.blob();
      if (blob.size > __MAX_ICON_BYTES__ || !blob.type.startsWith('image/')) return null;
      return await new Promise((resolve) => {
        const reader = new FileReader();
        reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : null);
        reader.onerror = () => resolve(null);
        reader.readAsDataURL(blob);
      });
    } catch (error) {
      return null;
    }
  };
  const infos = await chrome.developerPrivate.getExtensionsInfo({includeDisabled: true, includeTerminated: true});
  const extensions = [];
  for (const info of infos) {
    if (info.location === 'COMPONENT') continue;
    extensions.push({
      id: info.id,
      name: info.name,
      version: info.version,
      description: info.description || null,
      enabled: info.state === 'ENABLED',
      iconDataUrl: await icon(info.iconUrl),
      pinnedToToolbar: typeof info.pinnedToToolbar === 'boolean' ? info.pinnedToToolbar : null,
      optionsUrl: (info.optionsPage && info.optionsPage.url) || null,
      homepageUrl: (info.homePage && info.homePage.specified && info.homePage.url) || null,
      fromWebStore: info.location === 'FROM_STORE',
      removable: !info.controlledInfo && !info.mustRemainInstalled,
    });
  }
  return extensions;
})()"#;

struct Manager {
    view: BrowserView,
    window: Option<Window>,
    /// The page has loaded and the DevTools observer is registered.
    ready: bool,
    /// Keeps the DevTools observer registered (dropping it unregisters).
    _registration: Option<Registration>,
}

/// A call sent to the page: the Ferryx call id, and whether it showed the window.
struct Pending {
    id: u32,
    remove: bool,
}

thread_local! {
    static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    /// Calls that arrived before the page loaded.
    static QUEUE: RefCell<Vec<(u32, ExtensionCall)>> = const { RefCell::new(Vec::new()) };
    /// DevTools message id to the call waiting for it.
    static CALLS: RefCell<HashMap<i32, Pending>> = RefCell::new(HashMap::new());
    static NEXT_MESSAGE_ID: Cell<i32> = const { Cell::new(1) };
}

fn next_message_id() -> i32 {
    let id = NEXT_MESSAGE_ID.get();
    NEXT_MESSAGE_ID.set(if id >= MAX_MESSAGE_ID { 1 } else { id + 1 });
    id
}

/// `{"message":...}` on one line.
fn failure(message: &str) -> String {
    json!({ "message": message }).to_string()
}

fn fail(id: u32, message: &str) {
    emit(Event::Extensions { id, ok: false, result: failure(message) });
}

/// The JS expression a call runs, or `None` for an invalid extension id.
fn body(call: &ExtensionCall) -> Option<String> {
    let checked = |extension: &str| is_extension_id(extension).then(|| json_string(extension));
    Some(match call {
        ExtensionCall::List => LIST_BODY.replace("__MAX_ICON_BYTES__", &MAX_ICON_BYTES.to_string()),
        ExtensionCall::SetEnabled { extension, enabled } => {
            format!("chrome.management.setEnabled({}, {enabled})", checked(extension)?)
        }
        ExtensionCall::Remove { extension } => {
            format!("chrome.management.uninstall({}, {{showConfirmDialog: false}})", checked(extension)?)
        }
        ExtensionCall::Pin { extension, pinned } => format!(
            "chrome.developerPrivate.updateExtensionConfiguration({{extensionId: {}, pinnedToToolbar: {pinned}}})",
            checked(extension)?
        ),
    })
}

/// The whole script: it never throws, and resolves to a JSON string `{"ok":true,"value":...}`
/// or `{"ok":false,"message":...}` (`"cancelled"` when the user cancelled Chrome's dialog).
fn script(call: &ExtensionCall) -> Option<String> {
    let body = body(call)?;
    Some(format!(
        "(async () => {{ try {{ const value = await ({body}); \
         return JSON.stringify({{ok: true, value: value === undefined ? null : value}}); }} \
         catch (error) {{ const message = String((error && error.message) || error); \
         return JSON.stringify({{ok: false, message: /cancel/i.test(message) ? 'cancelled' : message}}); }} }})()"
    ))
}

/// The `Runtime.evaluate` params for `script`.
fn evaluate_params(script: &str) -> String {
    format!(
        "{{\"expression\":{},\"awaitPromise\":true,\"returnByValue\":true,\"userGesture\":true}}",
        json_string(script)
    )
}

/// Turns the DevTools answer to a call into the `Event::Extensions` fields: the call's value
/// as one line of JSON, or `{"message":...}`.
fn answer(success: bool, raw: &[u8]) -> (bool, String) {
    let Ok(value) = serde_json::from_slice::<Value>(raw) else {
        return (false, failure("malformed devtools answer"));
    };
    if !success {
        return (false, failure(value["message"].as_str().unwrap_or("the devtools call failed")));
    }
    if let Some(details) = value.get("exceptionDetails") {
        let message = details["exception"]["description"]
            .as_str()
            .or_else(|| details["text"].as_str())
            .unwrap_or("the extensions page threw");
        return (false, failure(message));
    }
    let Some(outcome) = value["result"]["value"].as_str().and_then(|text| serde_json::from_str::<Value>(text).ok())
    else {
        return (false, failure("no answer from the extensions page"));
    };
    if outcome["ok"].as_bool() == Some(true) {
        (true, outcome["value"].to_string())
    } else {
        (false, failure(outcome["message"].as_str().unwrap_or("the extension change failed")))
    }
}

/// Whether `browser` is the hidden extensions page.
pub(super) fn is_manager(browser: &mut Browser) -> bool {
    MANAGER.with_borrow(|manager| manager.as_ref().is_some_and(|manager| same(&manager.view, browser)))
}

/// Runs `call` now, or once the page has loaded (opening it on the first call).
pub(super) fn call(id: u32, call: ExtensionCall) {
    if QUITTING.get() {
        fail(id, "the engine is quitting");
        return;
    }
    match MANAGER.with_borrow(|manager| manager.as_ref().map(|manager| manager.ready)) {
        Some(true) => run(id, call),
        Some(false) => QUEUE.with_borrow_mut(|queue| queue.push((id, call))),
        None => {
            QUEUE.with_borrow_mut(|queue| queue.push((id, call)));
            if !open() {
                fail_all("the extensions page could not be opened");
            }
        }
    }
}

/// Creates the hidden browser on `chrome://extensions` in a window that is never shown,
/// except while Chrome's remove dialog is open.
fn open() -> bool {
    let mut client = EngineClient::new();
    let mut delegate = EngineBrowserViewDelegate::new(RuntimeStyle::CHROME);
    let Some(view) = browser_view_create(
        Some(&mut client),
        Some(&CefString::from(PAGE_URL)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut delegate),
    ) else {
        return false;
    };
    // Known before its window (and so its browser) exists, so the browser is never taken for
    // a stray Chrome window or a tab.
    MANAGER.with_borrow_mut(|manager| {
        *manager = Some(Manager { view: view.clone(), window: None, ready: false, _registration: None });
    });
    let mut window_delegate = EngineWindowDelegate::new(RefCell::new(Some(view)), Key::Manager);
    window_create_top_level(Some(&mut window_delegate));
    true
}

/// The page's window exists; it stays hidden.
pub(super) fn window_created(window: &Window) {
    MANAGER.with_borrow_mut(|manager| {
        if let Some(manager) = manager.as_mut() {
            manager.window = Some(window.clone());
        }
    });
}

/// Once the page first stopped loading, registers the DevTools observer and runs the queued
/// calls.
pub(super) fn on_loading_state_change(browser: &mut Browser, loading: bool) {
    if loading || !is_manager(browser) {
        return;
    }
    if MANAGER.with_borrow(|manager| manager.as_ref().is_none_or(|manager| manager.ready)) {
        return;
    }
    let Some(host) = browser.host() else { return };
    let mut observer = ExtensionsObserver::new();
    let registration = host.add_dev_tools_message_observer(Some(&mut observer));
    MANAGER.with_borrow_mut(|manager| {
        if let Some(manager) = manager.as_mut() {
            manager._registration = registration;
            manager.ready = true;
        }
    });
    for (id, call) in QUEUE.with_borrow_mut(std::mem::take) {
        run(id, call);
    }
}

fn run(id: u32, call: ExtensionCall) {
    let Some(script) = script(&call) else {
        fail(id, "invalid extension id");
        return;
    };
    let Some(host) = MANAGER
        .with_borrow(|manager| manager.as_ref().and_then(|manager| manager.view.browser()))
        .and_then(|browser| browser.host())
    else {
        fail(id, "the extensions page has no browser");
        return;
    };
    let remove = matches!(call, ExtensionCall::Remove { .. });
    if remove {
        show_window();
    }
    let message_id = next_message_id();
    CALLS.with_borrow_mut(|calls| calls.insert(message_id, Pending { id, remove }));
    let message = format!(
        "{{\"id\":{message_id},\"method\":\"Runtime.evaluate\",\"params\":{}}}",
        evaluate_params(&script)
    );
    if host.send_dev_tools_message(Some(message.as_bytes())) == 0 {
        CALLS.with_borrow_mut(|calls| calls.remove(&message_id));
        hide_window_when_idle();
        fail(id, "the browser refused the devtools call");
    }
}

fn manager_window() -> Option<Window> {
    MANAGER.with_borrow(|manager| manager.as_ref().and_then(|manager| manager.window.clone()))
}

/// Brings the page's window up on top so Chrome's remove dialog has a visible parent.
fn show_window() {
    if let Some(window) = manager_window() {
        window.center_window(Some(&Size { width: 960, height: 640 }));
        window.show();
        window.activate();
        window.bring_to_top();
    }
}

/// Hides the page's window again once no remove call is waiting.
fn hide_window_when_idle() {
    if CALLS.with_borrow(|calls| calls.values().any(|pending| pending.remove)) {
        return;
    }
    if let Some(window) = manager_window() {
        window.hide();
    }
}

/// Answers every queued and waiting call with `message` and forgets the page.
fn fail_all(message: &str) {
    let queued: Vec<u32> = QUEUE.with_borrow_mut(std::mem::take).into_iter().map(|(id, _)| id).collect();
    let waiting: Vec<u32> = CALLS.with_borrow_mut(|calls| calls.drain().map(|(_, pending)| pending.id).collect());
    for id in queued.into_iter().chain(waiting) {
        fail(id, message);
    }
    MANAGER.with_borrow_mut(|manager| *manager = None);
}

/// The page's browser closed (its window was closed, or it crashed): calls fail and the next
/// one opens the page again.
pub(super) fn on_closed() {
    fail_all("the extensions page closed");
}

/// For quitting: forgets the page and hands back its view so its browser can be closed.
pub(super) fn take_view() -> Option<BrowserView> {
    QUEUE.with_borrow_mut(Vec::clear);
    CALLS.with_borrow_mut(HashMap::clear);
    MANAGER.with_borrow_mut(Option::take).map(|manager| manager.view)
}

wrap_dev_tools_message_observer! {
    struct ExtensionsObserver;

    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(
            &self,
            _browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            // The vault's own calls on this browser use other ids and are not answered here.
            let Some(pending) = CALLS.with_borrow_mut(|calls| calls.remove(&message_id)) else { return };
            if pending.remove {
                hide_window_when_idle();
            }
            let (ok, result) = answer(success != 0, result.unwrap_or(&b"{}"[..]));
            emit(Event::Extensions { id: pending.id, ok, result });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "cjpalhdlnbpafiamejdnhcphjbkeiagm";

    fn evaluated(value: &Value) -> Vec<u8> {
        json!({ "result": { "type": "string", "value": value.to_string() } }).to_string().into_bytes()
    }

    #[test]
    fn scripts_name_the_extension_and_reject_bad_ids() {
        let enable = script(&ExtensionCall::SetEnabled { extension: ID.into(), enabled: false }).unwrap();
        assert!(enable.contains(&format!("chrome.management.setEnabled(\"{ID}\", false)")));
        let pin = script(&ExtensionCall::Pin { extension: ID.into(), pinned: true }).unwrap();
        assert!(pin.contains(&format!("extensionId: \"{ID}\", pinnedToToolbar: true")));
        let remove = script(&ExtensionCall::Remove { extension: ID.into() }).unwrap();
        assert!(remove.contains(&format!("chrome.management.uninstall(\"{ID}\"")));
        let list = script(&ExtensionCall::List).unwrap();
        assert!(list.contains("getExtensionsInfo"));
        assert!(list.contains(&MAX_ICON_BYTES.to_string()));
        assert!(!list.contains("__MAX_ICON_BYTES__"));
        assert_eq!(script(&ExtensionCall::Remove { extension: "x\"); evil(\"".into() }), None);
        assert_eq!(script(&ExtensionCall::Pin { extension: String::new(), pinned: true }), None);
    }

    #[test]
    fn evaluate_params_are_one_json_object_that_awaits_with_a_user_gesture() {
        let params: Value = serde_json::from_str(&evaluate_params("(async () => 'a\n\"b')()")).unwrap();
        assert_eq!(params["expression"], "(async () => 'a\n\"b')()");
        assert_eq!(params["awaitPromise"], true);
        assert_eq!(params["returnByValue"], true);
        assert_eq!(params["userGesture"], true);
    }

    #[test]
    fn answers_carry_the_value_or_a_message() {
        let list = json!({ "ok": true, "value": [{ "id": ID, "name": "A b" }] });
        let (ok, result) = answer(true, &evaluated(&list));
        assert!(ok);
        assert_eq!(serde_json::from_str::<Value>(&result).unwrap(), list["value"]);
        assert!(!result.contains('\n'));

        assert_eq!(answer(true, &evaluated(&json!({ "ok": true, "value": null }))), (true, "null".to_string()));
        assert_eq!(
            answer(true, &evaluated(&json!({ "ok": false, "message": "cancelled" }))),
            (false, r#"{"message":"cancelled"}"#.to_string())
        );
        assert_eq!(
            answer(false, br#"{"code":-32000,"message":"no target"}"#),
            (false, r#"{"message":"no target"}"#.to_string())
        );
        let thrown = br#"{"result":{},"exceptionDetails":{"text":"Uncaught","exception":{"description":"TypeError: x"}}}"#;
        assert_eq!(answer(true, thrown), (false, r#"{"message":"TypeError: x"}"#.to_string()));
        assert!(!answer(true, b"not json").0);
        assert!(!answer(true, br#"{"result":{"type":"number","value":3}}"#).0);
    }

    #[test]
    fn message_ids_wrap_before_the_vault_ids() {
        NEXT_MESSAGE_ID.set(MAX_MESSAGE_ID);
        assert_eq!(next_message_id(), MAX_MESSAGE_ID);
        assert_eq!(next_message_id(), 1);
    }
}
