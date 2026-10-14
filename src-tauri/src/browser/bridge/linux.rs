//! Linux trusted bridge (design 3.2): a WebKitGTK script world plus a world-scoped message
//! handler, and the native GTK key source for app-chrome shortcuts (design 3.0, 4.2).
//!
//! Install order follows wry (connect the signal, then register the handler) and adds the
//! broker only once the handler exists, so a document never sees a broker without its channel.
//! The handler name exists only in the "ferryx-bridge" world, and the broker is injected in the
//! top frame only (design 3.2 step 4). That page script and subframes cannot reach the handler
//! is the documented WebKitGTK contract, UNVERIFIED by probe here (design 5.2 L2).

use super::keys::{chord_to_action, Chord, KeyId, KeyPlatform};
use super::{Attestation, BridgeInstallCtx, TrustedBridgeKind, TRUSTED_BROKER_SCRIPT};
use crate::browser::security::BrowserError;
use gtk::gdk;
use gtk::glib;

/// Isolated script world shared with the other engines.
pub(crate) const WORLD_NAME: &str = "ferryx-bridge";
/// World-scoped message handler the broker posts to (webkit.messageHandlers.ferryxTrusted).
pub(crate) const HANDLER_NAME: &str = "ferryxTrusted";
const EVAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The native steps of an install, so the sequencing is testable without a display.
trait WorldInstallApi {
    type Handler;
    /// Connects the script-message-received signal for [`HANDLER_NAME`].
    fn connect_message_handler(&self) -> Self::Handler;
    /// Registers [`HANDLER_NAME`] in [`WORLD_NAME`]; false when WebKit refuses.
    fn register_in_world(&self) -> bool;
    /// Adds [`TRUSTED_BROKER_SCRIPT`] to [`WORLD_NAME`], top frame, document start.
    fn add_broker_script(&self);
    /// Undoes [`Self::connect_message_handler`].
    fn disconnect(&self, handler: Self::Handler);
}

/// Connect, then register, then inject. A refused registration leaves nothing installed.
fn install_world<A: WorldInstallApi>(api: &A) -> TrustedBridgeKind {
    let handler = api.connect_message_handler();
    if !api.register_in_world() {
        api.disconnect(handler);
        return TrustedBridgeKind::Unavailable;
    }
    api.add_broker_script();
    TrustedBridgeKind::Isolated
}

struct GtkWorldApi<'a> {
    ucm: &'a webkit2gtk::UserContentManager,
    ctx: &'a BridgeInstallCtx,
}

impl WorldInstallApi for GtkWorldApi<'_> {
    type Handler = glib::SignalHandlerId;

    fn connect_message_handler(&self) -> Self::Handler {
        use javascriptcore::ValueExt;
        use webkit2gtk::UserContentManagerExt;

        let browser_id = self.ctx.browser_id.clone();
        let sink = std::sync::Arc::clone(&self.ctx.sink);
        self.ucm
            .connect_script_message_received(Some(HANDLER_NAME), move |_, message| {
                // The broker posts JSON.stringify(envelope), so anything else is not ours.
                match message.js_value() {
                    Some(value) if value.is_string() => {
                        sink(&browser_id, value.to_str().as_str(), Attestation::IsolatedWorld)
                    }
                    _ => tracing::warn!(
                        browser_id = %browser_id,
                        "ignored a trusted bridge message that is not a string"
                    ),
                }
            })
    }

    fn register_in_world(&self) -> bool {
        use webkit2gtk::UserContentManagerExt;
        self.ucm
            .register_script_message_handler_in_world(HANDLER_NAME, WORLD_NAME)
    }

    fn add_broker_script(&self) {
        use webkit2gtk::UserContentManagerExt;
        let script = webkit2gtk::UserScript::for_world(
            TRUSTED_BROKER_SCRIPT,
            webkit2gtk::UserContentInjectedFrames::TopFrame,
            webkit2gtk::UserScriptInjectionTime::Start,
            WORLD_NAME,
            &[],
            &[],
        );
        self.ucm.add_script(&script);
    }

    fn disconnect(&self, handler: Self::Handler) {
        use glib::ObjectExt;
        self.ucm.disconnect(handler);
    }
}

