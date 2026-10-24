import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { parseTransferredLine, adoptableSessions, survivalRecord, assertAdoptionSurvival, qaQuitTarget, TRANSFERRED_LINE } from './scenarios/q03b.mjs';
import { RUNNERS, parseArgs } from './run.mjs';
import { SCENARIO_IDS, newSummary, settle } from './lib/summary.mjs';
import { HOOKS, QA_FAULTS, QA_FAULT_ENV, probeHooks, missingHooksFor } from './lib/hooks.mjs';
import { qaBundleIdentifier } from './lib/guards.mjs';
import { commitEvidence } from './scenarios/q07-q09.mjs';

const RUN = '20261024t120000z';
const REPO = new URL('../../../', import.meta.url).pathname;

test('Q03b is a mac+linux scenario next to Q03 with a runner', () => {
  assert.equal(SCENARIO_IDS[SCENARIO_IDS.indexOf('Q03') + 1], 'Q03b');
  assert.equal(typeof RUNNERS.Q03b, 'function');
  const applicable = (p) => newSummary({ runId: RUN, platform: p, host: 'h' }).scenarios.find((s) => s.id === 'Q03b').applicable;
  assert.deepEqual([applicable('mac'), applicable('linux'), applicable('windows')], [true, true, false]);
});

test('the transfer log line is server.rs transfer_sessions_reply and parses in any field order', () => {
  const server = readFileSync(`${REPO}src-tauri/src/daemon/server.rs`, 'utf8');
  assert.ok(server.includes(`"${TRANSFERRED_LINE}"`), 'server.rs logs the exact message Q03b waits for');
  const line = `2026-10-24T12:00:01.000Z  INFO ferryx_lib::daemon::server: ${TRANSFERRED_LINE} requested=3 transferred=2`;
  assert.deepEqual(parseTransferredLine(line), { requested: 3, transferred: 2 });
  assert.deepEqual(parseTransferredLine(`x transferred=2 requested=2 ${TRANSFERRED_LINE}`), { requested: 2, transferred: 2 });
  assert.equal(parseTransferredLine(`${TRANSFERRED_LINE} requested=2`), null);
  assert.equal(parseTransferredLine('Received session exports requested=2 transferred=2'), null);
  assert.equal(parseTransferredLine(null), null);
});

test('only live local sessions with a child pid are adoptable', () => {
  const describe = { type: 'describeLiveSessionsOk', sessions: [
    { kind: 'local', backendId: 'a', readerOpen: true, localChildPid: 10 },
    { kind: 'local', backendId: 'b', readerOpen: false, localChildPid: 11 },
    { kind: 'ssh', backendId: 'c', connected: true, remotePid: 12 },
    { kind: 'local', backendId: 'd', readerOpen: true, localChildPid: null },
  ] };
  assert.deepEqual(adoptableSessions(describe).map((s) => s.backendId), ['a']);
});

test('a pid counts alive only when it did not exit and still has its start time', () => {
  const before = { executable: '/bin/zsh', startTime: 'Fri Oct 24 12:00:00 2026' };
  assert.equal(survivalRecord({ sessionId: 's', pid: 1, before, exit: { timedOut: true }, after: before, successorListed: true }).alive, true);
  assert.equal(survivalRecord({ sessionId: 's', pid: 1, before, exit: { pid: 1, exitedAt: 5 }, after: null }).alive, false);
  // A reused pid running another process is not survival.
  assert.equal(survivalRecord({ sessionId: 's', pid: 1, before, exit: { timedOut: true }, after: { ...before, startTime: 'Fri Oct 24 12:00:09 2026' } }).alive, false);
  assert.equal(survivalRecord({ sessionId: 's', pid: 1, before: null, exit: { timedOut: true }, after: before }).alive, false);
});

const result = () => ({ id: 'Q03b', status: 'NOT_RUN', assertions: [], evidence: ['e'] });
const rec = (id, alive) => ({ sessionId: id, pid: id.length, alive, exitedInWindow: !alive, successorListed: true });

test('happy: adopted=N alive=N and requested == transferred settle PASS', () => {
  const r = result();
  const adoption = assertAdoptionSurvival(r, { transfer: { requested: 2, transferred: 2 }, records: [rec('a', true), rec('bb', true)] });
  assert.deepEqual([adoption.adopted, adoption.alive, adoption.requested, adoption.transferred], [2, 2, 2, 2]);
  assert.equal(settle(r).status, 'PASS');
});

