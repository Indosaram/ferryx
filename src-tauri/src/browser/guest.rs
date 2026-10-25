use url::Url;

pub const BROWSER_SESSION_CREATED_EVENT: &str = "browser_session_created";
pub const BROWSER_OPEN_REQUESTED_EVENT: &str = "browser_open_requested";
pub const BROWSER_DOWNLOAD_REQUESTED_EVENT: &str = "browser_download_requested";
pub const BROWSER_SHORTCUT_REQUESTED_EVENT: &str = "browser_shortcut_requested";
pub const BROWSER_ELEMENT_PICKED_EVENT: &str = "browser_element_picked";
pub const BROWSER_LINK_CLICKED_EVENT: &str = "browser_link_clicked";
pub const BROWSER_CLOSE_REQUESTED_EVENT: &str = "browser_close_requested";

const SHORTCUT_HOST: &str = "shortcut.ferryx.invalid";
/// Popup handle traffic from the opener page: params `handle`, `op=navigate|close|message`.
const POPUP_HOST: &str = "popup.ferryx.invalid";
/// `handle.close()` from the opener page: param `handle`.
const POPUP_CLOSE_HOST: &str = "popupclose.ferryx.invalid";
/// `window.opener.postMessage(...)` from a popup tab: param `msg` = JSON
/// `{ data, targetOrigin }`.
const OPENER_HOST: &str = "opener.ferryx.invalid";

/// Shortcuts the page route may still carry: they only affect the sending tab's own
/// document, which the page can already do itself (design 2.1 "b-self"). App-chrome
/// shortcuts come from native key sources through `crate::browser::bridge::keys`.
const PAGE_SHORTCUT_ACTIONS: [&str; 4] = ["back", "forward", "reload", "reload-hard"];

/// Class (b) guest actions: everything here is something the page could already do to
/// itself or to its own popups. Class (a) actions travel on the trusted bridge
/// (`crate::browser::bridge`) instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserGuestAction {
    /// A self-scoped shortcut (back, forward, reload, reload-hard).
    Shortcut(String),
    /// A later `location.href` / `assign` / `replace` on an already-routed handle.
    PopupNavigate { url: String, handle: String },
    /// `window.opener.postMessage(...)` from a popup tab.
    OpenerMessage { message: String },
    /// `handle.close()` from the opener tab for a specific popup handle.
    PopupCloseHandle { handle: String },
    /// `handle.postMessage(...)` from the opener tab to a specific popup handle.
    PopupMessage { handle: String, message: String },
}

/// Parses a class (b) control navigation from the page-world bridge.
///
/// The `nonce` is a frame-confusion filter, not an authorization boundary: it stops a
/// frame or document that never received the bridge from producing these URLs by accident,
/// but page script can learn it (design 1.2). Nothing accepted here may do more than the
/// page could do to itself or to its own popups.
pub fn parse_browser_guest_action(url: &Url, expected_nonce: &str) -> Option<BrowserGuestAction> {
    let host = url.host_str()?;
    if query_value(url, "nonce").as_deref() != Some(expected_nonce) {
        return None;
    }
    match host {
        SHORTCUT_HOST => query_value(url, "action")
            .filter(|action| PAGE_SHORTCUT_ACTIONS.contains(&action.as_str()))
            .map(BrowserGuestAction::Shortcut),
        POPUP_HOST => {
            let handle = query_value(url, "handle")?;
            match query_value(url, "op").as_deref() {
                Some("close") => Some(BrowserGuestAction::PopupCloseHandle { handle }),
                Some("message") | Some("postmessage") => {
                    let message = query_value(url, "msg")?;
                    Some(BrowserGuestAction::PopupMessage { handle, message })
                }
                Some("navigate") => {
                    let target = query_value(url, "url")?;
                    Some(BrowserGuestAction::PopupNavigate {
                        url: target,
                        handle,
                    })
                }
                // `op=open` (and the legacy op-less open) moved to the trusted bridge.
                _ => None,
            }
        }
        // A bare popupclose (the old popup self-close) is gone: it closed any tab.
        POPUP_CLOSE_HOST => query_value(url, "handle")
            .map(|handle| BrowserGuestAction::PopupCloseHandle { handle }),
        OPENER_HOST => {
            query_value(url, "msg").map(|message| BrowserGuestAction::OpenerMessage { message })
        }
        _ => None,
    }
}
fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.trim().is_empty())
}

const BRIDGE_NONCE_PLACEHOLDER: &str = "__FERRYX_BROWSER_BRIDGE_NONCE__";
const BRIDGE_HARD_RELOAD_PLACEHOLDER: &str = "__FERRYX_BROWSER_BRIDGE_HARD_RELOAD__";
const BRIDGE_NATIVE_OPEN_PLACEHOLDER: &str = "__FERRYX_BROWSER_BRIDGE_NATIVE_OPEN__";
const POPUP_SURFACE_PLACEHOLDER: &str = "__FERRYX_IS_POPUP_SURFACE__";
const WINDOW_CLOSE_BODY_PLACEHOLDER: &str = "__FERRYX_WINDOW_CLOSE_BODY__";

/// Body of the `window.close` replacement in a popup tab, substituted for
/// [`WINDOW_CLOSE_BODY_PLACEHOLDER`]. Closing asks the host, and the host is the only
/// thing that can: it owns the tab, and its close path is what calls
/// `window.__ferryxPopupClosed(handle)` in the opener. The request is an intent for the
/// trusted broker; the host honours it only for a tab it linked as a popup.
const POPUP_WINDOW_CLOSE_BODY: &str = "    postIntent({ op: 'selfClose' });";

/// Body of the `window.close` replacement in every other tab. A page must never close
/// a tab it did not open: WebKitGTK (wry) destroys the webview on a script close, which
/// left the manager holding a dead tab, and other engines may act on it too. So the
/// call reaches nothing and only leaves a console note.
const TAB_WINDOW_CLOSE_BODY: &str =
    "    pushConsoleEntry('warn', '[ferryx] window.close() is ignored: only a popup tab can close itself');";

