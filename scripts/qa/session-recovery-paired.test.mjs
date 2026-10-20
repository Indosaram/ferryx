import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PAIRED_SCENARIOS, pairedOwnerControls, runPairedScenario } from './session-recovery-paired.mjs';
import { provisionPairedFixture } from './fixtures/transport-provider.mjs';

test('paired scenario roster contains exactly the three authorized network cases', () => {
  assert.deepEqual(PAIRED_SCENARIOS, [
    'paired-relay-loss',
    'paired-backend-unavailable',
    'host-down-descriptor-restore',
  ]);
});

test('missing real paired owner fixture is reported unverified before transport mutation', async () => {
  let created = false;
  const result = await runPairedScenario({
    timeoutMs: 1000,
    transportFixture: {},
    createTransportProvider: async () => {
      created = true;
      return { close: async () => {} };
    },
  }, 'paired-relay-loss');
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'PAIRED_FIXTURE_REQUIRED');
  assert.equal(created, false);
});

test('all scenarios require real paired fixture before opening transport', async () => {
  for (const scenario of PAIRED_SCENARIOS) {
    const result = await runPairedScenario({
      timeoutMs: 1000,
      transportFixture: {},
    }, scenario);
    assert.equal(result.status, 'unverified');
    assert.equal(result.barrierCode, 'PAIRED_FIXTURE_REQUIRED');
    assert.deepEqual(result.observations, []);
  }
});

test('paired control adapter preserves authoritative tuple in retry wire request', async () => {
  const requests = [];
  const controls = pairedOwnerControls({
    async call(request) {
      requests.push(request);
      return { type: 'retryPairedSessionResult', admitted: true };
    },
  });
  const identity = { backendSessionId: 'backend', daemonEpoch: '7', incarnation: 'actor' };
  const result = await controls.retry({ identity });
  assert.equal(result.admitted, true);
  assert.deepEqual(requests, [{ type: 'retryPairedSession', ...identity }]);
});

test('dial audit adapter returns actual actor receipt without synthesizing operation counts', async () => {
  const requests = [];
  const audit = { pendingDial: true, cancellationCount: 2, recoveryAttempts: 3, replayCursor: '9' };
  const controls = pairedOwnerControls({
    async call(request) {
      requests.push(request);
      return { type: 'qaPairedDialAuditOk', audit };
    },
  });
  assert.deepEqual(await controls.dialAudit({ identity: { backendSessionId: 'backend' } }), audit);
  assert.deepEqual(requests, [{ type: 'qaPairedDialAudit', backendSessionId: 'backend' }]);
});

test('owner audit adapter returns actual daemon owner receipt matching schema', async () => {
  const requests = [];
  const controls = pairedOwnerControls({
    async call(request) {
      requests.push(request);
      return { type: 'qaPairedOwnerAuditOk', outcome: 'ok', daemonEpoch: '1', incarnation: 'inc-1', generation: 1, descriptor: null };
    },
  });
  const audit = await controls.ownerAudit({ identity: { backendSessionId: 'backend' } });
  assert.equal(audit.type, 'qaPairedOwnerAuditOk');
  assert.deepEqual(requests, [{ type: 'qaPairedOwnerAudit', backendSessionId: 'backend' }]);
});

test('runPairedScenario rejects paired-relay-loss with unverified and TRAFFIC_AUDITOR_PRODUCER_REQUIRED when producer contract is absent', async () => {
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

  const result = await runPairedScenario(ctx, 'paired-relay-loss');
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'TRAFFIC_AUDITOR_PRODUCER_REQUIRED');
  assert.ok(result.requirements.includes('traffic-endpoint-audit-producer-required'));
});

test('runPairedScenario rejects paired-relay-loss with unverified even when caller supplies synthetic transportDeliveryAuditor', async () => {
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
        deliveredWrites: 1,
        deliveredSignals: 1,
        replayedWrites: 0,
        replayedSignals: 0,
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

  const result = await runPairedScenario(ctx, 'paired-relay-loss');
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'TRAFFIC_AUDITOR_PRODUCER_REQUIRED');
  assert.ok(result.requirements.includes('traffic-endpoint-audit-producer-required'));
});

test('provisionPairedFixture rejects with PAIRED_PROVISIONING_SEAM_REQUIRED when no real spawn seam is provided', async () => {
  const ctx = {
    candidateStamp: 'r3-candidate-stamp',
    binary: '/bin/sh',
  };
  await assert.rejects(
    provisionPairedFixture(ctx, {}),
    (err) => err.code === 'PAIRED_PROVISIONING_SEAM_REQUIRED',
  );
});

