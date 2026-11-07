// NDJSON helpers for the `ferryx browser pipe` protocol specified in plan maho-browser-merge
// ("Concrete QA contract"). Requests are one JSON object per line with snake_case fields:
//   {"id":1,"op":"observe","tab_id":"<actual-id>"}
//   {"id":2,"op":"act","tab_id":"...","action":{"kind":"click","ref":"..."},"postcondition":{"text":"Saved"}}
//   {"id":3,"op":"navigate","tab_id":"...","url":"http://127.0.0.1:18761/form"}
//   {"id":4,"op":"grant","origin":"http://127.0.0.1:18761","scopes":["read","interact"]}
//   {"id":5,"op":"upload","tab_id":"...","path":"...","ref":"..."}
//   {"id":6,"op":"native","tab_id":"...","action":{...}}
// Responses carry the same id. IDs (tab/controller/approval/ref) always come from earlier
// responses; nothing here hardcodes production IDs. stdout carries frames only; stderr is
// diagnostics. Timeouts race the awaited response/exit signal; nothing sleeps or polls.

/** Frame cap shared with the T6 pipe (Q2 sends a frame larger than this and expects refusal). */
export const MAX_FRAME_BYTES = 1024 * 1024;
export const DEFAULT_TIMEOUT_MS = 15_000;
export const OPS = Object.freeze(['initialize', 'list', 'open', 'observe', 'act', 'navigate', 'grant', 'upload', 'native',
  'screenshot', 'lease', 'release', 'dialog', 'request_help', 'complete', 'capabilities']);

const encoder = new TextEncoder();

export class ProtocolError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.name = 'ProtocolError';
    this.code = code;
    Object.assign(this, details);
  }
}

/** Serializes one request frame and enforces the frame cap unless `allowOversize` (Q2 negative tests). */
export function encodeFrame(message, { maxBytes = MAX_FRAME_BYTES, allowOversize = false } = {}) {
  const bytes = encoder.encode(`${JSON.stringify(message)}\n`);
  if (!allowOversize && bytes.byteLength - 1 > maxBytes) {
    throw new ProtocolError('FRAME_TOO_LARGE', `frame of ${bytes.byteLength - 1} bytes exceeds ${maxBytes}`);
  }
  return bytes;
}

/** Builds a pipe request with snake_case fields; `id` is assigned by the client. */
export function request(op, fields = {}) {
  if (!OPS.includes(op)) throw new ProtocolError('UNKNOWN_OP', `unknown pipe op ${op}`);
  return { op, ...fields };
}

/**
 * Incremental NDJSON splitter with a per-line byte cap. push() returns parsed frames; a line over
 * the cap or invalid JSON yields an error entry instead of throwing so the stream can continue.
 */
export function createLineDecoder({ maxBytes = MAX_FRAME_BYTES } = {}) {
  const decoder = new TextDecoder();
  let buffer = '';
  let discarding = false;
  const parse = (line, out) => {
    if (line.trim() === '') return;
    try { out.push({ frame: JSON.parse(line), raw: line }); } catch (error) { out.push({ error: new ProtocolError('MALFORMED_FRAME', String(error?.message ?? error), { raw: line.slice(0, 256) }) }); }
  };
  return {
    push(chunk) {
      buffer += typeof chunk === 'string' ? chunk : decoder.decode(chunk, { stream: true });
      const out = [];
      if (discarding) {
        // Still inside an oversize line already reported: drop bytes up to its newline.
        const end = buffer.indexOf('\n');
        if (end === -1) { buffer = ''; return out; }
        buffer = buffer.slice(end + 1);
        discarding = false;
      }
      let nl;
      while ((nl = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, nl);
        buffer = buffer.slice(nl + 1);
        if (encoder.encode(line).byteLength > maxBytes) out.push({ error: new ProtocolError('FRAME_TOO_LARGE', `inbound frame exceeds ${maxBytes} bytes`) });
        else parse(line, out);
      }
      if (encoder.encode(buffer).byteLength > maxBytes) {
        out.push({ error: new ProtocolError('FRAME_TOO_LARGE', `inbound frame exceeds ${maxBytes} bytes`) });
        buffer = '';
        discarding = true;
      }
      return out;
    },
    /** Remaining unterminated text at EOF (a protocol violation if non-empty). */
    end() { const rest = buffer + decoder.decode(); buffer = ''; return rest; },
  };
}

export function withTimeout(promise, ms, label, onTimeout) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => { onTimeout?.(); reject(new ProtocolError('TIMEOUT', `deadline ${ms}ms: ${label}`)); }, ms);
    }),
  ]).finally(() => clearTimeout(timer));
}