/// Source of the popup-only bridge surface, substituted for
/// [`POPUP_SURFACE_PLACEHOLDER`].
///
/// The surface is substituted as source rather than gated behind a `true`/`false`
/// placeholder so a tab the host did NOT create as a popup cannot even contain the
/// opener proxy: real OAuth callback pages branch on `if (window.opener)`, and a fake
/// opener makes them postMessage into a proxy nobody reads and call close() instead of
/// continuing the redirect flow. The non-popup test pins that absence.
const POPUP_SURFACE_SOURCE: &str = r#"
  // Popup-only surface: this tab WAS opened by another tab, so it gets an opener
  // proxy whose messages travel through the host to the real opener webview.
  const openerProxy = {
    closed: false,
    postMessage: (message, targetOrigin) => {
      let payload;
      try {
        payload = JSON.stringify({ data: message, targetOrigin: targetOrigin || '*' });
      } catch (error) {
        pushConsoleEntry('warn', '[popup:opener-message] could not serialize the message: ' + error);
        return;
      }
      route('opener.ferryx.invalid', 'msg', payload);
    },
    focus: () => {},
    blur: () => {},
    location: {
      get href() {
        return '';
      },
      set href(value) {
        // The host protocol has no "navigate the opener" op, so say so instead of
        // pretending the redirect happened.
        pushConsoleEntry('warn', '[popup:opener-location] window.opener.location is not supported: ' + value);
      },
    },
  };
  try {
    window.opener = openerProxy;
  } catch (assignError) {
    // Some engines expose `opener` as an accessor that refuses plain assignment.
    try {
      Object.defineProperty(window, 'opener', {
        value: openerProxy,
        writable: true,
        configurable: true,
      });
    } catch (defineError) {
      reportBridgeFailure('could not install window.opener (' + assignError + ')', defineError);
    }
  }
  try {
    Object.defineProperty(window, '__ferryxOpenerProxyInstalled', { value: true });
  } catch (error) {
    reportBridgeFailure('could not mark the opener proxy as installed', error);
  }
"#;

/// Builds the guest bridge initialization script for one browser webview.
/// `nonce` is JSON-encoded into the script and appended to every class (b) control
/// URL. It is a frame-confusion filter, not an authorization boundary (see
/// `parse_browser_guest_action`); class (a) requests leave this script only as
/// `ferryx-intent` events for the isolated-world broker.
///
/// `is_popup` is true only for a tab the host created because a page called
/// `window.open`; every other tab must keep `window.opener` null (see
/// [`POPUP_SURFACE_SOURCE`]).
pub fn browser_guest_bridge_script(nonce: &str, is_popup: bool) -> String {
    let nonce_json = serde_json::to_string(nonce).unwrap_or_else(|_| "\"\"".to_string());
    BRIDGE_SCRIPT_TEMPLATE
        .replace(BRIDGE_NONCE_PLACEHOLDER, &nonce_json)
        .replace(
            POPUP_SURFACE_PLACEHOLDER,
            if is_popup { POPUP_SURFACE_SOURCE } else { "" },
        )
        .replace(
            WINDOW_CLOSE_BODY_PLACEHOLDER,
            if is_popup { POPUP_WINDOW_CLOSE_BODY } else { TAB_WINDOW_CLOSE_BODY },
        )
        .replace(
            BRIDGE_HARD_RELOAD_PLACEHOLDER,
            if crate::browser::hard_reload::hard_reload_supported() {
                "true"
            } else {
                "false"
            },
        )
        // Windows pairs a popup intent with WebView2's native NewWindowRequested
        // (IsUserInitiated) when no isolated world is available (design 3.5), so only there
        // does the shim also call the engine's own window.open.
        .replace(
            BRIDGE_NATIVE_OPEN_PLACEHOLDER,
            if cfg!(windows) { "true" } else { "false" },
        )
}

