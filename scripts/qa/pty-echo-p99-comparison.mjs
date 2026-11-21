#!/usr/bin/env node
/**
 * Real-PTY echo latency comparison: direct-PTY baseline vs the keeper data path.
 *
 * Source of truth: `.omo/plans/local-pty-crash-survival-20261116.md` (rev 5), risk row
 *   `Data-path copy hurts typing latency | keeper reads the master directly; p99 echo benchmark
 *    against today's baseline`.
 * This file exists to produce the measurement that row asks for, and nothing else.
 *
 * Honesty rules this file is built around:
 * 1. No historical baseline. There is no stored baseline this harness can read and it will not
 *    invent one. The baseline it reports is the direct path measured in this same run, and the
 *    report says so in `baseline.kind`. A run that cannot measure both arms reports a typed gap
 *    instead of a ratio.
 * 2. Not the same binary. The keeper arm talks to `ferryx-ptykeeper`; the daemon arm talks to
 *    `ferryx`. Their delta therefore includes whatever else differs between two binaries, and the
 *    report states `sameBinaryAsKeeperArm: false` rather than implying a pure data-path delta.
 * 3. Workload identity is proven, never assumed. Both arms spawn the same echo child, use the same
 *    fixed-size payloads, the same sample count and the same measurement code path. Before any
 *    timed sample is accepted, an identity probe asserts the bytes that came back are exactly the
 *    bytes that were written.
 * 4. No substituted boundary. If an arm cannot reach the seam the plan names, the run reports the
 *    gap code and exits non-zero. It never measures a nearer, cheaper boundary and labels it with
 *    the far one's name.
 * 5. Ownership is proven causally, never inferred from absence. The `keeper-daemon` arm is the one
 *    the plan asks for: the harness's client half is byte-identical to the direct arm's, and the
 *    only difference is which process holds the PTY master. It is accepted only when the keeper the
 *    harness itself started holds the session (`OWNERSHIP_LIST`), the durable ownership table names
 *    `keeper` for that session and agrees on the pid, and the daemon binary that routes it is
 *    byte-identical to the direct arm's. If any of those is unproven the arm fails with a typed gap
 *    instead of reporting a number. `describeLiveSessions` is deliberately *not* used as proof: it
 *    reports keeper-held sessions as `kind: "local"` with the keeper-reported pid
 *    (`terminal/service.rs::describe_live_sessions`), so its contents cannot distinguish the two
 *    owners and only serves as a pid consistency check.
 * 6. No substitution of the daemon endpoint. Both comparable arms speak the same NDJSON handshake
 *    on a socket named `daemon.sock` inside their own isolated runtime directory, and every sample
 *    enters and leaves through that client socket. The keeper-daemon arm reaches the keeper because
 *    the *daemon* routes the spawn there, not because the harness bypassed it.
 *
 * What one sample measures: the monotonic instant immediately before the harness issues a WRITE
 * carrying one payload, to the instant the last byte of that same payload has come back and been
 * matched. Both arms therefore include the whole loop: harness -> socket -> owner -> PTY master ->
 * child tty -> child -> PTY master -> owner -> socket -> harness. The arms differ only in which
 * process sits between socket and master, which is exactly the quantity the risk row is about.
 * The keeper-uds arm is a different boundary (no daemon hop at all), so it gets its own labelled
 * block and is never pooled into the daemon comparison. The `keeper-daemon` arm shares the direct
 * arm's boundary shape and is the arm the comparison is computed from.
 *
 * Measurement hygiene: `process.hrtime.bigint()` only (`Date.now()` is never a duration); the
 * echo subscription is armed before the write; every payload carries its own nonce so a match is
 * an identity, not a byte count; no fixed sleeps anywhere, readiness is an in-band marker awaited
 * event-driven and timeouts only fail a stuck run; blocks alternate arm order (A/B/B/A) so
 * machine drift cancels instead of landing on one arm.
 *
 * Isolation: every process is started by this harness inside one temp root with a scrubbed
 * FERRYX_* environment. The production runtime locations (/tmp/ferryx-<uid>, /tmp/ferryx-<uid>-dev,
 * $HOME/.ferryx) are refused with a usage error if any path overlaps them, checked before the
 * first spawn.
 *
 * Usage:
 *   node scripts/qa/pty-echo-p99-comparison.mjs \
 *     --daemon-binary src-tauri/target/release/ferryx \
 *     --keeper-binary crates/ferryx-ptykeeper/target/release/ferryx-ptykeeper \
 *     [--samples 200] [--warmup 20] [--payload-bytes 64] [--repeats 4] \
 *     [--arms direct-daemon,keeper-daemon] [--json] [--keep]
 *
 * Exit codes: 0 measured every selected arm, 1 a selected arm reported a gap, 2 usage error.
 */

import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

// -- keeper wire, from crates/ferryx-ptykeeper/src/protocol.rs ---------------------------------

const KEEPER_MAGIC = Buffer.from('FXKP', 'ascii');
const KEEPER_HEADER_BYTES = 60;
const KEEPER_FRAMING_VERSION = 1;
const KEEPER_MAX_PAYLOAD_BYTES = 1024 * 1024;
/** protocol::NO_REQUEST_ID: 36 dashes, the id on frames the keeper originates itself. */
const KEEPER_NO_REQUEST_ID = '-'.repeat(36);
/** KEEPER_MAX_WRITE_BYTES in keeper.rs; a larger write is refused, never split. */
const KEEPER_MAX_WRITE_BYTES = 64 * 1024;

const KIND = {
  Hello: 0x0001,
  HelloAck: 0x0002,
  Spawn: 0x0010,
  SpawnAck: 0x0011,
  Write: 0x0020,
  Kill: 0x0040,
  Attach: 0x0050,
  AttachAck: 0x0051,
  Output: 0x0052,
  ReplayGapEvent: 0x0063,
  OwnershipQuery: 0x0070,
  OwnershipList: 0x0071,
  Error: 0x00f0,
  Shutdown: 0x00f1,
};

const KIND_NAME = new Map(Object.entries(KIND).map(([name, value]) => [value, name]));

// -- daemon wire, from src-tauri/src/daemon/protocol.rs ----------------------------------------

const DAEMON_PROTOCOL_VERSION = 6;
/** crash_supervisor::KEEPER_SOCKET_NAME, plus the daemon's own socket name in the runtime dir. */
const KEEPER_SOCKET_NAME = 'keeper.sock';
const DAEMON_SOCKET_NAME = 'daemon.sock';
/** daemon/client.rs::DAEMON_READY_TOKEN, printed once on stdout after the listener binds. */
const DAEMON_READY_TOKEN = 'FERRYX_DAEMON_READY';
/** crates/ferryx-ptykeeper/src/main.rs prints this on stderr once its listener is bound. */
const KEEPER_READY_TOKEN = 'listening on';

/**
 * Frame types the daemon pushes that answer no request.
 *
 * `output` is the stream; `gap` and `replayGap` say bytes were lost. All three must bypass the
 * reply queue, because a request/reply reader that saw one would desynchronize onto the wrong frame.
 */
const STREAM_FRAME_TYPES = new Set(['output', 'gap', 'replayGap']);

/** macOS sockaddr_un.sun_path is 104 bytes including the terminator. */
const MAX_SOCKET_PATH_BYTES = 103;

/** keeper_launchd::QA_PLIST_LABEL / PLIST_LABEL. The production label is refused outright. */
const QA_KEEPER_LABEL = 'com.ferryx.qa.ptykeeper';
const PRODUCTION_KEEPER_LABEL = 'com.ferryx.ptykeeper';

/**
 * `keeper_launchd::KEEPER_OWNER_LABEL` / `DAEMON_OWNER_LABEL`, which mirror
 * `ferryx_ptykeeper::session::SessionOwner::as_str`. These are the strings the keeper puts in
 * `OWNERSHIP_LIST`'s `owner` field, so a comparison against them is a comparison against the
 * keeper's own vocabulary rather than one this file invented.
 */
const KEEPER_OWNER_LABEL = 'keeper';
const DAEMON_OWNER_LABEL = 'daemon';

/** `keeper_launchd::BINARY_NAME` and `keeper_startup::BUNDLE_KEEPER_DIR`. */
const KEEPER_BINARY_NAME = 'ferryx-ptykeeper';
const BUNDLE_KEEPER_DIR = 'keeper';
const BUNDLE_MANIFEST_NAME = 'keeper.json';

/**
 * The shell script both arms run as the echo child.
 *
 * `-opost` stops the tty translating newline into carriage-return-newline, and `-echo` stops the
 * tty echoing the write back a second time. What is left is `cat` copying its input to its output,
 * so the bytes read back are exactly the bytes written -- which is what makes an identity match
 * possible instead of a byte-count guess. Canonical mode delivers on newline, so every payload
 * ends with one.
 *
 * The `READY:<nonce>` line is printed only after the tty is already in that state, which is how both
 * arms prove readiness in band instead of sleeping. Both arms run this *same* text; only the way the
 * process is started differs.
 */
function echoChildScript(nonce) {
  return `stty -opost -echo 2>/dev/null || true; printf "READY:%s\\n" "${nonce}"; exec cat`;
}

function echoChildArgv(nonce) {
  return ['/bin/sh', '-c', echoChildScript(nonce)];
}

// -- typed gaps --------------------------------------------------------------------------------

const GAP = {
  DAEMON_BINARY_ABSENT: 'DAEMON_BINARY_ABSENT',
  KEEPER_BINARY_ABSENT: 'KEEPER_BINARY_ABSENT',
  DAEMON_START_FAILED: 'DAEMON_START_FAILED',
  KEEPER_START_FAILED: 'KEEPER_START_FAILED',
  DAEMON_SPAWN_REFUSED: 'DAEMON_SPAWN_REFUSED',
  ECHO_CHILD_NOT_IN_CONTROL: 'ECHO_CHILD_NOT_IN_CONTROL',
  KEEPER_SPAWN_REFUSED: 'KEEPER_SPAWN_REFUSED',
  ECHO_IDENTITY_UNPROVEN: 'ECHO_IDENTITY_UNPROVEN',
  KEEPER_OWNERSHIP_UNREACHABLE: 'KEEPER_OWNERSHIP_UNREACHABLE',
  LIVE_PUSH_ABSENT: 'LIVE_PUSH_ABSENT',
  RESIDUAL_UNCONSUMED_BYTES: 'RESIDUAL_UNCONSUMED_BYTES',
  NOT_ON_MACOS: 'NOT_ON_MACOS',
  UNSUPPORTED_PLATFORM: 'UNSUPPORTED_PLATFORM',
  /** The staged app bundle the daemon needs in order to install a keeper payload is not usable. */
  KEEPER_BUNDLE_UNUSABLE: 'KEEPER_BUNDLE_UNUSABLE',
  /** The keeper the harness started never answered, so no routing proof could be obtained. */
  KEEPER_ROUTING_UNPROVEN: 'KEEPER_ROUTING_UNPROVEN',
  /** The daemon did not route the session to the keeper: the flag, the probe, or the wire refused. */
  KEEPER_OWNERSHIP_UNPROVEN: 'KEEPER_OWNERSHIP_UNPROVEN',
  /** Two arms proved different daemon endpoints, so their numbers cannot be compared. */
  DAEMON_ENDPOINT_MISMATCH: 'DAEMON_ENDPOINT_MISMATCH',
};

class HarnessError extends Error {
  constructor(code, message, detail = null) {
    super(message);
    this.name = 'HarnessError';
    this.code = code;
    this.detail = detail;
  }
}

function fail(message) {
  return new HarnessError('FAIL', message);
}

function gap(code, message, detail = null) {
  return new HarnessError(code, message, detail);
}

// -- small utilities ---------------------------------------------------------------------------

function b64(bytes) {
  return Buffer.from(bytes).toString('base64');
}

function unb64(text) {
  if (typeof text !== 'string' || text.length === 0) return Buffer.alloc(0);
  return Buffer.from(text, 'base64');
}

/** Monotonic milliseconds. The only clock any measurement in this file reads. */
function monotonicMs() {
  return Number(process.hrtime.bigint()) / 1e6;
}

/** Races `promise` against a failure timeout. The timeout ends a stuck run; it never paces one. */
function within(promise, label, timeoutMs) {
  let timer = null;
  const timeout = new Promise((_resolve, rejectTimeout) => {
    timer = setTimeout(
      () => rejectTimeout(fail(`${label}: no progress within ${timeoutMs}ms`)),
      timeoutMs,
    );
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) clearTimeout(timer);
  });
}

/**
 * Waits until `probe` returns something other than null/undefined/false.
 *
 * `subscribe(onChange)` registers a change notification and returns an unsubscribe function, so
 * this never polls and never sleeps: the wait is woken by the event that actually matters.
 */
function waitFor(label, timeoutMs, subscribe, probe) {
  const settled = (value) => value !== null && value !== undefined && value !== false;
  const first = probe();
  if (settled(first)) return Promise.resolve(first);
  return new Promise((resolveWait, rejectWait) => {
    let unsubscribe = () => {};
    let done = false;
    const finish = (run, value) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      unsubscribe();
      run(value);
    };
    const timer = setTimeout(() => {
      finish(rejectWait, fail(`${label}: not satisfied within ${timeoutMs}ms`));
    }, timeoutMs);
    const check = () => {
      if (done) return;
      let value;
      try {
        value = probe();
      } catch (error) {
        finish(rejectWait, error);
        return;
      }
      if (settled(value)) finish(resolveWait, value);
    };
    unsubscribe = subscribe(check) ?? (() => {});
    check();
  });
}

/**
 * A byte queue with a cursor.
 *
 * Samples are consumed by exact byte count, so a payload can never be matched against a
 * neighbour's bytes, and anything left over stays visible in `pending()` for the end-of-block
 * assertion.
 */
class ByteStream {
  constructor() {
    this.buffer = Buffer.alloc(0);
    this.listeners = new Set();
    this.totalBytes = 0;
  }

  push(chunk) {
    if (!chunk || chunk.length === 0) return;
    this.buffer = Buffer.concat([this.buffer, chunk]);
    this.totalBytes += chunk.length;
    for (const listener of [...this.listeners]) listener();
  }

  pending() {
    return this.buffer.length;
  }

  peek(length) {
    return this.buffer.subarray(0, length);
  }

  take(length) {
    const taken = this.buffer.subarray(0, length);
    this.buffer = this.buffer.subarray(length);
    return taken;
  }

  preview(limit = 120) {
    return this.buffer.subarray(0, limit).toString('latin1');
  }

