// Daemon data-protocol and control-v1 clients over Unix sockets (macOS/Linux) or loopback TCP
// with a bearer token (Windows). Responses and stream messages are routed by event, never polled.

import net from 'node:net';
import { readFile } from 'node:fs/promises';
import { data as D, control as C, encodeLine, splitLines, withToken, endpoints } from './wire.mjs';
import { withDeadline } from './proc.mjs';

async function openEndpoint(ep) {
  if (ep.kind === 'unix') return { socket: net.createConnection(ep.path), token: null };
  const port = Number((await readFile(ep.portFile, 'utf8')).trim());
  const token = (await readFile(ep.tokenFile, 'utf8')).trim();
  return { socket: net.createConnection({ host: '127.0.0.1', port }), token };
}

class LineConnection {
  constructor(socket, token, label) {
    this.socket = socket;
    this.token = token;
    this.label = label;
    this.pending = [];
    this.queue = [];
    this.streamHandlers = new Set();
    this.closed = false;
    this.closedPromise = new Promise((resolve) => { this.onClosed = resolve; });
    let buffer = '';
    socket.setEncoding('utf8');
    socket.on('data', (chunk) => {
      buffer += chunk;
      let values;
      [values, buffer] = splitLines(buffer);
      for (const v of values) this.dispatch(v);
    });
    const fail = (err) => {
      if (this.closed) return;
      this.closed = true;
      for (const p of this.pending.splice(0)) p.reject(err);
      this.onClosed({ at: Date.now(), error: err?.message ?? null });
    };
    socket.on('error', fail);
    socket.on('close', () => fail(new Error(`${label} closed`)));
  }

  dispatch(v) {
    if (v && (v.type === 'output' || v.type === 'agentState' || v.type === 'event' || v.type === 'exit' || v.type === 'replayGap')) {
      for (const h of this.streamHandlers) h(v);
      return;
    }
    const next = this.pending.shift();
    if (next) next.resolve(v); else this.queue.push(v);
  }

  connected() {
    return withDeadline(new Promise((resolve, reject) => {
      if (this.socket.readyState === 'open') return resolve();
      this.socket.once('connect', resolve);
      this.socket.once('error', reject);
    }), 10_000, `connect ${this.label}`);
  }

  call(frame, deadlineMs = 30_000) {
    if (this.closed) return Promise.reject(new Error(`${this.label} closed`));
    const response = withDeadline(new Promise((resolve, reject) => {
      if (this.queue.length) resolve(this.queue.shift()); else this.pending.push({ resolve, reject });
    }), deadlineMs, `${this.label} ${frame.type ?? frame.op}`);
    const out = this.token ? withToken(frame, this.token) : frame;
    this.token = this.tokenEveryFrame ? this.token : null;
    this.socket.write(encodeLine(out));
    return response;
  }

  onStream(handler) {
    this.streamHandlers.add(handler);
    return () => this.streamHandlers.delete(handler);
  }

  close() {
    this.socket.destroy();
  }
}

/** Data protocol client. Throws on mismatch unless allowMismatch; returns { conn, handshake }. */
export async function connectData(runtimeDir, platform, { version = 5, allowMismatch = false } = {}) {
  const ep = endpoints(runtimeDir, platform).data;
  const { socket, token } = await openEndpoint(ep);
  const conn = new LineConnection(socket, token, 'data');
  conn.tokenEveryFrame = true; // the data protocol checks the token on every Windows line
  await conn.connected();
  const handshake = await conn.call(D.handshake(version));
  if (handshake.type !== 'handshakeOk' && !allowMismatch) {
    conn.close();
    throw new Error(`data handshake refused: ${JSON.stringify(handshake)}`);
  }
  return { conn, handshake };
}

/** control-v1 client; returns null when the endpoint is missing (pre-plan daemon). */
export async function connectControl(runtimeDir, platform) {
  const ep = endpoints(runtimeDir, platform).control;
  let opened;
  try {
    opened = await openEndpoint(ep);
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
  const conn = new LineConnection(opened.socket, opened.token, 'control');
  try {
    await conn.connected();
  } catch (error) {
    if (['ENOENT', 'ECONNREFUSED'].includes(error.code)) return null;
    throw error;
  }
  return conn;
}

export async function controlHello(runtimeDir, platform) {
  const conn = await connectControl(runtimeDir, platform);
  if (!conn) return null;
  try { return await conn.call(C.hello()); } finally { conn.close(); }
}

/** Attaches and resolves when the output stream contains marker (count tracked). */
export async function attachAndWatch(runtimeDir, platform, sessionId) {
  const { conn } = await connectData(runtimeDir, platform);
  let text = '';
  const waiters = new Set();
  conn.onStream((m) => {
    if (m.type !== 'output' || (m.sessionId && m.sessionId !== sessionId)) return;
    text += Buffer.from(m.data, 'base64').toString();
    for (const w of waiters) if (text.includes(w.marker)) { waiters.delete(w); w.resolve(text); }
  });
  const attached = await conn.call(D.attach(sessionId, 0));
  if (attached.type !== 'attachOk') { conn.close(); throw new Error(`attach failed: ${JSON.stringify(attached)}`); }
  text += Buffer.from(attached.history ?? '', 'base64').toString();
  return {
    conn,
    attached,
    text: () => text,
    waitFor(marker, deadlineMs = 15_000) {
      if (text.includes(marker)) return Promise.resolve(text);
      return withDeadline(new Promise((resolve) => waiters.add({ marker, resolve })), deadlineMs, `output ${marker}`);
    },
    count: (marker) => text.split(marker).length - 1,
    close: () => conn.close(),
  };
}

/**
 * Holds a control connection open; its 'closed' promise is the daemon-exit event (the daemon is
 * the only peer of the socket). Hello first so we know whose exit we are watching.
 */
export async function watchDaemonExit(runtimeDir, platform) {
  const conn = await connectControl(runtimeDir, platform);
  if (!conn) return null;
  const hello = await conn.call(C.hello());
  return { hello, exited: conn.closedPromise, close: () => conn.close() };
}
