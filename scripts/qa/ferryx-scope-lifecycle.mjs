import { once } from 'node:events';
import { spawn } from 'node:child_process';
import { Readable } from 'node:stream';

export function spawnChild(argv, options = {}) {
  const child = spawn(argv[0], argv.slice(1), { stdio: ['pipe', 'pipe', 'pipe'], env: options.env ?? process.env });
  const exited = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', resolve);
  });
  return { stdin: child.stdin, stdout: Readable.toWeb(child.stdout), stderr: Readable.toWeb(child.stderr), exited,
    get exitCode() { return child.exitCode; }, kill: signal => child.kill(signal) };
}

export function pageCurl(url) {
  return ['curl', '--disable', '--noproxy', '*', '--fail-with-body', '--silent', '--show-error', '--max-time', '5', '--dump-header', '-', url];
}
export function identityHeader(headers, id) {
  const values = headers.split('\r\n').filter(line => /^x-ferryx-qa-fixture:/i.test(line)).map(line => line.slice(line.indexOf(':') + 1).trim());
  return values.length === 1 && values[0] === id;
}
export async function releaseAll(actions) {
  const results = await Promise.allSettled(actions.map(action => Promise.resolve().then(action)));
  const errors = results.filter(result => result.status === 'rejected').map(result => result.reason);
  if (errors.length) throw new AggregateError(errors, 'QA_CLEANUP_FAILED');
}
export async function listen(server) {
  const controller = new AbortController();
  const ready = once(server, 'listening', { signal: controller.signal });
  try {
    server.listen(0, '127.0.0.1');
    await bounded(ready, 5000);
  } finally {
    controller.abort();
    await ready.catch(error => { if (error.name !== 'AbortError') throw error; });
  }
}
export async function bounded(work, milliseconds) {
  let timer;
  try {
    return await Promise.race([work, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('QA_DEADLINE')), milliseconds); })]);
  } finally { clearTimeout(timer); }
}
export async function collectChild(child) {
  try {
    return await bounded(Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]), 8000);
  } catch (error) {
    if (child.exitCode === null) child.kill('SIGKILL');
    await bounded(child.exited, 2000);
    throw error;
  }
}
export async function closeServer(server) {
  if (!server.listening) return;
  const closed = new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  server.closeAllConnections();
  await bounded(closed, 5000);
}
