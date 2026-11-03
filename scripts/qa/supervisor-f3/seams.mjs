// Pure seams of the F3 run, kept apart so the harness self-tests exercise them with no host,
// device or daemon: daemon reply checking, the run-owned gateway port, the iPhone page-script
// channel, and the checks behind the session-row, rules-line and phone-answer steps.
import { StepFailed } from './steps.mjs';

/**
 * The last reply of one daemon exchange, refusing anything the daemon did not accept: a
 * structured `error` or `protocolMismatch`, an unparsed line, no reply, or a reply of another
 * type than `expect`. Setup never continues past a refused request.
 */
export function acceptedReply(label, replies, expect) {
  const last = Array.isArray(replies) ? replies.at(-1) : undefined;
  if (!last || typeof last !== 'object') throw new StepFailed('DAEMON_NO_REPLY', `${label}: no reply`);
  if (last.type === 'error' || last.type === 'protocolMismatch' || last.unparsed !== undefined) {
    throw new StepFailed('DAEMON_REFUSED', `${label}: ${JSON.stringify(last).slice(0, 1500)}`);
  }
  if (expect && last.type !== expect) {
    throw new StepFailed('DAEMON_UNEXPECTED_REPLY', `${label}: expected ${expect}, got ${JSON.stringify(last).slice(0, 1500)}`);
  }
  return last;
}

/** The production gateway port; an isolated QA daemon is never configured onto it. */
export const PRODUCTION_GATEWAY_PORT = 43821;

/** The run-owned port a host helper reserved: unprivileged and never the production port. */
export function gatewayPortFor(lane, reported) {
  const port = reported?.port;
  if (!Number.isSafeInteger(port) || port < 1024 || port > 65535 || port === PRODUCTION_GATEWAY_PORT) {
    throw new StepFailed('GATEWAY_PORT', `${lane}: no usable run-owned port: ${JSON.stringify(reported)}`);
  }
  return port;
}

/**
 * The daemon's own status after `remoteConfigure`: running and bound to exactly the run's port.
 * A daemon that bound nothing, or another port, fails the lane.
 */
export function gatewayBound(label, reply, port) {
  const { status } = acceptedReply(label, [reply], 'remoteStatusOk');
  const bound = typeof status?.boundAddress === 'string' ? Number(status.boundAddress.split(':').pop()) : null;
  if (status?.isRunning !== true || status.port !== port || bound !== port) {
    throw new StepFailed('GATEWAY_NOT_BOUND', `${label}: wanted port ${port}, daemon reports ${JSON.stringify(status).slice(0, 800)}`);
  }
  return status;
}

/**
 * Configures this run's relay gateway on its own port and proves the bind from the daemon's own
 * status. `call(label, frames)` returns the raw replies of one exchange. A refused configure (the
 * port could not be bound, the relay was rejected) stops here: the status read is never reached.
 */
export async function configureGateway({ call, port, relayOrigin }) {
  acceptedReply('relay', await call('relay', [{ type: 'remoteConfigure', config: { mode: 'relay', port, allowControl: true, relayUrl: relayOrigin } }]), 'remoteConfigureOk');
  const replies = await call('relay-status', [{ type: 'remoteGetStatus' }]);
  return gatewayBound('relay-status', Array.isArray(replies) ? replies.at(-1) : undefined, port);
}

/**
 * Safari's web context from a `GET /session/:id/contexts` answer. Raw WebDriverAgent has neither
 * that route nor `execute/sync`; page reads exist only in an Appium XCUITest session switched
 * into a WEBVIEW context, so anything else is refused, never faked.
 */
export function webContextFrom(contexts) {
  if (!Array.isArray(contexts)) {
    throw new StepFailed('WEB_CONTEXT_UNSUPPORTED', `the driver lists no contexts (raw WebDriverAgent has no page-script route): ${String(JSON.stringify(contexts)).slice(0, 300)}`);
  }
  const web = contexts.map((entry) => (typeof entry === 'string' ? entry : entry?.id)).filter((id) => typeof id === 'string' && /^WEBVIEW_/.test(id));
  if (web.length === 0) throw new StepFailed('WEB_CONTEXT_ABSENT', `no WEBVIEW context in ${JSON.stringify(contexts).slice(0, 300)}`);
  return web.at(-1);
}

/**
 * The page channel on the iPhone, over the driver's bounded `request(method, suffix, body)` and
 * its `sessionPath`. `open()` lists the contexts and enters Safari's web context; `evaluate`
 * runs a READ-ONLY expression there; `native()` returns to NATIVE_APP before touches, source
 * and screenshots. There is no fallback to the raw-WDA `execute/sync` route.
 */
export function webPageChannel({ request, sessionPath }) {
  let current = null;
  let web = null;
  const use = async (context) => {
    if (current === context) return;
    await request('POST', sessionPath('/context'), { name: context });
    current = context;
  };
  return {
    async open() {
      let contexts;
      try {
        contexts = await request('GET', sessionPath('/contexts'));
      } catch (error) {
        throw new StepFailed('WEB_CONTEXT_UNSUPPORTED', `GET contexts refused (raw WebDriverAgent has no page-script route): ${error.message}`);
      }
      web = webContextFrom(contexts);
      await use(web);
      return web;
    },
    async evaluate(expression) {
      if (!web) throw new StepFailed('WEB_CONTEXT_CLOSED', 'evaluate before open()');
      await use(web);
      return request('POST', sessionPath('/execute/sync'), { script: `return (${expression});`, args: [] });
    },
    native: () => use('NATIVE_APP'),
    get context() { return current; },
  };
}

