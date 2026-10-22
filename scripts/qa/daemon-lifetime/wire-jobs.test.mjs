import test from 'node:test';
import assert from 'node:assert/strict';
import { isLive, liveSessions, compareInventories, control, inventoryTotal, helloSupportsHandover, generationKey, assertDistinctKeys, endpoints, data } from './lib/wire.mjs';
import { parseLaunchctlPrint, launchctlProgram, parseSystemctlShow, windowsTaskTree, parseScheduledTaskState, supervisorMutations } from './lib/os-jobs.mjs';
import { HOOKS, HANDOVER_FAULTS, HANDOVER_FAULT_ENV, TRANSFER_FAULTS, FORCED_COMMIT_FAILURE, probeHooks, missingHooksFor } from './lib/hooks.mjs';
import { hostFor, buildPlan, assertQaBuild } from './lib/hosts.mjs';
import { agentCommand } from './lib/remote.mjs';
import { macForceTerminateScript, macQuitScript, linuxScopeUnit, windowsLauncherCommand } from './lib/gui.mjs';
import { armFile, LISTENER_STALL, LISTENER_STALL_DEADLINE_MS, Q04, stripAnsi } from './scenarios/q04-q06.mjs';
import { counterProgress } from './scenarios/q07-q09.mjs';
import { remoteWorkspaceId, sshdConfig } from './lib/sshd.mjs';
import { readFileSync } from 'node:fs';
import { AUTHORITY_STAGE, HANDOVER_FAULT_REQUEST, HANDOVER_TRANSFER, ROLLBACK_RELINQUISHMENT, CANDIDATE_STAGES, PREDECESSOR_STAGES, PRE_COMMIT_STAGES, LISTENER_ROLES, authorityStages, authorityInvariant, stagePattern, DAEMON_CLIENT_BUSY, busyReceiptPattern, busyVerdict, aliveOwnerPid } from './lib/receipts.mjs';
import { newSummary, settle } from './lib/summary.mjs';
import { awaitLegacyReady } from './scenarios/q10-q13.mjs';
import { createJobs, legacyLogFiles } from './lib/jobs.mjs';
import { isolationEnv as qaIsolationEnv, isolationLayout as qaIsolationLayout, sshKnownHostsFile } from './lib/guards.mjs';

const RUN = '20261019t064512z';

test('live predicate: reader_open for local, connected for ssh/paired', () => {
  assert.equal(isLive({ kind: 'local', readerOpen: true }), true);
  assert.equal(isLive({ kind: 'local', readerOpen: false, connected: true }), false);
  assert.equal(isLive({ kind: 'ssh', connected: false, remotePid: 4 }), false);
  assert.equal(isLive({ kind: 'paired', connected: true }), true);
  assert.equal(isLive({ kind: 'unknown', connected: true }), false);
  assert.throws(() => liveSessions({ type: 'error' }), /describeLiveSessionsOk/);
});

test('inventory comparison catches a lost, dead or re-spawned session', () => {
  const a = { kind: 'local', backendId: 's1', readerOpen: true, localChildPid: 5, localChildStartTime: 't' };
  const r = { kind: 'ssh', backendId: 's2', connected: true, remotePid: 9 };
  assert.deepEqual(compareInventories([a, r], [a, r]), []);
  assert.match(compareInventories([a], [])[0], /disappeared/);
  assert.match(compareInventories([a], [{ ...a, readerOpen: false }])[0], /no longer live/);
  assert.match(compareInventories([r], [{ ...r, remotePid: 10 }])[0], /identity changed/);
  assert.match(compareInventories([a], [{ ...a, localChildStartTime: 'u' }])[0], /identity changed/);
});

test('control-v1 frames use the wire field names', () => {
  assert.deepEqual(control.requestUpgrade('/x', 'k'), { control: 1, op: 'RequestUpgrade', staged_executable: '/x', generation_key: 'k' });
  assert.deepEqual(control.shutdown(3), { control: 1, op: 'Shutdown', confirm_live_sessions: 3 });
  assert.equal(inventoryTotal({ ok: true, live_local: 1, live_remote: 2, live_paired: 3 }), 6);
  assert.throws(() => inventoryTotal({ ok: false, code: 'X' }));
  assert.equal(helloSupportsHandover({ ok: true, ops: ['Hello', 'RequestUpgrade', 'handover-v1'] }), true);
  assert.equal(helloSupportsHandover({ ok: true, ops: ['Hello', 'RequestUpgrade'] }), false);
  assert.deepEqual(data.spawnSsh({ clientRequestId: 'r', workspaceId: 'w', hostStorePath: '/h' }).startup, { kind: 'remoteSsh', hostStorePath: '/h' });
  assert.equal(endpoints('/r', 'linux').control.path, '/r/control.sock');
  assert.equal(endpoints('C:\\r', 'windows').control.tokenFile, 'C:\\r\\control.token');
});

test('generation keys must be pairwise distinct', () => {
  assert.equal(generationKey('2026.1019.1', 'ABCDEF0123'), '2026.1019.1-abcdef01');
  assert.throws(() => assertDistinctKeys({ A: 'k', B: 'k' }), /pairwise distinct/);
});

test('OS job parsers', () => {
  assert.deepEqual(parseLaunchctlPrint({ status: 113, stderr: 'Could not find service "x" in domain' }), { state: 'NotRegistered', pid: null });
  assert.deepEqual(parseLaunchctlPrint({ status: 0, stdout: 'x = {\n\tstate = running\n\tpid = 42\n' }), { state: 'Running', pid: 42 });
  assert.equal(parseLaunchctlPrint({ status: 0, stdout: '\tstate = not running\n' }).state, 'RegisteredIdle');
  assert.equal(parseLaunchctlPrint({ status: 5, stderr: 'boom' }).state, 'Unverifiable');
  assert.equal(launchctlProgram('\tprogram = /a b/ferryx\n'), '/a b/ferryx');
  const sd = parseSystemctlShow('MainPID=0\nActiveState=inactive\nUnitFileState=enabled\n');
  assert.equal(sd.pid, null);
  assert.equal(sd.unitFileState, 'enabled');
  const tree = windowsTaskTree([{ pid: 1, ppid: 0 }, { pid: 2, ppid: 1, commandLine: 'ferryx.exe --daemon' }, { pid: 3, ppid: 1, commandLine: 'ferryx.exe --daemon-supervisor' }], { watchdogPid: 1 });
  assert.equal(tree.daemon.pid, 2);
  assert.deepEqual(parseScheduledTaskState('{"State":4}'), { state: 'Running' });
  assert.equal(parseScheduledTaskState('').state, 'Unknown');
  assert.equal(parseScheduledTaskState('garbled').state, 'Unknown');
  assert.equal(supervisorMutations(['probe ok', 'launchctl kickstart gui/1/x', 'systemctl --user stop u', 'schtasks /Run /TN t', 'launchctl print x']).length, 3);
});

