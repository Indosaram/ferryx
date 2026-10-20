/** Task9 transport-only fixtures. Importing this module performs no I/O.
 * The verifier provisions the candidate and isolated stores; this module never
 * launches, kills, pairs, installs, or changes process-global environment.
 */
import assert from 'node:assert/strict';
import { createConnection, createServer } from 'node:net';
import { EventEmitter } from 'node:events';
import { createHash } from 'node:crypto';
import { realpath } from 'node:fs/promises';
import { relative, isAbsolute } from 'node:path';
import { connectDaemonControl, daemonControlTransport, setupIsolatedProfile } from '../herdr-reference-fixtures.mjs';

import { startTransportObserver } from './transport-observer.mjs';
export {
  launchPairedIsolatedHarness,
  provisionIsolatedPairedSession,
} from './paired-isolated-provider.mjs';

export const TRANSPORT_PROVIDER_SCHEMA = 'ferryx-task9-transport-provider/1';

export function encodeRemoteOwnerMutationData(data) {
  return Buffer.from(data).toString('base64');
}

export function remoteOwnerMutationRequest(sessionId, operation, generation, data = Buffer.alloc(0)) {
  return { type: 'qaRemoteOwnerMutation', sessionId, operation, generation,
    data: encodeRemoteOwnerMutationData(data) };
}

export class TransportBarrier extends Error {
  constructor(code, required) {
    super(`${code}: ${required}`);
    this.code = code;
    this.required = required;
  }
}

/** Register synchronously BEFORE the action. A deadline is not a polling clock. */
export function subscribe(emitter, event, predicate, timeoutMs, signal) {
  let timer;
  let settled = false;
  let rejectWait;
  const cleanup = () => {
    clearTimeout(timer);
    emitter.off(event, onEvent);
    signal?.removeEventListener('abort', onAbort);
  };
  const onAbort = () => {
    if (settled) return;
    settled = true;
    cleanup();
    rejectWait(new TransportBarrier('TRANSPORT_CANCELLED', event));
  };
  const onEvent = (value) => {
    if (settled || !predicate(value)) return;
    settled = true;
    cleanup();
    resolveWait(value);
  };
  let resolveWait;
  const promise = new Promise((resolve, reject) => {
    resolveWait = resolve;
    rejectWait = reject;
    emitter.on(event, onEvent);
    signal?.addEventListener('abort', onAbort, { once: true });
    timer = setTimeout(() => {
      settled = true;
      cleanup();
      reject(new TransportBarrier('TRANSPORT_DEADLINE', event));
    }, timeoutMs);
    if (signal?.aborted) onAbort();
  });
  // A trigger can fail before the caller awaits its subscription.
  promise.catch(() => {});
  return { promise, cancel: onAbort };
}

function loopback(endpoint) {
  if (!endpoint || !['127.0.0.1', '::1'].includes(endpoint.host) ||
      !Number.isInteger(endpoint.port) || endpoint.port < 1 || endpoint.port > 65535) {
    throw new TransportBarrier('ISOLATED_ENDPOINT_REQUIRED', '{host: loopback, port: 1..65535}');
  }
  return endpoint;
}

/** Real byte forwarding, usable for loopback SSH/helper or plaintext machine WS.
 * Refuse the next N actual candidate dials, then open automatically. No Retry
 * invocation and no fabricated attached response. TLS must terminate upstream.
 */
export class TransportFaultSocket extends EventEmitter {
  constructor(upstream) {
    super();
    this.upstream = loopback(upstream);
    this.server = null;
    this.sockets = new Set();
    this.trace = [];
    this.remaining = 0;
    this.released = false;
    this.serial = 0;
  }

  note(kind, detail = {}) {
    const receipt = { sequence: ++this.serial, kind, ...detail };
    this.trace.push(receipt);
    this.emit('receipt', receipt);
    return receipt;
  }

