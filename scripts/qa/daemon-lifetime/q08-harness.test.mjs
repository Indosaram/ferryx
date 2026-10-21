import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createAgent } from './lib/agent.mjs';
import { counterTags, mergeRoutedSessions, sshOwnerProbeTarget, installV1QaEnv, nextCounterTag, restartCounters, faultTarget, pairSessions, seedPair, successorDataConn, describeWithRoutes, ownedStagePattern, commitEvidence } from './scenarios/q07-q09.mjs';
import { createJobs, qaEnvDropIn, QA_UNIT_ENV_KEYS } from './lib/jobs.mjs';
import { Q11, Q12 } from './scenarios/q10-q13.mjs';
import { daemonLogFile, teardownRun } from './run.mjs';
import { stagePattern } from './lib/receipts.mjs';
import { newSummary } from './lib/summary.mjs';

const RUN = '20261021t000000z';

test('fix 1: upgrade steps wait for the counters seedPair started, not the step label', () => {
  // run-int1: seedPair(..., 'Q08') ran Q08_L/Q08_S while upgradeStep waited for A_B_L/B_C_S.
  assert.deepEqual(counterTags({ tag: 'Q08' }), { local: 'Q08_L', ssh: 'Q08_S' });
});

test('fix 2: the SSH-owning generation is probed, not the step predecessor', () => {
  // B_C: the step predecessor is B (906546) but the SSH session stays on A.
  assert.equal(sshOwnerProbeTarget({ sshOwnerPid: 900001 }, { pid: 906546 }), 900001);
});

test('fix 3: waitLog with fromLine ignores receipts written before the offset', async () => {
  const runDir = mkdtempSync(path.join(os.tmpdir(), 'q08-harness-'));
  const file = path.join(runDir, 'authority.jsonl');
  const stale = '{"producerPid":906546,"settledAtMs":1,"stage":"candidate-admitted"}';
  writeFileSync(file, `${stale}\n`);
  const { methods } = createAgent({ platform: 'linux', runId: RUN, runDir });
  const pattern = '"producerPid":(?!918583,)\\d+,[^\\n]*"stage":"candidate-admitted"';
  assert.equal((await methods.waitLog({ file, pattern, deadlineMs: 200 })).matched?.includes('906546'), true, 'the unscoped wait picks up the stale pid');
  assert.equal((await methods.waitLog({ file, pattern, fromLine: 1, deadlineMs: 200 })).timedOut, true);
  appendFileSync(file, '{"producerPid":934793,"settledAtMs":2,"stage":"candidate-admitted"}\n');
  const fresh = await methods.waitLog({ file, pattern, fromLine: 1, deadlineMs: 2_000 });
  assert.match(fresh.matched, /934793/);
});

test('fix 4: Q11 and Q12 block instead of dereferencing a missing data connection', async () => {
  const q11 = await Q11({ hooks: { writeAckHoldArm: { present: true } }, mahord: {}, sshd: { workspaceId: 'w' }, state: { conn: null } }, { assertions: [] });
  assert.equal(q11.status, 'BLOCKED');
  assert.equal(q11.barrier.kind, 'NO_DATA_CONNECTION');
  const q12 = await Q12({ state: { conn: null } }, { assertions: [] });
  assert.equal(q12.status, 'BLOCKED');
  assert.equal(q12.barrier.kind, 'NO_DATA_CONNECTION');
});

test('item 5: a session routed to a draining predecessor is described by that predecessor', async () => {
  // run-int1 A_B: B described only the local session; SSH 611cec90 lives on A behind the route.
  const own = { sessions: [{ backendId: 'local-1', kind: 'local' }] };
  const manifest = JSON.stringify({ routes: [{ legacySocketPath: '/run/legacy-1-1.sock', sessions: ['ssh-611'] }] });
  const asked = [];
  const merged = await mergeRoutedSessions(own, manifest, async (socket) => {
    asked.push(socket);
    return { sessions: [{ backendId: 'ssh-611', kind: 'ssh', remotePid: 4242 }, { backendId: 'other', kind: 'ssh' }] };
  });
  assert.deepEqual(asked, ['/run/legacy-1-1.sock']);
  assert.deepEqual(merged.sessions.map((s) => s.backendId), ['local-1', 'ssh-611']);
  assert.equal(merged.sessions[1].remotePid, 4242);
  assert.deepEqual((await mergeRoutedSessions(own, null, async () => { throw new Error('no route'); })).sessions, own.sessions);
});