test('hook probe marks absent product hooks and gates the scenarios that need them', () => {
  const files = new Map([['src-tauri/src/daemon/qa_producers.rs', 'FERRYX_QA_DISABLE_CONTROL']]);
  const hooks = probeHooks('/repo', 'linux', (f) => files.get(f.replace('/repo/', '')) ?? '', (f) => files.has(f.replace('/repo/', '')));
  assert.ok(Object.keys(hooks).length === HOOKS.length);
  assert.equal(hooks.macPlistForwardsIsolationEnv.notApplicable, true);
  const q04 = missingHooksFor('Q04', hooks).map((h) => h.id);
  assert.ok(q04.includes('listenerStall'));
  assert.ok(!missingHooksFor('Q01', hooks).some((h) => h.id === 'listenerStall'));
});

test('every product hook exists in this checkout (todos 8, 9, 12 merged); no scenario is hook-blocked', () => {
  const repo = new URL('../../../', import.meta.url).pathname;
  const todo12 = ['listenerStall', 'probeFault', 'buildSalt', 'forceFallback', 'macPlistForwardsIsolationEnv', 'jobForwardsQaBarrier', 'detachGuard', 'writeAckHoldArm'];
  for (const platform of ['mac', 'linux', 'windows']) {
    const hooks = probeHooks(repo, platform);
    for (const id of todo12) assert.equal(hooks[id].present, true, `${platform}: ${id} must be present`);
    for (const [id, r] of Object.entries(hooks)) assert.equal(r.present, true, `${platform}: ${id} missing (${r.owner})`);
    for (const id of ['Q00', 'Q04', 'Q05', 'Q06', 'Q06w', 'Q07', 'Q08', 'Q08b', 'Q09', 'Q11']) assert.deepEqual(missingHooksFor(id, hooks), [], `${platform} ${id}`);
  }
});

