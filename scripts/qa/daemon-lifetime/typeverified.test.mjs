import test from 'node:test';
import assert from 'node:assert/strict';
import { typeVerified, pressChar } from './lib/typeverified.mjs';
import { createOutputRoute } from './lib/agent.mjs';

/**
 * A MahoRD that loses the keys listed in `drops` (by send number) the way omarchy's virtual
 * keyboard did, feeding an echoing terminal. waitEcho answers at once from that echo, like
 * conn.waitOutput once its deadline passes.
 */
function lossyTerminal(drops = new Set()) {
  let echo = '';
  let sends = 0;
  const deliver = (ch) => { sends += 1; if (!drops.has(sends)) echo += ch; };
  const mahord = {
    key: async (k) => deliver(k),
    hotkey: async (keys) => { assert.deepEqual(keys.slice(0, 1), ['Shift']); deliver(keys[1].toUpperCase()); },
    type: async () => { throw new Error('a burst is not used: MahoRD drops keys from it'); },
  };
  return { mahord, waitEcho: async (prefix) => ({ timedOut: !echo.includes(prefix) }), echo: () => echo, sends: () => sends };
}

test('every key of a mixed-case marker arrives once, even when MahoRD drops some sends', async () => {
  const t = lossyTerminal(new Set([2, 3, 9])); // the 2nd key twice, and a later one, are lost
  const r = await typeVerified({ mahord: t.mahord, waitEcho: t.waitEcho, text: 'UNCERTAIN653F0E33D74A' });
  assert.equal(t.echo(), 'UNCERTAIN653F0E33D74A');
  assert.equal(r.resent, 3);
  assert.equal(r.sends[1], 3);
});

test('a key that never echoes fails with its index instead of passing a short marker', async () => {
  const t = lossyTerminal(new Set([5, 6, 7]));
  await assert.rejects(typeVerified({ mahord: t.mahord, waitEcho: t.waitEcho, text: 'UNCERTAIN', attempts: 3 }), (e) => e.code === 'TYPE_NOT_ECHOED' && e.detail.index === 4 && e.detail.char === 'R');
});

test('uppercase letters are sent as Shift+letter, other characters as a plain key', async () => {
  const calls = [];
  const mahord = { key: async (k) => calls.push(['key', k]), hotkey: async (k) => calls.push(['hotkey', k]) };
  for (const ch of 'A9f') await pressChar(mahord, ch);
  assert.deepEqual(calls, [['hotkey', ['Shift', 'a']], ['key', '9'], ['key', 'f']]);
});

test('an output waiter with since ignores output that was already there (the typed prefix is not pre-matched)', () => {
  const output = new Map([['s1', 'cat >> /run/ssh-repo/ssh-input.log\r\n']]);
  const waiters = new Set();
  const { route } = createOutputRoute({ platform: 'linux', output, waiters, send: async () => {} });
  const hits = [];
  // "s" is already in the old output; with since it must wait for new output.
  waiters.add({ sessionId: 's1', marker: 's', visible: false, since: output.get('s1').length, resolve: (t) => hits.push(t) });
  route({ type: 'output', sessionId: 's1', data: Buffer.from('U').toString('base64') });
  assert.deepEqual(hits, []);
  route({ type: 'output', sessionId: 's1', data: Buffer.from('s').toString('base64') });
  assert.deepEqual(hits, ['Us']);
});
