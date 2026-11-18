// Contract tests for the three harness defects found while gating the daemon-lifetime run:
// build-dependency probes that must not shell out or accept an empty submodule, the mac pane click
// point, the cliclick modifier release, and Q14's single-budget pane settle. No daemon, GUI, host or
// real clock is touched.
import test from 'node:test';
import assert from 'node:assert/strict';
import { BUILD_DEPS, joinFor, missingBuildDeps } from './run.mjs';
import { paneClickPoint, SIDEBAR_MAX_PX } from './scenarios/q10-q13.mjs';
import { settlePreBootPanes, classifyPreBootPanes, SETTLE_DEADLINE_MS } from './scenarios/q14.mjs';
import { CLICLICK_RELEASE_KEYS, cliclickReleaseArgv, createMacLocalInput } from './lib/mahord.mjs';

const PANES = [
  { localSessionId: 'q14-pane-0-r', backendSessionId: 'b-old-0' },
  { localSessionId: 'q14-pane-1-r', backendSessionId: 'b-old-1' },
];
const WORKSPACE = 'qa-daemon-lifetime';

/** Saved-session text whose panes are bound to `bound(localSessionId)` (undefined = entry absent). */
const savedText = (bound) => JSON.stringify({
  version: 2,
  timestamp: 1_700_000_000_000,
  activeWorkspaceId: WORKSPACE,
  workspaces: {
    [WORKSPACE]: {
      terminalSessions: Object.fromEntries(PANES.map((p) => [p.localSessionId, { backendSessionId: bound(p.localSessionId) }])),
    },
  },
});

/** An agent whose `exists` answers from a set of files. */
function agentWithFiles(files) {
  const asked = [];
  return {
    asked,
    call: async (method, { file }) => {
      assert.equal(method, 'exists', `unexpected agent method ${method}`);
      asked.push(file);
      return files.includes(file);
    },
  };
}

test('build deps are probed as files under src-tauri/vendor/ghostty, never the retired path', () => {
  // .gitmodules and both trees track the gitlink at src-tauri/vendor/ghostty; src-tauri/ghostty has
  // not existed since the submodule moved, and a directory-only probe passed on an empty gitlink.
  assert.deepEqual(BUILD_DEPS.ghostty, ['src-tauri', 'vendor', 'ghostty', 'build.zig']);
  assert.notDeepEqual(BUILD_DEPS.ghostty, ['src-tauri', 'ghostty']);
  assert.deepEqual(BUILD_DEPS.tsc, ['ui', 'node_modules', 'typescript', 'bin', 'tsc']);
});

test('missingBuildDeps reports only absent files and never shells out', async () => {
  const mac = { platform: 'mac' };
  const all = [joinFor('mac', '/w', ...BUILD_DEPS.tsc), joinFor('mac', '/w', ...BUILD_DEPS.ghostty)];
  assert.deepEqual(await missingBuildDeps({ host: mac, agent: agentWithFiles(all), worktree: '/w' }), []);

  const agent = agentWithFiles([]);
  assert.deepEqual(await missingBuildDeps({ host: mac, agent, worktree: '/w' }), ['tsc', 'ghostty']);
  // Probed with the agent's own `exists` (no `sh -c`), so a Windows lane without a POSIX shell works.
  assert.equal(agent.asked.length, 2);

  // An uninitialized submodule is an EMPTY directory: the build.zig file is what decides.
  const emptyGhostty = [joinFor('mac', '/w', ...BUILD_DEPS.tsc)];
  assert.deepEqual(await missingBuildDeps({ host: mac, agent: agentWithFiles(emptyGhostty), worktree: '/w' }), ['ghostty']);
});

test('build dep probes join with the host path flavour', async () => {
  const agent = agentWithFiles([]);
  await missingBuildDeps({ host: { platform: 'windows' }, agent, worktree: 'C:\\wt\\slug' });
  assert.deepEqual(agent.asked, [
    'C:\\wt\\slug\\ui\\node_modules\\typescript\\bin\\tsc',
    'C:\\wt\\slug\\src-tauri\\vendor\\ghostty\\build.zig',
  ]);
});

test('the mac pane click point clears the widest sidebar at every supported width', () => {
  for (const w of [800, 1280, 1920.5]) {
    const rect = { left: 100, top: 50, right: 100 + w, bottom: 50 + 850 };
    const p = paneClickPoint(rect, 'mac');
    assert.ok(p, `no point for width ${w}`);
    assert.ok(p.x > rect.left + SIDEBAR_MAX_PX, `x=${p.x} is inside the sidebar for width ${w}`);
    assert.ok(p.x < rect.right, `x=${p.x} leaves the window for width ${w}`);
    assert.equal(p.y, Math.round(rect.top + 850 * 0.55));
  }
  // The default 1280x850 window: published as a fix for terminalClick x=1220 in a 1280-wide window.
  assert.deepEqual(paneClickPoint({ left: 0, top: 0, right: 1280, bottom: 850 }, 'mac'), { x: 880, y: 468 });
});

