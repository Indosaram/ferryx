import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import {
  WEB_OUTCOME_SCENARIOS,
  executeWebOutcome,
  outcomeExpression,
  armDOM,
  hasLegacyPresentationProof,
  runConfirmedExitScenario,
  runAuthRevocationScenario,
  runCloseRebindPendingDialScenario,
  runOutputReplayGapScenario,
  runNoFocusStealScenario,
  runColdStartUnknownScenario,
} from './session-recovery-web-outcomes.mjs';

function createMockGateway(target, rebindTarget) {
  const emitter = new EventEmitter();
  const rawRequests = [];
  let deliveries = [];
  const selectionRequests = [];
  const current = {
    sessionId: target.sessionId,
    daemonEpoch: target.daemonEpoch,
    incarnation: null,
    socketId: 'sock-01',
    generation: 8,
    presentationSequence: 1,
    outcome: 'ready',
    fullGridReceipt: { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch,
      socketId: 'sock-01', receiptId: 'grid-01' },
  };

  return {
    on: emitter.on.bind(emitter),
    off: emitter.off.bind(emitter),
    emit: emitter.emit.bind(emitter),
    currentTarget: async () => current,
    validatedBinding: async () => current.sessionId === rebindTarget.sessionId ? rebindTarget : target,
    inject: async (command, { target: cmdTarget, tail } = {}) => {
      switch (command) {
        case 'ready':
          emitter.emit('presentation-sent', { target: cmdTarget, sequence: 1 });
          break;
        case 'confirmed-exit':
          emitter.emit('exit-sent', { target: cmdTarget, exitCode: 0, sequence: 2 });
          break;
        case 'revoke-token':
          emitter.emit('auth-rejected', { target: cmdTarget, sequence: 2 });
          break;
        case 'correct-token':
          emitter.emit('presentation-sent', { target: cmdTarget, sequence: 3 });
          break;
        case 'hold-dial':
          emitter.emit('dial-held', { target: cmdTarget, attemptId: 'att-1', sequence: 1 });
          break;
        case 'rebind':
          Object.assign(current, { sessionId: cmdTarget.sessionId, daemonEpoch: cmdTarget.daemonEpoch,
            socketId: 'sock-rebound', presentationSequence: 2,
            fullGridReceipt: { sessionId: cmdTarget.sessionId, daemonEpoch: cmdTarget.daemonEpoch,
              socketId: 'sock-rebound', receiptId: 'grid-rebound' } });
          emitter.emit('presentation-sent', { target: cmdTarget, sequence: 2 });
          emitter.emit('dial-cancelled', { target, attemptId: 'att-1', sequence: 2 });
          break;
        case 'release-stale-dial':
          break;
        case 'overflow-output-ring':
          emitter.emit('replay-gap-sent', {
            target: cmdTarget,
            requestedCursor: 0,
            oldestAvailableCursor: 512 * 1024 + 10,
            bytesWritten: 512 * 1024 + 11,
            capacity: 512 * 1024,
            sequence: 2,
          });
          break;
        case 'disconnect':
          emitter.emit('transport-closed', { target: cmdTarget, code: 1012, sequence: 2 });
          break;
        default:
          throw new Error('UNKNOWN_MOCK_COMMAND: ' + command);
      }
    },
    recordDelivery: (record) => {
      deliveries.push(record);
    },
    clearDeliveries: () => { deliveries = []; },
    recordRequest: (record) => {
      rawRequests.push(record);
    },
    clearRequests: () => { rawRequests.length = 0; },
    fence: async () => ({
      complete: true,
      fenceId: 'fence-123',
      source: 'mock-gateway',
      requests: rawRequests,
      deliveries: [...deliveries],
      selectionRequests,
      owner: {
        pendingTimers: 0,
        inFlight: false,
        exitSettlements: 1,
        cancelledAttemptIds: ['old-att'],
        staleCompletionsRejected: 1,
        identityConfirmed: false,
        retainedTarget: { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch },
      },
    }),
  };
}