const BRIDGE_SCRIPT_TEMPLATE: &str = r#"
(() => {
  if (window.__ferryxBrowserBridgeInstalled) return;
  Object.defineProperty(window, '__ferryxBrowserBridgeInstalled', { value: true });

  // Captured before the console patch below so a failure inside the capture machinery
  // itself can still be reported without recursing back through the patched console.
  const nativeConsoleWarn = typeof console !== 'undefined' && typeof console.warn === 'function'
    ? console.warn.bind(console)
    : null;

  const consoleRing = (window.__FERRYX_BROWSER_CONSOLE__ = window.__FERRYX_BROWSER_CONSOLE__ || []);
  const pushConsoleEntry = (level, text) => {
    try {
      while (consoleRing.length >= 500) {
        consoleRing.shift();
      }
      consoleRing.push({
        level: String(level),
        text: String(text),
        atMs: Date.now(),
      });
    } catch (error) {
      // The ring itself is broken, so the entry cannot be recorded. Report it on the
      // native console only: the patched console pushes into this same ring, so going
      // through it here would recurse forever.
      if (nativeConsoleWarn) nativeConsoleWarn.call(console, '[ferryx] console capture failed', error);
    }
  };

  // A swallowed failure is invisible to `ferryx browser console`, which is how a broken
  // OAuth callback turns into "nothing happened". Every catch in this bridge therefore
  // reports what failed -- into the ring first, so the CLI can read it -- while still
  // containing the error so the page keeps running.
  const reportBridgeFailure = (what, error) => {
    const detail = error && error.message ? String(error.message) : String(error);
    const text = '[ferryx] ' + what + ': ' + detail;
    try {
      pushConsoleEntry('warn', text);
    } catch (nested) {
      if (nativeConsoleWarn) nativeConsoleWarn.call(console, text, nested);
    }
  };

  ['log', 'info', 'warn', 'error'].forEach((level) => {
    const original = console[level];
    if (typeof original === 'function') {
      console[level] = function (...args) {
        try {
          const text = args.map((arg) => {
            if (arg === null) return 'null';
            if (arg === undefined) return 'undefined';
            if (typeof arg === 'object') {
              try { return JSON.stringify(arg); } catch (_) { return String(arg); }
            }
            return String(arg);
          }).join(' ');
          pushConsoleEntry(level, text);
        } catch (error) {
          reportBridgeFailure('console capture failed', error);
        }
        return original.apply(this, args);
      };
    }
  });

  const originalOnError = window.onerror;
  window.onerror = function (message, source, lineno, colno, error) {
    try {
      const text = error && error.stack ? String(error.stack) : `${message} at ${source}:${lineno}:${colno}`;
      pushConsoleEntry('error', text);
    } catch (error) {
      reportBridgeFailure('window.onerror capture failed', error);
    }
    if (typeof originalOnError === 'function') {
      return originalOnError.apply(this, arguments);
    }
    return false;
  };

  // Host-controlled JS Dialog interception (alert, confirm, prompt)
  const dialogHistory = (window.__FERRYX_BROWSER_DIALOGS__ = window.__FERRYX_BROWSER_DIALOGS__ || []);
  const dialogState = (window.__FERRYX_DIALOG_STATE__ = window.__FERRYX_DIALOG_STATE__ || {
    policy: 'auto-dismiss',
    pending: null,
    nextAction: null,
  });

  const recordDialog = (type, message, defaultValue) => {
    const entry = {
      id: Date.now().toString(36) + Math.random().toString(36).slice(2, 6),
      type: String(type),
      message: String(message),
      defaultValue: defaultValue !== undefined && defaultValue !== null ? String(defaultValue) : null,
      atMs: Date.now(),
      handled: true,
      action: 'dismiss',
      result: null,
    };

    const next = dialogState.nextAction;
    dialogState.nextAction = null;

    if (next && next.action) {
      entry.action = next.action;
      if (next.action === 'accept') {
        if (type === 'confirm') entry.result = 'true';
        else if (type === 'prompt') entry.result = next.promptText !== undefined && next.promptText !== null ? String(next.promptText) : (defaultValue ? String(defaultValue) : '');
        else entry.result = 'ok';
      } else {
        if (type === 'confirm') entry.result = 'false';
        else if (type === 'prompt') entry.result = null;
        else entry.result = 'dismissed';
      }
    } else if (dialogState.policy === 'auto-accept') {
      entry.action = 'accept';
      if (type === 'confirm') entry.result = 'true';
      else if (type === 'prompt') entry.result = defaultValue ? String(defaultValue) : '';
      else entry.result = 'ok';
    } else {
      // default: auto-dismiss policy
      entry.action = 'dismiss';
      if (type === 'confirm') entry.result = 'false';
      else if (type === 'prompt') entry.result = null;
      else entry.result = 'dismissed';
    }

    dialogState.pending = entry;
    dialogHistory.push(entry);
    if (dialogHistory.length > 100) dialogHistory.shift();

    pushConsoleEntry('info', '[dialog:' + type + '] ' + entry.message + ' -> ' + entry.action);

    if (type === 'confirm') return entry.result === 'true';
    if (type === 'prompt') return entry.result;
    return undefined;
  };

  window.alert = function (message) {
    try { recordDialog('alert', message); } catch (error) { reportBridgeFailure('alert interception failed', error); }
  };

  window.confirm = function (message) {
    try { return recordDialog('confirm', message); } catch (_) { return false; }
  };

  window.prompt = function (message, defaultValue) {
    try { return recordDialog('prompt', message, defaultValue); } catch (_) { return null; }
  };

  // This script runs at document start, before any page script, so these
  // references are still pristine. `route` below builds a URL containing the
  // bridge nonce and hands it to `assign`. The nonce is only a frame-confusion
  // filter for class (b) traffic, not an authorization boundary: nothing it guards lets a
  // page do more than it already could.
  const assign = location.assign.bind(location);
  const enc = encodeURIComponent;
  const addDocumentListener = document.addEventListener.bind(document);
  const dispatchDocumentEvent = document.dispatchEvent.bind(document);
  const IntentEvent = CustomEvent;
  const toJson = JSON.stringify;
  // Captured at document start, before page script runs.
  // In Chromium/Gecko, `isTrusted` is an accessor on Event.prototype.
  // In WebKit/WKWebView, `isTrusted` is an own [LegacyUnforgeable] property on each event
  // instance, so Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted') is undefined.
  // We take the prototype getter when present; otherwise we safely read the instance
  // property (which cannot be shadowed or reconfigured when [LegacyUnforgeable]).
  const isTrustedDesc = Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted');
  const isTrustedOf = isTrustedDesc && typeof isTrustedDesc.get === 'function' ? isTrustedDesc.get : null;
  // Only real user input may drive the self-scoped shortcuts and URL drops below.
  const isUserEvent = (event) => {
    try {
      if (!event) return false;
      return isTrustedOf ? isTrustedOf.call(event) === true : event.isTrusted === true;
    } catch (_) {
      return false;
    }
  };

  const resolveHttpUrl = (raw) => {
    if (!raw) return null;
    try {
      const parsed = new URL(String(raw), location.href);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? parsed.href : null;
    } catch (_) {
      return null;
    }
  };

  const routeParams = (host, params) => {
    const pairs = Object.keys(params)
      .map((key) => `${enc(key)}=${enc(params[key])}`)
      .join('&');
    assign(`https://${host}/?${pairs}&nonce=${__FERRYX_BROWSER_BRIDGE_NONCE__}`);
  };
  const route = (host, key, value) => {
    routeParams(host, { [key]: value });
  };
  // Class (a) requests (popup open, popup self-close) are only intents: the isolated-world
  // broker forwards them to the host if it saw a trusted user activation, and the host
  // re-checks them. This page-world script holds no route for them.
  const postIntent = (intent) => {
    try {
      dispatchDocumentEvent(new IntentEvent('ferryx-intent', { detail: toJson(intent) }));
    } catch (error) {
      reportBridgeFailure('could not post a ' + intent.op + ' intent', error);
    }
  };
  // The route must never be reachable from page script; it is kept in this closure.

  // OAuth popups are host-created browser tabs, so a popup's callback has to travel
  // through the host: a same-origin broadcast channel only reaches the same storage partition,
  // so two webviews silently deliver nothing to each other. The popup-only surface is
  // substituted below and only for a tab the host created as a popup: a normal tab must
  // keep `window.opener` null, because real OAuth callback pages branch on it and a fake
  // opener makes them postMessage into a proxy nobody reads and call close() instead of
  // continuing the redirect flow.
__FERRYX_IS_POPUP_SURFACE__

  // Every tab replaces the engine's window.close, because a page must not be able to
  // close or destroy a tab behind the host. A popup tab's replacement asks the host
  // through the broker; any other tab's is a no-op. The property is made read-only and
  // non-configurable so page script cannot put the native close back.
  const ferryxWindowClose = function close() {
__FERRYX_WINDOW_CLOSE_BODY__
  };
  try {
    Object.defineProperty(window, 'close', {
      value: ferryxWindowClose,
      writable: false,
      configurable: false,
      enumerable: true,
    });
  } catch (defineError) {
    // An engine that refuses the redefinition still gets the replacement by assignment.
    try {
      window.close = ferryxWindowClose;
      // This script is sloppy mode, so assigning a non-writable close fails silently.
      if (window.close !== ferryxWindowClose) {
        reportBridgeFailure('could not replace window.close (' + defineError + ')', 'the page kept its own window.close');
      }
    } catch (assignError) {
      reportBridgeFailure('could not replace window.close (' + defineError + ')', assignError);
    }
  }

  // Every tab, popup or not, gets real window.open handles: the opener drives the popup
  // through the handle and the host creates the actual tab. Each handle is registered
  // under a per-page id so the host can address it in both directions.
  const popupHandles = (window.__ferryxPopupHandles = window.__ferryxPopupHandles || {});
  let popupHandleCounter = 0;
  // Handle ids are host-visible keys for pending closes and popup links, and the host
  // keys them per tab. A per-document random prefix keeps a handle from a previous
  // document in this tab from ever matching a handle minted by this one.
  const popupDocumentId = (() => {
    const bytes = new Uint8Array(8);
    crypto.getRandomValues(bytes);
    return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
  })();

  const originalOpen = window.open.bind(window);
  const nativeOpenAlongsideIntent = __FERRYX_BROWSER_BRIDGE_NATIVE_OPEN__;
  const requestOpen = (url, handleId, name) => {
    postIntent({ op: 'open', url, handle: handleId });
    if (nativeOpenAlongsideIntent) {
      try {
        originalOpen(url, typeof name === 'string' ? name : '_blank');
      } catch (error) {
        reportBridgeFailure('native window.open failed', error);
      }
    }
  };
  window.open = (url, name, ...rest) => {
    if (window.__FERRYX_POPUP_POLICY__ === 'block') {
      pushConsoleEntry('warn', '[popup:blocked] Popup blocked by policy: ' + url);
      return null;
    }

    const raw = url === undefined || url === null ? '' : String(url);
    const deferred = !raw.trim() || raw.trim() === 'about:blank';
    const initial = resolveHttpUrl(raw);
    if (!initial && !deferred) {
      pushConsoleEntry('warn', '[popup:rejected] Invalid popup URL scheme: ' + url);
      return null;
    }

    popupHandleCounter += 1;
    const handleId = `${popupDocumentId}-p${popupHandleCounter}`;
    let closed = false;
    let currentUrl = initial || 'about:blank';
    let routed = false;

    const navigate = (nextUrl) => {
      const resolved = resolveHttpUrl(nextUrl);
      if (!resolved) {
        pushConsoleEntry('warn', '[popup:rejected] Invalid popup URL scheme: ' + nextUrl);
        return;
      }
      currentUrl = resolved;
      // The first assignment asks for the popup tab; later ones navigate the tab the host
      // already created for this handle (a no-op host-side if none was created).
      if (routed) {
        routeParams('popup.ferryx.invalid', { url: resolved, handle: handleId, op: 'navigate' });
      } else {
        requestOpen(resolved, handleId, name);
        routed = true;
      }
    };

    const handleLocation = {
      get href() {
        return currentUrl;
      },
      set href(nextUrl) {
        navigate(nextUrl);
      },
      assign: (nextUrl) => navigate(nextUrl),
      replace: (nextUrl) => navigate(nextUrl),
      reload: () => {},
      toString: () => currentUrl,
    };

    const popupHandle = {
      name: typeof name === 'string' ? name : '',
      opener: window,
      location: handleLocation,
      close: () => {
        // The host owns the tab, so ask it to close this linked popup by handleId.
        // `closed` flips when the host confirms through window.__ferryxPopupClosed, which
        // keeps it real state instead of an optimistic guess.
        routeParams('popupclose.ferryx.invalid', { handle: handleId });
      },
      focus: () => {},
      blur: () => {},
      postMessage: (message, targetOrigin) => {
        let payload;
        try {
          payload = JSON.stringify({ data: message, targetOrigin: targetOrigin || '*' });
        } catch (error) {
          pushConsoleEntry('warn', '[popup:handle-message] could not serialize the message: ' + error);
          return;
        }
        routeParams('popup.ferryx.invalid', { op: 'message', handle: handleId, msg: payload });
      },
    };

    Object.defineProperty(popupHandle, 'closed', {
      get: () => closed,
      set: (value) => { closed = Boolean(value); },
    });
    // The host calls this when the popup tab actually closes (by the user, by the page,
    // or by the host), which is the only way `closed` reflects reality.
    popupHandle.__ferryxMarkClosed = () => { closed = true; };
    popupHandles[handleId] = popupHandle;

    if (initial) {
      requestOpen(initial, handleId, name);
      routed = true;
    }
    return popupHandle;
  };

  window.__ferryxPopupClosed = (id) => {
    const handle = popupHandles[id];
    if (!handle) return false;
    if (typeof handle.__ferryxMarkClosed === 'function') handle.__ferryxMarkClosed();
    return true;
  };

  // Anchor clicks (links, downloads) are decided by the isolated-world broker from the
  // trusted click itself; this page-world script no longer handles them.
  const isMac = typeof navigator !== 'undefined' && (
    /Mac|iPhone|iPod|iPad/.test(navigator.platform || '') ||
    /Macintosh/.test(navigator.userAgent || '')
  );

  // A cache-bypassing reload needs a native binding this build only has on macOS and Linux; the
  // host substitutes the platform flag as a literal, so Windows never offers the chord at all.
  const hardReloadSupported = __FERRYX_BROWSER_BRIDGE_HARD_RELOAD__;

  // Only self-scoped shortcuts live here (back, forward, reload, reload-hard): the page can
  // already do each of them itself. App-chrome chords come from native key sources.
  addDocumentListener('keydown', (event) => {
    if (!isUserEvent(event)) return;
    let action = null;
    const composing = event.isComposing || event.keyCode === 229;
    if (composing) return;

    const code = event.code || '';
    const key = event.key ? event.key.toLowerCase() : '';
    const primaryMod = isMac ? (event.metaKey && !event.ctrlKey) : (event.ctrlKey && !event.metaKey);

    // Reload works everywhere, even while typing in text inputs, matching browser conventions.
    if (primaryMod && !event.altKey && (key === 'r' || code === 'KeyR')) {
      if (!event.shiftKey) action = 'reload';
      else if (hardReloadSupported) action = 'reload-hard';
    }

    // In-page navigation outside editable fields
    const keyTarget = event.target;
    const inEditable = keyTarget instanceof Element && (
      keyTarget.tagName === 'INPUT' || keyTarget.tagName === 'TEXTAREA' ||
      keyTarget.tagName === 'SELECT' || keyTarget.isContentEditable
    );

    if (!action && !inEditable) {
      if (primaryMod && key === '[') action = 'back';
      else if (primaryMod && key === ']') action = 'forward';
      else if (event.altKey && !event.metaKey && !event.ctrlKey && event.key === 'ArrowLeft') action = 'back';
      else if (event.altKey && !event.metaKey && !event.ctrlKey && event.key === 'ArrowRight') action = 'forward';
    }

    if (!action) return;
    event.preventDefault();
    route('shortcut.ferryx.invalid', 'action', action);
  }, true);

  addDocumentListener('drop', (event) => {
    if (!isUserEvent(event)) return;
    const uri = event.dataTransfer?.getData('text/uri-list') || event.dataTransfer?.getData('text/plain') || '';
    const target = resolveHttpUrl(uri.split(/\r?\n/).find((line) => line && !line.startsWith('#')) || uri);
    if (!target) return;
    event.preventDefault();
    assign(target);
  }, true);
})();
"#;