test('a mac window too narrow for a pane yields no click target instead of another window', () => {
  // 650 - 480 leaves 170 px: no pane. Clicking anyway would hit the sidebar or whatever is beside it.
  assert.equal(paneClickPoint({ left: 0, top: 0, right: 650, bottom: 850 }, 'mac'), null);
  assert.equal(paneClickPoint({ left: 0, top: 0, right: 300, bottom: 850 }, 'mac'), null);
  assert.equal(paneClickPoint({ left: 0, top: 0, right: 1280.4, bottom: 200 }, 'mac'), null);
});

test('non-mac platforms keep the established click coordinates', () => {
  const rect = { left: 10, top: 20, right: 1290, bottom: 870 };
  // rect.left + w*0.65, rect.top + h*0.55 - unchanged from the gated linux/windows runs.
  assert.deepEqual(paneClickPoint(rect, 'linux'), { x: 842, y: 488 });
  assert.deepEqual(paneClickPoint(rect, 'windows'), { x: 842, y: 488 });
});

test('the cliclick modifier release is one ku: command with plain key names', () => {
  assert.deepEqual(cliclickReleaseArgv(), ['ku:alt,cmd,ctrl,shift']);
  // `ku:alt,ku:cmd,...` is rejected by cliclick 5.1: 'Invalid key "ku:cmd" given as argument to
  // command "ku"' with exit 1, so the previous argv released nothing.
  for (const arg of cliclickReleaseArgv()) assert.equal((arg.match(/ku:/g) ?? []).length, 1);
  assert.deepEqual([...CLICLICK_RELEASE_KEYS], ['alt', 'cmd', 'ctrl', 'shift']);
});

function macInput(handler) {
  const calls = [];
  const exec = async (command, args) => {
    calls.push({ command, args });
    const r = await handler(command, args);
    if (r instanceof Error) throw r;
    return r;
  };
  return { calls, input: createMacLocalInput({ exec }) };
}

test('releaseModifiers reports a cliclick release as done only when cliclick exited 0', async () => {
  const { calls, input } = macInput((command) => (command.endsWith('cliclick') ? { status: 0, stdout: '', stderr: '' } : { status: 0 }));
  const r = await input.releaseModifiers();
  assert.deepEqual(r, { ok: true, via: 'cliclick', keys: ['alt', 'cmd', 'ctrl', 'shift'] });
  assert.deepEqual(calls.map((c) => c.args), [['ku:alt,cmd,ctrl,shift']]);
});

test('releaseModifiers falls back per key and reports a signalled key-up as a failure', async () => {
  // cliclick missing, and System Events killed the cmd release: proc.run resolves status null for a
  // child that died from a signal, which must not read as a successful key-up.
  const { input } = macInput((command, args) => {
    if (command.endsWith('cliclick')) return new Error('spawn cliclick ENOENT');
    const script = args[1];
    if (script.endsWith('key up command')) return { status: null, stdout: '', stderr: '' };
    return { status: 0, stdout: '', stderr: '' };
  });
  const r = await input.releaseModifiers();
  assert.equal(r.ok, false);
  assert.equal(r.via, 'osascript');
  assert.deepEqual(r.keys, ['alt', 'cmd', 'ctrl', 'shift']);
  const cmd = r.results.find((x) => x.name === 'cmd');
  assert.equal(cmd.ok, false);
  assert.equal(cmd.status, null);
  // The other three released and stay reported as such, one record per named modifier.
  assert.deepEqual(r.results.filter((x) => x.ok).map((x) => x.name), ['alt', 'ctrl', 'shift']);
});

test('releaseModifiers reports a non-zero osascript exit with its stderr', async () => {
  const { input } = macInput((command, args) => {
    if (command.endsWith('cliclick')) return new Error('spawn cliclick ENOENT');
    if (args[1].endsWith('key up shift')) return { status: 1, stdout: '', stderr: 'execution error: Invalid key' };
    return { status: 0, stdout: '', stderr: '' };
  });
  const r = await input.releaseModifiers();
  const shift = r.results.find((x) => x.name === 'shift');
  assert.equal(r.ok, false);
  assert.deepEqual({ ok: shift.ok, status: shift.status, error: shift.error }, { ok: false, status: 1, error: 'execution error: Invalid key' });
});

/** A clock that only moves when the sleep stub says so: no wall time, no real timer. */
function fakeClock(start) {
  let t = start;
  return {
    now: () => t,
    sleep: async (ms) => { t += ms; },
    advance: (ms) => { t += ms; },
    at: () => t,
  };
}

const describeEmpty = async () => ({ type: 'describeLiveSessionsOk', sessions: [] });

