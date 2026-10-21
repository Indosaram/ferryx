import { createServer, createConnection } from 'node:net';
import { EventEmitter } from 'node:events';
import { join } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { StringDecoder } from 'node:string_decoder';

// Unsolicited daemon stream messages that do not correspond to a solicited request
const UNSOLICITED_DAEMON_TYPES = new Set([
  'output',
  'status',
  'streamEnded',
  'sessionExited',
  'delta',
  'gap',
  'replayGap',
]);

/**
 * Native Forwarding Transport Provider
 * Implements real physical byte forwarding via NDJSON loopback TCP/UDS proxy,
 * exact-handle stream lifecycle controls, strict session & payload wire matching
 * for physical ACK suppression, and late-binding context attachment.
 *
 * Emits ONLY proxy transport socket events; never synthesizes product producer receipts.
 */
export class NativeForwardingTransport extends EventEmitter {
  constructor({ config = {}, ctx = {} } = {}) {
    super();
    this.config = config;
    this.ctx = ctx;
    this.agent = ctx.agent ?? null;
    this.setup = ctx.setup ?? null;
    this.target = ctx.target ?? null;
    this.mutation = ctx.mutation ?? null;
    this.runId = ctx.runId ?? null;
    this.mahord = ctx.mahord ?? null;

    this.subscribers = new Set();
    this.seq = 0;
    this.server = null;
    this.activeSockets = new Set();
    this.upstreamSockets = new Set();
    this.proxyEndpoint = null;

    this.activeAudit = null;
    this.lostAckArmed = false;
    this.armedLoss = null;
    this.refuseConnections = false;
    this.lastOutageStart = null;
    this.released = false;
  }

  publish(event) {
    if (this.released) return;
    const normalized = { ...event, seq: ++this.seq };
    for (const sub of this.subscribers) {
      try { sub(normalized); } catch (_) {}
    }
    this.emit('receipt', normalized);
    return normalized;
  }

  subscribe(callback) {
    this.subscribers.add(callback);
    return () => { this.subscribers.delete(callback); };
  }

  // Late binding: attaches real agent, setup profile, runId, mutation from createConfiguredNativeContext
  bindContext({ agent, setup, runId, mutation, mahord }) {
    if (agent) this.agent = agent;
    if (setup) this.setup = setup;
    if (runId) this.runId = runId;
    if (mutation) this.mutation = mutation;
    if (mahord) this.mahord = mahord;
  }

