// Self-tests of the F3 harness seams. Pure: no host, device, daemon or network.
import { describe, expect, test } from 'bun:test';
import {
  PRODUCTION_GATEWAY_PORT, acceptedReply, configureGateway, decisionsNamingNonce, gatewayBound, gatewayPortFor,
  globalRetryRule, namesNonce, pageToScreen, rowFocusVerdict, rulesLineText, typedInto, uniqueSession, webContextFrom, webPageChannel,
} from './seams.mjs';
import { SCENARIOS, validateRun } from './contract.mjs';

const code = (fn) => { try { fn(); } catch (error) { return error.code; } return null; };
const codeAsync = async (fn) => { try { await fn(); } catch (error) { return error.code; } return null; };

describe('daemon replies', () => {
  test('a structured error, a protocol mismatch, no reply or another type stops the lane', () => {
    expect(code(() => acceptedReply('relay', [{ type: 'handshakeOk' }, { type: 'error', message: 'Failed to bind to 127.0.0.1:43821: address in use' }]))).toBe('DAEMON_REFUSED');
    expect(code(() => acceptedReply('x', [{ type: 'protocolMismatch' }]))).toBe('DAEMON_REFUSED');
    expect(code(() => acceptedReply('x', [{ unparsed: 'garbage' }]))).toBe('DAEMON_REFUSED');
    expect(code(() => acceptedReply('x', []))).toBe('DAEMON_NO_REPLY');
    expect(code(() => acceptedReply('x', [{ type: 'registerWorkspaceOk' }], 'remoteConfigureOk'))).toBe('DAEMON_UNEXPECTED_REPLY');
    expect(acceptedReply('x', [{ type: 'handshakeOk' }, { type: 'remoteConfigureOk' }], 'remoteConfigureOk').type).toBe('remoteConfigureOk');
  });
});

describe('run-owned gateway port', () => {
  test('the production port, a privileged port or a missing port is refused', () => {
    expect(code(() => gatewayPortFor('maho-mac', { port: PRODUCTION_GATEWAY_PORT }))).toBe('GATEWAY_PORT');
    expect(code(() => gatewayPortFor('maho-mac', { port: 80 }))).toBe('GATEWAY_PORT');
    expect(code(() => gatewayPortFor('maho-mac', {}))).toBe('GATEWAY_PORT');
    expect(gatewayPortFor('maho-mac', { port: 51234 })).toBe(51234);
  });

  const running = (port, bound = `127.0.0.1:${port}`) => ({ type: 'remoteStatusOk', status: { isRunning: true, port, boundAddress: bound } });

  test('a refused configure never reaches the status read', async () => {
    const asked = [];
    const call = async (label, frames) => {
      asked.push(label);
      return label === 'relay' ? [{ type: 'handshakeOk' }, { type: 'error', message: 'Failed to bind to 127.0.0.1:51234: address in use' }] : [running(51234)];
    };
    expect(await codeAsync(() => configureGateway({ call, port: 51234, relayOrigin: 'https://r.example:1' }))).toBe('DAEMON_REFUSED');
    expect(asked).toEqual(['relay']);
  });

  test('a configure that left the gateway down or on another port fails', async () => {
    const with_ = (status) => async (label) => (label === 'relay' ? [{ type: 'remoteConfigureOk' }] : [status]);
    expect(await codeAsync(() => configureGateway({ call: with_({ type: 'remoteStatusOk', status: { isRunning: false, port: 51234, boundAddress: null } }), port: 51234, relayOrigin: 'https://r' }))).toBe('GATEWAY_NOT_BOUND');
    expect(await codeAsync(() => configureGateway({ call: with_(running(43821)), port: 51234, relayOrigin: 'https://r' }))).toBe('GATEWAY_NOT_BOUND');
    expect(await codeAsync(() => configureGateway({ call: with_({ type: 'error', message: 'down' }), port: 51234, relayOrigin: 'https://r' }))).toBe('DAEMON_REFUSED');
  });

  test('the frames ask for the run port, and a proven bind passes', async () => {
    const frames = [];
    const call = async (label, sent) => { frames.push(...sent); return label === 'relay' ? [{ type: 'remoteConfigureOk' }] : [running(51234)]; };
    const status = await configureGateway({ call, port: 51234, relayOrigin: 'https://r.example:1' });
    expect(frames[0].config.port).toBe(51234);
    expect(frames[1].type).toBe('remoteGetStatus');
    expect(status.port).toBe(51234);
    expect(code(() => gatewayBound('s', running(51234, '[::]:51234'), 51234))).toBe(null);
  });
});

