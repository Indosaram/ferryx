//! Windows trusted bridge (design 3.3): a CDP isolated world plus a world-scoped
//! Runtime.addBinding, guarded by a page-world leak probe. Independent of CDP, this module
//! also installs the native key source (AcceleratorKeyPressed, design 6.2 step 4) and the
//! IsUserInitiated observers that feed BrowserManager::record_native_user_initiated.
//!
//! # How install reports the kind
//!
//! Every CDP call completes asynchronously on the UI thread's message loop. install runs on
//! that same thread, so it must not block waiting for those completions: no completion can
//! be dispatched until install returns, so a wait would either deadlock or need a nested
//! message pump inside tauri's creation closure. install therefore returns
//! TrustedBridgeKind::Unavailable at once (the conservative kind, which the creation flow
//! records right away), and the CDP chain upgrades the tab later with
//! manager.set_trusted_bridge(Isolated) only after Runtime.addBinding succeeded, the leak
//! probe saw typeof __ferryxTrusted === "undefined" in the page world, and only then the
//! broker was registered with Page.addScriptToEvaluateOnNewDocument (ordering in
//! super::cdp_order). A degraded tab therefore never runs the broker and keeps native link
//! and download behaviour. Because the completions
//! run on the UI thread after the creation closure has returned, the upgrade cannot be
//! overwritten by the creation flow's own Unavailable write. Any later leak (re-probed on every
//! new main-frame ferryx-bridge context) downgrades the tab for good and removes the binding
//! and the broker script (by the identifier CDP returned).
//!
//! This module never sends the Runtime domain's disable command or any Debugger domain
//! method: an open DevTools window shares the target and owns that state (design 3.7).

use super::keys::{chord_to_action, Chord, KeyId, KeyPlatform};
use super::cdp_order::{
    leak_probe_outcome, parse_script_identifier, CdpBridgeCommand, CdpBridgeEvent,
    CdpBridgeLifecycle, ProbeOutcome, DISARM_BROKER_SCRIPT,
};
use super::{Attestation, BridgeInstallCtx, TrustedBridgeKind, TRUSTED_BROKER_SCRIPT};
use crate::browser::security::BrowserError;
use crate::browser::{BrowserShortcutRequestedPayload, BROWSER_SHORTCUT_REQUESTED_EVENT};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use tauri::{Emitter, Manager};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Controller, COREWEBVIEW2_KEY_EVENT_KIND,
    COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
    COREWEBVIEW2_PHYSICAL_KEY_STATUS,
};
use webview2_com::{
    take_pwstr, AcceleratorKeyPressedEventHandler, CallDevToolsProtocolMethodCompletedHandler,
    DevToolsProtocolEventReceivedEventHandler, ExecuteScriptCompletedHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler,
};
use windows::core::{BOOL, HSTRING, PWSTR};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// The isolated world name, shared with the broker and Page.addScriptToEvaluateOnNewDocument.
pub const WORLD_NAME: &str = "ferryx-bridge";
/// The CDP binding the broker calls; it must exist only inside WORLD_NAME.
pub const BINDING_NAME: &str = "__ferryxTrusted";
/// Evaluated in the page world. Anything but "undefined" means the binding leaked.
pub const LEAK_PROBE_SCRIPT: &str = "typeof __ferryxTrusted";

// ---------------------------------------------------------------------------------------
// Pure CDP message builders and parsers (unit-tested on Windows).
// ---------------------------------------------------------------------------------------

/// One CDP method call: (method name, parameters as a JSON object string).
pub type CdpMessage = (&'static str, String);

pub fn page_enable() -> CdpMessage {
    ("Page.enable", "{}".into())
}

pub fn page_get_frame_tree() -> CdpMessage {
    ("Page.getFrameTree", "{}".into())
}

pub fn runtime_enable() -> CdpMessage {
    ("Runtime.enable", "{}".into())
}

pub fn add_broker_script(source: &str) -> CdpMessage {
    (
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": source, "worldName": WORLD_NAME }).to_string(),
    )
}

pub fn add_binding() -> CdpMessage {
    (
        "Runtime.addBinding",
        json!({ "name": BINDING_NAME, "executionContextName": WORLD_NAME }).to_string(),
    )
}

pub fn remove_binding() -> CdpMessage {
    ("Runtime.removeBinding", json!({ "name": BINDING_NAME }).to_string())
}

pub fn remove_broker_script(identifier: &str) -> CdpMessage {
    (
        "Page.removeScriptToEvaluateOnNewDocument",
        json!({ "identifier": identifier }).to_string(),
    )
}

/// Catch-up world for a document that started before the new-document script was registered.
pub fn create_isolated_world(frame_id: &str) -> CdpMessage {
    (
        "Page.createIsolatedWorld",
        json!({ "frameId": frame_id, "worldName": WORLD_NAME, "grantUniveralAccess": false })
            .to_string(),
    )
}

pub fn evaluate_in_context(expression: &str, context_id: i64) -> CdpMessage {
    (
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "contextId": context_id,
            "returnByValue": true,
            "awaitPromise": false,
        })
        .to_string(),
    )
}

/// Page.getFrameTree result -> main frame id.
pub fn parse_frame_tree_main_id(result: &str) -> Option<String> {
    let value: Value = serde_json::from_str(result).ok()?;
    value["frameTree"]["frame"]["id"].as_str().map(str::to_string)
}

/// Page.createIsolatedWorld result -> execution context id.
pub fn parse_created_world_context(result: &str) -> Option<i64> {
    let value: Value = serde_json::from_str(result).ok()?;
    value["executionContextId"].as_i64()
}