  /** Drops everything buffered, used to discard replay history that precedes sampling. */
  reset() {
    this.buffer = Buffer.alloc(0);
  }

  /**
   * Records a failure that a reader must see.
   *
   * A stream cannot throw from inside a socket callback without killing the process, so the failure
   * is stored here and raised by the next read -- which is inside `waitFor`, where it is caught and
   * rejects the sample properly.
   */
  fail(error) {
    if (this.failed) return;
    this.failed = error;
    for (const listener of [...this.listeners]) listener();
  }

  onData(listener) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }
}

// ---------------------------------------------------------------------------------------------
// Keeper client: the real 60-byte FXKP wire.
//
// Two connections per arm, because the wire allows exactly one role per connection: one issues
// verbs, the other is the session's live-output tap. That mirrors how the daemon splits the roles.
// ---------------------------------------------------------------------------------------------

class KeeperConnection {
  constructor(socket) {
    this.socket = socket;
    this.buffer = Buffer.alloc(0);
    this.sequence = 0;
    /** Per correlation id: queued frames plus at most one waiting reader. */
    this.pending = new Map();
    /** Bytes the keeper pushed unprompted. For a tap connection this is the measurement surface. */
    this.output = new ByteStream();
    /** Frames the keeper originated, kept for evidence rather than correlation. */
    this.originatedFrames = [];
    this.closedError = null;
    socket.on('data', (chunk) => this.#onData(chunk));
    socket.on('close', () => this.#failAll(fail('the keeper closed the connection before answering')));
    socket.on('error', (error) => this.#failAll(error));
  }

  static async connect(socketPath, label) {
    const socket = net.createConnection({ path: socketPath });
    await within(once(socket, 'connect'), `keeper connect (${label})`, 10_000);
    socket.setNoDelay(true);
    return new KeeperConnection(socket);
  }

  #failAll(error) {
    if (this.closedError) return;
    this.closedError = error;
    for (const entry of this.pending.values()) {
      entry.waiter?.reject(error);
      entry.waiter = null;
    }
    this.pending.clear();
  }

  #onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      if (this.closedError) return;
      if (this.buffer.length < KEEPER_HEADER_BYTES) return;
      if (!this.buffer.subarray(0, 4).equals(KEEPER_MAGIC)) {
        this.#failAll(fail(`bad keeper magic ${this.buffer.subarray(0, 4).toString('hex')}`));
        return;
      }
      const version = this.buffer.readUInt16BE(4);
      if (version !== KEEPER_FRAMING_VERSION) {
        this.#failAll(
          fail(`keeper framing version ${version}, this harness speaks ${KEEPER_FRAMING_VERSION}`),
        );
        return;
      }
      const kind = this.buffer.readUInt16BE(6);
      const payloadLength = this.buffer.readUInt32BE(8);
      if (payloadLength > KEEPER_MAX_PAYLOAD_BYTES) {
        this.#failAll(
          fail(`keeper announced ${payloadLength} payload bytes, cap is ${KEEPER_MAX_PAYLOAD_BYTES}`),
        );
        return;
      }
      const total = KEEPER_HEADER_BYTES + payloadLength;
      if (this.buffer.length < total) return;
      const requestId = this.buffer.subarray(12, 48).toString('ascii');
      const frameSequence = Number(this.buffer.readBigUInt64BE(48));
      const fdCount = this.buffer.readUInt16BE(56);
      const flags = this.buffer.readUInt16BE(58);
      // This harness attaches no descriptors and the keeper sends none. A frame claiming any means
      // the kernel dropped them, and every later byte would be untrustworthy.
      if (fdCount !== 0 || flags !== 0) {
        this.#failAll(fail(`keeper frame declares ${fdCount} fds and flags ${flags}`));
        return;
      }
      let payload = null;
      if (payloadLength > 0) {
        try {
          payload = JSON.parse(this.buffer.subarray(KEEPER_HEADER_BYTES, total).toString('utf8'));
        } catch (error) {
          this.#failAll(fail(`keeper payload is not JSON: ${String(error)}`));
          return;
        }
      }
      this.buffer = this.buffer.subarray(total);
      const frame = {
        kind,
        name: KIND_NAME.get(kind) ?? `0x${kind.toString(16)}`,
        requestId,
        frameSequence,
        payload,
      };

      const entry = this.pending.get(requestId);
      if (entry) {
        if (entry.waiter) {
          const waiter = entry.waiter;
          entry.waiter = null;
          waiter.resolve(frame);
        } else {
          entry.queue.push(frame);
        }
        continue;
      }

      // Nothing is outstanding for this id, so only a frame the keeper originates may appear here.
      // Anything else means correlation is lost, and guessing would corrupt the measurement.
      const keeperOriginated =
        requestId === KEEPER_NO_REQUEST_ID || kind === KIND.Output || kind === KIND.Shutdown;
      if (!keeperOriginated) {
        this.#failAll(fail(`uncorrelated keeper frame ${frame.name} for ${requestId}`));
        return;
      }
      this.originatedFrames.push(frame);
      if (kind === KIND.Output) this.output.push(unb64(payload?.data));
    }
  }

  #encode(kind, payload, requestId) {
    const body = Buffer.from(JSON.stringify(payload ?? {}), 'utf8');
    if (body.length > KEEPER_MAX_PAYLOAD_BYTES) {
      throw fail(`keeper ${KIND_NAME.get(kind)} payload is ${body.length} bytes, cap is ${KEEPER_MAX_PAYLOAD_BYTES}`);
    }
    const id = requestId ?? KEEPER_NO_REQUEST_ID;
    if (id.length !== 36) throw fail(`request id ${JSON.stringify(id)} is not 36 characters`);
    const header = Buffer.alloc(KEEPER_HEADER_BYTES);
    KEEPER_MAGIC.copy(header, 0);
    header.writeUInt16BE(KEEPER_FRAMING_VERSION, 4);
    header.writeUInt16BE(kind, 6);
    header.writeUInt32BE(body.length, 8);
    header.write(id, 12, 36, 'ascii');
    header.writeBigUInt64BE(BigInt(this.sequence), 48);
    header.writeUInt16BE(0, 56);
    header.writeUInt16BE(0, 58);
    this.sequence += 1;
    return Buffer.concat([header, body]);
  }

  #next(label, timeoutMs, requestId) {
    const entry = this.pending.get(requestId) ?? { queue: [], waiter: null };
    this.pending.set(requestId, entry);
    if (entry.queue.length > 0) return Promise.resolve(entry.queue.shift());
    if (this.closedError) return Promise.reject(this.closedError);
    return new Promise((resolveFrame, rejectFrame) => {
      const timer = setTimeout(() => {
        entry.waiter = null;
        rejectFrame(fail(`${label}: no keeper frame within ${timeoutMs}ms`));
      }, timeoutMs);
      entry.waiter = {
        resolve: (frame) => {
          clearTimeout(timer);
          resolveFrame(frame);
        },
        reject: (error) => {
          clearTimeout(timer);
          rejectFrame(error);
        },
      };
    });
  }

  #release(requestId) {
    const entry = this.pending.get(requestId);
    if (!entry) return;
    this.pending.delete(requestId);
    entry.waiter?.reject(fail(`request ${requestId} was released while still awaiting a frame`));
    entry.waiter = null;
  }

  /** One request, one reply. An ERROR frame becomes a typed refusal carrying the keeper's code. */
  async call(kind, payload, expected, label, timeoutMs = 30_000) {
    const name = label ?? `keeper ${KIND_NAME.get(kind)}`;
    const requestId = randomUUID();
    const bytes = this.#encode(kind, payload, requestId);
    // Registered before the write: a reply that lands before the next await must not be misfiled.
    const waiting = this.#next(name, timeoutMs, requestId);
    try {
      this.socket.write(bytes);
      const frame = await waiting;
      if (frame.kind === KIND.Error) {
        const code = frame.payload?.code ?? 'ERROR';
        throw gap(
          code,
          `${name} refused: ${code} ${frame.payload?.message ?? ''}`.trim(),
          frame.payload ?? null,
        );
      }
      if (expected !== undefined && frame.kind !== expected) {
        throw fail(`${name}: expected ${KIND_NAME.get(expected)}, got ${frame.name}`);
      }
      return frame.payload ?? {};
    } finally {
      this.#release(requestId);
    }
  }

  /**
   * Registers a reply waiter and returns a promise for it.
   *
   * Split from the write so a caller can arm the waiter, take its own timestamp, and then write --
   * a reply that arrives before the caller resumes must already have somewhere to land.
   */
  expect(kind, requestId, label, timeoutMs = 30_000) {
    const name = label ?? `keeper ${KIND_NAME.get(kind)}`;
    return this.#next(name, timeoutMs, requestId)
      .then((frame) => {
        if (frame.kind === KIND.Error) {
          const code = frame.payload?.code ?? 'ERROR';
          throw gap(code, `${name} refused: ${code} ${frame.payload?.message ?? ''}`.trim(), frame.payload ?? null);
        }
        if (frame.kind !== kind) {
          throw fail(`${name}: expected ${KIND_NAME.get(kind)}, got ${frame.name}`);
        }
        return frame.payload ?? {};
      })
      .finally(() => this.#release(requestId));
  }

  /** Encodes a request without writing it, for callers that need their own write instant. */
  encode(kind, payload, requestId) {
    return this.#encode(kind, payload, requestId);
  }

  close() {
    this.socket.destroy();
  }
}

// ---------------------------------------------------------------------------------------------
// Daemon client: NDJSON on daemon.sock.
// ---------------------------------------------------------------------------------------------

class LineChannel {
  constructor(socket, options = {}) {
    this.socket = socket;
    this.text = '';
    this.frames = [];
    this.waiter = null;
    this.closedError = null;
    /**
     * Receives streamed frames (`output`, `gap`, `replayGap`) instead of the reply queue.
     *
     * A connection that is attached receives output frames that answer no request. If they were
     * queued as replies every later `call` would read the wrong frame, so they are diverted here.
     */
    this.sink = options.sink ?? null;
    socket.setEncoding('utf8');
    socket.on('data', (chunk) => {
      this.text += chunk;
      for (;;) {
        const newline = this.text.indexOf('\n');
        if (newline < 0) return;
        const line = this.text.slice(0, newline);
        this.text = this.text.slice(newline + 1);
        if (!line.trim()) continue;
        let parsed;
        try {
          parsed = JSON.parse(line);
        } catch (error) {
          parsed = { type: 'unparseable', raw: line.slice(0, 400), error: String(error) };
        }
        if (this.sink && STREAM_FRAME_TYPES.has(parsed.type)) {
          this.sink(parsed);
          continue;
        }
        const waiter = this.waiter;
        if (waiter) {
          this.waiter = null;
          waiter.resolve(parsed);
        } else {
          this.frames.push(parsed);
        }
      }
    });
    socket.on('close', () => this.#fail(fail('the daemon closed the connection')));
    socket.on('error', (error) => this.#fail(error));
  }

  static async connect(socketPath, label, options = {}) {
    const socket = net.createConnection({ path: socketPath });
    await within(once(socket, 'connect'), `daemon connect (${label})`, 10_000);
    socket.setNoDelay(true);
    return new LineChannel(socket, options);
  }

  #fail(error) {
    if (this.closedError) return;
    this.closedError = error;
    this.waiter?.reject(error);
    this.waiter = null;
  }

  /** Sends one NDJSON request. Returns nothing: the caller may take its own timestamp. */
  send(object) {
    this.socket.write(`${JSON.stringify(object)}\n`);
  }

  next(label, timeoutMs = 30_000) {
    if (this.frames.length > 0) return Promise.resolve(this.frames.shift());
    if (this.closedError) return Promise.reject(this.closedError);
    return new Promise((resolveFrame, rejectFrame) => {
      const timer = setTimeout(() => {
        this.waiter = null;
        rejectFrame(fail(`${label}: no daemon frame within ${timeoutMs}ms`));
      }, timeoutMs);
      this.waiter = {
        resolve: (frame) => {
          clearTimeout(timer);
          resolveFrame(frame);
        },
        reject: (error) => {
          clearTimeout(timer);
          rejectFrame(error);
        },
      };
    });
  }

  /**
   * Sends a request and returns the first reply that is not a streamed `output` frame.
   *
   * `output` frames are pushed unconditionally once attached, so a client that waits for "the next
   * frame" gets a stream frame instead of its answer. They are dropped here for setup calls; the
   * measurement path arms its own sink instead.
   */
  async call(object, label, timeoutMs = 30_000) {
    this.send(object);
    for (;;) {
      const frame = await this.next(label ?? object.type, timeoutMs);
      if (frame.type === 'output') continue;
      if (frame.type === 'error' || frame.type === 'protocolMismatch') {
        throw gap(
          frame.type === 'protocolMismatch' ? 'PROTOCOL_MISMATCH' : 'DAEMON_ERROR',
          `${label ?? object.type} refused: ${frame.message ?? frame.reason ?? JSON.stringify(frame).slice(0, 300)}`,
          frame,
        );
      }
      return frame;
    }
  }

  close() {
    this.socket.destroy();
  }
}

// ---------------------------------------------------------------------------------------------
// Payloads: one fixed-size payload per sample, each carrying its own nonce.
// ---------------------------------------------------------------------------------------------

/**
 * Builds a payload of exactly `payloadBytes` bytes ending in a newline.
 *
 * The nonce is unique per sample, so an echo can only resolve the sample that produced it. The
 * minimum size is enforced rather than silently truncated: a short payload would make the nonce
 * ambiguous and the measurement wrong.
 */
function makePayload(index, payloadBytes) {
  const nonce = `fxe${index.toString(16).padStart(6, '0')}${randomUUID().replace(/-/g, '').slice(0, 8)}`;
  if (payloadBytes < nonce.length + 1) {
    throw new HarnessError(
      'USAGE',
      `--payload-bytes ${payloadBytes} is too small to carry a unique nonce (needs ${nonce.length + 1})`,
    );
  }
  const body = `${nonce}${'.'.repeat(payloadBytes - nonce.length - 1)}\n`;
  const bytes = Buffer.from(body, 'ascii');
  if (bytes.length !== payloadBytes) {
    throw fail(`payload ${index} is ${bytes.length} bytes, expected ${payloadBytes}`);
  }
  return { nonce, bytes };
}

// ---------------------------------------------------------------------------------------------
// Percentiles: nearest rank, so every reported number is a sample the report can point at.
// ---------------------------------------------------------------------------------------------

function percentile(sortedAscending, p) {
  if (sortedAscending.length === 0) return null;
  if (p <= 0) return sortedAscending[0];
  const rank = Math.ceil((p / 100) * sortedAscending.length);
  return sortedAscending[Math.min(sortedAscending.length, Math.max(1, rank)) - 1];
}

function summarize(samplesMs) {
  const sorted = [...samplesMs].sort((a, b) => a - b);
  const total = samplesMs.reduce((sum, value) => sum + value, 0);
  return {
    count: samplesMs.length,
    method: 'nearest-rank; index = ceil(p/100 * n) - 1 on the ascending sample array; no interpolation',
    minMs: sorted.length > 0 ? sorted[0] : null,
    p50Ms: percentile(sorted, 50),
    p90Ms: percentile(sorted, 90),
    p99Ms: percentile(sorted, 99),
    maxMs: sorted.length > 0 ? sorted[sorted.length - 1] : null,
    meanMs: samplesMs.length > 0 ? Number((total / samplesMs.length).toFixed(4)) : null,
    samplesMs,
  };
}

// ---------------------------------------------------------------------------------------------
// Process supervision: only children this file started, only inside its own root.
// ---------------------------------------------------------------------------------------------

class Supervised {
  constructor(kind, child, argv, env) {
    this.kind = kind;
    this.child = child;
    this.argv = argv;
    this.env = env;
    this.stdout = '';
    this.stderr = '';
    this.exited = null;
    this.spawnError = null;
    child.stdout?.setEncoding('utf8');
    child.stderr?.setEncoding('utf8');
    // A spawn failure (missing file, no exec bit, wrong architecture) arrives asynchronously; left
    // unhandled it kills the harness with a stack trace instead of naming the binary.
    child.on('error', (error) => {
      this.spawnError = error;
    });
    child.stdout?.on('data', (chunk) => {
      this.stdout += chunk;
    });
    child.stderr?.on('data', (chunk) => {
      this.stderr += chunk;
    });
    child.on('exit', (code, signal) => {
      this.exited = { code, signal };
    });
  }

