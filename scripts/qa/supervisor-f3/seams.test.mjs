// Self-tests of the F3 harness seams. Pure: no host, device, daemon or network.
import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { decodePng, encodePng, ocrImage } from './png.mjs';
import {
  PRODUCTION_GATEWAY_PORT, acceptedReply, configProblems, configureGateway, decisionsNamingNonce, gatewayBound, gatewayPortFor,
  activationScript, globalRetryRule, guiBundleFiles, hubEnv, isolationProblems, namesNonce, newRunId, parseEnvBlock, untypeableShortNames, windowsEnvScript, pageToScreen, relayListenPort, remotePath, rowFocusVerdict, rpcCommand, rulesLineText,
  runLanes, sameServeConfig, serveAddedProblems, typedInto, uniqueSession, webContextFrom, webPageChannel, windowBoundsScript, windowRect,
  ENGINE_MODEL, ENGINE_PROVIDER, engineFiles, ocrShows, inputSourceScript, reportGoalRequest, stubReply, framePoint, panelReadRequest, compactOcrShows, workspaceProblem, qaProjectSeed, terminalTokenShown,
  singleLineOcrShows, scrollDelta, startupDsrObserved, classifyFixtureInput,
} from './seams.mjs';
import { LANE_SCHEMA, RUN_SCHEMA, SCENARIOS, validateRun } from './contract.mjs';
import './provenance.test.mjs';
import './native-seams.test.mjs';

const code = (fn) => { try { fn(); } catch (error) { return error.code; } return null; };
const codeAsync = async (fn) => { try { await fn(); } catch (error) { return error.code; } return null; };

describe('real surface input and reads', () => {
  test('terminal OCR requires the entire token and nonce', () => {
    expect(terminalTokenShown('f3token@88ebb', 'f3token088ebb')).toBe(true);
    expect(terminalTokenShown('f3token@88ebc', 'f3token088ebb')).toBe(false);
    expect(terminalTokenShown('xf3token@88ebb', 'f3token088ebb')).toBe(false);
    expect(terminalTokenShown('f3token88ebb', 'f3token088ebb')).toBe(false);
    expect(terminalTokenShown('f3token@88ebbx', 'f3token088ebb')).toBe(false);
    expect(terminalTokenShown('f3token@88ebb|', 'f3token088ebb')).toBe(true);
    expect(terminalTokenShown('f3token@88ebc|', 'f3token088ebb')).toBe(false);
  });
  test('desktop scroll direction follows each native host convention', () => {
    expect(scrollDelta('darwin', 'down', 520)).toBe(-520);
    expect(scrollDelta('win32', 'down', 520)).toBe(520);
    expect(scrollDelta('darwin', 'up', 900)).toBe(900);
    expect(scrollDelta('win32', 'up', 900)).toBe(-900);
    expect(code(() => scrollDelta('linux', 'down', 520))).toBe('SCROLL_PLATFORM');
  });
  test('Viewer marker must fit on one OCR line rather than adjacent panel rows', () => {
    const marker = 'Viewer \u00b7 Supervisor report';
    expect(singleLineOcrShows('Viewer - Supervisor report Rendered Markdown', marker)).toBe(true);
    expect(singleLineOcrShows('Open in Viewer\nSupervisor report', marker)).toBe(false);
    expect(singleLineOcrShows('Open in Viewer\r\nSupervisor report', marker)).toBe(false);
    expect(singleLineOcrShows('Supervisor report', marker)).toBe(false);
  });
  test('startup DSR answer requires the exact query in an attach history', () => {
    const history = (text) => Buffer.from(text).toString('base64');
    expect(startupDsrObserved({ type: 'attachOk', history: history('ready\x1b[6n') })).toBe(true);
    expect(startupDsrObserved({ type: 'attachOk', history: history('\x1b[5n') })).toBe(false);
    expect(startupDsrObserved({ type: 'attachOk', history: history('\x1b[6') })).toBe(false);
    expect(startupDsrObserved({ type: 'error', history: history('\x1b[6n') })).toBe(false);
    expect(startupDsrObserved(null)).toBe(false);
  });
  test('compact OCR tolerates split token glyphs but rejects a different nonce', () => {
    expect(compactOcrShows('F3 check f3tok 1653 3d48:', 'f3tok16533d48')).toBe(true);
    expect(compactOcrShows('f3tok16533d49', 'f3tok16533d48')).toBe(false);
    expect(workspaceProblem('Workspace unavailable (INVALID_REPO_ROOT)')).toBe(true);
    expect(workspaceProblem('f3 ready')).toBe(false);
  });
  test('QA project seed writes only the requested project and active id', () => {
    const values = new Map();
    const localStorage = { clear: () => values.clear(), setItem: (k, v) => values.set(k, v) };
    const script = qaProjectSeed('/qa/repo').replace(/^<script>|<\/script>$/g, '');
    Function('localStorage', 'location', script)(localStorage, { hostname: '127.0.0.1' });
    expect(JSON.parse(values.get('ferryx.projects'))).toEqual([{ workspaceId: 'f3', repoRoot: '/qa/repo', gitRoot: '/qa/repo' }]);
    expect(values.get('ferryx.active-project')).toBe('f3');
  });
  test('pixel points become top-left normalized coordinates independently of host logical size', () => {
    expect(framePoint({ x: 1028, y: 664 }, 2056, 1328)).toEqual({ x: 0.5, y: 0.5, normalized: true });
    expect(framePoint({ x: 514, y: 332 }, 2056, 1328)).toEqual({ x: 0.25, y: 0.25, normalized: true });
  });
  test('transcript is the app transcript, not a session-specific conversation', () => {
    expect(panelReadRequest('transcript')).toEqual({ type: 'supervisorReadV1', request: { view: 'transcript', cursor: 0 } });
    expect(panelReadRequest('panel')).toEqual({ type: 'supervisorReadV1', request: { view: 'panel' } });
  });
});

