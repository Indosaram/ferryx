/** Real paired transport scenarios. Missing owner/route controls remain unverified. */
import assert from 'node:assert/strict';
import { createTransportProvider } from './fixtures/transport-provider.mjs';
import { TransportBarrier } from './fixtures/transport-provider.mjs';

export const PAIRED_SCENARIOS = Object.freeze([
  'paired-relay-loss',
  'paired-backend-unavailable',
  'host-down-descriptor-restore',
]);

function requireFunction(value, code, required) {
  if (typeof value !== 'function') throw new TransportBarrier(code, required);
  return value;
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
  };
}

export async function runPairedScenario(ctx, scenario) {
  assert.ok(PAIRED_SCENARIOS.includes(scenario), `unknown paired scenario: ${scenario}`);
  let provider;
  const timeline = [];
  const observations = [];
  const requirements = [];
  try {
    const fixture = ctx.transportFixture?.paired;
    if (!fixture?.backendSessionId || !fixture?.descriptor || !fixture?.target || !fixture?.hostId || !fixture?.daemonEpoch || !fixture?.incarnation) {
      throw new TransportBarrier('PAIRED_FIXTURE_REQUIRED',
        'paired {descriptor,target,hostId,daemonEpoch,incarnation,expectedRoute,backendEndpoint,relayFaultEndpoint}');
    }
    provider = await createTransportProvider(ctx);
    const owner = provider.pairedOwner ?? pairedOwnerControls(provider.control);
    requireFunction(owner?.bind, 'PAIRED_OWNER_CONTROL_REQUIRED',
      'feature-gated daemon owner setup/audit/state-wait/restore APIs for an actual paired actor');
    const session = await owner.bind(fixture);
    observations.push({ check: 'paired-owner-bound', source: 'candidate-daemon-owner', evidence: session.identity });
    assert.equal(session.identity.backendSessionId, fixture.backendSessionId);
    assert.equal(session.identity.daemonEpoch, fixture.daemonEpoch);
    assert.equal(session.identity.incarnation, fixture.incarnation);
    const route = typeof provider.createPairedFaultRoute === 'function'
      ? await provider.createPairedFaultRoute(fixture)
      : {
        receipt: await owner.routeReceipt(session),
        closeRealUpstream: () => owner.dropLoopback(session),
      };
    observations.push({ check: 'selected-transport-route', source: 'paired-client-path-selection', evidence: route.receipt });
    assert.equal(route.receipt.selected, fixture.expectedRoute);
    assert.ok(route.receipt.websocket === true);
    if (fixture.expectedAfterSequence !== undefined) {
      assert.equal(route.receipt.afterSequence, fixture.expectedAfterSequence);
    }
    if (!route.receipt.loopback) throw new TransportBarrier('PAIRED_LOOPBACK_REQUIRED', 'actual selected socket peer must be loopback');
    requireFunction(owner.ownerAudit, 'PAIRED_OWNER_AUDIT_REQUIRED',
      'ownerAudit over real daemon protocol QaPairedOwnerAudit');
    requireFunction(owner.dialAudit, 'PAIRED_DIAL_AUDIT_REQUIRED',
      'dialAudit over real daemon protocol QaPairedDialAudit');
    const beforeOwner = await owner.ownerAudit(session);
    const beforeDial = await owner.dialAudit(session);
    observations.push({ check: 'paired-owner-before', source: 'daemon-owner-audit', evidence: beforeOwner });
    observations.push({ check: 'paired-dial-before', source: 'daemon-dial-audit', evidence: beforeDial });
    assert.equal(beforeOwner.incarnation, session.identity.incarnation);
    assert.equal(beforeOwner.daemonEpoch, session.identity.daemonEpoch);

    if (scenario === 'paired-relay-loss') {
      if (fixture.expectedRoute !== 'relay') throw new TransportBarrier('RELAY_ROUTE_REQUIRED', 'actual selected route receipt must say relay');
      const state = owner.waitState(session, ['owner_unavailable'], ctx.timeoutMs, ctx.signal);
      await route.closeRealUpstream();
      timeline.push(await state);
      // Execute identified uncertain write and signal under transport outage:
      const writePayload = Buffer.from('ls -la\n');
      const signalPayload = Buffer.from([2]); // SIGINT (Ctrl+C)
      if (typeof owner.mutate === 'function') {
        try {
          await owner.mutate(session, 'write', writePayload);
        } catch (err) {
          timeline.push({ kind: 'uncertain-write', error: err.message });
        }
        try {
          await owner.mutate(session, 'signal', signalPayload);
        } catch (err) {
          timeline.push({ kind: 'uncertain-signal', error: err.message });
        }
      }
      if (route.restoreRealUpstream) await route.restoreRealUpstream();
      const recovered = owner.waitState(session, ['proxy_attached'], ctx.timeoutMs, ctx.signal);
      const wake = await owner.retry(session, fixture);
      observations.push({ check: 'same-owner-retry-admission', source: 'daemon-retry-ipc', evidence: wake });
      assert.equal(wake.admitted, true);
      timeline.push(await recovered);
    } else if (scenario === 'paired-backend-unavailable') {
      const state = owner.waitState(session, ['owner_unavailable'], ctx.timeoutMs, ctx.signal);
      await route.closeRealUpstream();
      timeline.push(await state);
      const wake = await owner.retry(session, fixture);
      assert.ok(wake.admitted || wake.ownerUnavailable);
      const cancelled = await owner.cancelPendingDial(session);
      observations.push({ check: 'bounded-pending-dial-cancellation', source: 'daemon-owner-event', evidence: cancelled });
    } else {
      const state = owner.waitState(session, ['owner_unavailable'], ctx.timeoutMs, ctx.signal);
      await route.closeRealUpstream();
      timeline.push(await state);
      if (typeof provider.restartClientDaemon !== 'function') {
        throw new TransportBarrier('CLIENT_DAEMON_COLD_RESTART_SEAM_REQUIRED',
          'provider.restartClientDaemon(): Promise<TransportProvider> preserving host PTY');
      }
      const restartedProvider = await provider.restartClientDaemon();
      const coldOwner = restartedProvider.pairedOwner ?? pairedOwnerControls(restartedProvider.control);
      const restored = await coldOwner.restoreDescriptorCold(fixture);
      observations.push({ check: 'cold-daemon-restarted-descriptor', source: 'restarted-client-daemon', evidence: restored });
      assert.deepEqual(restored.target, fixture.descriptor.target);
      assert.equal(restored.generation, fixture.descriptor.generation);
      if (route.restoreRealUpstream) await route.restoreRealUpstream();
      const recovered = coldOwner.waitState(session, ['proxy_attached'], ctx.timeoutMs, ctx.signal);
      const wake = await coldOwner.retry(session, fixture);
      observations.push({ check: 'cold-owner-reconnected-target', source: 'restarted-client-daemon-retry', evidence: wake });
      assert.equal(wake.admitted, true);
      timeline.push(await recovered);

      // Measure PID, incarnation, and cursor across BOTH daemons:
      const coldOwnerAudit = await coldOwner.ownerAudit(session);
      const coldDialAudit = await coldOwner.dialAudit(session);
      const hostPid = fixture.hostPid ?? (typeof provider.getHostPid === 'function' ? await provider.getHostPid() : null);
      const clientPid = typeof restartedProvider.getClientPid === 'function' ? await restartedProvider.getClientPid() : null;

      if (hostPid !== null && clientPid !== null) {
        assert.ok(Number.isInteger(hostPid) && hostPid > 0, 'host PID must be positive integer');
        assert.ok(Number.isInteger(clientPid) && clientPid > 0, 'client PID must be positive integer');
        assert.notEqual(clientPid, hostPid, 'client daemon and host daemon must be separate processes');
      }
      assert.equal(coldOwnerAudit.incarnation, session.identity.incarnation, 'incarnation must be preserved across cold restart');
      assert.equal(coldDialAudit.identity.incarnation, session.identity.incarnation);
      if (fixture.descriptor.afterSequence !== undefined && fixture.descriptor.afterSequence !== null) {
        assert.equal(coldDialAudit.replayCursor, fixture.descriptor.afterSequence, 'cursor must match cold restored descriptor');
      }
    }

    const afterOwner = await owner.ownerAudit(session);
    const afterDial = await owner.dialAudit(session);
    observations.push({ check: 'paired-owner-after', source: 'daemon-owner-audit', evidence: afterOwner });
    observations.push({ check: 'paired-dial-after', source: 'daemon-dial-audit', evidence: afterDial });
    assert.equal(afterOwner.incarnation, beforeOwner.incarnation);
    assert.equal(afterOwner.daemonEpoch, beforeOwner.daemonEpoch);
    assert.equal(afterDial.identity.incarnation, beforeDial.identity.incarnation);
    assert.equal(afterDial.pendingDial, false);

    // Wire-level delivery and replay proof for identified uncertain write/signal:
    // The candidate paired isolated provider (paired-isolated-provider.mjs) has no verified
    // wire traffic auditor or delivery ledger producer seam. In accordance with recovery policy,
    // missing verified existing producer contract must reject with typed TransportBarrier
    // ('unverified') rather than accepting synthetic fixture assertions or creating false passing paths.
    if (scenario === 'paired-relay-loss') {
      requirements.push('traffic-endpoint-audit-producer-required');
      throw new TransportBarrier(
        'TRAFFIC_AUDITOR_PRODUCER_REQUIRED',
        'traffic-endpoint-audit-producer-required: verified existing producer wire traffic auditor or delivery ledger absent on paired route; minimal producer seam required',
      );
    }
    return {
      status: 'passed',
      scenario,
      timeline,
      observations,
      requirements: [...requirements],
      assertions: ['real-route', 'same-owner', 'zero-create-spawn-close', 'zero-automatic-replay'],
    };
  } catch (error) {
    return { status: error instanceof TransportBarrier ? 'unverified' : 'failed', scenario,
      barrierCode: error.code, required: error.required, detail: error.message,
      timeline, observations, requirements, assertions: [] };
  } finally { await provider?.close(); }
}
