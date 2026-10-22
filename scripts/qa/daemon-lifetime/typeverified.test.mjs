import test from 'node:test';
import assert from 'node:assert/strict';
import { typeVerified, pressChar, renderEcho } from './lib/typeverified.mjs';
import { createOutputRoute } from './lib/agent.mjs';

/**
 * A MahoRD feeding an echoing canonical tty (Backspace echoes "\\b \\b"), with omarchy's faults by
 * send number: `drops` are lost; `repeats` maps a send to extra copies of its key (MahoRD repeated
 * it); `late` echoes land just after the wait that timed out (before the harness re-reads);
 * `later` echoes land only after the next send (a slow echo overtaken by a resend).
 * waitMore answers at once, like conn.waitOutput once its deadline passes.
 */
function lossyTerminal({ drops = [], repeats = new Map(), late = [], later = [] } = {}) {
  let echo = '';
  let sends = 0;
  let afterWait = '';
  let afterSend = '';
  const deliver = (ch) => {
    sends += 1;
    echo += afterSend;
    afterSend = '';
    if (drops.includes(sends)) return;
    const out = (ch === 'Backspace' ? '\b \b' : ch).repeat(1 + (repeats.get(sends) ?? 0));
    if (late.includes(sends)) afterWait += out;
    else if (later.includes(sends)) afterSend += out;
    else echo += out;
  };
  const mahord = {
    key: async (k) => deliver(k),
    hotkey: async (keys) => { assert.deepEqual(keys.slice(0, 1), ['Shift']); deliver(keys[1].toUpperCase()); },
    type: async () => { throw new Error('a burst is not used: MahoRD drops keys from it'); },
  };
  const waitMore = async (length) => {
    if (echo.length > length) return { timedOut: false };
    echo += afterWait;
    afterWait = '';
    return { timedOut: true };
  };
  return { mahord, echo: async () => echo, waitMore, raw: () => echo, sends: () => sends };
}

test('every key of a mixed-case marker arrives once, even when MahoRD drops some sends', async () => {
  const t = lossyTerminal({ drops: [2, 3, 9] }); // the 2nd key twice, and a later one, are lost
  const r = await typeVerified({ ...t, text: 'UNCERTAIN653F0E33D74A' });
  assert.equal(renderEcho(t.raw()), 'UNCERTAIN653F0E33D74A');
  assert.equal(r.line, 'UNCERTAIN653F0E33D74A');
  assert.equal(r.resent, 3);
  assert.equal(r.sends[1], 3);
});

test('a key that never echoes fails with its index instead of passing a short marker', async () => {
  const t = lossyTerminal({ drops: [5, 6, 7] });
  await assert.rejects(typeVerified({ ...t, text: 'UNCERTAIN', attempts: 3 }), (e) => e.code === 'TYPE_NOT_ECHOED' && e.detail.index === 4 && e.detail.char === 'R');
});

test('a key MahoRD repeated is erased with Backspace before the next key (run-int2-all Q11: UNCERRRRR)', async () => {
  const t = lossyTerminal({ repeats: new Map([[5, 4]]) }); // the 5th send, R, echoed five times
  const r = await typeVerified({ ...t, text: 'UNCERTAIN' });
  assert.equal(renderEcho(t.raw()), 'UNCERTAIN');
  assert.equal(r.line, 'UNCERTAIN');
  assert.equal(r.erased, 4);
  assert.equal(r.resent, 0);
});

test('an echo that lands just after its wait timed out is accepted, not resent', async () => {
  const t = lossyTerminal({ late: [3] });
  const r = await typeVerified({ ...t, text: 'UNCERTAIN' });
  assert.equal(r.line, 'UNCERTAIN');
  assert.equal(r.resent, 0);
  assert.equal(r.erased, 0);
  assert.equal(t.sends(), 9);
});

test('a slow echo overtaken by its resend is erased back to one copy', async () => {
  const t = lossyTerminal({ later: [3] }); // C's echo lands only after the resend went out
  const r = await typeVerified({ ...t, text: 'UNCERTAIN' });
  assert.equal(renderEcho(t.raw()), 'UNCERTAIN');
  assert.equal(r.line, 'UNCERTAIN');
  assert.equal(r.resent, 1);
  assert.equal(r.erased, 1);
});

test('renderEcho applies the tty erase echo and drops escape sequences', () => {
  assert.equal(renderEcho('UNCERRRRR\b \b\b \b\b \b\b \bT'), 'UNCERT');
  assert.equal(renderEcho('\x1b[?25lAB\x1b[0mC'), 'ABC');
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

