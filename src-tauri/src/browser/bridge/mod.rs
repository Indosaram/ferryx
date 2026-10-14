//! Trusted guest -> host bridge (design: .omo/ulw-research/20261013-browser-audit/BRIDGE-DESIGN.md).
//!
//! Class (a) actions (downloads, link routing, popup open, element-pick acceptance and
//! popup self-close) no longer travel as nonce-bearing page-world navigations. They reach
//! the host only through a per-engine channel bound to an isolated JS world that page
//! script cannot enter, and every request is re-validated here by [`admit`] against the
//! table in design section 3.5.
//!
//! # Platform seam
//!
//! Each OS module (`macos.rs`, `linux.rs`, `windows.rs`, `fallback.rs`) implements exactly these
//! two functions with these signatures, so a platform step never edits this file:
//!
//! ```ignore
//! pub(crate) fn install<R: tauri::Runtime>(
//!     webview: &tauri::Webview<R>,
//!     ctx: &BridgeInstallCtx,
//! ) -> TrustedBridgeKind;
//!
//! pub(crate) async fn eval_in_world<R: tauri::Runtime>(
//!     webview: tauri::Webview<R>,
//!     script: String,
//! ) -> Result<String, BrowserError>;
//! ```
//!
//! `install` runs on the main thread right after `add_child`, while the guest is still at
//! about:blank. It injects [`TRUSTED_BROKER_SCRIPT`] into the isolated world (main frame,
//! document start), wires the world-scoped message channel to `ctx.sink` with
//! [`Attestation::IsolatedWorld`], and returns the kind it actually achieved. It must never
//! return [`TrustedBridgeKind::Isolated`] unless page script provably cannot reach the
//! channel. The macOS step also calls
//! `ctx.manager.register_guest_view(&ctx.browser_id, wk_webview_ptr as usize)` so the native
//! key monitor in `lib.rs` can attribute guest-focused chords. The Windows step records
//! native user-initiated navigations with `ctx.manager.record_native_user_initiated`.
//!
//! `eval_in_world` evaluates `script` in the same isolated world (main frame) and resolves to
//! the string result, like `crate::ipc::browser::eval_webview` does for the page world.
//! Without a world it fails with `BrowserError::PlatformUnsupported`.

// macOS, Linux and Windows each have a real platform module; only the catch-all target
// uses the fallback, and the bridge tests exercise it on every target.
pub mod cdp_order;
#[cfg(any(
    test,
    not(any(target_os = "macos", target_os = "linux", target_os = "windows"))
))]
pub mod fallback;
pub mod keys;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
use fallback as platform;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(target_os = "windows")]
use windows as platform;

use crate::browser::manager::BrowserManager;
use crate::browser::security::BrowserError;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Whether this guest has an unforgeable guest -> host channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustedBridgeKind {
    /// The broker runs in an isolated world bound to a world-scoped native channel.
    Isolated,
    /// No isolated world: the section 3.5 "Unavailable" rules apply. This is the default,
    /// so state persisted before the field existed loads as the conservative kind.
    #[default]
    Unavailable,
}

/// A class (a) request, as parsed from a broker envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedAction {
    Download { url: String },
    Link { url: String, modifier: bool },
    PopupOpen { url: String, handle: String },
    PickReady { token: String },
    PopupSelfClose,
}

/// Who vouches for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attestation {
    /// Delivered by the world-scoped channel; the broker saw a trusted DOM event.
    IsolatedWorld,
    /// The platform observed a native user-initiated navigation or new-window event.
    NativeUserInitiated,
    /// Nobody vouches for it.
    None,
}

/// Broker wire format, version 1 (design 3.4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedEnvelope {
    pub v: u32,
    pub kind: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub handle: Option<String>,
    #[serde(default)]
    pub modifier: Option<bool>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub activation_age_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("trusted envelope exceeds {MAX_ENVELOPE_BYTES} bytes")]
    TooLarge,
    #[error("trusted envelope is not valid: {0}")]
    Malformed(String),
    #[error("unsupported trusted envelope version {0}")]
    Version(u32),
    #[error("unknown trusted envelope kind {0:?}")]
    UnknownKind(String),
    #[error("trusted envelope field {0} is missing")]
    Missing(&'static str),
    #[error("trusted envelope URL is not http(s): {0}")]
    BadUrl(String),
    #[error("trusted envelope activation is stale or invalid")]
    StaleActivation,
}

