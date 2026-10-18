import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { createHash } from 'node:crypto';
import { createAckLossHelperClient } from './transport-provider.mjs';

const target = { hostId: 'isolated-host', ownerId: 'actual-owner', epoch: '42', backendSessionId: 'actual-session' };
const params = { target, text: 'recorded-operation-and-nonce\n' };
function frame(reply) {
  const body = Buffer.from(JSON.stringify(reply));
  const header = Buffer.alloc(4);
  header.writeUInt32BE(body.length);
  return Buffer.concat([header, body]);
}
function fixture(suppress, signal) {
  const socket = new EventEmitter();
  const writes = [];
  socket.write = (bytes) => { writes.push(Buffer.from(bytes)); return true; };
  socket.destroy = () => { socket.emit('close'); };
  const timers = new Map();
  let serial = 0;
  const clock = { setTimeout(fn) { timers.set(++serial, fn); return serial; },
    clearTimeout(id) { timers.delete(id); } };
  const client = createAckLossHelperClient({ socket, token: 'isolated-token',
    timeoutMs: 1000, ownerId: 'qa-owner', creationBaseline: 1, suppress, signal, clock });
  return { socket, writes, timers, client };
}

test('actual decoded matching accepted reply is suppressed with raw frame provenance', async () => {
  const io = fixture((r) => r.requestId === 'qa-owner:1' && r.op === 'pty.write' &&
    r.target.backendSessionId === target.backendSessionId && r.request.params.text === params.text);
  const result = io.client.call('pty.write', params);
  const rejected = assert.rejects(result, { code: 'HELPER_REPLY_UNCERTAIN' });
  const raw = frame({ ok: true, data: { accepted: true } });
  io.socket.emit('data', raw.subarray(0, 2));
  io.socket.emit('data', raw.subarray(2, 7));
  io.socket.emit('data', raw.subarray(7));
  await rejected;
  assert.equal(io.writes.length, 1);
  assert.deepEqual(JSON.parse(io.writes[0].subarray(4).toString()),
    { protocol: 1, token: 'isolated-token', op: 'pty.write', params });
  const snapshot = io.client.snapshot();
  assert.deepEqual(snapshot.audit.map((r) => r.sequence), [1, 2, 3, 4]);
  const loss = snapshot.audit.find((r) => r.kind === 'reply-suppressed');
  assert.equal(loss.requestId, 'qa-owner:1');
  assert.deepEqual(loss.target, target);
  assert.deepEqual(Buffer.from(loss.rawReplyBase64, 'base64'), raw);
  assert.equal(loss.rawReplySha256, createHash('sha256').update(raw).digest('hex'));
  assert.equal(loss.decodedSequence, 2);
  assert.equal(snapshot.creationBaseline, 1);
  assert.equal(snapshot.scope, 'this-helper-client-only');
  assert.equal(io.timers.size, 0);
  await assert.rejects(io.client.call('pty.write', params), { code: 'HELPER_CLIENT_CLOSED' });
  assert.equal(io.writes.length, 1);
});

test('unrelated target, negative ACK and nonmutation reply are delivered', async () => {
  const io = fixture((r) => r.target?.backendSessionId === target.backendSessionId);
  for (const [op, request, response] of [
    ['pty.write', { ...params, target: { ...target, backendSessionId: 'other-session' } }, { ok: true, data: { accepted: true } }],
    ['pty.write', params, { ok: false, error: 'write failed' }],
    ['pty.read', { target, cursor: '0' }, { ok: true, data: { chunks: [] } }],
  ]) {
    const result = io.client.call(op, request);
    io.socket.emit('data', frame(response));
    assert.deepEqual(await result, response);
  }
  assert.equal(io.writes.length, 3);
  assert.equal(io.client.snapshot().audit.filter((r) => r.kind === 'reply-suppressed').length, 0);
  io.client.close();
});

test('cancellation and controlled deadline never resend and fence late bytes', async () => {
  for (const mode of ['abort', 'deadline']) {
    const controller = new AbortController();
    const io = fixture(() => true, controller.signal);
    const result = io.client.call('pty.write', params);
    const rejected = assert.rejects(result, { code: mode === 'abort' ? 'TRANSPORT_CANCELLED' : 'TRANSPORT_DEADLINE' });
    if (mode === 'abort') controller.abort();
    else [...io.timers.values()][0]();
    await rejected;
    io.socket.emit('data', frame({ ok: true, data: { accepted: true } }));
    assert.equal(io.writes.length, 1);
    assert.equal(io.client.snapshot().audit.filter((r) => r.kind === 'decoded-reply').length, 0);
    assert.equal(io.timers.size, 0);
  }
});

test('single-flight owner audit captures mutation and creation calls monotonically', async () => {
  const io = fixture(() => false);
  const spawn = io.client.call('pty.spawn', { clientRequestId: 'recorded-create' });
  await assert.rejects(io.client.call('pty.write', params), { code: 'HELPER_SINGLE_FLIGHT' });
  io.socket.emit('data', frame({ ok: true, data: { target } }));
  await spawn;
  const write = io.client.call('pty.write', params);
  io.socket.emit('data', frame({ ok: true, data: { accepted: true } }));
  await write;
  const requests = io.client.snapshot().audit.filter((r) => r.kind === 'request-written');
  assert.deepEqual(requests.map((r) => r.requestId), ['qa-owner:1', 'qa-owner:2']);
  assert.deepEqual(requests.map((r) => [r.mutation, r.creation]), [[true, true], [true, false]]);
  assert.equal(io.writes.length, 2);
  io.client.close();
});