pub fn build_console_drain_script(clear: bool, errors_only: bool) -> String {
    format!(
        r#"(() => {{
  const ring = window.__FERRYX_BROWSER_CONSOLE__ || [];
  let entries = ring;
  if ({errors_only}) {{
    entries = ring.filter(e => e.level === "error");
  }}
  if ({clear}) {{
    if ({errors_only}) {{
      window.__FERRYX_BROWSER_CONSOLE__ = ring.filter(e => e.level !== "error");
    }} else {{
      window.__FERRYX_BROWSER_CONSOLE__ = [];
    }}
  }}
  return JSON.stringify(entries);
}})()"#,
        errors_only = errors_only,
        clear = clear,
    )
}

pub fn build_dialog_handle_script(action: &str, prompt_text: Option<&str>) -> String {
    let action_json = serde_json::to_string(action).unwrap_or_else(|_| "\"dismiss\"".into());
    let prompt_json = serde_json::to_string(&prompt_text).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const state = window.__FERRYX_DIALOG_STATE__;
  if (!state) return JSON.stringify({{ error: "no_active_dialog" }});
  const pending = state.pending;
  if (!pending) {{
    return JSON.stringify({{ error: "no_active_dialog" }});
  }}
  pending.action = {action_json};
  if ({action_json} === 'accept') {{
    if (pending.type === 'confirm') pending.result = 'true';
    else if (pending.type === 'prompt') pending.result = {prompt_json};
    else pending.result = 'ok';
  }} else {{
    if (pending.type === 'confirm') pending.result = 'false';
    else if (pending.type === 'prompt') pending.result = null;
    else pending.result = 'dismissed';
  }}
  state.pending = null;
  return JSON.stringify({{ ok: true, dialog: pending }});
}})()"#
    )
}

