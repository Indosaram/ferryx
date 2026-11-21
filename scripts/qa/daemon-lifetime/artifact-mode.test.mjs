import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, stat, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from './run.mjs';
import { assertIdleArtifactRun, claimFreshDirectories, resolveProductRoot, separateArtifactPaths } from './lib/artifact-mode.mjs';
import { createJobs } from './lib/jobs.mjs';

const runId = '20261119t010203z';
const sha = '2'.repeat(64);
const reuse = ['--platform', 'linux', '--reuse-artifacts', '/sets/build-a', '--manifest-sha256', sha, '--run-id', runId];

test('build, verify-only and desktop reuse are distinct CLI modes', () => {
  const build = parseArgs(['--platform', 'mac', '--build-only', '/sets/build-a']);
  assert.equal(build.buildOnly, '/sets/build-a');
  assert.deepEqual(build.only, []);
  assert.equal(parseArgs([...reuse, '--self-check']).selfCheck, true);
  assert.deepEqual(parseArgs([...reuse, '--only', 'Q10,Q11']).only, ['Q10', 'Q11']);
  assert.equal(parseArgs(['--platform', 'linux', '--only', 'Q01']).reuseArtifacts, null);
});

test('a reuse consumer may name a separate product checkout, but only with an absolute path in --reuse-artifacts mode', () => {
  const defaultRoot = '/driver/checkout';
  const overrideRoot = '/product/checkout';
  const reuseOpts = parseArgs([...reuse, '--only', 'Q10']);
  for (const empty of [undefined, '', null]) assert.equal(resolveProductRoot({ defaultRoot, overrideRoot: empty, opts: {} }), defaultRoot);
  assert.equal(resolveProductRoot({ defaultRoot, overrideRoot, opts: reuseOpts }), overrideRoot);
  for (const opts of [parseArgs(['--platform', 'linux', '--only', 'Q01']), parseArgs(['--platform', 'linux', '--build-only', '/set'])]) {
    assert.throws(() => resolveProductRoot({ defaultRoot, overrideRoot, opts }), { code: 'QA_ARTIFACT_OPTIONS' });
  }
  assert.throws(() => resolveProductRoot({ defaultRoot, overrideRoot: 'product/checkout', opts: reuseOpts }), { code: 'QA_ARTIFACT_OPTIONS' });
});

test('ambiguous modes and incomplete or duplicate pins are refused before building', () => {
  for (const args of [
    ['--platform', 'mac', '--build-only'],
    ['--platform', 'linux', '--build-only', '/set', '--only', 'Q01'],
    ['--platform', 'windows', '--only', 'Q01', '--reuse-artifacts', '/set'],
    [...reuse, '--build-only', '/x'], [...reuse, '--self-check', '--only', 'Q01'],
    [...reuse, '--only', 'Q08b', '--v1-rev', 'HEAD'], [...reuse, '--self-test-reload'],
    [...reuse, '--self-check', '--manifest-sha256', sha],
    ['--platform', 'linux', '--only', 'Q01', '--manifest-sha256', sha],
  ]) assert.throws(() => parseArgs(args), { code: 'QA_ARTIFACT_OPTIONS' });
});

test('a reused run ID never adopts an existing directory, evidence, or a partial claim', async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'qa-artifact-claim-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const a = path.join(root, 'bulk', runId), b = path.join(root, 'runtime', runId);
  await mkdir(b, { recursive: true });
  await writeFile(path.join(b, 'previous'), 'original');
  await assert.rejects(claimFreshDirectories([a, b]), { code: 'EEXIST' });
  await assert.rejects(stat(a), { code: 'ENOENT' });
  assert.equal(await readFile(path.join(b, 'previous'), 'utf8'), 'original');
  await claimFreshDirectories([a, a]);
  await assert.rejects(claimFreshDirectories([a]), { code: 'EEXIST' });
  const platform = { darwin: 'mac', win32: 'windows' }[process.platform] ?? 'linux';
  for (const p of [a, path.join(a, 'artifacts'), path.dirname(a)]) {
    assert.throws(() => separateArtifactPaths(p, [a], platform), { code: 'QA_ARTIFACT_PATH_OVERLAP' });
  }
});

test('an existing job, task file, or executable prevents reuse without touching it', async () => {
  const root = '/home/test/qa/' + runId;
  const input = (names, xml, rows) => ({
    jobs: { listQaJobs: async (o) => { assert.equal(o.strict, true); return names; }, listQaTaskXml: async () => xml },
    agent: { call: async (method) => { assert.equal(method, 'table'); return rows; } },
    runDirs: [root], platform: 'linux', runId,
  });
  const other = [{ pid: 7, executable: '/usr/bin/editor' }];
  await assertIdleArtifactRun(input([], [], other));
  for (const args of [[['occupied'], [], other], [[], ['task.xml'], other], [[], [], [{ pid: 8, executable: root + '/old/ferryx' }]]]) {
    await assert.rejects(assertIdleArtifactRun(input(...args)), { code: 'QA_ARTIFACT_RUN_IN_USE' });
  }
  await assert.rejects(assertIdleArtifactRun(input([], [], [])), { code: 'QA_ARTIFACT_INVENTORY' });
});

test('failure to query the task manager is not proof that a reused job name is free', async () => {
  for (const platform of ['mac', 'windows', 'linux']) {
    const jobs = createJobs({ host: { platform }, agent: { call: async () => ({ status: 1, stdout: '', stderr: 'unavailable' }) }, runId, layout: { logs: '/logs' } });
    await assert.rejects(jobs.listQaJobs({ strict: true }));
  }
});
