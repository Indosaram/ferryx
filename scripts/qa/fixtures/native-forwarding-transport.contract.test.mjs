import { describe, expect, it } from 'vitest';
import { createNativeTransport, NativeForwardingTransport } from './native-forwarding-transport.mjs';
import { createServer, createConnection } from 'node:net';
import { StringDecoder } from 'node:string_decoder';
import { tmpdir } from 'node:os';
import { mkdtemp, rm } from 'node:fs/promises';
import { join } from 'node:path';

// Helper to buffer lines using StringDecoder across arbitrary TCP fragments
function createLineReceiver(socket) {
  const decoder = new StringDecoder('utf8');
  let buffer = '';
  const listeners = [];
  const lines = [];

  socket.on('data', (chunk) => {
    buffer += decoder.write(chunk);
    let newlineIdx;
    while ((newlineIdx = buffer.indexOf('\n')) !== -1) {
      const line = buffer.slice(0, newlineIdx);
      buffer = buffer.slice(newlineIdx + 1);
      if (!line.trim()) continue;
      try {
        const parsed = JSON.parse(line);
        lines.push(parsed);
        for (let i = listeners.length - 1; i >= 0; i--) {
          const l = listeners[i];
          if (l.predicate(parsed)) {
            listeners.splice(i, 1);
            l.resolve(parsed);
          }
        }
      } catch (_) {}
    }
  });

  return {
    lines,
    waitFor(predicate, timeoutMs = 5000) {
      const existing = lines.find(predicate);
      if (existing) return Promise.resolve(existing);
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          const idx = listeners.findIndex((l) => l.resolve === resolve);
          if (idx !== -1) listeners.splice(idx, 1);
          reject(new Error(`Timeout waiting for line matching predicate after ${timeoutMs}ms`));
        }, timeoutMs);
        listeners.push({
          predicate,
          resolve: (val) => {
            clearTimeout(timer);
            resolve(val);
          },
        });
      });
    },
  };
}

