import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  captureIdentity, daemonGroupAlive, fleetTeardownClean, linuxProcess, mapHostProcess, mapRecordedIdentity, observeIdentity, paneTeardownState,
  teardownIdentityGroup, waitIdentityExit,
} from './f1-process-identity.mjs';

// Resolves on the child's first stdout line; bounded, and rejects if the child exits first.
function firstLine(child, label, deadlineMs = 15_000) {
  return new Promise((resolveLine, rejectLine) => {
    let buffer = '';
    const done = (fn, value) => { clearTimeout(timer); child.stdout.off('data', onData); child.off('exit', onExit); fn(value); };
    const onData = (chunk) => { buffer += chunk; const at = buffer.indexOf('\n'); if (at >= 0) done(resolveLine, buffer.slice(0, at)); };
    const onExit = (code, signal) => done(rejectLine, new Error(`${label} exited (${code}/${signal}) before its first line`));
    const timer = setTimeout(() => done(rejectLine, new Error(`${label} printed nothing within ${deadlineMs}ms`)), deadlineMs);
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', onData);
    child.once('exit', onExit);
  });
}

const PANE_FIXTURE = `import { spawn } from 'node:child_process';
const [mode] = process.argv.slice(3);
if (mode === 'leader') {
  const member = spawn(process.execPath, [process.argv[1], process.argv[2], 'member'], { stdio: 'ignore' });
  console.log(JSON.stringify({ member: member.pid }));
  process.stdin.once('data', () => process.exit(0));
} else {
  console.log('ready');
  setInterval(() => {}, 1 << 30);
}
`;

// Real processes through the real identity API in this process's own PID namespace (no helper).
// Subscriptions are armed before each action; every wait is a bounded exit/line event.
async function liveProcessChecks(check, stateRoot) {
  const dir = join(stateRoot, 'f1-live-identity');
  mkdirSync(dir, { recursive: true });
  const script = join(dir, 'pane-fixture.mjs');
  writeFileSync(script, PANE_FIXTURE);
  const logDir = join(dir, 'panes');
  const savedHelper = process.env.F1_HOST_HELPER;
  const savedIsolated = process.env.F1_PID_ISOLATED;
  delete process.env.F1_HOST_HELPER;
  delete process.env.F1_PID_ISOLATED;
  const children = [];
  const launch = (extra, options = {}) => { const child = spawn(process.execPath, [script, logDir, ...extra], { stdio: ['pipe', 'pipe', 'ignore'], ...options }); children.push(child); return child; };
  try {
    const pane = launch(['pane']);
    await firstLine(pane, 'pane fixture');
    const identity = captureIdentity(pane.pid);
    const before = observeIdentity(identity);
    const ended = [{ kind: 'stdin-closed' }];
    const live = paneTeardownState(ended, before, script, logDir);
    const stale = observeIdentity({ ...identity, startStamp: String(BigInt(identity.startStamp) + 1n) });
    const foreign = mapRecordedIdentity({ ...identity, pidNamespace: 'pid:[1]' });
    check('f1-live-pane-identity-fails-teardown', Boolean(identity) && before.verdict === 'match' && before.live === true && live.clean === false && live.signalPid === pane.pid
      && fleetTeardownClean([{ daemons: [], panes: [live] }]) === false && stale.verdict === 'gone' && foreign === null,
    { identity, before, live, stale, foreign, note: 'an end record never hides a live identical process; reused start stamp and unmapped namespace are refused' });

    // Isolated without a helper: no local fallback, the identity is unknown and teardown stays dirty.
    process.env.F1_PID_ISOLATED = '1';
    let mapRefused = null;
    try { mapRecordedIdentity(identity); } catch (error) { mapRefused = error.code ?? String(error); }
    const failClosed = { mapRefused, observed: observeIdentity(identity) };
    delete process.env.F1_PID_ISOLATED;
    check('f1-isolated-helper-absent-fails-closed', failClosed.mapRefused === 'HELPER_ABSENT' && failClosed.observed.live === null
      && paneTeardownState(ended, failClosed.observed, script, logDir).clean === false, failClosed);

    const timeout = await waitIdentityExit(identity, 50);
    const exit = await waitIdentityExit(identity, 10_000, () => pane.kill('SIGTERM'));
    const after = paneTeardownState(ended, observeIdentity(identity), script, logDir);
    check('f1-live-pane-exit-is-event-evidence', timeout.exited === false && timeout.live === true && exit.exited === true && exit.live === false && after.clean === true
      && fleetTeardownClean([{ daemons: [], panes: [after] }]) === true, { timeout, exit, after });

    const leader = launch(['leader'], { detached: true });
    const { member } = JSON.parse(await firstLine(leader, 'group leader'));
    const leaderIdentity = captureIdentity(leader.pid);
    const snapshot = daemonGroupAlive(leaderIdentity);
    const leaderExit = await waitIdentityExit(leaderIdentity, 10_000, () => leader.stdin.write('exit\n'));
    const remaining = daemonGroupAlive(leaderIdentity);
    const dirty = fleetTeardownClean([{ daemons: [{ exited: { code: 0 }, groupAlive: remaining }], panes: [] }]);
    const teardown = await teardownIdentityGroup(leaderIdentity, 'SIGTERM', 10_000);
    const final = daemonGroupAlive(leaderIdentity);
    check('f1-live-group-survives-leader-exit', snapshot === true && leaderExit.exited === true && remaining === true && dirty === false
      && teardown.members.every((entry) => entry.exited === true) && final === false && Number.isInteger(member),
    { snapshot, leaderExit, remaining, dirty, teardown, final, member });
  } catch (error) {
    check('f1-live-process-identity', false, { error: String(error?.stack ?? error) });
  } finally {
    for (const child of children) { try { child.kill('SIGKILL'); } catch { /* gone */ } }
    if (savedHelper === undefined) delete process.env.F1_HOST_HELPER; else process.env.F1_HOST_HELPER = savedHelper;
    if (savedIsolated === undefined) delete process.env.F1_PID_ISOLATED; else process.env.F1_PID_ISOLATED = savedIsolated;
  }
}

