import test from 'node:test';
import assert from 'node:assert/strict';
import { SCENARIO_IDS, newSummary, selectScenarios, assertion, settle, block, applyQ00Gate, computeExitCode, validateSummary } from './lib/summary.mjs';
import { readFileSync } from 'node:fs';
import { RUNNERS, assertRunnersComplete, parseArgs, artifactPaths, selfTestExitCode, SELF_TEST_PATCH } from './run.mjs';

const RUN = '20261019t064512z';

test('every todo 12 scenario exists with its platform scope and a runner', () => {
  assert.deepEqual(SCENARIO_IDS, ['Q00', 'Q01', 'Q02', 'Q03', 'Q04', 'Q04b', 'Q05', 'Q06', 'Q06w', 'Q07', 'Q08', 'Q08b', 'Q09', 'Q10', 'Q10b', 'Q11', 'Q12', 'Q13']);
  assert.ok(assertRunnersComplete(RUNNERS, SCENARIO_IDS));
  assert.throws(() => assertRunnersComplete({ Q00() {} }, ['Q00', 'Q01']), /no runner for Q01/);
  const by = (p) => Object.fromEntries(newSummary({ runId: RUN, platform: p, host: 'h' }).scenarios.map((s) => [s.id, s.applicable]));
  assert.equal(by('mac').Q04b, true);
  assert.equal(by('linux').Q04b, false);
  assert.equal(by('windows').Q06w, true);
  assert.equal(by('mac').Q06w, false);
  assert.equal(by('windows').Q08, false);
  assert.equal(by('windows').Q09, true);
  assert.equal(by('mac').Q09, false);
  assert.equal(by('windows').Q08b, true);
});

test('selection always includes the Q00 gate and rejects unknown ids', () => {
  assert.deepEqual(selectScenarios({ only: ['Q07'] }, 'linux'), ['Q00', 'Q07']);
  assert.equal(selectScenarios({ all: true }, 'mac').length, SCENARIO_IDS.length);
  assert.throws(() => selectScenarios({ only: ['Q99'] }, 'mac'), /unknown scenario/);
});

test('a scenario passes only with evidence and all-true assertions', () => {
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'omarchy' });
  const [q00, q01, q02] = s.scenarios;
  settle(q00);
  assert.equal(q00.status, 'FAIL');
  assertion(q01, 'a', true);
  settle(q01);
  assert.equal(q01.status, 'FAIL');
  assertion(q02, 'a', true);
  q02.evidence.push('/e/q02.log');
  settle(q02);
  assert.equal(q02.status, 'PASS');
  const q04 = s.scenarios.find((x) => x.id === 'Q04');
  block(q04, 'HOOK_MISSING', 'listenerStall');
  settle(q04);
  assert.equal(q04.status, 'BLOCKED');
});

test('a non-PASS Q00 blocks the rest with an exact barrier', () => {
  const s = newSummary({ runId: RUN, platform: 'windows', host: 'maho-win' });
  s.scenarios[0].status = 'FAIL';
  assert.equal(applyQ00Gate(s), true);
  const q01 = s.scenarios.find((x) => x.id === 'Q01');
  assert.equal(q01.status, 'BLOCKED');
  assert.deepEqual(q01.barrier, { kind: 'Q00_PRECONDITION', detail: 'Q00 is FAIL' });
  assert.equal(s.scenarios.find((x) => x.id === 'Q08').status, 'NOT_APPLICABLE');
  assert.deepEqual(validateSummary(s), []);
});

test('exit code is 0 only with all PASS, clean teardown and an untouched release daemon', () => {
  const s = newSummary({ runId: RUN, platform: 'mac', host: 'mac' });
  const sel = ['Q00', 'Q01'];
  for (const r of s.scenarios.filter((x) => sel.includes(x.id))) { r.status = 'PASS'; r.assertions.push({ name: 'a', ok: true }); r.evidence.push('e'); }
  s.releaseDaemon.unchanged = true;
  assert.equal(computeExitCode(s, sel), 0);
  s.teardown.leftovers.push({ kind: 'job', name: 'x' });
  assert.equal(computeExitCode(s, sel), 1);
  s.teardown.leftovers.length = 0;
  s.releaseDaemon.unchanged = false;
  assert.equal(computeExitCode(s, sel), 1);
});