/// Runtime.evaluate result -> JSON text of the value, shaped like WebView2 ExecuteScript
/// output (a string comes back JSON-quoted, undefined as "null"), so callers can use
/// crate::browser::picker::unwrap_eval_json exactly as for the page world.
pub fn parse_evaluate_response(result: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(result).map_err(|error| format!("bad Runtime.evaluate reply: {error}"))?;
    if let Some(details) = value.get("exceptionDetails") {
        let message = details["exception"]["description"]
            .as_str()
            .or_else(|| details["text"].as_str())
            .unwrap_or("script threw in the ferryx-bridge world");
        return Err(message.to_string());
    }
    let remote = &value["result"];
    Ok(match remote.get("value") {
        Some(inner) => inner.to_string(),
        None => "null".into(),
    })
}

/// The leak-probe decision: only an exact "undefined" (ExecuteScript returns it JSON-quoted)
/// proves the page world cannot see the binding. See cdp_order::leak_probe_outcome for how
/// errors (inconclusive, retried) differ from a positive leak.
#[cfg(test)]
pub fn leak_probe_passed(raw: &str) -> bool {
    leak_probe_outcome(Some(raw)) == ProbeOutcome::Passed
}

/// Runtime.bindingCalled params -> payload, only when the call came from the tracked
/// main-frame ferryx-bridge context under our binding name.
pub fn accept_binding_call(params: &str, main_context: Option<i64>) -> Option<String> {
    let main_context = main_context?;
    let value: Value = serde_json::from_str(params).ok()?;
    if value["name"].as_str() != Some(BINDING_NAME) {
        return None;
    }
    if value["executionContextId"].as_i64() != Some(main_context) {
        return None;
    }
    value["payload"].as_str().map(str::to_string)
}

/// Tracks the ferryx-bridge execution context of the main frame from CDP events.
#[derive(Debug, Default, Clone)]
pub struct ContextTracker {
    main_frame_id: Option<String>,
    /// frameId -> ferryx-bridge context id, for every frame (subframes are kept only so a
    /// later main-frame id can be resolved without waiting for another event).
    contexts: HashMap<String, i64>,
}

impl ContextTracker {
    pub fn set_main_frame(&mut self, frame_id: String) {
        self.main_frame_id = Some(frame_id);
    }

    /// Page.frameNavigated: a frame without parentId is the main frame.
    pub fn on_frame_navigated(&mut self, params: &str) {
        let Ok(value) = serde_json::from_str::<Value>(params) else {
            return;
        };
        let frame = &value["frame"];
        if frame.get("parentId").is_none_or(Value::is_null) {
            if let Some(id) = frame["id"].as_str() {
                self.main_frame_id = Some(id.to_string());
            }
        }
    }

    /// Runtime.executionContextCreated. Returns true when the new context is the main
    /// frame's ferryx-bridge world.
    pub fn on_context_created(&mut self, params: &str) -> bool {
        let Ok(value) = serde_json::from_str::<Value>(params) else {
            return false;
        };
        let context = &value["context"];
        if context["name"].as_str() != Some(WORLD_NAME) {
            return false;
        }
        let aux = &context["auxData"];
        if aux["isDefault"].as_bool() == Some(true) {
            return false;
        }
        let (Some(id), Some(frame)) = (context["id"].as_i64(), aux["frameId"].as_str()) else {
            return false;
        };
        self.contexts.insert(frame.to_string(), id);
        self.main_frame_id.as_deref() == Some(frame)
    }

    /// Records a world this module created itself with Page.createIsolatedWorld.
    pub fn on_world_created(&mut self, frame_id: &str, context_id: i64) {
        self.contexts.insert(frame_id.to_string(), context_id);
    }

    /// Runtime.executionContextDestroyed.
    pub fn on_context_destroyed(&mut self, params: &str) {
        let Ok(value) = serde_json::from_str::<Value>(params) else {
            return;
        };
        if let Some(id) = value["executionContextId"].as_i64() {
            self.contexts.retain(|_, existing| *existing != id);
        }
    }

    /// Runtime.executionContextsCleared.
    pub fn on_contexts_cleared(&mut self) {
        self.contexts.clear();
    }

    pub fn main_frame_id(&self) -> Option<&str> {
        self.main_frame_id.as_deref()
    }

    pub fn main_context(&self) -> Option<i64> {
        self.main_frame_id
            .as_deref()
            .and_then(|frame| self.contexts.get(frame).copied())
    }
}

// ---------------------------------------------------------------------------------------
// Pure key mapping (unit-tested on Windows).
// ---------------------------------------------------------------------------------------

const VK_TAB: u32 = 0x09;
const VK_PRIOR: u32 = 0x21;
const VK_NEXT: u32 = 0x22;
const VK_OEM_PLUS: u32 = 0xBB;
const VK_OEM_COMMA: u32 = 0xBC;
const VK_OEM_MINUS: u32 = 0xBD;
const VK_OEM_4: u32 = 0xDB;
const VK_OEM_6: u32 = 0xDD;
/// IME composition placeholder (keyCode 229): never a chord.
pub const VK_PROCESSKEY: u32 = 0xE5;

/// Windows virtual-key code to the platform-neutral key the chord table names.
pub fn windows_key_id(virtual_key: u32) -> KeyId {
    match virtual_key {
        0x41..=0x5A => KeyId::Letter(char::from(b'a' + (virtual_key - 0x41) as u8)),
        0x30..=0x39 => KeyId::Digit((virtual_key - 0x30) as u8),
        VK_OEM_COMMA => KeyId::Comma,
        VK_OEM_PLUS => KeyId::Equal,
        VK_OEM_MINUS => KeyId::Minus,
        VK_OEM_4 => KeyId::BracketLeft,
        VK_OEM_6 => KeyId::BracketRight,
        VK_TAB => KeyId::Tab,
        VK_PRIOR => KeyId::PageUp,
        VK_NEXT => KeyId::PageDown,
        _ => KeyId::Other,
    }
}

