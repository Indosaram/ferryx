import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runInNewContext } from 'node:vm';
import { WebOutcomeGateway } from './web-outcome-gateway.mjs';
import { browserReceiptScript } from './web-outcome-browser.mjs';

for (const theme of ['light', 'dark']) {
  test(`browser receipts persist explicit ${theme} appearance before application boot`, () => {
    const stored = new Map();
    const script = browserReceiptScript({ token: 'task9-token', theme });
    runInNewContext(script.slice('<script>'.length, -'</script>'.length), {
      window: { fetch() {}, WebSocket: class {}, setTimeout },
      localStorage: { setItem(key, value) { stored.set(key, value); } },
      location: { origin: 'http://localhost' },
      document: { documentElement: { classList: { toggle() {} }, setAttribute() {}, style: {} }, addEventListener() {} },
    });
    assert.deepEqual(JSON.parse(stored.get('ferryx.settings.appearance')), { theme });
  });
}

for (const receivedBeforeArm of [false, true]) {
  test(`legacy ticket completion survives owner abort with response received before arm: ${receivedBeforeArm}`, async () => {
    // Given a real ticket response buffered at the browser fetch boundary.
    const target = { sessionId: 'historical', daemonEpoch: 'old-epoch' };
    let receive, markReceived, query;
    const responseReceived = new Promise(resolve => { markReceived = resolve; });
    const response = new Response(JSON.stringify({ ticket: 'single-use-ticket', expiresAt: 123 }));
    const readBody = response.arrayBuffer.bind(response);
    response.arrayBuffer = async () => { const body = await readBody(); markReceived(); return body; };
    const window = {
      fetch: () => new Promise(resolve => { receive = resolve; }),
      WebSocket: class {}, setTimeout, clearTimeout,
      __ferryxRecoveryQA: { waitForRetiredStaleCompletion(...args) { query = args; return Promise.resolve({ rejectedAttemptIds: ['1'] }); } },
    };
    const script = browserReceiptScript({ token: 'task9-token', theme: 'dark', holdTicketTarget: target });
    runInNewContext(script.slice('<script>'.length, -'</script>'.length), {
      window, localStorage: { setItem() {} }, location: { origin: 'http://localhost', href: 'http://localhost/' },
      document: { documentElement: { classList: { toggle() {} }, setAttribute() {}, style: {} }, addEventListener() {} },
      crypto: { randomUUID: () => 'ticket-request' }, URL, Headers, Request, Response, setTimeout, clearTimeout,
    });
    const abort = new AbortController();
    let delivered = false;
    const completion = window.fetch('http://localhost/api/v1/socket-ticket', {
      method: 'POST', body: JSON.stringify({ target: '/api/v1/terminal/historical' }), signal: abort.signal,
    }).then(value => { delivered = true; return value.json(); });
    if (receivedBeforeArm) { receive(response); await responseReceived; }
    const key = window.__task9Wire.armTicketCompletion(target, '1', 1000);
    if (!receivedBeforeArm) receive(response);
    const held = await window.__task9Wire.waitTicketHeld(key, 1000);
    assert.equal(delivered, false);
    assert.equal(held.delivered, false);
    assert.deepEqual(JSON.parse(JSON.stringify(query)), [target, '1', 1000]);
    assert.throws(() => window.__task9Wire.armTicketCompletion({ ...target, daemonEpoch: 'wrong' }, '1', 1000),
      /TASK9_TICKET_COMPLETION_TARGET_MISMATCH/);

    // When the original owner aborts and only then the original response is released.
    abort.abort();
    const released = window.__task9Wire.releaseTicketCompletion(key);
    const body = await completion;

    // Then the original ticket, not a manufactured socket/counter, completes unchanged.
    assert.equal(released.requestId, held.requestId);
    assert.deepEqual(body, { ticket: 'single-use-ticket', expiresAt: 123 });
    assert.deepEqual(await window.__task9Wire.readRetiredObserver(key), { rejectedAttemptIds: ['1'] });
    assert.equal(window.__task9Wire.events.filter(event => event.kind === 'ticket-completion-delivered').length, 1);
  });
}

test('explicit creation makes only the new owner authoritative while historical owner stays unavailable', async () => {
  const target = { sessionId: 'old', daemonEpoch: 'old-epoch', incarnation: 'old-inc', pid: 1 };
  const rebindTarget = { sessionId: 'new', daemonEpoch: 'new-epoch', incarnation: 'new-inc', pid: 2 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget, initial: 'historical-owner-unavailable',
    createRoute: { method: 'POST', path: '/api/v1/workspace/select', response: () => ({ ok: true, sessionId: rebindTarget.sessionId }) } });
  const sessions = () => gateway.handleFetch(new Request('http://localhost/api/v1/sessions'), {});
  assert.equal((await sessions()).status, 503);
  await gateway.handleFetch(new Request('http://localhost/api/v1/workspace/select', {
    method: 'POST', body: JSON.stringify({ createTerminal: true, workspaceId: 'workspace', worktreeSlug: 'main' }),
  }), {});
  assert.deepEqual(await (await sessions()).json(), [{ ...rebindTarget, running: true }]);
  const state = await gateway.handleFetch(new Request('http://localhost/api/v1/workspace/state'), {});
  assert.deepEqual((await state.json()).sessions, [{ ...rebindTarget, running: true }]);
});

