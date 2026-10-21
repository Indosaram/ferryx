import { test } from 'node:test';
import assert from 'node:assert/strict';
import { joinOwnedPty, prearmPresentationBinding } from './native-configured-adapters.mjs';
import { createOwnedPtyStream } from './native-remote-agent.mjs';
import { ATTACH_TUPLE_FIELDS } from '../../lib/qa-scenarios/common-harness.mjs';

function fixture() {
  const common = { runId: 'run', operationId: 'op', candidate: 'candidate', hostId: 'host' };
  const attachTuple = Object.fromEntries(ATTACH_TUPLE_FIELDS.map((field) => [field, field]));
  attachTuple.backendSessionId = 'backend'; attachTuple.incarnation = 'inc';
  const recorded = [];
  return { ...common, cursor: { path: '/private/barriers/owned-pty-creation.receipt.jsonl', afterLine: 2 },
    daemon: { ...common, role: 'daemon', pid: 10, executable: '/candidate/ferryx', processStart: '10' },
    gui: { ...common, role: 'gui', pid: 11, executable: '/candidate/ferryx', processStart: '11' },
    binding: { runId: 'run', operationId: 'op', producerPid: 11, attachTuple },
    record: { path: '/private/barriers/owned-pty-creation.receipt.jsonl', lineIndex: 3,
      raw: { runId: 'run', operationId: 'op', producerPid: 10, role: 'pty', ownershipComplete: true,
        producerComponent: 'terminal-qa-owned-pty-creation', backendSessionId: 'backend', incarnation: 'inc',
        pid: 42, executable: '/actual/shell', processStart: '123:000007' } },
    probe: async () => ({ exited: false, executable: '/actual/shell', processStart: '123:000007' }),
    recordOwnedPid: async (target) => recorded.push(target), recorded };
}

test('ownership join records only after exact probe and actual GUI binding', async () => {
  const input = fixture();
  const target = await joinOwnedPty(input);
  assert.equal(input.recorded.length, 1);
  assert.equal(target.pid, 42);
  assert.equal(target.daemonEpoch, input.binding.attachTuple.daemonEpoch);
  assert.equal(target.ownershipLineIndex, 3);
});

test('stale, refused, foreign, unbound and replaced ownership never records', async () => {
  for (const alter of [
    (value) => { value.record.lineIndex = 2; },
    (value) => { value.record.raw.ownershipComplete = false; },
    (value) => { value.record.raw.producerPid = 99; },
    (value) => { value.record.raw.operationId = 'old'; },
    (value) => { value.binding = null; },
    (value) => { value.binding.attachTuple.incarnation = 'other'; },
    (value) => { value.probe = async () => ({ exited: true }); },
  ]) {
    const input = fixture(); alter(input);
    await assert.rejects(joinOwnedPty(input));
    assert.equal(input.recorded.length, 0);
  }
});

test('physical stream prearms, excludes partial lines and refuses rewrite', async () => {
  let bytes = '';
  let notify;
  const records = [];
  let closed = false;
  const stream = createOwnedPtyStream({ path: '/private/barriers/owned-pty-creation.receipt.jsonl',
    read: async () => Buffer.from(bytes),
    subscribe: (_directory, receive) => { notify = receive; return { close() { closed = true; } }; },
    emit: (record) => records.push(record) });
  assert.equal((await stream.cursor()).afterLine, -1);
  bytes = '{"pid":42}'; notify('change', 'owned-pty-creation.receipt.jsonl');
  await stream.drain(); assert.equal(records.length, 0);
  bytes += '\n'; notify('change', 'owned-pty-creation.receipt.jsonl');
  await stream.drain(); assert.equal(records[0].lineIndex, 0);
  bytes = ''; notify('change', 'owned-pty-creation.receipt.jsonl');
  await assert.rejects(stream.drain(), /truncated or rewrote/);
  stream.close(); assert.equal(closed, true);
});

function bindingEvents(input, signal) {
  const listeners = new Set();
  const cursor = { path: '/private/barriers/presentation.receipt.jsonl', afterLine: 4 };
  const subscription = prearmPresentationBinding({ cursor, runId: input.runId, operationId: input.operationId,
    signal, subscribe: (receive) => { listeners.add(receive); return () => listeners.delete(receive); } });
  return { ...subscription, listeners,
    emit(lineIndex, producerPid = input.gui.pid) {
      for (const receive of [...listeners]) receive({ kind: 'presentation', path: cursor.path, lineIndex,
        receipt: { ...input.binding, producerPid, presented: true, producerComponent: 'surface-host-gpu-render' } });
    } };
}

test('prearmed actual presentation plus owned receipt probes and records the complete tuple', async () => {
  const input = fixture();
  const controller = new AbortController();
  const events = bindingEvents(input, controller.signal);
  assert.equal(events.listeners.size, 1);
  events.emit(4);
  assert.equal(events.listeners.size, 1);
  events.emit(5);
  const binding = await events.promise;
  assert.equal(events.listeners.size, 0);
  const probes = [];
  const target = await joinOwnedPty({ ...input, binding, signal: controller.signal,
    probe: async ({ pid }) => { probes.push(pid); assert.equal(input.recorded.length, 0); return input.probe(); } });
  assert.deepEqual(probes, [42]);
  assert.deepEqual(target.attachTuple, input.binding.attachTuple);
  assert.deepEqual(input.recorded, [target]);
});

test('presentation emitted by another GUI refuses ownership with zero probes or records', async () => {
  const input = fixture();
  const events = bindingEvents(input, new AbortController().signal);
  events.emit(5, 99);
  let probes = 0;
  await assert.rejects(joinOwnedPty({ ...input, binding: await events.promise,
    probe: async () => { probes++; return input.probe(); } }), /frontend\/backend binding/);
  assert.equal(probes, 0);
  assert.equal(input.recorded.length, 0);
});

test('cancellation after exact probe starts prevents late ownership recording', async () => {
  const input = fixture();
  const controller = new AbortController();
  const events = bindingEvents(input, controller.signal);
  events.emit(5);
  let resolveProbe;
  let probeStarted;
  const started = new Promise((resolve) => { probeStarted = resolve; });
  const joined = joinOwnedPty({ ...input, binding: await events.promise, signal: controller.signal,
    probe: () => { probeStarted(); return new Promise((resolve) => { resolveProbe = resolve; }); } });
  const rejected = assert.rejects(joined, /cancelled/);
  await started;
  controller.abort(new Error('cancelled'));
  resolveProbe(await input.probe());
  await rejected;
  assert.equal(input.recorded.length, 0);
});

test('cancellation while presentation is pending removes the exact event subscription', async () => {
  const input = fixture();
  const controller = new AbortController();
  const events = bindingEvents(input, controller.signal);
  const rejected = assert.rejects(events.promise, /cancelled/);
  controller.abort(new Error('cancelled'));
  events.emit(5);
  await rejected;
  assert.equal(events.listeners.size, 0);
  assert.equal(input.recorded.length, 0);
});
