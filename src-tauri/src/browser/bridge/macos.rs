//! macOS trusted bridge (design 3.1): a named WKContentWorld ("ferryx-bridge") carries
//! [`TRUSTED_BROKER_SCRIPT`] and a world-scoped `ferryxTrusted` script message handler.
//!
//! Page script runs in `WKContentWorld.pageWorld`; the handler is registered only in our
//! world, so the documented WebKit contract is that page script has no
//! `window.webkit.messageHandlers.ferryxTrusted` (UNVERIFIED by a live-engine probe here).
//! Every message is re-checked natively: it must come from the world named "ferryx-bridge"
//! (WebKit does not document that worldWithName returns one instance, so identity is by
//! name, which page script cannot choose: page script always runs in the page world) and
//! from the main frame, and its body must be a string. The host then
//! re-validates the envelope itself (`parse_envelope` / `admit`).
//!
//! macOS 10.15 has no WKContentWorld: a runtime selector probe fails and `install` returns
//! [`TrustedBridgeKind::Unavailable`], so the section 3.5 "Unavailable" rules apply.
//! `register_guest_view` still runs there, because native key attribution needs no world.

use super::{Attestation, BridgeInstallCtx, BridgeSink, TrustedBridgeKind, TRUSTED_BROKER_SCRIPT};
use crate::browser::security::BrowserError;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{
    NSError, NSJSONSerialization, NSJSONWritingOptions, NSObject, NSObjectProtocol, NSString,
};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebView,
};
use std::panic::AssertUnwindSafe;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Name of the isolated world. Also how it shows up in Web Inspector.
pub(crate) const WORLD_NAME: &str = "ferryx-bridge";
/// Name of the world-scoped script message handler the broker posts to.
pub(crate) const HANDLER_NAME: &str = "ferryxTrusted";
const EVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

thread_local! {
    /// The world each guest was installed with, keyed by tauri webview label. Main thread
    /// only (WebKit objects are MainThreadOnly); eval_in_world reuses it instead of asking
    /// worldWithName again.
    static INSTALLED_WORLDS: RefCell<HashMap<String, Retained<WKContentWorld>>> =
        RefCell::new(HashMap::new());
}

fn installed_world(label: &str) -> Option<Retained<WKContentWorld>> {
    INSTALLED_WORLDS.with(|worlds| worlds.borrow().get(label).cloned())
}

/// Drops the retained world when the browser session is removed. The map is main-thread
/// local, so call it on the main thread.
pub(crate) fn forget_webview(label: &str) {
    INSTALLED_WORLDS.with(|worlds| {
        worlds.borrow_mut().remove(label);
    });
}

/// Which WebKit APIs the running OS provides. All of them are macOS 11+.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldApiProbe {
    pub content_world_class: bool,
    pub world_with_name: bool,
    pub add_handler_in_world: bool,
    pub user_script_in_world: bool,
    pub evaluate_in_world: bool,
}

/// The isolated world is usable only when every API it needs exists. Anything less
/// falls back to Unavailable rather than half-installing a channel.
pub(crate) fn world_api_available(probe: WorldApiProbe) -> bool {
    probe.content_world_class
        && probe.world_with_name
        && probe.add_handler_in_world
        && probe.user_script_in_world
        && probe.evaluate_in_world
}

fn class_responds(name: &std::ffi::CStr, sel: objc2::runtime::Sel) -> bool {
    AnyClass::get(name).is_some_and(|class| class.responds_to(sel))
}

/// Asks the Objective-C runtime (no engine needed) which world APIs exist.
pub(crate) fn probe_world_api() -> WorldApiProbe {
    let world_class = AnyClass::get(c"WKContentWorld");
    WorldApiProbe {
        content_world_class: world_class.is_some(),
        world_with_name: world_class
            .is_some_and(|class| class.metaclass().responds_to(sel!(worldWithName:))),
        add_handler_in_world: class_responds(
            c"WKUserContentController",
            sel!(addScriptMessageHandler:contentWorld:name:),
        ),
        user_script_in_world: class_responds(
            c"WKUserScript",
            sel!(initWithSource:injectionTime:forMainFrameOnly:inContentWorld:),
        ),
        evaluate_in_world: class_responds(
            c"WKWebView",
            sel!(evaluateJavaScript:inFrame:inContentWorld:completionHandler:),
        ),
    }
}

/// Why a script message was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MessageRejection {
    WrongHandler,
    ForeignWorld,
    Subframe,
    BodyNotString,
}

