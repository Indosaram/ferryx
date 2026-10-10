#!/usr/bin/env node
// Harness-side daemon control-wire client: the READ-ONLY session inventory.
//
// WHY THIS EXISTS (task-9 pass-13). The pre-split pane's backend session
// identity has to come from an honest producer. The product's own presentation
// receipt is the preferred one, but its producer requires the seven-field
// `attachTuple` and `pane_liveness_presentation_receipt` returns `None` - so it
// emits NOTHING AT ALL - when there is no attach tuple
// (`src-tauri/src/native_terminal/surface_host.rs`, read in pass 13). If a
// UI-created pane's native surface never attaches, the receipt stream stays
// empty, and three of the seven tuple fields (`frontendSessionId`,
// `paneIdentity`, `bindingKey`) are frontend-owned identities the daemon has no
// concept of, so no harness-side code may synthesise them. The second source is
// therefore a MEASURED one: the isolated daemon's own session inventory, read
// over the same control wire the product speaks, before and after the pane
// click. The delta names exactly the session the click created.
//
// READ-ONLY BY CONSTRUCTION. The only requests this module can send are
// `handshake` and `listSessions`; it never spawns, cancels, writes, resizes or
// mutates anything, and `READ_ONLY_REQUEST_TYPES` below is what the unit test
// asserts a fake daemon ever receives.
//
// FRAMING IS READ FROM THE PRODUCT, NOT GUESSED:
//   * Newline-delimited JSON: one request object per line, one response object
//     per line. The client serialises the handshake and appends `'\n'` before
//     writing (`src-tauri/src/daemon/client.rs:2117-2139`, `json.push('\n')`
//     then `write_all` + `flush`) and reads exactly one line back
//     (`client.rs:2141-2152`, `reader.read_line`). The server side reads the
//     connection with `reader.read_line` too (`src-tauri/src/daemon/server.rs:3253`).
//   * Tag spelling: both enums are `#[serde(tag = "type", rename_all =
//     "camelCase")]` (`protocol.rs:412` on `DaemonRequest`, `:691` on
//     `DaemonResponse`), so the request tags are `handshake` / `listSessions`
//     and the response tags are `handshakeOk` / `listSessionsOk` / `error` /
//     `protocolMismatch`. The product's own tests pin this shape:
//     `protocol.rs:1727-1758` asserts `{"type":"handshake","version":5}` with an
//     optional top-level `"token"` (`transport_token_from_line`,
//     `server.rs`), and `:1782` asserts
//     `{"type":"handshakeOk","version":3,...}`; `:1799-1806` serialises
//     `ListSessionsOk { epoch, sessions }`.
//   * Handshake fields: `version` (`DAEMON_PROTOCOL_VERSION = 5`,
//     `protocol.rs:14`) and, on the loopback transport only, a top-level
//     `token` (`protocol.rs:420-427`).
//   * Session-list fields: `listSessionsOk { epoch: u64, sessions: Vec<String> }`
//     (`protocol.rs:830-833`), answered by
//     `Ok(DaemonRequest::ListSessions) => self.session_router.list_sessions()`
//     (`server.rs:3791-3796`).
//
// TRANSPORT RESOLUTION (both platforms, mirroring the product):
//   * POSIX: the endpoint IS the Unix socket `<runtime>/daemon.sock`
//     (`server.rs:476-479` `get_socket_path`, `client.rs:1876-1879`
//     `DaemonStream::connect(path)`); no token is presented because the socket's
//     ownership and mode are the credential (`client.rs` `read_transport_token`,
//     `#[cfg(unix)] => None`).
//   * Windows: the endpoint file is `<runtime>/daemon.port`, holding the numeric
//     loopback port (`server.rs:481-484`), and the client reads it and connects
//     `127.0.0.1:<port>` (`client.rs:1881-1892`). A loopback port has no
//     filesystem ownership boundary, so the first frame must also carry this
//     boot's `<runtime>/daemon.token` (`server.rs:1163-1167`,
//     `client.rs:196-219`), which the server checks before dispatching anything
//     and rejects with `TRANSPORT_UNAUTHORIZED` (`server.rs:3264-3276`).
//
// EVERY BLOCKING STEP IS BOUNDED (connect, per-read, and a total deadline), so a
// hung or wedged daemon produces a typed failure result, never a hang - the
// pass-13 trap list records that the PowerShell probe's `TcpClient.Connect` has
// no timeout, and the Node equivalent needs an explicit bound of its own.