pub const MAX_ENVELOPE_BYTES: usize = 8 * 1024;
/// The transient-activation window the broker stamps and the host re-checks.
pub const ACTIVATION_WINDOW_MS: u64 = 1000;
const MAX_ID_BYTES: usize = 128;

fn http_url(raw: Option<String>) -> Result<String, EnvelopeError> {
    let raw = raw.ok_or(EnvelopeError::Missing("url"))?;
    match tauri::Url::parse(&raw) {
        Ok(parsed) if matches!(parsed.scheme(), "http" | "https") => Ok(parsed.to_string()),
        _ => Err(EnvelopeError::BadUrl(raw)),
    }
}

fn bounded_id(raw: Option<String>, field: &'static str) -> Result<String, EnvelopeError> {
    let value = raw
        .filter(|value| !value.trim().is_empty())
        .ok_or(EnvelopeError::Missing(field))?;
    if value.len() > MAX_ID_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    Ok(value)
}

/// Parses one broker message. Everything is re-validated: the broker is trusted to have
/// seen a trusted event, not to have produced well-formed values.
pub fn parse_envelope(raw: &str) -> Result<TrustedAction, EnvelopeError> {
    if raw.len() > MAX_ENVELOPE_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    let envelope: TrustedEnvelope =
        serde_json::from_str(raw).map_err(|error| EnvelopeError::Malformed(error.to_string()))?;
    if envelope.v != 1 {
        return Err(EnvelopeError::Version(envelope.v));
    }
    match envelope.kind.as_str() {
        "download" => Ok(TrustedAction::Download {
            url: http_url(envelope.url)?,
        }),
        "link" => Ok(TrustedAction::Link {
            url: http_url(envelope.url)?,
            modifier: envelope.modifier.unwrap_or(false),
        }),
        "popupOpen" => {
            let url = http_url(envelope.url)?;
            let handle = bounded_id(envelope.handle, "handle")?;
            let age = envelope
                .activation_age_ms
                .ok_or(EnvelopeError::Missing("activationAgeMs"))?;
            if !age.is_finite() || age < 0.0 || age > ACTIVATION_WINDOW_MS as f64 {
                return Err(EnvelopeError::StaleActivation);
            }
            Ok(TrustedAction::PopupOpen { url, handle })
        }
        "pickReady" => Ok(TrustedAction::PickReady {
            token: bounded_id(envelope.token, "token")?,
        }),
        "popupSelfClose" => Ok(TrustedAction::PopupSelfClose),
        other => Err(EnvelopeError::UnknownKind(other.to_string())),
    }
}

/// What the host does once a request is admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeEmit {
    Download { url: String },
    Link { url: String, modifier: bool },
    /// Navigate the sending tab itself; the page could already do that on its own.
    NavigateInTab { url: String },
    PopupOpen { url: String, handle: String },
    ElementPicked { token: String },
    CloseSelf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Emit(BridgeEmit),
    /// Reduced to an action the page could take by itself.
    Downgrade(BridgeEmit),
    /// Surfaced to the user; nothing is fetched until they confirm.
    NeedsConfirmation(BridgeEmit),
    Deny(&'static str),
}

/// Host facts `admit` decides on, built from `BrowserManager` and the popup registry.
#[derive(Debug, Clone)]
pub struct AdmitCtx<'a> {
    pub is_popup: bool,
    pub armed_pick_token: Option<&'a str>,
    pub recent_native_user_initiated: &'a [(String, Instant)],
    pub now: Instant,
}

impl AdmitCtx<'_> {
    /// A native user-initiated navigation or new-window for exactly this URL within the
    /// activation window. Only the platform layer records these, never page script.
    fn natively_attested(&self, url: &str) -> bool {
        let window = Duration::from_millis(ACTIVATION_WINDOW_MS);
        self.recent_native_user_initiated.iter().any(|(seen, at)| {
            seen == url
                && self
                    .now
                    .checked_duration_since(*at)
                    .is_some_and(|age| age <= window)
        })
    }
}

pub const DENY_PICK_TOKEN: &str = "element pick was not armed with this token";
pub const DENY_PICK_UNAVAILABLE: &str = "picker unavailable: no isolated world";
pub const DENY_SELF_CLOSE_NOT_POPUP: &str = "window.close() is honoured only in popup tabs";
pub const DENY_POPUP_ACTIVATION: &str =
    "popup blocked: window.open needs a user click within 1000 ms";
