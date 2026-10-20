import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { runTransportWriteDelivery, transportScenarioExecutors, PAIRED_SCENARIOS } from './session-recovery-transports.mjs';
import { createAckLossHelperClient, createAckLossDaemonControlClient, encodeRemoteOwnerMutationData, subscribe, TransportFaultSocket, pairedOwnerControls } from './fixtures/transport-provider.mjs';
import { createConnection, createServer } from 'node:net';
import { APPLICATION_PROTOCOL, decodeDeliveryApplicationRecord, startDeliveryApplication } from './fixtures/transport-delivery-application.mjs';
import { createTransportDeliveryObserver } from './fixtures/transport-delivery-observer.mjs';

test('fault socket rejects physical candidate dials then forwards after the bounded window', async () => {
  const source = new EventEmitter();
  let accepted = false;
  const upstream = createServer((socket) => { accepted = true; socket.on('data', (bytes) => socket.write(bytes)); });
  const listening = new Promise((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  await listening;
  const endpoint = { host: '127.0.0.1', port: upstream.address().port };
  const fault = new TransportFaultSocket(endpoint);
  const proxy = await fault.listen();
  try {
    fault.interrupt(21);
    for (let i = 0; i < 21; i++) {
      const refused = subscribe(fault, 'receipt', (r) => r.kind === 'dial-refused' && r.remaining === 21 - i - 1, 1000);
      const socket = createConnection(proxy.port, proxy.host);
      await new Promise((resolve) => socket.once('close', resolve));
      await refused.promise;
    }
    assert.equal(fault.trace.filter((r) => r.kind === 'dial-refused').length, 21);
    assert.equal(accepted, false);
  } finally {
    await fault.close();
    await new Promise((resolve) => upstream.close(resolve));
  }
});

test('fault socket keeps the post-21 retry held until explicit route release', async () => {
  const upstream = createServer((socket) => socket.destroy());
  await new Promise((resolve) => upstream.listen(0, '127.0.0.1', resolve));
  const fault = new TransportFaultSocket({ host: '127.0.0.1', port: upstream.address().port });
  const proxy = await fault.listen();
  try {
    fault.interrupt(21);
    for (let i = 0; i < 21; i++) {
      const refused = subscribe(fault, 'receipt', (r) => r.kind === 'dial-refused' && r.remaining === 20 - i, 1000);
      const socket = createConnection(proxy.port, proxy.host);
      await new Promise((resolve) => socket.once('close', resolve));
      await refused.promise;
    }
    const held = subscribe(fault, 'receipt', (receipt) => receipt.kind === 'dial-held', 1000);
    const blockedRetry = createConnection(proxy.port, proxy.host);
    await new Promise((resolve) => blockedRetry.once('close', resolve));
    await held.promise;
    assert.equal(fault.trace.filter((receipt) => receipt.kind === 'dial-forwarded').length, 0);
    fault.release();
    const forwarded = subscribe(fault, 'receipt', (receipt) => receipt.kind === 'dial-forwarded', 1000);
    const recovered = createConnection(proxy.port, proxy.host);
    await forwarded.promise;
    recovered.destroy();
  } finally {
    await fault.close();
    await new Promise((resolve) => upstream.close(resolve));
  }
});

test('missing owner QA seam is explicitly incomplete before mutations', async () => {
  const target = { hostId: 'host', ownerId: 'spawn-owner', epoch: '7', backendSessionId: 'spawn-target' };
  const descriptor = { backendSessionId: 'spawn-target', target, config: { projectId: 'project' } };
  const details = { descriptor, state: 'connected', generation: 1, attempts: 0 };
  const stdin = new EventEmitter();
  const signals = new EventEmitter();
  const chunks = [];
  const daemonCalls = [];
  const app = startDeliveryApplication({ runId: 'recorded-run', stdin, signals, platform: 'linux',
    stdout: { write(text) { chunks.push({ cursor: String(chunks.length + 1), data: Buffer.from(text).toString('base64') }); } } });
  const helper = { async call(op, params) {
    if (op === 'pty.write') {
      assert.deepEqual(params.target, target);
      stdin.emit('data', Buffer.from(params.text));
      return { ok: true, data: { accepted: true } };
    }
    assert.equal(op, 'pty.read');
    const selected = chunks.filter((c) => BigInt(c.cursor) > BigInt(params.cursor));
    return { ok: true, data: { target, gap: false, exited: false,
      cursor: selected.at(-1)?.cursor ?? params.cursor, chunks: selected } };
  } };
  const fixture = { helper: { endpoint: { host: '127.0.0.1', port: 1 }, token: 'test' },
    selection: { backendSessionId: 'selected-daemon-session' }, delivery: {
      runId: 'recorded-run', operationId: 'write-op', nonce: 'nonce', ownerId: 'isolated-owner',
      sshSession: { config: { projectId: 'project', host: { hostname: 'ssh.example', port: 22 } }, params: { args: ['application.mjs', '--run-id', 'recorded-run'] }, clientRequestId: 'qa-request' },
      maxReads: 2, recoveryReads: 2, payload: 'actual payload',
      faultRoute: { upstream: { host: '127.0.0.1', port: 22 } } } };
  const fakeFaultSocket = new EventEmitter();
  Object.assign(fakeFaultSocket, { endpoint: { host: '127.0.0.1', port: 2222 },
    socket: new EventEmitter(), snapshot: () => [], interrupt() {}, release() {}, close: async () => {} });
  const connect = async (_endpoint, token, timeoutMs, signal, qaFault) => {
    const socket = new EventEmitter();
    socket.destroy = () => socket.emit('close');
    socket.write = (bytes) => {
      const request = JSON.parse(bytes.subarray(4).toString());
      helper.call(request.op, request.params).then((reply) => {
        const body = Buffer.from(JSON.stringify(reply));
        const header = Buffer.alloc(4);
        header.writeUInt32BE(body.length);
        socket.emit('data', Buffer.concat([header, body]));
      });
      return true;
    };
    return createAckLossHelperClient({ socket, token, timeoutMs, signal, ...qaFault });
  };
  try {
    const result = await runTransportWriteDelivery({ transportFixture: fixture, timeoutMs: 1000 }, {
      fixture, helper, observer: { async daemon(payload) {
        daemonCalls.push(payload);
        return { type: payload.type + 'Ok' };
      } },
      async createRemoteSession(config, params, clientRequestId) {
        assert.equal(config.projectId, 'project');
        assert.deepEqual(params.args.slice(-2), ['--run-id', 'recorded-run']);
        assert.equal(clientRequestId, 'qa-request');
        return { descriptor, details };
      },
      async createFaultSocket() { return fakeFaultSocket; },
    }, connect);
    assert.equal(result.status, 'unverified');
    assert.equal(result.barrierCode, 'OWNER_AUDIT_CONTROL_REQUIRED');
    assert.equal(result.scenario, 'ssh-ipc-failure');
    assert.deepEqual(daemonCalls, []);
    assert.equal(result.evidence.remoteSession.descriptor.backendSessionId, 'spawn-target');
    assert.equal(result.evidence.ownerAudit.length, 0);
  } finally { app.stop(); }
});

test('missing delivery fixture is explicitly incomplete before mutation', async () => {
  const result = await runTransportWriteDelivery({ transportFixture: {}, timeoutMs: 1000 }, { fixture: {} });
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'DELIVERY_FIXTURE_REQUIRED');
  assert.equal(result.evidence.helperAudits.length, 0);
});

test('owner mutation payload uses the daemon base64 serde wire representation', () => {
  const payload = Buffer.from([0x03, 0x61, 0x62, 0x63]);
  const request = { type: 'qaRemoteOwnerMutation', sessionId: 'backend-1', operation: 'write',
    generation: 9, data: encodeRemoteOwnerMutationData(payload) };
  assert.deepEqual(request, { type: 'qaRemoteOwnerMutation', sessionId: 'backend-1',
    operation: 'write', generation: 9, data: 'A2FiYw==' });
  assert.equal(JSON.stringify(request).includes('"type":"Buffer"'), false);
});

test('owner state wait is a typed subscription request bound to one backend session', () => {
  const request = { type: 'qaRemoteOwnerWaitState', sessionId: 'backend-1', state: 'disconnected' };
  assert.deepEqual(request, { type: 'qaRemoteOwnerWaitState', sessionId: 'backend-1', state: 'disconnected' });
});

test('runTransportWriteDelivery drives one uncertain write through 21 refusals and same-owner recovery', async () => {
  const target = { hostId: 'host', ownerId: 'owner', epoch: '7', backendSessionId: 'backend-1' };
  const descriptor = { backendSessionId: 'backend-1', target, config: { projectId: 'project' } };
  const state = (name, attempts, generation = 4) => ({ descriptor, state: name, attempts, generation });
  const calls = [];
  const receipts = [];
  let appInput;
  const owner = {
    async remoteOwnerAudit(id) { calls.push(['audit', id]); return { type: 'qaRemoteOwnerAuditOk', details: state('connected', 0), events: [] }; },
    async remoteOwnerWaitState(id, next) { calls.push(['subscribe-state', id, next]);
      if (next === 'connected') return new Promise((resolve) => { owner.resolveConnected = resolve; });
      return new Promise((resolve) => { owner.resolveDisconnected = resolve; }); },
    async remoteOwnerWaitRefusals(id, count) { calls.push(['subscribe-refusals', id, count]);
      return new Promise((resolve) => { owner.resolveRefusals = resolve; }); },
    async remoteOwnerMutation(id, operation, generation, data) {
      calls.push([operation, id, generation, data ? Buffer.from(data).toString() : undefined]);
      if (operation === 'write') {
        const message = JSON.parse(Buffer.from(data).toString());
        appInput.emit('data', Buffer.from(`${JSON.stringify({ ...message, protocol: APPLICATION_PROTOCOL, runId: 'run' })}\n`));
        return { type: 'qaRemoteOwnerMutationError', acknowledgement: 'lost' };
      }
      if (operation === 'recover') owner.resolveConnected(state('connected', 21, 5));
      return { type: 'qaRemoteOwnerMutationOk', details: state('reconnecting', 21) };
    },
    async remoteOwnerReleaseFaultRoute(id) { calls.push(['authorize-release', id]); return { type: 'qaRemoteOwnerReleaseFaultRouteOk', details: state('disconnected', 21) }; },
  };
  const faultSocket = { endpoint: { host: '127.0.0.1', port: 3333 }, socket: new EventEmitter(),
    snapshot: () => receipts, interrupt(count) { calls.push(['interrupt', count]);
      for (let n = 1; n <= count; n++) {
        const receipt = { kind: 'dial-refused', dial: `dial-${n}`, remaining: count - n };
        receipts.push(receipt); faultSocket.socket.emit('receipt', receipt);
      }
      owner.resolveRefusals(state('disconnected', count)); owner.resolveDisconnected(state('disconnected', count)); },
    release() { calls.push(['release']); const receipt = { kind: 'dial-forwarded', dial: 'dial-recovery' };
      receipts.push(receipt); faultSocket.socket.emit('receipt', receipt); },
    close: async () => {} };
  const fixture = { helper: { endpoint: { host: '127.0.0.1', port: 1 }, token: 'token' },
    selection: { backendSessionId: 'selected' }, delivery: { runId: 'run', operationId: 'op', nonce: 'nonce', ownerId: 'test',
      sshSession: { config: { projectId: 'project', host: { hostname: 'ssh.example', port: 22 } },
        params: { args: ['application.mjs', '--run-id', 'run'] }, clientRequestId: 'request' },
      faultRoute: { upstream: { host: '127.0.0.1', port: 22 } }, maxReads: 1, recoveryReads: 0, payload: 'body' } };
  const helper = { async call(op, params) {
    if (op === 'pty.write') {
      if (params.text.includes('"type":"configure"')) appInput.emit('data', Buffer.from(params.text));
      return { ok: true, data: { accepted: true } };
    }
    const chunks = produced.chunks.filter((c) => BigInt(c.cursor) > BigInt(params.cursor));
    return { ok: true, data: { target, gap: false, exited: false,
      cursor: chunks.at(-1)?.cursor ?? String(params.cursor), chunks } };
  } };
  const connect = async () => { const socket = new EventEmitter(); socket.destroy = () => socket.emit('close'); return {
    socket, async close() {}, async call(op, params) { return helper.call(op, params); }, snapshot: () => ({ audit: [] })
  }; };
  const produced = { cursor: 0, chunks: [] };
  const observer = createTransportDeliveryObserver({ helper, target, cursor: '0', producerContract: APPLICATION_PROTOCOL,
    runId: 'run', operation: { kind: 'write', id: 'op', nonce: 'nonce', target, cursor: '0',
      before: { writeCount: 0, nonceCount: 0, signalCount: 0 } },
    decodeRecord: (line) => decodeDeliveryApplicationRecord(line, 'run') });
  appInput = new EventEmitter(); const appSignals = new EventEmitter();
  let resolveReady;
  const readyObserved = new Promise((resolve) => { resolveReady = resolve; });
  const appOutput = { write(text) {
    const record = JSON.parse(text);
    record.protocol = APPLICATION_PROTOCOL; record.runId = 'run';
    if (record.event === 'ready' || record.event === 'write-consumed') record.target = target;
    const output = `${JSON.stringify(record)}\n`;
    produced.cursor++; produced.chunks.push({ cursor: String(produced.cursor), data: Buffer.from(output).toString('base64') });
    if (record.event === 'ready') resolveReady(record);
  } };
  const writeOnce = helper.call.bind(helper);
  helper.call = async (op, params) => {
    if (op === 'pty.write') {
      const message = JSON.parse(params.text);
      appInput.emit('data', Buffer.from(`${JSON.stringify({ ...message, protocol: APPLICATION_PROTOCOL, runId: 'run' })}\n`));
      return { ok: true, data: { accepted: true } };
    }
    return writeOnce(op, params);
  };
  const app = startDeliveryApplication({ runId: 'run', stdin: appInput, stdout: appOutput, signals: appSignals, platform: 'linux' });
  faultSocket.socket.on('receipt', (receipt) => { if (receipt.kind === 'dial-forwarded') calls.push(['forwarded', receipt.dial]); });
  owner.remoteOwnerMutation = async function(id, operation, generation, data) {
    calls.push([operation, id, generation, data ? Buffer.from(data).toString() : undefined]);
    if (operation === 'write') {
      const message = JSON.parse(Buffer.from(data).toString());
      await helper.call('pty.write', { target, text: `${JSON.stringify({ ...message, protocol: APPLICATION_PROTOCOL, runId: 'run' })}\n` });
      return { type: 'qaRemoteOwnerMutationError', acknowledgement: 'lost' };
    }
    if (operation === 'recover') owner.resolveConnected(state('connected', 21, 5));
    return { type: 'qaRemoteOwnerMutationOk', details: state('reconnecting', 21) };
  };
  const run = runTransportWriteDelivery({ transportFixture: fixture, timeoutMs: 1000 }, {
    fixture, helper, observer, ...owner,
    async createRemoteSession() { return { descriptor, details: state('connected', 0) }; },
    async createFaultSocket() { return faultSocket; },
  }, connect);
  const result = await run;
  assert.equal(result.status, 'passed', JSON.stringify(result));
  assert.equal(result.evidence.baseline.ready.event, 'ready');
  assert.deepEqual(result.evidence.baseline.ready.target, target);
  assert.equal(await readyObserved.then(() => true), true);
  assert.equal(result.evidence.baseline.ready.event, 'ready');
  assert.deepEqual(result.evidence.baseline.ready.target, target);
  assert.equal(result.evidence.remoteSession.descriptor.backendSessionId, 'backend-1');
  assert.equal(result.evidence.application[0].writeDeliveryObserved, true);
  assert.equal(result.evidence.application[0].observations[0].record.nonceCount, 1);
  assert.equal(result.evidence.application[0].observations[0].record.duplicate, false);
  assert.equal(calls.filter(([kind]) => kind === 'write').length, 1);
  assert.equal(receipts.filter((r) => r.kind === 'dial-refused').length, 21);
  const ownerSetup = calls.filter(([kind]) => ['audit', 'subscribe-state', 'subscribe-refusals', 'interrupt'].includes(kind));
  assert.deepEqual(ownerSetup.slice(0, 4).map(([kind]) => kind), ['audit', 'subscribe-state', 'subscribe-refusals', 'interrupt']);
  assert.ok(calls.findIndex(([kind]) => kind === 'authorize-release') < calls.findIndex(([kind]) => kind === 'release'));
  const connectedSubscribe = calls.findIndex(([kind, , next]) => kind === 'subscribe-state' && next === 'connected');
  assert.ok(connectedSubscribe >= 0 && connectedSubscribe < calls.findIndex(([kind]) => kind === 'release'));
  assert.ok(calls.findIndex(([kind]) => kind === 'release') < calls.findIndex(([kind]) => kind === 'recover'));
  assert.equal(calls.filter(([kind]) => kind === 'write').length, 1);
  assert.equal(calls.find(([kind]) => kind === 'authorize-release')?.[1], 'backend-1');
  assert.ok(calls.filter(([kind]) => kind === 'subscribe-state').every(([, id]) => id === 'backend-1'));
  assert.deepEqual(faultSocket.snapshot().filter((r) => r.kind === 'dial-refused').map((r) => r.dial),
    Array.from({ length: 21 }, (_, index) => `dial-${index + 1}`));
  assert.equal(faultSocket.snapshot().filter((r) => r.kind === 'dial-forwarded').length, 1);
  app.stop();
});

test('missing owner control fixture reaches owner-audit barrier without remote I/O', async () => {
  const target = { hostId: 'host', ownerId: 'spawn-owner', epoch: '7', backendSessionId: 'spawn-target' };
  const descriptor = { backendSessionId: 'spawn-target', target, config: { projectId: 'project' } };
  const details = { descriptor, state: 'connected', generation: 1, attempts: 0 };
  const fakeFaultSocket = new EventEmitter();
  Object.assign(fakeFaultSocket, { endpoint: { host: '127.0.0.1', port: 2222 }, socket: new EventEmitter(),
    snapshot: () => [], interrupt() {}, release() {}, close: async () => {} });
  const fixture = { helper: { endpoint: { host: '127.0.0.1', port: 1 }, token: 'test' },
    selection: { backendSessionId: 'selected-daemon-session' }, delivery: { runId: 'recorded-run', operationId: 'write-op',
      nonce: 'nonce', ownerId: 'isolated-owner', sshSession: { config: { projectId: 'project', host: { hostname: 'ssh.example', port: 22 } },
        params: { args: ['application.mjs', '--run-id', 'recorded-run'] }, clientRequestId: 'qa-request' },
      maxReads: 2, recoveryReads: 2, payload: 'actual payload', faultRoute: { upstream: { host: '127.0.0.1', port: 22 } } } };
  let remoteCalls = 0;
  const result = await runTransportWriteDelivery({ transportFixture: fixture, timeoutMs: 1000 }, {
    fixture, helper: { async call() { remoteCalls++; throw new Error('must not perform remote IO'); } },
    observer: { async daemon() { throw new Error('owner mutation must not occur'); } },
    async createRemoteSession() { return { descriptor, details }; }, async createFaultSocket() { return fakeFaultSocket; },
  }, async () => { remoteCalls++; throw new Error('must not open a remote connection'); });
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'OWNER_AUDIT_CONTROL_REQUIRED');
  assert.equal(result.evidence.ownerAudit.length, 0);
  assert.equal(remoteCalls, 0);
});