test('provisionPairedFixture validates measured returned identity from real provision seam', async () => {
  const ctx = {
    candidateStamp: 'r3-candidate-stamp',
    binary: '/bin/sh',
  };
  const ledgerEntries = [];
  const fakeLedger = { record: (entry) => ledgerEntries.push(entry) };
  const fixture = await provisionPairedFixture(ctx, {
    ledger: fakeLedger,
    provisionPairedSession: async () => ({
      paired: {
        backendSessionId: 'b-measured',
        daemonEpoch: '99',
        incarnation: 'inc-measured',
        descriptor: { hostId: 'h-measured' },
        target: { hostId: 'h-measured' },
      },
      pid: 12345,
    }),
  });

  assert.ok(fixture.paired);
  assert.equal(fixture.paired.backendSessionId, 'b-measured');
  assert.equal(fixture.paired.daemonEpoch, '99');
  assert.equal(fixture.paired.incarnation, 'inc-measured');
  assert.equal(ledgerEntries.length, 1);
  assert.equal(ledgerEntries[0].pid, 12345);
});

test('host-down-descriptor-restore returns unverified when provider lacks cold daemon restart seam', async () => {
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
        expectedRoute: 'loopback',
      },
    },
    createTransportProvider: async () => ({
      pairedOwner: {
        bind: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' } }),
        routeReceipt: async () => ({ selected: 'loopback', websocket: true, loopback: true }),
        ownerAudit: async () => ({ outcome: 'ok', daemonEpoch: '1', incarnation: 'inc-1', generation: 1 }),
        dialAudit: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-1' }, pendingDial: false }),
        waitState: async () => ({ state: 'owner_unavailable' }),
        dropLoopback: async () => ({ type: 'closeOk' }),
      },
      close: async () => {},
    }),
  };

  const result = await runPairedScenario(ctx, 'host-down-descriptor-restore');
  assert.equal(result.status, 'unverified');
  assert.equal(result.barrierCode, 'CLIENT_DAEMON_COLD_RESTART_SEAM_REQUIRED');
});

test('host-down-descriptor-restore measures PID, incarnation, and cursor across both daemons when cold restart succeeds', async () => {
  const ctx = {
    timeoutMs: 1000,
    transportFixture: {
      schema: 'ferryx-task9-transport-provider/1',
      paired: {
        backendSessionId: 'b-1',
        descriptor: { hostId: 'h-1', backendSessionId: 'b-1', target: { hostId: 'h-1', incarnation: 'inc-host' }, generation: 1, afterSequence: 42 },
        target: { hostId: 'h-1', incarnation: 'inc-host' },
        hostId: 'h-1',
        daemonEpoch: '1',
        incarnation: 'inc-host',
        expectedRoute: 'loopback',
        hostPid: 1001,
      },
    },
    createTransportProvider: async () => ({
      pairedOwner: {
        bind: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-host' } }),
        routeReceipt: async () => ({ selected: 'loopback', websocket: true, loopback: true }),
        ownerAudit: async () => ({ outcome: 'ok', daemonEpoch: '1', incarnation: 'inc-host', generation: 1 }),
        dialAudit: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-host' }, pendingDial: false }),
        waitState: async () => ({ state: 'owner_unavailable' }),
        dropLoopback: async () => ({ type: 'closeOk' }),
      },
      restartClientDaemon: async () => ({
        getClientPid: async () => 2002,
        pairedOwner: {
          restoreDescriptorCold: async () => ({ target: { hostId: 'h-1', incarnation: 'inc-host' }, generation: 1 }),
          waitState: async () => ({ state: 'proxy_attached' }),
          retry: async () => ({ admitted: true }),
          ownerAudit: async () => ({ outcome: 'ok', daemonEpoch: '1', incarnation: 'inc-host', generation: 1 }),
          dialAudit: async () => ({ identity: { backendSessionId: 'b-1', daemonEpoch: '1', incarnation: 'inc-host' }, pendingDial: false, replayCursor: 42 }),
        },
      }),
      close: async () => {},
    }),
  };

  const result = await runPairedScenario(ctx, 'host-down-descriptor-restore');
  assert.equal(result.status, 'passed');
});