  async listen() {
    this.server = createServer((downstream) => {
      this.sockets.add(downstream);
      downstream.on('error', (error) => this.note('socket-error', { code: error.code }));
      downstream.once('close', () => this.sockets.delete(downstream));
      const dial = this.note('dial');
      if (this.remaining > 0) {
        this.remaining--;
        this.note('dial-refused', { dial: dial.sequence, remaining: this.remaining });
        downstream.destroy();
        return;
      }
      if (!this.released) {
        this.note('dial-held', { dial: dial.sequence });
        downstream.destroy();
        return;
      }
      const upstream = createConnection(this.upstream);
      this.sockets.add(upstream);
      upstream.on('error', (error) => {
        this.note('upstream-error', { dial: dial.sequence, code: error.code });
        downstream.destroy();
      });
      upstream.once('connect', () => this.note('dial-forwarded', { dial: dial.sequence }));
      upstream.once('close', () => { this.sockets.delete(upstream); downstream.destroy(); });
      downstream.once('close', () => upstream.destroy());
      // Hashes describe bytes actually traversing the fixture, not replay labels.
      downstream.on('data', (bytes) => this.note('upstream-bytes', {
        dial: dial.sequence, length: bytes.length,
        sha256: createHash('sha256').update(bytes).digest('hex'),
      }));
      upstream.on('data', (bytes) => this.note('downstream-bytes', {
        dial: dial.sequence, length: bytes.length,
        sha256: createHash('sha256').update(bytes).digest('hex'),
      }));
      downstream.pipe(upstream);
      upstream.pipe(downstream);
    });
    const ready = subscribe(this.server, 'listening', () => true, 10000);
    const error = (failure) => { this.emit('listener-error', failure); ready.cancel(); };
    this.server.once('error', error);
    this.server.listen(0, '127.0.0.1');
    try { await ready.promise; } finally { this.server.off('error', error); }
    return { host: '127.0.0.1', port: this.server.address().port };
  }

  interrupt(failures = 21) {
    if (!Number.isInteger(failures) || failures < 21) {
      throw new TransportBarrier('FAILURE_COUNT_REQUIRED', 'integer >= 21');
    }
    this.remaining = failures;
    this.released = false;
    this.note('outage-armed', { failures });
    for (const socket of this.sockets) socket.destroy();
  }

  release() {
    if (this.remaining !== 0) {
      throw new TransportBarrier('FAULT_ROUTE_RELEASE_EARLY', 'all armed upstream dial refusals observed');
    }
    this.released = true;
    this.note('outage-released');
  }

  async close() {
    for (const socket of this.sockets) socket.destroy();
    if (!this.server) return;
    await new Promise((resolve, reject) => this.server.close((error) => error ? reject(error) : resolve()));
    this.server = null;
  }
}

/** Concrete protocol-1 helper client, same framing as ssh-helper-survival.mjs.
 * One request at a time; a post-send disconnect rejects without resubmission.
 * Socket and token are supplied from verifier-owned temporary helper store.
 */
export async function connectTransportHelper(endpoint, token, timeoutMs, signal, qaFault) {
  const socket = createConnection(loopback(endpoint));
  if (qaFault) {
    const connected = subscribe(socket, 'connect', () => true, timeoutMs, signal);
    await connected.promise;
    return createAckLossHelperClient({ socket, token, timeoutMs, signal, ...qaFault });
  }
  const events = new EventEmitter();
  let bytes = Buffer.alloc(0);
  let pending = false;
  socket.on('error', (error) => events.emit('failure', error));
  socket.on('close', () => events.emit('failure', new Error('helper socket closed')));
  socket.on('data', (chunk) => {
    bytes = Buffer.concat([bytes, chunk]);
    while (bytes.length >= 4) {
      const length = bytes.readUInt32BE();
      if (length > 1024 * 1024) { socket.destroy(new Error('helper frame exceeds 1 MiB')); return; }
      if (bytes.length < length + 4) break;
      const frame = bytes.subarray(4, length + 4);
      bytes = bytes.subarray(length + 4);
      try { events.emit('reply', JSON.parse(frame.toString('utf8'))); }
      catch (error) { socket.destroy(error); return; }
    }
  });
  const connected = subscribe(socket, 'connect', () => true, timeoutMs, signal);
  await connected.promise;
  return {
    async call(op, params = {}) {
      if (pending) throw new TransportBarrier('HELPER_SINGLE_FLIGHT', op);
      pending = true;
      const reply = subscribe(events, 'reply', () => true, timeoutMs, signal);
      const failure = (error) => { reply.cancel(); socket.destroy(error); };
      events.once('failure', failure);
      const body = Buffer.from(JSON.stringify({ protocol: 1, token, op, params }));
      const header = Buffer.alloc(4);
      header.writeUInt32BE(body.length);
      try {
        socket.write(Buffer.concat([header, body]));
        return await reply.promise;
      } finally {
        events.off('failure', failure);
        reply.cancel();
        pending = false;
      }
    },
    close() { socket.destroy(); },
  };
}