test('pending owner cancellation queries only identity established before handshake', async () => {
  const target = { sessionId: 'pending', daemonEpoch: 'epoch', incarnation: 'not-yet-observed', pid: 7 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: { ...target, sessionId: 'other' } });
  let query;
  const observed = { disposed: true, staleCompletionsRejected: 1 };
  gateway.browser = { evaluate: expression => runInNewContext(expression, {
    window: { __ferryxRecoveryQA: { waitForStaleCompletion(value) { query = value; return observed; } } },
  }) };
  assert.equal(await gateway.inject('release-stale-dial', { target }), observed);
  assert.deepEqual(JSON.parse(JSON.stringify(query)), { sessionId: 'pending', daemonEpoch: 'epoch' });
});

test('legacy presentation sends only a full grid without synthetic ready identity', () => {
  // Given a legacy terminal route with an unobserved fixture incarnation.
  const target = { sessionId: 'retained', daemonEpoch: '1', incarnation: 'owner', pid: 7 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: { ...target, sessionId: 'other' } });
  const sent = [];
  const ws = { data: { sessionId: target.sessionId, socketId: 'socket' }, send(raw) { sent.push(JSON.parse(raw)); return 1; } };

  // When presentation is released on that route.
  const receipt = gateway.sendPresentation(ws);

  // Then the wire itself supplies the full-grid boundary, without a ready outcome.
  assert.equal(sent.length, 1);
  assert.equal(sent[0].type, 'grid');
  assert.equal(sent[0].lines[0].runs[0].text, 'task9-current-target');
  assert.equal(sent[0].task9ReceiptId, receipt.receiptId);
  assert.equal(gateway.rawFrames.length, 1);
  assert.deepEqual(gateway.proof, { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch,
    socketId: 'socket', receiptId: receipt.receiptId });
});

test('legacy current target separates observed fields from fixture identity and rejects invalid proof', async () => {
  const target = { sessionId: 'retained', daemonEpoch: '1', incarnation: 'fixture-only', pid: 7 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: { ...target, sessionId: 'other' } });
  const receipt = gateway.sendPresentation({ data: { sessionId: target.sessionId, socketId: 'socket' }, send() { return 1; } });
  const observed = { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch,
    incarnation: null, socketId: 'socket', generation: 1, presentationSequence: 1, outcome: 'ready' };
  let current = observed;
  gateway.browser = { evaluate: async () => current };
  const proof = await gateway.currentTarget();
  assert.equal(proof.incarnation, null);
  assert.equal(Object.hasOwn(proof, 'pid'), false);
  assert.deepEqual(proof.fullGridReceipt, { sessionId: target.sessionId, daemonEpoch: target.daemonEpoch,
    socketId: 'socket', receiptId: receipt.receiptId });
  assert.equal(proof.evidenceScope, 'legacy-web-presentation');
  assert.equal(proof.processSurvivalProven, false);
  assert.deepEqual(await gateway.validatedBinding(), target);
  for (const [change, error] of [
    [{ socketId: 'foreign' }, /TASK9_CURRENT_SOCKET_MISMATCH/],
    [{ daemonEpoch: 'wrong' }, /TASK9_CURRENT_EPOCH_NOT_VALIDATED/],
    [{ outcome: 'retryable' }, /TASK9_CURRENT_PRESENTATION_NOT_READY/],
    [{ presentationSequence: 0 }, /TASK9_CURRENT_PRESENTATION_NOT_READY/],
    [{ generation: 0 }, /TASK9_CURRENT_PRESENTATION_NOT_READY/],
  ]) {
    current = { ...observed, ...change };
    await assert.rejects(gateway.currentTarget(), error);
  }
});

test('serialized outcome frames retain protocol fields at the socket boundary', () => {
  const target = { sessionId: 'retained', daemonEpoch: '1', incarnation: 'owner', pid: 7 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: { ...target, sessionId: 'other' } });
  let sent;
  const ws = { data: { sessionId: target.sessionId, socketId: 'socket' }, send(raw) { sent = JSON.parse(raw); return 1; } };
  const frame = { type: 'terminalOutcome', target, incarnation: target.incarnation,
    generation: null, state: 'affirmative_exit', blockedReason: null, exitCode: 0 };
  const receipt = gateway.send(ws, JSON.stringify(frame));
  assert.deepEqual(sent, { ...frame, task9ReceiptId: receipt.receiptId });
  assert.deepEqual(JSON.parse(gateway.rawFrames[0].raw), sent);
});