/// Native facts about one `WKScriptMessage`, gathered by the handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptMessageFacts {
    pub handler_name: String,
    /// Name of the world the message came from (None for the unnamed page/default world).
    pub message_world_name: Option<String>,
    pub is_main_frame: bool,
    /// The body, when it is an NSString.
    pub body: Option<String>,
}

/// The accept/reject decision for one script message: our handler, the world named
/// WORLD_NAME, the main frame and a string body. Returns the raw envelope to sink.
pub(crate) fn judge_script_message(facts: ScriptMessageFacts) -> Result<String, MessageRejection> {
    if facts.handler_name != HANDLER_NAME {
        return Err(MessageRejection::WrongHandler);
    }
    if facts.message_world_name.as_deref() != Some(WORLD_NAME) {
        return Err(MessageRejection::ForeignWorld);
    }
    if !facts.is_main_frame {
        return Err(MessageRejection::Subframe);
    }
    facts.body.ok_or(MessageRejection::BodyNotString)
}

/// A script message body as a Rust string, only when it is an NSString.
pub(crate) fn body_string(body: &AnyObject) -> Option<String> {
    body.downcast_ref::<NSString>().map(NSString::to_string)
}

pub(crate) struct HandlerIvars {
    browser_id: String,
    sink: BridgeSink,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - WKScriptMessageHandler requires MainThreadOnly; WebKit calls it on the main thread.
    // - FerryxTrustedHandler does not implement Drop. Its ivars hold no reference back to
    //   the webview or its content controller, so the controller -> handler retain is not
    //   part of a cycle.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FerryxTrustedHandler"]
    #[ivars = HandlerIvars]
    pub(crate) struct FerryxTrustedHandler;

    unsafe impl NSObjectProtocol for FerryxTrustedHandler {}

    unsafe impl WKScriptMessageHandler for FerryxTrustedHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive_script_message(
            &self,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            self.handle_message(message);
        }
    }
);

impl FerryxTrustedHandler {
    fn new(
        mtm: MainThreadMarker,
        browser_id: String,
        sink: BridgeSink,
    ) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(HandlerIvars {
            browser_id,
            sink,
        });
        // SAFETY: freshly allocated, ivars initialised; NSObject's init is always valid.
        unsafe { msg_send![super(this), init] }
    }

    fn handle_message(&self, message: &WKScriptMessage) {
        let ivars = self.ivars();
        // SAFETY: plain property reads on a live WKScriptMessage, on the main thread.
        let facts = unsafe {
            ScriptMessageFacts {
                handler_name: message.name().to_string(),
                message_world_name: message.world().name().map(|name| name.to_string()),
                is_main_frame: message.frameInfo().isMainFrame(),
                body: body_string(&message.body()),
            }
        };
        match judge_script_message(facts) {
            Ok(body) => (ivars.sink)(&ivars.browser_id, &body, Attestation::IsolatedWorld),
            Err(reason) => tracing::warn!(
                browser_id = %ivars.browser_id,
                ?reason,
                "dropped a trusted bridge script message"
            ),
        }
    }
}

/// Installs the world, handler and broker on one guest webview. Must run on the main
/// thread (the create flow calls it inside `run_on_main_thread`).
pub(crate) fn install<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    ctx: &BridgeInstallCtx,
) -> TrustedBridgeKind {
    if MainThreadMarker::new().is_none() {
        tracing::warn!(browser_id = %ctx.browser_id, "trusted bridge install was called off the main thread");
        return TrustedBridgeKind::Unavailable;
    }
    let slot = Arc::new(Mutex::new(None::<TrustedBridgeKind>));
    let result_slot = Arc::clone(&slot);
    let ctx = ctx.clone();
    let label = webview.label().to_string();
    let dispatched = webview.with_webview(move |platform| {
        let kind = install_on_main(platform.inner(), platform.controller(), &ctx, &label);
        if let Ok(mut guard) = result_slot.lock() {
            *guard = Some(kind);
        }
    });
    if let Err(error) = dispatched {
        tracing::warn!(%error, "could not reach the guest webview to install the trusted bridge");
        return TrustedBridgeKind::Unavailable;
    }
    // On the main thread tauri-runtime-wry runs the closure synchronously, so the slot is
    // filled. If it is not, the conservative kind is reported.
    let kind = slot.lock().ok().and_then(|guard| *guard);
    kind.unwrap_or_else(|| {
        tracing::warn!("the trusted bridge install did not complete synchronously");
        TrustedBridgeKind::Unavailable
    })
}

