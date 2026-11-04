// Pure seams of the F3 run, kept apart so the harness self-tests exercise them with no host,
// device or daemon: daemon reply checking, the run-owned gateway port, the iPhone page-script
// channel, and the checks behind the session-row, rules-line and phone-answer steps.
import { LANES } from './contract.mjs';
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

const REQUIRED_DESKTOP = ['hostId', 'platform', 'sshDestination', 'bun', 'hostHelper', 'remoteAgent', 'worktreeRoot', 'worktree', 'binary', 'cli', 'uiDist', 'buildManifest', 'mahordAddress', 'pairingId', 'viewerMarker'];
const REQUIRED_SESSION_HOST = ['hostId', 'platform', 'sshDestination', 'bun', 'hostHelper', 'remoteAgent', 'worktreeRoot', 'worktree', 'binary', 'cli', 'relay', 'uiDist', 'buildManifest'];
const isPort = (value) => Number.isSafeInteger(value) && value >= 1024 && value <= 65535;

/**
 * The lanes this run drives: `config.lanes`, or all of them. A lane left out is recorded
 * blocked (LANE_NOT_IN_RUN), so a partial run can never pass F3.
 */
export const runLanes = (config) => config?.lanes ?? [...LANES];

/**
 * Every problem of an F3 config, empty when it can run. The session host (relay, fixture daemon,
 * fixture panes) is its own block: Linux behind `tailscale serve` (`relayFront.kind`
 * `tailscale-serve`: public HTTPS port -> loopback relay port), any host serving an operator
 * PEM itself (`tls`), or `loopback`: an all-local run where the control host, the session host
 * and every desktop in the run are one machine (`sshDestination` "local") and the relay is
 * plain HTTP on 127.0.0.1, which only a run without the iPhone may use (iOS push needs HTTPS).
 * Its hostId names its machine token, so it must differ from the desktops'.
 */
export function configProblems(config) {
  const lanes = runLanes(config);
  if (!Array.isArray(lanes) || lanes.length === 0 || new Set(lanes).size !== lanes.length || lanes.some((name) => !LANES.includes(name))) {
    return [`lanes ${JSON.stringify(config?.lanes)} must be distinct names from ${LANES.join(', ')}`];
  }
  const desktops = ['macos', 'windows'].filter((name) => lanes.includes(name));
  const missing = [];
  for (const name of desktops) for (const key of REQUIRED_DESKTOP) if (config?.hosts?.[name]?.[key] === undefined) missing.push(`hosts.${name}.${key}`);
  for (const key of REQUIRED_SESSION_HOST) if (config?.sessionHost?.[key] === undefined) missing.push(`sessionHost.${key}`);
  if (lanes.includes('iphone')) for (const key of ['serial', 'driverEndpoint']) if (!config?.iphone?.[key]) missing.push(`iphone.${key}`);
  for (const key of ['mahordExecutable', 'tesseract', 'ownerEmail', 'relayOrigin']) if (!config?.[key]) missing.push(key);
  if (missing.length) return [`missing ${missing.join(', ')}`];
  const problems = [];
  let origin;
  try { origin = new URL(config.relayOrigin); } catch { return [`relayOrigin ${config.relayOrigin} is not a URL`]; }
  const host = config.sessionHost;
  const front = host.relayFront;
  const originPort = Number(origin.port || (origin.protocol === 'http:' ? 80 : 443));
  if (front?.kind === 'loopback') {
    if (origin.protocol !== 'http:' || origin.hostname !== '127.0.0.1' || !isPort(originPort) || origin.pathname !== '/') problems.push(`relayOrigin ${config.relayOrigin} must be http://127.0.0.1:<port> for a loopback relay`);
    if (lanes.includes('iphone')) problems.push('a loopback relay cannot serve the iPhone lane (iOS push needs an HTTPS origin)');
    const remote = [['sessionHost', host], ...desktops.map((name) => [`hosts.${name}`, config.hosts[name]])].filter(([, h]) => h.sshDestination !== 'local');
    if (remote.length) problems.push(`a loopback relay needs every host on the control host: ${remote.map(([key]) => `${key}.sshDestination`).join(', ')} must be "local"`);
  } else if (origin.protocol !== 'https:' || /^\d+\.\d+\.\d+\.\d+$/.test(origin.hostname) || origin.hostname === 'localhost') {
    // iOS Web Push needs a secure context: the phone loads the relay over HTTPS with a name the
    // certificate covers, never plain HTTP to a tailnet address.
    problems.push(`relayOrigin ${config.relayOrigin} must be https:// with a DNS name the relay certificate covers (iOS push needs a secure context)`);
  }
  if (desktops.some((name) => config.hosts[name].hostId === host.hostId)) problems.push(`sessionHost.hostId ${host.hostId} must differ from the desktop hostIds`);
  if (!['linux', 'darwin', 'win32'].includes(host.platform)) problems.push(`sessionHost.platform ${host.platform} is not linux, darwin or win32`);
  if (front?.kind === 'tailscale-serve') {
    if (!isPort(front.publicPort) || !isPort(front.backendPort) || front.publicPort === front.backendPort) problems.push('sessionHost.relayFront needs distinct publicPort and backendPort in 1024..65535');
    if (front.publicPort !== originPort) problems.push(`relayOrigin port ${originPort} is not sessionHost.relayFront.publicPort ${front.publicPort}`);
    if (host.platform === 'win32') problems.push('sessionHost.relayFront tailscale-serve is supported on linux and darwin session hosts only');
  } else if (front?.kind === 'tls') {
    if (!front.cert || !front.key) problems.push('sessionHost.relayFront tls needs cert and key (PEM paths on the session host)');
  } else if (front?.kind !== 'loopback') {
    problems.push('sessionHost.relayFront.kind must be tailscale-serve, tls or loopback');
  }
  return [...problems, ...socketPathProblems(config)];
}