describe('iPhone page channel', () => {
  const driver = (routes) => {
    const calls = [];
    const request = async (method, suffix, body) => {
      calls.push(`${method} ${suffix}`);
      const route = routes[`${method} ${suffix.replace(/^\/session\/s1/, '')}`];
      if (route instanceof Error) throw route;
      return typeof route === 'function' ? route(body) : route;
    };
    return { calls, request, sessionPath: (suffix) => `/session/s1${suffix}` };
  };

  test('raw WebDriverAgent (no contexts route) is refused before any execute/sync call', async () => {
    const raw = driver({ 'GET /contexts': new Error('wda-http-error 404 unknown command') });
    const page = webPageChannel(raw);
    expect(await codeAsync(() => page.open())).toBe('WEB_CONTEXT_UNSUPPORTED');
    expect(await codeAsync(() => page.evaluate('1'))).toBe('WEB_CONTEXT_CLOSED');
    expect(raw.calls.some((c) => c.endsWith('/execute/sync'))).toBe(false);
  });

  test('a session with only NATIVE_APP has no page channel', async () => {
    expect(code(() => webContextFrom(['NATIVE_APP']))).toBe('WEB_CONTEXT_ABSENT');
    expect(code(() => webContextFrom(null))).toBe('WEB_CONTEXT_UNSUPPORTED');
    expect(webContextFrom(['NATIVE_APP', { id: 'WEBVIEW_123.1' }])).toBe('WEBVIEW_123.1');
  });

  test('reads run in the web context, native calls switch back first', async () => {
    const appium = driver({ 'GET /contexts': ['NATIVE_APP', 'WEBVIEW_9.1'], 'POST /context': null, 'POST /execute/sync': (body) => ({ script: body.script }) });
    const page = webPageChannel(appium);
    expect(await page.open()).toBe('WEBVIEW_9.1');
    expect((await page.evaluate('document.title')).script).toBe('return (document.title);');
    await page.native();
    await page.evaluate('1');
    expect(appium.calls).toEqual([
      'GET /session/s1/contexts', 'POST /session/s1/context', 'POST /session/s1/execute/sync',
      'POST /session/s1/context', 'POST /session/s1/context', 'POST /session/s1/execute/sync',
    ]);
  });

  test('a page rect maps onto the web area in points, never outside it or on a zoomed page', () => {
    const web = { x1: 0, y1: 100, x2: 390, y2: 760 };
    expect(pageToScreen({ x: 20, y: 300, w: 100, h: 44 }, web, { vvTop: 0, vvLeft: 0, scale: 1 })).toEqual({ x: 70, y: 422 });
    expect(pageToScreen({ x: 20, y: 300, w: 100, h: 44 }, web, { vvTop: 50, vvLeft: 0, scale: 1 })).toEqual({ x: 70, y: 372 });
    expect(code(() => pageToScreen({ x: 20, y: 900, w: 100, h: 44 }, web, { scale: 1 }))).toBe('TOUCH_OUTSIDE_WEBVIEW');
    expect(code(() => pageToScreen({ x: 20, y: 300, w: 100, h: 44 }, web, { scale: 1.5 }))).toBe('PAGE_ZOOMED');
    expect(code(() => pageToScreen({ x: 20, y: 300, w: 100, h: 44 }, null, { scale: 1 }))).toBe('WEBVIEW_FRAME_ABSENT');
  });
});

