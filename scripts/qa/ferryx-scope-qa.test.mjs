import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { validateRoot, validateEvidence, prepare, cleanup } from './ferryx-scope-fixtures.mjs';
import { symlinkSync, unlinkSync, rmdirSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { collectChild, spawnChild } from './ferryx-scope-lifecycle.mjs';

test('rejects an unsafe root before accessing resources', async () => {
  // Given a personal directory, when validation runs, then no adoption is allowed.
  await expect(Promise.resolve().then(() => validateRoot('/Users/indo'))).rejects.toThrow();
});

test('rejects claimed success without actual observations', () => {
  // Given a manifest assertion, when evaluated, then it cannot certify a scenario.
  expect(() => validateEvidence('foundation', { status: 'passed' })).toThrow();
});

test('rejects push evidence without a physical-device receipt', () => {
  // Given browser-only delivery, when evaluated, then device delivery remains blocked.
  expect(() => validateEvidence('push', { screenshots: ['browser.png'], simulated: true })).toThrow();
});

test('rejects a fixture child symlink without deleting its target', () => {
  // Given a private fixture with a redirected home.
  const fixture = prepare();
  const home = join(fixture.root, 'home');
  rmdirSync(home);
  symlinkSync('/Users/indo', home);
  try {
    // When validating, then reject before resource access.
    expect(() => validateRoot(fixture.root)).toThrow('UNSAFE_CHILD');
  } finally {
    unlinkSync(home);
    mkdirSync(home, { mode: 0o700 });
    cleanup(fixture.root);
  }
});

async function invoke(args) {
  const child = spawnChild([process.execPath, new URL('./ferryx-scope-qa.mjs', import.meta.url).pathname, ...args]);
  child.stdin.end();
  const [stdout, stderr, code] = await collectChild(child);
  return { report: JSON.parse(stdout), stderr, code };
}

test('real self-test records HTTP results and cleanup without claiming product success', async () => {
  // Given the executable driver, when self-test runs, then assert real boundary receipts.
  const { report, code, stderr } = await invoke(['--self-test']);
  expect(code).toBe(0);
  expect(stderr).toBe('');
  expect(report.status).toBe('self-test-passed');
  expect(report.productVerified).toBe(false);
  const boundaryRun = report.commands.find(command => command.argv.includes('scripts/qa/ferryx-scope-boundary.test.mjs'));
  expect(boundaryRun.exitCode).toBe(0);
  expect(boundaryRun.stdout).toMatch(/3 passed/);
  expect(report.commands[0].exitCode).toBe(0);
  expect(report.http[0].headers).toContain('HTTP/1.1 200');
  expect(report.cleanup.map(item => item.loopbackServersClosed ?? item.removed)).toEqual([2, true]);
}, 10000);

test('all scenarios exit blocked on real CLI boundary with owned listeners closed', async () => {
  // Given an explicit fixture manifest, when scenarios run, then no fake pass escapes.
  const fixture = prepare();
  try {
    for (const scenario of ['foundation', 'design', 'mobile', 'push']) {
      const { report, code } = await invoke(['--scenario', scenario, '--root', fixture.root]);
      expect(code).toBe(2);
      expect(report.status).toBe('blocked');
      expect(report.missingObservables.length).toBeGreaterThan(0);
      expect(report.cleanup[0].loopbackServersClosed).toBe(2);
    }
  } finally { cleanup(fixture.root); }
}, 10000);

test('prepare emits retention receipt and cleanup consumes only that fixture', async () => {
  // Given prepare, when explicit cleanup runs, then it records owned root removal.
  const prepared = await invoke(['--prepare', '--scenario', 'foundation']);
  let removed = false;
  try {
    expect(prepared.code).toBe(0);
    expect(prepared.report.status).toBe('prepared');
    const cleaned = await invoke(['--cleanup', '--root', prepared.report.fixture.root]);
    removed = cleaned.report.cleanup.some(receipt => receipt.removed === true);
    expect(cleaned.code).toBe(0);
    expect(removed).toBe(true);
  } finally { if (!removed && prepared.report.fixture?.root) cleanup(prepared.report.fixture.root); }
}, 10000);
