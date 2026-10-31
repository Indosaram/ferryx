// Isolated-world broker (design 3.4), loaded by bridge/mod.rs via include_str!.
// `hooks` is the literal null in production; ui/src/lib/bridgeBroker.test.ts rewrites only
// that one declaration to inject a channel stub and an isTrusted reader under jsdom.
(() => {
  const hooks = null;
  if (globalThis.__ferryxWorld) return;
  // Top frame only. WebView2 registers this as a CDP new-document script, which runs in
  // every frame, and its binding is visible in every frame's ferryx-bridge world, but the
  // host accepts posts only from the main frame. A subframe broker would cancel clicks the
  // host then drops, so it installs nothing and in-frame links and downloads stay native.
  // macOS and Linux already inject into the top frame only. Reading window.top is treated
  // as "not the top frame" if it ever throws.
  let topFrame = false;
  try {
    topFrame = window.top === window;
  } catch (error) {
    topFrame = false;
  }
  if (!topFrame) return;
  const ACTIVATION_WINDOW_MS = 1000;
  let lastActivationMs = -Infinity;
  // One physical click delivers pointerup and then click (or auxclick). When a popup
  // already spent the pointerup's activation, the click that completes the same gesture
  // must not stamp a fresh one, or one click would admit two popups.
  let activationType = null;
  let spentPointerGesture = false;
  const isTrusted = (event) => (hooks ? hooks.isTrusted(event) : event.isTrusted === true);
  // The world-scoped channel, or null when this document has none. Callers that cancel a
  // native default must check this first: a missing channel falls back to native behaviour.
  // A downgraded WebView2 tab sets __ferryxBridgeDisabled in this world, because
  // Runtime.removeBinding leaves the __ferryxTrusted function in place.
  const channel = () => {
    if (hooks) return hooks.channel();
    if (globalThis.__ferryxBridgeDisabled === true) return null;
    const handler = typeof webkit === 'object' && webkit && webkit.messageHandlers
      ? webkit.messageHandlers.ferryxTrusted
      : null;
    if (handler && typeof handler.postMessage === 'function') {
      return (body) => handler.postMessage(body);
    }
    if (typeof __ferryxTrusted === 'function') {
      return (body) => __ferryxTrusted(body);
    }
    return null;
  };
  const post = (envelope) => {
    const send = channel();
    if (!send) {
      console.warn('[ferryx] the trusted bridge channel is unavailable in this document');
      return false;
    }
    send(JSON.stringify(envelope));
    return true;
  };
  const activationAgeMs = () => performance.now() - lastActivationMs;
  const stamp = (event) => {
    if (!isTrusted(event)) return;
    // A held key auto-repeats keydown; only the first press is a user activation.
    if (event.type === 'keydown' && event.repeat === true) return;
    const completesSpentGesture = spentPointerGesture && (event.type === 'click' || event.type === 'auxclick');
    spentPointerGesture = false;
    if (completesSpentGesture) return;
    lastActivationMs = performance.now();
    activationType = event.type;
  };
  ['click', 'auxclick', 'keydown', 'pointerup', 'drop'].forEach((type) => {
    window.addEventListener(type, stamp, true);
  });
  const resolveHttpUrl = (raw) => {
    if (!raw) return null;
    try {
      const parsed = new URL(String(raw), location.href);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? parsed.href : null;
    } catch (error) {
      console.warn('[ferryx] could not resolve a URL', error);
      return null;
    }
  };
  const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '') || /Macintosh/.test(navigator.userAgent || '');
  const api = { picker: null, post, activationAgeMs, isTrusted, channel };
  // A link click to a fragment of the current document stays a native in-page jump.
  const isSameDocumentFragment = (url) => {
    try {
      const next = new URL(url);
      const here = new URL(location.href);
      next.hash = '';
      here.hash = '';
      return url.includes('#') && next.href === here.href;
    } catch (error) {
      return false;
    }
  };
  // The capture phase only records a trusted link click. The decision waits for the
  // bubble phase at window, after the page's own handlers ran, so a page that cancels the
  // click (a client-side router, a confirm dialog) keeps it, and the host is not asked to
  // navigate as well.
  let pendingLink = null;
  document.addEventListener('click', (event) => {
    pendingLink = null;
    if (!isTrusted(event)) return;
    if (api.picker && api.picker.armed) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const anchor = target.closest('a[href]');
    if (!(anchor instanceof HTMLAnchorElement)) return;
    const url = resolveHttpUrl(anchor.href);
    if (!url) return;
    const download = anchor.hasAttribute('download');
    if (!download && isSameDocumentFragment(url)) return;
    const modifier = isMac ? (event.metaKey && !event.ctrlKey) : (event.ctrlKey && !event.metaKey);
    const blank = (anchor.target || '').toLowerCase() === '_blank';
    pendingLink = { event, url, download, modifier: Boolean(modifier || blank) };
  }, true);
  window.addEventListener('click', (event) => {
    const pending = pendingLink;
    if (!pending || pending.event !== event) return;
    pendingLink = null;
    // The page cancelled the click itself: it owns the outcome.
    if (event.defaultPrevented) return;
    // No channel: leave the event alone so native navigation or download proceeds.
    const send = channel();
    if (!send) return;
    event.preventDefault();
    if (pending.download) {
      send(JSON.stringify({ v: 1, kind: 'download', url: pending.url }));
      return;
    }
    send(JSON.stringify({ v: 1, kind: 'link', url: pending.url, modifier: pending.modifier }));
  }, false);
  document.addEventListener('ferryx-intent', (event) => {
    let intent;
    try {
      intent = JSON.parse(String(event.detail));
    } catch (error) {
      console.warn('[ferryx] ignored a malformed intent', error);
      return;
    }
    if (!intent || typeof intent !== 'object') return;
    if (intent.op === 'open') {
      const age = activationAgeMs();
      if (!(age <= ACTIVATION_WINDOW_MS)) {
        console.warn('[popup:blocked] window.open needs a user click within 1000 ms: ' + intent.url);
        return;
      }
      const url = resolveHttpUrl(intent.url);
      if (!url || typeof intent.handle !== 'string') return;
      if (post({ v: 1, kind: 'popupOpen', url, handle: intent.handle, activationAgeMs: age })) {
        // One activation admits one popup.
        lastActivationMs = -Infinity;
        spentPointerGesture = activationType === 'pointerup';
      }
    } else if (intent.op === 'selfClose') {
      post({ v: 1, kind: 'popupSelfClose' });
    }
  }, true);
  Object.defineProperty(globalThis, '__ferryxWorld', { value: api, writable: false, configurable: false });
})();