import { existsSync, readFileSync } from 'node:fs';
import { connect as connectTransport } from 'node:net';
import { join } from 'node:path';
import { BUDGETS } from './common-harness.mjs';

// Mirrors `DAEMON_PROTOCOL_VERSION` (`src-tauri/src/daemon/protocol.rs:14`).
export const DAEMON_PROTOCOL_VERSION = 5;

// The complete request vocabulary this client may put on the wire. Read-only:
// there is no spawn/cancel/write/resize/close in this list, and the unit test
// asserts a fake daemon only ever receives these.
export const READ_ONLY_REQUEST_TYPES = Object.freeze(['handshake', 'listSessions']);

// Typed failure identities of the inventory read. These are RESULT codes on a
// non-throwing API, not harness verdicts: a read that cannot be taken simply
// leaves the inventory source unavailable, and the pane binding then fails with
// its own existing typed code (`PANE_BINDING_UNBOUND`) carrying this reason in
// the detail. Nothing here can turn a blocked run into a pass.
export const DAEMON_INVENTORY_FAILURES = Object.freeze([
  'DAEMON_RUNTIME_MISSING',
  'DAEMON_PORT_INVALID',
  'DAEMON_TOKEN_MISSING',
  'DAEMON_CONNECT_TIMEOUT',
  'DAEMON_CONNECT_FAILED',
  'DAEMON_READ_TIMEOUT',
  'DAEMON_DEADLINE',
  'DAEMON_FRAME_TOO_LARGE',
  'DAEMON_BAD_FRAME',
  'DAEMON_UNAUTHORIZED',
  'DAEMON_PROTOCOL_MISMATCH',
]);

// `server.rs:476-484` + `server.rs:1162-1167`: the two endpoint shapes, resolved
// the way the product resolves them. No token value is ever returned here - the
// credential is read at connect time and never recorded.
export function daemonEndpointForRuntimeDir(runtimeDir, platform = process.platform) {
  if (platform === 'win32') {
    return {
      transport: 'loopback-port',
      portPath: join(runtimeDir, 'daemon.port'),
      tokenPath: join(runtimeDir, 'daemon.token'),
    };
  }
  return { transport: 'unix-socket', socketPath: join(runtimeDir, 'daemon.sock') };
}

// The evidence-safe description of an endpoint: paths only, never the token.
export function describeDaemonEndpoint(endpoint) {
  return endpoint.transport === 'loopback-port'
    ? { transport: endpoint.transport, portPath: endpoint.portPath, tokenPath: endpoint.tokenPath }
    : { transport: endpoint.transport, socketPath: endpoint.socketPath };
}

function readText(path) {
  try {
    return readFileSync(path, 'utf8');
  } catch {
    return null;
  }
}