pub fn build_dialog_arm_script(action: &str, prompt_text: Option<&str>) -> String {
    let action_json = serde_json::to_string(action).unwrap_or_else(|_| "\"dismiss\"".into());
    let prompt_json = serde_json::to_string(&prompt_text).unwrap_or_else(|_| "null".into());
    format!(
        r#"(() => {{
  const state = (window.__FERRYX_DIALOG_STATE__ = window.__FERRYX_DIALOG_STATE__ || {{ policy: 'auto-dismiss', pending: null, nextAction: null }});
  state.nextAction = {{ action: {action_json}, promptText: {prompt_json} }};
  return JSON.stringify({{ ok: true }});
}})()"#
    )
}

pub fn build_dialog_policy_script(policy: &str) -> String {
    let policy_json = serde_json::to_string(policy).unwrap_or_else(|_| "\"auto-dismiss\"".into());
    format!(
        r#"(() => {{
  const state = (window.__FERRYX_DIALOG_STATE__ = window.__FERRYX_DIALOG_STATE__ || {{ policy: 'auto-dismiss', pending: null, nextAction: null }});
  state.policy = {policy_json};
  return JSON.stringify({{ ok: true, policy: state.policy }});
}})()"#
    )
}

pub fn build_dialog_drain_script() -> String {
    r#"(() => {
  const dialogs = window.__FERRYX_BROWSER_DIALOGS__ || [];
  return JSON.stringify(dialogs);
})()"#
        .into()
}

pub fn parse_dialog_entry(raw_json: &str) -> Result<crate::browser::model::BrowserDialogEntry, String> {
    if let Ok(entry) = serde_json::from_str::<crate::browser::model::BrowserDialogEntry>(raw_json) {
        return Ok(entry);
    }
    if let Ok(unquoted) = serde_json::from_str::<String>(raw_json) {
        if let Ok(entry) = serde_json::from_str::<crate::browser::model::BrowserDialogEntry>(&unquoted) {
            return Ok(entry);
        }
    }
    Err("failed to parse browser dialog entry".to_string())
}

pub fn parse_dialog_drain_result(
    raw_json: &str,
) -> Result<Vec<crate::browser::model::BrowserDialogEntry>, String> {
    if let Ok(entries) =
        serde_json::from_str::<Vec<crate::browser::model::BrowserDialogEntry>>(raw_json)
    {
        return Ok(entries);
    }
    if let Ok(unquoted) = serde_json::from_str::<String>(raw_json) {
        if let Ok(entries) =
            serde_json::from_str::<Vec<crate::browser::model::BrowserDialogEntry>>(&unquoted)
        {
            return Ok(entries);
        }
    }
    Err("failed to parse browser dialog entries".to_string())
}

pub fn parse_console_drain_result(
    raw_json: &str,
) -> Result<Vec<crate::browser::model::BrowserConsoleEntry>, String> {
    if let Ok(entries) =
        serde_json::from_str::<Vec<crate::browser::model::BrowserConsoleEntry>>(raw_json)
    {
        return Ok(entries);
    }
    if let Ok(unquoted) = serde_json::from_str::<String>(raw_json) {
        if let Ok(entries) =
            serde_json::from_str::<Vec<crate::browser::model::BrowserConsoleEntry>>(&unquoted)
        {
            return Ok(entries);
        }
    }
    Err("failed to parse console drain entries".to_string())
}