  // Starts the real NDJSON proxy server between GUI (downstream) and daemon (upstream)
  // Portably supports explicit TCP port on all platforms, or explicit UDS path under profile/forwarder/daemon.sock
  async startProxy({ upstreamSocket, upstreamPort, signal } = {}) {
    if (this.server) return this.proxyEndpoint;
    const useTcp = typeof upstreamPort === 'number' && upstreamPort > 0;

    if (!useTcp && !this.setup?.profile) {
      throw new Error('SETUP_PROFILE_REQUIRED: Profile directory is required to allocate forwarding socket; fallback is forbidden');
    }

    this.server = createServer((downstream) => {
      this.activeSockets.add(downstream);
      const downstreamDecoder = new StringDecoder('utf8');
      let downstreamBuffer = '';
      // Per-connection single-flight queue tracking solicited request flights
      const inflightQueue = [];
      const connectionId = `conn-${++this.seq}`;

      if (this.refuseConnections) {
        downstream.destroy();
        this.publish({ kind: 'dial-refused', remoteAddress: downstream.remoteAddress });
        return;
      }

      // Connect upstream to real daemon
      const upstream = useTcp
        ? createConnection({ host: '127.0.0.1', port: upstreamPort })
        : createConnection({ path: upstreamSocket });

      this.upstreamSockets.add(upstream);
      this.publish({ kind: 'dial-forwarded', upstream: useTcp ? upstreamPort : upstreamSocket });

      downstream.on('error', () => { upstream.destroy(); });
      upstream.on('error', () => { upstream.destroy(); });
      downstream.once('close', () => {
        this.activeSockets.delete(downstream);
        upstream.destroy();
      });
      upstream.once('close', () => {
        this.upstreamSockets.delete(upstream);
        downstream.destroy();
      });

      // Downstream -> Upstream: parse newline-delimited JSON
      downstream.on('data', (chunk) => {
        downstreamBuffer += downstreamDecoder.write(chunk);
        let newlineIdx;
        while ((newlineIdx = downstreamBuffer.indexOf('\n')) !== -1) {
          const line = downstreamBuffer.slice(0, newlineIdx);
          downstreamBuffer = downstreamBuffer.slice(newlineIdx + 1);
          if (!line.trim()) continue;

          let msg;
          try { msg = JSON.parse(line); } catch (_) {
            upstream.write(line + '\n');
            continue;
          }

          // Exact matching for write mutation without wildcards
          const isWrite = msg.type === 'write';
          const matchesSession = this.armedLoss ? msg.sessionId === this.armedLoss.sessionId : false;
          const matchesData = this.armedLoss ? msg.data === this.armedLoss.wireData : false;
          const matched = Boolean(this.armedLoss && isWrite && matchesSession && matchesData);

          this.publish({
            kind: 'request',
            operation: isWrite && !matched ? 'write' : msg.type,
            sessionId: msg.sessionId,
            connectionId,
            mutationId: matched ? this.armedLoss.mutationId : null,
            ...(matched ? { payload: this.armedLoss.payload } : {}),
          });
          if (msg.type === 'attach' && msg.sessionId) {
            downstream.attachedSessionId = msg.sessionId;
            upstream.attachedSessionId = msg.sessionId;
            this.publish({
              kind: 'stream-open',
              target: msg.sessionId,
              sessionId: msg.sessionId,
              connectionId,
            });
          }

          let expectSuppressedAck = false;
          if (this.lostAckArmed && matched && !this.armedLoss.reserved) {
            expectSuppressedAck = true;
            // Reserve arm once at request observation to prevent double suppression
            this.armedLoss.reserved = true;
            this.lostAckArmed = false;
          }

          // Track solicited flight in per-connection single-flight queue
          inflightQueue.push({
            type: msg.type,
            sessionId: msg.sessionId,
            expectSuppressedAck,
            mutationId: matched ? this.armedLoss.mutationId : null,
            payload: matched ? this.armedLoss.payload : undefined,
          });

          upstream.write(line + '\n');
        }
      });

      // Upstream -> Downstream: parse newline-delimited JSON
      const upstreamDecoder = new StringDecoder('utf8');
      let upstreamBuffer = '';
      upstream.on('data', (chunk) => {
        upstreamBuffer += upstreamDecoder.write(chunk);
        let newlineIdx;
        while ((newlineIdx = upstreamBuffer.indexOf('\n')) !== -1) {
          const line = upstreamBuffer.slice(0, newlineIdx);
          upstreamBuffer = upstreamBuffer.slice(newlineIdx + 1);
          if (!line.trim()) continue;

          let reply;
          try { reply = JSON.parse(line); } catch (_) {
            downstream.write(line + '\n');
            continue;
          }

          // Unsolicited async stream messages must NOT consume inflightQueue
          if (UNSOLICITED_DAEMON_TYPES.has(reply.type)) {
            downstream.write(line + '\n');
            continue;
          }

          // Solicited response consumes the next in-flight request
          const inflight = inflightQueue.shift();

          const isSuccessAck = (inflight?.type === 'write' && reply.type === 'writeOk');
          if (isSuccessAck) {
            this.publish({
              kind: 'delivery',
              operation: inflight.type,
              sessionId: inflight.sessionId,
              mutationId: inflight.mutationId,
              ...(inflight.payload !== undefined ? { payload: inflight.payload } : {}),
            });
          }

          // Physical ACK dropping: ONLY drop writeOk if this flight was matched and reserved
          if (inflight?.expectSuppressedAck && reply.type === 'writeOk') {
            this.publish({
              kind: 'lost-ack-suppressed',
              mutationId: inflight.mutationId,
              sessionId: inflight.sessionId,
              replyType: reply.type,
            });
            // Suppress writeOk and sever downstream socket to create physical in-flight uncertainty
            downstream.destroy();
            return;
          }

          // Unrelated writeOk or other replies pass through untouched
          downstream.write(line + '\n');
        }
      });
    });

    if (useTcp) {
      await new Promise((res, rej) => {
        this.server.listen(0, '127.0.0.1', () => res());
        this.server.once('error', rej);
      });
      const addr = this.server.address();
      let portPath;
      if (this.setup?.profile) {
        const forwarderDir = join(this.setup.profile, 'forwarder');
        await mkdir(forwarderDir, { recursive: true });
        portPath = join(forwarderDir, 'daemon.port');
        await writeFile(portPath, String(addr.port), 'utf8');
      }
      this.proxyEndpoint = { host: '127.0.0.1', port: addr.port, path: portPath };
    } else {
      const proxyDir = join(this.setup.profile, 'forwarder');
      await mkdir(proxyDir, { recursive: true });
      const proxyPath = join(proxyDir, 'daemon.sock');
      await new Promise((res, rej) => {
        this.server.listen(proxyPath, () => res());
        this.server.once('error', rej);
      });
      this.proxyEndpoint = { path: proxyPath };
    }

    this.publish({ kind: 'proxy-started', endpoint: this.proxyEndpoint });
    return this.proxyEndpoint;
  }