function createMockHarness(scenario, { targetOverrides } = {}) {
  const target = {
    sessionId: 'session-qa-1',
    daemonEpoch: 'epoch-qa-1',
    incarnation: 'inc-qa-1',
    pid: 4321,
    ...targetOverrides,
  };
  const rebindTarget = {
    sessionId: 'session-qa-2',
    daemonEpoch: 'epoch-qa-1',
    incarnation: 'inc-qa-2',
    pid: 4322,
  };

  return {
    openWebOutcome: async (options) => {
      const gateway = createMockGateway(target, rebindTarget);
      let state = 'initial';
      let oldOwnerDisposed = false;
      let armedCompletionKey = null;
      let armedAttemptId = null;
      let releasedSocketKey = null;
      let staleCompletionRejectedCount = 0;
      let currentLiveOwner = {
        ...target,
        socketId: 'sock-new',
        presentationSequence: 1,
      };

      const browser = {
        evaluate: async (code) => {
          if (typeof code === 'string') {
            if (code.includes('__task9Outcomes.set')) {
              return undefined;
            }
            if (code.includes('__task9Outcomes.get')) {
              // DOM evaluate read
              if (code.includes('.promise')) {
                if (state === 'initial') {
                  state = 'chat-checked';
                  return { chatDefault: true };
                }
                if (scenario === 'cold-start-unknown') {
                  return {
                    outcome: oldOwnerDisposed ? 'ready' : 'blocked',
                    reason: 'Recovery outcome unknown',
                    sessionId: oldOwnerDisposed ? rebindTarget.sessionId : target.sessionId,
                    daemonEpoch: oldOwnerDisposed ? rebindTarget.daemonEpoch : target.daemonEpoch,
                    incarnation: null,
                    activity: 'active',
                    text: 'Recovery outcome unknown',
                  };
                }
                if (state === 'chat-checked' || state === 'ready-requested') {
                  state = 'ready';
                  return {
                    outcome: 'ready',
                    reason: 'live',
                    sessionId: target.sessionId,
                    daemonEpoch: target.daemonEpoch,
                    incarnation: null,
                    activity: 'active',
                    text: 'Live',
                  };
                }
                if (scenario === 'confirmed-exit') {
                  return {
                    outcome: 'exited',
                    reason: 'Terminal exited (0)',
                    sessionId: target.sessionId,
                    daemonEpoch: target.daemonEpoch,
                    incarnation: null,
                    activity: 'settled',
                    text: 'Terminal exited (0)',
                  };
                }
                if (scenario === 'auth-revocation') {
                  if (state === 'revoking') {
                    state = 'revoked';
                    return {
                      outcome: 'blocked',
                      reason: 'Terminal blocked',
                      sessionId: target.sessionId,
                      daemonEpoch: target.daemonEpoch,
                      incarnation: null,
                      activity: 'active',
                      text: 'Terminal blocked',
                    };
                  }
                  return {
                    outcome: 'ready',
                    reason: 'live',
                    sessionId: target.sessionId,
                    daemonEpoch: target.daemonEpoch,
                    incarnation: null,
                    activity: 'active',
                    text: 'Live',
                  };
                }
                if (scenario === 'close-rebind-pending-dial') {
                  if (options.variant === 'close') {
                    return { ownerRemoved: true };
                  }
                  return {
                    outcome: 'ready',
                    reason: 'live',
                    sessionId: rebindTarget.sessionId,
                    daemonEpoch: rebindTarget.daemonEpoch,
                    incarnation: null,
                    activity: 'active',
                    text: 'Live',
                  };
                }
                if (scenario === 'output-replay-gap') {
                  return {
                    text: 'output gap text',
                    grid: 'task9-gap-tail',
                  };
                }
                if (scenario === 'no-focus-steal') {
                  if (state === 'disconnecting') {
                    state = 'disconnected';
                    return {
                      outcome: 'retryable',
                      reason: 'Connecting',
                      sessionId: target.sessionId,
                      daemonEpoch: target.daemonEpoch,
                      incarnation: null,
                      activity: 'active',
                      text: 'Connecting',
                    };
                  }
                  return {
                    outcome: 'ready',
                    reason: 'live',
                    sessionId: target.sessionId,
                    daemonEpoch: target.daemonEpoch,
                    incarnation: null,
                    activity: 'active',
                    text: 'Live',
                  };
                }
              }
              return undefined;
            }
            if (code.includes('TASK9_ACTION_UNAVAILABLE')) {
              return undefined; // click
            }
            if (code.includes('requestAnimationFrame')) {
              return true;
            }
            if (code.includes('window.__task9Focus')) {
              return { retained: true, stolen: [] };
            }
          }
          return true;
        },
        screenshot: async () => Buffer.from('mock-png'),
      };

      const run = {
        browser,
        gateway,
        target,
        rebindTarget,
        timeoutMs: 5000,
        focusSelector: '[data-testid="remote-terminal-grid"]',
        identityEvidence: 'mock-identity',
        saveScreenshot: async (name) => '/tmp/' + name,
        input: async (intent) => {
          gateway.emit('delivery-' + intent.kind, {
            payload: intent.payload,
            requestId: 'req-1',
            socketId: 'sock-01',
            kind: intent.kind,
            target,
            ...intent.delivery,
          });
          gateway.recordDelivery({
            payload: intent.payload,
            requestId: 'req-1',
            socketId: 'sock-01',
            kind: intent.kind,
            target,
          });
        },
        correctToken: async () => {
          state = 'correcting';
          await gateway.inject('correct-token', { target });
        },
        rebindOwner: async () => {
          await gateway.inject('rebind', { target: rebindTarget });
        },
        closeOwner: async () => {
          gateway.emit('dial-cancelled', { target, attemptId: 'att-1', sequence: 2 });
        },
        snapshotOwner: async (snapTarget) => ({
          identityConfirmed: false,
          generation: 1,
          cancelledAttemptIds: oldOwnerDisposed ? ['1'] : [],
          disposed: oldOwnerDisposed,
          pendingTimers: 0,
          inFlight: !oldOwnerDisposed,
          retainedTarget: { sessionId: snapTarget.sessionId, daemonEpoch: snapTarget.daemonEpoch },
        }),
        armTicketCompletion: async (armTarget, attemptId, waitMs) => {
          assert.equal(oldOwnerDisposed, false);
          assert.equal(attemptId, '1');
          armedCompletionKey = `${armTarget.sessionId}:${armTarget.daemonEpoch}:${attemptId}`;
          armedAttemptId = attemptId;
          return armedCompletionKey;
        },
        waitTicketHeld: async (key, waitMs) => {
          if (!armedCompletionKey || key !== armedCompletionKey) throw new Error('TASK9_SOCKET_COMPLETION_NOT_ARMED');
          return { status: 200, requestId: 'ticket-1', target, delivered: false };
        },
        releaseTicketCompletion: async (key) => {
          if (!armedCompletionKey || key !== armedCompletionKey) throw new Error('TASK9_SOCKET_COMPLETION_NOT_ARMED');
          if (!oldOwnerDisposed || releasedSocketKey === key) throw new Error('TASK9_COLDSTART_RELEASE_INVALID');
          releasedSocketKey = key;
          staleCompletionRejectedCount++;
          return { status: 200, requestId: 'ticket-1', target };
        },
        readRetiredObserver: async (key) => ({
          disposed: oldOwnerDisposed,
          pendingTimers: 0,
          inFlight: false,
          staleCompletionsRejected: staleCompletionRejectedCount,
          cancelledAttemptIds: armedAttemptId ? [armedAttemptId] : ['old-att'],
          rejectedAttemptIds: releasedSocketKey === key && armedAttemptId ? [armedAttemptId] : [],
        }),
        currentOwner: async () => currentLiveOwner,
        newTerminalOwner: async () => {
          oldOwnerDisposed = true;
          await gateway.inject('rebind', { target: rebindTarget });
          currentLiveOwner = {
            ...rebindTarget,
            socketId: 'sock-new',
            presentationSequence: 1,
          };
          gateway.recordRequest({
            type: 'createSession',
            requestId: 'create-req-1',
            target: rebindTarget,
          });
          gateway.emit('create-request', {
            target,
            requestId: 'create-req-1',
            browserActionId: 'action-1',
            browserActionTestId: 'btn-new-terminal',
          });
          return 'btn-new-terminal';
        },
        dispose: async () => {},
      };

      return run;
    },
  };
}