pub const DENY_POPUP_RATE: &str = "popup blocked: more than 5 popups per second from this tab";
pub const DENY_NEW_WINDOW_UNATTESTED: &str =
    "popup blocked: this tab has no trusted bridge, so a new window cannot be verified as user-initiated";

/// What the guest's native new-window request (WebviewBuilder::on_new_window) gets.
/// Native windows are never allowed: an admitted popup is always a Ferryx tab created from
/// an attested broker request, and nothing opens a tab from an unattested request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewWindowDecision {
    /// Deny silently: in an Isolated tab the broker already owns window.open.
    Deny,
    /// Deny and show this warning in the page.
    DenyWithWarning(&'static str),
}

pub fn new_window_policy(kind: TrustedBridgeKind, url: &str) -> NewWindowDecision {
    let http = tauri::Url::parse(url)
        .is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"));
    match kind {
        TrustedBridgeKind::Isolated => NewWindowDecision::Deny,
        TrustedBridgeKind::Unavailable if http => {
            NewWindowDecision::DenyWithWarning(DENY_NEW_WINDOW_UNATTESTED)
        }
        TrustedBridgeKind::Unavailable => NewWindowDecision::Deny,
    }
}

/// Drops per-webview platform bridge state for a closed session. Windows already does
/// this from BrowserManager::remove_session; macOS keeps its retained world in a
/// main-thread map, so the removal is dispatched there.
pub(crate) fn forget_webview<R: tauri::Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    #[cfg(target_os = "macos")]
    {
        let label = label.to_string();
        if let Err(error) = app.run_on_main_thread(move || macos::forget_webview(&label)) {
            tracing::debug!(%error, "could not drop the retained bridge world");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, label);
}

/// Encodes design section 3.5. Isolated-world trust needs both an Isolated kind and an
/// IsolatedWorld attestation; everything else runs the Unavailable column, where the only
/// escalation is a native user-initiated record for the same URL within 1000 ms.
pub fn admit(
    action: TrustedAction,
    attestation: Attestation,
    kind: TrustedBridgeKind,
    ctx: &AdmitCtx<'_>,
) -> Decision {
    let isolated =
        kind == TrustedBridgeKind::Isolated && attestation == Attestation::IsolatedWorld;
    let native = |url: &str| attestation == Attestation::NativeUserInitiated && ctx.natively_attested(url);
    match action {
        TrustedAction::Download { url } => {
            if isolated || native(&url) {
                Decision::Emit(BridgeEmit::Download { url })
            } else {
                Decision::NeedsConfirmation(BridgeEmit::Download { url })
            }
        }
        TrustedAction::Link { url, modifier } => {
            if isolated || native(&url) {
                Decision::Emit(BridgeEmit::Link { url, modifier })
            } else {
                Decision::Downgrade(BridgeEmit::NavigateInTab { url })
            }
        }
        TrustedAction::PopupOpen { url, handle } => {
            if isolated || native(&url) {
                Decision::Emit(BridgeEmit::PopupOpen { url, handle })
            } else {
                Decision::Deny(DENY_POPUP_ACTIVATION)
            }
        }
        TrustedAction::PickReady { token } => {
            if !isolated {
                Decision::Deny(DENY_PICK_UNAVAILABLE)
            } else if ctx.armed_pick_token == Some(token.as_str()) {
                Decision::Emit(BridgeEmit::ElementPicked { token })
            } else {
                Decision::Deny(DENY_PICK_TOKEN)
            }
        }
        TrustedAction::PopupSelfClose => {
            if ctx.is_popup {
                Decision::Emit(BridgeEmit::CloseSelf)
            } else {
                Decision::Deny(DENY_SELF_CLOSE_NOT_POPUP)
            }
        }
    }
}

/// Host receiver for broker messages: `(browser_id, raw envelope JSON, attestation)`. The
/// platform layer may call it from any thread.
pub type BridgeSink = Arc<dyn Fn(&str, &str, Attestation) + Send + Sync>;

/// Everything a platform `install` needs.
#[derive(Clone)]
pub struct BridgeInstallCtx {
    pub browser_id: String,
    pub manager: Arc<BrowserManager>,
    pub sink: BridgeSink,
}

/// Installs the trusted bridge for one freshly created guest (main thread, after
/// `add_child`, before the guest leaves about:blank).
pub(crate) fn install<R: tauri::Runtime>(
    webview: &tauri::Webview<R>,
    ctx: &BridgeInstallCtx,
) -> TrustedBridgeKind {
    platform::install(webview, ctx)
}