/// Modifier state read with GetKeyState at event time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WinModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

pub fn windows_chord(virtual_key: u32, mods: WinModifiers) -> Chord {
    Chord {
        key: windows_key_id(virtual_key),
        ctrl: mods.ctrl,
        meta: mods.win,
        alt: mods.alt,
        shift: mods.shift,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorOutcome {
    /// Not ours: leave the event to WebView2 and the page.
    Ignore,
    /// First press of an app-chrome chord: emit it and mark the event handled.
    Emit(&'static str),
    /// Auto-repeat of an app-chrome chord: mark handled, emit nothing (dedupe).
    Swallow,
}

/// Decides one AcceleratorKeyPressed event. Only KeyDown/SystemKeyDown count; a key that
/// was already down (auto-repeat) is swallowed without a second emit.
pub fn accelerator_outcome(
    kind: i32,
    was_key_down: bool,
    virtual_key: u32,
    mods: WinModifiers,
) -> AcceleratorOutcome {
    let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN.0
        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN.0;
    if !down || virtual_key == VK_PROCESSKEY {
        return AcceleratorOutcome::Ignore;
    }
    match chord_to_action(windows_chord(virtual_key, mods), KeyPlatform::Windows) {
        None => AcceleratorOutcome::Ignore,
        Some(_) if was_key_down => AcceleratorOutcome::Swallow,
        Some(action) => AcceleratorOutcome::Emit(action),
    }
}

/// A native user-initiated URI in the form admit() compares against (tauri::Url
/// normalization, http(s) only). Anything else is not worth recording.
pub fn normalize_user_initiated_uri(uri: &str) -> Option<String> {
    match tauri::Url::parse(uri) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => Some(parsed.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// Per-webview world state (Send: read by eval_in_world off the UI thread).
// ---------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct WorldState {
    tracker: ContextTracker,
    /// Setup ordering: Isolated only after binding, probe and broker registration;
    /// Downgraded is sticky and means binding and broker were removed.
    life: CdpBridgeLifecycle,
}

/// Keyed by webview label (eval_in_world only receives the webview).
static WORLDS: LazyLock<Mutex<HashMap<String, WorldState>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_world<T>(label: &str, f: impl FnOnce(&mut WorldState) -> T) -> Option<T> {
    WORLDS.lock().get_mut(label).map(f)
}

/// Drops the per-webview world state when the browser session is removed.
pub(crate) fn forget_webview(label: &str) {
    WORLDS.lock().remove(label);
}

// ---------------------------------------------------------------------------------------
// COM glue (UI thread only).
// ---------------------------------------------------------------------------------------

type Done<T> = Box<dyn FnOnce(T)>;

/// Shared single-use completion for one CDP call.
type CdpDoneSlot = Rc<RefCell<Option<Done<Result<String, String>>>>>;

/// Calls one CDP method; done gets the result JSON or an error string exactly once.
fn cdp_call(core: &ICoreWebView2, message: CdpMessage, done: Done<Result<String, String>>) {
    let (method, params) = message;
    let slot: CdpDoneSlot = Rc::new(RefCell::new(Some(done)));
    let handler_slot = Rc::clone(&slot);
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
        move |result: windows::core::Result<()>, json: String| {
            if let Some(done) = handler_slot.borrow_mut().take() {
                done(result.map(|()| json).map_err(|error| format!("{method}: {error}")));
            }
            Ok(())
        },
    ));
    let issued = unsafe {
        core.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &handler)
    };
    if let Err(error) = issued {
        if let Some(done) = slot.borrow_mut().take() {
            done(Err(format!("{method}: {error}")));
        }
    }
}

/// Runs CDP calls strictly one after another; stops at the first failure.
fn cdp_chain(
    core: ICoreWebView2,
    mut steps: VecDeque<(CdpMessage, Option<Done<String>>)>,
    finish: Done<Result<(), String>>,
) {
    let Some((message, on_ok)) = steps.pop_front() else {
        finish(Ok(()));
        return;
    };
    let next_core = core.clone();
    cdp_call(
        &core,
        message,
        Box::new(move |result| match result {
            Ok(json) => {
                if let Some(on_ok) = on_ok {
                    on_ok(json);
                }
                cdp_chain(next_core, steps, finish);
            }
            Err(error) => finish(Err(error)),
        }),
    );
}

/// Evaluates LEAK_PROBE_SCRIPT in the page world. An ExecuteScript error is Inconclusive,
/// never a leak.
fn leak_probe(core: &ICoreWebView2, done: Done<ProbeOutcome>) {
    let slot: Rc<RefCell<Option<Done<ProbeOutcome>>>> = Rc::new(RefCell::new(Some(done)));
    let handler_slot = Rc::clone(&slot);
    let handler = ExecuteScriptCompletedHandler::create(Box::new(
        move |result: windows::core::Result<()>, json: String| {
            if let Some(done) = handler_slot.borrow_mut().take() {
                done(leak_probe_outcome(result.ok().map(|()| json.as_str())));
            }
            Ok(())
        },
    ));
    let issued = unsafe { core.ExecuteScript(&HSTRING::from(LEAK_PROBE_SCRIPT), &handler) };
    if issued.is_err() {
        if let Some(done) = slot.borrow_mut().take() {
            done(ProbeOutcome::Inconclusive);
        }
    }
}

/// Everything the CDP callbacks need, cheap to clone into each closure.
#[derive(Clone)]
struct Installed {
    label: String,
    ctx: BridgeInstallCtx,
}

impl Installed {
    fn set_kind(&self, kind: TrustedBridgeKind) {
        if let Err(error) = self.ctx.manager.set_trusted_bridge(&self.ctx.browser_id, kind) {
            tracing::debug!(browser_id = %self.ctx.browser_id, %error, "trusted bridge kind not recorded (tab gone)");
        }
    }

    /// Feeds one event to the ordering state machine and runs the commands it returns.
    fn drive(&self, core: &ICoreWebView2, event: CdpBridgeEvent, reason: &str) {
        let commands = with_world(&self.label, |world| {
            let main_context = world.tracker.main_context();
            world.life.on(event, main_context)
        })
        .unwrap_or_default();
        if commands.contains(&CdpBridgeCommand::MarkUnavailable) {
            tracing::warn!(browser_id = %self.ctx.browser_id, reason, "WebView2 trusted bridge unavailable");
        }
        for command in commands {
            self.run(core, command);
        }
    }

    fn run(&self, core: &ICoreWebView2, command: CdpBridgeCommand) {
        let browser_id = self.ctx.browser_id.clone();
        match command {
            CdpBridgeCommand::RunLeakProbe => {
                let this = self.clone();
                let next_core = core.clone();
                leak_probe(
                    core,
                    Box::new(move |outcome: ProbeOutcome| {
                        let reason = match outcome {
                            ProbeOutcome::Leaked => "typeof __ferryxTrusted is not undefined in the page world",
                            _ => "the leak probe failed twice without a verdict",
                        };
                        this.drive(&next_core, outcome.event(), reason);
                    }),
                );
            }
            CdpBridgeCommand::RegisterBroker => {
                let this = self.clone();
                let next_core = core.clone();
                cdp_call(
                    core,
                    add_broker_script(TRUSTED_BROKER_SCRIPT),
                    Box::new(move |result| match result.map(|json| parse_script_identifier(&json)) {
                        Ok(Some(id)) => this.drive(&next_core, CdpBridgeEvent::BrokerRegistered(id), ""),
                        Ok(None) => this.drive(
                            &next_core,
                            CdpBridgeEvent::Failed,
                            "Page.addScriptToEvaluateOnNewDocument returned no identifier",
                        ),
                        Err(error) => this.drive(&next_core, CdpBridgeEvent::Failed, &error),
                    }),
                );
            }
            CdpBridgeCommand::BecomeIsolated => {
                self.set_kind(TrustedBridgeKind::Isolated);
                self.catch_up_world(core);
            }
            // Runtime.removeBinding leaves the function on the world's global, so the broker
            // in the current document must be told the channel is gone before the downgrade.
            CdpBridgeCommand::DisarmBroker(context) => cdp_call(
                core,
                evaluate_in_context(DISARM_BROKER_SCRIPT, context),
                Box::new(move |result| {
                    if let Err(error) = result.and_then(|json| parse_evaluate_response(&json)) {
                        tracing::warn!(browser_id = %browser_id, %error, "disarming the broker failed");
                    }
                }),
            ),
            CdpBridgeCommand::RemoveBinding => cdp_call(
                core,
                remove_binding(),
                Box::new(move |result| {
                    if let Err(error) = result {
                        tracing::warn!(browser_id = %browser_id, %error, "Runtime.removeBinding failed");
                    }
                }),
            ),
            CdpBridgeCommand::RemoveBroker(id) => cdp_call(
                core,
                remove_broker_script(&id),
                Box::new(move |result| {
                    if let Err(error) = result {
                        tracing::warn!(browser_id = %browser_id, %error, "Page.removeScriptToEvaluateOnNewDocument failed");
                    }
                }),
            ),
            CdpBridgeCommand::MarkUnavailable => self.set_kind(TrustedBridgeKind::Unavailable),
        }
    }

    /// The current document started before the broker was registered as a new-document
    /// script (it is registered only after the binding and the leak probe): give it a world
    /// and the broker now. Runs only on BecomeIsolated.
    fn catch_up_world(&self, core: &ICoreWebView2) {
        let Some(frame) = with_world(&self.label, |world| {
            match (world.tracker.main_context(), world.tracker.main_frame_id()) {
                (None, Some(frame)) => Some(frame.to_string()),
                _ => None,
            }
        })
        .flatten() else {
            return;
        };
        let this = self.clone();
        let eval_core = core.clone();
        cdp_call(
            core,
            create_isolated_world(&frame),
            Box::new(move |result| {
                let context = match result.map(|json| parse_created_world_context(&json)) {
                    Ok(Some(context)) => context,
                    Ok(None) => return,
                    Err(error) => {
                        tracing::debug!(browser_id = %this.ctx.browser_id, %error, "no catch-up world");
                        return;
                    }
                };
                with_world(&this.label, |world| world.tracker.on_world_created(&frame, context));
                // A downgrade may have landed while createIsolatedWorld was in flight; a
                // broker injected now would find the binding and cancel clicks nobody accepts.
                if !with_world(&this.label, |world| world.life.is_isolated()).unwrap_or(false) {
                    return;
                }
                let browser_id = this.ctx.browser_id.clone();
                cdp_call(
                    &eval_core,
                    evaluate_in_context(TRUSTED_BROKER_SCRIPT, context),
                    Box::new(move |result| {
                        if let Err(error) = result.and_then(|json| parse_evaluate_response(&json)) {
                            tracing::warn!(browser_id = %browser_id, %error, "catch-up broker failed");
                        }
                    }),
                );
            }),
        );
    }
}

fn subscribe(
    core: &ICoreWebView2,
    event: &str,
    mut on_event: impl FnMut(Option<ICoreWebView2>, &str) + 'static,
) -> windows::core::Result<()> {
    let receiver = unsafe { core.GetDevToolsProtocolEventReceiver(&HSTRING::from(event)) }?;
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |sender, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut raw = PWSTR::null();
        unsafe { args.ParameterObjectAsJson(&mut raw) }?;
        on_event(sender, &take_pwstr(raw));
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { receiver.add_DevToolsProtocolEventReceived(&handler, &mut token) }
}

fn register_cdp_events(core: &ICoreWebView2, installed: &Installed) -> windows::core::Result<()> {
    let this = installed.clone();
    subscribe(core, "Runtime.bindingCalled", move |_, params| {
        let main = with_world(&this.label, |world| {
            if world.life.is_isolated() {
                world.tracker.main_context()
            } else {
                None
            }
        })
        .flatten();
        match accept_binding_call(params, main) {
            Some(payload) => (this.ctx.sink)(&this.ctx.browser_id, &payload, Attestation::IsolatedWorld),
            None => tracing::debug!(browser_id = %this.ctx.browser_id, "dropped a binding call outside the main-frame ferryx-bridge world"),
        }
    })?;

    let this = installed.clone();
    subscribe(core, "Runtime.executionContextCreated", move |sender, params| {
        let (reprobe, disarm) = with_world(&this.label, |world| {
            let main = world.tracker.on_context_created(params);
            // A main-frame document that committed after the downgrade but before CDP
            // processed Runtime.removeBinding may still get the binding and the broker.
            let disarm = if main && world.life.is_downgraded() {
                world.tracker.main_context()
            } else {
                None
            };
            (main && world.life.is_isolated(), disarm)
        })
        .unwrap_or((false, None));
        if let (Some(context), Some(core)) = (disarm, sender.as_ref()) {
            this.run(core, CdpBridgeCommand::DisarmBroker(context));
        }
        // Every new main-frame document re-checks that the binding stayed out of the page
        // (MainWorldCreated -> RunLeakProbe, with one retry on a probe error).
        if let (true, Some(core)) = (reprobe, sender) {
            this.drive(&core, CdpBridgeEvent::MainWorldCreated, "");
        }
    })?;

    let label = installed.label.clone();
    subscribe(core, "Runtime.executionContextDestroyed", move |_, params| {
        with_world(&label, |world| world.tracker.on_context_destroyed(params));
    })?;

    let label = installed.label.clone();
    subscribe(core, "Runtime.executionContextsCleared", move |_, _| {
        with_world(&label, |world| world.tracker.on_contexts_cleared());
    })?;

    let label = installed.label.clone();
    subscribe(core, "Page.frameNavigated", move |_, params| {
        with_world(&label, |world| world.tracker.on_frame_navigated(params));
    })
}

fn start_cdp_chain(core: ICoreWebView2, installed: Installed) {
    let label = installed.label.clone();
    let frame_tree: Done<String> = Box::new(move |json| {
        if let Some(frame) = parse_frame_tree_main_id(&json) {
            with_world(&label, |world| world.tracker.set_main_frame(frame));
        }
    });
    let steps: VecDeque<(CdpMessage, Option<Done<String>>)> = VecDeque::from([
        (page_enable(), None),
        (page_get_frame_tree(), Some(frame_tree)),
        (runtime_enable(), None),
        (add_binding(), None),
    ]);
    let probe_core = core.clone();
    cdp_chain(
        core,
        steps,
        // Fail safe: no broker exists yet, so a failure leaves the tab on native behaviour.
        // removeBinding is harmless if addBinding never applied.
        Box::new(move |result| match result {
            Ok(()) => installed.drive(&probe_core, CdpBridgeEvent::SetupSucceeded, ""),
            Err(error) => installed.drive(&probe_core, CdpBridgeEvent::Failed, &error),
        }),
    );
}

fn read_modifiers() -> WinModifiers {
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) } < 0;
    WinModifiers {
        ctrl: down(VK_CONTROL.0),
        alt: down(VK_MENU.0),
        shift: down(VK_SHIFT.0),
        win: down(VK_LWIN.0) || down(VK_RWIN.0),
    }
}