// One buffered line reader over a connected socket: resolves the first complete
// `\n`-terminated line, and fails typed on a torn/oversized frame or a socket
// that closes before it answers.
function createLineReader(socket, maxFrameBytes) {
  let buffer = '';
  let failure = null;
  const waiting = [];

  const settleAll = error => {
    if (failure) return;
    failure = error;
    while (waiting.length > 0) waiting.shift()({ ok: false, ...error });
  };

  const parseLine = line => {
    const trimmed = line.replace(/\r+$/, '').trim();
    if (trimmed.length === 0) return { ok: false, code: 'DAEMON_BAD_FRAME', detail: 'daemon sent an empty line' };
    try {
      return { ok: true, frame: JSON.parse(trimmed) };
    } catch (error) {
      return { ok: false, code: 'DAEMON_BAD_FRAME', detail: `daemon sent an unparseable line ${JSON.stringify(trimmed.slice(0, 200))}: ${error?.message ?? error}` };
    }
  };

  function drain() {
    let index = buffer.indexOf('\n');
    while (waiting.length > 0 && index !== -1) {
      const line = buffer.slice(0, index);
      buffer = buffer.slice(index + 1);
      waiting.shift()(parseLine(line));
      index = buffer.indexOf('\n');
    }
  }

  socket.setEncoding('utf8');
  socket.on('data', chunk => {
    if (failure) return;
    buffer += chunk;
    if (buffer.length > maxFrameBytes) {
      settleAll({ code: 'DAEMON_FRAME_TOO_LARGE', detail: `daemon response exceeded ${maxFrameBytes} bytes without a line terminator` });
      return;
    }
    drain();
  });
  socket.on('error', error => settleAll({ code: 'DAEMON_CONNECT_FAILED', detail: `daemon transport error: ${error?.message ?? error}` }));
  socket.on('close', () => settleAll({ code: 'DAEMON_CONNECT_FAILED', detail: 'daemon closed the connection before answering' }));

  return {
    readLine(boundMs) {
      return new Promise(resolve => {
        let settled = false;
        const entry = value => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          resolve(value);
        };
        const timer = setTimeout(() => {
          const index = waiting.indexOf(entry);
          if (index !== -1) waiting.splice(index, 1);
          entry({ ok: false, code: 'DAEMON_READ_TIMEOUT', detail: `daemon did not answer within ${boundMs}ms` });
        }, boundMs);
        timer.unref?.();
        waiting.push(entry);
        // A line that arrived before this read (the daemon wrote its response and
        // then closed, so the data event ran with no waiter registered) is
        // consumed FIRST: a response the daemon really sent must never be lost to
        // the close that followed it.
        drain();
        if (settled) return;
        if (failure) {
          const index = waiting.indexOf(entry);
          if (index !== -1) waiting.splice(index, 1);
          entry({ ok: false, ...failure });
        }
      });
    },
  };
}

