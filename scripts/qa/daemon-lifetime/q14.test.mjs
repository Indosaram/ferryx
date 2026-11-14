// Q14 (adoption todo 14) pure helpers: pre-boot session seeding, pane classification, and the
// saved-session wait pattern. No daemon, GUI or host is touched.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { rebootTarget, preBootSession, classifyPreBootPanes, absentPattern, STANDBY_PREFIX, QA_HOST_BOOT_TIME_ENV, BOOT_OVERRIDE_RECEIPT } from './scenarios/q14.mjs';
import { RUNNERS, assertRunnersComplete } from './run.mjs';
import { SCENARIO_IDS, applicable } from './lib/summary.mjs';
import { probeHooks, missingHooksFor } from './lib/hooks.mjs';

const panes = [
  { localSessionId: 'q14-pane-0-r', backendSessionId: 'b-old-0', incarnation: 'i0' },
  { localSessionId: 'q14-pane-1-r', backendSessionId: 'b-old-1', incarnation: 'i1' },
];

test('Q14 is registered on every platform with a runner', () => {
  assert.ok(SCENARIO_IDS.includes('Q14'));
  for (const p of ['mac', 'linux', 'windows']) assert.equal(applicable('Q14', p), true);
  assert.ok(assertRunnersComplete(RUNNERS, ['Q14']));
});

test('the qaHostBootOverride hook is present in this checkout and gates only Q14', () => {
  const repo = new URL('../../../', import.meta.url).pathname;
  for (const platform of ['mac', 'linux', 'windows']) {
    const hooks = probeHooks(repo, platform);
    assert.equal(hooks.qaHostBootOverride.present, true, platform);
    assert.deepEqual(missingHooksFor('Q14', hooks), [], platform);
  }
  const absent = probeHooks('/nowhere', 'linux', () => '', () => false);
  assert.ok(missingHooksFor('Q14', absent).some((h) => h.id === 'qaHostBootOverride'));
});

test('harness env name and receipt match host_boot.rs', () => {
  const src = readFileSync(new URL('../../../src-tauri/src/host_boot.rs', import.meta.url), 'utf8');
  assert.match(src, new RegExp(`QA_HOST_BOOT_TIME_ENV: &str = "${QA_HOST_BOOT_TIME_ENV}"`));
  assert.match(src, /QA_RUN_ID_ENV: &str = "FERRYX_QA_RUN_ID"/);
  assert.ok(src.includes(`"${BOOT_OVERRIDE_RECEIPT}"`));
});

test('rebootTarget reuses the saved active local workspace, else the QA workspace', () => {
  const saved = { activeWorkspaceId: 'home', workspaces: { home: { workspaceId: 'home', repoRoot: '/h', activeWorktreePath: '/h', worktrees: [{ path: '/h' }] } } };
  assert.deepEqual(rebootTarget(saved, { workspaceId: 'qa', repoDir: '/r' }), { workspaceId: 'home', repoRoot: '/h', worktreePath: '/h', worktrees: [{ path: '/h' }], reused: true });
  const ssh = { activeWorkspaceId: 'ssh:x', workspaces: { 'ssh:x': { repoRoot: '/s', target: { kind: 'ssh', hostId: 'h' } } } };
  assert.equal(rebootTarget(ssh, { workspaceId: 'qa', repoDir: '/r' }).workspaceId, 'qa');
  assert.equal(rebootTarget(null, { workspaceId: 'qa', repoDir: '/r' }).repoRoot, '/r');
});

test('preBootSession binds every pane to its pre-boot backend under the pre-boot epoch and keeps other workspaces', () => {
  const existing = { version: 3, workspaces: { other: { workspaceId: 'other' } } };
  const target = rebootTarget(null, { workspaceId: 'qa', repoDir: '/r' });
  const s = preBootSession(existing, { target, panes, daemonEpoch: 1700, seed: 'SEED', now: 5 });
  assert.equal(s.activeWorkspaceId, 'qa');
  assert.ok(s.workspaces.other);
  const ws = s.workspaces.qa;
  assert.equal(ws.qaRebootSeed, 'SEED');
  assert.deepEqual(ws.layout.tabs.map((t) => t.sessionId), panes.map((p) => p.localSessionId));
  for (const p of panes) {
    assert.equal(ws.terminalSessions[p.localSessionId].backendSessionId, p.backendSessionId);
    assert.equal(ws.terminalSessions[p.localSessionId].daemonEpoch, '1700');
  }
});

test('absentPattern matches only a saved session that no longer names any of the texts', () => {
  const re = new RegExp(absentPattern(['b-old-0', 'b-old-1']));
  assert.equal(re.test('{"timestamp":1,"x":"b-old-1"}'), false);
  assert.equal(re.test('{"timestamp":1,"x":"b-new"}'), true);
  assert.equal(re.test(''), false, 'an empty or unwritten file is not a settled session');
  assert.equal(new RegExp(absentPattern(['a.b'])).test('{"timestamp":1,"v":"axb"}'), true, 'texts are literal');
});

test('classifyPreBootPanes counts panes still bound to a pre-boot id as Reconnecting', () => {
  const saved = (bindings) => ({ workspaces: { qa: { terminalSessions: Object.fromEntries(Object.entries(bindings).map(([k, v]) => [k, { backendSessionId: v }])) } } });
  const fixed = classifyPreBootPanes(saved({ 'q14-pane-0-r': null, 'q14-pane-1-r': 'b-new' }), { workspaceId: 'qa', panes, liveIds: ['b-new'] });
  assert.deepEqual(fixed.counts, { preBoot: 2, exited: 1, replaced: 1, reconnecting: 0, other: 0, missing: 0 });
  const standby = classifyPreBootPanes(saved({ 'q14-pane-0-r': `${STANDBY_PREFIX}q14-pane-0-r`, 'q14-pane-1-r': null }), { workspaceId: 'qa', panes, liveIds: [] });
  assert.equal(standby.counts.exited, 2);
  const stuck = classifyPreBootPanes(saved({ 'q14-pane-0-r': 'b-old-0', 'q14-pane-1-r': 'b-old-1' }), { workspaceId: 'qa', panes, liveIds: [] });
  assert.equal(stuck.counts.reconnecting, 2);
  const gone = classifyPreBootPanes(saved({ 'q14-pane-0-r': 'b-zzz' }), { workspaceId: 'qa', panes, liveIds: [] });
  assert.deepEqual([gone.counts.other, gone.counts.missing], [1, 1]);
});
