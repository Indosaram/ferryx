import test from 'node:test';
import assert from 'node:assert/strict';
import { macHubIsolationVerdict } from './lib/jobs.mjs';

const account = { username: 'fixture', uid: 502, homedir: '/Users/fixture' };
const home = { uid: account.uid, isDirectory: () => true };
const clean = (path) => path === account.homedir ? home : undefined;
const inspect = (options = {}) => macHubIsolationVerdict({
  account, runnerHome: account.homedir, dedicatedAccount: account.username, inspect: clean, ...options,
});

test('a fresh explicitly designated macOS login can run the desktop scenarios', () => {
  assert.equal(inspect().blocked, false);
});

test('a fresh ordinary login is not implicitly a QA login', () => {
  for (const dedicatedAccount of [undefined, '', 'another-account']) {
    assert.equal(inspect({ dedicatedAccount }).reason, 'dedicated-login-required');
  }
});

test('changing the runner HOME does not change the launchd login', () => {
  assert.equal(inspect({ runnerHome: '/private/tmp/a-run/home' }).reason, 'home-mismatch');
});

test('existing Ferryx state blocks even an explicitly named account', () => {
  for (const entry of [{ isDirectory: () => true }, { isSymbolicLink: () => true }]) {
    assert.equal(inspect({ inspect: (path) => path === account.homedir ? home : entry }).reason, 'existing-ferryx-state');
  }
});

test('an unreadable home is not evidence that the real installation is absent', () => {
  const inaccessible = (path) => {
    if (path === account.homedir) return home;
    throw Object.assign(new Error('denied'), { code: 'EACCES' });
  };
  assert.equal(inspect({ inspect: inaccessible }).reason, 'account-state-unreadable');
  assert.equal(inspect({ inspect: () => undefined }).reason, 'unverified-home');
  assert.equal(inspect({ inspect: () => ({ uid: 999, isDirectory: () => true }) }).reason, 'unverified-home');
});

test('unknown account identity cannot authorize a macOS run', () => {
  for (const suspect of [null, { ...account, uid: 0 }, { ...account, homedir: '/' }, { ...account, homedir: 'relative' }]) {
    assert.equal(inspect({ account: suspect }).reason, 'unverified-account');
  }
});