describe('fixture input buffering', () => {
  const classify = (text, extra = {}) => classifyFixtureInput({ bytes: Buffer.from(text), controlInput: '', drawn: true, mode: 'upstream', ...extra });

  test.each(['\r', '\n'])('a report token is complete only after its %j terminator', (terminator) => {
    const prefix = classify('F3-REPORT:abc');
    expect(prefix.kind).toBe('input');
    const suffix = classify('123', { controlInput: prefix.controlInput });
    expect(suffix.kind).toBe('input');
    const completed = classify(terminator, { controlInput: suffix.controlInput });
    expect(completed.kind).toBe('report:abc123');
    expect(completed.controlInput).toBe('');
  });

  test('a held draw command survives chunk boundaries and clears the control buffer', () => {
    const prefix = classify('F3-DR', { drawn: false, mode: 'held' });
    expect(prefix.kind).toBe('input');
    expect(classify('AW\r', { drawn: false, mode: 'held', controlInput: prefix.controlInput })).toEqual({ kind: 'draw', controlInput: '' });
    expect(classify('F3-DRAW\r').kind).toBe('input');
  });

  test('control buffering is bounded and Ctrl-C takes precedence over commands', () => {
    expect(classify('x'.repeat(5000)).controlInput.length).toBe(4096);
    expect(classify('F3-REPORT:abc\r\x03').kind).toBe('stop');
  });
});

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
    win.hosts = { windows: { ...win.hosts.macos, hostId: 'maho-win', platform: 'win32', worktree: 'C:\\Users\\u\\ferryx-wt\\f3win' } };
    win.sessionHost = { ...win.sessionHost, hostId: 'maho-win-session', platform: 'win32' };
    expect(configProblems(win)).toEqual([]);
  });

  const winLocal = () => JSON.parse(readFileSync(new URL('./config.win-local.example.json', import.meta.url), 'utf8'));

  test('the shipped win-local example runs only the Windows lane on a loopback relay, all hosts local', () => {
    const config = winLocal();
    expect(configProblems(config)).toEqual([]);
    expect(runLanes(config)).toEqual(['windows']);
    expect(config.sessionHost.platform).toBe('win32');
    expect(rpcCommand(config.hosts.windows, 'C:\\h\\x.mjs')).toEqual([config.hosts.windows.bun, ['C:\\h\\x.mjs', '--rpc']]);
  });

  test('a Windows worktree whose 8.3 launcher path would hold - or _ is refused', () => {
    expect(untypeableShortNames('C:\\Users\\sook\\ferryx-wt\\f3winlocal-20261104')).toEqual([]);
    expect(untypeableShortNames('C:\\Users\\sook\\ferryx-wt\\supv-f3-final')).toEqual(['supv-f3-final']);
    expect(untypeableShortNames('C:\\Users\\sook\\ferryx-wt\\f3_win')).toEqual(['f3_win']);
    expect(untypeableShortNames('C:\\a\\.native-qa')).toEqual([]);
    const bad = winLocal();
    bad.hosts.windows.worktree = 'C:\\Users\\sook\\ferryx-wt\\supv-f3-final';
    expect(configProblems(bad).join()).toContain("hosts.windows.worktree");
  });

  test('a run id is short and its 8.3 name has no - or _', () => {
    const id = newRunId(new Date('2026-11-04T09:15:00Z'), 'a1b2c3d4-0000');
    expect(id).toBe('f311040915a1b2');
    expect(id).toMatch(/^[a-z0-9]+$/);
    expect(untypeableShortNames(`C:\\w\\.native-qa\\${id}`)).toEqual([]);
    expect(untypeableShortNames(`C:\\w\\.native-qa\\${id}-s`)).toEqual([]);
  });

  test('the Windows live env read names one pid, reads only, and its answer parses to KEY=VALUE', () => {
    const script = windowsEnvScript(4242);
    expect(script).toContain('[F3ProcEnv]::Read(4242)');
    expect(script).toContain('OpenProcess(0x0410');
    expect(script).not.toMatch(/WriteProcessMemory|TerminateProcess|Stop-Process|taskkill/);
    expect(code(() => windowsEnvScript(0))).toBe('GUI_NOT_ISOLATED');
    const block = Buffer.from('=C:=C:\\w\0HOME=C:\\w\\.native-qa\\r\\home\0FERRYX_QA_ISOLATED=1\0\0', 'utf8').toString('base64');
    expect(parseEnvBlock(`${block}\r\n`)).toEqual(['HOME=C:\\w\\.native-qa\\r\\home', 'FERRYX_QA_ISOLATED=1']);
  });

  test('a Windows GUI is proven isolated by its env alone; nothing read is never a pass', () => {
    const want = { expected: { HOME: 'C:\\w\\.native-qa\\r\\home', FERRYX_QA_ISOLATED: '1' }, profile: 'C:\\w\\.native-qa\\r', ownerRoots: ['C:\\Users\\o\\AppData\\Roaming\\Ferryx'] };
    expect(isolationProblems({ env: ['Home=C:\\w\\.native-qa\\r\\home', 'FERRYX_QA_ISOLATED=1'], openPaths: null }, want)).toEqual([]);
    expect(isolationProblems({ env: ['HOME=C:\\Users\\o', 'FERRYX_QA_ISOLATED=1'], openPaths: null }, want).join()).toContain('HOME is');
    expect(isolationProblems({ env: ['FERRYX_QA_ISOLATED=1'], openPaths: null }, want).join()).toContain('HOME is null');
    expect(isolationProblems({ env: null, openPaths: null }, want).join()).toContain('could be read');
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

  test('a worktree whose daemon socket path would pass the unix limit is refused', () => {
    const long = local();
    long.hosts.macos.worktree = `/Users/someone/code/ferryx-wt/${'w'.repeat(40)}`;
    expect(configProblems(long).join()).toContain('hosts.macos.worktree');
    expect(configProblems(long).join()).toContain('over the darwin limit 103');
    const linux = full();
    linux.sessionHost.worktree = `/home/indo/${'w'.repeat(80)}`;
    expect(configProblems(linux).join()).toContain('over the linux limit 107');
    const win = local();
    win.lanes = ['windows'];
    win.hosts = { windows: { ...win.hosts.macos, hostId: 'maho-win', platform: 'win32', worktree: `C:\\${'w'.repeat(120)}` } };
    win.sessionHost = { ...win.sessionHost, hostId: 'maho-win-session', platform: 'win32', worktree: `C:\\${'w'.repeat(120)}` };
    expect(configProblems(win)).toEqual([]);
  });

  test('unknown, duplicate or empty lanes are refused', () => {
    for (const lanes of [[], ['macos', 'macos'], ['linux']]) {
      const config = local();
      config.lanes = lanes;
      expect(configProblems(config).join()).toContain('must be distinct names');
    }
  });

  test('a live GUI is isolated only by its own env and open files, never the owner\'s', () => {
    const want = { expected: { HOME: '/w/.native-qa/r/home', FERRYX_QA_ISOLATED: '1' }, profile: '/w/.native-qa/r', ownerRoots: ['/Users/o/Library/Application Support/Ferryx', '/Users/o/.ferryx'] };
    const held = ['/w/.native-qa/r/evidence/gui-stdout.log', '/w/.native-qa/r/data/ferryx.db'];
    expect(isolationProblems({ env: null, openPaths: held }, want)).toEqual([]);
    expect(isolationProblems({ env: ['PATH=/usr/bin', 'HOME=/w/.native-qa/r/home', 'FERRYX_QA_ISOLATED=1'], openPaths: held }, want)).toEqual([]);
    expect(isolationProblems({ env: ['HOME=/Users/o', 'FERRYX_QA_ISOLATED=1'], openPaths: held }, want).join()).toContain('HOME is "/Users/o"');
    expect(isolationProblems({ env: null, openPaths: [...held, '/Users/o/Library/Application Support/Ferryx/state.db'] }, want).join()).toContain("owner's files");
    // The launcher's own log fds under evidence/ prove nothing about the GUI's env.
    expect(isolationProblems({ env: null, openPaths: ['/w/.native-qa/r/evidence/gui-stdout.log'] }, want).join()).toContain('holds nothing open under');
    expect(isolationProblems({ env: null, openPaths: ['/w/.native-qa/rx/data/a'] }, want).join()).toContain('holds nothing open under');
  });

  test('the macOS raise names one pid and activates it only when it runs the run binary', () => {
    const script = activationScript(4242, '/w/.qa/f3-bin/gui/ferryx');
    expect(script).toContain('runningApplicationWithProcessIdentifier(4242)');
    expect(script).toContain('const want = "/w/.qa/f3-bin/gui/ferryx";');
    expect(script.indexOf('exe !== want')).toBeLessThan(script.indexOf('activateWithOptions'));
    expect(script).not.toContain('runningApplicationsWithBundleIdentifier');
    expect(code(() => activationScript(0, '/w/ferryx'))).toBe('GUI_NOT_FRONT');
    expect(code(() => activationScript(4242, 'ferryx'))).toBe('GUI_NOT_FRONT');
    // Raised means the frontmost application is that pid, read back, not the request's answer.
    expect(script).toContain('activated: frontPid === 4242');
    expect(script.indexOf('exe !== want')).toBeLessThan(script.indexOf('System Events'));
  });

  test('every isolated daemon runs the hub on a database inside its own profile', () => {
    expect(hubEnv({ platform: 'darwin' }, '/w/.native-qa/r')).toEqual({ FERRYX_HUB: 'on', FERRYX_HUB_DB: '/w/.native-qa/r/data/hub/hub.sqlite', FERRYX_HUB_QUIET_HOURS: 'off', OMON_INBOX_ENABLED: '0' });
    expect(hubEnv({ platform: 'win32' }, 'C:\\w\\.native-qa\\r').FERRYX_HUB_DB).toBe('C:\\w\\.native-qa\\r\\data\\hub\\hub.sqlite');
  });

  test('a smoke run never passes, and a lane left out blocks its scenarios', () => {
    const rev = 'b'.repeat(40);
    const session = { schema: LANE_SCHEMA, lane: 'session', blocked: null, failure: null, machine: { id: 'm' }, candidate: { headRevision: rev, clean: true, stamp: `${rev}+e3b0` }, harness: { headRevision: rev, clean: true, status: '' }, steps: [], cleanup: [] };
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
    candidate: { headRevision: rev, clean: true, stamp: `${rev}+e3b0` }, harness: { headRevision: rev, clean: true, status: '' }, steps: [], cleanup: [], ...extra });
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

  test('a separate harness revision does not relax the candidate revision', () => {
    const harness = 'd'.repeat(40);
    const receipt = { ...run(session()), harnessRevision: harness };
    receipt.control.head = harness;
    receipt.session.harness.headRevision = harness;
    expect(validateRun(receipt).errors).toEqual([]);
    receipt.session.candidate.headRevision = harness;
    expect(validateRun(receipt).errors.some((error) => error.includes('candidate head'))).toBe(true);
    receipt.control.head = rev;
    expect(validateRun(receipt).errors.some((error) => error.includes('control checkout'))).toBe(true);
  });

  test('a blocked session host blocks, never passes', () => {
    const verdict = validateRun(run(session({ blocked: { code: 'HOST_UNAVAILABLE' } })));
    expect(verdict.errors).toEqual([]);
    expect(verdict.verdict).not.toBe('pass');
  });
});