// Read the isolated daemon's session inventory. Never throws: the caller gets
// either `{ ok: true, sessions, epoch }` or a typed failure result.
export async function readDaemonSessionInventory({
  runtimeDir,
  platform = process.platform,
  connectTimeoutMs = BUDGETS.daemonInventoryConnectMs,
  readTimeoutMs = BUDGETS.daemonInventoryReadMs,
  totalMs = BUDGETS.daemonInventoryTotalMs,
  maxFrameBytes = 1 << 20,
  now = () => Date.now(),
  connect = connectTransport,
} = {}) {
  const startedAt = now();
  const deadlineAt = startedAt + totalMs;
  const remaining = () => deadlineAt - now();
  const endpoint = daemonEndpointForRuntimeDir(runtimeDir, platform);
  const described = describeDaemonEndpoint(endpoint);
  const elapsed = () => now() - startedAt;
  const fail = (code, detail) => ({
    ok: false, code, detail, transport: endpoint.transport, endpoint: described,
    sessions: null, epoch: null, elapsedMs: elapsed(),
  });

  let target;
  let credential = null;
  if (endpoint.transport === 'unix-socket') {
    if (!existsSync(endpoint.socketPath)) return fail('DAEMON_RUNTIME_MISSING', `no daemon socket at ${endpoint.socketPath}`);
    target = { path: endpoint.socketPath };
  } else {
    if (!existsSync(endpoint.portPath)) return fail('DAEMON_RUNTIME_MISSING', `no daemon port file at ${endpoint.portPath}`);
    const rawPort = readText(endpoint.portPath);
    if (rawPort === null) return fail('DAEMON_RUNTIME_MISSING', `daemon port file ${endpoint.portPath} could not be read`);
    const port = Number(rawPort.trim());
    if (!Number.isInteger(port) || port <= 0 || port > 65535) {
      return fail('DAEMON_PORT_INVALID', `daemon port file ${endpoint.portPath} held ${JSON.stringify(rawPort.trim().slice(0, 32))}, which is not a loopback port`);
    }
    // `client.rs` `read_transport_token_at`: an absent file, or one holding only
    // whitespace, is no credential at all - reported as absent rather than
    // presented as an empty token the daemon would reject.
    const rawToken = readText(endpoint.tokenPath);
    credential = typeof rawToken === 'string' ? rawToken.trim() : '';
    if (credential.length === 0) {
      return fail('DAEMON_TOKEN_MISSING', `no transport token at ${endpoint.tokenPath}; the loopback transport authenticates the first frame by this boot's token`);
    }
    target = { host: '127.0.0.1', port };
  }
  const targetLabel = target.path ?? `${target.host}:${target.port}`;

  const boundConnectMs = Math.min(connectTimeoutMs, Math.max(0, remaining()));
  if (boundConnectMs <= 0) return fail('DAEMON_DEADLINE', `total deadline ${totalMs}ms elapsed before the connection could be made`);

  let socket = null;
  const connected = await new Promise(resolve => {
    let settled = false;
    const done = value => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(value);
    };
    const timer = setTimeout(() => done({ error: { code: 'DAEMON_CONNECT_TIMEOUT', detail: `connecting to ${targetLabel} exceeded ${boundConnectMs}ms` } }), boundConnectMs);
    timer.unref?.();
    let pending;
    try {
      pending = connect(target);
    } catch (error) {
      done({ error: { code: 'DAEMON_CONNECT_FAILED', detail: `connect to ${targetLabel} threw: ${error?.message ?? error}` } });
      return;
    }
    socket = pending;
    pending.setNoDelay?.(true);
    pending.once('connect', () => done({ socket: pending }));
    pending.once('error', error => done({ error: { code: 'DAEMON_CONNECT_FAILED', detail: `connect to ${targetLabel} failed: ${error?.message ?? error}` } }));
  });
  if (connected.error) {
    try { socket?.destroy(); } catch { /* nothing to release */ }
    return fail(connected.error.code, connected.error.detail);
  }

  const reader = createLineReader(connected.socket, maxFrameBytes);
  const writeFrame = frame => new Promise(resolve => {
    const boundWriteMs = Math.max(0, remaining());
    if (boundWriteMs <= 0) {
      resolve({ ok: false, code: 'DAEMON_DEADLINE', detail: `total deadline ${totalMs}ms elapsed before a request could be written` });
      return;
    }
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve({ ok: false, code: 'DAEMON_READ_TIMEOUT', detail: `writing a request did not flush within ${boundWriteMs}ms` });
    }, boundWriteMs);
    timer.unref?.();
    connected.socket.write(`${JSON.stringify(frame)}\n`, () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ ok: true });
    });
  });
  const readFrame = () => {
    const bound = Math.min(readTimeoutMs, Math.max(0, remaining()));
    if (bound <= 0) {
      return Promise.resolve({ ok: false, code: 'DAEMON_DEADLINE', detail: `total deadline ${totalMs}ms elapsed before the daemon answered` });
    }
    return reader.readLine(bound);
  };

  try {
    // POSIX presents no token (`client.rs` `read_transport_token` is `None` on
    // unix); Windows presents this boot's token as a top-level field beside
    // `version`, exactly as `client.rs:2117-2121` serialises it.
    const handshake = credential === null
      ? { type: 'handshake', version: DAEMON_PROTOCOL_VERSION }
      : { type: 'handshake', version: DAEMON_PROTOCOL_VERSION, token: credential };
    const wroteHandshake = await writeFrame(handshake);
    if (!wroteHandshake.ok) return fail(wroteHandshake.code, wroteHandshake.detail);

    const handshakeRead = await readFrame();
    if (!handshakeRead.ok) return fail(handshakeRead.code, handshakeRead.detail);
    const handshakeFrame = handshakeRead.frame;
    if (handshakeFrame?.type === 'protocolMismatch') {
      return fail('DAEMON_PROTOCOL_MISMATCH', `daemon expects protocol ${handshakeFrame.expectedVersion} but this harness speaks ${DAEMON_PROTOCOL_VERSION}`);
    }
    if (handshakeFrame?.type === 'error') {
      const code = handshakeFrame.code === 'TRANSPORT_UNAUTHORIZED' ? 'DAEMON_UNAUTHORIZED' : 'DAEMON_BAD_FRAME';
      return fail(code, `daemon refused the handshake: ${JSON.stringify(handshakeFrame)}`);
    }
    if (handshakeFrame?.type !== 'handshakeOk') {
      return fail('DAEMON_BAD_FRAME', `expected handshakeOk, got ${JSON.stringify(handshakeFrame)}`);
    }

    const wroteList = await writeFrame({ type: 'listSessions' });
    if (!wroteList.ok) return fail(wroteList.code, wroteList.detail);

    const listRead = await readFrame();
    if (!listRead.ok) return fail(listRead.code, listRead.detail);
    const listFrame = listRead.frame;
    if (listFrame?.type === 'error') {
      return fail('DAEMON_BAD_FRAME', `daemon answered listSessions with an error: ${JSON.stringify(listFrame)}`);
    }
    if (listFrame?.type !== 'listSessionsOk' || !Array.isArray(listFrame.sessions)) {
      return fail('DAEMON_BAD_FRAME', `expected listSessionsOk with a sessions array, got ${JSON.stringify(listFrame)}`);
    }
    const sessions = listFrame.sessions.filter(id => typeof id === 'string' && id.length > 0);
    return {
      ok: true, code: null, detail: null, transport: endpoint.transport, endpoint: described,
      sessions, epoch: typeof listFrame.epoch === 'number' ? listFrame.epoch : null,
      daemonVersion: typeof handshakeFrame.daemonVersion === 'string' ? handshakeFrame.daemonVersion : null,
      elapsedMs: elapsed(),
    };
  } finally {
    try { connected.socket.destroy(); } catch { /* already closed */ }
  }
}

