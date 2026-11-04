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
  for (const name of desktops.filter((n) => config.hosts[n].platform === 'win32')) {
    const typed = untypeableShortNames(config.hosts[name].worktree);
    if (typed.length) problems.push(`hosts.${name}.worktree ${config.hosts[name].worktree}: ${typed.join(', ')} would put '-' or '_' into the launcher's 8.3 path, which MahoRD cannot type into the Run dialog; use a name whose first 6 characters have neither`);
  }
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
/**
 * A run id: `f3MMDDhhmmxxxx` (UTC time, 4 hex). Short for the unix socket limit, and free of '-'
 * and '_' in its first 6 characters, so its Windows 8.3 name (`F31104~1`) stays typeable: the
 * Windows lane types the launcher's 8.3 path into the Run dialog and MahoRD drops '-' and '_'.
 */
export const newRunId = (now, hex) => `f3${now.toISOString().replace(/[^0-9]/g, '').slice(4, 12)}${hex.replace(/[^0-9a-f]/g, '').slice(0, 4)}`;
/** The longest run id `newRunId` makes. */
const RUN_ID_MAX = 'f3MMDDhhmmxxxx'.length;

/**
 * Path components whose Windows 8.3 short name would hold '-' or '_'. A component that is
 * already a short name (<= 8 + 3 chars, one dot at most, not leading) keeps its own spelling;
 * any other becomes its first 6 characters (dots and spaces dropped) plus `~N`.
 */
export function untypeableShortNames(path) {
  return String(path).split(/[\\/]/).slice(1).filter(Boolean).filter((name) => {
    const short = /^[^. ]{1,8}(\.[^. ]{1,3})?$/.test(name) ? name : name.replace(/[. ]/g, '').slice(0, 6);
    return /[-_]/.test(short);
  });
}
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
 * files the process holds open; null where the OS gives no unprivileged list, as Windows does)
 * must include one inside the profile's data, runtime or home dir (the launcher's own log fds
 * under evidence/ do not count) and none under the owner's roots. At least one of the two must
 * have been read. A GUI that runs on the owner's HOME or data dir is never driven.
 */
export function isolationProblems({ env = null, openPaths = [] }, { expected, profile, ownerRoots = [] }) {
  const problems = [];
  if (!Array.isArray(env) && !Array.isArray(openPaths)) return ['neither the environment nor the open files of the live GUI could be read'];
  if (Array.isArray(env)) {
    const seen = new Map();
    for (const entry of env) {
      const at = String(entry).indexOf('=');
      if (at > 0) seen.set(entry.slice(0, at), entry.slice(at + 1));
    }
    // Windows environment names are case-insensitive; an exact-case key wins when both exist.
    const lookup = (key) => (seen.has(key) ? seen.get(key) : [...seen].find(([k]) => k.toUpperCase() === key.toUpperCase())?.[1]);
    for (const [key, value] of Object.entries(expected)) if (lookup(key) !== value) problems.push(`${key} is ${JSON.stringify(lookup(key) ?? null)}, not ${JSON.stringify(value)}`);
  }
  if (!Array.isArray(openPaths)) return problems;
  const under = (root, path) => path === root || path.startsWith(`${root}/`) || path.startsWith(`${root}\\`);
  const leaked = openPaths.filter((path) => ownerRoots.some((root) => under(root, path)));
  if (leaked.length) problems.push(`holds the owner's files open: ${leaked.slice(0, 5).join(', ')}`);
  // Open files, where listed (macOS, Linux), must include one under the profile.
  const isolated = ['data', 'runtime', 'home'].map((name) => `${profile}/${name}`);
  if (!openPaths.some((path) => isolated.some((root) => under(root, path)))) problems.push(`holds nothing open under ${profile}/{data,runtime,home}`);
  return problems;
}

/**
 * Windows: the PowerShell that reads one live process's environment block from its PEB (same
 * user, PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, no admin grant) and prints it as base64 of
 * UTF-8, entries separated by NUL. x64 offsets: PEB.ProcessParameters 0x20,
 * RTL_USER_PROCESS_PARAMETERS.Environment 0x80 and EnvironmentSize 0x3F0. Read-only.
 */