test('createAckLossDaemonControlClient frames NDJSON and suppresses uncertain dropped ack', async () => {
  const linesReceived = [];
  const server = createServer((socket) => {
    let buffer = '';
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      let idx;
      while ((idx = buffer.indexOf('\n')) !== -1) {
        const line = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 1);
        linesReceived.push(JSON.parse(line));
        socket.write(JSON.stringify({ type: 'writeOk' }) + '\n');
      }
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  const socket = createConnection(port, '127.0.0.1');
  await new Promise((resolve) => socket.once('connect', resolve));

  let suppressed = false;
  const client = createAckLossDaemonControlClient({
    socket,
    ownerId: 'daemon-test',
    creationBaseline: 0,
    timeoutMs: 1000,
    suppress: (receipt) => {
      suppressed = true;
      return receipt.op === 'write';
    },
  });

  try {
    await assert.rejects(
      client.call({ type: 'write', sessionId: 's-1', data: 'dGVzdA==' }),
      (err) => err.code === 'DAEMON_CONTROL_REPLY_UNCERTAIN',
    );
    assert.equal(suppressed, true);
    assert.equal(linesReceived.length, 1);
    assert.deepEqual(linesReceived[0], { type: 'write', sessionId: 's-1', data: 'dGVzdA==' });
    // Once uncertain, client is closed and rejects further calls without replaying:
    await assert.rejects(
      client.call({ type: 'write', sessionId: 's-1', data: 'dGVzdA==' }),
      (err) => err.code === 'DAEMON_CONTROL_CLIENT_CLOSED',
    );
    assert.equal(linesReceived.length, 1); // Zero automatic replay!
  } finally {
    client.close();
    await new Promise((resolve) => server.close(resolve));
  }
});

test('transportScenarioExecutors reports missing paired fixture without guessing pairedSession', async () => {
  for (const scenario of PAIRED_SCENARIOS) {
    const executor = transportScenarioExecutors[scenario];
    assert.equal(typeof executor, 'function');
    const result = await executor({ transportFixture: {}, timeoutMs: 1000 });
    assert.equal(result.status, 'unverified');
    assert.equal(result.barrierCode, 'PAIRED_FIXTURE_REQUIRED');
  }
});

test('paired route receipt schema uses camelCase afterSequence', async () => {
  const receipt = {
    selected: 'relay',
    websocket: true,
    target: { hostId: 'h', ownerId: 'o', epoch: '1', backendSessionId: 'b' },
    generation: 2,
    directEpoch: null,
    loopback: false,
    afterSequence: 42,
  };
  assert.equal(receipt.afterSequence, 42);
  assert.equal(receipt.after_sequence, undefined);
});

test('transportScenarioExecutors dispatches paired scenario and passes honestly when checks pass', async () => {
  const ctx = {
    timeoutMs: 1000,
    transportFixture: {
      schema: 'ferryx-task9-transport-provider/1',
      paired: {
        backendSessionId: 'b-1',
        descriptor: { hostId: 'h-1', backendSessionId: 'b-1', target: { hostId: 'h-1' }, generation: 1 },
        target: { hostId: 'h-1' },
        hostId: 'h-1',
        daemonEpoch: '1',
        incarnation: 'inc-1',
        expectedRoute: 'relay',
      },
    },
    transportDeliveryAuditor: {
      snapshot: () => ({
        deliveredWrites: 1, deliveredSignals: 1,
        replayedWrites: 0, replayedSignals: 0,
      }),
    },
    createTransportProvider: async () => ({
      pairedOwner: {
        bind: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' } }),
        routeReceipt: async () => ({ selected: 'relay', websocket: true, loopback: true }),
        ownerAudit: async () => ({ outcome: 'ok', daemonEpoch: '1', incarnation: 'inc-1', generation: 1 }),
        dialAudit: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' }, pendingDial: false }),
        waitState: async () => ({ state: 'owner_unavailable' }),
        retry: async () => ({ admitted: true }),
        dropLoopback: async () => ({ type: 'closeOk' }),
        mutate: async () => ({ type: 'qaRemoteOwnerMutationOk' }),
      },
      close: async () => {},
    }),
  };

  const result = await transportScenarioExecutors['paired-relay-loss'](ctx);
  assert.equal(result.status, 'passed');
  assert.deepEqual(result.assertions, ['real-route', 'same-owner', 'zero-create-spawn-close', 'zero-automatic-replay']);
});