export async function f1NamespaceUnitChecks(check, stateRoot) {
  const script = '/fixture/f1-pane.mjs';
  const logDir = '/fixture/panes';
  const local = { pid: 2, namespacePid: 2, pidNamespace: 'pid:[nested]', startStamp: '900' };
  const host = { ...local, pid: 42002 };
  const collision = { ...host, pid: 2, pidNamespace: 'pid:[outer]' };
  const mappings = [
    ['nested-namespace-maps-to-host-not-collision', mapHostProcess(local, [collision, host])?.pid === 42002],
    ['no-namespace-maps-to-self', mapHostProcess(host, [host])?.pid === 42002],
    ['missing-mapping-is-not-local-pid', mapHostProcess(local, [collision]) === null],
    ['reused-pid-stamp-refused', mapHostProcess(local, [{ ...host, startStamp: '901' }]) === null],
    ['ambiguous-mapping-refused', mapHostProcess(local, [host, { ...host, pid: 42003 }]) === null],
  ].map(([label, ok]) => ({ label, ok }));
  check('f1-namespace-host-pid-mapping', mappings.every((entry) => entry.ok), mappings);

  if (process.platform !== 'win32') {
    const procRoot = join(stateRoot, 'f1-proc');
    const processRoot = join(procRoot, '42002');
    mkdirSync(join(processRoot, 'ns'), { recursive: true });
    const fields = Array(20).fill('0');
    fields[0] = 'S'; fields[2] = '42000'; fields[19] = '900';
    writeFileSync(join(processRoot, 'stat'), `42002 (pane (fixture)) ${fields.join(' ')}\n`);
    writeFileSync(join(processRoot, 'status'), 'Name:\tpane\nNSpid:\t42002\t18\t2\n');
    writeFileSync(join(processRoot, 'cmdline'), `bun\0${script}\0${logDir}\0`);
    symlinkSync('pid:[nested]', join(processRoot, 'ns', 'pid'));
    const parsed = linuxProcess(42002, procRoot);
    check('f1-host-proc-records-nested-pid-and-start-stamp', parsed?.namespacePid === 2 && parsed?.startStamp === '900'
      && parsed?.pgrp === 42000 && parsed?.pidNamespace === 'pid:[nested]' && mapHostProcess(local, [parsed])?.pid === 42002, parsed);
  }

  const live = { verdict: 'match', live: true, pid: 42, args: [script, logDir] };
  const ended = [{ kind: 'stdin-closed' }];
  const cases = [
    ['exited-pane-passes', ended, { verdict: 'gone', live: false }, true, null],
    ['missing-mapping-with-end-fails', ended, { verdict: 'unknown', live: null }, false, null],
    ['missing-mapping-without-end-fails', [], { verdict: 'unknown', live: null }, false, null],
    ['matching-live-pane-fails', [], live, false, 42],
    ['end-does-not-hide-matching-live-pane', ended, live, false, 42],
    ['live-nested-pane-unreachable-fails', ended, { ...live, verdict: 'unreachable', pid: null }, false, null],
    ['unmatched-owner-never-signalled', [], { ...live, args: ['/owner/attach'] }, false, null],
    ['namespace-local-collision-is-irrelevant', ended, { verdict: 'gone', live: false, pid: 2, args: ['/owner/attach'] }, true, null],
  ].map(([label, records, observation, clean, signalPid]) => {
    const result = paneTeardownState(records, observation, script, logDir);
    return { label, result, ok: result.clean === clean && result.signalPid === signalPid };
  });
  check('f1-namespace-pane-teardown', cases.every((entry) => entry.ok), cases);

  const receipt = { daemons: [{ exited: { code: 0 }, groupAlive: false }], panes: [{ clean: true }] };
  const verdicts = [
    ['exited-fleet-passes', fleetTeardownClean([receipt]), true],
    ['live-daemon-group-fails', fleetTeardownClean([{ ...receipt, daemons: [{ exited: { code: 0 }, groupAlive: true }] }]), false],
    ['unknown-daemon-group-fails', fleetTeardownClean([{ ...receipt, daemons: [{ exited: { code: 0 }, groupAlive: null }] }]), false],
    ['signalled-but-live-pane-fails', fleetTeardownClean([{ ...receipt, panes: [{ clean: false, signalled: true }] }]), false],
  ].map(([label, got, expected]) => ({ label, got, expected, ok: got === expected }));
  check('f1-namespace-fleet-teardown-verdict', verdicts.every((entry) => entry.ok), verdicts);

  if (process.platform !== 'win32') {
    const wrapper = join(dirname(fileURLToPath(import.meta.url)), 'f1-pid-isolate.sh');
    const nested = spawnSync('/bin/sh', [wrapper, 'pane', 'argument with spaces'], {
      env: { ...process.env, F1_PID_ISOLATED: '1', F1_ISOLATION_ROOT: '/not-used', F1_BUN: '/bin/echo' }, encoding: 'utf8', timeout: 5_000,
    });
    check('f1-isolation-wrapper-nested-does-not-enter-bwrap', nested.status === 0 && nested.stdout === 'pane argument with spaces\n', { status: nested.status, stdout: nested.stdout, stderr: nested.stderr });
  }
  if (process.platform === 'linux') await liveProcessChecks(check, stateRoot);
}