test('settlePreBootPanes settles on the first observation that is already exited', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  const r = await settlePreBootPanes({
    readSaved: async () => savedText(() => null),
    describe: describeEmpty,
    workspaceId: WORKSPACE, panes: PANES, restoredAt,
    now: clock.now, sleep: clock.sleep,
  });
  assert.equal(r.settled, true);
  assert.equal(r.polls, 1);
  assert.equal(r.expired, false);
  // A duration from restoredAt, not an epoch timestamp.
  assert.equal(r.elapsedMs, 0);
  assert.equal(r.verdict.counts.exited, 2);
});

test('settlePreBootPanes re-polls a rebound pane and measures elapsed time from the restore', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  let reads = 0;
  const r = await settlePreBootPanes({
    readSaved: async () => {
      reads += 1;
      // First poll: the saved session still binds the pre-boot ids. Then the replacement shell lands.
      return reads === 1 ? savedText((id) => (id === 'q14-pane-0-r' ? 'b-old-0' : 'b-new-1')) : savedText(() => null);
    },
    describe: async () => ({ sessions: [{ kind: 'local', backendId: 'b-new-1', readerOpen: true }] }),
    workspaceId: WORKSPACE, panes: PANES, restoredAt, now: clock.now, sleep: clock.sleep,
  });
  assert.equal(r.settled, true);
  assert.equal(r.polls, 2);
  assert.equal(r.elapsedMs, 500);
  assert.ok(r.elapsedMs < SETTLE_DEADLINE_MS);
  assert.equal(r.verdict.counts.exited, 2);
});

test('settlePreBootPanes refuses an observation that crossed the deadline', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  // The describe hangs past the budget; the answer is settled but late, so it is not accepted.
  const r = await settlePreBootPanes({
    readSaved: async () => savedText(() => null),
    describe: async () => { clock.advance(SETTLE_DEADLINE_MS + 1); return { sessions: [] }; },
    workspaceId: WORKSPACE, panes: PANES, restoredAt, now: clock.now, sleep: clock.sleep,
  });
  assert.equal(r.settled, false);
  assert.equal(r.expired, true);
  assert.equal(r.polls, 1);
  assert.ok(r.elapsedMs > SETTLE_DEADLINE_MS);
});

test('settlePreBootPanes stops at the deadline and never sleeps past it', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  const sleeps = [];
  const r = await settlePreBootPanes({
    readSaved: async () => savedText((id) => (id === 'q14-pane-0-r' ? 'b-old-0' : null)),
    describe: describeEmpty,
    workspaceId: WORKSPACE, panes: PANES, restoredAt,
    now: clock.now,
    sleep: async (ms) => { sleeps.push(ms); clock.advance(ms); },
  });
  assert.equal(r.settled, false);
  assert.equal(r.verdict.counts.reconnecting, 1);
  assert.ok(sleeps.every((ms) => ms <= 500), JSON.stringify(sleeps));
  // The whole budget is spent once, and the wait ends exactly on the deadline.
  assert.equal(sleeps.reduce((a, b) => a + b, 0), SETTLE_DEADLINE_MS);
  assert.equal(r.elapsedMs, SETTLE_DEADLINE_MS);
});

test('settlePreBootPanes never classifies an unparsable save as settled', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  // Truncated by a concurrent writer: no pre-boot id survives in the text, so a marker-only wait
  // would call this settled. The classification comes from the JSON parsed out of this same text,
  // and an unreadable save is never a settled state.
  const broken = savedText(() => null).slice(0, 40);
  const r = await settlePreBootPanes({
    readSaved: async () => broken,
    describe: describeEmpty,
    workspaceId: WORKSPACE, panes: PANES, restoredAt, now: clock.now, sleep: clock.sleep,
  });
  assert.equal(r.settled, false);
  assert.equal(r.saved, null);
  assert.equal(r.verdict.counts.missing, 2);
  assert.ok(!broken.includes(PANES[0].backendSessionId));
  assert.equal(r.elapsedMs, SETTLE_DEADLINE_MS);
});

test('settlePreBootPanes keeps one snapshot: verdict rows describe the returned saved session', async () => {
  const clock = fakeClock(1_700_000_000_000);
  const restoredAt = clock.at();
  let reads = 0;
  const r = await settlePreBootPanes({
    readSaved: async () => {
      reads += 1;
      return reads === 1 ? savedText((id) => (id === 'q14-pane-1-r' ? 'b-old-1' : null)) : savedText(() => null);
    },
    describe: describeEmpty,
    workspaceId: WORKSPACE, panes: PANES, restoredAt, now: clock.now, sleep: clock.sleep,
  });
  assert.equal(r.settled, true);
  for (const row of r.verdict.rows) {
    assert.equal(r.saved.workspaces[WORKSPACE].terminalSessions[row.localSessionId].backendSessionId, row.bound);
  }
  assert.deepEqual(classifyPreBootPanes(r.saved, { workspaceId: WORKSPACE, panes: PANES, liveIds: r.liveIds }).counts, r.verdict.counts);
});