/** The session daemon's profile is `.native-qa/<runId><suffix>`, apart from a desktop's. */
export const SESSION_PROFILE_SUFFIX = '-s';
/** The longest run id run.mjs makes: `f3-MMDDhhmm-xxxx`. */
const RUN_ID_MAX = 'f3-00000000-0000'.length;
const SUN_PATH_MAX = { darwin: 103, linux: 107 };

/**
 * Hosts whose isolated daemon socket (`<worktree>/.native-qa/<runId>[-s]/runtime/daemon.sock`)
 * would exceed the platform's unix socket path limit, so the daemon could never bind.
 */
export function socketPathProblems(config) {
  const hosts = [['sessionHost', config.sessionHost, SESSION_PROFILE_SUFFIX], ...['macos', 'windows'].filter((name) => runLanes(config).includes(name)).map((name) => [`hosts.${name}`, config.hosts[name], ''])];
  return hosts.filter(([, host]) => SUN_PATH_MAX[host.platform])
    .map(([key, host, suffix]) => [key, host, Buffer.byteLength(`${host.worktree}/.native-qa/${'x'.repeat(RUN_ID_MAX)}${suffix}/runtime/daemon.sock`)])
    .filter(([, host, bytes]) => bytes > SUN_PATH_MAX[host.platform])
    .map(([key, host, bytes]) => `${key}.worktree ${host.worktree} makes the daemon socket path ${bytes} bytes, over the ${host.platform} limit ${SUN_PATH_MAX[host.platform]}; use a shorter worktree path`);
}

/** The port the relay process itself listens on. */
export const relayListenPort = (config) => {
  const front = config.sessionHost.relayFront;
  return front.kind === 'tailscale-serve' ? front.backendPort : Number(new URL(config.relayOrigin).port || 443);
};

/**
 * Problems of a live GUI process against the run's isolated profile, empty when it is proven
 * isolated. `env` (`KEY=VALUE` entries, null when the OS hides another process's environment,
 * as macOS does) must hold every expected key with exactly the run's value. `openPaths` (the
 * files the process holds open) must include one inside the profile's data, runtime or home
 * dir (the launcher's own log fds under evidence/ do not count) and none under the owner's
 * roots. A GUI that runs on the owner's HOME or data dir is never driven.
 */