describe('native forwarding transport physical proxy contract R19', () => {
  it('implements all 12 required native transport methods', async () => {
    const transport = await createNativeTransport({}, {});
    const REQUIRED_METHODS = [
      'armLostAcknowledgment',
      'subscribe',
      'beginAudit',
      'endCurrentStream',
      'disconnect',
      'holdOutage',
      'restore',
      'changeOwnerEpoch',
      'emitOutput',
      'flush',
      'release',
      'prearmFinalDrain',
    ];
    for (const method of REQUIRED_METHODS) {
      expect(typeof transport[method]).toBe('function');
    }
  });

  it('armLostAcknowledgment enforces explicit session and payload, converting payload to wire Base64 without wildcards', async () => {
    const transport = new NativeForwardingTransport();

    // 1. Missing target session throws typed error
    await expect(
      transport.armLostAcknowledgment({
        mutation: { id: 'm1', operation: 'write', payload: 'nonce' },
      })
    ).rejects.toThrowError(/TARGET_SESSION_REQUIRED/);

    // 2. Unsupported operation throws typed error
    await expect(
      transport.armLostAcknowledgment({
        mutation: { id: 'm1', operation: 'invalid', payload: 'nonce' },
        target: { backendSessionId: 'sess-1' },
      })
    ).rejects.toThrowError(/UNSUPPORTED_MUTATION_OPERATION/);

    // 3. Missing payload throws typed error
    await expect(
      transport.armLostAcknowledgment({
        mutation: { id: 'm1', operation: 'write' },
        target: { backendSessionId: 'sess-1' },
      })
    ).rejects.toThrowError(/MUTATION_PAYLOAD_REQUIRED/);

    // 4. Valid mutation shape converts payload to Base64
    const res = await transport.armLostAcknowledgment({
      mutation: { id: 'mut-valid-1', operation: 'write', payload: 'hello-world' },
      target: { backendSessionId: 'sess-1' },
    });

    expect(res.armed).toBe(true);
    expect(res.mutationId).toBe('mut-valid-1');
    expect(transport.armedLoss.wireData).toBe(Buffer.from('hello-world').toString('base64'));
    expect(transport.armedLoss.sessionId).toBe('sess-1');
  });

  it('startProxy rejects when setup.profile is missing on non-windows without upstreamPort', async () => {
    const transport = new NativeForwardingTransport();
    if (process.platform !== 'win32') {
      await expect(transport.startProxy({ upstreamSocket: '/nonexistent.sock' })).rejects.toThrowError(
        /SETUP_PROFILE_REQUIRED/
      );
    }
  });

  it('beginAudit rejects without physical agent and profile baseline', async () => {
    const transport = new NativeForwardingTransport();
    await expect(transport.beginAudit({ setup: null, roster: [] })).rejects.toThrowError(
      /AUDIT_PHYSICAL_BASELINE_REQUIRED/
    );
  });

  it('changeOwnerEpoch and emitOutput throw typed missing product seam errors', async () => {
    const transport = new NativeForwardingTransport();
    await expect(transport.changeOwnerEpoch({ target: { backendSessionId: 's1' } })).rejects.toThrowError(
      /UNAVAILABLE_SEAM_AUTHORITY_EPOCH/
    );
    await expect(transport.emitOutput({ target: { backendSessionId: 's1' }, marker: 'm1' })).rejects.toThrowError(
      /UNAVAILABLE_SEAM_FIXTURE_OUTPUT/
    );
  });

  it('physical proxy: exact session and payload matching, unsolicited frame resilience, and chunked UTF-8 handling', async () => {
    // Multi-byte UTF-8 test string with Korean characters and emojis
    const payloadA = '한글_데이터_🔥_alpha';
    const wireA = Buffer.from(payloadA, 'utf8').toString('base64');
    const payloadB = '한글_데이터_✨_beta';
    const wireB = Buffer.from(payloadB, 'utf8').toString('base64');

    let daemonServer;
    let transport;
    let tempProfileDir;
    const ownedClients = [];

    try {
      // Ephemeral owned fixture profile created via mkdtemp (no hardcoded /test/profile)
      tempProfileDir = await mkdtemp(join(tmpdir(), 'ferryx-forwarder-test-'));

      // Upstream mock daemon handling NDJSON requests portably via TCP
      daemonServer = createServer((sock) => {
        const receiver = createLineReceiver(sock);
        sock.on('data', () => {
          while (receiver.lines.length > 0) {
            const msg = receiver.lines.shift();
            if (msg.type === 'write') {
              // Echo unsolicited output frame containing split multi-byte UTF-8 BEFORE solicited writeOk
              const outFrame = Buffer.from(
                JSON.stringify({ type: 'output', sessionId: msg.sessionId, data: 'echo-한글_🔥_테스트' }) + '\n',
                'utf8'
              );
              const split = Math.floor(outFrame.length / 2);
              sock.write(outFrame.subarray(0, split));
              sock.write(outFrame.subarray(split));
              sock.write(JSON.stringify({ type: 'writeOk' }) + '\n');
            }
          }
        });
      });

      await new Promise((res) => daemonServer.listen(0, '127.0.0.1', () => res()));
      const daemonPort = daemonServer.address().port;

      transport = new NativeForwardingTransport({
        ctx: { setup: { profile: tempProfileDir } },
      });
      const proxyEvents = [];
      transport.subscribe((e) => proxyEvents.push(e));

      // Portable startProxy with upstreamPort: listens on loopback TCP on all platforms
      const proxyEndpoint = await transport.startProxy({ upstreamPort: daemonPort });
      expect(typeof proxyEndpoint.port).toBe('number');

      // Arm lost ACK specifically for session "target-sess" and payload "payloadA"
      await transport.armLostAcknowledgment({
        mutation: { id: 'mut-targeted', operation: 'write', payload: payloadA },
        target: { backendSessionId: 'target-sess' },
      });

      // Test Case 1: Unrelated write - SAME payload but OTHER session -> writeOk passes!
      const client1 = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(client1);
      await new Promise((res) => client1.once('connect', res));
      const receiver1 = createLineReceiver(client1);

      // Subscribe to exact reply BEFORE write with bounded timeout
      const replyPromise1 = receiver1.waitFor((r) => r.type === 'writeOk', 5000);
      client1.write(JSON.stringify({ type: 'write', sessionId: 'other-sess', data: wireA }) + '\n');
      const reply1 = await replyPromise1;

      expect(reply1.type).toBe('writeOk');
      const outMsg1 = receiver1.lines.find((r) => r.type === 'output');
      expect(outMsg1?.data).toBe('echo-한글_🔥_테스트');

      // Test Case 2: Unrelated write - SAME session but OTHER payload -> writeOk passes!
      const client2 = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(client2);
      await new Promise((res) => client2.once('connect', res));
      const receiver2 = createLineReceiver(client2);

      const replyPromise2 = receiver2.waitFor((r) => r.type === 'writeOk', 5000);
      client2.write(JSON.stringify({ type: 'write', sessionId: 'target-sess', data: wireB }) + '\n');
      const reply2 = await replyPromise2;

      expect(reply2.type).toBe('writeOk');

      // Test Case 3: Matched write sent as arbitrary multi-byte UTF-8 fragments across TCP chunks
      const client3 = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(client3);
      await new Promise((res) => client3.once('connect', res));
      const receiver3 = createLineReceiver(client3);

      const client3ClosePromise = new Promise((res) => client3.once('close', res));
      const rawNdjson = Buffer.from(
        JSON.stringify({ type: 'write', sessionId: 'target-sess', data: wireA }) + '\n',
        'utf8'
      );

      // Intentionally split raw bytes at arbitrary offsets including middle of multi-byte sequences
      const splitPoint1 = Math.floor(rawNdjson.length / 3);
      const splitPoint2 = Math.floor((rawNdjson.length * 2) / 3);
      const chunk1 = rawNdjson.subarray(0, splitPoint1);
      const chunk2 = rawNdjson.subarray(splitPoint1, splitPoint2);
      const chunk3 = rawNdjson.subarray(splitPoint2);

      client3.write(chunk1);
      client3.write(chunk2);
      client3.write(chunk3);

      // Await client closure driven purely by physical proxy socket severing
      await client3ClosePromise;

      // Unsolicited output arrived before writeOk
      expect(receiver3.lines.some((r) => r.type === 'output')).toBe(true);
      // writeOk was physically suppressed from client3
      expect(receiver3.lines.some((r) => r.type === 'writeOk')).toBe(false);
      // Suppressed receipt event emitted
      expect(proxyEvents.some((e) => e.kind === 'lost-ack-suppressed' && e.mutationId === 'mut-targeted')).toBe(true);
    } finally {
      // Leak-free cleanup of all resources even if assertions fail
      for (const client of ownedClients) {
        try { client.destroy(); } catch (_) {}
      }
      if (transport) {
        try { await transport.release(); } catch (_) {}
      }
      if (daemonServer) {
        try { await new Promise((res) => daemonServer.close(res)); } catch (_) {}
      }
      if (tempProfileDir) {
        try { await rm(tempProfileDir, { recursive: true, force: true }); } catch (_) {}
      }
    }
  });

  it('endCurrentStream, disconnect, holdOutage, restore publish only actual proxy socket events', async () => {
    const transport = new NativeForwardingTransport();
    const events = [];
    transport.subscribe((e) => events.push(e));

    await transport.endCurrentStream({ target: { backendSessionId: 's1' } });
    expect(events.some((e) => e.kind === 'stream-ended' && e.target === 's1')).toBe(true);

    await transport.disconnect({ target: { backendSessionId: 's1' } });
    expect(events.some((e) => e.kind === 'outage-start' && e.target === 's1')).toBe(true);

    await transport.holdOutage({ target: { backendSessionId: 's1' }, minimumMs: 1 });
    expect(events.some((e) => e.kind === 'outage-elapsed' && e.target === 's1')).toBe(true);

    await transport.restore({ target: { backendSessionId: 's1' } });
    expect(events.some((e) => e.kind === 'proxy-restored' && e.target === 's1')).toBe(true);

    const FORBIDDEN_SYNTHETIC = ['attached', 'ready', 'binding-persisted', 'listener', 'output', 'authority'];
    for (const forbidden of FORBIDDEN_SYNTHETIC) {
      expect(events.some((e) => e.kind === forbidden)).toBe(false);
    }
  });
 
  it('publishes request and delivery events with correlation, payload, and ack lifecycle', async () => {
    let daemonServer;
    let transport;
    let tempProfileDir;
    const ownedClients = [];

    try {
      tempProfileDir = await mkdtemp(join(tmpdir(), 'proxy-req-del-contract-'));
      const payloadText = 'test-payload-123';
      const wireBase64 = Buffer.from(payloadText, 'utf8').toString('base64');

      daemonServer = createServer((daemonConn) => {
        const receiver = createLineReceiver(daemonConn);
        daemonConn.on('data', () => {
          for (const msg of receiver.lines) {
            if (msg._acked) continue;
            msg._acked = true;
            if (msg.type === 'write') {
              daemonConn.write(JSON.stringify({ type: 'writeOk', sessionId: msg.sessionId }) + '\n');
            } else if (msg.type === 'resize') {
              daemonConn.write(JSON.stringify({ type: 'resizeOk', sessionId: msg.sessionId }) + '\n');
            }
          }
        });
      });

      await new Promise((res) => daemonServer.listen(0, '127.0.0.1', () => res()));
      const daemonPort = daemonServer.address().port;

      transport = new NativeForwardingTransport({
        ctx: { setup: { profile: tempProfileDir } },
      });
      const events = [];
      transport.subscribe((e) => events.push(e));

      const proxyEndpoint = await transport.startProxy({ upstreamPort: daemonPort });

      // Arm lost ack for target-sess and payloadText
      await transport.armLostAcknowledgment({
        mutation: { id: 'mut-req-del-1', operation: 'write', payload: payloadText },
        target: { backendSessionId: 'target-sess' },
      });

      const client = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(client);
      await new Promise((res) => client.once('connect', res));
      const receiver = createLineReceiver(client);

      // 1. Send unmatched write -> request event without mutationId, delivery event on writeOk
      const unmatchedWritePromise = receiver.waitFor((r) => r.type === 'writeOk', 5000);
      client.write(JSON.stringify({ type: 'write', sessionId: 'other-sess', data: 'unmatched-data' }) + '\n');
      await unmatchedWritePromise;

      const req1 = events.find((e) => e.kind === 'request' && e.sessionId === 'other-sess');
      expect(req1).toBeDefined();
      expect(req1.operation).toBe('write');
      expect(req1.mutationId).toBeNull();
      expect(req1.payload).toBeUndefined();
      expect(typeof req1.connectionId).toBe('string');

      const del1 = events.find((e) => e.kind === 'delivery' && e.sessionId === 'other-sess');
      expect(del1).toBeDefined();
      expect(del1.operation).toBe('write');
      expect(del1.mutationId).toBeNull();
      expect(del1.payload).toBeUndefined();

      // 2. Send non-write request (resize) -> request event published, but no write delivery
      const resizePromise = receiver.waitFor((r) => r.type === 'resizeOk', 5000);
      client.write(JSON.stringify({ type: 'resize', sessionId: 'other-sess', cols: 80, rows: 24 }) + '\n');
      await resizePromise;

      const reqResize = events.find((e) => e.kind === 'request' && e.operation === 'resize');
      expect(reqResize).toBeDefined();
      expect(reqResize.mutationId).toBeNull();
      const delResize = events.find((e) => e.kind === 'delivery' && e.operation === 'resize');
      expect(delResize).toBeUndefined();

      // 3. Send matched write -> request event with mutationId and payload,
      // delivery event fires even on suppressed lost-ack before lost-ack-suppressed
      const clientClosePromise = new Promise((res) => client.once('close', res));
      client.write(JSON.stringify({ type: 'write', sessionId: 'target-sess', data: wireBase64 }) + '\n');
      await clientClosePromise;

      const reqMatched = events.find((e) => e.kind === 'request' && e.sessionId === 'target-sess' && e.mutationId === 'mut-req-del-1');
      expect(reqMatched).toBeDefined();
      expect(reqMatched.operation).toBe('write');
      expect(reqMatched.payload).toBe(payloadText);

      const delMatched = events.find((e) => e.kind === 'delivery' && e.sessionId === 'target-sess' && e.mutationId === 'mut-req-del-1');
      expect(delMatched).toBeDefined();
      expect(delMatched.operation).toBe('write');
      expect(delMatched.payload).toBe(payloadText);

      // Verify ordering: delivery event must precede lost-ack-suppressed
      const delIdx = events.findIndex((e) => e.kind === 'delivery' && e.mutationId === 'mut-req-del-1');
      const lostAckIdx = events.findIndex((e) => e.kind === 'lost-ack-suppressed' && e.mutationId === 'mut-req-del-1');
      expect(delIdx).toBeGreaterThan(-1);
      expect(lostAckIdx).toBeGreaterThan(-1);
      expect(delIdx).toBeLessThan(lostAckIdx);

      // 4. Send attach -> publishes stream-open with target === sessionId
      const client2 = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(client2);
      await new Promise((res) => client2.once('connect', res));
      client2.write(JSON.stringify({ type: 'attach', sessionId: 'target-sess' }) + '\n');
      await new Promise((res) => setTimeout(res, 20));
      const streamOpen = events.find((e) => e.kind === 'stream-open' && e.target === 'target-sess');
      expect(streamOpen).toBeDefined();
      expect(streamOpen.sessionId).toBe('target-sess');

      // 5. endCurrentStream with holdRecovery: true refuses new connections until restore()
      await transport.endCurrentStream({ target: { backendSessionId: 'target-sess' }, holdRecovery: true });
      const refusedPromise = new Promise((res) => {
        const sub = transport.subscribe((e) => { if (e.kind === 'dial-refused') { sub(); res(e); } });
      });
      const clientRefused = createConnection({ host: '127.0.0.1', port: proxyEndpoint.port });
      ownedClients.push(clientRefused);
      await refusedPromise;
      await transport.restore({ target: { backendSessionId: 'target-sess' } });
    } finally {
      for (const client of ownedClients) {
        try { client.destroy(); } catch (_) {}
      }
      if (transport) {
        try { await transport.release(); } catch (_) {}
      }
      if (daemonServer) {
        try { await new Promise((res) => daemonServer.close(res)); } catch (_) {}
      }
      if (tempProfileDir) {
        try { await rm(tempProfileDir, { recursive: true, force: true }); } catch (_) {}
      }
    }
  });
});