  /** Starts one child of this harness. Never a detached or re-parented process. */
  static start(kind, binary, args, env, cwd, codeWhenAbsent) {
    let stats;
    try {
      stats = statSync(binary);
    } catch {
      throw gap(codeWhenAbsent, `${kind} binary does not exist: ${binary}`, { binary });
    }
    if (!stats.isFile()) {
      throw gap(codeWhenAbsent, `${kind} binary is not a regular file: ${binary}`, { binary });
    }
    const child = spawn(binary, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    return new Supervised(kind, child, [binary, ...args], env);
  }

  get pid() {
    return this.child.pid ?? null;
  }

  /**
   * Resolves once the child has printed `token` on the given stream.
   *
   * The wait is woken by output arriving, so it never polls and never sleeps. The deadline exists
   * only to end a run whose child will never be ready.
   */
  waitForToken(streamName, token, timeoutMs) {
    return waitFor(
      `${this.kind} ready (${token})`,
      timeoutMs,
      (onChange) => {
        this.child[streamName]?.on('data', onChange);
        this.child.on('exit', onChange);
        return () => {
          this.child[streamName]?.off('data', onChange);
          this.child.off('exit', onChange);
        };
      },
      () => {
        if (this.spawnError) {
          throw fail(`${this.kind} could not be executed: ${this.spawnError.message}`);
        }
        if (this.exited) {
          throw fail(
            `${this.kind} exited before becoming ready (code ${this.exited.code}, signal ${this.exited.signal}); stderr: ${this.stderr.slice(-800).trim()}`,
          );
        }
        return this.stdout.includes(token) || this.stderr.includes(token);
      },
    );
  }

  async stop() {
    if (this.exited) return this.exited;
    this.child.kill('SIGTERM');
    try {
      const [code, signal] = await within(once(this.child, 'exit'), `${this.kind} exit`, 5_000);
      return { code, signal };
    } catch {
      this.child.kill('SIGKILL');
      await within(once(this.child, 'exit'), `${this.kind} kill`, 5_000).catch(() => null);
      return this.exited;
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Isolation. One temp root; every path the children are told about is proven to lie inside it.
// ---------------------------------------------------------------------------------------------

/** The runtime locations a production Ferryx would use. None of these may be touched. */
function productionRuntimeDirs() {
  const uid = typeof process.getuid === 'function' ? process.getuid() : 0;
  return [
    path.resolve(`/tmp/ferryx-${uid}`),
    path.resolve(`/tmp/ferryx-${uid}-dev`),
    path.resolve(os.homedir(), '.ferryx'),
  ];
}

/** True when `candidate` is `root` itself or lives beneath it, after resolution. */
function isUnder(root, candidate) {
  const rel = path.relative(path.resolve(root), path.resolve(candidate));
  return rel === '' || (!rel.startsWith('..') && !path.isAbsolute(rel));
}

/**
 * Builds every path this run uses, all of them inside `root`.
 *
 * Paths are derived, never inherited: nothing here can be pointed at an existing install by an
 * environment variable the harness did not set itself.
 */
function planPaths(root) {
  const runtimeDir = path.join(root, 'rt');
  const legacyRuntimeDir = path.join(root, 'legacy');
  const dataDir = path.join(root, 'data');
  /**
   * The staged macOS app bundle the keeper-daemon arm runs from.
   *
   * `keeper_startup::bundle_keeper_dir` derives the payload from `current_exe()`: it requires the
   * executable's parent to be named `MacOS`, and looks for `../Resources/keeper`. So the daemon that
   * routes to a keeper must itself live at `Contents/MacOS/<exe>`; no environment variable can point
   * it at a payload instead. This layout is what makes that reachable without touching a real app.
   */
  const bundleContents = path.join(root, 'bundle', 'Ferryx Daemon.app', 'Contents');
  const bundleMacOsDir = path.join(bundleContents, 'MacOS');
  const bundleKeeperDir = path.join(bundleContents, 'Resources', BUNDLE_KEEPER_DIR);
  return {
    root,
    runtimeDir,
    legacyRuntimeDir,
    dataDir,
    sessionDir: path.join(root, 'sessions'),
    keeperDir: path.join(root, 'keepers'),
    homeDir: path.join(root, 'home'),
    tmpDir: path.join(root, 'tmp'),
    workDir: path.join(root, 'work'),
    logsDir: path.join(root, 'logs'),
    wrapperPath: path.join(root, 'echo-shell'),
    daemonPlistPath: path.join(runtimeDir, 'com.ferryx.qa.daemon.plist'),
    keeperPlistPath: path.join(runtimeDir, `${QA_KEEPER_LABEL}.plist`),
    keeperSocketPath: path.join(runtimeDir, KEEPER_SOCKET_NAME),
    daemonSocketPath: path.join(runtimeDir, DAEMON_SOCKET_NAME),
    /**
     * `OwnershipStore::for_runtime()` is `logs_dir().parent()`, and `logs_dir()` is
     * `$FERRYX_DATA_DIR/logs` whenever that variable is set. So the durable table this run writes
     * is exactly `<dataDir>/pty-ownership.json` -- never `$HOME/.ferryx/...`.
     */
    ownershipPath: path.join(dataDir, 'pty-ownership.json'),
    bundleContents,
    bundleMacOsDir,
    bundleKeeperDir,
    /** The daemon copy inside the staged bundle; the keeper-daemon arm's executable path. */
    bundledDaemonPath: path.join(bundleMacOsDir, 'ferryx-daemon-qa'),
    bundledKeeperPath: path.join(bundleKeeperDir, KEEPER_BINARY_NAME),
    bundledManifestPath: path.join(bundleKeeperDir, BUNDLE_MANIFEST_NAME),
  };
}

/**
 * Refuses to run unless every configured path is inside the fixture root.
 *
 * This is the isolation proof the acceptance harness also demands: an isolated run that silently
 * writes one path outside its root is not isolated, and "we set the variable" is not evidence that
 * the resulting path stayed inside. Checked before the first spawn.
 */
function assertIsolation(paths) {
  const production = productionRuntimeDirs();
  const inside = [
    'runtimeDir',
    'legacyRuntimeDir',
    'dataDir',
    'sessionDir',
    'keeperDir',
    'homeDir',
    'tmpDir',
    'workDir',
    'logsDir',
    'wrapperPath',
    'daemonPlistPath',
    'keeperPlistPath',
    'keeperSocketPath',
    'daemonSocketPath',
    'ownershipPath',
    'bundleContents',
    'bundleMacOsDir',
    'bundleKeeperDir',
    'bundledDaemonPath',
    'bundledKeeperPath',
    'bundledManifestPath',
  ];
  for (const key of inside) {
    const value = paths[key];
    if (!value || !path.isAbsolute(value)) {
      throw new HarnessError('USAGE', `${key} must be an absolute path`);
    }
    if (!isUnder(paths.root, value)) {
      throw new HarnessError('USAGE', `${key} (${value}) escapes the fixture root ${paths.root}`);
    }
    for (const reserved of production) {
      if (isUnder(reserved, value) || isUnder(value, reserved)) {
        throw new HarnessError(
          'USAGE',
          `${key} (${value}) overlaps the production runtime location ${reserved}; refusing to run`,
        );
      }
    }
  }
  if (isUnder(paths.runtimeDir, paths.legacyRuntimeDir) || isUnder(paths.legacyRuntimeDir, paths.runtimeDir)) {
    throw new HarnessError('USAGE', 'runtime and legacy runtime roots overlap, so isolation is not provable');
  }
  for (const socket of [paths.keeperSocketPath, paths.daemonSocketPath]) {
    const bytes = Buffer.byteLength(socket);
    if (bytes > MAX_SOCKET_PATH_BYTES) {
      throw new HarnessError(
        'USAGE',
        `${socket} is ${bytes} bytes, over the ${MAX_SOCKET_PATH_BYTES}-byte unix socket limit`,
      );
    }
  }
}

/**
 * The complete child environment.
 *
 * Built from scratch rather than by adding to `process.env`, so no inherited `FERRYX_*` override can
 * point a child at the user's real daemon, keeper, log directory or LaunchAgent. The two plist
 * variables are the ones the shipping code actually reads: `FERRYX_DAEMON_PLIST_PATH`
 * (`daemon/launchd.rs`) and `FERRYX_PTY_KEEPER_PLIST_PATH` (`daemon/keeper_launchd.rs`).
 */
function childEnv(paths, supervisorName) {
  if (supervisorName === PRODUCTION_KEEPER_LABEL) {
    throw new HarnessError('USAGE', `refusing to run under the production launchd label ${supervisorName}`);
  }
  return {
    PATH: process.env.PATH ?? '/usr/bin:/bin:/usr/sbin:/sbin',
    HOME: paths.homeDir,
    TMPDIR: paths.tmpDir,
    LC_ALL: 'C',
    FERRYX_RUNTIME_DIR: paths.runtimeDir,
    FERRYX_LEGACY_RUNTIME_DIR: paths.legacyRuntimeDir,
    FERRYX_DATA_DIR: paths.dataDir,
    FERRYX_SESSION_DIR: paths.sessionDir,
    FERRYX_KEEPER_DIR: paths.keeperDir,
    FERRYX_DAEMON_SUPERVISOR_NAME: supervisorName,
    FERRYX_DAEMON_PLIST_PATH: paths.daemonPlistPath,
    FERRYX_PTY_KEEPER_PLIST_PATH: paths.keeperPlistPath,
  };
}

/** Creates every directory the children expect, 0700, before anything is spawned. */
function prepareDirectories(paths) {
  for (const dir of [
    paths.runtimeDir,
    paths.legacyRuntimeDir,
    paths.dataDir,
    paths.sessionDir,
    paths.keeperDir,
    paths.homeDir,
    paths.tmpDir,
    paths.workDir,
    paths.logsDir,
    paths.bundleMacOsDir,
    paths.bundleKeeperDir,
  ]) {
    mkdirSync(dir, { recursive: true, mode: 0o700 });
  }
}

// ---------------------------------------------------------------------------------------------
// Staging the bundle the keeper-daemon arm runs from.
//
// Fixture setup only: it copies bytes that already exist and writes a manifest describing them.
// Nothing here compiles, links or signs anything, and nothing is written outside the fixture root.
// ---------------------------------------------------------------------------------------------

/**
 * The generation version the staged manifest names.
 *
 * `keeper_launchd::keeper_generation_version()` is the *app's* `CARGO_PKG_VERSION`, and
 * `ensure_keeper_service` installs this pass's generation under exactly that name. `looks_like_
 * version` requires three numeric components, which `1.0.0` satisfies.
 *
 * The app version is not readable here without parsing `src-tauri/Cargo.toml`, and the manifest is
 * consulted only for its `sha256` (`keeper_startup::payload_digest`), so one fixed valid name keeps
 * the fixture deterministic instead of guessing at a version that would change per release.
 */
const STAGED_GENERATION_VERSION = '1.0.0';

/** The manifest schema `scripts/lib/keeper-bundle.mjs` writes for a real release. */
const KEEPER_MANIFEST_SCHEMA = 'ferryx-keeper-bundle/v1';

/**
 * Stages `<root>/bundle/Ferryx Daemon.app` so a daemon started from it can install a keeper.
 *
 * `keeper_startup::bundle_keeper_dir` derives the payload from `current_exe()`: the executable's
 * parent must be named `MacOS`, and the payload is `../Resources/keeper`. No environment variable
 * redirects it, so the daemon that is expected to route to a keeper must physically live inside a
 * bundle-shaped directory. A plain `target/release/ferryx` returns `PayloadAbsent`, aborts
 * `start_production`, and leaves the ownership flag off -- after which the daemon comes up healthy
 * and silently owns every PTY itself. This staging is what removes that silent fallback.
 *
 * Three facts are proven before returning, because each is a way the arm could start and then
 * quietly measure the direct path:
 * 1. the executable's parent is literally named `MacOS`,
 * 2. the payload binary exists as a regular file under `Resources/keeper`,
 * 3. `keeper.json` records the sha256 of exactly those payload bytes. `payload_digest` compares that
 *    value before installing and returns `PayloadUnusable` on a mismatch.
 */
function stageDaemonBundle(paths, binaries) {
  if (path.basename(paths.bundleMacOsDir) !== 'MacOS') {
    throw gap(
      GAP.KEEPER_BUNDLE_UNUSABLE,
      `the staged executable directory is ${paths.bundleMacOsDir}, which bundle_keeper_dir does not accept`,
    );
  }

  copyExecutable(binaries.daemon, paths.bundledDaemonPath);
  copyExecutable(binaries.keeper, paths.bundledKeeperPath);

  const digest = createHash('sha256').update(readFileSync(paths.bundledKeeperPath)).digest('hex');
  const manifest = {
    schema: KEEPER_MANIFEST_SCHEMA,
    version: STAGED_GENERATION_VERSION,
    sha256: digest,
  };
  writeFileSync(paths.bundledManifestPath, `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o600 });

  // Read back rather than trust the write: the daemon hashes the file it finds on disk, so the only
  // useful proof is the digest of those bytes against the digest the manifest records.
  const onDisk = createHash('sha256').update(readFileSync(paths.bundledKeeperPath)).digest('hex');
  if (onDisk !== digest) {
    throw gap(
      GAP.KEEPER_BUNDLE_UNUSABLE,
      `the staged keeper payload changed between copy and manifest (${digest} vs ${onDisk})`,
    );
  }
  for (const required of [paths.bundledDaemonPath, paths.bundledKeeperPath, paths.bundledManifestPath]) {
    if (!statSync(required).isFile()) {
      throw gap(GAP.KEEPER_BUNDLE_UNUSABLE, `the staged bundle is missing ${required}`);
    }
  }
  return {
    bundleExecutable: paths.bundledDaemonPath,
    payloadBinary: paths.bundledKeeperPath,
    manifestPath: paths.bundledManifestPath,
    generationVersion: STAGED_GENERATION_VERSION,
    payloadSha256: digest,
  };
}

/** Copies one binary into the fixture verbatim, then makes it executable. */
function copyExecutable(source, destination) {
  let stats;
  try {
    stats = statSync(source);
  } catch {
    throw gap(GAP.KEEPER_BUNDLE_UNUSABLE, `cannot stage ${destination}: ${source} does not exist`, { source });
  }
  if (!stats.isFile()) {
    throw gap(
      GAP.KEEPER_BUNDLE_UNUSABLE,
      `cannot stage ${destination}: ${source} is not a regular file`,
      { source },
    );
  }
  writeFileSync(destination, readFileSync(source), { mode: 0o700 });
  chmodSync(destination, 0o700);
}

/**
 * Reads the durable ownership table, if a daemon wrote one.
 *
 * `OwnershipStore::for_runtime()` writes this file, and `OwnershipRecord` is
 * `{sessionId, owner, epoch, cols, rows, pid, processStartTime, generation, createdAt,
 * checkpointOffset}` in camelCase. The `pid` is the child pid the *keeper* reported at spawn, so it
 * is one half of the cross-check that proves a session really lives in the keeper.
 *
 * A missing table is the documented default (`OwnershipTable::default()`) and means the flag is off,
 * which routes new sessions to the direct path.
 */
function readOwnership(paths) {
  try {
    const parsed = JSON.parse(readFileSync(paths.ownershipPath, 'utf8'));
    const sessions = parsed?.sessions ?? {};
    return {
      path: paths.ownershipPath,
      present: true,
      keeperOwnershipEnabled: parsed?.keeperOwnershipEnabled === true,
      sessionOwners: Object.fromEntries(
        Object.entries(sessions).map(([id, record]) => [id, record?.owner ?? null]),
      ),
      records: Object.fromEntries(
        Object.entries(sessions).map(([id, record]) => [
          id,
          {
            owner: record?.owner ?? null,
            epoch: typeof record?.epoch === 'number' ? record.epoch : null,
            pid: typeof record?.pid === 'number' ? record.pid : null,
            processStartTime:
              typeof record?.processStartTime === 'number' ? record.processStartTime : null,
            generation: record?.generation ?? null,
          },
        ]),
      ),
    };
  } catch {
    return {
      path: paths.ownershipPath,
      present: false,
      keeperOwnershipEnabled: false,
      sessionOwners: {},
      records: {},
    };
  }
}

// ---------------------------------------------------------------------------------------------
// Arm interface, so the measurement loop is written once and shared by both arms.
//
// An arm owns: a session that has an echo child in it, a tap that delivers that child's output to
// `output`, and a `write()` that issues the request and returns the instant it was issued. Nothing
// arm-specific is allowed into the sampling loop: that is what makes the two arms comparable.
// ---------------------------------------------------------------------------------------------

/**
 * Decides whether the evidence actually proves the keeper owns `sessionId`.
 *
 * This is pure so it can be pinned by a unit test without a daemon, a keeper or a socket. It is the
 * one place that answers "is this the arm the plan asked for", and it is deliberately strict: every
 * clause must hold, and each clause is evidence the implementation really produces.
 *
 * The clauses, and why absence alone is never one of them:
 *
 * - `keeperSessions` is the `sessions` array of an `OWNERSHIP_LIST` reply read from the keeper this
 *   harness started. `crates/ferryx-ptykeeper/src/server.rs` emits one entry per live session with
 *   `id`, `owner`, `pid`, `epoch` and `sequenceOffset`. Only the keeper's own registry can say which
 *   sessions it holds, so this is causal proof rather than an inference.
 * - `owner` must be the keeper's own label. `daemon` here would mean the session exists but is not
 *   keeper-held, which is exactly the failure mode this gate exists for.
 * - `ownershipTable` is the durable table the daemon wrote. `keeperOwnershipEnabled` proves the
 *   routing flag was on; the per-session record proves the daemon recorded a `keeper` owner.
 * - The pids must agree. `OwnershipRecord.pid` is the pid the keeper reported at spawn
 *   (`KeeperSessionHost::spawn`), and `KeeperSessionStatus.pid` feeds `describeLiveSessions`, so a
 *   three-way agreement ties the keeper's registry, the daemon's durable table and the daemon's live
 *   view to the same child. A disagreement means one of them is describing a different process and
 *   no sample from it can be trusted.
 *
 * `describeLiveSessions` is used only for that pid cross-check. It cannot be used as ownership proof:
 * `TerminalService::describe_live_sessions` lists keeper-held sessions with `kind: "local"` and the
 * keeper-reported pid, so it renders a keeper session indistinguishably from a direct one.
 */
function verifyKeeperOwnershipEvidence({
  sessionId,
  keeperSessions,
  ownershipTable,
  liveSession,
}) {
  const failures = [];

  if (!sessionId) failures.push('no session id was recorded for the arm');

  const keeperEntry = Array.isArray(keeperSessions)
    ? keeperSessions.find((entry) => entry?.id === sessionId)
    : undefined;
  if (!Array.isArray(keeperSessions)) {
    failures.push('the keeper returned no OWNERSHIP_LIST sessions array');
  } else if (!keeperEntry) {
    failures.push(`the keeper does not hold session ${sessionId}, so the daemon did not route it there`);
  } else if (keeperEntry.owner !== KEEPER_OWNER_LABEL) {
    failures.push(
      `the keeper reports owner ${JSON.stringify(keeperEntry.owner)} for ${sessionId}, not ${JSON.stringify(KEEPER_OWNER_LABEL)}`,
    );
  }

  if (!ownershipTable) {
    failures.push('the durable ownership table was never read');
  } else {
    if (ownershipTable.keeperOwnershipEnabled !== true) {
      failures.push('the durable ownership table does not have keeper ownership enabled');
    }
    const record = ownershipTable.records?.[sessionId];
    if (!record) {
      failures.push(`the durable ownership table has no record for ${sessionId}`);
    } else if (record.owner !== KEEPER_OWNER_LABEL) {
      failures.push(
        `the durable ownership table records owner ${JSON.stringify(record.owner)} for ${sessionId}`,
      );
    }
  }

  // Pid agreement across the three independent views, when each one actually supplied a pid.
  const keeperPid = typeof keeperEntry?.pid === 'number' ? keeperEntry.pid : null;
  const tablePid =
    typeof ownershipTable?.records?.[sessionId]?.pid === 'number'
      ? ownershipTable.records[sessionId].pid
      : null;
  const livePid = typeof liveSession?.localChildPid === 'number' ? liveSession.localChildPid : null;
  if (keeperPid === null && tablePid === null) {
    failures.push('neither the keeper nor the durable table reported a child pid');
  }
  if (keeperPid !== null && tablePid !== null && keeperPid !== tablePid) {
    failures.push(`the keeper reports pid ${keeperPid} but the durable table records ${tablePid}`);
  }
  if (livePid !== null && keeperPid !== null && livePid !== keeperPid) {
    failures.push(`describeLiveSessions reports pid ${livePid} but the keeper holds pid ${keeperPid}`);
  }

  if (failures.length > 0) {
    throw gap(
      GAP.KEEPER_OWNERSHIP_UNPROVEN,
      `keeper ownership of ${sessionId} is unproven: ${failures.join('; ')}`,
      {
        sessionId,
        keeperEntry: keeperEntry ?? null,
        tableRecord: ownershipTable?.records?.[sessionId] ?? null,
        liveSession: liveSession ?? null,
        failures,
      },
    );
  }

  return {
    sessionId,
    keeperPid,
    tablePid,
    livePid,
    keeperEpoch: typeof keeperEntry?.epoch === 'number' ? keeperEntry.epoch : null,
    keeperSequenceOffset:
      typeof keeperEntry?.sequenceOffset === 'number' ? keeperEntry.sequenceOffset : null,
    /** What proved it, named so the report shows the mechanism instead of a bare boolean. */
    provedBy: [
      'keeper OWNERSHIP_LIST entry with owner=keeper',
      'durable ownership table keeperOwnershipEnabled=true',
      'durable record owner=keeper for this session id',
      'pid agreement between keeper registry, durable record and describeLiveSessions',
    ],
  };
}

/**
 * Waits for the next `count` bytes on a stream and returns them.
 *
 * Woken by data arriving, so it neither polls nor sleeps; the deadline only ends a stuck sample.
 */
function takeBytes(stream, count, label, timeoutMs) {
  return waitFor(
    label,
    timeoutMs,
    (onChange) => stream.onData(onChange),
    () => {
      if (stream.failed) throw stream.failed;
      return stream.pending() >= count ? stream.take(count) : null;
    },
  );
}

/**
 * Waits for the echo child's in-band readiness marker and asserts it arrived byte-identical.
 *
 * The child prints the marker only after its tty is already in the state the samples are measured
 * in, so nothing has to guess how long a shell needs to start. Consuming it here keeps it out of
 * every measured payload, and comparing it byte-for-byte is the first proof that the path returns
 * exactly what it was given.
 */
async function awaitEchoReady(output, name, nonce, timeoutMs) {
  const marker = Buffer.from(`READY:${nonce}\n`, 'ascii');
  const seen = await takeBytes(output, marker.length, `${name} echo readiness`, timeoutMs);
  if (!seen.equals(marker)) {
    throw gap(
      GAP.ECHO_IDENTITY_UNPROVEN,
      `${name}: the readiness marker did not come back as written (saw ${JSON.stringify(seen.toString('latin1'))})`,
    );
  }
}

// ---------------------------------------------------------------------------------------------
// Arm A: the direct-owner baseline. A real daemon, ownership flag off, no keeper involved.
// ---------------------------------------------------------------------------------------------

class DirectDaemonArm {
  constructor(paths, env, binaries, options) {
    this.name = 'direct-daemon';
    this.kind = 'direct-owner-via-daemon';
    this.paths = paths;
    this.env = env;
    this.binaries = binaries;
    this.options = options;
    this.daemon = null;
    this.control = null;
    this.tap = null;
    this.output = new ByteStream();
    this.sessionId = null;
    this.spawnReply = null;
    this.liveSession = null;
    this.lastReply = null;
  }

  async start() {
    this.daemon = Supervised.start(
      'daemon',
      this.binaries.daemon,
      ['--daemon'],
      this.env,
      this.paths.workDir,
      GAP.DAEMON_BINARY_ABSENT,
    );
    try {
      await this.daemon.waitForToken('stdout', DAEMON_READY_TOKEN, 30_000);
    } catch (error) {
      throw gap(GAP.DAEMON_START_FAILED, `the daemon never became ready: ${error.message}`, {
        stderr: this.daemon.stderr.slice(-1500),
      });
    }

    this.control = await LineChannel.connect(this.paths.daemonSocketPath, this.name);
    const handshake = await this.control.call(
      { type: 'handshake', version: DAEMON_PROTOCOL_VERSION },
      `${this.name} handshake`,
    );
    if (handshake.type !== 'handshakeOk') {
      throw fail(`${this.name}: unexpected handshake reply ${JSON.stringify(handshake).slice(0, 300)}`);
    }

    const workspaceId = `p99-${randomUUID()}`;
    await this.control.call(
      { type: 'registerWorkspace', workspaceId, repoRoot: this.paths.workDir },
      `${this.name} registerWorkspace`,
    );

    /**
     * The direct arm cannot pass an argv: `Spawn` takes a shell *preference*, which the daemon
     * resolves into `<shell> -l` on macOS. So the echo child is reached by making the "shell" itself
     * the fixture: a wrapper the daemon is told to use as its shell. That is a real difference from
     * the keeper arm, which spawns a child argv directly, and the report records it as
      * `workload.residualAsymmetryNote` rather than glossing over it. `residualAsymmetry` itself is
      * `null` because the keeper-daemon arm uses this same wrapper, so the wrapper is no longer a
      * difference between the two arms that are actually compared.
     */
    writeFileSync(
      this.paths.wrapperPath,
      [
        '#!/bin/sh',
        '# Direct-arm fixture: set the measured tty state, announce readiness, then echo bytes back.',
        // Byte-identical to the keeper arm's `argv`, built from the same script text, so the two arms
        // differ only in the process that starts the child -- never in what the child runs.
        `exec /bin/sh -c '${echoChildScript(this.options.readyNonce)}'`,
        '',
      ].join('\n'),
      { mode: 0o700 },
    );
    chmodSync(this.paths.wrapperPath, 0o700);

    this.sessionId = randomUUID();
    this.spawnReply = await this.control.call(
      {
        type: 'spawn',
        clientRequestId: `p99-spawn-${randomUUID()}`,
        workspaceId,
        worktree: null,
        cwd: this.paths.workDir,
        cols: this.options.cols,
        rows: this.options.rows,
        shell: this.paths.wrapperPath,
        sessionId: this.sessionId,
      },
      `${this.name} spawn`,
    );
    if (this.spawnReply.type !== 'spawnOk') {
      throw gap(
        GAP.DAEMON_SPAWN_REFUSED,
        `${this.name}: spawn did not answer spawnOk: ${JSON.stringify(this.spawnReply).slice(0, 500)}`,
        this.spawnReply,
      );
    }
    this.sessionId = this.spawnReply.sessionId ?? this.sessionId;

    // Ownership evidence. A session the daemon owns directly has a local child pid; the check fails
    // the arm rather than measuring a path whose owner is unknown.
    await this.describeOwnership();

    /**
     * The tap is a second connection, attached with no history. Its sink is wired as the connection
     * is constructed, so an output frame can never be queued as a reply and every streamed byte
     * reaches the measurement stream instead.
     */
    this.tap = await LineChannel.connect(this.paths.daemonSocketPath, `${this.name} tap`, {
      sink: (frame) => this.onTapFrame(frame),
    });
    await this.tap.call({ type: 'handshake', version: DAEMON_PROTOCOL_VERSION }, `${this.name} tap handshake`);
    const attached = await this.tap.call(
      { type: 'attach', sessionId: this.sessionId, afterSequence: null },
      `${this.name} attach`,
    );
    if (attached.type !== 'attachOk') {
      throw fail(`${this.name}: attach did not answer attachOk: ${JSON.stringify(attached).slice(0, 400)}`);
    }
    const ownership = readOwnership(this.paths);
    this.ownership = {
      ...ownership,
      daemonLiveSession: this.liveSession ?? null,
      daemonSocketPath: this.paths.daemonSocketPath,
      daemonExecutable: this.binaries.daemon,
    };
    if (ownership.keeperOwnershipEnabled) {
      throw gap(
        GAP.KEEPER_OWNERSHIP_UNREACHABLE,
        `${this.name}: the ownership table says keeper ownership is on, so this run is not a direct baseline`,
        ownership,
      );
    }
  }

  /** Routes one streamed daemon frame into the measurement stream, or fails the stream. */
  onTapFrame(frame) {
    if (frame.type === 'output' && frame.sessionId === this.sessionId) {
      this.output.push(unb64(frame.data));
      return;
    }
    // A gap means bytes were lost, so no sample spanning it could be an identity match.
    this.output.fail(
      gap(
        GAP.LIVE_PUSH_ABSENT,
        `${this.name}: the daemon reported an output gap, so the stream is no longer continuous`,
        frame,
      ),
    );
  }

  async describeOwnership() {
    const reply = await this.control.call(
      { type: 'describeLiveSessions' },
      `${this.name} describeLiveSessions`,
    );
    const sessions = reply.sessions ?? [];
    this.liveSession = sessions.find((session) => session.backendId === this.sessionId)
      ?? sessions.find((session) => session.localChildPid != null)
      ?? null;
    if (!this.liveSession) {
      throw gap(
        GAP.DAEMON_SPAWN_REFUSED,
        `${this.name}: the spawned session is not among the daemon's live sessions, so its owner is unproven`,
        { sessions },
      );
    }
    if (this.liveSession.localChildPid == null) {
      throw gap(
        GAP.KEEPER_OWNERSHIP_UNREACHABLE,
        `${this.name}: the session has no local child pid, which means the daemon is not the direct owner`,
        this.liveSession,
      );
    }
  }

  /**
   * Issues one write and returns the monotonic instant it was issued at.
   *
   * `write` is answered with `writeOk`, and an unread reply would later be mistaken for another
   * request's answer. The waiter is therefore armed here, before the timestamp is taken and long
   * before the bytes leave, and `settleWrite()` collects it after the echo has been matched -- so
   * awaiting the reply never lands inside the measured window.
   */
  write(bytes) {
    this.lastReply = this.control.next(`${this.name} write`, 30_000);
    const issuedAt = monotonicMs();
    this.control.send({ type: 'write', sessionId: this.sessionId, data: b64(bytes) });
    return issuedAt;
  }

  /** Awaits the reply to the last `write()` and fails the sample if the daemon refused it. */
  async settleWrite() {
    if (!this.lastReply) return null;
    const reply = await this.lastReply;
    this.lastReply = null;
    if (reply.type === 'error') {
      throw gap(
        'DAEMON_ERROR',
        `${this.name}: the daemon refused a write: ${reply.message ?? JSON.stringify(reply).slice(0, 300)}`,
        reply,
      );
    }
    return reply;
  }

  /**
   * Consumes the echo child's `READY:<nonce>` line before any sample is taken.
   *
   * The attach above asked for full history, so the daemon replays whatever the child already
   * printed -- which by then includes the readiness line. Waiting for it here, through the same
   * stream the samples use, both proves the echo path end to end and keeps the marker out of the
   * first measured payload.
   */
  async armReadiness(nonce, timeoutMs) {
    await awaitEchoReady(this.output, this.name, nonce, timeoutMs);
    // Anything the child printed after the marker but before sampling would shift the first
    // payload's bytes, so it is flushed and then proven empty.
    this.output.reset();
  }

  async stop() {
    this.tap?.close();
    this.control?.close();
    if (this.daemon) await this.daemon.stop();
  }
}

// ---------------------------------------------------------------------------------------------
// Arm B: the keeper data path, over the keeper's own socket. A labelled diagnostic, not acceptance.
//
// This arm deliberately bypasses the daemon, so its boundary has no daemon hop and its numbers are
// not comparable with a daemon arm's. It is kept because it is the cheapest way to see whether the
// keeper's own master-read path is behaving, but the report never subtracts it from a daemon arm and
// `comparisonOf` never selects it.
//
// The comparable arm is `KeeperDaemonArm` (arm C): the keeper reached *through* the daemon. That arm
// is reachable in this build -- `keeper_startup::start_production` calls
// `KeeperRuntime::activate_new_sessions`, which flips the ownership flag after `KeeperSocketProbe`
// proves a live keeper on the same runtime socket -- provided the daemon runs from a bundle-shaped
// directory, which arm C stages for itself.
// ---------------------------------------------------------------------------------------------

class KeeperArm {
  constructor(paths, env, binaries, options) {
    this.name = 'keeper-uds';
    this.kind = 'keeper-owned-via-keeper-socket';
    this.paths = paths;
    this.env = env;
    this.binaries = binaries;
    this.options = options;
    this.keeper = null;
    this.verbs = null;
    this.tap = null;
    this.output = new ByteStream();
    this.sessionId = null;
    this.epoch = null;
  }

  async start() {
    // The keeper refuses a socket whose parent is group- or world-writable; the runtime dir is 0700.
    this.keeper = Supervised.start(
      'keeper',
      this.binaries.keeper,
      ['--socket', this.paths.keeperSocketPath],
      this.env,
      this.paths.workDir,
      GAP.KEEPER_BINARY_ABSENT,
    );
    try {
      // `main.rs` prints "ferryx-ptykeeper <version> listening on <path>" on stderr once bound.
      await this.keeper.waitForToken('stderr', KEEPER_READY_TOKEN, 20_000);
    } catch (error) {
      throw gap(GAP.KEEPER_START_FAILED, `the keeper never began listening: ${error.message}`, {
        stderr: this.keeper.stderr.slice(-1500),
      });
    }

    // One connection issues verbs; the other is the session's output tap, because the wire allows
    // exactly one role per connection and an attached connection is the tap.
    this.verbs = await KeeperConnection.connect(this.paths.keeperSocketPath, `${this.name} verbs`);
    const hello = await this.verbs.call(KIND.Hello, {}, KIND.HelloAck, `${this.name} hello`);

    this.sessionId = randomUUID();
    const spawn = await this.verbs.call(
      KIND.Spawn,
      {
        sessionId: this.sessionId,
        requestId: `p99-spawn-${randomUUID()}`,
        argv: echoChildArgv(this.options.readyNonce),
        env: this.envAsPairs(),
        cwd: this.paths.workDir,
        cols: this.options.cols,
        rows: this.options.rows,
        epoch: 1,
      },
      KIND.SpawnAck,
      `${this.name} spawn`,
    );
    if (spawn.sessionId !== this.sessionId) {
      throw gap(
        GAP.KEEPER_SPAWN_REFUSED,
        `${this.name}: the keeper acknowledged session ${spawn.sessionId}, not ${this.sessionId}`,
        spawn,
      );
    }

    /**
     * Attach fences the epoch, and every later write must present the newest one. A second
     * connection attaching at epoch 2 makes 2 the epoch in force, so the verb connection writes at 2
     * rather than the 1 the spawn used.
     */
    this.tap = await KeeperConnection.connect(this.paths.keeperSocketPath, `${this.name} tap`);
    const attach = await this.tap.call(
      KIND.Attach,
      { sessionId: this.sessionId, epoch: 2, afterSequence: 0, liveOutput: true },
      KIND.AttachAck,
      `${this.name} attach`,
    );
    this.epoch = typeof attach.epoch === 'number' ? attach.epoch : 2;
    if (attach.data) this.output.push(unb64(attach.data));
    if (attach.replayGap) {
      throw gap(
        GAP.LIVE_PUSH_ABSENT,
        `${this.name}: the attach reported a replay gap, so the output stream is not continuous`,
        attach.replayGap,
      );
    }
    this.ownership = {
      keeperPid: hello.pid ?? null,
      keeperVersion: hello.keeperVersion ?? null,
      protocolMajor: hello.protocolMajor ?? null,
      protocolMinor: hello.protocolMinor ?? null,
      spawnSequenceOffset: spawn.sequenceOffset ?? null,
      spawnedPid: spawn.pid ?? null,
      epoch: this.epoch,
    };
  }

  /**
   * Consumes the echo child's `READY:<nonce>` line before any sample is taken.
   *
   * The attach replays the session's retained suffix, which by now includes the readiness line, so
   * this reads through the same stream and the same tap the samples use. That makes readiness a
   * property of the measured path rather than of a shell having had time to start, and it leaves the
   * first measured payload unshifted.
   */
  async armReadiness(nonce, timeoutMs) {
    await awaitEchoReady(this.output, this.name, nonce, timeoutMs);
    // Any byte the child printed after the marker would shift the first payload, so it is flushed
    // and the flush is proven empty before sampling starts.
    this.output.reset();
  }

  /** The keeper's `env` accepts an object or `[key, value]` pairs; pairs keep the order explicit. */
  envAsPairs() {
    return Object.entries(this.env).filter(([, value]) => typeof value === 'string');
  }

  /**
   * Issues one write and returns the monotonic instant it was issued at.
   *
   * The reply is correlated by request id and awaited by `settle()`, not here: taking the timestamp
   * after an `await` would fold the event loop's own latency into the measurement.
   */
  write(bytes) {
    if (bytes.length > KEEPER_MAX_WRITE_BYTES) {
      throw fail(`payload of ${bytes.length} bytes exceeds the keeper's ${KEEPER_MAX_WRITE_BYTES}-byte write cap`);
    }
    const requestId = randomUUID();
    const encoded = this.verbs.encode(
      KIND.Write,
      { sessionId: this.sessionId, epoch: this.epoch, data: b64(bytes) },
      requestId,
    );
    const reply = this.verbs.expect(KIND.Write, requestId, `${this.name} write`);
    const issuedAt = monotonicMs();
    this.verbs.socket.write(encoded);
    this.lastReply = reply;
    return issuedAt;
  }

  /** Awaits the write reply issued by the last `write()` and fails on a typed keeper refusal. */
  async settleWrite() {
    if (!this.lastReply) return null;
    const reply = await this.lastReply;
    this.lastReply = null;
    return reply;
  }

  async stop() {
    // `KILL` only fences an epoch the keeper already knows, and this connection holds the newest one.
    if (this.sessionId && this.epoch != null) {
      await this.verbs
        ?.call(
          KIND.Kill,
          { sessionId: this.sessionId, epoch: this.epoch },
          KIND.Kill,
          `${this.name} kill`,
          5_000,
        )
        .catch(() => null);
    }
    this.tap?.close();
    this.verbs?.close();
    if (this.keeper) await this.keeper.stop();
  }
}

// ---------------------------------------------------------------------------------------------
// Arm C: the comparable arm. Keeper-owned PTY, reached through the daemon.
//
// This is the arm the plan's risk row asks for, and the only one whose numbers may be compared with
// the direct baseline. Its client half is byte-identical to `DirectDaemonArm`: the same NDJSON
// handshake, the same `spawn`/`attach`/`write` verbs, the same payloads, the same readiness contract,
// the same measurement loop. The single difference is which process holds the PTY master, and that
// is decided by the *daemon*, not by this harness: the daemon routes the spawn to its keeper when
// `OwnershipTable::new_session_owner()` says `keeper`.
//
// Why the ownership flag can be on in an isolated runtime, without touching anything of the user's:
// `keeper_session::activate_ownership` flips the flag only after `KeeperSocketProbe` gets a
// `HELLO`/`HELLO_ACK` on `get_runtime_dir()/keeper.sock`. That runtime dir is this fixture's
// (`FERRYX_RUNTIME_DIR`), so the keeper this harness starts *is* the keeper the daemon proves and
// routes to. `may_activate` is false for an isolated runtime, so no `launchctl` call is made and no
// LaunchAgent exists in this path at all.
//
// Why the bundle is staged: `keeper_startup::start_production` returns `PayloadAbsent` unless the
// running executable sits at `Contents/MacOS/<exe>` with a payload at `../Resources/keeper`. A plain
// `target/release/ferryx` therefore never installs a keeper, never activates ownership, and silently
// owns every session directly -- which would make this arm a second copy of the baseline under a
// different name. Staging the bundle is what makes the arm real, and `verifyKeeperOwned()` is what
// proves it did not silently degrade.
// ---------------------------------------------------------------------------------------------

class KeeperDaemonArm {
  constructor(paths, env, binaries, options) {
    this.name = 'keeper-daemon';
    this.kind = 'keeper-owned-via-daemon';
    this.paths = paths;
    this.env = env;
    this.binaries = binaries;
    this.options = options;
    this.keeper = null;
    this.daemon = null;
    this.control = null;
    this.tap = null;
    /** Verbs-only connection to the keeper this arm started; never attached. */
    this.keeperVerbs = null;
    this.output = new ByteStream();
    this.sessionId = null;
    this.spawnReply = null;
    this.liveSession = null;
    this.lastReply = null;
    this.bundle = null;
    this.keeperHandshake = null;
  }

  async start() {
    // Fail closed: if isolation cannot be proven, this arm never starts a process at all.
    assertIsolation(this.paths);

    // 1. Stage the bundle, so the daemon that runs is one that can install and prove a keeper.
    this.bundle = stageDaemonBundle(this.paths, this.binaries);

    // 2. Start the keeper on this fixture's runtime socket. The daemon proves *this* process,
    //    because `KeeperSocketProbe::for_runtime()` resolves the same `FERRYX_RUNTIME_DIR`.
    this.keeper = Supervised.start(
      'keeper',
      this.paths.bundledKeeperPath,
      ['--socket', this.paths.keeperSocketPath],
      this.env,
      this.paths.workDir,
      GAP.KEEPER_BINARY_ABSENT,
    );
    try {
      await this.keeper.waitForToken('stderr', KEEPER_READY_TOKEN, 20_000);
    } catch (error) {
      throw gap(GAP.KEEPER_START_FAILED, `the keeper never began listening: ${error.message}`, {
        stderr: this.keeper.stderr.slice(-1500),
      });
    }

    // The keeper's own handshake, on a verbs-only connection. It is never attached, so it can never
    // steal the session's output tap away from the daemon (one role per connection on this wire).
    this.keeperVerbs = await KeeperConnection.connect(this.paths.keeperSocketPath, `${this.name} keeper verbs`);
    this.keeperHandshake = await this.keeperVerbs.call(
      KIND.Hello,
      {},
      KIND.HelloAck,
      `${this.name} keeper hello`,
    );

    // 3. Start the daemon from inside the bundle, so `bundle_keeper_dir` finds the payload.
    this.daemon = Supervised.start(
      'daemon',
      this.paths.bundledDaemonPath,
      ['--daemon'],
      this.env,
      this.paths.workDir,
      GAP.KEEPER_BUNDLE_UNUSABLE,
    );
    try {
      await this.daemon.waitForToken('stdout', DAEMON_READY_TOKEN, 30_000);
    } catch (error) {
      throw gap(GAP.DAEMON_START_FAILED, `the daemon never became ready: ${error.message}`, {
        stderr: this.daemon.stderr.slice(-1500),
      });
    }

    // 4. The activation the daemon performed is asserted, not assumed. `start_production` logs
    //    `keeper ownership activation` with the `ActivationOutcome`; `NotProven` means the probe did
    //    not see a live keeper and new sessions stayed direct-owned.
    const activation = readActivation(this.daemon);
    if (activation.outcome === null) {
      throw gap(
        GAP.KEEPER_OWNERSHIP_UNPROVEN,
        `${this.name}: the daemon logged no keeper ownership activation, so routing to a keeper was never proven`,
        { stderr: this.daemon.stderr.slice(-2000) },
      );
    }
    if (activation.outcome === 'NotProven') {
      throw gap(
        GAP.KEEPER_OWNERSHIP_UNPROVEN,
        `${this.name}: the daemon's keeper probe did not prove a live keeper, so new sessions stay direct-owned`,
        activation,
      );
    }

    // 5. The ownership table must already be on before the spawn. `new_session_owner()` is consulted
    //    only at spawn time, so a flag written later would leave this session direct-owned.
    const tableBefore = readOwnership(this.paths);
    if (tableBefore.keeperOwnershipEnabled !== true) {
      throw gap(
        GAP.KEEPER_OWNERSHIP_UNPROVEN,
        `${this.name}: the durable table does not route new sessions to the keeper before the spawn`,
        { ownershipPath: this.paths.ownershipPath, activation, table: tableBefore },
      );
    }

    // 6. Same client handshake and the same spawn verb as the direct arm.
    this.control = await LineChannel.connect(this.paths.daemonSocketPath, this.name);
    const handshake = await this.control.call(
      { type: 'handshake', version: DAEMON_PROTOCOL_VERSION },
      `${this.name} handshake`,
    );
    if (handshake.type !== 'handshakeOk') {
      throw fail(`${this.name}: unexpected handshake reply ${JSON.stringify(handshake).slice(0, 300)}`);
    }

    const workspaceId = `p99-${randomUUID()}`;
    await this.control.call(
      { type: 'registerWorkspace', workspaceId, repoRoot: this.paths.workDir },
      `${this.name} registerWorkspace`,
    );

    // The same wrapper the direct arm uses, for the same reason: `Spawn` takes a shell preference,
    // not an argv. Using the identical file keeps the child's argv/env identical across arms.
    writeFileSync(
      this.paths.wrapperPath,
      [
        '#!/bin/sh',
        '# Keeper-daemon-arm fixture: set the measured tty state, announce readiness, then echo.',
        `exec /bin/sh -c '${echoChildScript(this.options.readyNonce)}'`,
        '',
      ].join('\n'),
      { mode: 0o700 },
    );
    chmodSync(this.paths.wrapperPath, 0o700);

    this.sessionId = randomUUID();
    this.spawnReply = await this.control.call(
      {
        type: 'spawn',
        clientRequestId: `p99-spawn-${randomUUID()}`,
        workspaceId,
        worktree: null,
        cwd: this.paths.workDir,
        cols: this.options.cols,
        rows: this.options.rows,
        shell: this.paths.wrapperPath,
        sessionId: this.sessionId,
      },
      `${this.name} spawn`,
    );
    if (this.spawnReply.type !== 'spawnOk') {
      throw gap(
        GAP.DAEMON_SPAWN_REFUSED,
        `${this.name}: spawn did not answer spawnOk: ${JSON.stringify(this.spawnReply).slice(0, 500)}`,
        this.spawnReply,
      );
    }
    this.sessionId = this.spawnReply.sessionId ?? this.sessionId;

    // 7. The gate. Nothing is measured until the keeper is proven to hold this exact session.
    this.ownership = await this.verifyKeeperOwned();

    // 8. The tap: a second daemon connection, sink wired at construction, exactly as in arm A.
    //    The daemon is the session's output tap in this arm, because the *daemon* attached to the
    //    keeper; the harness only ever attaches to the daemon.
    this.tap = await LineChannel.connect(this.paths.daemonSocketPath, `${this.name} tap`, {
      sink: (frame) => this.onTapFrame(frame),
    });
    await this.tap.call({ type: 'handshake', version: DAEMON_PROTOCOL_VERSION }, `${this.name} tap handshake`);
    const attached = await this.tap.call(
      { type: 'attach', sessionId: this.sessionId, afterSequence: null },
      `${this.name} attach`,
    );
    if (attached.type !== 'attachOk') {
      throw fail(`${this.name}: attach did not answer attachOk: ${JSON.stringify(attached).slice(0, 400)}`);
    }
  }

  /**
   * Collects the three independent views of this session's owner and requires them to agree.
   *
   * The keeper's registry is read over a verbs-only connection this arm owns, so the answer can only
   * describe sessions held by the keeper this harness started on this fixture's socket. That is what
   * makes the proof causal: a session id appearing there can only be there because the daemon sent a
   * `SPAWN` to it.
   */
  async verifyKeeperOwned() {
    const reply = await this.keeperVerbs.call(
      KIND.OwnershipQuery,
      {},
      KIND.OwnershipList,
      `${this.name} keeper ownership list`,
    );
    // `server.rs::Kind::OwnershipQuery` answers `{sessions: [{id, owner, pid, epoch, ...}]}`.
    const keeperSessions = reply.sessions ?? [];

    const liveReply = await this.control.call(
      { type: 'describeLiveSessions' },
      `${this.name} describeLiveSessions`,
    );
    const liveSession =
      (liveReply.sessions ?? []).find((session) => session.backendId === this.sessionId) ?? null;

    const proven = verifyKeeperOwnershipEvidence({
      sessionId: this.sessionId,
      keeperSessions,
      ownershipTable: readOwnership(this.paths),
      liveSession,
    });

    return {
      ...proven,
      keeperPidFromHandshake:
        typeof this.keeperHandshake?.pid === 'number' ? this.keeperHandshake.pid : null,
      keeperVersion: this.keeperHandshake?.keeperVersion ?? null,
      protocolMajor: this.keeperHandshake?.protocolMajor ?? null,
      protocolMinor: this.keeperHandshake?.protocolMinor ?? null,
      bundle: this.bundle,
      daemonExecutable: this.paths.bundledDaemonPath,
      daemonSocketPath: this.paths.daemonSocketPath,
      routingVerb: 'daemon spawn -> keeper SPAWN (routed by the daemon, not by the harness)',
    };
  }

  /** Routes one streamed daemon frame into the measurement stream, or fails the stream. */
  onTapFrame(frame) {
    if (frame.type === 'output' && frame.sessionId === this.sessionId) {
      this.output.push(unb64(frame.data));
      return;
    }
    this.output.fail(
      gap(
        GAP.LIVE_PUSH_ABSENT,
        `${this.name}: the daemon reported an output gap, so the stream is no longer continuous`,
        frame,
      ),
    );
  }

  /** Same contract as the direct arm's: the reply waiter is armed before the timestamp is taken. */
  write(bytes) {
    this.lastReply = this.control.next(`${this.name} write`, 30_000);
    const issuedAt = monotonicMs();
    this.control.send({ type: 'write', sessionId: this.sessionId, data: b64(bytes) });
    return issuedAt;
  }

  /** Awaits the reply to the last `write()` and fails the sample if the daemon refused it. */
  async settleWrite() {
    if (!this.lastReply) return null;
    const reply = await this.lastReply;
    this.lastReply = null;
    if (reply.type === 'error') {
      throw gap(
        'DAEMON_ERROR',
        `${this.name}: the daemon refused a write: ${reply.message ?? JSON.stringify(reply).slice(0, 300)}`,
        reply,
      );
    }
    return reply;
  }

  /** Same in-band readiness contract as every other arm. */
  async armReadiness(nonce, timeoutMs) {
    await awaitEchoReady(this.output, this.name, nonce, timeoutMs);
    this.output.reset();
  }

  async stop() {
    // The session is closed through the daemon, which forwards a KILL to the keeper under its own
    // epoch. Signalling the keeper directly would fence an epoch the daemon still holds.
    if (this.sessionId) {
      await this.control
        ?.call({ type: 'close', sessionId: this.sessionId }, `${this.name} close`, 5_000)
        .catch(() => null);
    }
    this.tap?.close();
    this.control?.close();
    this.keeperVerbs?.close();
    // The daemon is stopped before the keeper: a keeper that outlives its daemon is exactly the
    // survivor this fixture must not leave behind.
    if (this.daemon) await this.daemon.stop();
    if (this.keeper) await this.keeper.stop();
  }
}

/**
 * Reads the daemon's keeper activation outcome out of its log output.
 *
 * `keeper_startup::start_production` emits exactly one of two things on the keeper path: an
 * `info!` line `keeper ownership activation` carrying `outcome=<ActivationOutcome>`, or a `warn!`
 * line saying ownership could not be activated. `ActivationOutcome` is
 * `{NotProven, AlreadyActive, Activated { keeper_pid }}`, so `NotProven` is the one value that means
 * "new sessions stayed direct-owned".
 *
 * Returns `{outcome: null}` when neither line was seen, which the caller treats as unproven rather
 * than as success: silence is not evidence that routing happened.
 */
function readActivation(supervised) {
  const text = `${supervised.stdout}\n${supervised.stderr}`;
  const lines = text.split('\n').filter((line) => line.includes('keeper ownership activation'));
  const last = lines.length > 0 ? lines[lines.length - 1] : null;
  if (!last) {
    return {
      outcome: null,
      line: null,
      sawFailureWarning: text.includes('keeper ownership could not be activated'),
    };
  }
  // The tracing formatter writes `outcome=NotProven`, `outcome=AlreadyActive`, or
  // `outcome=Activated { keeper_pid: Some(123) }`.
  const match = /outcome=([A-Za-z]+)/.exec(last);
  return {
    outcome: match ? match[1] : null,
    line: last.slice(0, 400),
    sawFailureWarning: text.includes('keeper ownership could not be activated'),
  };
}

// ---------------------------------------------------------------------------------------------
// The measurement loop. Written once and used by every arm, so no arm can accidentally measure a
// different quantity than its neighbour.
// ---------------------------------------------------------------------------------------------

/**
 * One timed sample: write one payload, wait for exactly those bytes to come back, report elapsed
 * monotonic time.
 *
 * The read is armed *before* the write is issued, so an echo that arrives faster than the following
 * `await` is still matched to this sample rather than lost. The payload's own nonce makes the match
 * an identity check, not a byte count.
 */
async function sampleOnce(arm, index, options) {
  const { bytes, nonce } = makePayload(index, options.payloadBytes);
  const echoed = takeBytes(arm.output, bytes.length, `${arm.name} sample ${index}`, options.timeoutMs);
  const issuedAt = arm.write(bytes);
  const observed = await echoed;
  const observedAt = monotonicMs();
  if (!observed.equals(bytes)) {
    throw gap(
      GAP.ECHO_IDENTITY_UNPROVEN,
      `${arm.name}: sample ${index} (nonce ${nonce}) came back different: ${JSON.stringify(observed.toString('latin1'))}`,
      { expected: bytes.toString('latin1'), observed: observed.toString('latin1') },
    );
  }
  // The write's own acknowledgement is drained per sample, so it cannot accumulate and be mistaken
  // for another reply later.
  await arm.settleWrite();
  return { elapsedMs: observedAt - issuedAt, issuedAt, observedAt };
}

/**
 * Honest interleaving.
 *
 * Every arm stays live for the whole run and is sampled in alternating blocks, with the order
 * reversed each round (A/B, then B/A). A machine that drifts during the run -- thermal throttling,
 * background load, a scheduled task -- therefore lands on both arms instead of entirely on whichever
 * one ran second.
 */
function blockPlan(repeats, names) {
  const plan = [];
  for (let block = 0; block < repeats; block += 1) {
    // Derived from the arms actually selected, so a single-arm run is legal and a reversed order is
    // only ever a permutation of what was asked for.
    plan.push(block % 2 === 0 ? [...names] : [...names].reverse());
  }
  return plan;
}

/**
 * Runs every timed sample for every arm.
 *
 * Warmup samples are executed and discarded: the first writes through a fresh socket pay for lazily
 * allocated buffers and are not representative of steady typing. After each block the arm's stream
 * must be empty -- any leftover byte means a sample consumed someone else's data and the numbers are
 * not trustworthy, so the run fails instead of reporting them.
 */
async function measure({ arms, options, onProgress }) {
  const results = new Map(arms.map((arm) => [arm.name, []]));

  /**
   * Readiness first, for every arm, before one sample is written.
   *
   * Each arm waits on its own in-band `READY:<nonce>` line through the stream the samples will use,
   * so "the shell had time to start" is never an assumption. An arm that cannot prove readiness
   * fails here instead of producing a first sample that matched the wrong bytes.
   */
  const readiness = [];
  for (const arm of arms) {
    await arm.armReadiness(options.readyNonce, options.timeoutMs);
    readiness.push({ arm: arm.name, marker: `READY:${options.readyNonce}`, proven: true });
  }

  for (const arm of arms) {
    for (let index = 0; index < options.warmup; index += 1) {
      await sampleOnce(arm, 0x1000 + index, options);
    }
    const residual = arm.output.pending();
    if (residual !== 0) {
      throw gap(
        GAP.RESIDUAL_UNCONSUMED_BYTES,
        `${arm.name}: ${residual} bytes were echoed that no sample accounted for, so samples are ambiguous`,
        { preview: arm.output.preview() },
      );
    }
  }

  const plan = blockPlan(options.repeats, arms.map((arm) => arm.name));
  const perBlock = Math.floor(options.samples / options.repeats);
  if (perBlock < 1) {
    throw new HarnessError(
      'USAGE',
      `--samples ${options.samples} is smaller than --repeats ${options.repeats}, so a block would be empty`,
    );
  }
  let written = 0;
  for (const [blockIndex, order] of plan.entries()) {
    for (const name of order) {
      const arm = arms.find((candidate) => candidate.name === name);
      for (let i = 0; i < perBlock; i += 1) {
        const sample = await sampleOnce(arm, written, options);
        results.get(arm.name).push(Number(sample.elapsedMs.toFixed(4)));
        written += 1;
      }
      const residual = arm.output.pending();
      if (residual !== 0) {
        throw gap(
          GAP.RESIDUAL_UNCONSUMED_BYTES,
          `${arm.name}: block ${blockIndex + 1} left ${residual} unaccounted bytes, so the stream and the samples disagree`,
          { preview: arm.output.preview() },
        );
      }
      onProgress?.(`block ${blockIndex + 1}/${plan.length} ${name}: ${results.get(name).length} samples`);
    }
  }
  return { results, readiness };
}

// ---------------------------------------------------------------------------------------------
// Provenance. Every number in the report traces back to the exact bytes that produced it.
// ---------------------------------------------------------------------------------------------

function fileFacts(filePath) {
  try {
    const stats = statSync(filePath);
    const digest = createHash('sha256').update(readFileSync(filePath)).digest('hex');
    return {
      path: filePath,
      exists: true,
      bytes: stats.size,
      modifiedAt: stats.mtime.toISOString(),
      sha256: digest,
    };
  } catch (error) {
    return { path: filePath, exists: false, error: String(error.message ?? error) };
  }
}

function hostFacts() {
  const cpus = os.cpus() ?? [];
  return {
    platform: process.platform,
    arch: process.arch,
    release: os.release(),
    nodeVersion: process.version,
    cpuModel: cpus[0]?.model ?? null,
    cpuCount: cpus.length || null,
    loadAverageAtStart: os.loadavg(),
    totalMemoryBytes: os.totalmem(),
  };
}

// ---------------------------------------------------------------------------------------------
// Report shaping. Plain data only: a JSON object plus a text renderer over the same object.
// ---------------------------------------------------------------------------------------------

const REPORT_SCHEMA = 'ferryx-pty-echo-p99/v1';

/**
 * Describes each measurement boundary in words the reader can check against the code.
 *
 * The two daemon arms share one boundary; the keeper-uds arm's is strictly shorter because no daemon
 * sits in it. Naming that difference is what stops a reader from subtracting one arm's p99 from the
 * other's and calling the result "keeper overhead".
 */
const BOUNDARY = {
  'direct-daemon': {
    shape: 'harness -> daemon.sock -> daemon -> PTY master -> child tty -> child -> PTY master -> daemon -> daemon.sock -> harness',
    comparableTo: 'keeper-daemon',
    owner: 'the daemon process itself',
  },
  'keeper-daemon': {
    shape:
      'harness -> daemon.sock -> daemon -> keeper.sock -> keeper -> PTY master -> child tty -> child -> ' +
      'PTY master -> keeper -> keeper.sock -> daemon -> daemon.sock -> harness',
    comparableTo: 'direct-daemon',
    owner: 'the keeper process, reached and routed by the daemon',
    note:
      'Same client endpoints as direct-daemon: the harness enters and leaves on daemon.sock. The' +
      'difference is the interior hop, which is the quantity the plan risk row asks about.',
  },
  'keeper-uds': {
    shape: 'harness -> keeper.sock -> keeper -> PTY master -> child tty -> child -> PTY master -> keeper -> keeper.sock -> harness',
    comparableTo: null,
    owner: 'the keeper process',
    note:
      'No daemon in the path. This boundary is strictly shorter than the daemon arms\', so its numbers must ' +
      'not be subtracted from a daemon arm and reported as keeper overhead. It is a diagnostic only and ' +
      'is never accepted as the p99 comparison.',
  },
};

/**
 * Builds the comparison the report carries.
 *
 * Acceptance requires both comparable arms to have been measured **and** to have proven the same
 * daemon endpoint. The `keeper-uds` arm is deliberately excluded: its boundary has no daemon hop, so
 * subtracting it from a daemon arm would compute a number that describes two different quantities.
 * When `keeper-daemon` is absent, the report says the comparison was not obtained rather than
 * falling back to the incomparable pair.
 */
function comparisonOf(report) {
  const direct = report.arms['direct-daemon'] ?? null;
  const keeper = report.arms['keeper-daemon'] ?? null;
  const diagnostic = report.arms['keeper-uds'] ?? null;
  if (!direct || !keeper) {
    return {
      comparedArms: null,
      boundaryKind: null,
      comparable: false,
      obtained: false,
      whyNotDirectlyComparable:
        'The comparable pair (direct-daemon vs keeper-daemon) was not both measured in this run, so no ' +
        'p99 comparison exists. The keeper-uds arm, when present, is a different boundary and is never ' +
        'substituted for it.',
      diagnosticArm: diagnostic ? 'keeper-uds' : null,
      p50DeltaMs: null,
      p99DeltaMs: null,
      maxDeltaMs: null,
      passFailThreshold: null,
      verdict: 'NO COMPARISON OBTAINED. This run does not answer the plan risk row.',
    };
  }

  // Endpoint identity: both arms must have entered through a daemon.sock inside their own isolated
  // runtime. A mismatch means the numbers describe different clients and cannot be subtracted.
  const directSocket = direct.ownership?.daemonSocketPath ?? null;
  const keeperSocket = keeper.ownership?.daemonSocketPath ?? null;
  if (directSocket && keeperSocket && directSocket !== keeperSocket) {
    return {
      comparedArms: ['direct-daemon', 'keeper-daemon'],
      boundaryKind: 'mismatched-daemon-endpoints',
      comparable: false,
      obtained: false,
      whyNotDirectlyComparable: `The arms entered through different daemon sockets (${directSocket} vs ${keeperSocket}).`,
      diagnosticArm: diagnostic ? 'keeper-uds' : null,
      p50DeltaMs: null,
      p99DeltaMs: null,
      maxDeltaMs: null,
      passFailThreshold: null,
      verdict: 'NO COMPARISON OBTAINED. The daemon endpoints differ.',
    };
  }

  const delta = (key) =>
    typeof direct[key] === 'number' && typeof keeper[key] === 'number'
      ? Number((keeper[key] - direct[key]).toFixed(4))
      : null;
  return {
    comparedArms: ['direct-daemon', 'keeper-daemon'],
    boundaryKind: 'same-client-endpoints-different-interior-hop',
    comparable: true,
    obtained: true,
    whyNotDirectlyComparable:
      'Both arms enter and leave on daemon.sock with the same verbs, payloads and readiness contract. ' +
      'They differ in where the PTY master lives: in the daemon for the direct arm, in the keeper for this ' +
      'one, reached by the daemon over keeper.sock. The delta is therefore the keeper hop, which is the ' +
      'quantity the plan risk row asks about -- not a pure copy cost, because the keeper also checkpoints ' +
      'and pumps output through its own thread.',
    p50DeltaMs: delta('p50Ms'),
    p99DeltaMs: delta('p99Ms'),
    maxDeltaMs: delta('maxMs'),
    diagnosticArm: diagnostic ? 'keeper-uds' : null,
    passFailThreshold: null,
    verdict:
      'REPORT ONLY. The plan sets no numeric p99 target, so this harness does not invent one. Supply a ' +
      'threshold to turn this into a pass/fail gate.',
  };
}

function buildReport({ options, paths, host, facts, results, readiness, arms, startedAt, finishedAt }) {
  const armReports = {};
  for (const arm of arms) {
    const samples = results.get(arm.name) ?? [];
    armReports[arm.name] = {
      ...summarize(samples),
      kind: arm.kind,
      boundary: BOUNDARY[arm.name] ?? null,
      ownership: arm.ownership ?? null,
    };
  }
  const report = {
    schema: REPORT_SCHEMA,
    plan: {
      file: '.omo/plans/local-pty-crash-survival-20261116.md',
      revision: 'rev 5',
      riskRow: 'Data-path copy hurts typing latency | keeper reads the master directly; p99 echo benchmark against today\'s baseline',
    },
    baseline: {
      // Not a stored baseline and not a same-binary one. Both facts are stated, not implied.
      kind: 'measured-now-direct-owner',
      historical: false,
      historicalBaseline: null,
      storedBaselineRead: false,
      sameBinaryAsKeeperArm: false,
      explanation:
        'The baseline is the direct-owner path measured in this same run, in the same process, with the same ' +
        'payloads. No historical measurement exists in this tree and none was fabricated. The daemon binary ' +
        'and the keeper binary are different binaries.',
    },
    boundary: {
      measured: 'write issued by the harness -> the same bytes observed back on the same client stream',
      includesKernelPtyRoundTrip: true,
      includesChildProcessEcho: true,
      clock: 'process.hrtime.bigint() (monotonic); Date.now() is never used for a duration',
      perSample: 'unique nonce per payload; the echo must match those bytes exactly',
      arms: BOUNDARY,
    },
    parameters: {
      samples: options.samples,
      warmup: options.warmup,
      payloadBytes: options.payloadBytes,
      repeats: options.repeats,
      cols: options.cols,
      rows: options.rows,
      sampleTimeoutMs: options.timeoutMs,
      interleaving: 'alternating blocks, order reversed each round',
      percentileMethod: 'nearest-rank; index = ceil(p/100 * n) - 1; no interpolation',
    },
    workload: {
      echoChildArgv: echoChildArgv(options.readyNonce),
      identicalScriptInBothArms: true,
      readiness,
      readinessContract:
        'Before any sample, each arm waits on its own in-band `READY:<nonce>` line through the same stream the ' +
        'samples read, and the marker must match byte-for-byte. No sleep is used to let a shell start.',
      daemonArmShell: 'wrapper script used as the daemon shell preference, because Spawn takes no argv',
      residualAsymmetry: null,
      residualAsymmetryNote:
        'Both daemon arms (direct-daemon and keeper-daemon) spawn the child through the same wrapper file, ' +
        'because the daemon Spawn verb takes a shell preference rather than an argv. The keeper-uds arm passes ' +
        'an argv directly, but it is a diagnostic and is never compared.',
    },
    isolation: {
      root: paths.root,
      productionPathsRefused: productionRuntimeDirs(),
      allPathsInsideRoot: true,
      envKeysProvided: [
        'FERRYX_RUNTIME_DIR',
        'FERRYX_LEGACY_RUNTIME_DIR',
        'FERRYX_DATA_DIR',
        'FERRYX_SESSION_DIR',
        'FERRYX_KEEPER_DIR',
        'FERRYX_DAEMON_SUPERVISOR_NAME',
        'FERRYX_DAEMON_PLIST_PATH',
        'FERRYX_PTY_KEEPER_PLIST_PATH',
        'HOME',
        'TMPDIR',
      ],
      inheritedFerryxEnvScrubbed: true,
    },
    host,
    harness: {
      file: facts.harness,
      invocation: process.argv.slice(2),
      startedAt,
      finishedAt,
      durationMs: Number((finishedAt - startedAt).toFixed(3)),
    },
    binaries: facts.binaries,
    arms: armReports,
    comparison: null,
    gaps: [],
    limitations: [
      'The keeper-daemon arm runs the daemon from a staged bundle under the fixture root, because ' +
        'start_production derives the keeper payload from current_exe() and returns PayloadAbsent for a plain ' +
        'target/release/ferryx. The staged executable is a byte copy of --daemon-binary; the staging is fixture ' +
        'setup, not a build.',
      'The keeper-daemon arm depends on the ownership flag being flipped before the spawn. It asserts the ' +
        "daemon's logged activation outcome and the durable table before spawning, and fails with " +
        'KEEPER_OWNERSHIP_UNPROVEN rather than measuring a silently direct-owned session.',
      'describeLiveSessions is used only as a pid cross-check in the keeper-daemon arm. It lists keeper-held ' +
        'sessions as kind: "local" with the keeper-reported pid (TerminalService::describe_live_sessions), so ' +
        'its contents cannot prove ownership in either direction.',
      'The keeper-uds arm talks to keeper.sock directly, so its boundary omits the daemon hop entirely and is ' +
        'never accepted as the p99 comparison.',
      'Both daemon arms start the echo child through the same shell wrapper (Spawn takes a shell preference, ' +
        'not an argv), so this asymmetry is now shared rather than differential.',
      'The keeper hop measured by this comparison includes the keeper checkpoint cadence ' +
        '(CheckpointCadence::default(), wired by start_production) and the keeper output pump thread, which ' +
        'have no counterpart in the direct arm. The delta is a keeper-hop cost, not a pure copy cost.',
      'No numeric p99 threshold is defined by the plan, so the verdict is report-only.',
      'A single run on one machine cannot separate this build from cache, thermal and scheduler effects; only ' +
        'the interleaved block order mitigates them.',
    ],
  };
  report.comparison = comparisonOf(report);
  return report;
}

function renderText(report) {
  const lines = [];
  lines.push(`PTY echo latency comparison (${report.schema})`);
  lines.push(`plan: ${report.plan.file} ${report.plan.revision}`);
  lines.push(`host: ${report.host.platform}/${report.host.arch} ${report.host.release}, ${report.host.nodeVersion}`);
  lines.push(`root: ${report.isolation.root}`);
  lines.push(
    `baseline: ${report.baseline.kind} (historical=${report.baseline.historical}, sameBinaryAsKeeperArm=${report.baseline.sameBinaryAsKeeperArm})`,
  );
  lines.push(
    `params: samples=${report.parameters.samples} warmup=${report.parameters.warmup} payloadBytes=${report.parameters.payloadBytes} repeats=${report.parameters.repeats}`,
  );
  lines.push('');
  lines.push('arm                 n     min      p50      p90      p99      max      mean   (ms)');
  for (const [name, arm] of Object.entries(report.arms)) {
    const fmt = (value) => (typeof value === 'number' ? value.toFixed(3).padStart(8) : '       -');
    lines.push(
      `${name.padEnd(18)} ${String(arm.count).padStart(4)}${fmt(arm.minMs)}${fmt(arm.p50Ms)}${fmt(arm.p90Ms)}${fmt(arm.p99Ms)}${fmt(arm.maxMs)}${fmt(arm.meanMs)}`,
    );
  }
  lines.push('');
  lines.push('boundaries:');
  for (const [name, boundary] of Object.entries(report.boundary.arms)) {
    lines.push(`  ${name}: ${boundary.shape}`);
    if (boundary.note) lines.push(`    note: ${boundary.note}`);
  }
  const comparison = report.comparison;
  if (comparison) {
    lines.push('');
    lines.push('comparison (NOT a pure keeper-overhead figure):');
    lines.push(`  comparable: ${comparison.comparable}`);
    lines.push(`  why: ${comparison.whyNotDirectlyComparable}`);
    lines.push(`  p50 delta: ${comparison.p50DeltaMs} ms   p99 delta: ${comparison.p99DeltaMs} ms`);
    lines.push(`  verdict: ${comparison.verdict}`);
  }
  // Printed explicitly, including when null: a reader must be able to see that the asymmetry was
  // checked and found absent, rather than having to infer it from a missing line.
  if (report.workload) {
    lines.push('');
    lines.push(`workload residual asymmetry: ${report.workload.residualAsymmetry ?? 'none'}`);
    if (report.workload.residualAsymmetryNote) {
      lines.push(`  note: ${report.workload.residualAsymmetryNote}`);
    }
  }
  if (report.gaps.length > 0) {
    lines.push('');
    lines.push('gaps:');
    for (const item of report.gaps) lines.push(`  ${item.code}: ${item.message}`);
  }
  lines.push('');
  lines.push('limitations:');
  for (const item of report.limitations) lines.push(`  - ${item}`);
  return `${lines.join('\n')}\n`;
}

// ---------------------------------------------------------------------------------------------
// Command line. Every value has a default, and every override is validated before anything starts.
// ---------------------------------------------------------------------------------------------

const USAGE = `Real-PTY echo latency comparison: direct-PTY baseline vs the keeper data path.

Usage:
  node scripts/qa/pty-echo-p99-comparison.mjs \\
    --daemon-binary <path/to/ferryx> \\
    --keeper-binary <path/to/ferryx-ptykeeper> \\
    [--samples 200] [--warmup 20] [--payload-bytes 64] [--repeats 4] \\
    [--cols 120] [--rows 40] [--timeout-ms 5000] \\
    [--arms direct-daemon,keeper-uds] [--json] [--keep] [--quiet]

Every sample measures: the instant before one fixed-size payload is written, to the instant those
exact bytes have been echoed back on the same client stream. Both arms run the same echo child and
the same payloads; they differ only in which process owns the PTY.

Readiness is an in-band READY:<nonce> line awaited before the first sample, never a sleep. Each
recorded value is a raw sample; the report also carries the p50/p90/p99 computed from them.

Exit codes: 0 every selected arm measured, 1 a selected arm hit a typed gap, 2 usage error.`;

function parseArgs(argv) {
  const options = {
    daemonBinary: null,
    keeperBinary: null,
    samples: 200,
    warmup: 20,
    payloadBytes: 64,
    repeats: 4,
    cols: 120,
    rows: 40,
    timeoutMs: 5_000,
    arms: ['direct-daemon', 'keeper-daemon'],
    json: false,
    keep: false,
    quiet: false,
    readyNonce: randomUUID().replace(/-/g, '').slice(0, 16),
  };

  const needValue = (flag, index) => {
    const value = argv[index + 1];
    if (value === undefined || value.startsWith('--')) {
      throw new HarnessError('USAGE', `${flag} needs a value`);
    }
    return value;
  };
  const positiveInt = (flag, raw) => {
    const value = Number(raw);
    if (!Number.isInteger(value) || value < 1) {
      throw new HarnessError('USAGE', `${flag} must be a positive integer, got ${JSON.stringify(raw)}`);
    }
    return value;
  };

  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    switch (flag) {
      case '--daemon-binary':
        options.daemonBinary = path.resolve(needValue(flag, index));
        index += 1;
        break;
      case '--keeper-binary':
        options.keeperBinary = path.resolve(needValue(flag, index));
        index += 1;
        break;
      case '--samples':
        options.samples = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--warmup':
        options.warmup = Number(needValue(flag, index));
        if (!Number.isInteger(options.warmup) || options.warmup < 0) {
          throw new HarnessError('USAGE', '--warmup must be zero or a positive integer');
        }
        index += 1;
        break;
      case '--payload-bytes':
        options.payloadBytes = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--repeats':
        options.repeats = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--cols':
        options.cols = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--rows':
        options.rows = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--timeout-ms':
        options.timeoutMs = positiveInt(flag, needValue(flag, index));
        index += 1;
        break;
      case '--arms': {
        const requested = needValue(flag, index).split(',').map((name) => name.trim()).filter(Boolean);
        const unknown = requested.filter((name) => !(name in BOUNDARY));
        if (unknown.length > 0) {
          throw new HarnessError(
            'USAGE',
            `unknown arm(s) ${unknown.join(', ')}; known arms are ${Object.keys(BOUNDARY).join(', ')}`,
          );
        }
        options.arms = [...new Set(requested)];
        index += 1;
        break;
      }
      case '--json':
        options.json = true;
        break;
      case '--keep':
        options.keep = true;
        break;
      case '--quiet':
        options.quiet = true;
        break;
      case '--help':
      case '-h':
        options.help = true;
        break;
      default:
        throw new HarnessError('USAGE', `unknown argument ${JSON.stringify(flag)}\n\n${USAGE}`);
    }
  }

  if (options.help) return options;
  if (options.arms.length === 0) throw new HarnessError('USAGE', '--arms selected no arm');
  if (options.repeats > options.samples) {
    throw new HarnessError(
      'USAGE',
      `--repeats ${options.repeats} exceeds --samples ${options.samples}, so a block would be empty`,
    );
  }
  if (options.arms.includes('direct-daemon') && !options.daemonBinary) {
    throw new HarnessError('USAGE', '--daemon-binary is required for the direct-daemon arm');
  }
  if (
    (options.arms.includes('keeper-uds') || options.arms.includes('keeper-daemon')) &&
    !options.keeperBinary
  ) {
    throw new HarnessError(
      'USAGE',
      '--keeper-binary is required for the keeper-uds and keeper-daemon arms',
    );
  }
  // The keeper-daemon arm stages a bundle the daemon is started from, so it needs both binaries.
  if (options.arms.includes('keeper-daemon') && !options.daemonBinary) {
    throw new HarnessError('USAGE', '--daemon-binary is required for the keeper-daemon arm');
  }
  return options;
}

/**
 * Owns every child this run started, and nothing else.
 *
 * Teardown signals a child only through the `ChildProcess` handle this process created, so a pid
 * cannot be recycled into an unrelated process that happens to carry the same number. Nothing here
 * scans the process table, matches a command line, or signals a pid it did not record. `stopAll` is
 * idempotent and never throws, because it runs on failure paths where an exception would mask the
 * real error.
 */
class OwnedProcesses {
  constructor() {
    this.children = [];
  }