export function isolationProblems({ env = null, openPaths = [] }, { expected, profile, ownerRoots = [] }) {
  const problems = [];
  if (Array.isArray(env)) {
    const seen = new Map();
    for (const entry of env) {
      const at = String(entry).indexOf('=');
      if (at > 0) seen.set(entry.slice(0, at), entry.slice(at + 1));
    }
    for (const [key, value] of Object.entries(expected)) if (seen.get(key) !== value) problems.push(`${key} is ${JSON.stringify(seen.get(key) ?? null)}, not ${JSON.stringify(value)}`);
  }
  const under = (root, path) => path === root || path.startsWith(`${root}/`) || path.startsWith(`${root}\\`);
  const leaked = openPaths.filter((path) => ownerRoots.some((root) => under(root, path)));
  if (leaked.length) problems.push(`holds the owner's files open: ${leaked.slice(0, 5).join(', ')}`);
  const isolated = ['data', 'runtime', 'home'].map((name) => `${profile}/${name}`);
  if (!openPaths.some((path) => isolated.some((root) => under(root, path)))) problems.push(`holds nothing open under ${profile}/{data,runtime,home}`);
  return problems;
}

/**
 * The hub settings every isolated F3 daemon runs with. A fresh hub database fails closed to
 * "stopped", which leaves the supervisor (decision cards, Replies, reports, rules) off; the run
 * forces it on against a database inside the run's own profile, as the unified-supervisor
 * harness does, and turns quiet hours off so no reply is deferred.
 */
export const hubEnv = (host, profile) => ({ FERRYX_HUB: 'on', FERRYX_HUB_DB: remotePath(host, profile, 'data', 'hub', 'hub.sqlite'), FERRYX_HUB_QUIET_HOURS: 'off' });

/**
 * The JXA that raises exactly one run-owned macOS GUI: the process with this pid, and only when
 * its executable is this run's candidate binary. Any other process (the owner's Ferryx, another
 * app, a reused pid) is reported and never activated.
 */
export function activationScript(pid, executable) {
  if (!Number.isSafeInteger(pid) || pid <= 0) throw new StepFailed('GUI_NOT_FRONT', `not a pid: ${pid}`);
  if (typeof executable !== 'string' || !executable.startsWith('/')) throw new StepFailed('GUI_NOT_FRONT', `not an absolute executable: ${executable}`);
  return [
    "ObjC.import('AppKit');",
    `const a = $.NSRunningApplication.runningApplicationWithProcessIdentifier(${pid});`,
    `const want = ${JSON.stringify(executable)};`,
    "const exe = a.isNil() ? null : ObjC.unwrap(a.executableURL.path);",
    "JSON.stringify(exe === null ? { activated: false, reason: 'absent' } : exe !== want ? { activated: false, reason: 'not the run GUI', executable: exe }",
    "  : (a.unhide, { activated: a.activateWithOptions(3) === true, executable: exe }));",
  ].join('\n');
}

/** A path on `host`, joined with that host's separator. */
export const remotePath = (host, ...parts) => parts.join(host.platform === 'win32' ? '\\' : '/');

const shq = (value) => `'${String(value).replaceAll("'", "'\\''")}'`;
const psq = (value) => `'${String(value).replaceAll("'", "''")}'`;

/**
 * The [executable, argv] that starts `script --rpc` on `host` under its bun: over SSH, or as a
 * direct child when the host is the control host itself (`sshDestination: "local"`).
 */
export function rpcCommand(host, script, sshExecutable = 'ssh') {
  if (host.sshDestination === 'local') return [host.bun, [script, '--rpc']];
  const remote = host.platform === 'win32'
    ? `powershell.exe -NoProfile -NonInteractive -Command "& ${psq(host.bun)} ${psq(script)} --rpc"`
    : `${shq(host.bun)} ${shq(script)} --rpc`;
  return [sshExecutable, ['-T', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', host.sshDestination, remote]];
}

const canonical = (value) => JSON.stringify(value, (_, v) => (v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => a.localeCompare(b))) : v));