/** Explicit isolated QA client over the real protocol-1 framed socket boundary.
 * The wire has no IDs: local request IDs correlate FIFO single-flight replies.
 * Closing on uncertainty prevents late replies being attributed to a new call.
 */
export function createAckLossHelperClient({ socket, token, timeoutMs, signal,
  ownerId, creationBaseline, suppress, clock = globalThis }) {
  if (!ownerId || !Number.isSafeInteger(creationBaseline) || creationBaseline < 0 ||
      !Number.isFinite(timeoutMs) || timeoutMs <= 0 || typeof suppress !== 'function') {
    throw new TransportBarrier('ACK_LOSS_CONFIG_REQUIRED', 'ownerId, creationBaseline, bounded timeout, explicit predicate');
  }
  let serial = 0;
  let requestSerial = 0;
  let active;
  let closed = false;
  let buffer = Buffer.alloc(0);
  const audit = [];
  const note = (kind, details) => {
    const receipt = { sequence: ++serial, ownerId, kind, ...details };
    audit.push(structuredClone(receipt));
    return receipt;
  };
  const mutations = new Set(['pty.write', 'pty.resize', 'pty.stop', 'pty.spawn', 'project.register']);
  function finish(error, result, uncertain = false) {
    const call = active;
    if (!call) return;
    active = undefined;
    clock.clearTimeout(call.timer);
    signal?.removeEventListener('abort', call.abort);
    if (error) {
      note(uncertain ? 'uncertain' : 'rejected', { requestId: call.request.requestId, code: error.code });
      closed = true;
      socket.destroy();
      call.reject(error);
    } else call.resolve(result);
  }
  const failure = () => finish(new TransportBarrier('HELPER_REPLY_UNCERTAIN', ownerId), undefined, true);
  socket.on('error', failure);
  socket.on('close', () => { closed = true; failure(); });
  socket.on('data', (bytes) => {
    if (closed) return;
    buffer = Buffer.concat([buffer, bytes]);
    while (buffer.length >= 4) {
      const length = buffer.readUInt32BE();
      if (length > 1024 * 1024) { failure(); return; }
      if (buffer.length < length + 4) return;
      const raw = Buffer.from(buffer.subarray(0, length + 4));
      buffer = buffer.subarray(length + 4);
      if (!active) { closed = true; socket.destroy(); return; }
      let reply;
      try { reply = JSON.parse(raw.subarray(4).toString('utf8')); }
      catch { failure(); return; }
      const request = active.request;
      const receipt = note('decoded-reply', { requestId: request.requestId, op: request.op,
        target: request.params.target, request: structuredClone(request),
        rawReplyBase64: raw.toString('base64'),
        rawReplySha256: createHash('sha256').update(raw).digest('hex'), reply });
      // accepted proves helper write/flush only. Application stdout is separate.
      let suppressed = false;
      try {
        suppressed = request.op === 'pty.write' && reply.ok === true && reply.data?.accepted === true &&
          suppress(structuredClone(receipt)) === true;
      } catch { failure(); return; }
      if (suppressed) {
        const { sequence, kind, ...provenance } = receipt;
        note('reply-suppressed', { ...provenance, decodedSequence: sequence });
        finish(new TransportBarrier('HELPER_REPLY_UNCERTAIN', request.requestId), undefined, true);
      } else finish(undefined, reply);
    }
  });
  return {
    snapshot() { return { ownerId, creationBaseline, scope: 'this-helper-client-only',
      audit: structuredClone(audit), closed }; },
    call(op, params = {}) {
      if (closed) return Promise.reject(new TransportBarrier('HELPER_CLIENT_CLOSED', ownerId));
      if (active) return Promise.reject(new TransportBarrier('HELPER_SINGLE_FLIGHT', op));
      if (signal?.aborted) return Promise.reject(new TransportBarrier('TRANSPORT_CANCELLED', op));
      const request = { requestId: `${ownerId}:${++requestSerial}`, op, params: structuredClone(params),
        mutation: mutations.has(op), creation: op === 'pty.spawn' };
      const body = Buffer.from(JSON.stringify({ protocol: 1, token, op, params: request.params }));
      const header = Buffer.alloc(4);
      header.writeUInt32BE(body.length);
      return new Promise((resolve, reject) => {
        const abort = () => finish(new TransportBarrier('TRANSPORT_CANCELLED', request.requestId), undefined, true);
        active = { request, resolve, reject, abort };
        active.timer = clock.setTimeout(() => finish(new TransportBarrier('TRANSPORT_DEADLINE',
          request.requestId), undefined, true), timeoutMs);
        signal?.addEventListener('abort', abort, { once: true });
        note('request-written', request);
        try { socket.write(Buffer.concat([header, body])); }
        catch { failure(); }
      });
    },
    close() { closed = true; failure(); socket.destroy(); },
  };
}