  adopt(supervised) {
    this.children.push(supervised);
    return supervised;
  }

  async stopAll() {
    const failures = [];
    for (const supervised of [...this.children].reverse()) {
      try {
        await supervised.stop();
      } catch (error) {
        failures.push({
          kind: supervised.kind,
          pid: supervised.pid,
          error: String(error.message ?? error),
        });
      }
    }
    this.children = [];
    return failures;
  }
}

/**
 * Runs one comparison end to end.
 *
 * The teardown is the point of the try/finally: each arm is adopted the instant it exists, so a
 * failure while starting the *second* arm still reaps the first one's child. Both arms close through
 * optional chaining because `stop()` can run against a half-built object.
 */
async function runComparison(options, log) {
  const owned = new OwnedProcesses();
  const arms = [];
  const startedAt = monotonicMs();
  let root = null;
  try {
    root = mkdtempSync(path.join(os.tmpdir(), 'ferryx-p99-echo-'));
    // mkdtempSync creates 0700 already, but the fixture's isolation depends on that exact mode, so
    // it is set rather than assumed.
    chmodSync(root, 0o700);
    const paths = planPaths(root);
    assertIsolation(paths);
    prepareDirectories(paths);
    const env = childEnv(paths, QA_KEEPER_LABEL);
    writeFileSync(
      path.join(paths.root, 'run.json'),
      `${JSON.stringify({ argv: process.argv.slice(2), options, startedAt }, null, 2)}\n`,
      { mode: 0o600 },
    );

    const binaries = { daemon: options.daemonBinary, keeper: options.keeperBinary };
    for (const name of options.arms) {
      log(`starting arm ${name}`);
      const arm = makeArm(name, paths, env, binaries, options);
      arms.push(arm);
      try {
        await arm.start();
      } catch (error) {
        // `start()` can throw after its child exists (readiness never arrives, a spawn is refused).
        // The handle is adopted before the error propagates, so teardown still reaps that child
        // instead of leaving an orphan running for the rest of the session.
        for (const child of armChildren(arm)) owned.adopt(child);
        throw error;
      }
      // Adoption happens only after the child exists, and only for the child this arm started.
      // The keeper-daemon arm starts two children, and both are adopted so neither can survive this
      // run: the daemon first, then the keeper it was routed to.
      for (const child of armChildren(arm)) owned.adopt(child);
      log(`arm ${name} ready (ownership ${JSON.stringify(arm.ownership ?? null).slice(0, 160)})`);
    }

    const { results, readiness } = await measure({
      arms,
      options,
      onProgress: (text) => log(text),
    });
    const finishedAt = monotonicMs();
    const report = buildReport({
      options,
      paths,
      host: hostFacts(),
      facts: {
        harness: fileFacts(fileURLToPath(import.meta.url)),
        binaries: {
          daemon: options.daemonBinary ? fileFacts(options.daemonBinary) : null,
          keeper: options.keeperBinary ? fileFacts(options.keeperBinary) : null,
        },
      },
      results,
      readiness,
      arms,
      startedAt,
      finishedAt,
    });
    report.cleanup = { stopFailures: [], keptRoot: options.keep ? root : null };

    // Connections are closed before the children they talk to, so a dying owner is not held open by
    // a live socket on this side.
    for (const arm of arms) closeArmConnections(arm);
    report.cleanup.stopFailures = await owned.stopAll();
    return report;
  } finally {
    for (const arm of arms) closeArmConnections(arm);
    const stopFailures = await owned.stopAll();
    if (stopFailures.length > 0 && !options.quiet) {
      process.stderr.write(
        `warning: ${stopFailures.length} child process(es) did not stop cleanly: ${JSON.stringify(stopFailures)}\n`,
      );
    }
    if (root && !options.keep) {
      try {
        rmSync(root, { recursive: true, force: true });
      } catch {
        // Failing to remove the fixture must never mask a measurement or a typed gap.
      }
    }
  }
}

/** Closes an arm's sockets. Safe on a partially started arm, and safe to call twice. */
function closeArmConnections(arm) {
  for (const channel of [arm.tap, arm.control, arm.verbs, arm.keeperVerbs]) {
    try {
      channel?.close();
    } catch {
      // An already-closed socket needs no reporting; the child's exit status is authoritative.
    }
  }
}

/**
 * Builds one arm by name.
 *
 * A table rather than an if/else chain, so an arm cannot be added to `BOUNDARY` (and therefore to
 * `--arms`) without a constructor behind it.
 */
function makeArm(name, paths, env, binaries, options) {
  switch (name) {
    case 'direct-daemon':
      return new DirectDaemonArm(paths, env, binaries, options);
    case 'keeper-daemon':
      return new KeeperDaemonArm(paths, env, binaries, options);
    case 'keeper-uds':
      return new KeeperArm(paths, env, binaries, options);
    default:
      throw new HarnessError('USAGE', `unknown arm ${JSON.stringify(name)}`);
  }
}

/**
 * The children one arm started.
 *
 * Every one of them is adopted, so no process this harness created can outlive the run -- including
 * the keeper in the keeper-daemon arm, which exists only to be routed to.
 */
function armChildren(arm) {
  return [arm.daemon, arm.keeper].filter(Boolean);
}

async function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 2;
  }
  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return 0;
  }
  const log = options.quiet ? () => {} : (text) => process.stderr.write(`${text}\n`);

  if (process.platform !== 'darwin' && process.platform !== 'linux') {
    process.stderr.write(
      `${GAP.UNSUPPORTED_PLATFORM}: this harness uses unix sockets and a POSIX tty; ${process.platform} is unsupported\n`,
    );
    return 1;
  }

  try {
    const report = await runComparison(options, log);
    if (options.json) {
      process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
    } else {
      process.stdout.write(renderText(report));
      if (report.cleanup.keptRoot) process.stdout.write(`fixture kept at ${report.cleanup.keptRoot}\n`);
    }
    return 0;
  } catch (error) {
    // A typed gap is a finding, not a crash: it names the seam that is missing and exits non-zero.
    const typed = error instanceof HarnessError;
    const code = typed ? error.code : 'UNEXPECTED';
    if (options.json) {
      process.stdout.write(
        `${JSON.stringify(
          {
            schema: REPORT_SCHEMA,
            failed: true,
            code,
            message: error.message,
            detail: typed ? error.detail : null,
          },
          null,
          2,
        )}\n`,
      );
    } else {
      process.stderr.write(`${code}: ${error.message}\n`);
      if (!typed) process.stderr.write(`${String(error.stack ?? '')}\n`);
    }
    return 1;
  }
}

export {
  BOUNDARY,
  GAP,
  HarnessError,
  assertIsolation,
  blockPlan,
  buildReport,
  comparisonOf,
  echoChildArgv,
  echoChildScript,
  makePayload,
  parseArgs,
  percentile,
  planPaths,
  readActivation,
  renderText,
  summarize,
  verifyKeeperOwnershipEvidence,
};

// Run only when executed directly, so importing this file for its helpers has no side effects.
if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().then((code) => {
    process.exitCode = code;
  });
}