// The measured delta: the sessions the second inventory reports and the first
// did not, minus the settled fixture sessions (they exist before the click, so
// they can never be the pane the click created - and one re-created inside the
// click window must not be mistaken for it either). Pure and deterministic: the
// candidate lists are sorted so a failure detail names them in a stable order.
export function computeInventoryDelta({ before, after, fixtureSessionIds = [] } = {}) {
  const fixture = new Set(fixtureSessionIds);
  const beforeIds = new Set(Array.isArray(before?.sessions) ? before.sessions : []);
  const afterIds = new Set(Array.isArray(after?.sessions) ? after.sessions : []);
  const added = [...afterIds].filter(id => !beforeIds.has(id) && !fixture.has(id)).sort();
  const removed = [...beforeIds].filter(id => !afterIds.has(id)).sort();
  return {
    added,
    removed,
    beforeCount: beforeIds.size,
    afterCount: afterIds.size,
    fixtureExcluded: [...fixture].sort(),
  };
}

// One line describing an inventory read result, for a failure detail. A blocked
// run has to say WHY the measurement is missing, not only that it is.
export function describeInventoryRead(result) {
  if (!result) return 'not taken';
  if (result.ok === true) {
    return `ok (${result.sessions?.length ?? 0} session(s) over ${result.transport}, epoch ${JSON.stringify(result.epoch)})`;
  }
  return `${result.code ?? 'unknown'}: ${result.detail ?? 'no detail'}`;
}