describe('desktop checks', () => {
  const decision = (command) => ({ sessionRef: 'h:s', prompt: { promptId: 'p', command, question: 'Do you want to proceed?' } });

  test('an answered nonce is matched exactly, not as a prefix of a later one', () => {
    expect(namesNonce('sqlx migrate run --database-url postgres://prod.example.com/f3decision1', 'f3decision', 1)).toBe(true);
    expect(namesNonce('postgres://prod.example.com/f3decision12', 'f3decision', 1)).toBe(false);
    const view = { decisions: [decision('postgres://prod.example.com/f3decision3')] };
    expect(decisionsNamingNonce(view, 'f3decision', 1)).toHaveLength(0);
    expect(decisionsNamingNonce(view, 'f3decision', 3)).toHaveLength(1);
  });

  test('the session row is this computer\'s, and an ambiguous title fails', () => {
    const row = (title, hostLabel = 'This computer') => ({ title, hostLabel, sessionRef: `h:${title}` });
    expect(uniqueSession({ sessions: [row('f3focusmac'), row('f3focusmac', 'win')] }, 'f3focusmac').sessionRef).toBe('h:f3focusmac');
    expect(uniqueSession({ sessions: [] }, 'f3focusmac')).toBe(null);
    expect(code(() => uniqueSession({ sessions: [row('a'), row('a')] }, 'a'))).toBe('ROW_AMBIGUOUS');
  });

  test('typed keys are found across one-key chunks and only from received input', () => {
    const chunks = [...'f3tok'].map((c) => ({ kind: 'received', hex: Buffer.from(c).toString('hex') }));
    expect(typedInto(chunks, 'f3tok')).toBe(true);
    expect(typedInto([{ kind: 'focus-drawn', hex: Buffer.from('f3tok').toString('hex') }], 'f3tok')).toBe(false);
  });

  test('the rules line is read from its first line to its Change control', () => {
    const text = 'Done for you today\nRetries upstream errors every 5 min, restarts sessions\nsilent for 5 min, and asks before anything irreversible. Change\nTell the supervisor anything';
    expect(rulesLineText(text)).toBe('Retries upstream errors every 5 min, restarts sessions silent for 5 min, and asks before anything irreversible.');
    expect(rulesLineText('Tell the supervisor anything')).toBe(null);
  });

  test('a row click is judged against a proven other focus target, never by timing', () => {
    const keys = (text) => [...text].map((c) => ({ kind: 'received', hex: Buffer.from(c).toString('hex') }));
    const probe = 'f3probe1';
    const token = 'f3token1';
    // Focus moved from away to focus on the click.
    expect(rowFocusVerdict({ away: keys(probe), focus: keys(token), probe, token })).toEqual({ focusWasElsewhere: true, clickFocusedTerminal: true });
    // The focus pane already held focus: the probe (however late it was logged) lands there.
    expect(rowFocusVerdict({ away: [], focus: [...keys(probe), ...keys(token)], probe, token }).focusWasElsewhere).toBe(false);
    // The click did nothing: the token stays in the away pane.
    expect(rowFocusVerdict({ away: [...keys(probe), ...keys(token)], focus: [], probe, token }).clickFocusedTerminal).toBe(false);
    // Keys split across panes do not count as either pane having them.
    expect(rowFocusVerdict({ away: keys('f3pro'), focus: keys('be1'), probe, token }).focusWasElsewhere).toBe(false);
  });

  test('the newest global retry rule wins over a scoped one', () => {
    const view = { rules: [
      { id: 1, kind: 'upstreamRetry', scope: {}, params: { thenEverySecs: 600 } },
      { id: 7, kind: 'upstreamRetry', scope: { repo: 'x' }, params: { thenEverySecs: 60 } },
      { id: 5, kind: 'upstreamRetry', scope: {}, params: { thenEverySecs: 300 } },
    ] };
    expect(globalRetryRule(view).params.thenEverySecs).toBe(300);
  });
});

describe('contract', () => {
  test('the plan F3 clauses are scenarios on both desktops, judged both-desktops-pass', () => {
    for (const id of ['F3-D5', 'F3-D6', 'F3-D7']) {
      const scenario = SCENARIOS.find((s) => s.id === id);
      expect(scenario.lanes).toEqual(['macos', 'windows']);
      expect(scenario.cross).toEqual(['both-desktops-pass']);
    }
  });

  test('a run with no D5-D7 steps is not a pass', () => {
    const verdict = validateRun({ schema: 'ferryx-supervisor-f3.run/2', revision: 'a'.repeat(40), lanes: {}, cross: [] });
    for (const id of ['F3-D5', 'F3-D6', 'F3-D7']) expect(verdict.scenarios.find((s) => s.id === id).status).not.toBe('pass');
  });
});