/**
 * Problems of a `tailscale serve status --json` change, empty when `after` is exactly `before`
 * plus this run's endpoint: TCP[port] with HTTPS and Web["<hostName>:<port>"] proxying `/` to
 * the loopback backend. Every other entry (the owner's :443, :8443, ...) must be byte-identical.
 */
export function serveAddedProblems(before, after, { hostName, port, backend }) {
  const problems = [];
  const key = `${hostName}:${port}`;
  for (const section of ['TCP', 'Web']) {
    const added = section === 'TCP' ? String(port) : key;
    const was = { ...(before?.[section] ?? {}) };
    const now = { ...(after?.[section] ?? {}) };
    if (added in was) problems.push(`${section}.${added} existed before this run`);
    delete now[added];
    if (canonical(was) !== canonical(now)) problems.push(`${section} entries other than ${added} changed`);
  }
  if (after?.TCP?.[String(port)]?.HTTPS !== true) problems.push(`TCP.${port} is not HTTPS`);
  const proxy = after?.Web?.[key]?.Handlers?.['/']?.Proxy;
  if (proxy !== `http://127.0.0.1:${backend}`) problems.push(`Web.${key} / proxies ${proxy}, not http://127.0.0.1:${backend}`);
  const rest = (value) => canonical(Object.fromEntries(Object.entries(value ?? {}).filter(([k]) => k !== 'TCP' && k !== 'Web')));
  if (rest(before) !== rest(after)) problems.push('serve settings other than TCP and Web changed');
  return problems;
}

/** Whether two serve status documents are the same config (teardown restored the snapshot). */
export const sameServeConfig = (a, b) => canonical(a ?? {}) === canonical(b ?? {});

const xml = (value) => String(value).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');

/**
 * The files of the run-owned macOS launcher bundle (relative path -> body). LaunchServices
 * (`open -n`) starts it in the logged-in Aqua session, which an SSH session cannot reach; its
 * executable replaces itself with `bun native-remote-agent.mjs --launch-gui <spec>`, which
 * checks the candidate hash and isolated env, spawns the GUI and writes the exact-pid launch and
 * exit receipts. LSUIElement keeps the launcher itself out of the Dock.
 */
export function guiBundleFiles({ bundleId, bun, agent, specPath, logPath }) {
  if (!/^[A-Za-z0-9.-]+$/.test(bundleId ?? '')) throw new StepFailed('GUI_BUNDLE', `bundle id ${bundleId} is not [A-Za-z0-9.-]`);
  for (const [name, value] of Object.entries({ bun, agent, specPath, logPath })) {
    if (typeof value !== 'string' || !value.startsWith('/')) throw new StepFailed('GUI_BUNDLE', `${name} ${value} is not an absolute path`);
  }
  const plist = ['<?xml version="1.0" encoding="UTF-8"?>',
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">',
    '<plist version="1.0"><dict>',
    '<key>CFBundleExecutable</key><string>f3-launch</string>',
    `<key>CFBundleIdentifier</key><string>${xml(bundleId)}</string>`,
    '<key>CFBundleName</key><string>F3Launch</string>',
    '<key>CFBundlePackageType</key><string>APPL</string>',
    '<key>LSUIElement</key><true/>',
    '</dict></plist>', ''].join('\n');
  const bunDir = bun.slice(0, bun.lastIndexOf('/'));
  const script = ['#!/bin/sh',
    `PATH=${shq(`/usr/bin:/bin:/usr/sbin:/sbin:${bunDir}`)}; export PATH`,
    `exec ${shq(bun)} ${shq(agent)} --launch-gui ${shq(specPath)} >> ${shq(logPath)} 2>&1`, ''].join('\n');
  return { 'Contents/Info.plist': plist, 'Contents/MacOS/f3-launch': script };
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
