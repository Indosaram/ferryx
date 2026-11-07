import { expect, test } from 'bun:test';
import { mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gitEnv, runGit } from './git-env.mjs';

const KEYS = ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES', 'GIT_CEILING_DIRECTORIES', 'GIT_COMMON_DIR', 'GIT_NAMESPACE'];

function withAmbientGit(action) {
  const saved = Object.fromEntries(KEYS.map((key) => [key, process.env[key]]));
  const root = mkdtempSync(join(tmpdir(), 'qa-git-env-'));
  try {
    for (const key of KEYS) process.env[key] = join(root, `foreign-${key}`);
    return action(root);
  } finally {
    for (const key of KEYS) {
      if (saved[key] === undefined) delete process.env[key];
      else process.env[key] = saved[key];
    }
    rmSync(root, { recursive: true, force: true });
  }
}

test('ambient git repository variables cannot redirect a fixture init or commit', () => {
  withAmbientGit((root) => {
    const repo = join(root, 'fixture');
    const git = (...args) => {
      const result = runGit(['-c', 'user.name=qa', '-c', 'user.email=qa@example.invalid', ...args], { encoding: 'utf8' });
      expect(result.error).toBeUndefined();
      expect(result.status).toBe(0);
      return result.stdout.trim();
    };
    git('init', '-q', repo);
    expect(statSync(join(repo, '.git')).isDirectory()).toBe(true);
    expect(readFileSync(join(repo, '.git', 'HEAD'), 'utf8')).toMatch(/^ref: refs\/heads\//);
    writeFileSync(join(repo, 'fixture.txt'), 'owned fixture\n');
    git('-C', repo, 'add', 'fixture.txt');
    git('-C', repo, 'commit', '-qm', 'fixture-owned-commit');
    const head = readFileSync(join(repo, '.git', 'HEAD'), 'utf8').trim().slice(5);
    const commit = readFileSync(join(repo, '.git', head), 'utf8').trim();
    expect(git('-C', repo, 'log', '-1', '--format=%H:%s')).toBe(`${commit}:fixture-owned-commit`);
    expect(git('-C', repo, 'show', 'HEAD:fixture.txt')).toBe('owned fixture');
  });
});

test('gitEnv scrubs a copy without mutating process.env or caller overrides', () => {
  withAmbientGit(() => {
    const before = { ...process.env };
    const extra = { GIT_DIR: '/foreign/override', QA_GIT_MARKER: 'kept' };
    const env = gitEnv(extra);
    for (const key of KEYS) expect(env[key]).toBeUndefined();
    for (const key of ['PATH', 'HOME', 'TMPDIR', 'LANG', 'SSH_AUTH_SOCK', 'SystemRoot', 'APPDATA', 'USERPROFILE', ...Object.keys(before).filter((key) => key.startsWith('LC_'))]) {
      expect(env[key]).toBe(before[key]);
    }
    expect(env.QA_GIT_MARKER).toBe('kept');
    expect(extra.GIT_DIR).toBe('/foreign/override');
    expect({ ...process.env }).toEqual(before);
  });
});