test('todo 8 receipt names match qa_producers.rs and every server.rs authority stage', () => {
  const src = (f) => readFileSync(new URL(`../../../src-tauri/src/${f}`, import.meta.url), 'utf8');
  const producers = src('daemon/qa_producers.rs');
  const constant = (name) => new RegExp(`pub const ${name}: &str = "([^"]+)"`).exec(producers)?.[1];
  assert.equal(constant('AUTHORITY_STAGE'), AUTHORITY_STAGE);
  assert.equal(constant('HANDOVER_FAULT_REQUEST'), HANDOVER_FAULT_REQUEST);
  assert.equal(constant('HANDOVER_TRANSFER'), HANDOVER_TRANSFER);
  assert.equal(constant('ROLLBACK_RELINQUISHMENT'), ROLLBACK_RELINQUISHMENT);
  for (const role of LISTENER_ROLES) assert.match(producers, new RegExp(`"${role}": manager\\.accept_count`));
  const server = src('daemon/server.rs');
  const emitted = new Set([...server.matchAll(/note_authority_stage\(\s*"([a-z-]+)"/g)].map((m) => m[1]));
  assert.deepEqual([...emitted].sort(), [...CANDIDATE_STAGES, ...PREDECESSOR_STAGES].sort());
  for (const s of PRE_COMMIT_STAGES) assert.ok(CANDIDATE_STAGES.includes(s));
  const faults = [...producers.matchAll(/Some\("([a-z-]+)"\) => Some\(CommitFault::/g)].map((m) => m[1]);
  assert.deepEqual([...faults, constant('HANDOVER_FAULT_ABORT_AFTER_ADOPT')], [...HANDOVER_FAULTS]);
});

test('authority invariant flags pre-commit accepts, a free lock and a missing socket', () => {
  const line = (pid, stage, extra = {}) => JSON.stringify({ runId: RUN, operationId: 'op', producer: 'ipc-qa-barrier', producerPid: pid, settledAtMs: 1, stage, acceptCounts: { canonical: 0, control: 0, agentState: 0 }, probeFlockRefused: { legacy: true, persistent: true }, publicSocketPresent: true, ...extra });
  const ok = [line(1, 'predecessor-prepared'), line(2, 'candidate-admitted'), line(2, 'authority-validated'), line(2, 'commit-sent'), line(1, 'predecessor-committed'), line(2, 'authority-adopted', { acceptCounts: { canonical: 3, control: 0, agentState: 0 } })].join('\n');
  const inv = authorityInvariant(authorityStages(`old\n${ok}`, RUN, 1), 2);
  assert.deepEqual(inv.candidatePreCommit, ['candidate-admitted', 'authority-validated', 'commit-sent']);
  assert.deepEqual([inv.acceptedBeforeCommit, inv.lockFreeAt, inv.socketMissingAt], [[], [], []]);
  const bad = [line(2, 'commit-sent', { acceptCounts: { canonical: 0, control: 1, agentState: 0 } }), line(1, 'predecessor-prepared', { probeFlockRefused: { legacy: false, persistent: null } }), line(2, 'candidate-admitted', { publicSocketPresent: false })].join('\n');
  const worse = authorityInvariant(authorityStages(bad, RUN), 2);
  assert.equal(worse.acceptedBeforeCommit.length, 1);
  assert.equal(worse.lockFreeAt.length, 1);
  assert.deepEqual(worse.socketMissingAt, ['candidate-admitted']);
  assert.ok(new RegExp(stagePattern(2, 'commit-sent')).test(line(2, 'commit-sent')));
  assert.ok(!new RegExp(stagePattern(1, 'commit-sent')).test(line(2, 'commit-sent')));
});

test('handover fault names are todo 8 FERRYX_QA_HANDOVER_FAULT values', () => {
  assert.equal(HANDOVER_FAULT_ENV, 'FERRYX_QA_HANDOVER_FAULT');
  assert.deepEqual([...HANDOVER_FAULTS], ['decision-write-failure', 'drop-commit-ok-before-record', 'drop-commit-ok-after-record', 'abort-after-adopt']);
});

test('Q08 runs todo 8 four transfer failure variants from the product, plus the forced commit failure', () => {
  const producers = readFileSync(new URL('../../../src-tauri/src/daemon/qa_producers.rs', import.meta.url), 'utf8');
  const transfer = [...producers.matchAll(/pub const TRANSFER_FAULT_[A-Z_]+: &str = "([a-z-]+)"/g)].map((m) => m[1]);
  assert.deepEqual(transfer, [...TRANSFER_FAULTS]);
  assert.equal(TRANSFER_FAULTS.length, 4);
  assert.ok(HANDOVER_FAULTS.includes(FORCED_COMMIT_FAILURE));
  assert.ok(!TRANSFER_FAULTS.includes(FORCED_COMMIT_FAILURE));
  const q08 = readFileSync(new URL('./scenarios/q07-q09.mjs', import.meta.url), 'utf8');
  assert.match(q08, /for \(const fault of TRANSFER_FAULTS\) await faultVariant/);
  assert.match(q08, /await faultVariant\(ctx, result, FORCED_COMMIT_FAILURE/);
});

test('Q04 stall arm outlasts the GUI wait and Windows launcher forbids breakaway on request', () => {
  const arm = armFile({ barriers: '/r/barriers' }, LISTENER_STALL, RUN, 'op', { deadlineMs: LISTENER_STALL_DEADLINE_MS });
  assert.equal(arm.file, '/r/barriers/daemon-listener-stall.arm.json');
  assert.deepEqual(JSON.parse(arm.content), { name: 'daemon-listener-stall', runId: RUN, operationId: 'op', deadlineMs: LISTENER_STALL_DEADLINE_MS });
  assert.ok(LISTENER_STALL_DEADLINE_MS > 35_000);
  const cmd = windowsLauncherCommand({ root: 'C:\\r', exe: 'C:\\r\\ferryx.exe', envFile: 'C:\\r\\e.json', tag: 'q06w', noBreakaway: true });
  assert.ok(cmd.endsWith('-Tag q06w -NoBreakaway'), cmd);
  assert.ok(!windowsLauncherCommand({ root: 'C:\\r', exe: 'x', envFile: 'e', tag: 't' }).includes('NoBreakaway'));
});

// One daemon-client-busy line exactly as qa_barrier.rs append_receipt + client.rs daemon_busy_settlement write it.
const busyLine = ({ runId = RUN, operationId = `q04-${RUN}`, code = 'DAEMON_BUSY', cause = 'handshakeSilent', ownerState = 'Alive { pid: 4242, start_time: 99 }' } = {}) =>
  JSON.stringify({ runId, operationId, producer: 'ipc-qa-barrier', producerPid: 7, settledAtMs: 1, name: DAEMON_CLIENT_BUSY, code, details: { type: 'daemonOwnerBusy', cause, ownerState } });

test('Q04 busy receipt: correlated to runId and q04-<runId>, handshakeSilent from the pre-stall Alive pid', () => {
  const op = `q04-${RUN}`;
  const re = new RegExp(busyReceiptPattern(RUN, op));
  assert.ok(re.test(busyLine()));
  assert.ok(!re.test(busyLine({ runId: 'other' })));
  assert.ok(!re.test(busyLine({ operationId: 'q04-other' })));
  assert.ok(!re.test(busyLine({ code: 'IO_ERROR' })));
  assert.equal(aliveOwnerPid('Alive { pid: 4242, start_time: 99 }'), 4242);
  assert.equal(aliveOwnerPid('Unknown { reason: "silent" }'), null);
  const ok = busyVerdict(busyLine(), { runId: RUN, operationId: op, pidBefore: 4242 });
  assert.equal(ok.correlated, 1);
  assert.ok(ok.match);
  // Correlated DAEMON_BUSY that is not the pre-stall Alive owner's silent handshake does not count.
  for (const bad of [busyLine({ ownerState: 'Alive { pid: 4243, start_time: 99 }' }), busyLine({ ownerState: 'Unknown { reason: "silent" }' }), busyLine({ cause: 'ownerUnresponsive' })]) {
    const v = busyVerdict(bad, { runId: RUN, operationId: op, pidBefore: 4242 });
    assert.equal(v.correlated, 1);
    assert.equal(v.match, null, bad);
  }
  assert.equal(busyVerdict(busyLine(), { runId: RUN, operationId: op, pidBefore: null }).match, null);
  assert.equal(busyVerdict(busyLine({ operationId: 'q04-older' }), { runId: RUN, operationId: op, pidBefore: 4242 }).correlated, 0);
  // Measured GUI log shape (full-r2/Q04-gui-during-stall.log): ANSI between a field name and '='.
  assert.equal(stripAnsi('\x1b[3mstate\x1b[0m\x1b[2m=\x1b[0mAlive'), 'state=Alive');
});

/** Q04 against an in-memory host: the stall is entered, and the barrier holds whatever busy receipt text is given. */
function q04Harness({ busyText, guiLog = '', pidDuring = 4242 }) {
  const files = new Map();
  const pids = [4242, pidDuring];
  const call = async (op, a = {}) => {
    switch (op) {
      case 'writeFile': files.set(a.file, a.content); return {};
      case 'dataOpen': return { handle: 1 };
      case 'controlOpen': return { handle: null };
      case 'waitPath': return true;
      case 'readFile':
        if (a.file.endsWith(`${DAEMON_CLIENT_BUSY}.receipt.jsonl`)) return busyText;
        if (a.file.endsWith('owner.json')) return JSON.stringify({ pid: 4242 });
        if (a.file.includes('/logs/gui-')) return guiLog;
        return files.get(a.file) ?? null;
      case 'waitLog': {
        const text = a.file.endsWith(`${DAEMON_CLIENT_BUSY}.receipt.jsonl`) ? busyText : '';
        const m = new RegExp(a.pattern, a.flags ?? '').exec(text);
        return m ? { matched: m[0] } : { matched: null, timedOut: true };
      }
      case 'probe': return { startTime: 99 };
      case 'call':
        if (a.frame.type === 'attach') return { type: 'attachOk' };
        if (a.frame.type === 'describeLiveSessions') return { type: 'describeLiveSessionsOk', sessions: [] };
        return { type: 'writeOk' };
      case 'waitOutput': return { count: 1 };
      default: return {};
    }
  };
  const conn = { close: async () => {} };
  const ctx = {
    runId: RUN, platform: 'linux', keys: { A: 'a' }, artifacts: { A: {} }, evidenceDir: '/e',
    layout: { barriers: '/r/barriers', runtime: '/r/runtime', logs: '/r/logs' },
    agent: { call }, writeEvidence: async () => {},
    jobs: { inspect: async () => ({ pid: pids.shift() }) },
    gui: { quit: async () => ({}), launch: async () => ({ tag: 'q04' }) },
    state: { gui: { tag: 'q00' }, conn, localA: 's1' },
  };
  const result = newSummary({ runId: RUN, platform: 'linux', host: 'h' }).scenarios.find((s) => s.id === 'Q04');
  return { ctx, result, files };
}

test('Q04 passes only on the correlated busy receipt and keeps every no-start/recovery check', async () => {
  const failing = (r) => r.assertions.filter((a) => !a.ok).map((a) => a.name);
  const good = q04Harness({ busyText: busyLine() });
  await Q04(good.ctx, good.result);
  assert.equal(settle(good.result).status, 'PASS', JSON.stringify(failing(good.result)));
  assert.ok(good.files.has('/r/barriers/daemon-listener-stall.release.json'), 'the stall is released after the busy checks');
  for (const name of ['job pid unchanged during the stall', 'zero supervisor mutations in the GUI log', 'after release: daemon pid and start time unchanged', 'original session after release: nonce']) {
    assert.ok(good.result.assertions.some((a) => a.name.startsWith(name)), name);
  }
  // Prose in the GUI log is no longer the verdict: without a receipt Q04 fails even when the log says DAEMON_BUSY.
  const prose = q04Harness({ busyText: null, guiLog: 'WARN code="DAEMON_BUSY" cause="handshakeSilent"' });
  await Q04(prose.ctx, prose.result);
  assert.equal(settle(prose.result).status, 'FAIL');
  assert.ok(failing(prose.result).some((n) => n.startsWith('client received DAEMON_BUSY for this stall')));
  // Another run's or another operation's receipt, or an owner other than the pre-stall pid, is not this stall's outcome.
  for (const busyText of [busyLine({ runId: 'other' }), busyLine({ operationId: 'q04-older' }), busyLine({ ownerState: 'Alive { pid: 5555, start_time: 1 }' })]) {
    const h = q04Harness({ busyText });
    await Q04(h.ctx, h.result);
    assert.equal(settle(h.result).status, 'FAIL', busyText);
  }
  // The self-test reload build: the job pid changes and the GUI log carries a styled restart line.
  const reload = q04Harness({ busyText: busyLine(), pidDuring: 5555, guiLog: '\x1b[33mWARN\x1b[0m self-test: systemctl --user restart ferryx-qa-x.service' });
  await Q04(reload.ctx, reload.result);
  assert.equal(settle(reload.result).status, 'FAIL');
  assert.deepEqual(failing(reload.result).sort(), ['job pid unchanged during the stall', 'zero supervisor mutations in the GUI log']);
});

test('builds: only debug QA bundles with a private target dir', () => {
  const host = hostFor('linux');
  const plan = buildPlan({ host, runDir: '/tmp/r', worktree: '/w', v1Worktree: null, baselineWorktree: '/b' });
  for (const k of ['A', 'B', 'C', 'bump', 'baseline']) assert.ok(assertQaBuild(plan[k]));
  assert.equal(plan.v1bump, null);
  assert.notEqual(plan.A.env.FERRYX_QA_BUILD_SALT, plan.B.env.FERRYX_QA_BUILD_SALT);
  assert.throws(() => assertQaBuild({ ...plan.A, args: plan.A.args.filter((a) => a !== '--debug') }), /debug/);
  assert.throws(() => assertQaBuild({ ...plan.baseline, args: [...plan.baseline.args, '--release'] }), /release/);
  assert.throws(() => assertQaBuild({ ...plan.A, env: {} }), /CARGO_TARGET_DIR/);
  const win = buildPlan({ host: hostFor('windows'), runDir: 'C:\\r', worktree: 'C:\\w', v1Worktree: 'C:\\v', baselineWorktree: 'C:\\b' });
  for (const k of ['A', 'B', 'C', 'bump', 'v1bump', 'baseline']) assert.equal(win[k].env.CARGO_INCREMENTAL, '0', k);
});

test('linux run storage leaves tmpfs while sockets keep a short path', async () => {
  const { forbiddenRoots, isolationLayout, isInside } = await import('./lib/guards.mjs');
  const host = hostFor('linux');
  assert.ok(host.storageRoot && !isInside(host.storageRoot, '/tmp', 'linux'), 'bulk storage is not on /tmp');
  for (const root of forbiddenRoots({ platform: 'linux', home: '/home/indo', uid: 1000 })) assert.ok(!isInside(host.storageRoot, root, 'linux'), root);
  const layout = isolationLayout(`${host.storageRoot}/${RUN}`, 'linux', { runtimeDir: `${host.runRoot}/${RUN}/run` });
  // Longest socket the daemon binds in its runtime dir: handover-<uuid v4>.sock (handover_socket.rs).
  const longest = `${layout.runtime}/handover-00000000-0000-0000-0000-000000000000.sock`;
  assert.ok(Buffer.byteLength(longest) < 108, `${longest} exceeds sun_path`);
  assert.ok(isInside(layout.data, host.storageRoot, 'linux') && isInside(layout.logs, host.storageRoot, 'linux'));
});

test('agent command: local node or quoted ssh', () => {
  const local = agentCommand(hostFor('mac'), { agentPath: '/a/agent.mjs', runId: RUN, runDir: '/r' });
  assert.equal(local.args[0], '/a/agent.mjs');
  const ssh = agentCommand(hostFor('linux'), { agentPath: "/a b/agent's.mjs", runId: RUN, runDir: '/r', allowRoots: ['/t'] });
  assert.equal(ssh.command, 'ssh');
  assert.ok(ssh.args.includes('BatchMode=yes'));
  assert.match(ssh.args.at(-1), /'\/a b\/agent'\\''s\.mjs'/);
  assert.match(ssh.args.at(-1), /'--allow-root' '\/t'/);
  const win = agentCommand(hostFor('windows'), { agentPath: 'C:\\a\\agent.mjs', runId: RUN, runDir: 'C:\\r' });
  assert.match(win.args.at(-1), /^"node" "C:\\a\\agent\.mjs"/);
});

test('MahoRD chords go through remote_hotkey, single keys through remote_key_press', async () => {
  const { TOOLS } = await import('./lib/mahord.mjs');
  assert.equal(TOOLS.hotkey, 'remote_hotkey');
  assert.equal(TOOLS.key, 'remote_key_press');
  const gui = readFileSync(new URL('./lib/gui.mjs', import.meta.url), 'utf8');
  assert.ok(!/mahord\.key\('[^']*\+/.test(gui), 'no chord passed to remote_key_press');
});

test('GUI scripts target exactly the QA bundle id', () => {
  const id = `com.ferryx.app.qa-${RUN}`;
  assert.match(macForceTerminateScript(id), new RegExp(`runningApplicationsWithBundleIdentifier\\('${id.replace(/\./g, '\\.')}'\\)`));
  assert.equal(macQuitScript(id), `tell application id "${id}" to quit`);
  assert.equal(linuxScopeUnit(RUN, 'g1'), `ferryx-qa-${RUN}-gui-g1.scope`);
});

test('counter continuity detects a restart', () => {
  assert.deepEqual(counterProgress('Q_TICK_4\nQ_TICK_5\nQ_TICK_6', 'Q'), { max: 6, restarted: false });
  assert.equal(counterProgress('Q_TICK_4\nQ_TICK_1\nQ_TICK_2', 'Q').restarted, true);
});

test('private sshd: loopback only, key auth only; workspace id matches projects.rs', () => {
  const cfg = sshdConfig({ port: 2222, hostKey: '/k', authorizedKeys: '/a', pidFile: '/p', user: 'qa' });
  assert.match(cfg, /^ListenAddress 127\.0\.0\.1$/m);
  assert.match(cfg, /^PasswordAuthentication no$/m);
  const id = remoteWorkspaceId('h', '/r');
  assert.match(id, /^ssh:[0-9a-f]{64}$/);
  assert.notEqual(id, remoteWorkspaceId('h', '/r2'));
});

test('host paths join in the host flavour, not the controller flavour', async () => {
  const { hjoin } = await import('./lib/hostpath.mjs');
  assert.equal(hjoin('C:\\Users\\sook\\ferryx-wt\\.qa-daemon-lifetime\\r', 'run', 'owner.json'), 'C:\\Users\\sook\\ferryx-wt\\.qa-daemon-lifetime\\r\\run\\owner.json');
  assert.equal(hjoin('/tmp/ferryx-qa-daemon-lifetime/r', 'run', 'owner.json'), '/tmp/ferryx-qa-daemon-lifetime/r/run/owner.json');
  for (const f of ['lib/sshd.mjs', 'scenarios/common.mjs', 'scenarios/q00-q03.mjs', 'scenarios/q04-q06.mjs', 'scenarios/q07-q09.mjs', 'scenarios/q10-q13.mjs', 'lib/gui.mjs']) {
    assert.ok(!/(?<![.\w])path\.join\(/.test(readFileSync(new URL('./' + f, import.meta.url), 'utf8')), f);
  }
});

test('Windows GUI launch types only a short path MahoRD can deliver', async () => {
  const { mahordTypable, windowsShortPath } = await import('./lib/gui.mjs');
  assert.equal(mahordTypable('C:\\Users\\sook\\FERRYX~1\\qaruns\\202610~1\\launchg1.cmd'), true);
  assert.equal(mahordTypable('C:\\Users\\sook\\ferryx-wt\\.qa-daemon-lifetime'), false);
  const agent = (stdout) => ({ call: async () => ({ status: 0, stdout, stderr: '' }) });
  assert.equal(await windowsShortPath(agent('C:\\Users\\sook\\FERRYX~1\\qaruns\\202610~1\\launchg1.cmd\r\n'), 'x'), 'C:\\Users\\sook\\FERRYX~1\\qaruns\\202610~1\\launchg1.cmd');
  await assert.rejects(windowsShortPath(agent('C:\\Users\\sook\\FERRYX~1\\QA-DAE~1\\l.cmd'), 'x'), /MahoRD cannot type/);
  // The long runRoot legitimately contains 'ferryx-wt'; only its 8.3 form is typed, and
  // windowsShortPath refuses a short path that still carries '-' or '_' at run time.
  const { HOSTS } = await import('./lib/hosts.mjs');
  assert.ok(!/[-_]/.test(HOSTS.windows.runRoot.split('\\').pop()), 'the run root leaf itself has no - or _');
});

// Windows launch readiness (r5 Q00). The fake agent answers every receipt from a scripted table,
// and the observer's records arrive only when the harness has done the step that causes them
// (arm file, Win+R, target file, typed text), so each outcome is decided by the script, never by
// timing. A waitLog nothing satisfies resolves { matched: null, timedOut: true }, as the real one
// does at its deadline; pending receipts stay pending.
const focusRecord = (o) => JSON.stringify({ kind: 'focus', focusHwnd: 0x5012c, rootHwnd: 0x5012a, processName: 'explorer.exe', rootClass: '#32770', focusedClass: 'Edit', parentClass: 'ComboBox', foreground: true, ...o, seq: 1 });

async function windowsLaunchFixture({ afterWinR = [focusRecord()], typedMatch = true, lostBeforeText = false, receipts = {}, observer = {}, exec = {} } = {}) {
  const { createGuiDriver, windowsLaunchFiles, windowsObserverTask } = await import('./lib/gui.mjs');
  const { isolationLayout } = await import('./lib/guards.mjs');
  const layout = isolationLayout('C:\\r', 'windows');
  const files = windowsLaunchFiles(layout, 'g1');
  const task = windowsObserverTask(RUN, 'g1');
  const calls = [];
  const written = new Map();
  const events = [];
  const state = { armed: false, latched: null };
  const ob = { pid: 9100, sessionId: 1, activeConsoleSessionId: 1, hooks: 3, start: '1337', executable: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', ready: true, armAck: true, latchAck: true, ...observer };
  const pending = new Promise(() => {});
  const answer = { [files.cmdStarted]: true, [files.launcherStarted]: true, [files.published]: true, [files.launcherError]: pending, [files.observerError]: pending, ...receipts };
  const agent = { call: async (method, params) => {
    calls.push(['agent', method, params.file ?? params.command ?? params.pid, params.args?.[0]]);
    if (method === 'writeFile') {
      written.set(params.file, params.content);
      if (params.file === files.observerArm && ob.armAck) { state.armed = true; events.push(JSON.stringify({ kind: 'armed', pid: ob.pid, seq: 0 })); }
      const m = /\.target-(\d+)$/.exec(params.file);
      if (m && ob.latchAck) { state.latched = Number(m[1]); events.push(JSON.stringify({ kind: 'latched', focusHwnd: state.latched, seq: 2 })); }
      return true;
    }
    if (method === 'exec' && params.command === 'schtasks') return exec[params.args[0]] ?? { status: 0, stdout: 'SUCCESS', stderr: '' };
    if (method === 'exec') return { status: 0, stdout: 'C:\\R\\LAUNCHG1.CMD\r\n', stderr: '' };
    if (method === 'waitPath') {
      if (params.file === files.observerReady) return ob.ready;
      return answer[params.file] ?? false;
    }
    if (method === 'readFile') {
      if (params.file === files.observerReady) return JSON.stringify({ pid: ob.pid, sessionId: ob.sessionId, activeConsoleSessionId: ob.activeConsoleSessionId, nonce: ob.nonce ?? JSON.parse(written.get(files.observerArgs)).nonce, hooks: ob.hooks, start: ob.start });
      if (params.file === files.published) return JSON.stringify({ pid: 4242, jobAssigned: true, noBreakaway: false, launcherPid: 7 });
      return 'boom at line 44';
    }
    if (method === 'probe') return params.pid === ob.pid ? { executable: ob.executable, startTime: ob.probeStart ?? ob.start } : null;
    if (method === 'waitLog') {
      const re = new RegExp(params.pattern, params.flags);
      const hit = events.map((line) => re.exec(line)).find(Boolean);
      return hit ? { matched: hit[0] } : { matched: null, timedOut: true };
    }
    if (method === 'waitExit') return { pid: params.pid, exitedAt: 1 };
    if (method === 'record') return { pid: params.pid, startTime: 1 };
    throw new Error(`unexpected agent ${method}`);
  } };
  const mahord = {
    click: async () => calls.push(['mahord', 'click']),
    hotkey: async (keys) => {
      calls.push(['mahord', 'hotkey', keys.join('+')]);
      // A focus change is reported only once armed: the observer drops events before "armed".
      if (keys.join('+') === 'Meta+r' && state.armed) events.push(...afterWinR);
    },
    type: async (text) => {
      calls.push(['mahord', 'type', text]);
      if (lostBeforeText) events.push(JSON.stringify({ kind: 'lost', focusHwnd: 77, seq: 3 }));
      else if (state.latched) events.push(JSON.stringify({ kind: 'text', match: typedMatch, length: text.length, seq: 3 }));
    },
    key: async (key) => calls.push(['mahord', 'key', key]),
    screenshot: async () => { calls.push(['mahord', 'screenshot']); return Buffer.from('png'); },
  };
  const gui = createGuiDriver({ host: { platform: 'windows', transport: { user: 'sook' } }, agent, runId: RUN, layout, allowedRoots: ['C:\\r'], forbidden: [], mahord });
  const mahordCalls = () => calls.filter((c) => c[0] === 'mahord').map((c) => c.slice(1).join(' '));
  const schtasks = () => calls.filter((c) => c[2] === 'schtasks').map((c) => c[3]);
  const sentInput = () => mahordCalls().filter((c) => c.startsWith('type') || c.startsWith('key') || c === 'hotkey Control+a');
  const receipt = () => JSON.parse(written.get(files.receipt));
  return { gui, files, task, calls, written, mahordCalls, schtasks, sentInput, receipt, artifact: { guiExecutable: 'C:\\r\\ferryx.exe' } };
}

test('Windows launch: observer acknowledged before Win+R, typing only into the identified Run dialog, Enter only after read-back', async () => {
  const fx = await windowsLaunchFixture();
  const gui = await fx.gui.launch(fx.artifact, { tag: 'g1' });
  assert.equal(gui.pid, 4242);
  assert.deepEqual(fx.mahordCalls(), ['click', 'hotkey Meta+r', 'hotkey Control+a', 'type C:\\R\\LAUNCHG1.CMD', 'key Enter']);
  assert.deepEqual(fx.schtasks(), ['/Create', '/Run', '/End', '/Delete']);
  const order = fx.calls.map((c) => (c[0] === 'mahord' ? c[2] ?? c[1] : c[3] ?? (typeof c[2] === 'string' ? c[2].split('\\').pop() : c[1])));
  assert.ok(order.indexOf('observer-g1.arm') < order.indexOf('Meta+r'), 'armed before Win+R');
  assert.ok(order.indexOf('/Delete') < order.indexOf('launch-g1.cmd.started'), 'observer removed before waiting on the launch');
  const stages = fx.receipt().stages.map((s) => s.stage);
  assert.deepEqual(stages, ['observer-ready', 'observer-armed', 'run-dialog-focus', 'latched', 'typed', 'observer-stopped', 'cmd-executed', 'launcher-started', 'published']);
  assert.equal(fx.receipt().observer.task, fx.task);
  assert.equal(fx.receipt().observer.pid, 9100);
  assert.deepEqual(fx.receipt().observer.cleanup, { stopRequested: true, exited: true, ended: 0, deleted: true, errors: [] });
});

test('observer task name is this run\'s QA task and its command line launches only the observer fixture', async () => {
  const { windowsObserverTask, windowsObserverCommand, windowsLaunchFiles } = await import('./lib/gui.mjs');
  const { isQaJobName } = await import('./lib/guards.mjs');
  assert.equal(windowsObserverTask(RUN, 'g1'), `Ferryx\\ferryx-qa-${RUN}-obs-g1`);
  assert.ok(isQaJobName(windowsObserverTask(RUN, 'g1'), RUN, 'windows'));
  const f = windowsLaunchFiles({ root: 'C:\\r', logs: 'C:\\r\\logs' }, 'g1');
  assert.equal(windowsObserverCommand(f, 'g1'), 'conhost.exe --headless powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\\r\\observer.ps1 -Tag g1');
  assert.ok(windowsObserverCommand(f, 'g1').length <= 261, 'schtasks /TR limit');
});

test('Run dialog identity: explorer #32770 root, Edit focused, foreground; title and control id are not used', async () => {
  const { isRunDialogFocus, RUN_DIALOG_FOCUS_PATTERN } = await import('./lib/gui.mjs');
  const re = new RegExp(RUN_DIALOG_FOCUS_PATTERN, 'm');
  const accepted = JSON.parse(focusRecord());
  assert.ok(isRunDialogFocus(accepted) && re.test(focusRecord()));
  for (const other of [
    { processName: 'WindowsTerminal.exe' }, // a foreign dialog
    { rootClass: 'Shell_TrayWnd', focusedClass: 'Shell_TrayWnd' }, // the taskbar after the click
    { focusedClass: 'Button' }, // explorer "cannot find" message box
    { foreground: false }, // Run dialog behind a focus-stealing console
    { focusHwnd: 0 },
  ]) {
    const line = focusRecord(other);
    assert.ok(!isRunDialogFocus(JSON.parse(line)), line);
    assert.ok(!re.test(line), `pattern rejects ${line}`);
  }
  assert.ok(!isRunDialogFocus({ ...accepted, kind: 'foreground' }));
  assert.ok(!isRunDialogFocus(null));
});

test('unrelated focus changes never unlock typing; the launch fails at run-dialog-not-identified and still removes the observer', async () => {
  const fx = await windowsLaunchFixture({ afterWinR: [focusRecord({ rootClass: 'Shell_TrayWnd', focusedClass: 'Shell_TrayWnd' }), focusRecord({ processName: 'OpenConsole.exe', rootClass: 'ConsoleWindowClass', focusedClass: 'ConsoleWindowClass' }), focusRecord({ focusedClass: 'Button' })] });
  await assert.rejects(fx.gui.launch(fx.artifact, { tag: 'g1' }), (e) => e.name === 'WindowsLaunchError' && e.stage === 'run-dialog-not-identified');
  assert.deepEqual(fx.sentInput(), []);
  assert.deepEqual(fx.schtasks(), ['/Create', '/Run', '/End', '/Delete']);
  assert.equal(fx.written.get(fx.files.observerStop) !== undefined, true);
  assert.equal(fx.written.get(fx.files.screenshot), Buffer.from('png').toString('base64'));
  assert.equal(fx.receipt().failedStage, 'run-dialog-not-identified');
  assert.equal(fx.receipt().observer.cleanup.deleted, true);
});

test('Enter is never sent unless the latched Edit reads back the typed path', async () => {
  for (const [opts, detail] of [[{ typedMatch: false }, /did not read back/], [{ lostBeforeText: true }, /focus left the Run dialog/]]) {
    const fx = await windowsLaunchFixture(opts);
    await assert.rejects(fx.gui.launch(fx.artifact, { tag: 'g1' }), (e) => e.stage === 'typed-text-not-confirmed' && detail.test(e.message));
    assert.ok(!fx.mahordCalls().includes('key Enter'), fx.mahordCalls().join(', '));
    assert.deepEqual(fx.schtasks(), ['/Create', '/Run', '/End', '/Delete']);
  }
});

test('observer startup must be acknowledged and owned before any GUI input; every failure removes the task', async () => {
  const cases = [
    [{ observer: { ready: false } }, 'observer-not-ready', false],
    [{ observer: { nonce: 'ffff' } }, 'observer-not-owned', false],
    [{ observer: { probeStart: '9' } }, 'observer-not-owned', false],
    [{ observer: { executable: 'C:\\x\\notepad.exe' } }, 'observer-not-owned', false],
    [{ observer: { sessionId: 0, activeConsoleSessionId: 1 } }, 'observer-wrong-session', true],
    [{ observer: { hooks: 2 } }, 'observer-not-ready', true],
    [{ observer: { armAck: false } }, 'observer-not-armed', true],
    [{ observer: { latchAck: false } }, 'observer-not-latched', true],
    [{ exec: { '/Run': { status: 1, stdout: '', stderr: 'ERROR: denied' } } }, 'observer-task-not-run', false],
    [{ exec: { '/Create': { status: 1, stdout: '', stderr: 'ERROR: denied' } } }, 'observer-task-not-created', false],
  ];
  for (const [opts, stage, pidKnown] of cases) {
    const fx = await windowsLaunchFixture(opts);
    await assert.rejects(fx.gui.launch(fx.artifact, { tag: 'g1' }), (e) => e.stage === stage, stage);
    const input = fx.mahordCalls().filter((c) => c !== 'screenshot');
    if (stage === 'observer-not-latched') assert.deepEqual(input, ['click', 'hotkey Meta+r'], stage);
    else if (stage === 'observer-not-armed') assert.deepEqual(input, ['click'], stage);
    else assert.deepEqual(input, [], `${stage}: no GUI input before the observer is ready`);
    assert.deepEqual(fx.schtasks().slice(-2), ['/End', '/Delete'], `${stage} removes the task`);
    assert.equal(fx.calls.some((c) => c[1] === 'waitExit'), pidKnown, `${stage}: only a confirmed pid is waited on`);
    assert.equal(fx.receipt().failedStage, stage);
  }
});

test('observer error receipt before ready fails at observer-error and removes the task', async () => {
  const { windowsLaunchFiles } = await import('./lib/gui.mjs');
  const { isolationLayout } = await import('./lib/guards.mjs');
  const f = windowsLaunchFiles(isolationLayout('C:\\r', 'windows'), 'g1');
  const fx = await windowsLaunchFixture({ observer: { ready: new Promise(() => {}) }, receipts: { [f.observerError]: true } });
  await assert.rejects(fx.gui.launch(fx.artifact, { tag: 'g1' }), (e) => e.stage === 'observer-error' && /boom at line 44/.test(e.message));
  assert.deepEqual(fx.mahordCalls(), ['screenshot']);
  assert.deepEqual(fx.schtasks(), ['/Create', '/Run', '/End', '/Delete']);
});

test('Windows launch names the failed stage after Enter: not executed, launcher not started, launcher error', async () => {
  const cases = [
    [(f) => ({ [f.cmdStarted]: false }), 'cmd-not-executed'],
    [(f) => ({ [f.launcherStarted]: false }), 'launcher-not-started'],
    [(f) => ({ [f.launcherError]: true, [f.published]: new Promise(() => {}) }), 'launcher-error'],
    [(f) => ({ [f.published]: false }), 'launcher-not-published'],
  ];
  const { windowsLaunchFiles } = await import('./lib/gui.mjs');
  const { isolationLayout } = await import('./lib/guards.mjs');
  const f = windowsLaunchFiles(isolationLayout('C:\\r', 'windows'), 'g1');
  for (const [receipts, stage] of cases) {
    const fx = await windowsLaunchFixture({ receipts: { ...receipts(f), [f.observerError]: new Promise(() => {}) } });
    await assert.rejects(fx.gui.launch(fx.artifact, { tag: 'g1' }), (e) => e.stage === stage, stage);
    assert.ok(fx.written.has(fx.files.screenshot), `${stage} keeps a MahoRD screenshot`);
    assert.equal(fx.receipt().failedStage, stage);
    assert.ok(!fx.calls.some((c) => c[1] === 'record'), `${stage} records no pid`);
  }
});

test('launcher and observer receipt file names match the names gui.mjs waits for', async () => {
  const { windowsLaunchFiles } = await import('./lib/gui.mjs');
  const f = windowsLaunchFiles({ root: 'C:\\r', logs: 'C:\\r\\logs' }, 'TAG');
  assert.equal(f.launcherStarted, 'C:\\r\\logs\\launcher-TAG.started');
  assert.equal(f.launcherError, 'C:\\r\\logs\\launcher-TAG.err');
  assert.equal(f.observerReady, 'C:\\r\\logs\\observer-TAG.ready.json');
  assert.equal(f.observerEvents, 'C:\\r\\logs\\observer-TAG.events.jsonl');
  assert.equal(f.observerTarget(42), 'C:\\r\\logs\\observer-TAG.target-42');
});

test('Q11: the QA daemon env pins the private sshd known_hosts that startSshd writes', () => {
  for (const [platform, root] of [['mac', '/tmp/qa/' + RUN], ['linux', '/home/q/.qaruns/' + RUN], ['windows', 'C:\\q\\qaruns\\' + RUN]]) {
    const layout = qaIsolationLayout(root, platform);
    const env = qaIsolationEnv({ runId: RUN, layout, platform });
    const sep = platform === 'windows' ? '\\' : '/';
    assert.equal(env.FERRYX_QA_SSH_KNOWN_HOSTS, [root, 'sshd', 'known_hosts'].join(sep), platform);
    assert.equal(sshKnownHostsFile(layout), env.FERRYX_QA_SSH_KNOWN_HOSTS);
    assert.ok(!Object.values(layout).includes(env.FERRYX_QA_SSH_KNOWN_HOSTS), 'a file, never mkdir-ed as a layout dir');
  }
  const sshd = readFileSync(new URL('./lib/sshd.mjs', import.meta.url), 'utf8');
  assert.match(sshd, /const knownHosts = sshKnownHostsFile\(ctx\.layout\)/);
});

/** Q13 readiness against an in-memory host: what the job's stdout log and the data socket say. */
function legacyWorld({ ready = true, handshake = { type: 'handshakeOk', version: 5, pid: 77 }, refuse = null }) {
  const w = { calls: [], closed: 0 };
  const ctx = {
    layout: { runtime: '/r/run', logs: '/r/logs' },
    agent: {
      call: async (method, params = {}) => {
        w.calls.push({ method, params });
        if (method === 'waitLog') return ready ? { matched: 'FERRYX_DAEMON_READY' } : { matched: null, timedOut: true };
        if (method === 'readFile') return params.file === '/r/logs/legacy.stderr.log' ? 'boom' : null;
        if (method === 'dataOpen') { if (refuse) throw new Error(refuse); return { handle: 'h1', handshake }; }
        if (method === 'close') { w.closed++; return true; }
        throw new Error(`unexpected agent call ${method}`);
      },
    },
  };
  return { w, ctx };
}

test('Q13 readiness: a stale daemon.sock never passes; only the READY line plus an accepted v5 handshake does', async () => {
  // Stale socket of the retired owner: no READY line yet, so no connect is even attempted.
  const stale = legacyWorld({ ready: false });
  const r1 = await awaitLegacyReady(stale.ctx, { retiredPid: 9 });
  assert.equal(r1.reason, 'NO_READY_LINE');
  assert.equal(r1.stderr, 'boom', 'the legacy stderr log is surfaced');
  assert.ok(!stale.w.calls.some((c) => c.method === 'dataOpen' || c.method === 'waitPath'));
  const wait = stale.w.calls.find((c) => c.method === 'waitLog').params;
  assert.equal(wait.file, '/r/logs/legacy.stdout.log');
  assert.ok(new RegExp(wait.pattern, wait.flags).test('booting\nFERRYX_DAEMON_READY\n'));
  assert.ok(!new RegExp(wait.pattern, wait.flags).test('FERRYX_DAEMON_READY_EXTRA\n'));
  assert.equal(wait.deadlineMs, 30_000);

  const refused = await awaitLegacyReady(legacyWorld({ refuse: 'connect ECONNREFUSED /r/run/daemon.sock' }).ctx);
  assert.equal(refused.reason, 'HANDSHAKE_FAILED');
  assert.match(refused.error, /ECONNREFUSED/);

  for (const handshake of [{ type: 'protocolMismatch', expectedVersion: 4 }, { type: 'handshakeOk', version: 6, pid: 77 }, { type: 'handshakeOk', version: 5, pid: 9 }]) {
    const odd = legacyWorld({ handshake });
    const r = await awaitLegacyReady(odd.ctx, { retiredPid: 9 });
    assert.equal(r.reason, 'HANDSHAKE_NOT_LEGACY_V5', JSON.stringify(handshake));
    assert.equal(odd.w.closed, 1, 'a refused handshake connection is closed');
  }

  const good = legacyWorld({});
  const ok = await awaitLegacyReady(good.ctx, { retiredPid: 9 });
  assert.equal(ok.conn.handshake.pid, 77);
  assert.deepEqual(good.w.calls.find((c) => c.method === 'dataOpen').params, { runtimeDir: '/r/run', version: 5 });
});

test('Q13 legacy job captures the daemon stdout/stderr on every platform', async () => {
  for (const platform of ['mac', 'linux', 'windows']) {
    const layout = platform === 'windows' ? { root: 'C:\\r', logs: 'C:\\r\\logs', launchAgents: 'C:\\r\\LA' } : { root: '/r', logs: '/r/logs', launchAgents: '/r/LA' };
    const logs = legacyLogFiles(layout);
    const written = [];
    const agent = { call: async (method, p) => {
      if (method === 'writeFile') written.push(p.content);
      if (method === 'exec' && p.input) written.push(p.input);
      if (method === 'exec' && p.command === 'sh' && !p.input) return { status: 0, stdout: '/home/q' };
      // Windows: the non-elevated helper's result file (jobs.mjs createLegacyNonElevated).
      if (method === 'waitPath') return true;
      if (method === 'readFile') return JSON.stringify({ create: 0, run: 0, stdout: '' });
      return { status: 0, stdout: '' };
    } };
    const jobs = createJobs({ host: { platform }, agent, runId: RUN, layout, uid: 501 });
    await jobs.installLegacy({ executable: platform === 'windows' ? 'C:\\t\\ferryx.exe' : '/t/ferryx', env: { FERRYX_DATA_DIR: '/d' } });
    const text = written.join('\n');
    if (platform === 'linux') {
      assert.match(text, new RegExp(`^StandardOutput=append:${logs.stdout}$`, 'm'));
      assert.match(text, new RegExp(`^StandardError=append:${logs.stderr}$`, 'm'));
      assert.ok(text.indexOf('StandardError=') < text.indexOf('[Install]'), 'inside [Service]');
    } else if (platform === 'mac') {
      assert.ok(text.includes(`<string>${logs.stdout}</string>`) && text.includes(`<string>${logs.stderr}</string>`));
    } else {
      assert.ok(text.includes(`--daemon 1>>"${logs.stdout}" 2>>"${logs.stderr}"`), text);
    }
  }
});