pub(crate) fn install<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    ctx: &BridgeInstallCtx,
) -> TrustedBridgeKind {
    use tauri::Manager;

    let outcome = std::sync::Arc::new(std::sync::Mutex::new(None::<TrustedBridgeKind>));
    let slot = std::sync::Arc::clone(&outcome);
    let app = webview.app_handle().clone();
    let ctx = ctx.clone();
    // `install` runs on the main thread, where tauri-runtime-wry runs this closure inline
    // (send_user_message). If it were ever deferred, the slot stays empty and the tab keeps
    // the conservative Unavailable kind.
    let dispatched = webview.with_webview(move |platform_webview| {
        use webkit2gtk::WebViewExt;

        let guest = platform_webview.inner();
        install_key_source(&guest, app, ctx.browser_id.clone());
        let kind = match guest.user_content_manager() {
            Some(ucm) => install_world(&GtkWorldApi { ucm: &ucm, ctx: &ctx }),
            None => {
                tracing::warn!(browser_id = %ctx.browser_id, "guest webview has no user content manager");
                TrustedBridgeKind::Unavailable
            }
        };
        if let Ok(mut slot) = slot.lock() {
            *slot = Some(kind);
        }
    });
    if let Err(error) = dispatched {
        tracing::warn!(%error, "could not reach the guest webview to install the trusted bridge");
        return TrustedBridgeKind::Unavailable;
    }
    let kind = outcome.lock().ok().and_then(|slot| *slot);
    kind.unwrap_or_else(|| {
        tracing::warn!("trusted bridge install did not run synchronously; treating the tab as Unavailable");
        TrustedBridgeKind::Unavailable
    })
}

pub(crate) async fn eval_in_world<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    script: String,
) -> Result<String, BrowserError> {
    let (sender, receiver) = tokio::sync::oneshot::channel::<Result<String, String>>();
    webview
        .with_webview(move |platform_webview| {
            use javascriptcore::ValueExt;
            use webkit2gtk::WebViewExt;

            platform_webview.inner().evaluate_javascript(
                &script,
                Some(WORLD_NAME),
                None,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    // Same shape as the page-world eval (wry's to_json), so callers such as
                    // picker::unwrap_eval_json read both alike.
                    let outcome = result
                        .map(|value| value.to_json(0).map(|json| json.to_string()).unwrap_or_default())
                        .map_err(|error| error.to_string());
                    let _ = sender.send(outcome);
                },
            );
        })
        .map_err(|error| BrowserError::AutomationFailed(error.to_string()))?;
    tokio::time::timeout(EVAL_TIMEOUT, receiver)
        .await
        .map_err(|_| BrowserError::AutomationFailed("isolated world evaluation timed out".into()))?
        .map_err(|_| BrowserError::AutomationFailed("isolated world evaluation was cancelled".into()))?
        .map_err(BrowserError::AutomationFailed)
}

/// GDK keyval to the physical key a chord names. Shifted US symbols map back to their key so
/// Ctrl+Shift+= / Ctrl+Shift+[ / Shift+Tab match the table the same way macOS keyCodes do.
pub(crate) fn gdk_key_id(keyval: u32) -> KeyId {
    use gdk::keys::constants as k;
    let key = gdk::keys::Key::from(keyval);
    match keyval {
        0x61..=0x7a => return KeyId::Letter(char::from(keyval as u8)),
        0x41..=0x5a => return KeyId::Letter(char::from(keyval as u8).to_ascii_lowercase()),
        0x30..=0x39 => return KeyId::Digit((keyval - 0x30) as u8),
        _ => {}
    }
    if key == k::comma || key == k::less {
        KeyId::Comma
    } else if key == k::equal || key == k::plus {
        KeyId::Equal
    } else if key == k::minus || key == k::underscore {
        KeyId::Minus
    } else if key == k::bracketleft || key == k::braceleft {
        KeyId::BracketLeft
    } else if key == k::bracketright || key == k::braceright {
        KeyId::BracketRight
    } else if key == k::Tab || key == k::ISO_Left_Tab || key == k::KP_Tab {
        KeyId::Tab
    } else if key == k::Page_Up || key == k::KP_Page_Up {
        KeyId::PageUp
    } else if key == k::Page_Down || key == k::KP_Page_Down {
        KeyId::PageDown
    } else {
        KeyId::Other
    }
}

/// GDK keyval + modifier state to a [`Chord`]. Meta is the Super/Hyper key (MOD4). GDK's
/// virtual META_MASK is deliberately ignored: X keymaps often alias it to Alt (MOD1), which
/// would turn every Alt chord into Alt+Meta (UNVERIFIED per keymap; Wayland and X11 differ).
pub(crate) fn gdk_chord(keyval: u32, state: gdk::ModifierType) -> Chord {
    use gdk::ModifierType as M;
    Chord {
        key: gdk_key_id(keyval),
        ctrl: state.contains(M::CONTROL_MASK),
        meta: state.intersects(M::SUPER_MASK | M::HYPER_MASK | M::MOD4_MASK),
        alt: state.contains(M::MOD1_MASK),
        shift: state.contains(M::SHIFT_MASK),
    }
}