fn install_on_main(
    webview_ptr: *mut std::ffi::c_void,
    controller_ptr: *mut std::ffi::c_void,
    ctx: &BridgeInstallCtx,
    label: &str,
) -> TrustedBridgeKind {
    let Some(mtm) = MainThreadMarker::new() else {
        return TrustedBridgeKind::Unavailable;
    };
    if webview_ptr.is_null() || controller_ptr.is_null() {
        tracing::warn!(browser_id = %ctx.browser_id, "guest webview has no native handle");
        return TrustedBridgeKind::Unavailable;
    }
    // Native key attribution does not depend on the world, so register on every macOS.
    ctx.manager
        .register_guest_view(&ctx.browser_id, webview_ptr as usize);

    let probe = probe_world_api();
    if !world_api_available(probe) {
        tracing::info!(browser_id = %ctx.browser_id, ?probe, "WKContentWorld is unavailable; trusted bridge disabled");
        return TrustedBridgeKind::Unavailable;
    }

    // SAFETY: tauri hands us the live WKUserContentController of this webview, valid for
    // the duration of the with_webview callback on the main thread.
    let controller: &WKUserContentController = unsafe { &*controller_ptr.cast() };
    let installed = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        let world = WKContentWorld::worldWithName(&NSString::from_str(WORLD_NAME), mtm);
        let handler = FerryxTrustedHandler::new(
            mtm,
            ctx.browser_id.clone(),
            Arc::clone(&ctx.sink),
        );
        controller.addScriptMessageHandler_contentWorld_name(
            ProtocolObject::from_ref(&*handler),
            &world,
            &NSString::from_str(HANDLER_NAME),
        );
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &NSString::from_str(TRUSTED_BROKER_SCRIPT),
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &world,
        );
        controller.addUserScript(&script);
        world
    }));
    match installed {
        Ok(world) => {
            INSTALLED_WORLDS.with(|worlds| {
                worlds.borrow_mut().insert(label.to_string(), world);
            });
            TrustedBridgeKind::Isolated
        }
        Err(exception) => {
            tracing::warn!(
                browser_id = %ctx.browser_id,
                exception = ?exception,
                "WebKit rejected the trusted bridge install"
            );
            TrustedBridgeKind::Unavailable
        }
    }
}

/// Serialises a WebKit evaluation result like wry's page-world eval: JSON with fragments
/// allowed, and an empty string for nil.
fn result_json(value: *mut AnyObject) -> Result<String, BrowserError> {
    if value.is_null() {
        return Ok(String::new());
    }
    // SAFETY: WebKit passes a live object for the duration of the completion handler.
    let value = unsafe { &*value };
    let serialised = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        NSJSONSerialization::dataWithJSONObject_options_error(
            value,
            NSJSONWritingOptions::FragmentsAllowed,
        )
    }));
    match serialised {
        Ok(Ok(data)) => String::from_utf8(data.to_vec())
            .map_err(|error| BrowserError::AutomationFailed(error.to_string())),
        Ok(Err(error)) => Err(BrowserError::AutomationFailed(
            error.localizedDescription().to_string(),
        )),
        Err(_) => Err(BrowserError::AutomationFailed(
            "isolated world result is not JSON-serialisable".into(),
        )),
    }
}

