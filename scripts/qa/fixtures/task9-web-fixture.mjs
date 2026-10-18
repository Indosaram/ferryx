/**
 * Isolated web outcome fixture for Task 9 web recovery scenarios.
 * Supplies distinct target identities, wire frame codecs, and selection route.
 */
export function createWebOutcomeFixture(ctx) {
  const target = {
    sessionId: 'task9-web-01',
    daemonEpoch: 'epoch-web-1',
    incarnation: 'inc-web-1',
    pid: 8881,
  };

  const rebindTarget = {
    sessionId: 'task9-web-02',
    daemonEpoch: 'epoch-web-1',
    incarnation: 'inc-web-2',
    pid: 8882,
  };

  return {
    target,
    rebindTarget,
    evidenceScope: 'controlled-ui',
    identityEvidence: 'fixture-identity-proof-not-machine-pid',
    exitFrame(t, exitCode = 0) {
      return JSON.stringify({
        type: 'terminalOutcome',
        target: { sessionId: t.sessionId, backendSessionId: t.sessionId, daemonEpoch: t.daemonEpoch },
        incarnation: t.incarnation,
        generation: null,
        state: 'affirmative_exit',
        blockedReason: null,
        exitCode,
      });
    },
    gapFrame(t, requestedCursor, oldestAvailableCursor) {
      return JSON.stringify({
        type: 'replayGap',
        sessionId: t.sessionId,
        requestedCursor: Number(requestedCursor) || 0,
        oldestAvailableCursor: Number(oldestAvailableCursor) || 0,
      });
    },
    createRoute: {
      method: 'POST',
      path: '/api/v1/workspace/select',
      response(previousTarget, body) {
        return { ok: true, sessionId: rebindTarget.sessionId };
      },
    },
  };
}

export default createWebOutcomeFixture;