fn register_accelerators<R: tauri::Runtime>(
    controller: &ICoreWebView2Controller,
    app: tauri::AppHandle<R>,
    browser_id: String,
) -> windows::core::Result<()> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        unsafe { args.KeyEventKind(&mut kind) }?;
        let mut virtual_key = 0u32;
        unsafe { args.VirtualKey(&mut virtual_key) }?;
        let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
        unsafe { args.PhysicalKeyStatus(&mut status) }?;
        match accelerator_outcome(kind.0, status.WasKeyDown.as_bool(), virtual_key, read_modifiers()) {
            AcceleratorOutcome::Ignore => {}
            AcceleratorOutcome::Swallow => unsafe { args.SetHandled(true) }?,
            AcceleratorOutcome::Emit(action) => {
                let _ = app.emit(
                    BROWSER_SHORTCUT_REQUESTED_EVENT,
                    BrowserShortcutRequestedPayload {
                        browser_id: browser_id.clone(),
                        action: action.to_string(),
                    },
                );
                unsafe { args.SetHandled(true) }?;
            }
        }
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) }
}

/// Observe only: never SetCancel/SetHandled; wry's own handlers keep deciding (design 3.3).
fn register_user_initiated_observers(core: &ICoreWebView2, ctx: &BridgeInstallCtx) -> windows::core::Result<()> {
    let record = {
        let manager = Arc::clone(&ctx.manager);
        let browser_id = ctx.browser_id.clone();
        move |uri: PWSTR, user: BOOL| {
            let uri = take_pwstr(uri);
            if user.as_bool() {
                if let Some(url) = normalize_user_initiated_uri(&uri) {
                    manager.record_native_user_initiated(&browser_id, &url, Instant::now());
                }
            }
        }
    };
    let navigation_record = record.clone();
    let navigation = NavigationStartingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut user = BOOL::default();
        unsafe { args.IsUserInitiated(&mut user) }?;
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri) }?;
        navigation_record(uri, user);
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { core.add_NavigationStarting(&navigation, &mut token) }?;

    let new_window = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else {
            return Ok(());
        };
        let mut user = BOOL::default();
        unsafe { args.IsUserInitiated(&mut user) }?;
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri) }?;
        record(uri, user);
        Ok(())
    }));
    let mut token = 0i64;
    unsafe { core.add_NewWindowRequested(&new_window, &mut token) }
}

