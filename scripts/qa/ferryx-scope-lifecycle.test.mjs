import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { EventEmitter } from 'node:events';
import { pageCurl, identityHeader, releaseAll, listen, bounded } from './ferryx-scope-lifecycle.mjs';

test('page curl disables config before other arguments and bypasses proxies', () => {
  const argv = pageCurl('http://127.0.0.1:12345');
  expect(argv[1]).toBe('--disable');
  expect(argv.slice(argv.indexOf('--noproxy'), argv.indexOf('--noproxy') + 2)).toEqual(['--noproxy', '*']);
});
test('last-position fixture identity header is accepted', () => {
  expect(identityHeader('HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Ferryx-QA-Fixture: abc', 'abc')).toBe(true);
});
test('duplicate identity headers do not establish identity', () => {
  expect(identityHeader('HTTP/1.1 200 OK\r\nX-Ferryx-QA-Fixture: abc\r\nX-Ferryx-QA-Fixture: foreign\r\n', 'abc')).toBe(false);
});
test('root cleanup runs even if page close rejects', async () => {
  const called = [];
  await expect(releaseAll([async () => { called.push('page'); throw new Error('close failed'); }, async () => { called.push('root'); }])).rejects.toThrow();
  expect(called).toEqual(['page', 'root']);
});
test('all listener cleanups settle even when one throws synchronously', async () => {
  const called = [];
  await expect(releaseAll([() => { throw new Error('frame failed'); }, async () => { called.push('main'); }])).rejects.toThrow();
  expect(called).toEqual(['main']);
});
test('readiness subscription exists before listen triggers', async () => {
  const server = new EventEmitter();
  server.listen = () => {
    expect(server.listenerCount('listening')).toBe(1);
    server.emit('listening');
  };
  await listen(server);
});
test('unfinished child or close operation rejects at its deadline', async () => {
  // Timeouts themselves are the behavior under test; no sleep or polling.
  await expect(bounded(new Promise(() => {}), 10)).rejects.toThrow('QA_DEADLINE');
}, 100);