test('legacy browser proof requires the exact full-grid socket without fixture lifetime fields', () => {
  const target = { sessionId: 's', daemonEpoch: 'e', incarnation: 'fixture-only', pid: 7 };
  const proof = { sessionId: 's', daemonEpoch: 'e', incarnation: null,
    socketId: 'socket', generation: 1, presentationSequence: 1, outcome: 'ready',
    fullGridReceipt: { sessionId: 's', daemonEpoch: 'e', socketId: 'socket', receiptId: 'grid' } };
  assert.equal(hasLegacyPresentationProof(proof, target), true);
  for (const change of [{ socketId: 'foreign' }, { daemonEpoch: 'wrong' },
    { outcome: 'retryable' }, { presentationSequence: 0 }, { generation: 0 }]) {
    assert.equal(hasLegacyPresentationProof({ ...proof, ...change }, target), false, JSON.stringify(change));
  }
  assert.equal(hasLegacyPresentationProof({ ...proof,
    fullGridReceipt: { ...proof.fullGridReceipt, daemonEpoch: 'wrong' } }, target), false);
});

test('web outcome scenarios roster contains exactly the six authorized recovery scenarios', () => {
  assert.deepEqual(WEB_OUTCOME_SCENARIOS, [
    'confirmed-exit',
    'auth-revocation',
    'close-rebind-pending-dial',
    'output-replay-gap',
    'no-focus-steal',
    'cold-start-unknown',
  ]);
});