describe('frame OCR input', () => {
  test('a frame is cut to the run window, inverted to dark-on-light and doubled, boxes mapping back', () => {
    // 4x2 RGB frame: white text pixel (255) on dark (20) at (2,1).
    const gray = Buffer.from([20, 20, 20, 20, 20, 20, 255, 20]);
    const rgb = Buffer.alloc(8 * 3); gray.forEach((v, i) => rgb.fill(v, i * 3, i * 3 + 3));
    const frame = encodePng(4, 2, 3, rgb);
    const image = ocrImage(frame, { x: 1, y: 0, width: 3, height: 2 }, 2);
    const out = decodePng(image.png);
    expect([out.width, out.height, out.channels]).toEqual([6, 4, 1]);
    expect(out.px[2 * 6 + 2]).toBe(0); // the white pixel is now black
    expect(out.px[0]).toBe(235); // the dark background is now light
    expect(image.origin).toEqual({ x: 1, y: 0 });
  });

  test('window bounds in points map to frame pixels on a Retina frame', () => {
    expect(windowRect({ x: 194, y: 63, width: 640, height: 425 }, { width: 1028, height: 664.5 }, 2056)).toEqual({ x: 388, y: 126, width: 1280, height: 850 });
    expect(windowRect(null, { width: 1 }, 2)).toBeNull();
    expect(() => windowBoundsScript(8754, 'relative')).toThrow();
    expect(windowBoundsScript(42, '/w/.qa/f3-bin/gui/ferryx')).toContain('kCGWindowOwnerPID === 42');
  });
});


