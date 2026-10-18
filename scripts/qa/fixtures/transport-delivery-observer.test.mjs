import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { APPLICATION_PROTOCOL, startDeliveryApplication,
  decodeDeliveryApplicationRecord } from './transport-delivery-application.mjs';
import { createTransportDeliveryObserver } from './transport-delivery-observer.mjs';

const target = Object.freeze({ hostId: 'isolated-host', ownerId: 'spawn-owner',
  epoch: '18446744073709551615', backendSessionId: 'spawn-session' });
const operation = { kind: 'write', id: 'recorded-operation', nonce: 'recorded-nonce',
  target, cursor: '9007199254740993',
  before: { writeCount: 4, nonceCount: 0, signalCount: 2 } };
const record = (overrides = {}) => ({ protocol: 'injected-test-application-json-lines/1',
  runId: 'observed-run', operationId: operation.id, nonce: operation.nonce,
  target, writeCount: 5, nonceCount: 1, signalCount: 2, ...overrides });
const line = (value = record()) => `${JSON.stringify(value)}\n`;
const chunk = (cursor, text) => ({ cursor, data: Buffer.from(text).toString('base64') });
const reply = (chunks, overrides = {}) => ({ ok: true, data: { target, gap: false,
  exited: false, cursor: chunks.at(-1)?.cursor ?? operation.cursor, chunks, ...overrides } });
function setup(replies, overrides = {}) {
  const calls = [];
  const helper = { async call(op, params) {
    calls.push({ op, params });
    const next = replies.shift();
    return typeof next === 'function' ? next() : next;
  } };
  const observer = createTransportDeliveryObserver({ helper, target, operation,
    cursor: operation.cursor, producerContract: 'injected-test-application-json-lines/1',
    runId: 'observed-run',
    decodeRecord: (text) => text.startsWith('{') ? JSON.parse(text) : null, ...overrides });
  return { observer, calls };
}

test('write ACK cannot be counted as application delivery', async () => {
  const { observer, calls } = setup([{ ok: true, data: { accepted: true } }]);
  await assert.rejects(observer.read(), { code: 'STALE_DELIVERY_TARGET' });
  assert.equal(observer.snapshot().writeDeliveryObserved, false);
  assert.deepEqual(calls.map((call) => call.op), ['pty.read']);
});

test('split records retain exact cursors and duplicate chunks are not recounted', async () => {
  const text = line();
  const first = chunk('9007199254740994', text.slice(0, 42));
  const second = chunk('9007199254740995', text.slice(42));
  const { observer, calls } = setup([reply([first]), reply([first, second]), reply([second])]);
  assert.equal((await observer.read()).observations.length, 0);
  const observed = await observer.read();
  assert.equal(observed.observations.length, 1);
  assert.equal(observed.observations[0].firstCursor, first.cursor);
  assert.equal(observed.observations[0].cursor, second.cursor);
  assert.deepEqual(observed.observations[0].target, target);
  assert.equal((await observer.read()).observations.length, 1);
  assert.equal(calls[1].params.cursor, first.cursor);
  assert.equal(calls[2].params.cursor, second.cursor);
  assert.equal(observed.status, 'incomplete');
  assert.equal(observed.signalCoverage, false);
  assert.ok(observed.remaining.includes('actual-delivery-before-ack-loss-boundary'));
});

test('operation ID and nonce must both match actual application output', async () => {
  const text = line(record({ operationId: 'other-operation' })) +
    line(record({ nonce: 'other-nonce' })) + line();
  const { observer } = setup([reply([chunk('9007199254740994', text)])]);
  const result = await observer.read();
  assert.equal(result.observations.length, 1);
  assert.equal(result.observations[0].operationId, operation.id);
  assert.equal(result.observations[0].nonce, operation.nonce);
});

test('stale helper or application target cannot establish delivery', async () => {
  const stale = { ...target, ownerId: 'previous-owner' };
  for (const response of [reply([chunk('9007199254740994', line())], { target: stale }),
    reply([chunk('9007199254740994', line(record({ target: stale })))])]) {
    const { observer } = setup([response]);
    await assert.rejects(observer.read(), (error) =>
      ['STALE_DELIVERY_TARGET', 'STALE_APPLICATION_TARGET'].includes(error.code));
    assert.equal(observer.snapshot().writeDeliveryObserved, false);
  }
});

test('stale cursor, missing chunk and replay gap fail closed', async () => {
  for (const response of [reply([], { cursor: '1' }),
    reply([chunk('9007199254740995', line())]),
    reply([chunk('9007199254740994', line())], { gap: true })]) {
    const { observer } = setup([response]);
    await assert.rejects(observer.read());
    assert.equal(observer.snapshot().streamInvalid, true);
    await assert.rejects(observer.read(), { code: 'DELIVERY_STREAM_INVALID' });
  }
});

