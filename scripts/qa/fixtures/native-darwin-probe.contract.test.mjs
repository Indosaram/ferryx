import { test } from 'node:test';
import assert from 'node:assert/strict';
import { probeDarwin } from './native-remote-agent.mjs';

test('Darwin exact probe uses candidate CLI and preserves full start token', async () => {
  const calls = [];
  const actual = await probeDarwin(42, '/isolated/ferryx', undefined, async (...args) => {
    calls.push(args);
    return JSON.stringify({ exited: false, executable: '/actual/shell', processStart: '1700000000:000007' });
  });
  assert.deepEqual(calls, [['/isolated/ferryx', ['qa-exact-pid-probe', '42'], undefined]]);
  assert.deepEqual(actual, { exited: false, executable: '/actual/shell', processStart: '1700000000:000007' });
});

test('Darwin probe refuses rounded tokens, absent paths and unreadable identity', async () => {
  for (const value of [
    { exited: false, executable: '/actual/shell', processStart: '1700000000' },
    { exited: false, executable: null, processStart: '1700000000:000007' },
    { exited: true },
  ]) {
    await assert.rejects(probeDarwin(42, '/isolated/ferryx', undefined, async () => JSON.stringify(value)));
  }
  await assert.rejects(probeDarwin(42, '/isolated/ferryx', undefined, async () => { throw new Error('permission denied'); }), /permission denied/);
});