/// Evaluates `script` in the guest's isolated world (main frame).
pub(crate) async fn eval_in_world<R: tauri::Runtime>(
    webview: tauri::Webview<R>,
    script: String,
) -> Result<String, BrowserError> {
    platform::eval_in_world(webview, script).await
}

/// Isolated-world broker (design 3.4). The platform `install` injects it at document start
/// in the main frame only. All state is closure- or world-local: an isolated world has its
/// own global object, so `globalThis.__ferryxWorld` is invisible to page script, and
/// `event.isTrusted` is read through this world's own bindings, which the page cannot patch.
/// The channel is `webkit.messageHandlers.ferryxTrusted` (WKContentWorld / WebKitGTK world)
/// or the CDP binding `__ferryxTrusted` (WebView2); both exist only in this world.
/// The source lives in `broker.js` so `ui/src/lib/bridgeBroker.test.ts` can execute it.
/// When the channel is missing the broker never cancels a native default.
pub const TRUSTED_BROKER_SCRIPT: &str = include_str!("broker.js");

const PICK_TOKEN_PLACEHOLDER: &str = "__FERRYX_PICK_TOKEN__";

/// Arms the picker in the isolated world (design 3.6). Use [`picker_arm_script`], which
/// embeds the host's one-time token. Only a trusted click is accepted; the pick stays on
/// the world-local `api.picker` and the host is told with the token.
pub const PICKER_ARM_SCRIPT: &str = r#"(() => {
  const api = globalThis.__ferryxWorld;
  if (!api) throw new Error('the ferryx trusted bridge is not installed in this document');
  const previous = api.picker;
  if (previous && previous.armed) previous.disarm();
  const outlined = { el: null, outline: '' };
  const restore = () => {
    if (outlined.el) outlined.el.style.outline = outlined.outline;
    outlined.el = null;
  };
  const onOver = (event) => {
    if (event.isTrusted !== true) return;
    const target = event.target;
    if (!target || !target.style) return;
    if (outlined.el && outlined.el !== target) restore();
    outlined.outline = target.style.outline;
    outlined.el = target;
    target.style.outline = '2px solid #4c8dff';
  };
  const onOut = (event) => {
    if (outlined.el === event.target) restore();
  };
  const state = { armed: true, token: __FERRYX_PICK_TOKEN__, pick: null };
  const onClick = (event) => {
    if (event.isTrusted !== true) return;
    event.preventDefault();
    event.stopPropagation();
    const target = event.target;
    const rect = target.getBoundingClientRect();
    state.pick = JSON.stringify({
      outerHTML: target.outerHTML,
      css: getComputedStyle(target).cssText,
      devicePixelRatio: devicePixelRatio || 1,
      rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    });
    if (api.post({ v: 1, kind: 'pickReady', token: state.token })) {
      // One pick per arm: stop capturing but keep state.pick for PICKER_READ_SCRIPT.
      detach();
      state.armed = false;
    }
  };
  const detach = () => {
    document.removeEventListener('mouseover', onOver, true);
    document.removeEventListener('mouseout', onOut, true);
    document.removeEventListener('click', onClick, true);
    restore();
  };
  state.disarm = () => {
    detach();
    state.armed = false;
    state.pick = null;
  };
  document.addEventListener('mouseover', onOver, true);
  document.addEventListener('mouseout', onOut, true);
  document.addEventListener('click', onClick, true);
  api.picker = state;
  return 'armed';
})()"#;

/// Reads the world-local pick. Never touches a page global.
pub const PICKER_READ_SCRIPT: &str = r#"(() => {
  const api = globalThis.__ferryxWorld;
  const picker = api ? api.picker : null;
  return picker && picker.pick ? picker.pick : null;
})()"#;

/// Removes the picker's listeners and outline and drops the world-local pick.
pub const PICKER_DISARM_SCRIPT: &str = r#"(() => {
  const api = globalThis.__ferryxWorld;
  const picker = api ? api.picker : null;
  if (picker && picker.armed) picker.disarm();
  if (api) api.picker = null;
  return 'disarmed';
})()"#;

/// [`PICKER_ARM_SCRIPT`] with the host's one-time token embedded as a JSON string literal.
pub fn picker_arm_script(token: &str) -> String {
    PICKER_ARM_SCRIPT.replace(
        PICK_TOKEN_PLACEHOLDER,
        &crate::browser::popup::script_string(token),
    )
}

#[cfg(test)]
mod tests;