/** Explicit isolated QA daemon control client over the real NDJSON socket boundary.
 * Frames are \n delimited JSON. Used to verify dropped-ACK mutations on the daemon wire.
 * Mutating commands under dropped ACK must fail with uncertainty and zero automatic replay.
 */
export function createAckLossDaemonControlClient({ socket, token, timeoutMs, signal,
  ownerId, creationBaseline, suppress, clock = globalThis }) {
  if (!ownerId || !Number.isSafeInteger(creationBaseline) || creationBaseline < 0 ||
      typeof suppress !== 'function') {
    throw new TransportBarrier('DAEMON_CONTROL_CLIENT_OPTIONS_REQUIRED', '{ownerId,creationBaseline>=0,suppress:Function}');
  }
  let closed = false;
  let active = null;
  const audit = [];
  let requestSerial = 0;
  let responseSerial = 0;
  let lineBuffer = '';
  const note = (kind, detail = {}) => {
    const entry = { sequence: ++responseSerial, kind, at: new Date().toISOString(), ...detail };
    audit.push(entry);
    return entry;
  };
  const finish = (error, reply, shouldClose = false) => {
    if (!active) return;
    const current = active;
    active = null;
    clock.clearTimeout(current.timer);
    signal?.removeEventListener('abort', current.abort);
    if (shouldClose) { closed = true; socket.destroy(); }
    if (error) current.reject(error);
    else current.resolve(reply);
  };
  const failure = () => finish(new TransportBarrier('DAEMON_CONTROL_SOCKET_CLOSED', ownerId), undefined, true);
  socket.on('error', failure);
  socket.once('close', failure);
  socket.on('data', (chunk) => {
    lineBuffer += chunk.toString('utf8');
    let idx;
    while ((idx = lineBuffer.indexOf('\n')) !== -1) {
      const line = lineBuffer.slice(0, idx).replace(/\r$/, '');
      lineBuffer = lineBuffer.slice(idx + 1);
      if (!line.trim()) continue;
      if (!active) { closed = true; socket.destroy(); return; }
      let reply;
      try { reply = JSON.parse(line); }
      catch { failure(); return; }
      const request = active.request;
      const receipt = note('decoded-reply', { requestId: request.requestId, op: request.payload?.type,
        request: structuredClone(request), reply });
      let suppressed = false;
      try {
        suppressed = suppress(structuredClone(receipt)) === true;
      } catch { failure(); return; }
      if (suppressed) {
        const { sequence, kind, ...provenance } = receipt;
        note('reply-suppressed', { ...provenance, decodedSequence: sequence });
        finish(new TransportBarrier('DAEMON_CONTROL_REPLY_UNCERTAIN', request.requestId), undefined, true);
      } else finish(undefined, reply);
    }
  });
  return {
    snapshot() { return { ownerId, creationBaseline, scope: 'this-daemon-control-client-only',
      audit: structuredClone(audit), closed }; },
    call(payload) {
      if (closed) return Promise.reject(new TransportBarrier('DAEMON_CONTROL_CLIENT_CLOSED', ownerId));
      if (active) return Promise.reject(new TransportBarrier('DAEMON_CONTROL_SINGLE_FLIGHT', payload?.type));
      if (signal?.aborted) return Promise.reject(new TransportBarrier('TRANSPORT_CANCELLED', payload?.type));
      const request = { requestId: `${ownerId}:${++requestSerial}`, payload: structuredClone(payload) };
      const body = JSON.stringify(token ? { token, ...payload } : payload) + '\n';
      return new Promise((resolve, reject) => {
        const abort = () => finish(new TransportBarrier('TRANSPORT_CANCELLED', request.requestId), undefined, true);
        active = { request, resolve, reject, abort };
        active.timer = clock.setTimeout(() => finish(new TransportBarrier('TRANSPORT_DEADLINE',
          request.requestId), undefined, true), timeoutMs);
        signal?.addEventListener('abort', abort, { once: true });
        note('request-written', request);
        try { socket.write(body); }
        catch { failure(); }
      });
    },
    close() { closed = true; failure(); socket.destroy(); },
  };
}

