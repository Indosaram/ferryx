//! Password vault hooks inside the engine, over each browser's own DevTools channel (no
//! remote-debugging port).
//!
//! At creation every browser gets the shared login-capture script registered for new documents
//! in the `ferryx-vault` isolated world, plus a binding that exists only in that world. Page
//! script cannot enter the world, so a `Runtime.bindingCalled` for the binding comes from the
//! capture script; it is accepted only from the main frame's vault context and forwarded to
//! Ferryx as `Event::FormSubmitted`. Ferryx binds the payload to the tab's origin again.
//! `Command::Fill` evaluates the fill script in the main frame's vault context only, so a
//! cross-origin iframe never receives a secret; the script also re-checks the origin itself.

use cef::*;
use ferryx_browser_engine_protocol::{
    Event, VAULT_BINDING_NAME, VAULT_CAPTURE_SCRIPT, VAULT_WORLD_NAME, json_string,
};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

#[derive(Default)]
struct VaultState {
    /// Keeps the DevTools observer registered for the browser's lifetime; dropped with the state.
    _registration: Option<Registration>,
    main_frame: Option<String>,
    /// Frame id -> execution context id of its `ferryx-vault` world.
    contexts: HashMap<String, i64>,
    frame_tree_request: Option<i32>,
}

thread_local! {
    static STATES: RefCell<HashMap<i32, VaultState>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<i32> = const { Cell::new(1_000_000) };
}

fn next_id() -> i32 {
    let id = NEXT_ID.get();
    NEXT_ID.set(id.wrapping_add(1).max(1_000_000));
    id
}

/// Sends one DevTools method; returns its message id.
fn send(browser: &Browser, method: &str, params: &str) -> Option<i32> {
    let host = browser.host()?;
    let id = next_id();
    let message = format!("{{\"id\":{id},\"method\":{},\"params\":{params}}}", json_string(method));
    (host.send_dev_tools_message(Some(message.as_bytes())) != 0).then_some(id)
}

/// Registers the capture script and binding for `browser` (UI thread, at creation).
pub(super) fn attach(browser: &mut Browser) {
    let Some(host) = browser.host() else { return };
    let key = browser.identifier();
    if STATES.with_borrow(|states| states.contains_key(&key)) {
        return;
    }
    let mut observer = VaultObserver::new();
    let registration = host.add_dev_tools_message_observer(Some(&mut observer));
    STATES.with_borrow_mut(|states| {
        states.insert(key, VaultState { _registration: registration, ..Default::default() })
    });
    let browser = browser.clone();
    send(&browser, "Page.enable", "{}");
    send(&browser, "Runtime.enable", "{}");
    send(
        &browser,
        "Runtime.addBinding",
        &format!(
            "{{\"name\":{},\"executionContextName\":{}}}",
            json_string(VAULT_BINDING_NAME),
            json_string(VAULT_WORLD_NAME)
        ),
    );
    send(
        &browser,
        "Page.addScriptToEvaluateOnNewDocument",
        &format!(
            "{{\"source\":{},\"worldName\":{}}}",
            json_string(VAULT_CAPTURE_SCRIPT),
            json_string(VAULT_WORLD_NAME)
        ),
    );
    let request = send(&browser, "Page.getFrameTree", "{}");
    STATES.with_borrow_mut(|states| {
        if let Some(state) = states.get_mut(&key) {
            state.frame_tree_request = request;
        }
    });
}

pub(super) fn detach(browser: &mut Browser) {
    let key = browser.identifier();
    STATES.with_borrow_mut(|states| states.remove(&key));
}

/// Evaluates `script` in the main frame's vault world of `browser`.
pub(super) fn fill(browser: &Browser, script: &str) {
    let key = browser.identifier();
    let context = STATES.with_borrow(|states| {
        let state = states.get(&key)?;
        let main = state.main_frame.as_ref()?;
        state.contexts.get(main).copied()
    });
    let Some(context) = context else {
        eprintln!("ferryx-engine: no vault world in this page yet; fill skipped");
        return;
    };
    send(
        browser,
        "Runtime.evaluate",
        &format!("{{\"expression\":{},\"contextId\":{context},\"returnByValue\":true}}", json_string(script)),
    );
}

fn with_state<T>(browser: &Browser, f: impl FnOnce(&mut VaultState) -> T) -> Option<T> {
    let key = browser.identifier();
    STATES.with_borrow_mut(|states| states.get_mut(&key).map(f))
}

fn on_event(browser: &mut Browser, method: &str, params: &Value) {
    match method {
        "Page.frameNavigated" => {
            let frame = &params["frame"];
            if frame.get("parentId").is_none()
                && let Some(id) = frame["id"].as_str()
            {
                with_state(browser, |state| state.main_frame = Some(id.to_string()));
            }
        }
        "Runtime.executionContextCreated" => {
            let context = &params["context"];
            if context["name"].as_str() == Some(VAULT_WORLD_NAME)
                && let (Some(id), Some(frame)) = (context["id"].as_i64(), context["auxData"]["frameId"].as_str())
            {
                with_state(browser, |state| state.contexts.insert(frame.to_string(), id));
            }
        }
        "Runtime.executionContextDestroyed" => {
            if let Some(id) = params["executionContextId"].as_i64() {
                with_state(browser, |state| state.contexts.retain(|_, context| *context != id));
            }
        }
        "Runtime.executionContextsCleared" => {
            with_state(browser, |state| state.contexts.clear());
        }
        "Runtime.bindingCalled" => {
            if params["name"].as_str() != Some(VAULT_BINDING_NAME) {
                return;
            }
            let from_main = with_state(browser, |state| {
                let main = state.main_frame.as_ref()?;
                Some(state.contexts.get(main).copied() == params["executionContextId"].as_i64())
            })
            .flatten()
            .unwrap_or(false);
            let Some(payload) = params["payload"].as_str() else { return };
            if !from_main || payload.len() > 8 * 1024 {
                return;
            }
            if let Some(super::Key::Tab(tab)) = super::key_of(browser) {
                super::emit(Event::FormSubmitted { tab, payload: payload.to_string() });
            }
        }
        _ => {}
    }
}

fn on_result(browser: &mut Browser, message_id: i32, result: &Value) {
    let expected = with_state(browser, |state| state.frame_tree_request).flatten();
    if expected == Some(message_id)
        && let Some(id) = result["frameTree"]["frame"]["id"].as_str()
    {
        with_state(browser, |state| {
            state.main_frame.get_or_insert_with(|| id.to_string());
        });
    }
}

wrap_dev_tools_message_observer! {
    struct VaultObserver;

    impl DevToolsMessageObserver {
        fn on_dev_tools_event(&self, browser: Option<&mut Browser>, method: Option<&CefString>, params: Option<&[u8]>) {
            let (Some(browser), Some(method)) = (browser, method) else { return };
            let params = params.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok()).unwrap_or(Value::Null);
            on_event(browser, &method.to_string(), &params);
        }

        fn on_dev_tools_method_result(
            &self,
            browser: Option<&mut Browser>,
            message_id: ::std::os::raw::c_int,
            success: ::std::os::raw::c_int,
            result: Option<&[u8]>,
        ) {
            let Some(browser) = browser else { return };
            if success == 0 {
                return;
            }
            if let Some(result) = result.and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok()) {
                on_result(browser, message_id, &result);
            }
        }
    }
}