fn install_on_ui_thread<R: tauri::Runtime>(
    controller: ICoreWebView2Controller,
    app: tauri::AppHandle<R>,
    installed: Installed,
) {
    let browser_id = installed.ctx.browser_id.clone();
    let core = match unsafe { controller.CoreWebView2() } {
        Ok(core) => core,
        Err(error) => {
            tracing::warn!(browser_id = %browser_id, %error, "no CoreWebView2 for the trusted bridge");
            return;
        }
    };
    // Independent of CDP: native shortcuts and user-initiated attestation.
    if let Err(error) = register_accelerators(&controller, app, browser_id.clone()) {
        tracing::warn!(browser_id = %browser_id, %error, "AcceleratorKeyPressed registration failed");
    }
    if let Err(error) = register_user_initiated_observers(&core, &installed.ctx) {
        tracing::warn!(browser_id = %browser_id, %error, "IsUserInitiated observer registration failed");
    }
    // Subscribe before Runtime.enable so the replayed executionContextCreated events land.
    if let Err(error) = register_cdp_events(&core, &installed) {
        tracing::warn!(browser_id = %browser_id, %error, "CDP event subscription failed; the trusted bridge stays unavailable");
        return;
    }
    start_cdp_chain(core, installed);
}

/// See the module docs: returns Unavailable now and upgrades asynchronously.
pub(crate) fn install<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    ctx: &BridgeInstallCtx,
) -> TrustedBridgeKind {
    let label = webview.label().to_string();
    WORLDS.lock().insert(label.clone(), WorldState::default());
    let installed = Installed { label: label.clone(), ctx: ctx.clone() };
    let app = webview.app_handle().clone();
    if let Err(error) = webview.with_webview(move |platform| {
        install_on_ui_thread(platform.controller(), app, installed);
    }) {
        tracing::warn!(browser_id = %ctx.browser_id, %error, "with_webview failed; the trusted bridge stays unavailable");
        WORLDS.lock().remove(&label);
    }
    TrustedBridgeKind::Unavailable
}