/// The action a native key press on a guest maps to, or None. Pure, so it is testable.
pub(crate) fn guest_key_action(keyval: u32, state: gdk::ModifierType) -> Option<&'static str> {
    chord_to_action(gdk_chord(keyval, state), KeyPlatform::Linux)
}

/// The keyval of this key on the first layout group with no modifiers, so Ctrl+W still means W
/// under a non-Latin active layout (the event keyval is then Cyrillic, Hangul, ...).
fn base_layout_keyval(event: &gdk::EventKey) -> Option<u32> {
    let display = gdk::Display::default()?;
    let keymap = gdk::Keymap::for_display(&display)?;
    keymap
        .translate_keyboard_state(
            u32::from(event.hardware_keycode()),
            gdk::ModifierType::empty(),
            0,
        )
        .map(|(keyval, ..)| keyval)
}

/// Native key source (design 3.0): GTK key-press-event on the guest WebView. Page script cannot
/// synthesize these. A matched chord emits browser_shortcut_requested exactly like the macOS
/// key monitor and stops the event; everything else goes on to WebKit.
fn install_key_source<R: tauri::Runtime>(
    guest: &webkit2gtk::WebView,
    app: tauri::AppHandle<R>,
    browser_id: String,
) {
    use gtk::prelude::WidgetExt;
    use std::cell::Cell;
    use std::rc::Rc;
    use tauri::Emitter;
    use webkit2gtk::{InputMethodContextExt, WebViewExt};

    // Only an app-provided WebKitInputMethodContext reports preedit. With the default GTK IM
    // module there is none and composition is not observable here (UNVERIFIED: whether an IM
    // module ever holds a Ctrl/Alt chord inside a preedit).
    let composing = Rc::new(Cell::new(false));
    if let Some(context) = guest.input_method_context() {
        let started = Rc::clone(&composing);
        context.connect_preedit_started(move |_| started.set(true));
        let finished = Rc::clone(&composing);
        context.connect_preedit_finished(move |_| finished.set(false));
    }
    guest.connect_key_press_event(move |_, event| {
        if composing.get() || event.is_modifier() {
            return glib::Propagation::Proceed;
        }
        let state = event.state();
        let mut action = guest_key_action(*event.keyval(), state);
        if action.is_none() && gdk_key_id(*event.keyval()) == KeyId::Other {
            action = base_layout_keyval(event).and_then(|keyval| guest_key_action(keyval, state));
        }
        let Some(action) = action else {
            return glib::Propagation::Proceed;
        };
        let _ = app.emit(
            crate::browser::BROWSER_SHORTCUT_REQUESTED_EVENT,
            crate::browser::BrowserShortcutRequestedPayload {
                browser_id: browser_id.clone(),
                action: action.to_string(),
            },
        );
        glib::Propagation::Stop
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdk::keys::constants as k;
    use gdk::ModifierType as M;
    use std::cell::RefCell;

    #[test]
    fn letters_digits_and_symbols_map_to_physical_keys() {
        assert_eq!(gdk_key_id(*k::w), KeyId::Letter('w'));
        assert_eq!(gdk_key_id(*k::W), KeyId::Letter('w'));
        assert_eq!(gdk_key_id(*k::T), KeyId::Letter('t'));
        assert_eq!(gdk_key_id(*k::_0), KeyId::Digit(0));
        assert_eq!(gdk_key_id(*k::_9), KeyId::Digit(9));
        assert_eq!(gdk_key_id(*k::comma), KeyId::Comma);
        assert_eq!(gdk_key_id(*k::plus), KeyId::Equal);
        assert_eq!(gdk_key_id(*k::equal), KeyId::Equal);
        assert_eq!(gdk_key_id(*k::underscore), KeyId::Minus);
        assert_eq!(gdk_key_id(*k::braceleft), KeyId::BracketLeft);
        assert_eq!(gdk_key_id(*k::bracketright), KeyId::BracketRight);
        assert_eq!(gdk_key_id(*k::ISO_Left_Tab), KeyId::Tab);
        assert_eq!(gdk_key_id(*k::Tab), KeyId::Tab);
        assert_eq!(gdk_key_id(*k::Page_Up), KeyId::PageUp);
        assert_eq!(gdk_key_id(*k::Page_Down), KeyId::PageDown);
        assert_eq!(gdk_key_id(*k::Escape), KeyId::Other);
        assert_eq!(gdk_key_id(*k::Cyrillic_tse), KeyId::Other);
    }

    #[test]
    fn modifier_state_maps_to_chord_flags_without_meta_alias() {
        let chord = gdk_chord(*k::w, M::CONTROL_MASK | M::SHIFT_MASK);
        assert!(chord.ctrl && chord.shift && !chord.alt && !chord.meta);
        let chord = gdk_chord(*k::_1, M::MOD1_MASK | M::META_MASK);
        assert!(chord.alt && !chord.meta, "META_MASK alone must not read as Super");
        assert!(gdk_chord(*k::w, M::SUPER_MASK).meta);
        assert!(gdk_chord(*k::w, M::MOD4_MASK).meta);
        // Lock and mouse-button bits never count as modifiers.
        let chord = gdk_chord(*k::w, M::LOCK_MASK | M::MOD2_MASK | M::BUTTON1_MASK);
        assert!(!chord.ctrl && !chord.meta && !chord.alt && !chord.shift);
    }

    #[test]
    fn linux_guest_chords_reach_the_shared_table() {
        let ctrl = M::CONTROL_MASK;
        let shift = M::SHIFT_MASK;
        assert_eq!(guest_key_action(*k::w, ctrl), Some("tab-close"));
        assert_eq!(guest_key_action(*k::t, ctrl), Some("tab-new-browser"));
        assert_eq!(guest_key_action(*k::T, ctrl | shift), Some("tab-reopen-closed"));
        assert_eq!(guest_key_action(*k::l, ctrl | M::LOCK_MASK), Some("focus-address"));
        assert_eq!(guest_key_action(*k::plus, ctrl | shift), Some("zoom-in"));
        assert_eq!(guest_key_action(*k::braceleft, ctrl | shift), Some("tab-previous"));
        assert_eq!(guest_key_action(*k::Tab, ctrl), Some("tab-next"));
        assert_eq!(guest_key_action(*k::ISO_Left_Tab, ctrl | shift), Some("tab-previous"));
        assert_eq!(guest_key_action(*k::Page_Down, ctrl), Some("tab-next"));
        assert_eq!(guest_key_action(*k::_3, M::MOD1_MASK), Some("workspace-select-3"));
        assert_eq!(guest_key_action(*k::_3, ctrl), Some("tab-select-3"));
        // Not chords: plain typing, Super as primary, and the self-scoped reload.
        assert_eq!(guest_key_action(*k::w, M::empty()), None);
        assert_eq!(guest_key_action(*k::w, M::SUPER_MASK), None);
        assert_eq!(guest_key_action(*k::r, ctrl), None);
        assert_eq!(guest_key_action(*k::Cyrillic_tse, ctrl), None);
    }

    #[derive(Default)]
    struct FakeApi {
        register_ok: bool,
        calls: RefCell<Vec<&'static str>>,
    }

    impl WorldInstallApi for FakeApi {
        type Handler = u32;
        fn connect_message_handler(&self) -> u32 {
            self.calls.borrow_mut().push("connect");
            7
        }
        fn register_in_world(&self) -> bool {
            self.calls.borrow_mut().push("register");
            self.register_ok
        }
        fn add_broker_script(&self) {
            self.calls.borrow_mut().push("add_script");
        }
        fn disconnect(&self, handler: u32) {
            assert_eq!(handler, 7);
            self.calls.borrow_mut().push("disconnect");
        }
    }

    #[test]
    fn install_connects_before_registering_and_injects_last() {
        let api = FakeApi { register_ok: true, ..FakeApi::default() };
        assert_eq!(install_world(&api), TrustedBridgeKind::Isolated);
        assert_eq!(*api.calls.borrow(), ["connect", "register", "add_script"]);
    }

    #[test]
    fn refused_registration_is_unavailable_and_rolls_back() {
        let api = FakeApi { register_ok: false, ..FakeApi::default() };
        assert_eq!(install_world(&api), TrustedBridgeKind::Unavailable);
        assert_eq!(*api.calls.borrow(), ["connect", "register", "disconnect"]);
    }

    #[test]
    fn world_and_handler_names_match_the_broker() {
        assert!(TRUSTED_BROKER_SCRIPT.contains(&format!("messageHandlers.{HANDLER_NAME}")));
        assert_eq!(WORLD_NAME, "ferryx-bridge");
    }
}
