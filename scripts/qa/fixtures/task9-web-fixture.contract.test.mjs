import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createWebOutcomeFixture } from './task9-web-fixture.mjs';

test('explicit creation returns a distinct target rather than the historical owner', () => {
  const fixture = createWebOutcomeFixture({});
  const response = fixture.createRoute.response(fixture.target, { createTerminal: true });
  assert.equal(response.sessionId, fixture.rebindTarget.sessionId);
  assert.notEqual(response.sessionId, fixture.target.sessionId);
});

test('exit codec preserves the selected target in the consumed terminal outcome shape', () => {
  const fixture = createWebOutcomeFixture({});
  for (const target of [fixture.target, fixture.rebindTarget]) {
    const frame = JSON.parse(fixture.exitFrame(target, 17));
    assert.equal(frame.type, 'terminalOutcome');
    assert.equal(frame.state, 'affirmative_exit');
    assert.deepEqual(frame.target, {
      sessionId: target.sessionId,
      backendSessionId: target.sessionId,
      daemonEpoch: target.daemonEpoch,
    });
    assert.equal(frame.incarnation, target.incarnation);
    assert.equal(frame.generation, null);
    assert.equal(frame.blockedReason, null);
    assert.equal(frame.exitCode, 17);
  }
});