export function windowsEnvScript(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) throw new StepFailed('GUI_NOT_ISOLATED', `not a pid: ${pid}`);
  return [
    "$ErrorActionPreference = 'Stop'",
    'Add-Type -TypeDefinition @"',
    'using System; using System.Runtime.InteropServices; using System.Text;',
    'public static class F3ProcEnv {',
    '  [StructLayout(LayoutKind.Sequential)] struct Pbi { public IntPtr Exit; public IntPtr Peb; public IntPtr Affinity; public IntPtr Priority; public IntPtr Pid; public IntPtr Parent; }',
    '  [DllImport("ntdll.dll")] static extern int NtQueryInformationProcess(IntPtr h, int c, ref Pbi p, int l, out int r);',
    '  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint a, bool i, int pid);',
    '  [DllImport("kernel32.dll", SetLastError = true)] static extern bool ReadProcessMemory(IntPtr h, IntPtr a, byte[] b, IntPtr n, out IntPtr r);',
    '  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);',
    '  [DllImport("kernel32.dll")] static extern bool IsWow64Process(IntPtr h, out bool w);',
    '  static byte[] Mem(IntPtr h, long at, int n) { var b = new byte[n]; IntPtr got; if (!ReadProcessMemory(h, new IntPtr(at), b, new IntPtr(n), out got) || got.ToInt64() != n) throw new Exception("ReadProcessMemory " + Marshal.GetLastWin32Error()); return b; }',
    '  public static string Read(int pid) {',
    '    IntPtr h = OpenProcess(0x0410, false, pid);',
    '    if (h == IntPtr.Zero) throw new Exception("OpenProcess " + Marshal.GetLastWin32Error());',
    '    try {',
    '      bool wow; if (IsWow64Process(h, out wow) && wow) throw new Exception("32-bit process");',
    '      var p = new Pbi(); int len; int st = NtQueryInformationProcess(h, 0, ref p, Marshal.SizeOf(p), out len);',
    '      if (st != 0) throw new Exception("NtQueryInformationProcess " + st);',
    '      long pp = BitConverter.ToInt64(Mem(h, p.Peb.ToInt64() + 0x20, 8), 0);',
    '      long env = BitConverter.ToInt64(Mem(h, pp + 0x80, 8), 0);',
    '      long size = BitConverter.ToInt64(Mem(h, pp + 0x3F0, 8), 0);',
    '      if (size <= 0 || size > (1 << 24)) throw new Exception("environment size " + size);',
    '      return Encoding.Unicode.GetString(Mem(h, env, (int)size));',
    '    } finally { CloseHandle(h); }',
    '  }',
    '}',
    '"@',
    `[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([F3ProcEnv]::Read(${pid})))`,
  ].join('\r\n');
}

/** `KEY=VALUE` entries of a `windowsEnvScript` answer (drive-cwd `=C:=...` entries dropped). */
export const parseEnvBlock = (base64) => Buffer.from(String(base64).trim(), 'base64').toString('utf8').split('\0').filter((entry) => entry && !entry.startsWith('='));

/**
 * The hub settings every isolated F3 daemon runs with. A fresh hub database fails closed to
 * "stopped", which leaves the supervisor (decision cards, Replies, reports, rules) off; the run
 * forces it on against a database inside the run's own profile, as the unified-supervisor
 * harness does, and turns quiet hours off so no reply is deferred.
 */
// The inbox watchers stay off: they spawn the owner's `imsg`/`zele` from PATH and poll a remote
// kakao endpoint (ferryx-hub inbox_watch.rs inbox_enabled), which no F3 scenario drives.
export const hubEnv = (host, profile) => ({ FERRYX_HUB: 'on', FERRYX_HUB_DB: remotePath(host, profile, 'data', 'hub', 'hub.sqlite'), FERRYX_HUB_QUIET_HOURS: 'off', OMON_INBOX_ENABLED: '0' });

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
    // activateWithOptions is only a request (macOS cooperative activation may ignore it from a
    // background process), so success is the frontmost application being this exact pid, read
    // back; System Events (the same pid, after the same executable check) is the fallback.
    "const front = () => ObjC.unwrap($.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier);",
    `const settle = () => { for (let i = 0; i < 20 && front() !== ${pid}; i += 1) delay(0.1); return front(); };`,
    "let result;",
    "if (exe === null) result = { activated: false, reason: 'absent' };",
    "else if (exe !== want) result = { activated: false, reason: 'not the run GUI', executable: exe };",
    "else {",
    "  a.unhide; a.activateWithOptions(3); let frontPid = settle(); let via = 'NSRunningApplication';",
    `  if (frontPid !== ${pid}) { try { const se = Application('System Events'); const p = se.processes.whose({ unixId: ${pid} })(); if (p.length === 1) { p[0].frontmost = true; via = 'System Events'; } } catch (e) { via = 'System Events failed: ' + e; } frontPid = settle(); }`,
    `  result = { activated: frontPid === ${pid}, frontPid, via, executable: exe };`,
    "}",
    "JSON.stringify(result);",
  ].join('\n');
}

/**
 * The JXA that reads the on-screen bounds (points, top-left origin) of the largest normal window
 * of exactly one run-owned GUI, with the main screen size, so a frame can be cut to that window.
 * The same executable check as `activationScript`; another process is reported, never read.
 */