export function pairedOwnerControls(control) {
  const read = async (backendSessionId) => {
    const response = await control.call({ type: 'pairedRecoveryOutcome', backend_session_id: backendSessionId });
    if (response.type !== 'pairedRecoveryOutcomeResult' || !response.outcome) {
      throw new TransportBarrier('PAIRED_OWNER_UNAVAILABLE', backendSessionId);
    }
    return response.outcome;
  };
  return {
    async bind(fixture) {
      const identity = await read(fixture.backendSessionId);
      assert.equal(identity.daemonEpoch, fixture.daemonEpoch);
      assert.equal(identity.incarnation, fixture.incarnation);
      return { identity };
    },
    async retry(session) {
      const { backendSessionId, daemonEpoch, incarnation } = session.identity;
      const response = await control.call({ type: 'retryPairedSession', backendSessionId, daemonEpoch, incarnation });
      if (response.type !== 'retryPairedSessionResult') throw new TransportBarrier('PAIRED_RETRY_UNAVAILABLE', JSON.stringify(response));
      return response;
    },
    async dropLoopback(session) {
      const response = await control.call({ type: 'qaPairedDropLoopbackTransport', backendSessionId: session.identity.backendSessionId });
      if (response.type !== 'closeOk') throw new TransportBarrier('PAIRED_LOOPBACK_LOSS_UNAVAILABLE', JSON.stringify(response));
      return response;
    },
    async routeReceipt(session) {
      const response = await control.call({ type: 'qaPairedRouteReceipt', backendSessionId: session.identity.backendSessionId });
      if (response.type !== 'qaPairedRouteReceiptOk') throw new TransportBarrier('PAIRED_ROUTE_UNAVAILABLE', JSON.stringify(response));
      return response.receipt;
    },
    async dialAudit(session) {
      const response = await control.call({ type: 'qaPairedDialAudit', backendSessionId: session.identity.backendSessionId });
      if (response.type !== 'qaPairedDialAuditOk') throw new TransportBarrier('PAIRED_DIAL_AUDIT_UNAVAILABLE', JSON.stringify(response));
      return response.audit;
    },
    async ownerAudit(session) {
      const response = await control.call({ type: 'qaPairedOwnerAudit', backendSessionId: session.identity.backendSessionId });
      if (response.type !== 'qaPairedOwnerAuditOk') throw new TransportBarrier('PAIRED_OWNER_AUDIT_UNAVAILABLE', JSON.stringify(response));
      return response;
    },
    async restoreDescriptorCold(fixture) {
      const response = await control.call({ type: 'qaPairedColdDescriptor', backendSessionId: fixture.backendSessionId });
      if (response.type !== 'pairedTerminalDescriptorOk' || !response.descriptor) {
        throw new TransportBarrier('PAIRED_COLD_DESCRIPTOR_UNAVAILABLE', JSON.stringify(response));
      }
      return response.descriptor;
    },
    async cancelPendingDial(session) {
      const response = await control.call({ type: 'qaPairedCancelPendingDial', backendSessionId: session.identity.backendSessionId });
      if (response.type !== 'closeOk') throw new TransportBarrier('PAIRED_DIAL_CANCEL_UNAVAILABLE', JSON.stringify(response));
      return response;
    },
    async waitState(session, states) {
      assert.equal(states.length, 1);
      const response = await control.call({ type: 'qaPairedOwnerWaitState', backendSessionId: session.identity.backendSessionId, state: states[0] });
      if (response.type !== 'qaPairedOwnerWaitStateOk') throw new TransportBarrier('PAIRED_STATE_UNAVAILABLE', JSON.stringify(response));
      return response;
    },
    async mutate(session, operation, data = Buffer.alloc(0)) {
      const response = await control.call(remoteOwnerMutationRequest(session.identity.backendSessionId, operation, session.identity.generation ?? 1, data));
      if (response.type !== 'qaRemoteOwnerMutationOk') {
        throw new TransportBarrier('PAIRED_MUTATION_UNAVAILABLE', JSON.stringify(response));
      }
      return response;
    },
  };
}