test('owner notification wakes a retryable predicate without any DOM mutation', async () => {
  const window = new EventTarget();
  let owner = { retainedTarget: { sessionId: 's', daemonEpoch: 'e' }, identityConfirmed: false,
    disposed: false, inFlight: false, pendingTimers: 1, outcome: null };
  window.__ferryxRecoveryQA = { snapshot: () => owner };
  let disconnected = 0;
  class MutationObserver {
    observe() {}
    disconnect() { disconnected++; }
  }
  const status = { textContent: 'Connecting', getClientRects: () => [{}] };
  const document = { documentElement: {}, querySelectorAll: () => [status] };
  const browser = { evaluate: code => new Function('window', 'document', 'MutationObserver',
    `return (${code});`)(window, document, MutationObserver) };
  const subscription = await armDOM(browser, outcomeExpression(owner.retainedTarget, ['retryable']), 5000);
  try {
    // No DOM observer callback is invoked: only owner state changes.
    owner = { ...owner, inFlight: true, outcome: 'retryable', generation: 4 };
    window.dispatchEvent(new Event('ferryx-web-owner-observation'));
    const rendered = await subscription.read();
    assert.equal(rendered.outcome, 'retryable');
    assert.equal(rendered.generation, 4);
    assert.equal(disconnected, 1);
  } finally { await subscription.dispose(); }
});

test('outcomeExpression returns null while waiting for the recovery observer to mount', () => {
  const expr = outcomeExpression({ sessionId: 's1', daemonEpoch: 'e1' }, ['ready']);
  const fn = new Function('window', 'document', `return (${expr})()`);
  assert.equal(fn({}, {}), null);
});

test('outcomeExpression enforces exact sessionId and daemonEpoch match on snapshot query', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['ready']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  // Target epoch mismatch must return null
  const mockQA = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'wrong-epoch' },
    }),
  };
  assert.equal(fn({ __ferryxRecoveryQA: mockQA }, {}), null);
});

test('outcomeExpression matches ready when snapshot is live, grid mounted, and status is Live', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['ready']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  const mockQA = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      outcome: 'ready',
      socketId: 'sock-1',
      generation: 1,
      presentationSequence: 2,
    }),
  };
  const mockDoc = {
    querySelector: (sel) => {
      if (sel === '[data-testid="remote-terminal-connection-status"]') {
        return { textContent: 'Live' };
      }
      if (sel === '[data-testid="remote-terminal-grid"]') {
        return { getClientRects: () => [{ width: 100 }] };
      }
      return null;
    },
  };

  const res = fn({ __ferryxRecoveryQA: mockQA }, mockDoc);
  assert.equal(res.outcome, 'ready');
  assert.equal(res.sessionId, 's1');
  assert.equal(res.daemonEpoch, 'e1');
  assert.equal(res.activity, 'active');
});

test('outcomeExpression matches affirmative exit and requires idle activity for inferred settlement', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['exited']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  // Missing settlement evidence must remain unproven.
  const mockQANull = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      inFlight: false,
      pendingTimers: 0,
      outcome: 'affirmative_exit',
      exitSettlements: null,
    }),
  };
  const mockDocExited = {
    querySelector: (sel) => {
      if (sel === '[data-testid="remote-terminal-outcome"]') {
        return { textContent: 'Terminal exited (0) Retry', getClientRects: () => [{ width: 100 }] };
      }
      return null;
    },
  };
  const unproven = fn({ __ferryxRecoveryQA: mockQANull }, mockDocExited);
  assert.equal(unproven.outcome, 'affirmative_exit');
  assert.equal(unproven.activity, null);

  // A settlement count proves settlement only when no timer or dial remains active.
  const mockQAProven = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      inFlight: false,
      pendingTimers: 0,
      outcome: 'affirmative_exit',
      exitSettlements: 1,
    }),
  };
  const proven = fn({ __ferryxRecoveryQA: mockQAProven }, mockDocExited);
  assert.equal(proven.outcome, 'affirmative_exit');
  assert.equal(proven.activity, 'settled');

  for (const activity of [{ pendingTimers: 1 }, { inFlight: true }]) {
    const active = fn({ __ferryxRecoveryQA: {
      snapshot: () => ({ ...mockQAProven.snapshot(), activity: 'settled', ...activity }),
    } }, mockDocExited);
    assert.equal(active.outcome, 'affirmative_exit');
    assert.equal(active.activity, 'unsettled');
  }

  // Rendered exit text alone cannot establish an authoritative exit.
  const unconfirmed = fn({ __ferryxRecoveryQA: {
    snapshot: () => ({ ...mockQAProven.snapshot(), outcome: 'retryable' }),
  } }, mockDocExited);
  assert.equal(unconfirmed, null);
});

