import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  DEFAULT_PAYLOAD_BYTES, FD_LIMIT, FootprintError, assertAlive, assertNoNulls, dirSizeBytes, isolationEnv,
  loadCommand, parseArgs, parseLsofCount, parseMacFootprint, parsePsRss, parseSmapsRollup, parseWindowsProcess,
  stageDaemon, validateFootprint, watchChild,
} from './run.mjs';

const RUN = fileURLToPath(new URL('./run.mjs', import.meta.url));
const COMMIT = '5d5499806a1b207849778f488b4e3e7b821a751b';

function withTmp(fn) {
  const dir = mkdtempSync(path.join(tmpdir(), 'ferryx-footprint-test-'));
  const done = () => rmSync(dir, { recursive: true, force: true });
  let result;
  try { result = fn(dir); } catch (e) { done(); throw e; }
  if (result && typeof result.then === 'function') return result.finally(done);
  done();
  return result;
}

const sample = (sessions, over = {}) => ({ sessions, guiRssBytes: 200e6, daemonRssBytes: 50e6, fdCount: 40 + sessions * 4, daemonRuntimeBytes: 30e6, ...over });
const doc = (over = {}) => ({ schema: 'ferryx-footprint/v1', commit: COMMIT, platform: 'linux', mode: 'isolated-daemon', timestamp: '2026-10-25T00:00:00.000Z', payloadBytes: 1, fdLimit: FD_LIMIT, samples: [sample(1), sample(10), sample(30)], ...over });

test('parseArgs defaults to 1,10,30 sessions, footprint.json and a 50 MiB payload', () => {
  const o = parseArgs(['--platform', 'linux', '--isolated-daemon', '--daemon-exe', '/x/ferryx'], { host: 'linux' });
  assert.deepEqual(o.sessions, [1, 10, 30]);
  assert.equal(o.out, 'footprint.json');
  assert.equal(o.payloadBytes, DEFAULT_PAYLOAD_BYTES);
  assert.equal(o.payloadBytes, 50 * 1024 * 1024);
  assert.equal(o.killGuiMidRun, false);
});

test('parseArgs reads every flag and sorts/dedupes --sessions', () => {
  const o = parseArgs(['--platform', 'macos', '--gui-exe', '/qa/ferryx', '--sessions', '30,1,10,10', '--out', 'x.json', '--kill-gui-mid-run'], { host: 'macos' });
  assert.deepEqual(o.sessions, [1, 10, 30]);
  assert.equal(o.out, 'x.json');
  assert.equal(o.killGuiMidRun, true);
  assert.equal(o.guiExe, '/qa/ferryx');
});

test('parseArgs refuses bad input with FOOTPRINT_USAGE', () => {
  const bad = [
    ['--platform', 'bsd', '--isolated-daemon', '--daemon-exe', 'x'],
    ['--platform', 'linux'],
    ['--platform', 'linux', '--isolated-daemon'],
    ['--platform', 'linux', '--isolated-daemon', '--daemon-exe', 'x', '--gui-exe', 'y'],
    ['--platform', 'linux', '--isolated-daemon', '--daemon-exe', 'x', '--sessions', '0,10'],
    ['--platform', 'linux', '--isolated-daemon', '--daemon-exe', 'x', '--sessions', 'a'],
    ['--platform', 'windows', '--isolated-daemon', '--daemon-exe', 'x'],
    ['--platform', 'linux', '--out'],
    ['--bogus'],
  ];
  for (const argv of bad) assert.throws(() => parseArgs(argv, { host: 'linux' }), (e) => e instanceof FootprintError && e.code === 'FOOTPRINT_USAGE', argv.join(' '));
});

test('samplers parse footprint, ps, smaps_rollup, lsof and PowerShell output', () => {
  assert.equal(parseMacFootprint('ferryx [4888]: 64-bit    Footprint: 1616 KB (16384 bytes per page)'), 1616 * 1024);
  assert.equal(parseMacFootprint('ferryx [1]: 64-bit    Footprint: 12.5 MB (16384 bytes per page)'), Math.round(12.5 * 1024 * 1024));
  assert.equal(parseMacFootprint('footprint: no such process'), null);
  assert.equal(parsePsRss('  20480\n'), 20480 * 1024);
  assert.equal(parsePsRss(''), null);
  const smaps = '55d0-7ffd ---p 00000000 00:00 0 [rollup]\nRss:              123456 kB\nPss:               65432 kB\nPss_Anon:          1 kB\n';
  assert.deepEqual(parseSmapsRollup(smaps), { rss: 123456 * 1024, pss: 65432 * 1024 });
  const lsof = [
    'COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME',
    'ferryx  100 indo  cwd    DIR   1,16      640    2 /',
    'ferryx  100 indo  txt    REG   1,16   123456    3 /x/ferryx',
    'ferryx  100 indo    0r   CHR    3,2      0t0  333 /dev/null',
    'ferryx  100 indo    1u   CHR   16,1      0t0  444 /dev/ttys001',
    'ferryx  100 indo   12u  unix 0x1234      0t0      /tmp/ffp-x/daemon.sock',
  ].join('\n');
  assert.equal(parseLsofCount(lsof), 3);
  assert.equal(parseLsofCount('lsof: no such pid'), null);
  assert.deepEqual(parseWindowsProcess('104857600 512\r\n'), { workingSet: 104857600, handles: 512 });
  assert.equal(parseWindowsProcess(''), null);
});