describe('supervisor engine, owner and typing seams', () => {
  test('the engine runs on the loopback stub only, offline, with the remote MCP servers off', () => {
    const files = engineFiles({ baseUrl: 'http://127.0.0.1:5000/v1', mhcBin: '/u/.local/bin/mhc' });
    expect(files.envFile.split('\n').filter(Boolean)).toEqual(['FERRYX_HUB_MHC_SUPERVISOR=1', 'FERRYX_HUB_MHC_BIN=/u/.local/bin/mhc', `FERRYX_HUB_MHC_PROVIDER=${ENGINE_PROVIDER}`, `FERRYX_HUB_MHC_MODEL=${ENGINE_MODEL}`]);
    expect(files.models.providers[ENGINE_PROVIDER].baseUrl).toBe('http://127.0.0.1:5000/v1');
    expect(files.mcp.mcpServers).toEqual({ context7: { enabled: false }, grep_app: { enabled: false } });
    expect(files.offline).toMatchObject({ MAHO_OFFLINE: '1', DO_NOT_TRACK: '1' });
  });

  test('the stub replies to the owner prompt naming its token, and to nothing else', () => {
    expect(stubReply('F3 check f3tokab12cd34: what is waiting on me?')).toBe('Reply for f3tokab12cd34 is ready');
    expect(stubReply('assess the goal')).toBeNull();
  });

  test('the report goal binds exactly the given local pane', () => {
    const frame = reportGoalRequest('f3-1104-ab', 'pane-7');
    expect(frame.type).toBe('supervisorDispatch');
    expect(frame.request).toMatchObject({ action: 'submit', hostId: 'local', sessionId: 'pane-7', messageId: 'f3-report-goal-f3-1104-ab' });
    expect(frame.request.work.requirements).toHaveLength(1);
  });

  test('an input source is selected by its exact id only', () => {
    expect(inputSourceScript('com.apple.keylayout.ABC')).toContain('"com.apple.keylayout.ABC"');
    expect(code(() => inputSourceScript('x"; evil'))).toBe('INPUT_SOURCE');
  });
});

describe('OCR text matching', () => {
  test('the Viewer title matches with its middle dot read as a hyphen, and 1 read as i', () => {
    expect(ocrShows('Viewer - Supervisor report Rendered Markdown', 'Viewer \u00b7 Supervisor')).toBe(true);
    expect(ocrShows('F3 long report maci326cf22', 'F3 long report mac1326cf22')).toBe(true);
    expect(ocrShows('Section 19 Migration check 19 for x', 'Migration check 9 for')).toBe(false);
    expect(ocrShows('Conversation Export transcript', 'supv-f3-mac-3428ce97')).toBe(false);
  });
});