/// Evaluates `script` in the "ferryx-bridge" world of the main frame.
pub(crate) async fn eval_in_world<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    script: String,
) -> Result<String, BrowserError> {
    if !world_api_available(probe_world_api()) {
        return Err(BrowserError::PlatformUnsupported(
            "no isolated JS world is available for this browser tab".into(),
        ));
    }
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<String, BrowserError>>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let send = move |result: Result<String, BrowserError>| {
        if let Some(sender) = sender.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = sender.send(result);
        }
    };
    let send = Arc::new(send);
    let label = webview.label().to_string();
    webview
        .with_webview(move |platform| {
            if MainThreadMarker::new().is_none() {
                send(Err(BrowserError::AutomationFailed(
                    "isolated world evaluation ran off the main thread".into(),
                )));
                return;
            }
            // The world retained at install; a tab that never got one has no world.
            let Some(world) = installed_world(&label) else {
                send(Err(BrowserError::PlatformUnsupported(
                    "no isolated JS world is available for this browser tab".into(),
                )));
                return;
            };
            let webview_ptr = platform.inner();
            if webview_ptr.is_null() {
                send(Err(BrowserError::AutomationFailed(
                    "guest webview has no native handle".into(),
                )));
                return;
            }
            // SAFETY: tauri hands us the live WKWebView on the main thread.
            let native: &WKWebView = unsafe { &*webview_ptr.cast() };
            let completion_send = Arc::clone(&send);
            let handler = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                let result = if error.is_null() {
                    result_json(value)
                } else {
                    // SAFETY: WebKit passes a live NSError for the duration of the call.
                    Err(BrowserError::AutomationFailed(unsafe {
                        (*error).localizedDescription().to_string()
                    }))
                };
                completion_send(result);
            });
            let started = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
                native.evaluateJavaScript_inFrame_inContentWorld_completionHandler(
                    &NSString::from_str(&script),
                    None,
                    &world,
                    Some(&handler),
                );
            }));
            if started.is_err() {
                send(Err(BrowserError::AutomationFailed(
                    "WebKit rejected the isolated world evaluation".into(),
                )));
            }
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    tokio::time::timeout(EVAL_TIMEOUT, receiver)
        .await
        .map_err(|_| BrowserError::AutomationFailed("webview evaluation timed out".into()))?
        .map_err(|_| BrowserError::AutomationFailed("webview evaluation was cancelled".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_present() -> WorldApiProbe {
        WorldApiProbe {
            content_world_class: true,
            world_with_name: true,
            add_handler_in_world: true,
            user_script_in_world: true,
            evaluate_in_world: true,
        }
    }

    fn facts() -> ScriptMessageFacts {
        ScriptMessageFacts {
            handler_name: HANDLER_NAME.into(),
            message_world_name: Some(WORLD_NAME.into()),
            is_main_frame: true,
            body: Some(r#"{"v":1,"kind":"popupSelfClose"}"#.into()),
        }
    }

    #[test]
    fn world_api_requires_every_selector() {
        assert!(world_api_available(all_present()));
        let missing: [fn(&mut WorldApiProbe); 5] = [
            |p| p.content_world_class = false,
            |p| p.world_with_name = false,
            |p| p.add_handler_in_world = false,
            |p| p.user_script_in_world = false,
            |p| p.evaluate_in_world = false,
        ];
        for knock_out in missing {
            let mut probe = all_present();
            knock_out(&mut probe);
            assert!(!world_api_available(probe), "{probe:?} must fall back to Unavailable");
        }
    }

    #[test]
    fn runtime_probe_finds_the_world_api_on_this_host() {
        // Host-dependent by design: every supported dev/CI Mac runs macOS 11 or newer.
        assert_eq!(probe_world_api(), all_present());
    }

    #[test]
    fn accepts_only_the_named_world_main_frame_string_body() {
        assert_eq!(
            judge_script_message(facts()),
            Ok(r#"{"v":1,"kind":"popupSelfClose"}"#.to_string())
        );
    }

    #[test]
    fn rejects_a_message_from_another_world() {
        // The page world and the default client world have no name.
        let mut page_world = facts();
        page_world.message_world_name = None;
        assert_eq!(judge_script_message(page_world), Err(MessageRejection::ForeignWorld));
        let mut other = facts();
        other.message_world_name = Some("another-world".into());
        assert_eq!(judge_script_message(other), Err(MessageRejection::ForeignWorld));
        let mut empty = facts();
        empty.message_world_name = Some(String::new());
        assert_eq!(judge_script_message(empty), Err(MessageRejection::ForeignWorld));
    }

    #[test]
    fn rejects_a_subframe_message_even_from_our_world() {
        let mut subframe = facts();
        subframe.is_main_frame = false;
        assert_eq!(judge_script_message(subframe), Err(MessageRejection::Subframe));
    }

    #[test]
    fn rejects_non_string_bodies_and_other_handlers() {
        let mut object_body = facts();
        object_body.body = None;
        assert_eq!(judge_script_message(object_body), Err(MessageRejection::BodyNotString));
        let mut other = facts();
        other.handler_name = "ipc".into();
        assert_eq!(judge_script_message(other), Err(MessageRejection::WrongHandler));
    }

    #[test]
    fn body_extraction_takes_only_nsstring() {
        let text = NSString::from_str(r#"{"v":1}"#);
        assert_eq!(body_string(&text).as_deref(), Some(r#"{"v":1}"#));
        let object = NSObject::new();
        assert_eq!(body_string(&object), None);
    }

    #[test]
    fn world_results_serialise_like_page_eval() {
        assert_eq!(result_json(std::ptr::null_mut()), Ok(String::new()));
        let text = NSString::from_str("armed");
        let ptr = Retained::as_ptr(&text) as *mut AnyObject;
        assert_eq!(result_json(ptr), Ok("\"armed\"".to_string()));
        let object = NSObject::new();
        let ptr = Retained::as_ptr(&object) as *mut AnyObject;
        assert!(result_json(ptr).is_err());
    }
}
