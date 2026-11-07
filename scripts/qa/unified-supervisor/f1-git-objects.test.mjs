import { expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gitObjectsSource } from './f1-git-objects.mjs';
import { f1IsolationChecks } from './f1-isolation-check.mjs';
import { gitEnv } from './git-env.mjs';

for (const variant of ['explicit', 'missing', 'relative', 'empty', 'repository']) {
  test(`F1 git objects ${variant}`, () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'f1-objects-')));
    const git = (...args) => execFileSync('git', ['-C', root, ...args], { encoding: 'utf8', env: gitEnv(), input: '' }).trim();
    try {
      git('init', '-q');
      git('hash-object', '-w', '--stdin');
      const objects = join(root, '.git', 'objects');
      const empty = join(root, 'empty');
      mkdirSync(empty);
      const override = { explicit: objects, missing: join(root, 'missing'), relative: '.git/objects', empty, repository: undefined }[variant];
      const previous = process.env.F1_GIT_OBJECTS;
      if (variant === 'repository') delete process.env.F1_GIT_OBJECTS;
      let result;
      try { result = gitObjectsSource({ override, scriptDir: root }); }
      finally { if (previous !== undefined) process.env.F1_GIT_OBJECTS = previous; }
      if (['explicit', 'repository'].includes(variant)) expect(result).toEqual({ path: objects, reason: null });
      else { expect(result.path).toBeNull(); expect(result.reason).toBeTruthy(); }
    } finally { rmSync(root, { recursive: true, force: true }); }
  });
}

test('F1 bubblewrap alone keeps the run not-ready', async () => {
  // N3.2: a missing bwrap must by itself make ready===false. Every other precondition is a valid
  // object here, so a `typeof ... === 'boolean'` assertion can no longer pass vacuously.
  const objects = mkdtempSync(join(tmpdir(), 'f1-objects-'));
  const fanout = join(objects, 'ab');
  mkdirSync(fanout);
  writeFileSync(join(fanout, 'c'.repeat(38)), 'object');
  const checks = [];
  await f1IsolationChecks((...entry) => checks.push(entry), '/unused', {
    platform: 'linux',
    bwrapPath: '/usr/bin/bwrap',
    exists: (path) => path !== '/usr/bin/bwrap',
    gitObjects: { path: objects, reason: null },
    pythonExitWatch: true,
  });
  const check = checks.find(([name]) => name === 'f1-isolation-requirements-available');
  expect(check).toBeDefined();
  expect(check[2].required).toEqual({ bwrap: false, helperSource: true, wrapperSource: true, gitObjects: true, pythonExitWatch: true });
  expect(check[2].gitObjects.reason).toBeNull();
  expect(check[1]).toBe(false);
  rmSync(objects, { recursive: true, force: true });
});

test('F1 refuses a git-objects source that is not a populated repository', async () => {
  const root = mkdtempSync(join(tmpdir(), 'f1-empty-'));
  const checks = [];
  await f1IsolationChecks((...entry) => checks.push(entry), '/unused', {
    platform: 'linux',
    bwrapPath: '/usr/bin/bwrap',
    exists: () => true,
    gitObjects: { path: null, reason: 'no object entry under ' + root },
    pythonExitWatch: true,
  });
  const check = checks.find(([name]) => name === 'f1-isolation-requirements-available');
  expect(check[2].required.gitObjects).toBe(false);
  expect(check[2].gitObjects.reason).toBeTruthy();
  expect(check[1]).toBe(false);
  rmSync(root, { recursive: true, force: true });
});