test('conflicting duplicate bytes are not accepted', async () => {
  const { observer } = setup([reply([chunk('9007199254740994', line())]),
    reply([chunk('9007199254740994', 'changed\n')])]);
  await observer.read();
  await assert.rejects(observer.read(), { code: 'DUPLICATE_CHUNK_CONFLICT' });
  assert.equal(observer.snapshot().streamInvalid, true);
});

test('counter changes must correlate with exactly one recorded write', async () => {
  for (const value of [record({ writeCount: 6 }), record({ nonceCount: 2 }),
    record({ signalCount: 3 })]) {
    const { observer } = setup([reply([chunk('9007199254740994', line(value))])]);
    await assert.rejects(observer.read(), { code: 'DELIVERY_COUNTER_CONFLICT' });
    assert.equal(observer.snapshot().streamInvalid, true);
    assert.deepEqual(observer.snapshot().observations[0].record, value);
  }
});

test('two identical input frames reach observer as nonce count two, not read replay', async () => {
  const stdin = new EventEmitter();
  const signals = new EventEmitter();
  const output = [];
  const app = startDeliveryApplication({ runId: 'actual-run', stdin, signals,
    stdout: { write(text) { output.push(text); } }, platform: 'linux' });
  const send = (body) => stdin.emit('data', Buffer.from(JSON.stringify({
    protocol: APPLICATION_PROTOCOL, runId: 'actual-run', ...body }) + '\n'));
  send({ type: 'configure', target, operations: [{ kind: 'write', id: operation.id, nonce: operation.nonce }] });
  output.length = 0;
  const frame = { type: 'write', target, operationId: operation.id,
    nonce: operation.nonce, payload: 'actual input' };
  send(frame);
  send(frame);
  const first = chunk('9007199254740994', output[0].slice(0, 25));
  const second = chunk('9007199254740995', output[0].slice(25));
  const repeatedWrite = chunk('9007199254740996', output[1]);
  const { observer } = setup([reply([first]), reply([first, second]),
    reply([second]), reply([repeatedWrite])], { producerContract: APPLICATION_PROTOCOL,
    runId: 'actual-run', decodeRecord: (text) => decodeDeliveryApplicationRecord(text, 'actual-run'),
    operation: { ...operation, before: { writeCount: 0, nonceCount: 0, signalCount: 0 } } });
  try {
    assert.equal((await observer.read()).observations.length, 0);
    assert.equal((await observer.read()).observations.length, 1);
    assert.equal((await observer.read()).observations.length, 1);
    await assert.rejects(observer.read(), { code: 'DELIVERY_COUNTER_CONFLICT' });
    const evidence = observer.snapshot();
    assert.equal(evidence.streamInvalid, true);
    assert.deepEqual(evidence.observations.map((r) => r.record.nonceCount), [1, 2]);
    assert.equal(evidence.observations[1].record.writeCount, 2);
    assert.equal(evidence.observations[1].cursor, repeatedWrite.cursor);
  } finally { app.stop(); }
});

test('observer rejects wrong run or protocol and retains uncorrelated signal evidence', async () => {
  for (const overrides of [{ runId: 'stale-run' }, { protocol: 'stale-protocol' }]) {
    const { observer } = setup([reply([chunk('9007199254740994', line(record(overrides)))])]);
    await assert.rejects(observer.read(), { code: 'STALE_APPLICATION_RUN' });
  }
  const { observer } = setup([reply([chunk('9007199254740994', line({
    protocol: 'injected-test-application-json-lines/1', runId: 'observed-run',
    target, event: 'uncorrelated-signal', writeCount: 4, signalCount: 3, signal: 'SIGINT' }))])]);
  await assert.rejects(observer.read(), { code: 'UNEXPECTED_SIGNAL_DELIVERY' });
  assert.equal(observer.snapshot().signalObservations[0].record.signalCount, 3);
});

test('cancellation fences an in-flight reply and does not replay or close helper', async () => {
  let complete;
  const controller = new AbortController();
  const { observer, calls } = setup([() => new Promise((resolve) => { complete = resolve; })],
    { signal: controller.signal });
  const reading = observer.read();
  const rejected = assert.rejects(reading, { code: 'DELIVERY_CANCELLED' });
  await assert.rejects(observer.read(), { code: 'DELIVERY_SINGLE_FLIGHT' });
  controller.abort();
  complete(reply([chunk('9007199254740994', line())]));
  await rejected;
  assert.equal(observer.snapshot().observations.length, 0);
  assert.equal(calls.length, 1);
  await assert.rejects(observer.read(), { code: 'DELIVERY_CANCELLED' });
});

test('factory requires provisioned IO, recorded operation and cursor', () => {
  for (const overrides of [{ helper: null }, { cursor: undefined },
    { operation: { ...operation, kind: 'signal' } },
    { operation: { ...operation, target: { ...target, epoch: '1' } } },
    { decodeRecord: undefined }]) {
    assert.throws(() => setup([], overrides));
  }
  const { observer, calls } = setup([]);
  observer.cancel();
  assert.equal(observer.snapshot().cancelled, true);
  assert.equal(calls.length, 0);
});