/** The decisions a panel read lists whose prompt names the fixture prompt with this nonce. */
export const decisionsNamingNonce = (view, title, nonce) =>
  (view?.decisions ?? []).filter((d) => namesNonce(`${d.prompt?.command ?? ''} ${d.prompt?.question ?? ''}`, title, nonce));

/** This computer's session titled exactly `title`; null when absent, throws when ambiguous. */
export function uniqueSession(view, title, hostLabel = 'This computer') {
  const rows = (view?.sessions ?? []).filter((s) => s.title === title && s.hostLabel === hostLabel);
  if (rows.length > 1) throw new StepFailed('ROW_AMBIGUOUS', `${rows.length} sessions titled ${title}`);
  return rows[0] ?? null;
}

/** Whether `text` names the fixture prompt with exactly this nonce (not a longer number). */
export const namesNonce = (text, title, nonce) => new RegExp(`prod\\.example\\.com/${title}${nonce}(?!\\d)`).test(String(text ?? ''));

/** The newest upstream-retry rule that applies to every session. */
export const globalRetryRule = (view) => (view?.rules ?? [])
  .filter((r) => r.kind === 'upstreamRetry' && !r.scope?.repo && !r.scope?.hostId && !r.scope?.agent)
  .sort((a, b) => b.id - a.id)[0] ?? null;

/**
 * Whether the fixture pane received `token` as typed keys: the focused-terminal proof. Typing
 * may arrive one key per chunk, so the received bytes are joined in arrival order.
 */
export const typedInto = (entries, token) =>
  entries.filter((e) => e.kind === 'received').map((e) => Buffer.from(e.hex ?? '', 'hex').toString('utf8')).join('').includes(token);

/**
 * F3-D6 verdict from two instrumented panes' logs. The probe, typed after a click on the away
 * row, must have arrived in full in the away pane (so focus was provably elsewhere before the
 * focus-row click) and never in the focus pane; the token, typed after the focus-row click,
 * must have arrived in the focus pane. A pane that already held focus cannot pass.
 */
export const rowFocusVerdict = ({ away, focus, probe, token }) => ({
  focusWasElsewhere: typedInto(away, probe) && !typedInto(focus, probe),
  clickFocusedTerminal: typedInto(focus, token) && !typedInto(away, token),
});

/**
 * The rules line as OCR read it: from the line that opens the sentence to the line that holds
 * its "Change" control, joined. null when the frame shows no rules line.
 */
export function rulesLineText(ocrText) {
  const lines = String(ocrText ?? '').split('\n');
  const start = lines.findIndex((line) => /^\s*(Retries|Restarts|Resumes|Asks|No standing rules)\b/i.test(line));
  if (start < 0) return null;
  const end = lines.findIndex((line, i) => i >= start && /\bChange\b/.test(line));
  if (end < 0) return null;
  return lines.slice(start, end + 1).join(' ').replace(/\s*\bChange\b\s*$/, '').replace(/\s+/g, ' ').trim();
}

/** The Safari web area's frame in the accessibility tree (points), or null. */
export const webViewFrame = (nodes) => nodes.find((n) => n.cls === 'XCUIElementTypeWebView' && n.w > 0 && n.h > 0)?.bounds ?? null;

/**
 * A page rect (CSS px, relative to the visual viewport origin) as a touch point in screen
 * points: the web area's origin plus the rect centre. WDA source and W3C actions are both in
 * points and Safari at scale 1 maps one CSS px to one point, so no devicePixelRatio is applied;
 * a pinch-zoomed page (visualViewport.scale != 1) or a point outside the web area is refused.
 */
export function pageToScreen(rect, webView, viewport) {
  if (!webView) throw new StepFailed('WEBVIEW_FRAME_ABSENT', 'the accessibility tree exposes no Safari web area');
  if (viewport?.scale !== undefined && viewport.scale !== 1) throw new StepFailed('PAGE_ZOOMED', `visualViewport.scale ${viewport.scale}`);
  if (!rect || ![rect.x, rect.y, rect.w, rect.h].every(Number.isFinite)) throw new StepFailed('PAGE_RECT_ABSENT', JSON.stringify(rect));
  // getBoundingClientRect is relative to the layout viewport; the web area shows the visual one.
  const x = Math.round(webView.x1 + rect.x - (viewport?.vvLeft ?? 0) + rect.w / 2);
  const y = Math.round(webView.y1 + rect.y - (viewport?.vvTop ?? 0) + rect.h / 2);
  if (x <= webView.x1 || x >= webView.x2 || y <= webView.y1 || y >= webView.y2) {
    throw new StepFailed('TOUCH_OUTSIDE_WEBVIEW', `(${x},${y}) is outside the web area ${JSON.stringify(webView)}`);
  }
  return { x, y };
}
