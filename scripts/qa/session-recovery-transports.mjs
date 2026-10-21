/** Task9 reachable binary observations; unsupported rows never become PASS. */
import assert from 'node:assert/strict';
import { createTransportProvider, connectTransportHelper, subscribe, TransportBarrier } from './fixtures/transport-provider.mjs';
import { APPLICATION_PROTOCOL, decodeDeliveryApplicationRecord } from './fixtures/transport-delivery-application.mjs';
import { createTransportDeliveryObserver } from './fixtures/transport-delivery-observer.mjs';
import { PAIRED_SCENARIOS, runPairedScenario } from './session-recovery-paired.mjs';

export const TRANSPORT_SCENARIOS = Object.freeze([
  'ssh-ipc-failure', 'paired-relay-loss', 'paired-backend-unavailable', 'host-down-descriptor-restore',
]);

async function execute(ctx, scenario) {
  let provider;
  const timeline = [];
  const observations = [];
  const requirements = [];
  try {
    provider = await createTransportProvider(ctx);
    if (scenario === 'ssh-ipc-failure' && ctx.transportFixture.delivery) {
      return await runTransportWriteDelivery(ctx, provider);
    }
    provider.observer.events.on('receipt', (r) => timeline.push(r));
    const before = await provider.observer.snapshot();
    observations.push({ check: 'existing-rpc-snapshot', source: 'real-binary-rpc', evidence: before });
    if (before.helper) {
      assert.equal(before.helper.ok, true);
      assert.ok(Number.isInteger(before.helper.data.pid) && before.helper.data.pid > 0);
      assert.equal(before.helper.data.exited, false);
      observations.push({ check: 'real-helper-live-pid', source: 'real-helper-rpc', evidence: before.helper });
    } else requirements.push(provider.observer.unsupported('real-owner-helper-output'));

    if (scenario === 'ssh-ipc-failure') {
      if (ctx.transportFixture?.delivery) {
        return await runTransportWriteDelivery(ctx, provider);
      }
      const failed = subscribe(provider.observer.events, 'receipt',
        (r) => r.kind === 'ipc-rearm-failure' && r.count === 21, ctx.timeoutMs, ctx.signal);
      try {
        const owner = provider.observer.startSsh(21);
        await failed.promise;
        assert.equal(owner.failureCount, 21);
        observations.push({ check: '21-product-retry-callback-failures', source: 'candidate-sshRecovery-injection', count: owner.failureCount });
      } finally { failed.cancel(); }
      const after = await provider.observer.snapshot();
      observations.push({ check: 'post-rearm-rpc-snapshot', source: 'real-binary-rpc', evidence: after });
      if (before.helper && after.helper) assert.equal(after.helper.data.pid, before.helper.data.pid);
      requirements.push(provider.observer.unsupported('daemon-ssh-status-event-subscription'));
      requirements.push(provider.observer.unsupported('uncertain-delivery'),
        provider.observer.unsupported('real-owner-nonce-delivery'),
        provider.observer.unsupported('21-network-refusals'),
        provider.observer.unsupported('validated-geometry'),
        provider.observer.unsupported('zero-create-replay'), provider.observer.unsupported('native-presentation'));
      return { status: 'unverified', scenario, timeline, observations, requirements, assertions: [] };
    }
    if (PAIRED_SCENARIOS.includes(scenario)) {
      return await runPairedScenario(ctx, scenario);
    }
    requirements.push(provider.observer.unsupported('uncertain-delivery'),
      provider.observer.unsupported('real-owner-nonce-delivery'),
      provider.observer.unsupported('21-network-refusals'),
      provider.observer.unsupported('validated-geometry'),
      provider.observer.unsupported('zero-create-replay'), provider.observer.unsupported('native-presentation'));
    return { status: 'unverified', scenario, timeline, observations, requirements, assertions: [] };
  } catch (error) {
    return { status: error instanceof TransportBarrier ? 'unverified' : 'failed', scenario,
      barrierCode: error.code, required: error.required, detail: error.message,
      timeline, observations, requirements, assertions: [] };
  } finally { await provider?.close(); }
}

/** Same fixture/context as existing transport scenarios. Inject connectors only
 * at this existing helper IO boundary for deterministic source contract tests.
 */