test('teardown copies the QA daemons\' daemon.log into the evidence before the run dir is removed', async () => {
  // run-int1/run-q08: <FERRYX_DATA_DIR>/logs/daemon.log was outside layout.logs and was deleted.
  const evidence = mkdtempSync(path.join(os.tmpdir(), 'q08-evidence-'));
  const layout = { logs: '/r/logs', data: '/r/data' };
  assert.equal(daemonLogFile('linux', layout), '/r/data/logs/daemon.log');
  const agent = { call: async (method, params) => {
    if (method === 'exec') return { status: 0, stdout: '[]', stderr: '' };
    if (method === 'readFile') return params.file === '/r/data/logs/daemon.log' ? 'Handover transfer aborted\n' : null;
    if (method === 'teardownRecorded') return { killed: [], refused: [] };
    if (method === 'table') return [];
    if (method === 'exists') return false;
    return null;
  } };
  const summary = newSummary({ runId: RUN, platform: 'linux', host: 'h' });
  summary.releaseDaemon.before = 'owner';
  await teardownRun({ summary, agent, jobs: { listQaJobs: async () => [], remove: async () => {} }, host: { platform: 'linux' }, layout, runDirs: [], qaRoots: [], localEvidence: evidence, prodSnapshot: async () => 'owner' });
  const copied = path.join(evidence, 'runlog-daemon.log');
  assert.equal(existsSync(copied), true);
  assert.equal(readFileSync(copied, 'utf8'), 'Handover transfer aborted\n');
});