/** Setup path for paired fixture.
 * Must ONLY provision through actual existing spawn/paired-registration APIs with measured
 * returned identity and recorded owned PIDs; never invents synthetic defaults.
 */
export async function provisionPairedFixture(ctx, options = {}) {
  const binary = ctx.binary ?? options.binary;
  if (!binary || typeof binary !== 'string') {
    throw new TransportBarrier('CANDIDATE_BINARY_REQUIRED', 'candidate binary path for isolated paired daemon');
  }
  const provisionSession = ctx.provisionPairedSession ?? options.provisionPairedSession;
  if (typeof provisionSession !== 'function') {
    throw new TransportBarrier(
      'PAIRED_PROVISIONING_SEAM_REQUIRED',
      'ctx.provisionPairedSession({binary, clientProfile, hostProfile, ledger}): Promise<{paired, pid}>',
    );
  }
  const result = await provisionSession({ binary, ...options });
  if (!result?.paired?.backendSessionId || !result?.paired?.daemonEpoch || !result?.paired?.incarnation ||
      !result?.paired?.descriptor || !result?.paired?.target || !Number.isInteger(result?.pid)) {
    throw new TransportBarrier(
      'PAIRED_PROVISION_IDENTITY_INVALID',
      '{paired: {backendSessionId, daemonEpoch, incarnation, descriptor, target}, pid: Integer}',
    );
  }
  if (options.ledger && typeof options.ledger.record === 'function') {
    options.ledger.record({ pid: result.pid, binary });
  }
  if (result.harness?.restartClientDaemon && !ctx.restartIsolatedDaemon) {
    ctx.restartIsolatedDaemon = async () => result.harness.restartClientDaemon();
  }
  ctx.transportFixture = {
    schema: TRANSPORT_PROVIDER_SCHEMA,
    root: options.clientProfile?.root ?? result.paired.root ?? result.harness?.fixture?.root,
    profile: options.clientProfile ?? result.paired.profile ?? result.harness?.fixture?.profile,
    candidate: ctx.candidateStamp ?? result.candidateStamp ?? 'candidate',
    binary,
    selection: { backendSessionId: result.paired.backendSessionId },
    paired: result.paired,
  };
  return ctx.transportFixture;
}