export async function runTransportWriteDelivery(ctx, provider, connectHelper = connectTransportHelper) {
  const f = ctx.transportFixture;
  const d = f.delivery;
  const evidence = { application: [], helperAudits: [], recovery: [], ownerAudit: [] };
  const connections = [];
  const requirements = ['candidate-ssh-fault-route', 'machine-native-signal-surface', 'native-presentation'];
  let target;
  let cursor = '0';
  try {
    if (!f.helper || !provider.helper || !d?.runId || !d.operationId || !d.nonce ||
        !d.sshSession || !d.ownerId || !Number.isInteger(d.maxReads) || d.maxReads < 1 ||
        !Number.isInteger(d.recoveryReads) || d.recoveryReads < 1 ||
        !provider.fixture.selection.backendSessionId) {
      throw new TransportBarrier('DELIVERY_FIXTURE_REQUIRED',
        'delivery {runId,operationId,nonce,ownerId,sshSession:{config,params,clientRequestId},maxReads,recoveryReads,payload}; existing helper');
    }
    assert.equal(typeof d.payload, 'string');
    assert.deepEqual(d.sshSession.params.args?.slice(-2), ['--run-id', d.runId]);
    const createRemoteSession = provider.createRemoteSession ?? provider.control?.createRemoteSession;
    if (typeof createRemoteSession !== 'function') {
      throw new TransportBarrier('SSH_SETUP_CONTROL_REQUIRED', 'daemon control createRemoteSession method');
    }
    let createConfig = d.sshSession.config;
    let createParams = d.sshSession.params;
    let faultSocket = null;
    if (!d.faultRoute?.upstream) throw new TransportBarrier('FAULT_ROUTE_REQUIRED',
      'delivery.faultRoute.upstream {host,port} from verifier provisioning');
    if (d.faultRoute?.upstream) {
      if (typeof provider.createFaultSocket !== 'function') {
        throw new TransportBarrier('FAULT_SOCKET_REQUIRED', 'QA fault route upstream and explicit SSH endpoint fields');
      }
      faultSocket = await provider.createFaultSocket(d.faultRoute.upstream);
      createConfig = structuredClone(createConfig);
      if (!createConfig.host || typeof createConfig.host !== 'object') {
        throw new TransportBarrier('SSH_HOST_CONFIG_REQUIRED', 'delivery.sshSession.config.host {hostname,port}');
      }
      createConfig.host.hostname = faultSocket.endpoint.host;
      createConfig.host.port = faultSocket.endpoint.port;
      createParams = structuredClone(createParams);
      createParams.project_id = createConfig.projectId;
    }
    const created = await createRemoteSession.call(provider, createConfig, createParams,
      d.sshSession.clientRequestId);
    const descriptor = created.descriptor;
    const details = created.details;
    target = descriptor.target;
    assert.ok(target && descriptor.backendSessionId);
    assert.deepEqual(details.descriptor, descriptor);
    assert.deepEqual(details.descriptor.target, descriptor.target);
    assert.equal(details.descriptor.backendSessionId, descriptor.backendSessionId);
    evidence.remoteSession = { descriptor, details };
    const backendSessionId = descriptor.backendSessionId;
    const ownerMutation = provider.remoteOwnerMutation;
    const ownerAudit = provider.remoteOwnerAudit;
    if (typeof ownerMutation !== 'function' || typeof ownerAudit !== 'function') {
      throw new TransportBarrier('OWNER_AUDIT_CONTROL_REQUIRED',
        'feature-gated daemon QA owner mutation and audit control methods');
    }
    if (!provider.fixture.helper || !provider.helper) throw new TransportBarrier('HELPER_OBSERVER_REQUIRED',
      'separately provisioned helper endpoint/token for read-only output observation');
    const baselineAudit = await ownerAudit.call(provider, backendSessionId);
    assert.equal(baselineAudit.type, 'qaRemoteOwnerAuditOk');
    assert.equal(baselineAudit.details.descriptor.backendSessionId, backendSessionId);
    assert.deepEqual(baselineAudit.details.descriptor.target, target);
    evidence.ownerAudit.push(baselineAudit);
    const encode = (body) => JSON.stringify({ protocol: APPLICATION_PROTOCOL, runId: d.runId, ...body }) + '\n';
    const operations = [{ kind: 'write', id: d.operationId, nonce: d.nonce }];
    if (d.ctrlCOperationId && d.ctrlCNonce) operations.push({ kind: 'write', id: d.ctrlCOperationId, nonce: d.ctrlCNonce });
    const configured = await provider.helper.call('pty.write', { target, text: encode({ type: 'configure', target, operations }) });
    assert.equal(configured.ok, true);
    let ready;
    let partial = '';
    for (let index = 0; index < d.maxReads && !ready; index++) {
      if (ctx.signal?.aborted) throw new TransportBarrier('TRANSPORT_CANCELLED', 'delivery baseline');
      const reply = await provider.helper.call('pty.read', { target, cursor, waitMs: Math.min(ctx.timeoutMs, 10000) });
      assert.equal(reply.ok, true);
      assert.deepEqual(reply.data.target, target);
      assert.equal(reply.data.gap, false);
      for (const chunk of reply.data.chunks) {
        assert.equal(BigInt(chunk.cursor), BigInt(cursor) + 1n);
        cursor = chunk.cursor;
        partial += Buffer.from(chunk.data, 'base64').toString('utf8');
      }
      assert.equal(reply.data.cursor, cursor);
      let end;
      while ((end = partial.indexOf('\n')) !== -1) {
        const line = partial.slice(0, end).replace(/\r$/, '');
        partial = partial.slice(end + 1);
        let record;
        try { record = JSON.parse(line); } catch { continue; }
        if (record.protocol === APPLICATION_PROTOCOL && record.runId === d.runId && record.event === 'ready') {
          assert.deepEqual(record.target, target);
          ready = record;
        }
      }
    }
    if (!ready) throw new TransportBarrier('APPLICATION_READY_REQUIRED', 'actual ready stdout and cursor');
    assert.equal(partial, '');
    evidence.baseline = { ready, cursor };
    const writeParams = { target, text: encode({ type: 'write', target,
      operationId: d.operationId, nonce: d.nonce, payload: d.payload }) };
    const generation = baselineAudit.details.generation;
    if (!faultSocket) throw new TransportBarrier('CANDIDATE_SSH_FAULT_ROUTE_REQUIRED',
      'delivery.faultRoute.upstream {host,port} must target verifier-owned candidate SSH listener');
    const initialDialCount = faultSocket.snapshot().filter((r) => r.kind === 'dial').length;
    const waitOwnerState = provider.remoteOwnerWaitState;
    const waitRefusals = provider.remoteOwnerWaitRefusals;
    if (typeof waitOwnerState !== 'function') throw new TransportBarrier('OWNER_STATE_SUBSCRIPTION_REQUIRED',
      'feature-gated owner RemoteRuntime state subscription');
    if (typeof waitRefusals !== 'function') throw new TransportBarrier('OWNER_DIAL_COUNT_SUBSCRIPTION_REQUIRED',
      'owner retry dial count subscription');
    const disconnected = waitOwnerState.call(provider, backendSessionId, 'disconnected');
    const exactRefusals = waitRefusals.call(provider, backendSessionId, 21);
    const releaseFaultRoute = provider.remoteOwnerReleaseFaultRoute;
    if (typeof releaseFaultRoute !== 'function') throw new TransportBarrier('FAULT_ROUTE_RELEASE_REQUIRED',
      'QA owner authorization to release the 21-refusal SSH route');
    const recoveredDial = subscribe(faultSocket.socket, 'receipt',
      (r) => r.kind === 'dial-forwarded', ctx.timeoutMs, ctx.signal);
    const connected = waitOwnerState.call(provider, backendSessionId, 'connected');
    const lastRefused = subscribe(faultSocket.socket, 'receipt',
      (r) => r.kind === 'dial-refused' && r.remaining === 0, ctx.timeoutMs, ctx.signal);
    faultSocket.socket.interrupt(21);
    let ownerWrite;
    try {
      ownerWrite = await ownerMutation.call(provider, backendSessionId, 'write', generation,
        Buffer.from(writeParams.text));
      const [lastReceipt, ownerRefusalWindow] = await Promise.all([lastRefused.promise, exactRefusals]);
      assert.equal(lastReceipt.remaining, 0);
      assert.equal(ownerRefusalWindow.attempts, 21);
      assert.equal(ownerRefusalWindow.descriptor.backendSessionId, backendSessionId);
      assert.deepEqual(ownerRefusalWindow.descriptor.target, target);
      const refused = faultSocket.snapshot().filter((r) => r.kind === 'dial-refused');
      assert.equal(refused.length, 21);
      assert.equal(refused.filter((r) => r.remaining === 0).length, 1);
      const refusedDials = new Set(refused.map((receipt) => receipt.dial));
      assert.equal(refusedDials.size, 21);
      const disconnectedDetails = await disconnected;
      assert.equal(disconnectedDetails.state, 'disconnected');
      assert.equal(disconnectedDetails.attempts, ownerRefusalWindow.attempts);
      assert.equal(disconnectedDetails.descriptor.backendSessionId, backendSessionId);
      assert.deepEqual(disconnectedDetails.descriptor.target, target);
    } catch (error) {
      lastRefused.cancel();
      throw error;
    }
    evidence.ownerWrite = { operation: 'write', generation, outcome: 'dispatch-returned', result: ownerWrite };
    const ownerAfterUncertain = await ownerAudit.call(provider, backendSessionId);
    evidence.ownerAudit.push(ownerAfterUncertain);
    assert.equal(ownerAfterUncertain.details.descriptor.backendSessionId, backendSessionId);
    assert.deepEqual(ownerAfterUncertain.details.descriptor.target, target);
    assert.equal(ownerAfterUncertain.details.generation, generation);
    const reader = await connectHelper(f.helper.endpoint, f.helper.token, ctx.timeoutMs, ctx.signal, {
      ownerId: `${d.ownerId}:read`, creationBaseline: 1, suppress: () => false,
    });
    connections.push(reader);
    const observer = createTransportDeliveryObserver({ helper: reader, target,
      operation: { kind: 'write', id: d.operationId, nonce: d.nonce, target, cursor,
        before: { writeCount: ready.writeCount, nonceCount: ready.nonceCount, signalCount: ready.signalCount } },
      cursor, producerContract: APPLICATION_PROTOCOL, runId: d.runId,
      decodeRecord: (line) => decodeDeliveryApplicationRecord(line, d.runId), signal: ctx.signal });
    const routeRelease = await releaseFaultRoute.call(provider, backendSessionId);
    assert.equal(routeRelease.type, 'qaRemoteOwnerReleaseFaultRouteOk');
    assert.equal(routeRelease.details.descriptor.backendSessionId, backendSessionId);
    assert.deepEqual(routeRelease.details.descriptor.target, target);
    evidence.recovery.push({ source: 'owner-authorized-fault-route-release', response: routeRelease });
    faultSocket.release();
    const recovery = await ownerMutation.call(provider, backendSessionId, 'recover');
    assert.equal(recovery.type, 'qaRemoteOwnerMutationOk');
    evidence.recovery.push({ source: 'qa-gated-real-RemoteRuntime-retry', response: recovery });
    await recoveredDial.promise;
    const connectedDetails = await connected;
    assert.equal(connectedDetails.descriptor.backendSessionId, backendSessionId);
    assert.deepEqual(connectedDetails.descriptor.target, target);
    for (let index = 0; index < d.maxReads && !observer.snapshot().writeDeliveryObserved; index++)
      await observer.read({ waitMs: Math.min(ctx.timeoutMs, 10000) });
    evidence.application.push(observer.snapshot());
    if (!observer.snapshot().writeDeliveryObserved) throw new TransportBarrier('APPLICATION_CONSUMPTION_REQUIRED', 'actual correlated stdout');
    const applicationRecord = observer.snapshot().observations[0].record;
    assert.equal(applicationRecord.writeCount, ready.writeCount + 1);
    assert.equal(applicationRecord.nonceCount, ready.nonceCount + 1);
    assert.equal(applicationRecord.signalCount, ready.signalCount);
    evidence.networkFault = faultSocket.snapshot();
    const refusedDials = evidence.networkFault.filter((receipt) => receipt.kind === 'dial-refused');
    assert.equal(refusedDials.length, 21);
    assert.equal(refusedDials.filter((receipt) => receipt.remaining === 0).length, 1);
    assert.equal(faultSocket.socket.remaining, 0);
    evidence.physicalDialCount = { baseline: initialDialCount, refused: refusedDials.length,
      totalCandidateDials: evidence.networkFault.filter((receipt) => receipt.kind === 'dial').length };
    for (let index = 0; index < d.recoveryReads; index++) await observer.read({ waitMs: Math.min(ctx.timeoutMs, 10000) });
    const recoveredAudit = await ownerAudit.call(provider, backendSessionId);
    evidence.ownerAudit.push(recoveredAudit);
    assert.equal(recoveredAudit.details.descriptor.backendSessionId, backendSessionId);
    assert.deepEqual(recoveredAudit.details.descriptor.target, target);
    assert.equal(recoveredAudit.details.generation, generation + 1);
    if (d.ctrlCOperationId && d.ctrlCNonce) {
      const ctrlCGeneration = recoveredAudit.details.generation;
      const ctrlCText = encode({ type: 'write', target, operationId: d.ctrlCOperationId,
        nonce: d.ctrlCNonce, payload: '\u0003' });
      const ctrlCWrite = await ownerMutation.call(provider, backendSessionId, 'write', ctrlCGeneration, Buffer.from(ctrlCText));
      evidence.sshCtrlC = { operationId: d.ctrlCOperationId, payloadHex: '03', ownerQueue: 'RemoteRuntime::write', result: ctrlCWrite };
      for (let index = 0; index < d.maxReads && !observer.snapshot().observations.some((item) => item.operationId === d.ctrlCOperationId); index++)
        await observer.read({ waitMs: Math.min(ctx.timeoutMs, 10000) });
      evidence.application.push(observer.snapshot());
    }
    evidence.application.push(observer.snapshot());
    evidence.helperAudits.push(reader.snapshot());
    const calls = evidence.helperAudits.flatMap((a) => a.audit.filter((r) => r.kind === 'request-written'));
    assert.equal(calls.filter((r) => r.op === 'pty.write').length, 0);
    assert.equal(calls.filter((r) => r.op === 'pty.spawn').length, 0);
    const ownerEvents = recoveredAudit.events;
    assert.equal(ownerEvents.some((event) => event.operation === 'create' && event.outcome === 'registered'), true);
    assert.equal(ownerEvents.filter((event) => event.operation === 'write' && event.outcome === 'dispatched').length,
      d.ctrlCOperationId && d.ctrlCNonce ? 2 : 1);
    assert.equal(ownerEvents.filter((event) => event.operation === 'recovery' && event.outcome === 'rearmed').length, 1);
    assert.equal(ownerEvents.every((event) => event.backendSessionId === backendSessionId && JSON.stringify(event.target) === JSON.stringify(target)), true);
    evidence.mapping = { backendSessionId: descriptor.backendSessionId,
      descriptorTarget: descriptor.target, detailsTarget: details.descriptor.target,
      exactMatch: JSON.stringify(descriptor.target) === JSON.stringify(details.descriptor.target) &&
        details.descriptor.backendSessionId === descriptor.backendSessionId };
    assert.equal(evidence.mapping.exactMatch, true);
    assert.equal(evidence.ownerWrite.outcome, 'dispatch-returned');
    evidence.window = { scope: 'daemon-owned-create-write-retry-and-helper-output-read',
      ownerSessionId: backendSessionId, target, events: ownerEvents,
      recoveryObserved: true, uncertainWriteNotReplayed: true,
      ownerMutationAuditComplete: true,
      closeNotYetObserved: true, nativePresentationObserved: false };
    return { scenario: 'ssh-ipc-failure', status: 'unverified', timeline: [],
      barrierCode: 'SSH_SURFACE_EVIDENCE_REQUIRED',
      required: [...requirements, 'owner-close'],
      detail: 'Transport delivery observed; native presentation, owner close, and outstanding scenario requirements remain unverified.',
      observations: [{ check: 'daemon-created-descriptor-details-target-match',
        source: 'qa-gated-daemon-create-rpc', evidence: evidence.mapping }],
      requirements, evidence, assertions: [] };
  } catch (error) {
    return { scenario: 'ssh-ipc-failure', status: error instanceof TransportBarrier ? 'unverified' : 'failed',
      barrierCode: error.code, detail: error.message, required: error.required, evidence, requirements, assertions: [] };
  } finally { for (const connection of connections) connection.close(); }
}

export const runSshIpcFailureScenario = (ctx) => execute(ctx, 'ssh-ipc-failure');
export const runPairedRelayLossScenario = (ctx) => execute(ctx, 'paired-relay-loss');
export const runPairedBackendUnavailableScenario = (ctx) => execute(ctx, 'paired-backend-unavailable');
export const runHostDownDescriptorRestoreScenario = (ctx) => execute(ctx, 'host-down-descriptor-restore');
export const transportScenarioExecutors = Object.freeze({
  ...Object.fromEntries(PAIRED_SCENARIOS.map((s) => [s, (ctx) => execute(ctx, s)])),
  'ssh-ipc-failure': runSshIpcFailureScenario,
  'paired-relay-loss': runPairedRelayLossScenario,
  'paired-backend-unavailable': runPairedBackendUnavailableScenario,
  'host-down-descriptor-restore': runHostDownDescriptorRestoreScenario,
});

export { PAIRED_SCENARIOS, runPairedScenario };
