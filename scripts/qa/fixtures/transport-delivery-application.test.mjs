import { test } from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { APPLICATION_PROTOCOL, startDeliveryApplication,
  decodeDeliveryApplicationRecord } from './transport-delivery-application.mjs';

const target = { hostId: 'host', ownerId: 'actual-spawn-owner',
  epoch: '18446744073709551615', backendSessionId: 'actual-spawn-session' };
const operations = [{ kind: 'write', id: 'write-1', nonce: 'nonce-write' },
  { kind: 'signal', id: 'interrupt-1', nonce: 'nonce-int', signal: 'SIGINT' },
  { kind: 'signal', id: 'terminate-1', nonce: 'nonce-term', signal: 'SIGTERM' }];
function setup(platform = 'linux') {
  const stdin = new EventEmitter();
  const signals = new EventEmitter();
  const records = [];
  const stdout = { write(text) { records.push(JSON.parse(text)); } };
  const app = startDeliveryApplication({ runId: 'real-run', stdin, stdout, signals, platform });
  const frame = (body) => JSON.stringify({ protocol: APPLICATION_PROTOCOL, runId: 'real-run', ...body }) + '\n';
  const send = (body) => stdin.emit('data', Buffer.from(frame(body)));
  const configure = (ops = operations) => send({ type: 'configure', target, operations: ops });
  const operation = (index, type, extra = {}) => ({ type, target,
    operationId: operations[index].id, nonce: operations[index].nonce, ...extra });
  return { stdin, signals, records, app, frame, send, configure, operation };
}

test('actual complete input increments counters once and echoes consumed payload', () => {
  const io = setup();
  io.configure();
  const message = io.operation(0, 'write', { payload: 'actual payload é' });
  const bytes = Buffer.from(io.frame(message));
  const split = bytes.indexOf(Buffer.from('é')) + 1;
  io.stdin.emit('data', bytes.subarray(0, split));
  assert.equal(io.records.length, 1);
  io.stdin.emit('data', bytes.subarray(split));
  const consumed = io.records.at(-1);
  assert.equal(consumed.event, 'write-consumed');
  assert.equal(consumed.payload, message.payload);
  assert.deepEqual(consumed.target, target);
  assert.equal(consumed.operationId, operations[0].id);
  assert.equal(consumed.nonceCount, 1);
  assert.equal(consumed.writeCount, 1);
  assert.equal(consumed.signalCount, 0);
  io.send(message);
  assert.equal(io.records.at(-1).event, 'write-consumed');
  assert.equal(io.records.at(-1).duplicate, true);
  assert.equal(io.records.at(-1).writeCount, 2);
  assert.equal(io.records.at(-1).nonceCount, 2);
  io.app.stop();
});

test('only a prearmed actual signal event produces correlated signal consumption', () => {
  const io = setup();
  io.configure();
  io.send(io.operation(1, 'arm-signal', { signal: 'SIGINT' }));
  assert.equal(io.records.at(-1).event, 'signal-armed');
  assert.equal(io.records.at(-1).signalCount, 0);
  io.signals.emit('SIGINT');
  const receipt = io.records.at(-1);
  assert.equal(receipt.event, 'signal-consumed');
  assert.equal(receipt.operationId, 'interrupt-1');
  assert.equal(receipt.signalCount, 1);
  assert.equal(receipt.nonceCount, 1);
  assert.equal(receipt.writeCount, 0);
  io.signals.emit('SIGINT');
  assert.equal(io.records.at(-1).event, 'uncorrelated-signal');
  assert.equal(io.records.at(-1).signalCount, 2);
  assert.equal(decodeDeliveryApplicationRecord(JSON.stringify(io.records.at(-1)), 'real-run').signalCount, 2);
  io.send(io.operation(1, 'arm-signal', { signal: 'SIGINT' }));
  assert.equal(io.records.at(-1).code, 'SIGNAL_ALREADY_ARMED_OR_CONSUMED');
  io.send(io.operation(2, 'arm-signal', { signal: 'SIGTERM' }));
  io.signals.emit('SIGTERM');
  assert.equal(io.records.at(-1).operationId, 'terminate-1');
  assert.equal(io.records.at(-1).signalCount, 3);
  io.app.stop();
});

