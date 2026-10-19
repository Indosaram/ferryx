import test from 'node:test';
import assert from 'node:assert/strict';
import { isLive, liveSessions, compareInventories, control, inventoryTotal, helloSupportsHandover, generationKey, assertDistinctKeys, endpoints, data } from './lib/wire.mjs';
import { parseLaunchctlPrint, launchctlProgram, parseSystemctlShow, windowsTaskTree, parseScheduledTaskState, supervisorMutations } from './lib/os-jobs.mjs';
import { HOOKS, HANDOVER_FAULTS, HANDOVER_FAULT_ENV, TRANSFER_FAULTS, FORCED_COMMIT_FAILURE, probeHooks, missingHooksFor } from './lib/hooks.mjs';
import { hostFor, buildPlan, assertQaBuild } from './lib/hosts.mjs';
import { agentCommand } from './lib/remote.mjs';
import { macForceTerminateScript, macQuitScript, linuxScopeUnit, windowsLauncherCommand } from './lib/gui.mjs';
import { armFile, LISTENER_STALL, LISTENER_STALL_DEADLINE_MS } from './scenarios/q04-q06.mjs';
import { counterProgress } from './scenarios/q07-q09.mjs';
import { remoteWorkspaceId, sshdConfig } from './lib/sshd.mjs';
import { readFileSync } from 'node:fs';
import { AUTHORITY_STAGE, HANDOVER_FAULT_REQUEST, HANDOVER_TRANSFER, ROLLBACK_RELINQUISHMENT, CANDIDATE_STAGES, PREDECESSOR_STAGES, PRE_COMMIT_STAGES, LISTENER_ROLES, authorityStages, authorityInvariant, stagePattern } from './lib/receipts.mjs';

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
  const arm = armFile({ barrier: '/r/barrier' }, LISTENER_STALL, RUN, 'op', { deadlineMs: LISTENER_STALL_DEADLINE_MS });
  assert.equal(arm.file, '/r/barrier/daemon-listener-stall.arm.json');
  assert.deepEqual(JSON.parse(arm.content), { name: 'daemon-listener-stall', runId: RUN, operationId: 'op', deadlineMs: LISTENER_STALL_DEADLINE_MS });
  assert.ok(LISTENER_STALL_DEADLINE_MS > 35_000);
  const cmd = windowsLauncherCommand({ root: 'C:\\r', exe: 'C:\\r\\ferryx.exe', envFile: 'C:\\r\\e.json', tag: 'q06w', noBreakaway: true });
  assert.ok(cmd.endsWith('-Tag q06w -NoBreakaway'), cmd);
  assert.ok(!windowsLauncherCommand({ root: 'C:\\r', exe: 'x', envFile: 'e', tag: 't' }).includes('NoBreakaway'));
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