test('validateFootprint accepts a full document and refuses nulls, gaps and the fd ceiling', () => {
  assert.equal(validateFootprint(doc()).samples.length, 3);
  assert.throws(() => validateFootprint(doc({ samples: [sample(1, { guiRssBytes: null })] })), (e) => e.code === 'FOOTPRINT_NULL_FIELD' && /guiRssBytes/.test(e.message));
  assert.throws(() => validateFootprint(doc({ commit: null })), (e) => e.code === 'FOOTPRINT_NULL_FIELD');
  assert.throws(() => validateFootprint(doc({ samples: [{ sessions: 1, guiRssBytes: 1, daemonRssBytes: 1, fdCount: 1 }] })), (e) => e.code === 'FOOTPRINT_INVALID' && /daemonRuntimeBytes/.test(e.message));
  assert.throws(() => validateFootprint(doc({ samples: [] })), (e) => e.code === 'FOOTPRINT_INVALID');
  assert.throws(() => validateFootprint(doc({ samples: [sample(30, { fdCount: FD_LIMIT })] })), (e) => e.code === 'FOOTPRINT_FD_LIMIT');
  assert.throws(() => assertNoNulls({ a: [1, { b: Number.NaN }] }), (e) => e.code === 'FOOTPRINT_NULL_FIELD' && /a\[1\]\.b/.test(e.message));
});

test('loadCommand runs the payload and then marks it done', () => {
  assert.equal(loadCommand('linux', 52428800, '/tmp/d/0'), "yes | head -c 52428800; : > '/tmp/d/0'\n");
  assert.equal(loadCommand('macos', 10, "/tmp/it's/1"), "yes | head -c 10; : > '/tmp/it'\\''s/1'\n");
  assert.match(loadCommand('windows', 262144, 'C:\\t\\0'), /^1\.\.2 \| ForEach-Object .*New-Item -ItemType File -Force 'C:\\t\\0' \| Out-Null\r$/);
});

test('isolationEnv points every daemon location key at the run dirs', () => {
  const layout = { root: '/tmp/ferryx-footprint-AbC1', runtime: '/tmp/ffp-1', data: '/d', sessions: '/s' };
  const env = isolationEnv(layout, 'macos', { HOME: '/Users/x' });
  assert.equal(env.FERRYX_DATA_DIR, '/d');
  assert.equal(env.FERRYX_RUNTIME_DIR, '/tmp/ffp-1');
  assert.equal(env.FERRYX_SESSION_DIR, '/s');
  assert.equal(env.FERRYX_DAEMON_SUPERVISOR_NAME, 'ferryx-qa-footprint-ferryxfootprintabc1');
  assert.equal(env.FERRYX_DAEMON_PLIST_PATH, '/tmp/ferryx-footprint-AbC1/LaunchAgents');
  assert.equal(isolationEnv(layout, 'linux', {}).FERRYX_DAEMON_PLIST_PATH, undefined);
});

test('stageDaemon and dirSizeBytes measure the staged daemon-runtime generation', () => withTmp((dir) => {
  const exe = path.join(dir, 'ferryx');
  writeFileSync(exe, Buffer.alloc(1000, 1));
  const staged = stageDaemon(exe, path.join(dir, 'data'), '2026.1024.1');
  assert.match(staged, /daemon-runtime[\\/]2026\.1024\.1-[0-9a-f]{8}[\\/]ferryx$/);
  const runtime = path.join(dir, 'data', 'daemon-runtime');
  const size = dirSizeBytes(runtime);
  assert.equal(size, 1000 + readFileSync(path.join(path.dirname(staged), '.staged-at')).length);
  if (process.platform !== 'win32') {
    symlinkSync('/', path.join(runtime, 'loop'));
    assert.ok(dirSizeBytes(runtime) > size, 'a symlink counts as its own entry and is not followed');
  }
  assert.equal(dirSizeBytes(path.join(dir, 'absent')), 0);
}));

test('watchChild reports an armed child that exits as FOOTPRINT_SAMPLED_PROCESS_DIED', async () => {
  const child = spawn(process.execPath, ['-e', 'process.stdin.resume()'], { stdio: ['pipe', 'ignore', 'ignore'] });
  const state = watchChild(child, 'gui companion');
  assert.doesNotThrow(() => assertAlive([state]));
  child.kill('SIGKILL');
  await assert.rejects(state.died, (e) => e.code === 'FOOTPRINT_SAMPLED_PROCESS_DIED' && /gui companion pid \d+ exited/.test(e.message));
  assert.throws(() => assertAlive([state]), (e) => e.code === 'FOOTPRINT_SAMPLED_PROCESS_DIED');
  state.armed = false;
  assert.doesNotThrow(() => assertAlive([state]));
});

