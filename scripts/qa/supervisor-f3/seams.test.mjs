// Self-tests of the F3 harness seams. Pure: no host, device, daemon or network.
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  PRODUCTION_GATEWAY_PORT, acceptedReply, configProblems, configureGateway, decisionsNamingNonce, envProblems, gatewayBound, gatewayPortFor,
  globalRetryRule, guiBundleFiles, namesNonce, pageToScreen, relayListenPort, remotePath, rowFocusVerdict, rpcCommand, rulesLineText,
  runLanes, sameServeConfig, serveAddedProblems, typedInto, uniqueSession, webContextFrom, webPageChannel,
} from './seams.mjs';
import { LANE_SCHEMA, RUN_SCHEMA, SCENARIOS, validateRun } from './contract.mjs';

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
    const verdict = validateRun({ schema: RUN_SCHEMA, revision: 'a'.repeat(40), lanes: {}, cross: [] });
    for (const id of ['F3-D5', 'F3-D6', 'F3-D7']) expect(verdict.scenarios.find((s) => s.id === id).status).not.toBe('pass');
  });
});

describe('session host config', () => {
  const example = () => JSON.parse(readFileSync(new URL('./config.example.json', import.meta.url), 'utf8'));

  test('the shipped example is a Linux session host behind tailscale serve and passes the checks', () => {
    const config = example();
    expect(configProblems(config)).toEqual([]);
    expect(config.sessionHost.platform).toBe('linux');
    expect(relayListenPort(config)).toBe(config.sessionHost.relayFront.backendPort);
    expect(config.hosts.macos.launchTerminalHotkey).toBeUndefined();
  });

  test('the relay origin port must be the serve port, and the backend another port', () => {
    const config = example();
    config.relayOrigin = 'https://indo.taila0cef2.ts.net:47931';
    expect(configProblems(config).join()).toContain('is not sessionHost.relayFront.publicPort');
    const same = example();
    same.sessionHost.relayFront.backendPort = same.sessionHost.relayFront.publicPort;
    expect(configProblems(same).join()).toContain('distinct publicPort and backendPort');
  });

  test('a missing session host, a desktop hostId reused or an unknown front is refused', () => {
    const none = example();
    delete none.sessionHost;
    expect(configProblems(none).join()).toContain('sessionHost.hostId');
    const reused = example();
    reused.sessionHost.hostId = reused.hosts.windows.hostId;
    expect(configProblems(reused).join()).toContain('must differ from the desktop hostIds');
    const front = example();
    front.sessionHost.relayFront = { kind: 'plain' };
    expect(configProblems(front).join()).toContain('tailscale-serve, tls or loopback');
  });

  test('an IP or plain-HTTP origin is refused; a self-served PEM listens on the origin port', () => {
    const ip = example();
    ip.relayOrigin = 'https://100.91.254.71:47930';
    expect(configProblems(ip).join()).toContain('must be https:// with a DNS name');
    const tls = example();
    tls.sessionHost.relayFront = { kind: 'tls', cert: '/c.pem', key: '/k.pem' };
    tls.relayOrigin = 'https://relay.example.ts.net:47950';
    expect(configProblems(tls)).toEqual([]);
    expect(relayListenPort(tls)).toBe(47950);
  });

  test('helpers start over ssh per platform, or as a local child for the control host itself', () => {
    const linux = { platform: 'linux', sshDestination: 'omarchy', bun: '/usr/local/bin/bun' };
    expect(rpcCommand(linux, '/h/x.mjs')).toEqual(['ssh', ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', 'omarchy', "'/usr/local/bin/bun' '/h/x.mjs' --rpc"]]);
    expect(rpcCommand({ ...linux, sshDestination: 'local' }, '/h/x.mjs')).toEqual(['/usr/local/bin/bun', ['/h/x.mjs', '--rpc']]);
    const win = { platform: 'win32', sshDestination: 'maho-win', bun: 'C:\\b\\bun.exe' };
    expect(rpcCommand(win, 'C:\\h\\x.mjs', '/usr/bin/ssh')[1].at(-1)).toBe(`powershell.exe -NoProfile -NonInteractive -Command "& 'C:\\b\\bun.exe' 'C:\\h\\x.mjs' --rpc"`);
    expect(remotePath(linux, '/w', '.native-qa', 'r')).toBe('/w/.native-qa/r');
    expect(remotePath(win, 'C:\\w', 'r')).toBe('C:\\w\\r');
  });
});

describe('all-local run', () => {
  const local = () => JSON.parse(readFileSync(new URL('./config.mac-local.example.json', import.meta.url), 'utf8'));
  const full = () => JSON.parse(readFileSync(new URL('./config.example.json', import.meta.url), 'utf8'));

  test('the shipped mac-local example runs only the macOS lane on a loopback relay, all hosts local', () => {
    const config = local();
    expect(configProblems(config)).toEqual([]);
    expect(runLanes(config)).toEqual(['macos']);
    expect(relayListenPort(config)).toBe(47960);
    expect(rpcCommand(config.sessionHost, '/h/x.mjs')).toEqual([config.sessionHost.bun, ['/h/x.mjs', '--rpc']]);
    expect(rpcCommand(config.hosts.macos, '/h/x.mjs')[0]).toBe(config.hosts.macos.bun);
    expect(runLanes(full())).toEqual(['macos', 'windows', 'iphone']);
  });

  test('a lane left out needs no config block; the same shape serves a Windows-only run', () => {
    const win = local();
    win.lanes = ['windows'];
    win.hosts = { windows: { ...win.hosts.macos, hostId: 'maho-win', platform: 'win32' } };
    win.sessionHost = { ...win.sessionHost, hostId: 'maho-win-session', platform: 'win32' };
    expect(configProblems(win)).toEqual([]);
  });

  test('a loopback relay refuses the iPhone, a remote host, a non-loopback or https origin', () => {
    const phone = local();
    phone.lanes = ['macos', 'iphone'];
    phone.iphone = { serial: 's', driverEndpoint: 'http://127.0.0.1:8100' };
    expect(configProblems(phone).join()).toContain('cannot serve the iPhone lane');
    const remote = local();
    remote.hosts.macos.sshDestination = 'maho-mac';
    expect(configProblems(remote).join()).toContain('hosts.macos.sshDestination must be "local"');
    for (const origin of ['http://100.68.224.49:47960', 'https://127.0.0.1:47960', 'http://localhost:47960', 'http://127.0.0.1']) {
      const bad = local();
      bad.relayOrigin = origin;
      expect(configProblems(bad).join()).toContain('must be http://127.0.0.1:<port>');
    }
  });

  test('a plain-HTTP origin is still refused for every other front', () => {
    const config = full();
    config.relayOrigin = 'http://127.0.0.1:47930';
    expect(configProblems(config).join()).toContain('must be https://');
  });

  test('unknown, duplicate or empty lanes are refused', () => {
    for (const lanes of [[], ['macos', 'macos'], ['linux']]) {
      const config = local();
      config.lanes = lanes;
      expect(configProblems(config).join()).toContain('must be distinct names');
    }
  });

  test('a live GUI env is isolated only when every run key holds exactly the run value', () => {
    const expected = { HOME: '/w/.native-qa/r/home', FERRYX_QA_ISOLATED: '1' };
    expect(envProblems(['PATH=/usr/bin', 'HOME=/w/.native-qa/r/home', 'FERRYX_QA_ISOLATED=1'], expected)).toEqual([]);
    expect(envProblems(['HOME=/Users/owner', 'FERRYX_QA_ISOLATED=1'], expected).join()).toContain('HOME is "/Users/owner"');
    expect(envProblems(['HOME=/w/.native-qa/r/home'], expected).join()).toContain('FERRYX_QA_ISOLATED is null');
  });

  test('a smoke run never passes, and a lane left out blocks its scenarios', () => {
    const rev = 'b'.repeat(40);
    const session = { schema: LANE_SCHEMA, lane: 'session', blocked: null, failure: null, machine: { id: 'm' }, candidate: { headRevision: rev, clean: true, stamp: `${rev}+e3b0` }, steps: [], cleanup: [] };
    const base = { schema: RUN_SCHEMA, revision: rev, control: { hostname: 'c', head: rev, clean: true, scripts: [{ path: 'a', sha256: 'c'.repeat(64) }] }, session, cross: [] };
    const out = { schema: LANE_SCHEMA, blocked: { code: 'LANE_NOT_IN_RUN' }, steps: [], cleanup: [] };
    const verdict = validateRun({ ...base, lanes: { macos: out, windows: out, iphone: out } });
    expect(verdict.verdict).toBe('blocked');
    const smoke = validateRun({ ...base, smoke: true, lanes: { macos: out, windows: out, iphone: out } });
    expect(smoke.verdict).toBe('fail');
    expect(smoke.errors.join()).toContain('smoke run');
  });
});

describe('tailscale serve endpoint', () => {
  const owner = { TCP: { 443: { HTTPS: true }, 8443: { HTTPS: true } }, Web: {
    'indo.ts.net:443': { Handlers: { '/': { Proxy: 'http://127.0.0.1:8088' } } },
    'indo.ts.net:8443': { Handlers: { '/': { Proxy: 'http://127.0.0.1:8099' } } } } };
  const withRun = (base = owner) => ({ TCP: { ...structuredClone(base.TCP), 47930: { HTTPS: true } },
    Web: { ...structuredClone(base.Web), 'indo.ts.net:47930': { Handlers: { '/': { Proxy: 'http://127.0.0.1:47931' } } } } });
  const want = { hostName: 'indo.ts.net', port: 47930, backend: 47931 };

  test('only the run endpoint was added, proxying to the loopback relay', () => {
    expect(serveAddedProblems(owner, withRun(), want)).toEqual([]);
    expect(serveAddedProblems({}, withRun({ TCP: {}, Web: {} }), want)).toEqual([]);
  });

  test('a changed owner entry, a wrong backend or a pre-existing port is reported', () => {
    const changed = withRun();
    changed.Web['indo.ts.net:443'].Handlers['/'].Proxy = 'http://127.0.0.1:9999';
    expect(serveAddedProblems(owner, changed, want).join()).toContain('Web entries other than indo.ts.net:47930 changed');
    expect(serveAddedProblems(owner, withRun(), { ...want, backend: 47932 }).join()).toContain('not http://127.0.0.1:47932');
    expect(serveAddedProblems(withRun(), withRun(), want).join()).toContain('TCP.47930 existed before this run');
  });

  test('teardown is proven only by the exact snapshot', () => {
    const reordered = { Web: owner.Web, TCP: { 8443: { HTTPS: true }, 443: { HTTPS: true } } };
    expect(sameServeConfig(owner, reordered)).toBe(true);
    expect(sameServeConfig(owner, withRun())).toBe(false);
  });
});

describe('macOS GUI launcher bundle', () => {
  const args = { bundleId: 'dev.ferryx.qa.f3-20261104-abc123', bun: '/Users/u/.local/bin/bun', agent: "/Users/u/wt/it's/native-remote-agent.mjs",
    specPath: '/Users/u/wt/.native-qa/r/gui-launch-spec.json', logPath: '/Users/u/wt/.native-qa/r/evidence/l.log' };

  test('the bundle runs the native agent launcher on the spec, quoted, with bun on PATH', () => {
    const files = guiBundleFiles(args);
    expect(Object.keys(files).sort()).toEqual(['Contents/Info.plist', 'Contents/MacOS/f3-launch']);
    const script = files['Contents/MacOS/f3-launch'];
    expect(script.startsWith('#!/bin/sh\n')).toBe(true);
    expect(script).toContain("exec '/Users/u/.local/bin/bun' '/Users/u/wt/it'\\''s/native-remote-agent.mjs' --launch-gui '/Users/u/wt/.native-qa/r/gui-launch-spec.json'");
    expect(script).toContain(':/Users/u/.local/bin');
    expect(files['Contents/Info.plist']).toContain('<key>CFBundleExecutable</key><string>f3-launch</string>');
    expect(files['Contents/Info.plist']).toContain(`<string>${args.bundleId}</string>`);
    expect(files['Contents/Info.plist']).toContain('<key>LSUIElement</key><true/>');
  });

  test('a relative path or an unsafe bundle id is refused', () => {
    expect(code(() => guiBundleFiles({ ...args, specPath: 'spec.json' }))).toBe('GUI_BUNDLE');
    expect(code(() => guiBundleFiles({ ...args, bundleId: 'dev.ferryx/<x>' }))).toBe('GUI_BUNDLE');
  });
});

describe('session host in the verdict', () => {
  const rev = 'b'.repeat(40);
  const session = (extra = {}) => ({ schema: LANE_SCHEMA, lane: 'session', blocked: null, failure: null, machine: { id: 'm' },
    candidate: { headRevision: rev, clean: true, stamp: `${rev}+e3b0` }, steps: [], cleanup: [], ...extra });
  const run = (s) => ({ schema: RUN_SCHEMA, revision: rev, control: { hostname: 'c', head: rev, clean: true, scripts: [{ path: 'a', sha256: 'c'.repeat(64) }] },
    session: s, lanes: {}, cross: [] });

  test('a failed session host, a wrong stamp or a leaked serve endpoint fails the run', () => {
    expect(validateRun(run(session())).errors).toEqual([]);
    expect(validateRun(run(session({ failure: { code: 'SERVE_ENDPOINT' } }))).errors.join()).toContain('session host: lane failed: SERVE_ENDPOINT');
    expect(validateRun(run(session({ candidate: { headRevision: rev, clean: true, stamp: 'x+y' } }))).errors.join()).toContain('session host: candidate build stamp');
    const leak = session({ cleanup: [{ resource: 'serve-endpoint', port: 47930, action: 'reported-not-killed' }] });
    expect(validateRun(run(leak)).verdict).toBe('fail');
    expect(validateRun(run(null)).errors.join()).toContain('session host: lane receipt missing');
  });

  test('a blocked session host blocks, never passes', () => {
    const verdict = validateRun(run(session({ blocked: { code: 'HOST_UNAVAILABLE' } })));
    expect(verdict.errors).toEqual([]);
    expect(verdict.verdict).not.toBe('pass');
  });
});