export function windowBoundsScript(pid, executable) {
  activationScript(pid, executable);
  return [
    "ObjC.import('AppKit'); ObjC.import('CoreGraphics');",
    `const a = $.NSRunningApplication.runningApplicationWithProcessIdentifier(${pid});`,
    `const want = ${JSON.stringify(executable)};`,
    "const exe = a.isNil() ? null : ObjC.unwrap(a.executableURL.path);",
    "const list = exe === want ? (ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0))) || []) : [];",
    `const mine = list.filter((w) => w.kCGWindowOwnerPID === ${pid} && w.kCGWindowLayer === 0 && w.kCGWindowBounds).map((w) => w.kCGWindowBounds).sort((p, q) => q.Width * q.Height - p.Width * p.Height);`,
    "const f = $.NSScreen.mainScreen.frame;",
    "JSON.stringify(exe === null ? { bounds: null, reason: 'absent' } : exe !== want ? { bounds: null, reason: 'not the run GUI', executable: exe }",
    "  : { bounds: mine[0] ? { x: mine[0].X, y: mine[0].Y, width: mine[0].Width, height: mine[0].Height } : null, screen: { width: f.size.width, height: f.size.height },",
    "      frontPid: ObjC.unwrap($.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier) });",
  ].join('\n');
}

/**
 * The JXA that selects keyboard input source `id` (e.g. com.apple.keylayout.ABC) and returns the
 * one selected before plus the one selected after. MahoRD types ASCII as key codes, so an input
 * method (Korean 2-Set) would compose them into other text; the lane selects ABC while it types
 * and selects the previous source again afterwards.
 */
export function inputSourceScript(id) {
  if (typeof id !== 'string' || !/^[A-Za-z0-9.]+$/.test(id)) throw new StepFailed('INPUT_SOURCE', `not an input source id: ${id}`);
  return [
    "ObjC.import('Carbon');",
    'const idOf = (s) => ObjC.unwrap(ObjC.castRefToObject($.TISGetInputSourceProperty(s, $.kTISPropertyInputSourceID)));',
    'const previous = idOf($.TISCopyCurrentKeyboardInputSource());',
    'const list = ObjC.castRefToObject($.TISCreateInputSourceList($(), false));',
    'let found = null;',
    `for (let i = 0; i < list.count; i += 1) if (idOf(list.objectAtIndex(i)) === ${JSON.stringify(id)}) found = list.objectAtIndex(i);`,
    'const status = found ? $.TISSelectInputSource(found) : null;',
    'JSON.stringify({ previous, current: idOf($.TISCopyCurrentKeyboardInputSource()), status, found: Boolean(found) });',
  ].join('\n');
}

/**
 * The mhc engine the isolated desktop daemon runs (FERRYX_HUB_ENV_FILE): the supervisor on, with
 * the loopback stub provider only (no real model or network). `agentDir` gets models.json and
 * mcp.json (built-in remote MCP servers off); the env keys keep the engine offline.
 */
export const ENGINE_PROVIDER = 'qa-stub';
export const ENGINE_MODEL = 'qa-supervisor-1';
export function engineFiles({ baseUrl, mhcBin }) {
  return {
    models: { providers: { [ENGINE_PROVIDER]: { api: 'openai-completions', baseUrl, apiKey: 'qa-loopback-fixture',
      models: [{ id: ENGINE_MODEL, contextWindow: 200_000, maxTokens: 8192 }] } } },
    mcp: { mcpServers: { context7: { enabled: false }, grep_app: { enabled: false } } },
    envFile: ['FERRYX_HUB_MHC_SUPERVISOR=1', `FERRYX_HUB_MHC_BIN=${mhcBin}`, `FERRYX_HUB_MHC_PROVIDER=${ENGINE_PROVIDER}`, `FERRYX_HUB_MHC_MODEL=${ENGINE_MODEL}`, ''].join('\n'),
    offline: { DO_NOT_TRACK: '1', OMO_DISABLE_POSTHOG: '1', MAHO_OFFLINE: '1', MAHO_SKIP_VERSION_CHECK: '1' },
  };
}

/** The Reply text the stub provider gives the F3 owner prompt: it names the prompt's token. */
export const stubReply = (promptText) => {
  const token = /\bf3tok[a-z0-9]+\b/.exec(promptText)?.[0];
  return token ? `Reply for ${token} is ready` : null;
};

/**
 * The owner request that binds the F3 report pane to a goal (`supervisorDispatch` submit with
 * the pane): a worker report is stored only for a pane an open goal is bound to
 * (ferryx-hub lanes/dispatch.rs report_from_worker, NotBound).
 */
export const reportGoalRequest = (runId, sessionId) => ({
  type: 'supervisorDispatch',
  request: { action: 'submit', messageId: `f3-report-goal-${runId}`, body: 'Review the F3 migration and report back.', hostId: 'local', sessionId,
    work: { title: 'Review the F3 migration', instruction: 'Review the migration and send one report.', requirements: [{ id: 'r1', text: 'one report is sent', verifier: 'the report is in the transcript' }], operations: [] } },
});

/** The frame-pixel rectangle of window `bounds` (screen points) in a frame `frameWidth` pixels wide. */
export function windowRect(bounds, screen, frameWidth) {
  if (!bounds || !screen?.width) return null;
  const k = frameWidth / screen.width;
  return { x: bounds.x * k, y: bounds.y * k, width: bounds.width * k, height: bounds.height * k };
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