/// Runtime.evaluate in the tracked main-frame ferryx-bridge context.
pub(crate) async fn eval_in_world<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    script: String,
) -> Result<String, BrowserError> {
    let label = webview.label().to_string();
    let context = with_world(&label, |world| {
        if world.life.is_isolated() {
            Ok(world.tracker.main_context())
        } else {
            Err(())
        }
    });
    let context_id = match context {
        Some(Ok(Some(id))) => id,
        Some(Ok(None)) => {
            return Err(BrowserError::AutomationFailed(
                "the ferryx-bridge world is not ready in this document".into(),
            ))
        }
        _ => {
            return Err(BrowserError::PlatformUnsupported(
                "no isolated JS world is available for this browser tab".into(),
            ))
        }
    };
    let message = evaluate_in_context(&script, context_id);
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<String, String>>();
    webview
        .with_webview(move |platform| match unsafe { platform.controller().CoreWebView2() } {
            Ok(core) => cdp_call(
                &core,
                message,
                Box::new(move |result| {
                    let _ = sender.send(result.and_then(|json| parse_evaluate_response(&json)));
                }),
            ),
            Err(error) => {
                let _ = sender.send(Err(error.to_string()));
            }
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver)
        .await
        .map_err(|_| BrowserError::AutomationFailed("isolated-world evaluation timed out".into()))?
        .map_err(|_| BrowserError::AutomationFailed("isolated-world evaluation was cancelled".into()))?
        .map_err(BrowserError::AutomationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_DOWN: i32 = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN.0;
    const SYSTEM_KEY_DOWN: i32 = COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN.0;
    const KEY_UP: i32 = 1;
    const CTRL: WinModifiers = WinModifiers { ctrl: true, alt: false, shift: false, win: false };
    const ALT: WinModifiers = WinModifiers { ctrl: false, alt: true, shift: false, win: false };

    #[test]
    fn virtual_keys_map_to_physical_keys() {
        assert_eq!(windows_key_id(0x41), KeyId::Letter('a'));
        assert_eq!(windows_key_id(0x57), KeyId::Letter('w'));
        assert_eq!(windows_key_id(0x5A), KeyId::Letter('z'));
        assert_eq!(windows_key_id(0x30), KeyId::Digit(0));
        assert_eq!(windows_key_id(0x39), KeyId::Digit(9));
        assert_eq!(windows_key_id(0xBC), KeyId::Comma);
        assert_eq!(windows_key_id(0xBB), KeyId::Equal);
        assert_eq!(windows_key_id(0xBD), KeyId::Minus);
        assert_eq!(windows_key_id(0xDB), KeyId::BracketLeft);
        assert_eq!(windows_key_id(0xDD), KeyId::BracketRight);
        assert_eq!(windows_key_id(0x09), KeyId::Tab);
        assert_eq!(windows_key_id(0x21), KeyId::PageUp);
        assert_eq!(windows_key_id(0x22), KeyId::PageDown);
        assert_eq!(windows_key_id(0x70), KeyId::Other); // F1
        assert_eq!(windows_key_id(0x60), KeyId::Other); // numpad 0 is not the top-row digit
    }

    #[test]
    fn windows_chords_map_through_the_single_table() {
        let chord = |vk, mods| chord_to_action(windows_chord(vk, mods), KeyPlatform::Windows);
        assert_eq!(chord(0x57, CTRL), Some("tab-close"));
        assert_eq!(chord(0x54, CTRL), Some("tab-new-browser"));
        assert_eq!(chord(0x4C, CTRL), Some("focus-address"));
        assert_eq!(chord(0x54, WinModifiers { shift: true, ..CTRL }), Some("tab-reopen-closed"));
        assert_eq!(chord(0x31, ALT), Some("workspace-select-1"));
        assert_eq!(chord(0x33, CTRL), Some("tab-select-3"));
        assert_eq!(chord(0x09, CTRL), Some("tab-next"));
        assert_eq!(chord(0x09, WinModifiers { shift: true, ..CTRL }), Some("tab-previous"));
        assert_eq!(chord(0x22, CTRL), Some("tab-next"));
        // AltGr arrives as Ctrl+Alt and must never be a chord.
        assert_eq!(chord(0x57, WinModifiers { alt: true, ..CTRL }), None);
        // The Windows key is not the primary modifier on Windows.
        assert_eq!(chord(0x57, WinModifiers { win: true, ..WinModifiers::default() }), None);
        assert_eq!(chord(0x57, WinModifiers::default()), None);
        // Ctrl+R is self-scoped and stays on the page route.
        assert_eq!(chord(0x52, CTRL), None);
    }

    #[test]
    fn accelerator_emits_once_and_swallows_repeats() {
        assert_eq!(accelerator_outcome(KEY_DOWN, false, 0x57, CTRL), AcceleratorOutcome::Emit("tab-close"));
        assert_eq!(accelerator_outcome(KEY_DOWN, true, 0x57, CTRL), AcceleratorOutcome::Swallow);
        assert_eq!(
            accelerator_outcome(SYSTEM_KEY_DOWN, false, 0x31, ALT),
            AcceleratorOutcome::Emit("workspace-select-1")
        );
        assert_eq!(accelerator_outcome(KEY_UP, false, 0x57, CTRL), AcceleratorOutcome::Ignore);
        assert_eq!(accelerator_outcome(3, false, 0x31, ALT), AcceleratorOutcome::Ignore);
        assert_eq!(accelerator_outcome(KEY_DOWN, false, VK_PROCESSKEY, CTRL), AcceleratorOutcome::Ignore);
        assert_eq!(accelerator_outcome(KEY_DOWN, false, 0x41, CTRL), AcceleratorOutcome::Ignore);
        assert_eq!(accelerator_outcome(KEY_DOWN, true, 0x41, CTRL), AcceleratorOutcome::Ignore);
    }

    #[test]
    fn cdp_builders_produce_the_documented_messages() {
        let (method, params) = add_broker_script("SRC");
        assert_eq!(method, "Page.addScriptToEvaluateOnNewDocument");
        let value: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(value, json!({ "source": "SRC", "worldName": "ferryx-bridge" }));

        let (method, params) = add_binding();
        assert_eq!(method, "Runtime.addBinding");
        assert_eq!(
            serde_json::from_str::<Value>(&params).unwrap(),
            json!({ "name": "__ferryxTrusted", "executionContextName": "ferryx-bridge" })
        );

        let (method, params) = remove_binding();
        assert_eq!(method, "Runtime.removeBinding");
        assert_eq!(serde_json::from_str::<Value>(&params).unwrap(), json!({ "name": "__ferryxTrusted" }));

        let (method, params) = remove_broker_script("12");
        assert_eq!(method, "Page.removeScriptToEvaluateOnNewDocument");
        assert_eq!(serde_json::from_str::<Value>(&params).unwrap(), json!({ "identifier": "12" }));

        let (method, params) = evaluate_in_context("1 + \"x\"", 42);
        assert_eq!(method, "Runtime.evaluate");
        let value: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(value["expression"], "1 + \"x\"");
        assert_eq!(value["contextId"], 42);
        assert_eq!(value["returnByValue"], true);

        let (method, params) = create_isolated_world("F1");
        assert_eq!(method, "Page.createIsolatedWorld");
        let value: Value = serde_json::from_str(&params).unwrap();
        assert_eq!(value["frameId"], "F1");
        assert_eq!(value["worldName"], "ferryx-bridge");

        for (method, params) in [page_enable(), page_get_frame_tree(), runtime_enable()] {
            assert!(["Page.enable", "Page.getFrameTree", "Runtime.enable"].contains(&method));
            assert_eq!(params, "{}");
        }
        // The module must never touch state the DevTools frontend owns.
        let source = include_str!("windows.rs");
        let forbidden_disable = ["Runtime", ".disable"].concat();
        let forbidden_debugger = ["\"Debugger", "."].concat();
        assert!(!source.contains(&forbidden_disable));
        assert!(!source.contains(&forbidden_debugger));
    }

    #[test]
    fn evaluate_and_frame_tree_replies_parse() {
        assert_eq!(parse_frame_tree_main_id(r#"{"frameTree":{"frame":{"id":"MAIN","loaderId":"L"}}}"#).as_deref(), Some("MAIN"));
        assert_eq!(parse_frame_tree_main_id("{}"), None);
        assert_eq!(parse_created_world_context(r#"{"executionContextId":7}"#), Some(7));
        assert_eq!(
            parse_evaluate_response(r#"{"result":{"type":"string","value":"{\"a\":1}"}}"#).unwrap(),
            r#""{\"a\":1}""#
        );
        assert_eq!(
            crate::browser::picker::unwrap_eval_json(
                &parse_evaluate_response(r#"{"result":{"type":"string","value":"armed"}}"#).unwrap()
            ),
            "armed"
        );
        assert_eq!(parse_evaluate_response(r#"{"result":{"type":"undefined"}}"#).unwrap(), "null");
        assert_eq!(parse_evaluate_response(r#"{"result":{"type":"object","subtype":"null","value":null}}"#).unwrap(), "null");
        let error = parse_evaluate_response(
            r#"{"result":{"type":"object"},"exceptionDetails":{"text":"Uncaught","exception":{"description":"Error: no world"}}}"#,
        )
        .unwrap_err();
        assert_eq!(error, "Error: no world");
        assert!(parse_evaluate_response("not json").is_err());
    }

    #[test]
    fn leak_probe_passes_only_for_undefined() {
        assert!(leak_probe_passed("\"undefined\""));
        assert!(leak_probe_passed(" \"undefined\"\n"));
        assert!(!leak_probe_passed("\"function\""));
        assert!(!leak_probe_passed("\"object\""));
        assert!(!leak_probe_passed("undefined"));
        assert!(!leak_probe_passed("null"));
        assert!(!leak_probe_passed(""));
    }

    fn created(id: i64, name: &str, frame: &str, is_default: bool) -> String {
        json!({
            "context": {
                "id": id,
                "origin": "https://example.com",
                "name": name,
                "auxData": { "isDefault": is_default, "type": if is_default { "default" } else { "isolated" }, "frameId": frame }
            }
        })
        .to_string()
    }

    #[test]
    fn tracker_follows_the_main_frame_world_only() {
        let mut tracker = ContextTracker::default();
        // A context that arrives before the frame tree still resolves later.
        assert!(!tracker.on_context_created(&created(5, WORLD_NAME, "MAIN", false)));
        tracker.set_main_frame("MAIN".into());
        assert_eq!(tracker.main_context(), Some(5));
        // The page world and other worlds are ignored.
        assert!(!tracker.on_context_created(&created(6, "", "MAIN", true)));
        assert!(!tracker.on_context_created(&created(7, "other-world", "MAIN", false)));
        assert_eq!(tracker.main_context(), Some(5));
        // A subframe world is never the main context.
        assert!(!tracker.on_context_created(&created(8, WORLD_NAME, "SUB", false)));
        assert_eq!(tracker.main_context(), Some(5));
        // Navigation: old world destroyed, new one created.
        tracker.on_context_destroyed(r#"{"executionContextId":5}"#);
        assert_eq!(tracker.main_context(), None);
        tracker.on_frame_navigated(r#"{"frame":{"id":"SUB","parentId":"MAIN"}}"#);
        assert_eq!(tracker.main_frame_id(), Some("MAIN"));
        tracker.on_frame_navigated(r#"{"frame":{"id":"MAIN2","loaderId":"L2"}}"#);
        assert_eq!(tracker.main_frame_id(), Some("MAIN2"));
        assert!(tracker.on_context_created(&created(9, WORLD_NAME, "MAIN2", false)));
        assert_eq!(tracker.main_context(), Some(9));
        tracker.on_contexts_cleared();
        assert_eq!(tracker.main_context(), None);
        tracker.on_world_created("MAIN2", 11);
        assert_eq!(tracker.main_context(), Some(11));
    }

    #[test]
    fn binding_calls_are_accepted_only_from_the_main_frame_world() {
        let call = |name: &str, ctx: i64, payload: Value| {
            json!({ "name": name, "payload": payload, "executionContextId": ctx }).to_string()
        };
        let envelope = r#"{"v":1,"kind":"download","url":"https://e.com/a"}"#;
        assert_eq!(
            accept_binding_call(&call(BINDING_NAME, 9, json!(envelope)), Some(9)).as_deref(),
            Some(envelope)
        );
        // Page world or subframe world context ids are dropped.
        assert_eq!(accept_binding_call(&call(BINDING_NAME, 1, json!(envelope)), Some(9)), None);
        // No tracked context means nothing is trusted.
        assert_eq!(accept_binding_call(&call(BINDING_NAME, 9, json!(envelope)), None), None);
        // Another binding name, a non-string payload, or garbage are dropped.
        assert_eq!(accept_binding_call(&call("other", 9, json!(envelope)), Some(9)), None);
        assert_eq!(accept_binding_call(&call(BINDING_NAME, 9, json!({"v":1})), Some(9)), None);
        assert_eq!(accept_binding_call("not json", Some(9)), None);
    }

    #[test]
    fn user_initiated_uris_are_normalized_like_admit() {
        assert_eq!(normalize_user_initiated_uri("https://Example.com").as_deref(), Some("https://example.com/"));
        assert_eq!(normalize_user_initiated_uri("http://e.com/a?b=1").as_deref(), Some("http://e.com/a?b=1"));
        assert_eq!(normalize_user_initiated_uri("about:blank"), None);
        assert_eq!(normalize_user_initiated_uri("javascript:alert(1)"), None);
        assert_eq!(normalize_user_initiated_uri("not a url"), None);
    }
}