test('Q08b Linux: every generation unit of the run gets the QA env through one prefix drop-in, and teardown removes it', async () => {
  // run-q08 rerun daemon.log 17:53:40: the v1bump owner's unit lacked FERRYX_DAEMON_SUPERVISOR_NAME,
  // so RequestUpgrade was refused with "No daemon supervisor is supported in this runtime".
  const env = { FERRYX_DAEMON_SUPERVISOR_NAME: 'ferryx-qa-run', FERRYX_QA_BARRIER_DIR: '/r/barriers', FERRYX_QA_RUN_ID: 'run', FERRYX_QA_SSH_KNOWN_HOSTS: '/r/kh', FERRYX_DATA_DIR: '/r/data' };
  const dropIn = qaEnvDropIn(env);
  assert.match(dropIn, /^\[Service\]\n/);
  for (const key of QA_UNIT_ENV_KEYS) assert.ok(dropIn.includes(`Environment="${key}=${env[key]}"`), key);
  assert.ok(!dropIn.includes('FERRYX_DATA_DIR'), 'only QA-only variables; the unit renders the rest itself');
  assert.throws(() => qaEnvDropIn({ FERRYX_QA_RUN_ID: 'run' }), /SUPERVISOR_NAME/);
  const calls = [];
  const agent = { call: async (method, params) => { calls.push([method, params]); return { status: 0, stdout: '', stderr: '' }; } };
  const jobs = createJobs({ host: { platform: 'linux' }, agent, runId: RUN, layout: { logs: '/r/logs' } });
  // rerun2b: the v1bump unit is <base>-2026.1013.2-<sha>, not the HEAD-version key, so a per-key
  // drop-in missed it; systemd's "<base>-.service.d" applies to every "<base>-*.service".
  const out = await installV1QaEnv({ keys: { v1bump: 'K1', A: 'KA' }, jobs, isolationEnv: env });
  assert.equal(out.dropIn.status, 0);
  const written = calls.filter(([, p]) => p.input).map(([, p]) => p.args.at(-1));
  assert.deepEqual(written, [`${jobs.base}-.service`]);
  assert.match(calls[0][1].args[1], /\$1\.d"; mkdir -p "\$d" && cat > "\$d\/qa-env\.conf"/);
  await jobs.remove(`${jobs.unit('K1')}.service`);
  const rm = calls.at(-1)[1];
  assert.match(rm.args[1], /rm -rf -- "\$d\/\$2"/);
  assert.deepEqual(rm.args.slice(-2), [`${jobs.unit('K1')}.service`, `${jobs.base}-.service.d`]);
});

test('rerun2 Q08 B_C: an upgrade step restarts the counters its nonce round trip interrupted', async () => {
  assert.equal(nextCounterTag('Q08'), 'Q08R1');
  assert.equal(nextCounterTag('Q08R1'), 'Q08R2');
  const writes = [];
  const conn = {
    call: async (frame) => {
      if (frame.type === 'attach') { writes.push([frame.sessionId, 'attach']); return { type: 'attachOk' }; }
      writes.push([frame.sessionId, Buffer.from(frame.data, 'base64').toString()]);
      return { type: 'writeOk' };
    },
    waitOutput: async (sid, marker) => ({ text: marker, count: 1 }),
  };
  const pair = { local: 'L', ssh: 'S', tag: 'Q08' };
  await restartCounters({ platform: 'linux' }, conn, pair);
  assert.equal(pair.tag, 'Q08R1');
  assert.deepEqual(counterTags(pair), { local: 'Q08R1_L', ssh: 'Q08R1_S' });
  // Attach (a replaced connection never streamed the pair), Ctrl-C whatever counter runs there,
  // then the fresh counters.
  const kind = (d) => (d === 'attach' ? 'attach' : d === '\u0003' ? '^C' : 'counter');
  assert.deepEqual(writes.map(([sid, d]) => [sid, kind(d)]), [['L', 'attach'], ['S', 'attach'], ['L', '^C'], ['S', '^C'], ['L', 'counter'], ['S', 'counter']]);
  assert.match(writes[4][1], /echo Q08R1_L_TICK_/);
  const stalled = { ...conn, waitOutput: async () => ({ timedOut: true }) };
  await assert.rejects(restartCounters({ platform: 'linux' }, stalled, { local: 'L', ssh: 'S', tag: 'X' }), /no output/);
});

test('rerun2 abort-after-adopt: a fault variant targets a generation that is neither the owner nor running', async () => {
  const running = new Set();
  const ctx = { keys: { A: 'KA', B: 'KB', C: 'KC' }, artifacts: { A: { name: 'A' }, B: { name: 'B' }, C: { name: 'C' } },
    jobs: { inspect: async (key) => ({ state: running.has(key) ? 'Running' : 'inactive' }) } };
  assert.equal((await faultTarget(ctx, { hello: { generation_key: 'KC' } }, ctx.artifacts.B)).name, 'B');
  // after-record recovery made B the owner (refused "already running this generation"), and C
  // still drains under its own unit ("successor unit ... is already active", rerun2b).
  running.add('KB'); running.add('KC');
  assert.equal((await faultTarget(ctx, { hello: { generation_key: 'KB' } }, ctx.artifacts.B)).name, 'A');
});

test('rerun2c Q08b (Unix): the old owner is probed and seeded before any HEAD GUI launches', async () => {
  // A HEAD GUI upgrades a protocol-6 owner on its own (update_pending::resolve_mismatch), so in
  // rerun2c the mismatch probe and control Hello ran after the upgrade and met the HEAD owner.
  const src = readFileSync(new URL('./scenarios/q07-q09.mjs', import.meta.url), 'utf8');
  const body = src.slice(src.indexOf('export async function Q08b'), src.indexOf('export async function Q09'));
  const at = (needle) => { const i = body.indexOf(needle); assert.ok(i >= 0, needle); return i; };
  assert.ok(at("'dataOpen'") < at('seedPair('), 'mismatch probe before the seed');
  assert.ok(at('ctl.hello()') < at('seedPair('));
  const unix = body.slice(body.indexOf('} else {'));
  assert.ok(!unix.includes('ctx.gui.launch'), 'the Unix branch launches no HEAD GUI before upgradeStep');
  assert.ok(unix.includes('guiDriven: true'));
});

test('rerun2d Q08b: a local-only pair (no SSH leg) is seeded, restarted and asserted without an SSH session', async () => {
  // handover_v1_rev has no FERRYX_QA_SSH_KNOWN_HOSTS pin, so its ssh cannot verify the private sshd.
  assert.deepEqual(pairSessions({ local: 'L', ssh: null }), ['L']);
  assert.deepEqual(pairSessions({ local: 'L', ssh: 'S' }), ['L', 'S']);
  const frames = [];
  const conn = {
    call: async (frame) => { frames.push(frame); return frame.type === 'attach' ? { type: 'attachOk' } : { type: 'writeOk' }; },
    waitOutput: async (sid, marker) => ({ text: marker, count: 1 }),
  };
  const agent = { call: async (m) => (m === 'controlOpen' ? { handle: null } : m === 'readFile' ? JSON.stringify({ pid: 7 }) : m === 'probe' ? { executable: '/x' } : null) };
  const ctx = { platform: 'linux', state: { localA: 'L' }, layout: { runtime: '/r/run' }, agent };
  const pair = await seedPair(ctx, conn, 'Q8B', { ssh: false });
  assert.deepEqual(pair, { local: 'L', ssh: null, tag: 'Q8B', sshOwnerPid: null });
  assert.ok(!frames.some((f) => f.type === 'spawn'), 'no SSH spawn');
  frames.length = 0;
  await restartCounters(ctx, conn, pair);
  assert.ok(frames.every((f) => f.sessionId === 'L'), 'restart touches only the local session');
  const src = readFileSync(new URL('./scenarios/q07-q09.mjs', import.meta.url), 'utf8');
  const q8b = src.slice(src.indexOf('export async function Q08b'), src.indexOf('export async function Q09'));
  assert.match(q8b, /seedPair\(ctx, ctx\.state\.conn, 'Q8B', \{ ssh: false \}\)/);
});

test('rerun2e Q08b: successorDataConn skips accepts that land on the draining v1 predecessor', async () => {
  // 24cbfd25 keeps accepting on the shared public data listener after commit; such a connect is
  // refused with protocolMismatch (data handshake refused), so take the next accept.
  const opens = [{ refuse: true }, { pid: 11 }, { pid: 22 }];
  let closed = 0;
  const files = [];
  const agent = {
    call: async (m) => {
      if (m === 'dataOpen') {
        const next = opens.shift();
        if (next.refuse) throw new Error('data handshake refused: {"type":"protocolMismatch"}');
        return { handle: next.pid, handshake: { type: 'handshakeOk', pid: next.pid } };
      }
      if (m === 'close') { closed += 1; return null; }
      return null;
    },
  };
  const ctx = { layout: { runtime: '/r' }, agent, evidenceDir: '/e', writeEvidence: async (f, c) => files.push([f, c]) };
  const conn = await successorDataConn(ctx, { id: 'Q08b', evidence: [] }, 'V1_HEAD', 22, 5);
  assert.equal(conn.handshake.pid, 22);
  assert.equal(closed, 1, 'the non-successor connection is closed');
  assert.match(files[0][1], /protocolMismatch/);
  opens.push({ pid: 11 }, { pid: 11 });
  await assert.rejects(successorDataConn(ctx, { id: 'Q08b', evidence: [] }, 'V1_HEAD', 22, 2), /no data connection reached the successor 22/);
});

test('rerun2f/2g Q08b: each draining route is described on the protocol its own daemon expects', async () => {
  const opened = [];
  const expects = { '/r/head.sock': 5, '/r/v1.sock': 6 };
  const agent = {
    call: async (m, p) => {
      if (m === 'readFile') return JSON.stringify({ routes: [{ legacySocketPath: '/r/head.sock', sessions: ['H'] }, { legacySocketPath: '/r/v1.sock', sessions: ['V'] }] });
      if (m === 'dataOpen') {
        opened.push([p.socketPath, p.version ?? 5]);
        const want = expects[p.socketPath];
        if ((p.version ?? 5) === want) return { handle: p.socketPath, handshake: { type: 'handshakeOk' } };
        if (!p.allowMismatch) throw new Error('data handshake refused');
        return { handle: 0, handshake: { type: 'protocolMismatch', expectedVersion: want, receivedVersion: p.version ?? 5 } };
      }
      if (m === 'call') return { sessions: [{ backendId: p.handle === '/r/head.sock' ? 'H' : 'V' }] };
      return null;
    },
  };
  const ctx = { layout: { runtime: '/r' }, agent };
  const own = { describe: async () => ({ sessions: [] }) };
  const merged = await describeWithRoutes(ctx, own);
  assert.deepEqual(opened, [['/r/head.sock', 5], ['/r/v1.sock', 5], ['/r/v1.sock', 6]]);
  assert.deepEqual(merged.sessions.map((s) => s.backendId).sort(), ['H', 'V']);
});

test('rerun2h Q08b: only the GUI-driven v1 step may own through authority-recovered (M3a)', () => {
  const line = (pid, stage) => `{"producerPid":${pid},"settledAtMs":1,"stage":"${stage}"}`;
  const owns = (guiDriven, pid, stage) => new RegExp(stagePattern(42, ownedStagePattern(guiDriven))).test(line(pid, stage));
  assert.ok(owns(true, 42, 'authority-recovered'));
  assert.ok(owns(true, 42, 'authority-adopted'));
  assert.ok(!owns(true, 43, 'authority-recovered'), 'another process never counts');
  assert.ok(owns(false, 42, 'authority-adopted'));
  assert.ok(!owns(false, 42, 'authority-recovered'), 'a HEAD-to-HEAD step (Q08) must adopt from a received reply');
});

test('rerun2h Q08b: a recovered owner must show commit-sent before authority-recovered (M3b)', () => {
  const stage = (pid, s) => ({ producerPid: pid, stage: s });
  const recoveredLine = '{"producerPid":2,"stage":"authority-recovered"}';
  const full = [stage(2, 'candidate-admitted'), stage(2, 'commit-sent'), stage(2, 'authority-recovered')];
  assert.deepEqual(commitEvidence({ stages: full, predPid: 1, succPid: 2, guiDriven: true, ownedLine: recoveredLine }).ok, true);
  // No commit-sent: the candidate never committed, so a recovery claim is not evidence of one.
  const noCommit = [stage(2, 'candidate-admitted'), stage(2, 'authority-recovered')];
  assert.equal(commitEvidence({ stages: noCommit, predPid: 1, succPid: 2, guiDriven: true, ownedLine: recoveredLine }).ok, false);
  // commit-sent by another process does not count.
  assert.equal(commitEvidence({ stages: [stage(9, 'commit-sent'), stage(2, 'authority-recovered')], predPid: 1, succPid: 2, guiDriven: true, ownedLine: recoveredLine }).ok, false);
  // Adopted from a received reply: the v1 predecessor's predecessor-committed is required.
  const adoptedLine = '{"producerPid":2,"stage":"authority-adopted"}';
  assert.equal(commitEvidence({ stages: full, predPid: 1, succPid: 2, guiDriven: true, ownedLine: adoptedLine }).ok, false);
  assert.equal(commitEvidence({ stages: [...full, stage(1, 'predecessor-committed')], predPid: 1, succPid: 2, guiDriven: true, ownedLine: adoptedLine }).ok, true);
  // HEAD-to-HEAD: both predecessor receipts, and a recovered line never relaxes that.
  const head = [stage(1, 'predecessor-prepared'), stage(1, 'predecessor-committed')];
  assert.equal(commitEvidence({ stages: head, predPid: 1, succPid: 2, guiDriven: false, ownedLine: adoptedLine }).ok, true);
  assert.equal(commitEvidence({ stages: full, predPid: 1, succPid: 2, guiDriven: false, ownedLine: recoveredLine }).ok, false);
});