test('blocked-only observation rejects an affirmative exit', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const fn = new Function('window', 'document', `return (${outcomeExpression(target, ['blocked'])})()`);
  const result = fn({ __ferryxRecoveryQA: { snapshot: () => ({
    retainedTarget: target, identityConfirmed: true, disposed: false,
    outcome: 'affirmative_exit', exitSettlements: 1,
  }) } }, { querySelector: selector => selector === '[data-testid="remote-terminal-outcome"]'
    ? { textContent: 'Terminal exited (0) Retry', getClientRects: () => [{}] } : null });
  assert.equal(result, null);
});

test('outcomeExpression matches all blockedReason variants including identity_conflict, auth_revoked, permission_denied', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['blocked']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  const mockQA = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      outcome: 'blocked',
    }),
  };

  for (const reason of ['identity_conflict', 'auth_revoked', 'protocol_error', 'permission_denied', 'Terminal blocked']) {
    const mockDoc = {
      querySelector: (sel) => {
        if (sel === '[data-testid="remote-terminal-outcome"]') {
          return { textContent: `${reason} Retry`, getClientRects: () => [{ width: 100 }] };
        }
        return null;
      },
    };
    const res = fn({ __ferryxRecoveryQA: mockQA }, mockDoc);
    assert.equal(res.outcome, 'blocked');
    assert.equal(res.reason, reason);
    assert.equal(res.activity, 'active');
  }

  // Also verify unknown generation outcome
  const mockDocUnknown = {
    querySelector: (sel) => {
      if (sel === '[data-testid="remote-terminal-outcome-unknown"]') {
        return { textContent: 'Recovery outcome unknown Retry', getClientRects: () => [{ width: 100 }] };
      }
      return null;
    },
  };
  const resUnknown = fn({ __ferryxRecoveryQA: mockQA }, mockDocUnknown);
  assert.equal(resUnknown.outcome, 'blocked');
  assert.equal(resUnknown.reason, 'Recovery outcome unknown');
});

test('outcomeExpression matches retryable when connecting status is rendered and inFlight or pendingTimers active', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['retryable']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  const mockQA = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      inFlight: true,
      pendingTimers: 1,
      disposed: false,
      outcome: 'retryable',
    }),
  };
  const mockDoc = {
    querySelectorAll: (sel) => {
      if (sel === '[role="status"]') {
        return [{ textContent: 'Connecting', getClientRects: () => [{ width: 50 }] }];
      }
      return [];
    },
  };

  const res = fn({ __ferryxRecoveryQA: mockQA }, mockDoc);
  assert.equal(res.outcome, 'retryable');
  assert.equal(res.reason, 'Connecting');
});

test('executeWebOutcome throws TASK9_UNKNOWN_SCENARIO on invalid scenario', async () => {
  await assert.rejects(
    executeWebOutcome('non-existent-scenario', {}),
    err => err.code === 'TASK9_UNKNOWN_SCENARIO'
  );
});

test('executeWebOutcome throws TASK9_GATEWAY_ADAPTER_REQUIRED when openWebOutcome is missing', async () => {
  await assert.rejects(
    executeWebOutcome('confirmed-exit', {}),
    err => err.code === 'TASK9_GATEWAY_ADAPTER_REQUIRED'
  );
});