test('schema validation rejects a PASS without evidence and a BLOCKED without a barrier', () => {
  const s = newSummary({ runId: RUN, platform: 'linux', host: 'omarchy' });
  s.scenarios[0].status = 'PASS';
  s.scenarios[1].status = 'BLOCKED';
  const problems = validateSummary(s);
  assert.ok(problems.some((p) => p.startsWith('Q00: PASS without evidence')));
  assert.ok(problems.some((p) => p.startsWith('Q01: BLOCKED without')));
  assert.ok(validateSummary({ ...s, runId: 'x' }).includes('runId is not a harness run id'));
});

test('CLI arguments', () => {
  assert.deepEqual(parseArgs(['--platform', 'linux', '--only', 'Q01, Q08']).only, ['Q01', 'Q08']);
  assert.equal(parseArgs(['--platform', 'mac', '--self-check']).selfCheck, true);
  assert.throws(() => parseArgs(['--platform', 'mac']), /--all or --only/);
  assert.throws(() => parseArgs(['--platform', 'bsd', '--all']), /--platform/);
  assert.throws(() => parseArgs(['--platform', 'mac', '--all', '--release']), /unknown argument/);
  assert.equal(parseArgs(['--platform', 'linux', '--all']).worktreeSlug, 'daemon-lifetime-w5-t12-20261018');
  assert.equal(parseArgs(['--platform', 'windows', '--all', '--worktree-slug', 'w5qa-r3-20261020']).worktreeSlug, 'w5qa-r3-20261020');
  assert.throws(() => parseArgs(['--platform', 'windows', '--all', '--worktree-slug', '../x']), /--worktree-slug/);
  const run = readFileSync(new URL('./run.mjs', import.meta.url), 'utf8');
  assert.ok(!/worktreePaths\(host, '/.test(run), 'every remote worktree path honors --worktree-slug');
});

test('--self-test-reload runs Q04 against the reload patch and passes only when Q04 fails', () => {
  const opts = parseArgs(['--platform', 'linux', '--self-test-reload']);
  assert.equal(opts.selfTestReload, true);
  assert.deepEqual(opts.only, ['Q04']);
  assert.throws(() => parseArgs(['--platform', 'mac', '--self-test-reload', '--all']), /takes no --all/);
  const summary = (status) => ({ scenarios: [{ id: 'Q00', status: 'PASS' }, { id: 'Q04', status }] });
  assert.equal(selfTestExitCode(summary('FAIL')), 0);
  for (const status of ['PASS', 'BLOCKED', 'NOT_RUN']) assert.equal(selfTestExitCode(summary(status)), 1, status);
  // The patch re-adds supervisor::reload in the client's owner-wait path and restores a reload
  // that restarts the job holding the live daemon (launchctl kickstart -k / systemctl restart).
  const patch = readFileSync(new URL(`./${SELF_TEST_PATCH}`, import.meta.url), 'utf8');
  assert.match(patch, /\+\+\+ b\/src-tauri\/src\/daemon\/client\.rs/);
  assert.match(patch, /^\+\s+crate::daemon::supervisor::reload\(\)/m);
  assert.match(patch, /^\+.*"kickstart", "-k"/m);
  assert.match(patch, /^\+.*"--user", "restart"/m);
});

test('artifact paths per platform layout', () => {
  const mac = artifactPaths({ platform: 'mac', targetDir: '/t', productName: 'Ferryx QA x', name: 'A' });
  assert.equal(mac.guiExecutable, '/t/debug/bundle/macos/Ferryx QA x.app/Contents/MacOS/ferryx');
  const win = artifactPaths({ platform: 'windows', targetDir: 'C:\\t', productName: 'p', name: 'A' });
  assert.equal(win.guiExecutable, 'C:\\t\\debug\\ferryx.exe');
  assert.equal(win.cliExecutable, 'C:\\t\\debug\\ferryx-cli.exe');
});