test('wrong signal and Ctrl-C payload do not establish an armed signal delivery', () => {
  const io = setup();
  io.configure();
  io.send(io.operation(1, 'arm-signal', { signal: 'SIGINT' }));
  io.send(io.operation(0, 'write', { payload: '\u0003' }));
  assert.equal(io.records.at(-1).event, 'write-consumed');
  assert.equal(io.records.at(-1).signalCount, 0);
  io.signals.emit('SIGTERM');
  assert.equal(io.records.at(-1).event, 'uncorrelated-signal');
  assert.equal(io.records.filter((r) => r.event === 'signal-consumed').length, 0);
  io.signals.emit('SIGINT');
  assert.equal(io.records.at(-1).event, 'signal-consumed');
  assert.equal(io.records.at(-1).signalCount, 2);
  io.app.stop();
});

test('run, target, nonce and malformed input rejection cannot increment write count', () => {
  const io = setup();
  io.configure();
  for (const changes of [{ runId: 'stale-run' }, { target: { ...target, epoch: '1' } },
    { nonce: 'wrong-nonce' }, { payload: 12 }]) {
    io.send({ ...io.operation(0, 'write', { payload: 'valid' }), ...changes });
    assert.equal(io.records.at(-1).event, 'error');
  }
  io.stdin.emit('data', Buffer.from('not-json\n'));
  assert.equal(io.records.at(-1).code, 'INVALID_JSON');
  io.send(io.operation(0, 'write', { payload: 'valid' }));
  assert.equal(io.records.at(-1).writeCount, 1);
  io.app.stop();
});

test('Windows refuses signal configuration and installs no OS signal listeners', () => {
  const io = setup('win32');
  io.configure();
  assert.equal(io.records.at(-1).code, 'SIGNAL_UNSUPPORTED_PLATFORM');
  assert.equal(io.signals.listenerCount('SIGINT'), 0);
  assert.equal(io.signals.listenerCount('SIGTERM'), 0);
  io.configure([operations[0]]);
  assert.equal(io.records.at(-1).event, 'ready');
  assert.equal(io.records.at(-1).signalSupport, 'unsupported');
  io.send(io.operation(0, 'write', { payload: 'portable write' }));
  assert.equal(io.records.at(-1).writeCount, 1);
  io.app.stop();
});

test('decoder excludes echoed input but retains repeated actual consumption', () => {
  const io = setup();
  io.configure();
  const write = io.operation(0, 'write', { payload: 'consumed' });
  assert.equal(decodeDeliveryApplicationRecord(io.frame(write).trim(), 'real-run'), null);
  io.send(write);
  io.send(write);
  const decoded = io.records.map((r) => decodeDeliveryApplicationRecord(JSON.stringify(r), 'real-run'))
    .filter(Boolean);
  assert.equal(decoded.length, 2);
  assert.equal(decoded[0].event, 'write-consumed');
  assert.equal(decoded[1].nonceCount, 2);
  assert.equal(decodeDeliveryApplicationRecord(JSON.stringify(decoded[0]), 'other-run'), null);
  io.app.stop();
});

test('EOF rejects partial frames and removes owned listeners without sending signals', () => {
  const io = setup();
  io.configure();
  io.stdin.emit('data', Buffer.from('{'));
  io.stdin.emit('end');
  assert.equal(io.records.at(-1).code, 'INCOMPLETE_INPUT_FRAME');
  assert.equal(io.stdin.listenerCount('data'), 0);
  assert.equal(io.signals.listenerCount('SIGINT'), 0);
  const before = io.records.length;
  io.signals.emit('SIGINT');
  assert.equal(io.records.length, before);
});
