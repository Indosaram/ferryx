import { expect, test } from 'bun:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { exitWatchSource } from './f1-exit-watch.mjs';

const fallback = { pidfd: 'False', kqueue: 'False' };
const stampOf = (pid) => readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ').at(-1).trim().split(/\s+/)[19];

async function watch(pid, stamp, { timeout = 1, options = fallback, armed = () => {}, signal = 'none' } = {}) {
  const child = spawn('python3', ['-u', '-c', exitWatchSource(options), String(pid), stamp, String(timeout), signal]);
  const lines = []; let stderr = ''; let action = Promise.resolve();
  const closed = once(child, 'close', { signal: AbortSignal.timeout(5000) });
  const reader = createInterface({ input: child.stdout });
  reader.on('line', (line) => {
    lines.push(line);
    if (line === 'ARMED') action = Promise.resolve().then(armed);
  });
  child.stderr.on('data', (data) => { stderr += data; });
  try {
    const [code] = await closed;
    await action;
    return { code, lines, stderr };
  } finally { reader.close(); child.kill('SIGKILL'); }
}

async function withChild(run) {
  const child = spawn('/bin/cat', [], { stdio: ['pipe', 'ignore', 'ignore'] });
  const closed = once(child, 'close');
  await once(child, 'spawn');
  try { return await run(child, closed); }
  finally { child.kill('SIGKILL'); await closed; }
}

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), 'f1-watch-'));
  mkdirSync(join(root, '42'));
  mkdirSync(join(root, 'self'));
  writeFileSync(join(root, 'self/stat'), readFileSync('/proc/self/stat'));
  const stat = (stamp) => writeFileSync(join(root, '42/stat'), `42 (name with ) spaces) S ${Array(18).fill('0').join(' ')} ${stamp}\n`);
  stat('123');
  return Promise.resolve().then(() => run(root, stat)).finally(() => rmSync(root, { recursive: true, force: true }));
}

test('proc capability selection without pidfd or kqueue', async () => {
  // Inject false capability expressions, not a monkeypatched Python interpreter.
  const result = await watch(process.pid, stampOf(process.pid), { timeout: 0.02 });
  expect(result.code).toBe(0);
  expect(result.stderr.trim()).toBe('WATCH_VIA=proc');
  expect(result.lines).toEqual(['ARMED', 'TIMEOUT']);
});

test('proc real child exit after arming', async () => {
  await withChild(async (child, closed) => {
    const result = await watch(child.pid, stampOf(child.pid), { armed: async () => { child.stdin.end(); await closed; } });
    expect(result.code).toBe(0);
    expect(result.lines).toEqual(['ARMED', 'EXIT']);
  });
});

test('proc reused pid is not armed or signalled as the recorded process', async () => {
  await withChild(async (child) => {
    // A live different identity at the same numeric pid simulates reuse; no PID wrap is claimed.
    const result = await watch(child.pid, String(BigInt(stampOf(child.pid)) + 1n), { signal: 'SIGKILL' });
    expect(result.code).toBe(0);
    expect(result.lines).toEqual(['GONE']);
    expect(child.exitCode).toBeNull();
    expect(stampOf(child.pid)).toBeTruthy();
  });
});

test('proc deadline is TIMEOUT not false EXIT', async () => {
  const result = await watch(process.pid, stampOf(process.pid), { timeout: 0.03 });
  expect(result.code).toBe(0);
  expect(result.lines).toEqual(['ARMED', 'TIMEOUT']);
});

test('proc start stamp change ends only the original identity wait', async () => {
  await fixture(async (root, stat) => {
    const result = await watch(42, '123', { options: { ...fallback, procRoot: root }, armed: () => stat('456') });
    expect(result.code).toBe(0);
    expect(result.lines).toEqual(['ARMED', 'EXIT']);
    expect(readFileSync(join(root, '42/stat'), 'utf8').trim().endsWith('456')).toBe(true);
  });
});

test('proc absent pid is GONE but missing proc mount fails loudly', async () => {
  await fixture(async (root) => {
    rmSync(join(root, '42'), { recursive: true });
    const absent = await watch(42, '123', { options: { ...fallback, procRoot: root } });
    expect(absent.code).toBe(0);
    expect(absent.lines).toEqual(['GONE']);
    rmSync(join(root, 'self'), { recursive: true });
    const unobservable = await watch(42, '123', { options: { ...fallback, procRoot: root } });
    expect(unobservable.code).not.toBe(0);
    expect(unobservable.lines).toEqual([]);
  });
});

test('proc malformed identity fails loudly', async () => {
  await fixture(async (root) => {
    writeFileSync(join(root, '42/stat'), 'not a stat record');
    const result = await watch(42, '123', { options: { ...fallback, procRoot: root } });
    expect(result.code).not.toBe(0);
    expect(result.lines).toEqual([]);
  });
});

test('proc host-helper signal occurs after arming', async () => {
  await withChild(async (child) => {
    const result = await watch(child.pid, stampOf(child.pid), { signal: 'SIGTERM' });
    expect(result.code).toBe(0);
    expect(result.lines).toEqual(['ARMED', 'EXIT']);
  });
});
