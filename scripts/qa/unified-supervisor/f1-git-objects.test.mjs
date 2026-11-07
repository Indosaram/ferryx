import { expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs';
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

test('F1 bubblewrap remains a required Linux prerequisite', async () => {
  const previous = process.env.F1_GIT_OBJECTS;
  process.env.F1_GIT_OBJECTS = '/missing-f1-objects';
  const checks = [];
  try { await f1IsolationChecks((...entry) => checks.push(entry), '/unused'); }
  finally {
    if (previous === undefined) delete process.env.F1_GIT_OBJECTS;
    else process.env.F1_GIT_OBJECTS = previous;
  }
  const check = checks.find(([name]) => name === 'f1-isolation-requirements-available');
  expect(check).toBeDefined();
  expect(check[1]).toBe(false);
  expect(typeof check[2].required.bwrap).toBe('boolean');
  expect(check[2].required.gitObjects).toBe(false);
  expect(check[2].gitObjects.reason).toBeTruthy();
});