// ---- end to end against a fake isolated daemon (data protocol v5 subset over a unix socket) ----

const FAKE_DAEMON = `#!/usr/bin/env node
const net = require('node:net');
const { spawn } = require('node:child_process');
const path = require('node:path');
if (process.argv[2] !== '--daemon') process.exit(2);
const runtime = process.env.FERRYX_RUNTIME_DIR;
if (!runtime || !process.env.FERRYX_DATA_DIR || !process.env.FERRYX_SESSION_DIR) process.exit(3);
const sessions = new Map();
let next = 0;
const server = net.createServer((socket) => {
  let buf = '';
  socket.setEncoding('utf8');
  socket.on('error', () => {});
  socket.on('data', (chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf('\\n')) >= 0) {
      const req = JSON.parse(buf.slice(0, i));
      buf = buf.slice(i + 1);
      const send = (v) => socket.write(JSON.stringify(v) + '\\n');
      if (req.type === 'handshake') send({ type: 'handshakeOk', version: 5, pid: process.pid, epoch: 1 });
      else if (req.type === 'registerWorkspace') send({ type: 'registerWorkspaceOk' });
      else if (req.type === 'spawn') {
        const id = 's' + next++;
        const sh = spawn('/bin/sh', [], { cwd: req.cwd, stdio: ['pipe', 'ignore', 'ignore'] });
        sessions.set(id, sh);
        send({ type: 'spawnOk', sessionId: id, epoch: 1, session: {} });
      } else if (req.type === 'write') { sessions.get(req.sessionId).stdin.write(Buffer.from(req.data, 'base64')); send({ type: 'writeOk' }); }
      else if (req.type === 'attach') send({ type: 'attachOk', epoch: 1, sessionId: req.sessionId, history: '' });
      else if (req.type === 'shutdown') { for (const sh of sessions.values()) sh.kill('SIGKILL'); process.exit(0); }
      else send({ type: 'error', message: 'unsupported ' + req.type });
    }
  });
});
server.listen(path.join(runtime, 'daemon.sock'), () => console.log('FERRYX_DAEMON_READY'));
`;

function runBench(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [RUN, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.once('error', reject);
    child.once('close', (status) => resolve({ status, stdout, stderr }));
  });
}

const unixOnly = { skip: process.platform === 'win32' ? 'fake daemon listens on a unix socket' : false, timeout: 120_000 };
const host = { darwin: 'macos', linux: 'linux' }[process.platform];

test('isolated-daemon run writes footprint.json with no null field', unixOnly, () => withTmp(async (dir) => {
  const exe = path.join(dir, 'ferryx');
  writeFileSync(exe, FAKE_DAEMON);
  chmodSync(exe, 0o755);
  const out = path.join(dir, 'footprint.json');
  const r = await runBench(['--platform', host, '--isolated-daemon', '--daemon-exe', exe, '--sessions', '1,3', '--payload-bytes', '4096', '--out', out, '--commit', COMMIT]);
  assert.equal(r.status, 0, r.stderr);
  const fp = JSON.parse(readFileSync(out, 'utf8'));
  validateFootprint(fp);
  assert.equal(fp.commit, COMMIT);
  assert.equal(fp.platform, host);
  assert.equal(fp.mode, 'isolated-daemon');
  assert.deepEqual(fp.samples.map((s) => s.sessions), [1, 3]);
  for (const s of fp.samples) {
    assert.ok(s.guiRssBytes > 0 && s.daemonRssBytes > 0 && s.fdCount > 0 && s.fdCount < FD_LIMIT, JSON.stringify(s));
    assert.ok(s.daemonRuntimeBytes >= FAKE_DAEMON.length, 'the staged executable is counted');
  }
}));

test('--kill-gui-mid-run makes the benchmark exit non-zero without footprint.json', unixOnly, () => withTmp(async (dir) => {
  const exe = path.join(dir, 'ferryx');
  writeFileSync(exe, FAKE_DAEMON);
  chmodSync(exe, 0o755);
  const out = path.join(dir, 'footprint.json');
  const r = await runBench(['--platform', host, '--isolated-daemon', '--daemon-exe', exe, '--sessions', '1,3', '--payload-bytes', '4096', '--out', out, '--commit', COMMIT, '--kill-gui-mid-run']);
  assert.equal(r.status, 1, r.stderr);
  assert.match(r.stderr, /FOOTPRINT_SAMPLED_PROCESS_DIED: gui companion pid \d+ exited mid-run/);
  assert.equal(existsSync(out), false);
}));

test('a usage error exits 2', async () => {
  const r = await runBench(['--platform', 'bsd']);
  assert.equal(r.status, 2);
  assert.match(r.stderr, /FOOTPRINT_USAGE/);
});
