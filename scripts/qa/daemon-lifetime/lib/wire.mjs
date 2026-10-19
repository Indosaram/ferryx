// Pure wire helpers: daemon data protocol v5 (JSON lines, serde tag "type", camelCase) and the
// frozen control-v1 channel (src-tauri/src/daemon/control.rs, tests/fixtures/control-v1.json).

export const DATA_PROTOCOL_VERSION = 5;
export const CONTROL_SCHEMA = 1;
export const CONTROL_OPS = Object.freeze(['Hello', 'Inventory', 'RequestUpgrade', 'RetireWhenIdle', 'Shutdown']);
export const HANDOVER_V1_OP = 'handover-v1';

export function encodeLine(value) {
  return `${JSON.stringify(value)}\n`;
}

/** Splits a stream chunk into complete JSON lines, returning [values, rest]. */
export function splitLines(buffer) {
  const values = [];
  let rest = buffer;
  for (;;) {
    const index = rest.indexOf('\n');
    if (index < 0) break;
    const line = rest.slice(0, index).trim();
    rest = rest.slice(index + 1);
    if (line) values.push(JSON.parse(line));
  }
  return [values, rest];
}

/** Windows endpoints take the bearer token on the first frame only. */
export function withToken(frame, token) {
  return token ? { ...frame, token } : frame;
}

// ---- data protocol ----
export const data = Object.freeze({
  handshake: (version = DATA_PROTOCOL_VERSION) => ({ type: 'handshake', version }),
  listSessions: () => ({ type: 'listSessions' }),
  describeLiveSessions: () => ({ type: 'describeLiveSessions' }),
  registerWorkspace: (workspaceId, repoRoot) => ({ type: 'registerWorkspace', workspaceId, repoRoot }),
  spawnLocal: ({ clientRequestId, workspaceId, cwd, shell = null, cols = 80, rows = 24 }) => {
    const req = { type: 'spawn', clientRequestId, workspaceId, worktree: null, cwd, cols, rows };
    if (shell) req.shell = shell;
    return req;
  },
  spawnSsh: ({ clientRequestId, workspaceId, hostStorePath, cols = 80, rows = 24 }) => ({
    type: 'spawn', clientRequestId, workspaceId, worktree: null, cwd: null, cols, rows,
    startup: { kind: 'remoteSsh', hostStorePath },
  }),
  attach: (sessionId, afterSequence = 0) => ({ type: 'attach', sessionId, afterSequence }),
  write: (sessionId, text) => ({ type: 'write', sessionId, data: Buffer.from(text).toString('base64') }),
  close: (sessionId) => ({ type: 'close', sessionId }),
  shutdown: () => ({ type: 'shutdown' }),
});

export function isHandshakeOk(msg) {
  return msg?.type === 'handshakeOk';
}

export function isProtocolMismatch(msg) {
  return msg?.type === 'protocolMismatch' || (msg?.type === 'error' && /mismatch/i.test(String(msg?.code ?? '')));
}

export function hasControlCapability(handshake) {
  return Array.isArray(handshake?.capabilities) && handshake.capabilities.includes('control-v1');
}

/** The live definition of todo 11: reader_open for local, connected for ssh/paired. */
export function isLive(session) {
  if (session?.kind === 'local') return session.readerOpen === true;
  if (session?.kind === 'ssh' || session?.kind === 'paired') return session.connected === true;
  return false;
}

export function liveSessions(describe) {
  if (describe?.type !== 'describeLiveSessionsOk') throw new Error(`not a describeLiveSessionsOk: ${JSON.stringify(describe)}`);
  return describe.sessions.filter(isLive);
}

/** Identity tuple compared across lifecycle events (never remotePid against a local pid). */
export function identityOf(session) {
  if (session.kind === 'local') {
    return { kind: 'local', backendId: session.backendId, childPid: session.localChildPid, childStart: session.localChildStartTime };
  }
  return { kind: session.kind, backendId: session.backendId, remotePid: session.remotePid };
}

export function sameIdentity(a, b) {
  return JSON.stringify(identityOf(a)) === JSON.stringify(identityOf(b));
}

/** Pre/post inventory comparison: every pre live session is still live with the same identity. */
export function compareInventories(before, after) {
  const problems = [];
  const afterById = new Map(after.map((s) => [s.backendId, s]));
  for (const s of before.filter(isLive)) {
    const next = afterById.get(s.backendId);
    if (!next) problems.push(`${s.backendId} disappeared`);
    else if (!isLive(next)) problems.push(`${s.backendId} is no longer live`);
    else if (!sameIdentity(s, next)) problems.push(`${s.backendId} identity changed ${JSON.stringify(identityOf(s))} -> ${JSON.stringify(identityOf(next))}`);
  }
  return problems;
}

/** Output body of a data-protocol "output" stream message. */
export function outputText(msg) {
  return msg?.type === 'output' ? Buffer.from(msg.data, 'base64').toString() : '';
}

// ---- control-v1 ----
export const control = Object.freeze({
  hello: () => ({ control: CONTROL_SCHEMA, op: 'Hello' }),
  inventory: () => ({ control: CONTROL_SCHEMA, op: 'Inventory' }),
  requestUpgrade: (stagedExecutable, generationKey) => ({ control: CONTROL_SCHEMA, op: 'RequestUpgrade', staged_executable: stagedExecutable, generation_key: generationKey }),
  retireWhenIdle: () => ({ control: CONTROL_SCHEMA, op: 'RetireWhenIdle' }),
  shutdown: (confirm) => ({ control: CONTROL_SCHEMA, op: 'Shutdown', confirm_live_sessions: confirm }),
});

export function controlOk(resp) {
  return resp?.ok === true;
}

export function controlCode(resp) {
  return resp?.ok === false ? String(resp.code) : null;
}

export function inventoryTotal(resp) {
  if (!controlOk(resp)) throw new Error(`Inventory failed: ${JSON.stringify(resp)}`);
  return resp.live_local + resp.live_remote + resp.live_paired;
}

export function helloSupportsHandover(resp) {
  return controlOk(resp) && Array.isArray(resp.ops) && resp.ops.includes('RequestUpgrade') && resp.ops.includes(HANDOVER_V1_OP);
}

// ---- endpoints ----
export function endpoints(runtimeDir, platform) {
  if (platform === 'windows') {
    const j = (f) => `${runtimeDir}\\${f}`;
    return {
      data: { kind: 'tcp', portFile: j('daemon.port'), tokenFile: j('daemon.token') },
      control: { kind: 'tcp', portFile: j('control.port'), tokenFile: j('control.token') },
    };
  }
  return {
    data: { kind: 'unix', path: `${runtimeDir}/daemon.sock` },
    control: { kind: 'unix', path: `${runtimeDir}/control.sock` },
  };
}

// ---- generation key (runtime_stage.rs generation_key) ----
export function generationKey(version, sha256Hex) {
  return `${version}-${String(sha256Hex).slice(0, 8).toLowerCase()}`;
}

export function assertDistinctKeys(keys) {
  const values = Object.values(keys);
  if (new Set(values).size !== values.length) throw new Error(`generation keys are not pairwise distinct: ${JSON.stringify(keys)}`);
  return keys;
}
