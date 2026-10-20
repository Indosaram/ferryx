import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import {
  WEB_OUTCOME_SCENARIOS,
  executeWebOutcome,
  outcomeExpression,
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
  const deliveries = [];
  const selectionRequests = [];
  const current = {
    ...target,
    socketId: 'sock-01',
    generation: '8',
    presentationSequence: 1,
  };

  return {
    on: emitter.on.bind(emitter),
    off: emitter.off.bind(emitter),
    emit: emitter.emit.bind(emitter),
    currentTarget: async () => current,
    validatedBinding: async () => target,
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
          emitter.emit('presentation-sent', { target: cmdTarget, sequence: 2 });
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
    fence: async () => ({
      complete: true,
      fenceId: 'fence-123',
      source: 'mock-gateway',
      requests: rawRequests,
      deliveries,
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
                    outcome: 'blocked',
                    reason: 'Recovery outcome unknown',
                    sessionId: target.sessionId,
                    daemonEpoch: target.daemonEpoch,
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
          });
        },
        correctToken: async () => {
          state = 'correcting';
        },
        rebindOwner: async () => {},
        closeOwner: async () => {},
        newTerminalOwner: async () => {
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

test('outcomeExpression requires window.__ferryxRecoveryQA and throws typed error when absent', () => {
  const expr = outcomeExpression({ sessionId: 's1', daemonEpoch: 'e1' }, ['ready']);
  const fn = new Function('window', 'document', `return (${expr})()`);
  assert.throws(
    () => fn({}, {}),
    err => err.message.includes('TASK9_PRODUCT_SEAM_REQUIRED: __ferryxRecoveryQA')
  );
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

test('outcomeExpression matches exited and does not claim settled unless exitSettlements > 0', () => {
  const target = { sessionId: 's1', daemonEpoch: 'e1' };
  const expr = outcomeExpression(target, ['exited']);
  const fn = new Function('window', 'document', `return (${expr})()`);

  // Product currently has exitSettlements: null -> activity must be null (unproven)
  const mockQANull = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      inFlight: false,
      pendingTimers: 0,
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
  assert.equal(unproven.outcome, 'exited');
  assert.equal(unproven.activity, null);

  // Once product author 1395d supplies exitSettlements: 1 -> activity is settled
  const mockQAProven = {
    snapshot: () => ({
      retainedTarget: { sessionId: 's1', daemonEpoch: 'e1' },
      identityConfirmed: true,
      disposed: false,
      inFlight: false,
      pendingTimers: 0,
      exitSettlements: 1,
    }),
  };
  const proven = fn({ __ferryxRecoveryQA: mockQAProven }, mockDocExited);
  assert.equal(proven.outcome, 'exited');
  assert.equal(proven.activity, 'settled');
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