test('workspace selection counts creation only for an explicit createTerminal request', async () => {
  const target = { sessionId: 'first', daemonEpoch: '1', incarnation: 'owner', pid: 7 };
  const other = { ...target, sessionId: 'second' };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: other,
    createRoute: { method: 'POST', path: '/api/v1/workspace/select', response: () => ({ ok: true, sessionId: target.sessionId }) } });
  let creates = 0;
  const events = [];
  gateway.eventSockets.add({ send: raw => events.push(JSON.parse(raw)) });
  gateway.on('create-request', () => { creates++; });
  const select = body => gateway.handleFetch(new Request('http://localhost/api/v1/workspace/select', {
    method: 'POST', body: JSON.stringify(body),
  }), {});
  const selection = { workspaceId: 'workspace', worktreeSlug: 'main', tabId: other.sessionId };
  assert.equal((await select(selection)).status, 200);
  assert.deepEqual(events, [{ event: 'remote_active_selection_changed', payload: selection }]);
  assert.equal(gateway.activeSessionId, other.sessionId);
  assert.equal(creates, 0);
  assert.equal(gateway.rawRequests.filter(r => r.type === 'createSession').length, 0);
  assert.equal((await select({ sessionId: other.sessionId, createTerminal: true })).status, 200);
  assert.equal(creates, 1);
  assert.equal(gateway.rawRequests.filter(r => r.type === 'createSession').length, 1);
  assert.equal(gateway.activeSessionId, target.sessionId);
  assert.equal(gateway.autoReady, true);
  assert.equal(events.at(-1).event, 'remote_active_selection_changed');
  assert.equal(events.at(-1).payload.tabId, target.sessionId);
});

test('preferences requests preserve selected target and do not count as terminal dials', async () => {
  const target = { sessionId: 'retained', daemonEpoch: '1', incarnation: 'owner', pid: 7 };
  const gateway = new WebOutcomeGateway({ target, rebindTarget: { ...target, sessionId: 'other' } });
  const server = { upgrade() { assert.fail('Preferences must not upgrade a socket'); } };
  for (let i = 0; i < 2; i++) {
    const response = await gateway.handleFetch(new Request('http://localhost/api/v1/terminal/preferences'), server);
    assert.equal(response.status, 200);
    assert.equal(gateway.activeSessionId, target.sessionId);
    assert.equal(gateway.rawRequests.at(-1).dialIndex, undefined);
    assert.equal(gateway.rawRequests.at(-1).socketId, undefined);
  }
  const state = await gateway.handleFetch(new Request('http://localhost/api/v1/workspace/state'), server);
  assert.equal(state.status, 200);
  assert.equal(gateway.activeSessionId, target.sessionId);
  assert.equal(gateway.terminalDials.length, 0);
});

test('GET sessions publishes exact supplied daemonEpoch and historical-owner-unavailable remains 503', async () => {
  const target = { sessionId: 'task9-web-01', daemonEpoch: 'epoch-web-1', incarnation: 'owner-1', pid: 101 };
  const rebindTarget = { sessionId: 'task9-web-02', daemonEpoch: 'epoch-web-2', incarnation: 'owner-2', pid: 102 };
  const server = { upgrade() { assert.fail('Sessions must not upgrade a socket'); } };

  // Normal gateway publishes exact supplied epoch for all target sessions
  const gateway = new WebOutcomeGateway({ target, rebindTarget });
  const response = await gateway.handleFetch(new Request('http://localhost/api/v1/sessions'), server);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get('Access-Control-Allow-Origin'), '*');
  const sessions = await response.json();
  assert.deepEqual(sessions, [
    { ...target, running: true },
    { ...rebindTarget, running: true },
  ]);
  assert.equal(sessions[0].daemonEpoch, 'epoch-web-1');
  assert.equal(sessions[1].daemonEpoch, 'epoch-web-2');

  // historical-owner-unavailable initial mode yields 503 OWNER_UNAVAILABLE
  const unavailableGateway = new WebOutcomeGateway({
    target,
    rebindTarget,
    initial: 'historical-owner-unavailable',
  });
  const unavailableResponse = await unavailableGateway.handleFetch(
    new Request('http://localhost/api/v1/sessions'),
    server,
  );
  assert.equal(unavailableResponse.status, 503);
  const errorBody = await unavailableResponse.json();
  assert.deepEqual(errorBody, { error: 'OWNER_UNAVAILABLE' });
});