pub fn browser_guest_geometry_observation_script() -> &'static str {
    r#"
    (() => {
      try {
        return JSON.stringify({
          innerWidth: window.innerWidth || 0,
          innerHeight: window.innerHeight || 0,
          scrollX: window.scrollX || 0,
          scrollY: window.scrollY || 0,
          devicePixelRatio: window.devicePixelRatio || 1.0,
        });
      } catch (e) {
        return null;
      }
    })()
    "#
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_NONCE: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn bridge_script_captures_nonce_carrying_builtins_at_document_start() {
        let script = browser_guest_bridge_script(TEST_NONCE, false);

        assert!(script.contains("const assign = location.assign.bind(location);"));
        assert!(script.contains("const enc = encodeURIComponent;"));

        // The routed URL carries the nonce, so it must be built and dispatched
        // through the captured references: resolving them at call time lets a
        // page swap either one in and read the nonce out of the target URL.
        assert!(script.contains("&nonce="));
        assert!(
            !script.contains("encodeURIComponent(value)"),
            "route must encode through the captured reference"
        );
        assert!(
            !script.contains("location.assign(target)"),
            "route must navigate through the captured reference"
        );
    }

    #[test]
    fn parses_bridge_actions_and_ignores_normal_navigation() {
        let shortcut = Url::parse(&format!(
            "https://shortcut.ferryx.invalid/?action=reload&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&shortcut, TEST_NONCE),
            Some(BrowserGuestAction::Shortcut("reload".into()))
        );

        // The hard-reload chord is emitted only where the platform can honour it, so the bridge
        // must carry the resolved platform flag instead of a leftover placeholder.
        let script = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(script.contains("action = 'reload-hard'"));
        assert!(!script.contains(BRIDGE_HARD_RELOAD_PLACEHOLDER));
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(script.contains("const hardReloadSupported = true;"));
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        assert!(script.contains("const hardReloadSupported = false;"));

        assert_eq!(
            parse_browser_guest_action(&Url::parse("https://example.com/").unwrap(), TEST_NONCE),
            None
        );
    }

    #[test]
    fn guest_control_urls_require_matching_nonce() {
        // Correct nonce still parses.
        let back = Url::parse(&format!(
            "https://shortcut.ferryx.invalid/?nonce={TEST_NONCE}&action=back"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&back, TEST_NONCE),
            Some(BrowserGuestAction::Shortcut("back".into()))
        );

        // Wrong nonce is rejected on every control host.
        let forged_shortcut =
            Url::parse("https://shortcut.ferryx.invalid/?action=back&nonce=attacker-guess")
                .unwrap();
        assert_eq!(
            parse_browser_guest_action(&forged_shortcut, TEST_NONCE),
            None
        );

        // Missing nonce (the legacy shape any page can produce) is rejected.
        let legacy_shortcut = Url::parse("https://shortcut.ferryx.invalid/?action=back").unwrap();
        assert_eq!(
            parse_browser_guest_action(&legacy_shortcut, TEST_NONCE),
            None
        );
    }

    #[test]
    fn guest_bridge_covers_popup_intents_self_shortcuts_and_url_drop() {
        let script = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(script.contains("window.open"));
        assert!(script.contains("'ferryx-intent'"));
        assert!(script.contains("shortcut.ferryx.invalid"));
        assert!(script.contains("text/uri-list"));
        assert!(script.contains("action = 'reload'"));
        assert!(script.contains("action = 'back'"));
        // The OAuth callback travels through the host now, not through BroadcastChannel,
        // which is same-origin and same-storage-partition only and therefore delivered
        // nothing between two webviews.
        assert!(!script.contains("__ferryx_oauth_bridge__"));
        assert!(!script.contains("BroadcastChannel"));
        // The nonce is embedded JSON-encoded and appended to every routed URL.
        let nonce_json = serde_json::to_string(TEST_NONCE).unwrap();
        assert!(script.contains(&format!("&nonce=${{{nonce_json}}}")));
        assert!(!script.contains(BRIDGE_NONCE_PLACEHOLDER));
    }

    #[test]
    fn test_console_drain_script_and_parser() {
        let drain_script_all = build_console_drain_script(false, false);
        assert!(drain_script_all.contains("__FERRYX_BROWSER_CONSOLE__"));

        let sample_json = r#"[{"level":"log","text":"hello world","atMs":12345678},{"level":"error","text":"failed","atMs":12345679}]"#;
        let entries = parse_console_drain_result(sample_json).expect("parse console entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "log");
        assert_eq!(entries[0].text, "hello world");
        assert_eq!(entries[0].at_ms, 12345678);
        assert_eq!(entries[1].level, "error");

        let bridge = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(bridge.contains("__FERRYX_BROWSER_CONSOLE__"));
        assert!(bridge.contains("500"));
    }

    #[test]
    fn test_dialog_scripts_and_parser() {
        let handle_script = build_dialog_handle_script("accept", Some("yes"));
        assert!(handle_script.contains("__FERRYX_DIALOG_STATE__"));
        assert!(handle_script.contains("\"accept\""));
        assert!(handle_script.contains("\"yes\""));

        let arm_script = build_dialog_arm_script("accept", Some("ok-prompt"));
        assert!(arm_script.contains("nextAction"));
        assert!(arm_script.contains("\"accept\""));

        let policy_script = build_dialog_policy_script("auto-accept");
        assert!(policy_script.contains("policy = \"auto-accept\""));

        let drain_script = build_dialog_drain_script();
        assert!(drain_script.contains("__FERRYX_BROWSER_DIALOGS__"));

        let sample_entry_json = r#"{"id":"d1","type":"confirm","message":"Leave?","defaultValue":null,"atMs":1234,"handled":true,"action":"accept","result":"true"}"#;
        let entry = parse_dialog_entry(sample_entry_json).expect("parse dialog entry");
        assert_eq!(entry.id, "d1");
        assert_eq!(entry.r#type, "confirm");
        assert_eq!(entry.message, "Leave?");
        assert_eq!(entry.action, "accept");

        let sample_list_json = format!("[{sample_entry_json}]");
        let list = parse_dialog_drain_result(&sample_list_json).expect("parse dialog drain");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "d1");

        let bridge = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(bridge.contains("__FERRYX_BROWSER_DIALOGS__"));
        assert!(bridge.contains("window.alert = function"));
        assert!(bridge.contains("window.confirm = function"));
        assert!(bridge.contains("window.prompt = function"));
    }

    #[test]
    fn parses_popup_navigate_and_opener_message_actions() {
        // `window.open(url)` no longer creates a tab through the page route: op=open moved
        // to the trusted bridge, with or without an explicit op.
        let open = Url::parse(&format!(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fauth.example.com%2Fauthorize&handle=p1&op=open&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(parse_browser_guest_action(&open, TEST_NONCE), None);
        let implicit_open = Url::parse(&format!(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fauth.example.com%2Fauthorize&handle=p2&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(parse_browser_guest_action(&implicit_open, TEST_NONCE), None);

        // A later `handle.location.href = ...` navigates the tab the host already made.
        let navigate = Url::parse(&format!(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fapp.example.com%2Fcallback%3Fcode%3Dabc&handle=p1&op=navigate&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&navigate, TEST_NONCE),
            Some(BrowserGuestAction::PopupNavigate {
                url: "https://app.example.com/callback?code=abc".into(),
                handle: "p1".into(),
            })
        );

        // An unknown op is not guessed into an open.
        let unknown_op = Url::parse(&format!(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fapp.example.com&handle=p1&op=replace&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(parse_browser_guest_action(&unknown_op, TEST_NONCE), None);

        // A popup action without a handle cannot be addressed back to the opener.
        let missing_handle = Url::parse(&format!(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fapp.example.com&op=navigate&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(parse_browser_guest_action(&missing_handle, TEST_NONCE), None);

        // `window.opener.postMessage(data, targetOrigin)` from the popup.
        let message = Url::parse(&format!(
            "https://opener.ferryx.invalid/?msg=%7B%22data%22%3A%7B%22code%22%3A%22abc%22%7D%2C%22targetOrigin%22%3A%22*%22%7D&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&message, TEST_NONCE),
            Some(BrowserGuestAction::OpenerMessage {
                message: r#"{"data":{"code":"abc"},"targetOrigin":"*"}"#.into(),
            })
        );

        // A bare popupclose (the old popup self-close) is refused: it closed any tab.
        let close = Url::parse(&format!(
            "https://popupclose.ferryx.invalid/?nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(parse_browser_guest_action(&close, TEST_NONCE), None);

        // `handle.close()` from the opener carried via popupclose.ferryx.invalid with handle.
        let handle_close = Url::parse(&format!(
            "https://popupclose.ferryx.invalid/?handle=p1&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&handle_close, TEST_NONCE),
            Some(BrowserGuestAction::PopupCloseHandle { handle: "p1".into() })
        );

        // `handle.close()` carried via popup.ferryx.invalid with op=close.
        let op_close = Url::parse(&format!(
            "https://popup.ferryx.invalid/?op=close&handle=p2&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&op_close, TEST_NONCE),
            Some(BrowserGuestAction::PopupCloseHandle { handle: "p2".into() })
        );

        // `handle.postMessage(msg)` from opener to child popup.
        let opener_to_popup = Url::parse(&format!(
            "https://popup.ferryx.invalid/?op=message&handle=p1&msg=%7B%22data%22%3A%22token%22%7D&nonce={TEST_NONCE}"
        ))
        .unwrap();
        assert_eq!(
            parse_browser_guest_action(&opener_to_popup, TEST_NONCE),
            Some(BrowserGuestAction::PopupMessage {
                handle: "p1".into(),
                message: r#"{"data":"token"}"#.into(),
            })
        );
    }

    #[test]
    fn guest_control_urls_require_matching_nonce_for_popup_hosts() {
        // A popup action carrying a forged nonce is refused even though every other
        // parameter is well formed: the nonce is the only thing that stops a page which
        // never received the bridge from driving the opener's tab.
        let forged_open = Url::parse(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fevil.example.com&handle=p1&op=open&nonce=attacker-guess",
        )
        .unwrap();
        assert_eq!(parse_browser_guest_action(&forged_open, TEST_NONCE), None);

        let forged_message = Url::parse(
            "https://opener.ferryx.invalid/?msg=%7B%22data%22%3A%22steal%22%7D&nonce=attacker-guess",
        )
        .unwrap();
        assert_eq!(parse_browser_guest_action(&forged_message, TEST_NONCE), None);

        let forged_close =
            Url::parse("https://popupclose.ferryx.invalid/?nonce=attacker-guess").unwrap();
        assert_eq!(parse_browser_guest_action(&forged_close, TEST_NONCE), None);

        let forged_handle_close =
            Url::parse("https://popupclose.ferryx.invalid/?handle=p1&nonce=attacker-guess").unwrap();
        assert_eq!(parse_browser_guest_action(&forged_handle_close, TEST_NONCE), None);

        let forged_popup_msg =
            Url::parse("https://popup.ferryx.invalid/?op=message&handle=p1&msg=%7B%7D&nonce=attacker-guess").unwrap();
        assert_eq!(parse_browser_guest_action(&forged_popup_msg, TEST_NONCE), None);

        // Missing nonce is the legacy shape any page can produce, so it is refused too.
        let legacy_open = Url::parse(
            "https://popup.ferryx.invalid/?url=https%3A%2F%2Fevil.example.com&handle=p1&op=open",
        )
        .unwrap();
        assert_eq!(parse_browser_guest_action(&legacy_open, TEST_NONCE), None);
        let legacy_close = Url::parse("https://popupclose.ferryx.invalid/").unwrap();
        assert_eq!(parse_browser_guest_action(&legacy_close, TEST_NONCE), None);
        let legacy_message = Url::parse("https://opener.ferryx.invalid/?msg=%7B%7D").unwrap();
        assert_eq!(parse_browser_guest_action(&legacy_message, TEST_NONCE), None);
    }

    #[test]
    fn guest_bridge_installs_the_opener_proxy_only_in_popup_tabs() {
        // A normal tab must keep `window.opener` null: real OAuth callback pages branch on
        // `if (window.opener)`, and a fake opener makes them postMessage into a proxy
        // nobody reads and call close() instead of continuing the redirect flow.
        let normal = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(!normal.contains("__ferryxOpenerProxyInstalled"));
        assert!(!normal.contains("window.opener = openerProxy"));
        assert!(!normal.contains("opener.ferryx.invalid"));
        assert!(!normal.contains("openerProxy"));
        assert!(!normal.contains(POPUP_SURFACE_PLACEHOLDER));

        let popup = browser_guest_bridge_script(TEST_NONCE, true);
        assert!(popup.contains("window.opener = openerProxy"));
        assert!(popup.contains("opener.ferryx.invalid"));
        assert!(popup.contains("postIntent({ op: 'selfClose' })"));
        assert!(popup.contains("window.__ferryxPopupClosed"));
        assert!(!popup.contains(POPUP_SURFACE_PLACEHOLDER));
    }

    /// Body of the bridge's `window.close` replacement, for asserting what a page's
    /// `window.close()` can reach.
    fn window_close_body(script: &str) -> &str {
        let start_marker = "const ferryxWindowClose = function close() {";
        let start = script
            .find(start_marker)
            .expect("the bridge must install its own window.close")
            + start_marker.len();
        let end = start + script[start..].find("\n  };").expect("window.close body must end");
        &script[start..end]
    }

    #[test]
    fn guest_bridge_neuters_window_close_in_non_popup_tabs() {
        // Measured on WebKitGTK: a page's window.close() in a normal tab destroyed the
        // webview behind the manager (wry connects close -> destroy), leaving a zombie tab.
        let normal = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(normal.contains(
            "Object.defineProperty(window, 'close', {\n      value: ferryxWindowClose,\n      writable: false,\n      configurable: false,"
        ));
        let body = window_close_body(&normal);
        assert!(body.contains("pushConsoleEntry('warn'"), "a blocked close should say so");
        for forbidden in ["postIntent", "route", "assign", "selfClose", "popupclose", "originalClose"] {
            assert!(!body.contains(forbidden), "non-popup window.close must route nothing: {forbidden}");
        }
        assert!(!normal.contains("op: 'selfClose'"));
    }

    #[test]
    fn guest_bridge_popup_window_close_keeps_the_self_close_intent() {
        let popup = browser_guest_bridge_script(TEST_NONCE, true);
        assert!(popup.contains(
            "Object.defineProperty(window, 'close', {\n      value: ferryxWindowClose,\n      writable: false,\n      configurable: false,"
        ));
        let body = window_close_body(&popup);
        assert!(body.contains("postIntent({ op: 'selfClose' })"));
        assert!(!body.contains("route"), "the self-close stays a broker intent, never a page route");
    }

    #[test]
    fn guest_bridge_routes_popup_handles_and_real_closed_state() {
        let script = browser_guest_bridge_script(TEST_NONCE, false);
        // Handles live under a per-page registry the host can address by id.
        assert!(script.contains("window.__ferryxPopupHandles"));
        // `window.open('')` + `w.location.href = url` is the common OAuth pattern, so the
        // handle's location must route instead of only mutating a field.
        assert!(script.contains("const handleLocation = {"));
        assert!(script.contains("assign: (nextUrl) => navigate(nextUrl)"));
        assert!(script.contains("replace: (nextUrl) => navigate(nextUrl)"));
        assert!(script.contains("postIntent({ op: 'open', url, handle: handleId })"));
        assert!(script.contains("op: 'navigate'"));
        assert!(script.contains("popup.ferryx.invalid"));
        // `closed` is host-confirmed state, not an optimistic guess flipped by close().
        assert!(script.contains("popupHandle.__ferryxMarkClosed"));
        assert!(script.contains("window.__ferryxPopupClosed = (id) => {"));
        // A handle's close() asks the host with handleId, which owns the tab and links.
        assert!(script.contains("routeParams('popupclose.ferryx.invalid', { handle: handleId })"));
        assert!(script.contains("op: 'message'"));
    }

    #[test]
    fn guest_bridge_never_swallows_a_failure_silently() {
        // An empty catch hides exactly the OAuth failure this lane exists to surface: the
        // callback routes into nothing and the page sees no error at all.
        for is_popup in [false, true] {
            let script = browser_guest_bridge_script(TEST_NONCE, is_popup);
            assert!(
                !script.contains("catch (_) {}"),
                "the bridge must report what failed instead of dropping it"
            );
            assert!(script.contains("reportBridgeFailure"));
        }
    }

    #[test]
    fn guest_bridge_is_trusted_handles_wkwebview_instance_unforgeable_property() {
        // RED mutation: revert to unguarded `Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted').get;`
        // or remove the `event.isTrusted === true` instance fallback.
        assert!(
            !BRIDGE_SCRIPT_TEMPLATE.contains("Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted').get;"),
            "bridge template must not unguardedly index .get on prototype descriptor because WKWebView lacks it"
        );
        assert!(
            BRIDGE_SCRIPT_TEMPLATE.contains("event.isTrusted === true"),
            "bridge template must fall back to reading instance isTrusted property"
        );

        for is_popup in [false, true] {
            let script = browser_guest_bridge_script(TEST_NONCE, is_popup);
            assert!(
                !script.contains("Object.getOwnPropertyDescriptor(Event.prototype, 'isTrusted').get;"),
                "instantiated script must not contain unguarded prototype getter"
            );
            assert!(
                script.contains("event.isTrusted === true"),
                "instantiated script must contain instance isTrusted fallback"
            );
        }
    }

    #[test]
    fn guest_bridge_console_entry_emits_only_at_ms_camel_case() {
        // RED mutation: re-add `at_ms:` to BRIDGE_SCRIPT_TEMPLATE or serialize both keys into the console ring.
        assert!(
            !BRIDGE_SCRIPT_TEMPLATE.contains("at_ms:"),
            "bridge template must never emit at_ms: alongside atMs, preventing serde duplicate field error"
        );

        let parsed = parse_console_drain_result(r#"[{"level":"log","text":"x","atMs":1}]"#)
            .expect("console drain parser must succeed with canonical atMs single key");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].level, "log");
        assert_eq!(parsed[0].text, "x");
        assert_eq!(parsed[0].at_ms, 1);
    }

    #[test]
    fn guest_bridge_never_exposes_the_nonce_router_on_window() {
        // A-CORE-1: the router stays in the bridge closure. The nonce is only a
        // frame-confusion filter now, but there is still no reason to publish it.
        for is_popup in [false, true] {
            let script = browser_guest_bridge_script(TEST_NONCE, is_popup);
            assert!(!script.contains("__ferryxRoute"), "the nonce router must not be published");
            for line in script.lines() {
                let trimmed = line.trim();
                let assigns_router = trimmed.contains("= route;")
                    || trimmed.contains("= routeParams;")
                    || trimmed.contains("= postIntent;")
                    || trimmed.contains("value: route,")
                    || trimmed.contains("value: routeParams,")
                    || trimmed.contains("value: postIntent,");
                assert!(!assigns_router, "bridge must not hand the router to page script: {trimmed}");
            }
            assert!(!script.contains("__ferryxPickReady"));
        }
    }

    #[test]
    fn native_open_alongside_intent_is_substituted_per_platform() {
        let script = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(!script.contains(BRIDGE_NATIVE_OPEN_PLACEHOLDER));
        #[cfg(windows)]
        assert!(script.contains("const nativeOpenAlongsideIntent = true;"));
        #[cfg(not(windows))]
        assert!(script.contains("const nativeOpenAlongsideIntent = false;"));
        assert!(script.contains("if (nativeOpenAlongsideIntent) {"));
    }

    #[test]
    fn guest_bridge_popup_handles_are_unique_per_document() {
        // A-CORE-2: handle ids restarted at p1 on every document load, so a stale pending
        // close or link recorded for the previous document matched the new document's p1.
        let script = browser_guest_bridge_script(TEST_NONCE, false);
        assert!(!script.contains("const handleId = `p${popupHandleCounter}`;"));
        assert!(script.contains("getRandomValues"));
        assert!(script.contains("const handleId = `${popupDocumentId}-p${popupHandleCounter}`;"));
    }

    /// Class (a) routes removed by the trusted-bridge redesign (REVIEW M2). A page that
    /// learns the nonce must not be able to drive any of them by navigation.
    const REMOVED_CLASS_A_URLS: [&str; 9] = [
        "https://open.ferryx.invalid/?url=https%3A%2F%2Fevil.example",
        "https://download.ferryx.invalid/?url=https%3A%2F%2Fevil.example%2Ff.bin",
        "https://pick.ferryx.invalid/?ready=1",
        "https://click.ferryx.invalid/?url=https%3A%2F%2Fevil.example",
        "https://modclick.ferryx.invalid/?url=https%3A%2F%2Fevil.example",
        "https://popup.ferryx.invalid/?url=https%3A%2F%2Fevil.example&handle=p1&op=open",
        "https://popup.ferryx.invalid/?url=https%3A%2F%2Fevil.example&handle=p1",
        "https://popupclose.ferryx.invalid/?x=1",
        "https://shortcut.ferryx.invalid/?action=tab-close",
    ];

    #[test]
    fn removed_class_a_routes_are_refused_even_with_the_correct_nonce() {
        for raw in REMOVED_CLASS_A_URLS {
            let url = Url::parse(&format!("{raw}&nonce={TEST_NONCE}")).unwrap();
            assert_eq!(
                parse_browser_guest_action(&url, TEST_NONCE),
                None,
                "removed class (a) route must not parse: {raw}"
            );
        }
    }

    #[test]
    fn page_shortcut_route_carries_only_self_scoped_actions() {
        for action in ["back", "forward", "reload", "reload-hard"] {
            let url = Url::parse(&format!(
                "https://shortcut.ferryx.invalid/?action={action}&nonce={TEST_NONCE}"
            ))
            .unwrap();
            assert_eq!(
                parse_browser_guest_action(&url, TEST_NONCE),
                Some(BrowserGuestAction::Shortcut(action.into()))
            );
        }
        for action in [
            "tab-close",
            "tab-new-browser",
            "command-palette",
            "settings-toggle",
            "split-right",
            "workspace-select-1",
            "tab-select-1",
            "focus-address",
            "find",
        ] {
            let url = Url::parse(&format!(
                "https://shortcut.ferryx.invalid/?action={action}&nonce={TEST_NONCE}"
            ))
            .unwrap();
            assert_eq!(
                parse_browser_guest_action(&url, TEST_NONCE),
                None,
                "app-chrome action {action} must come from a native key source"
            );
        }
    }

    #[test]
    fn bridge_script_contains_no_class_a_host_or_pick_ready_hook() {
        for is_popup in [false, true] {
            let script = browser_guest_bridge_script(TEST_NONCE, is_popup);
            for host in [
                "open.ferryx.invalid",
                "download.ferryx.invalid",
                "pick.ferryx.invalid",
                "click.ferryx.invalid",
                "modclick.ferryx.invalid",
            ] {
                assert!(!script.contains(&format!("'{host}'")), "{host} must be gone");
            }
            assert!(!script.contains("__ferryxPickReady"));
            // No op=open route parameter: popup open is only a broker intent now.
            assert!(!script.contains("handle: handleId, op: 'open'"));
            assert!(!script.contains("routeParams('popup.ferryx.invalid', { url: initial"));
            assert!(!script.contains("routeHost('popupclose.ferryx.invalid')"));
            for action in ["tab-close", "tab-new-browser", "command-palette", "workspace-select-"] {
                assert!(!script.contains(&format!("'{action}")), "{action} must not be page-sourced");
            }
        }
    }
}