test('kill-adopted: a dead adopted child FAILs Q03b and keeps per-pid evidence', () => {
  const r = result();
  const adoption = assertAdoptionSurvival(r, { transfer: { requested: 2, transferred: 2 }, records: [rec('a', false), rec('bb', false)] });
  assert.deepEqual([adoption.adopted, adoption.alive], [2, 0]);
  assert.deepEqual(adoption.pids.map((p) => [p.sessionId, p.pid, p.alive]), [['a', 1, false], ['bb', 2, false]]);
  assert.equal(settle(r).status, 'FAIL');
  assert.ok(r.assertions.some((a) => !a.ok && /child pid 1 alive/.test(a.name)));
});

test('a missing or partial transfer line FAILs even when every child lives', () => {
  for (const transfer of [null, { requested: 3, transferred: 2 }]) {
    const r = result();
    assertAdoptionSurvival(r, { transfer, records: [rec('a', true), rec('bb', true)] });
    assert.equal(settle(r).status, 'FAIL', JSON.stringify(transfer));
  }
  const none = result();
  assertAdoptionSurvival(none, { transfer: { requested: 0, transferred: 0 }, records: [] });
  assert.equal(settle(none).status, 'FAIL', 'zero adopted sessions is never a pass');
});

test('Q03b\'s handover accepts a commit settled by the recorded decision (authority-recovered)', () => {
  const stage = (producerPid, s) => ({ producerPid, stage: s });
  const recovered = [stage(2, 'commit-sent'), stage(2, 'authority-recovered')];
  const line = 'pid=2 stage=authority-recovered';
  assert.equal(commitEvidence({ stages: recovered, predPid: 1, succPid: 2, guiDriven: false, recoverable: true, ownedLine: line }).ok, true);
  assert.equal(commitEvidence({ stages: recovered, predPid: 1, succPid: 2, guiDriven: false, ownedLine: line }).ok, false);
  const adopted = [stage(1, 'predecessor-prepared'), stage(1, 'predecessor-committed'), stage(2, 'authority-adopted')];
  assert.equal(commitEvidence({ stages: adopted, predPid: 1, succPid: 2, guiDriven: false, recoverable: true, ownedLine: 'pid=2 stage=authority-adopted' }).ok, true);
});

test('the mac quit targets only this run\'s QA bundle id, never the real Ferryx id', () => {
  assert.equal(qaQuitTarget({ runId: RUN, gui: { bundleId: qaBundleIdentifier(RUN) } }), qaBundleIdentifier(RUN));
  assert.throws(() => qaQuitTarget({ runId: RUN, gui: { bundleId: 'com.ferryx.app' } }), /refusing to quit com\.ferryx\.app/);
});

test('--fault kill-adopted runs only with --only Q03b', () => {
  assert.deepEqual(QA_FAULTS, { 'kill-adopted': 'Q03b' });
  assert.equal(QA_FAULT_ENV, 'FERRYX_QA_FAULT');
  assert.equal(parseArgs(['--platform', 'mac', '--only', 'Q03b', '--fault', 'kill-adopted']).fault, 'kill-adopted');
  assert.equal(parseArgs(['--platform', 'mac', '--only', 'Q03b']).fault, null);
  assert.throws(() => parseArgs(['--platform', 'mac', '--all', '--fault', 'kill-adopted']), /--only Q03b alone/);
  assert.throws(() => parseArgs(['--platform', 'mac', '--only', 'Q03,Q03b', '--fault', 'kill-adopted']), /--only Q03b alone/);
  assert.throws(() => parseArgs(['--platform', 'mac', '--only', 'Q03b', '--fault', 'nope']), /--fault must be one of kill-adopted/);
});

test('the kill-adopted product hook gates Q03b and exists in this checkout', () => {
  assert.ok(HOOKS.some((h) => h.id === 'killAdoptedFault' && h.scenarios.includes('Q03b')));
  for (const platform of ['mac', 'linux']) assert.deepEqual(missingHooksFor('Q03b', probeHooks(REPO, platform)), [], platform);
  const producers = readFileSync(`${REPO}src-tauri/src/daemon/qa_producers.rs`, 'utf8');
  assert.match(producers, /pub const QA_FAULT_ENV: &str = "FERRYX_QA_FAULT"/);
  // The fault reaches the job daemons only through the QA-named job's forwarded env.
  for (const file of ['launchd.rs', 'supervisor_linux.rs']) {
    assert.match(readFileSync(`${REPO}src-tauri/src/daemon/${file}`, 'utf8'), /"FERRYX_QA_FAULT",\n\s*\];/, file);
  }
});