/** Connect only to the verifier-owned candidate and optional real helper. */
export async function createTransportProvider(ctx) {
  const f = ctx.transportFixture;
  if (!f || f.schema !== TRANSPORT_PROVIDER_SCHEMA) throw new TransportBarrier('TRANSPORT_FIXTURE_REQUIRED', TRANSPORT_PROVIDER_SCHEMA);
  const root = await realpath(f.root);
  for (const path of [f.profile.root, f.profile.paths.runtime]) {
    const canonical = await realpath(path);
    const rel = relative(root, canonical);
    if (rel.startsWith('..') || isAbsolute(rel)) throw new TransportBarrier('FIXTURE_PATH_NOT_OWNED', path);
  }
  if (!ctx.binary || f.candidate !== ctx.candidateStamp || f.binary !== ctx.binary) {
    throw new TransportBarrier('CANDIDATE_PROVENANCE_REQUIRED', 'fixture candidate/binary equal context');
  }
  if (!Number.isFinite(ctx.timeoutMs) || ctx.timeoutMs <= 0) throw new TransportBarrier('BOUNDED_DEADLINE_REQUIRED', 'ctx.timeoutMs > 0');
  const control = await connectDaemonControl(daemonControlTransport(f.profile), { timeoutMs: ctx.timeoutMs });
  let helper;
  let observer;
  try {
    if (f.helper) helper = await connectTransportHelper(f.helper.endpoint, f.helper.token, ctx.timeoutMs, ctx.signal);
    observer = startTransportObserver({ control, helper, selection: f.selection, timeoutMs: ctx.timeoutMs, signal: ctx.signal });
    return { fixture: f, observer, control, helper, sockets: {}, faultSockets: [],
      async restartClientDaemon() {
        if (typeof ctx.restartIsolatedDaemon === 'function') {
          return await ctx.restartIsolatedDaemon(ctx);
        }
        throw new TransportBarrier('CLIENT_DAEMON_COLD_RESTART_SEAM_REQUIRED', 'ctx.restartIsolatedDaemon(ctx): Promise<TransportProvider>');
      },
      pairedOwner: pairedOwnerControls(control),
      async createFaultSocket(upstream) {
        const socket = new TransportFaultSocket(upstream);
        const endpoint = await socket.listen();
        this.faultSockets.push(socket);
        return { endpoint, socket, snapshot: () => structuredClone(socket.trace), close: () => socket.close() };
      },
      async createRemoteSession(config, params, clientRequestId) {
        if (!config || typeof config !== 'object' || !params || typeof params !== 'object' ||
            typeof clientRequestId !== 'string' || clientRequestId.length === 0) {
          throw new TransportBarrier('SSH_SESSION_CONFIG_REQUIRED',
            'fixture.sshSession {config: RemoteSessionConfig, params: SpawnParams, clientRequestId}');
        }
        const result = await control.call({ type: 'qaCreateRemoteSession', config, params, clientRequestId });
        if (result.type !== 'qaCreateRemoteSessionOk') {
          throw new TransportBarrier('SSH_SESSION_CREATE_FAILED', JSON.stringify(result));
        }
        return result;
      },
      async remoteOwnerMutation(sessionId, operation, generation, data = Buffer.alloc(0)) {
        return control.call(remoteOwnerMutationRequest(sessionId, operation, generation, data));
      },
      async remoteOwnerAudit(sessionId) {
        return control.call({ type: 'qaRemoteOwnerAudit', sessionId });
      },
      async remoteOwnerWaitState(sessionId, state) {
        return control.call({ type: 'qaRemoteOwnerWaitState', sessionId, state });
      },
      async remoteOwnerWaitRefusals(sessionId, count) {
        return control.call({ type: 'qaRemoteOwnerWaitRefusals', sessionId, count });
      },
      async remoteOwnerReleaseFaultRoute(sessionId) {
        return control.call({ type: 'qaRemoteOwnerReleaseFaultRoute', sessionId });
      },
      async close() { observer.close(); helper?.close();
        await Promise.all(this.faultSockets.map((socket) => socket.close())); control.close(); } };
  } catch (error) { observer?.close(); helper?.close(); control.close(); throw error; }
}