/**
 * Correlates responses to requests over any byte duplex. `write(bytes)` sends; feed() receives
 * stdout chunks; close(reason) rejects everything still pending. `onTranscript` sees every frame
 * in both directions (callers must redact secrets before persisting).
 */
export function createPipeSession({ write, maxBytes = MAX_FRAME_BYTES, timeoutMs = DEFAULT_TIMEOUT_MS, onTranscript = () => {} }) {
  const decoder = createLineDecoder({ maxBytes });
  const pending = new Map();
  const unsolicited = [];
  const errors = [];
  let nextId = 1;
  let closed = null;

  const settle = ({ frame, error, raw }) => {
    if (error) { errors.push(error); onTranscript({ dir: 'in', error: { code: error.code, message: error.message } }); return; }
    onTranscript({ dir: 'in', frame });
    const entry = frame && Object.hasOwn(frame, 'id') ? pending.get(frame.id) : undefined;
    if (!entry) { unsolicited.push({ frame, raw }); return; }
    pending.delete(frame.id);
    entry.resolve(frame);
  };

  return {
    /** Sends `message` (id assigned unless provided) and resolves with the matching response frame. */
    send(message, { timeout = timeoutMs, allowOversize = false } = {}) {
      if (closed) return Promise.reject(closed);
      const id = message.id ?? nextId++;
      if (pending.has(id)) return Promise.reject(new ProtocolError('DUPLICATE_ID', `request id ${id} already pending`));
      const frame = { ...message, id };
      const bytes = encodeFrame(frame, { maxBytes, allowOversize });
      const response = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
      onTranscript({ dir: 'out', frame: allowOversize ? { id, op: frame.op, oversizeBytes: bytes.byteLength } : frame });
      write(bytes);
      return withTimeout(response, timeout, `pipe ${frame.op} #${id}`, () => pending.delete(id));
    },
    /** Raw bytes for malformed-input tests; there is no response correlation. */
    sendRaw(text) {
      if (closed) throw closed;
      onTranscript({ dir: 'out', raw: text.slice(0, 256), bytes: encoder.encode(text).byteLength });
      write(encoder.encode(text));
    },
    feed(chunk) { for (const item of decoder.push(chunk)) settle(item); },
    close(reason = new ProtocolError('CLOSED', 'pipe closed')) {
      if (closed) return;
      closed = reason;
      const rest = decoder.end();
      if (rest.trim()) errors.push(new ProtocolError('UNTERMINATED_FRAME', 'stdout ended inside a frame', { raw: rest.slice(0, 256) }));
      for (const entry of pending.values()) entry.reject(reason);
      pending.clear();
    },
    pendingCount: () => pending.size,
    unsolicited,
    errors,
  };
}

/** True when a response is a typed refusal (never treat a missing field as success). */
export function isRefusal(response, code) {
  const err = response?.error;
  if (!err) return false;
  return code === undefined || err.code === code;
}

/**
 * Spawns `<cli> browser pipe` (or any argv) with Bun.spawn and wires a pipe session to it.
 * Returns { session, stderr(), exited, close() }; close() ends stdin and awaits exit within the
 * bounded timeout (the Q6 "bounded exit" observable).
 */
export function spawnPipe(argv, { env, cwd, timeoutMs = DEFAULT_TIMEOUT_MS, onTranscript } = {}) {
  const child = Bun.spawn(argv, { env, cwd, stdin: 'pipe', stdout: 'pipe', stderr: 'pipe' });
  const session = createPipeSession({
    write: (bytes) => { child.stdin.write(bytes); child.stdin.flush(); },
    timeoutMs,
    onTranscript,
  });
  let stderr = '';
  const pump = async (stream, onChunk) => { for await (const chunk of stream) onChunk(chunk); };
  const stdoutDone = pump(child.stdout, (c) => session.feed(c));
  const stderrDone = pump(child.stderr, (c) => { stderr += new TextDecoder().decode(c); });
  const exited = Promise.all([child.exited, stdoutDone, stderrDone]).then(([code]) => {
    session.close(new ProtocolError('EXITED', `pipe exited ${code}`, { exitCode: code }));
    return code;
  });
  return {
    pid: child.pid,
    session,
    stderr: () => stderr,
    exited,
    async close({ timeout = timeoutMs } = {}) {
      try { child.stdin.end(); } catch { /* already closed */ }
      try {
        return await withTimeout(exited, timeout, 'pipe exit after EOF');
      } catch (error) {
        child.kill();
        await exited;
        throw error;
      }
    },
  };
}