test('executeWebOutcome throws TASK9_FIXTURE_IDENTITY_REQUIRED on missing or invalid timeoutMs', async () => {
  const harness = {
    openWebOutcome: async () => ({
      timeoutMs: -1,
      target: { sessionId: 's1', daemonEpoch: 'e1', incarnation: 'i1', pid: 100 },
      browser: { evaluate: async () => {} },
      dispose: async () => {},
    }),
  };
  await assert.rejects(
    executeWebOutcome('confirmed-exit', harness),
    err => err.code === 'TASK9_FIXTURE_IDENTITY_REQUIRED'
  );
});

test('runConfirmedExitScenario executes confirmed-exit across viewports and themes with deterministic receipts', async () => {
  const harness = createMockHarness('confirmed-exit');
  const result = await runConfirmedExitScenario(harness);
  assert.equal(result.scenario, 'confirmed-exit');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 4); // 2 viewports (desktop, mobile) x 2 themes (light, dark)
  for (const item of result.evidence) {
    assert.equal(item.scenario, 'confirmed-exit');
    assert.ok(item.ledger.complete);
    assert.ok(item.captures.length > 0);
  }
});

test('runAuthRevocationScenario executes auth revocation and token correction cycle', async () => {
  const harness = createMockHarness('auth-revocation');
  const result = await runAuthRevocationScenario(harness);
  assert.equal(result.scenario, 'auth-revocation');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 4);
});

test('runCloseRebindPendingDialScenario executes close and rebind variants', async () => {
  const harness = createMockHarness('close-rebind-pending-dial');
  const result = await runCloseRebindPendingDialScenario(harness);
  assert.equal(result.scenario, 'close-rebind-pending-dial');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 8); // 2 viewports x 2 themes x 2 variants (close, rebind)
});

test('runNoFocusStealScenario verifies active element retention without focus steal', async () => {
  const harness = createMockHarness('no-focus-steal');
  const result = await runNoFocusStealScenario(harness);
  assert.equal(result.scenario, 'no-focus-steal');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 4);
});

test('runColdStartUnknownScenario verifies historical owner unavailable and explicit creation', async () => {
  const harness = createMockHarness('cold-start-unknown');
  const result = await runColdStartUnknownScenario(harness);
  assert.equal(result.scenario, 'cold-start-unknown');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 4);
});

test('runOutputReplayGapScenario executes gap wire receipt verification', async () => {
  const harness = createMockHarness('output-replay-gap');
  const result = await runOutputReplayGapScenario(harness);
  assert.equal(result.scenario, 'output-replay-gap');
  assert.equal(result.status, 'passed');
  assert.equal(result.evidence.length, 4);
});

test('rebindOwner and correctToken emit wire receipts preserving target and cancellation identities', async () => {
  const rebindHarness = createMockHarness('close-rebind-pending-dial');
  const rebindRun = await rebindHarness.openWebOutcome({ scenario: 'close-rebind-pending-dial', variant: 'rebind' });
  let rebindPresentation = null;
  let rebindCancelled = null;
  rebindRun.gateway.on('presentation-sent', receipt => { rebindPresentation = receipt; });
  rebindRun.gateway.on('dial-cancelled', receipt => { rebindCancelled = receipt; });
  await rebindRun.rebindOwner();
  assert.equal(rebindPresentation?.target?.sessionId, rebindRun.rebindTarget.sessionId);
  assert.equal(rebindCancelled?.target?.sessionId, rebindRun.target.sessionId);
  assert.equal(rebindCancelled?.attemptId, 'att-1');

  const authHarness = createMockHarness('auth-revocation');
  const authRun = await authHarness.openWebOutcome({ scenario: 'auth-revocation' });
  let authPresentation = null;
  authRun.gateway.on('presentation-sent', receipt => { authPresentation = receipt; });
  await authRun.correctToken();
  assert.equal(authPresentation?.target?.sessionId, authRun.target.sessionId);
});

test('cold-start explicit create records request in ledger fence and rejects omission', async () => {
  const harness = createMockHarness('cold-start-unknown');
  const run = await harness.openWebOutcome({ scenario: 'cold-start-unknown' });
  const before = await run.gateway.fence();
  assert.equal(before.requests.filter(r => r.type === 'createSession').length, 0);

  await run.newTerminalOwner();
  const after = await run.gateway.fence();
  const creates = after.requests.filter(r => r.type === 'createSession');
  assert.equal(creates.length, 1);
  assert.equal(creates[0].requestId, 'create-req-1');

  run.gateway.clearRequests();
  const stripped = await run.gateway.fence();
  assert.equal(stripped.requests.filter(r => r.type === 'createSession').length, 0);
});