  // 1. armLostAcknowledgment: requires explicit target session & converts payload to actual Base64 wire data
  async armLostAcknowledgment({ mutation, target, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    if (!mutation?.id) throw new Error('MUTATION_ID_REQUIRED: mutation.id is required to arm lost acknowledgment');

    const effectiveTargetSession = target?.backendSessionId ?? mutation.sessionId ?? this.target?.backendSessionId;
    if (!effectiveTargetSession) {
      throw new Error('TARGET_SESSION_REQUIRED: Explicit target session is required to arm lost acknowledgment; wildcards are forbidden');
    }

    if (mutation.operation !== 'write') {
      throw new Error(`UNSUPPORTED_MUTATION_OPERATION: Only "write" mutation can be armed for physical ACK suppression, got "${mutation.operation}"`);
    }

    if (mutation.payload === undefined || mutation.payload === null) {
      throw new Error('MUTATION_PAYLOAD_REQUIRED: Explicit mutation payload is required; wildcards are forbidden');
    }

    // Convert known adapter payload to actual wire data encoding (Base64)
    const wireData = typeof mutation.payload === 'string'
      ? Buffer.from(mutation.payload, 'utf8').toString('base64')
      : Buffer.isBuffer(mutation.payload)
        ? mutation.payload.toString('base64')
        : Buffer.from(String(mutation.payload), 'utf8').toString('base64');

    this.armedLoss = {
      mutationId: mutation.id,
      payload: mutation.payload,
      sessionId: effectiveTargetSession,
      wireData,
      reserved: false,
    };
    this.lostAckArmed = true;

    // If attached to a remote agent, delegate to the remote agent's proxy instance
    if (this.agent && this.setup?.profile) {
      await this.agent.call('arm-forwarder-lost-ack', {
        profile: this.setup.profile,
        mutation,
        target: { backendSessionId: effectiveTargetSession },
      }, signal);

      await this.agent.call('barrier-prearm', {
        profile: this.setup.profile,
        name: 'lost-ack',
        options: {
          plan: 'lost-ack',
          clientRequestId: mutation.id,
          deadlineMs: 60_000,
        },
      }, signal);
    }

    this.publish({ kind: 'lost-ack-armed', mutationId: mutation.id, sessionId: effectiveTargetSession });
    return { armed: true, mutationId: mutation.id, sessionId: effectiveTargetSession };
  }

  // 2. beginAudit: binds to actual physical producer interval
  async beginAudit({ setup, target, roster = [], signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    if (setup) this.setup = setup;
    if (target) this.target = target;

    if (!this.agent || !this.setup?.profile) {
      throw new Error('AUDIT_PHYSICAL_BASELINE_REQUIRED: Actual agent and profile setup are required for producer interval binding');
    }

    const baseline = await this.agent.call('audit-producer-roster', { profile: this.setup.profile }, signal);
    if (!Array.isArray(baseline) || baseline.length === 0) {
      throw new Error('AUDIT_PRODUCER_ROSTER_EMPTY: Physical producer roster from remote agent is empty');
    }

    const auditId = `audit-${this.setup.profile.split(/[\\/]/).pop()}-${Date.now()}`;
    this.activeAudit = {
      auditId,
      scope: 'registered-isolated-owner-set',
      registeredOwners: baseline,
      begunAt: Date.now(),
    };

    this.publish({
      kind: 'audit-begun',
      auditId,
      scope: 'registered-isolated-owner-set',
      registeredOwners: this.activeAudit.registeredOwners,
    });

    return {
      auditId,
      scope: 'registered-isolated-owner-set',
      registeredOwners: this.activeAudit.registeredOwners,
    };
  }

  // 3. endCurrentStream: destroys proxy sockets; emits proxy stream events only
  async endCurrentStream({ target, holdRecovery = false, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    const targetSessionId = target?.backendSessionId ?? this.target?.backendSessionId;

    if (this.agent && this.setup?.profile) {
      await this.agent.call('end-forwarder-stream', {
        profile: this.setup.profile,
        target: { backendSessionId: targetSessionId },
        holdRecovery,
      }, signal);
    }

    if (holdRecovery) {
      this.refuseConnections = true;
    }
    for (const socket of this.activeSockets) {
      if (!targetSessionId || !socket.attachedSessionId || socket.attachedSessionId === targetSessionId) socket.destroy();
    }
    for (const socket of this.upstreamSockets) {
      if (!targetSessionId || !socket.attachedSessionId || socket.attachedSessionId === targetSessionId) socket.destroy();
    }

    this.publish({ kind: 'stream-ended', target: targetSessionId });
    return { ended: true, target: targetSessionId, holdRecovery };
  }

  // 4. disconnect: closes proxy listener and severs active connections
  async disconnect({ target, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    const targetSessionId = target?.backendSessionId ?? this.target?.backendSessionId;

    if (this.agent && this.setup?.profile) {
      await this.agent.call('disconnect-forwarder', {
        profile: this.setup.profile,
        target: { backendSessionId: targetSessionId },
      }, signal);
    }

    this.refuseConnections = true;
    for (const socket of this.activeSockets) {
      if (!targetSessionId || !socket.attachedSessionId || socket.attachedSessionId === targetSessionId) socket.destroy();
    }
    for (const socket of this.upstreamSockets) {
      if (!targetSessionId || !socket.attachedSessionId || socket.attachedSessionId === targetSessionId) socket.destroy();
    }

    const monotonicMs = Date.now();
    this.lastOutageStart = { monotonicMs, target: targetSessionId };

    this.publish({
      kind: 'outage-start',
      monotonicMs,
      target: targetSessionId,
    });

    return { disconnected: true, monotonicMs, target: targetSessionId };
  }

  // 5. holdOutage: maintains refusal window
  async holdOutage({ target, minimumMs = 0, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    const targetSessionId = target?.backendSessionId ?? this.target?.backendSessionId;
    const start = this.lastOutageStart ?? { monotonicMs: Date.now(), target: targetSessionId };

    const delegated = Boolean(this.agent && this.setup?.profile);
    if (this.agent && this.setup?.profile) {
      await this.agent.call('hold-forwarder-outage', {
        profile: this.setup.profile,
        target: { backendSessionId: targetSessionId },
        minimumMs,
      }, signal);
    }

    if (minimumMs > 0 && !delegated) {
      await new Promise((resolve, reject) => {
        const timer = setTimeout(resolve, minimumMs);
        if (signal) {
          const onAbort = () => {
            clearTimeout(timer);
            reject(signal.reason);
          };
          signal.addEventListener('abort', onAbort, { once: true });
        }
      });
    }

    const elapsedMs = Date.now();
    this.publish({
      kind: 'outage-elapsed',
      startedAt: start.monotonicMs,
      monotonicMs: elapsedMs,
      target: targetSessionId,
    });

    return { held: true, elapsedMs, target: targetSessionId };
  }

  // 6. restore: re-enables proxy forwarding
  async restore({ target, signal } = {}) {
    if (signal?.aborted) throw signal.reason;

    if (this.agent && this.setup?.profile) {
      await this.agent.call('restore-forwarder', {
        profile: this.setup.profile,
        target,
      }, signal);
    }

    this.refuseConnections = false;
    this.publish({ kind: 'proxy-restored', target: target?.backendSessionId });
    return { restored: true };
  }

  // 7. changeOwnerEpoch: throws typed missing product seam error until implemented in Rust daemon
  async changeOwnerEpoch({ target, holdRecovery = false, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    throw new Error('UNAVAILABLE_SEAM_AUTHORITY_EPOCH: Isolated owner-routing replacement seam is unavailable in Rust product daemon (requires QaAdvanceAuthorityEpoch in src-tauri/src/daemon/server.rs)');
  }

  // 8. emitOutput: throws typed missing product seam error until implemented in Rust daemon
  async emitOutput({ target, marker, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    throw new Error('UNAVAILABLE_SEAM_FIXTURE_OUTPUT: Fixture process output command seam is unavailable in Rust product daemon (requires QaCommandFixtureOutput in src-tauri/src/daemon/server.rs)');
  }

  // 9. prearmFinalDrain
  async prearmFinalDrain({ auditId, owners = [], signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    const effectiveAuditId = auditId ?? this.activeAudit?.auditId ?? `audit-${Date.now()}`;
    const drainArmId = `drain-${effectiveAuditId}-${Date.now()}`;
    const ownerIds = owners.map((o) => o.ownerId ?? `${o.component}-${o.pid}`);

    if (this.agent && this.setup?.profile) {
      await this.agent.call('barrier-prearm', {
        profile: this.setup.profile,
        name: 'final-drain',
        options: { plan: 'final-drain', deadlineMs: 30_000 },
      }, signal);
    }

    this.publish({
      kind: 'drain-prearmed',
      auditId: effectiveAuditId,
      drainArmId,
      ownerIds,
    });

    return { armed: true, auditId: effectiveAuditId, drainArmId, ownerIds };
  }

  // 10. flush
  async flush({ auditId, drainArmId, signal } = {}) {
    if (signal?.aborted) throw signal.reason;
    const effectiveAuditId = auditId ?? this.activeAudit?.auditId ?? `audit-${Date.now()}`;

    if (this.agent && this.setup?.profile) {
      await this.agent.call('drain', { profile: this.setup.profile }, signal);
    }

    this.publish({
      kind: 'flushed',
      auditId: effectiveAuditId,
      drainArmId,
      throughSeq: this.seq,
    });

    return {
      complete: true,
      auditId: effectiveAuditId,
      drainArmId,
      throughSeq: this.seq,
      scope: 'registered-isolated-owner-set',
    };
  }

  // 11. release
  async release({ signal } = {}) {
    this.released = true;
    if (this.agent && this.setup?.profile) {
      try {
        await this.agent.call('release-forwarder', { profile: this.setup.profile }, signal);
      } catch (_) {}
    }

    for (const socket of this.activeSockets) socket.destroy();
    for (const socket of this.upstreamSockets) socket.destroy();
    this.activeSockets.clear();
    this.upstreamSockets.clear();

    if (this.server) {
      await new Promise((resolve) => this.server.close(() => resolve()));
      this.server = null;
    }
    this.subscribers.clear();
    this.publish({ kind: 'released' });
    return { released: true };
  }
}

export async function createNativeTransport(config = {}, ctx = {}) {
  return new NativeForwardingTransport({ config, ctx });
}
