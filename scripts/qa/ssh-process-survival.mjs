import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { mkdtemp, mkdir, readFile, writeFile, chmod, copyFile, rm, realpath } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import net from 'node:net';
import readline from 'node:readline';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const scenario = process.argv[process.argv.indexOf('--scenario') + 1];
assert(['transport-loss', 'daemon-restart', 'reconnect-safety'].includes(scenario), '--scenario transport-loss|daemon-restart|reconnect-safety required');
const evidence = join(repo, 'docs/evidence/ssh-process-survival', `${scenario}.log`);
const records = [];
function log(value) { const line = typeof value === 'string' ? value : JSON.stringify(value); records.push(line); console.log(line); }
async function bounded(promise, label, ms = 30000) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${label}: deadline ${ms}ms`)), ms); })]); }
  finally { clearTimeout(timer); }
}
const children = [];
const sockets = new Set();
let root, proxy, helper, daemon, descriptor, remotePid;
let rejectTransports = false;
const owner = randomUUID();
async function hash(path) { try { return createHash('sha256').update(await readFile(path)).digest('hex'); } catch (e) { if (e.code === 'ENOENT') return null; throw e; } }
const trustPaths = [join(homedir(), '.ssh/known_hosts'), join(homedir(), '.ssh/config')];
const trustBefore = await Promise.all(trustPaths.map(hash));
function child(command, args, options = {}) {
  const p = spawn(command, args, { cwd: repo, stdio: ['pipe', 'pipe', 'pipe'], ...options });
  p.done = new Promise((resolve, reject) => { p.once('error', reject); p.once('close', (code, signal) => resolve({ code, signal })); });
  p.text = ''; p.err = '';
  p.stdout.on('data', b => { p.text += b; }); p.stderr.on('data', b => { p.err += b; });
  children.push(p); return p;
}
async function run(command, args, options, ms = 120000) {
  const p = child(command, args, options); const status = await bounded(p.done, command, ms);
  log({ command, args, ...status, stdout: p.text, stderr: p.err });
  assert.equal(status.code, 0, `${command} failed: ${p.err}`); return p.text;
}
async function stop(p) { if (!p || p.exitCode !== null || p.signalCode !== null) return; p.kill('SIGTERM'); await bounded(p.done, `reap child ${p.pid}`); log({ event: 'child-reaped', pid: p.pid }); }
function waitLine(p, prefix) {
  const lines = readline.createInterface({ input: p.stdout });
  return bounded(new Promise((resolve, reject) => {
    lines.on('line', line => { if (line.startsWith(prefix)) { lines.close(); resolve(line.slice(prefix.length)); } });
    p.done.then(status => { lines.close(); reject(new Error(`exited before ${prefix}: ${JSON.stringify(status)} ${p.err} ${p.text}`)); });
  }), prefix);
}
async function connect() {
  const s = net.createConnection(join(root, 'runtime/daemon.sock')); sockets.add(s); s.on('close', () => sockets.delete(s));
  const lines = readline.createInterface({ input: s })[Symbol.asyncIterator]();
  await bounded(once(s, 'connect'), 'daemon socket connect');
  return { s, async next() { const item = await bounded(lines.next(), 'daemon message'); assert(!item.done, 'daemon stream closed'); return JSON.parse(item.value); }, send(v) { s.write(JSON.stringify(v) + '\n'); } };
}
async function rpc(request) { const c = await connect(); try { c.send(request); return await c.next(); } finally { c.s.destroy(); } }
async function attach(id) {
  const c = await connect(); c.send({ type: 'attach', sessionId: id, afterSequence: null });
  const initial = await c.next(); assert.equal(initial.type, 'attachOk', JSON.stringify(initial));
  c.output = Buffer.from(initial.history, 'base64').toString();
  c.until = async predicate => { for (;;) { const m = await c.next(); log({ event: 'daemon-stream', ...m }); if (m.type === 'output') c.output += Buffer.from(m.data, 'base64'); if (predicate(m)) return m; } };
  return c;
}
let executable;
function daemonChild(mode) {
  const env = { ...process.env, HOME: join(root, 'home'), PATH: `${root}/bin:${process.env.PATH}`, FERRYX_RUNTIME_DIR: join(root, 'runtime'), FERRYX_SESSION_DIR: root, FERRYX_DATA_DIR: root, FERRYX_SURVIVAL_ROOT: root, FERRYX_SURVIVAL_OWNER: owner, FERRYX_SURVIVAL_MODE: mode };
  return child(executable, [], { env });
}
async function startDaemon() { daemon = daemonChild('serve'); await waitLine(daemon, 'QA_DAEMON_READY'); log({ event: 'daemon-ready', pid: daemon.pid }); }
async function details() { const v = await rpc({ type: 'remoteSessionDetails', sessionId: descriptor.backendSessionId }); assert(v.details, JSON.stringify(v)); return v.details; }
async function tick(stream, label, n) {
  const d = await details();
  const received = stream.until(() => stream.output.includes(`ACK:${owner}:${n}:${label}`));
  const reply = await rpc({ type: 'remoteWrite', sessionId: descriptor.backendSessionId, generation: d.generation, data: Buffer.from(label + '\n').toString('base64') });
  assert.equal(reply.type, 'writeOk', JSON.stringify(reply)); await received;
}
async function fixtureProcesses(previousPids = []) {
  const p = child('/bin/ps', ['-axo', 'pid=,ppid=,command=']);
  const status = await bounded(p.done, 'exact-fixture process snapshot');
  assert.equal(status.code, 0, p.err);
  const rows = p.text.trim().split('\n').filter(Boolean).map(line => {
    const match = line.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/);
    assert(match, `unparseable ps row: ${line}`);
    return { pid: Number(match[1]), ppid: Number(match[2]), command: match[3] };
  });
  const owned = new Set([...previousPids, ...children.filter(c => c !== p).map(c => c.pid)]);
  for (const row of rows) if (row.command.includes(root) || row.command.includes(owner)) owned.add(row.pid);
  // Finite graph traversal over one snapshot, not process polling.
  for (let changed = true; changed;) {
    changed = false;
    for (const row of rows) if (owned.has(row.ppid) && !owned.has(row.pid)) { owned.add(row.pid); changed = true; }
  }
  return rows.filter(row => owned.has(row.pid));
}
let failure;
try {
  assert(['darwin', 'linux'].includes(process.platform), 'UNAVAILABLE: Windows requires native SSH/ConPTY fixture; no POSIX substitute');
  // Dedicated unoptimized executable: ordinary cargo test never discovers a fixture-dependent test.
  const compiled = await run('cargo', ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--example', 'ssh_process_survival_qa', '--message-format=json'], { env: { ...process.env, CARGO_PROFILE_DEV_OPT_LEVEL: '0' } }, 240000);
  executable = compiled.split('\n').filter(Boolean).map(line => JSON.parse(line)).find(v => v.reason === 'compiler-artifact' && v.target.name === 'ssh_process_survival_qa' && v.executable)?.executable;
  assert(executable, 'QA executable missing');
  log({ event: 'qa-example-built', executable, optimized: false, rustTestHarness: false });
  root = await realpath(await mkdtemp('/tmp/fx-survival-')); await chmod(root, 0o700);
  for (const dir of ['home', 'runtime', 'bin', 'state', 'project']) await mkdir(join(root, dir), { mode: 0o700 });
  await writeFile(join(root, 'owner'), owner, { mode: 0o600 });
  log({ event: 'fixture', root, owner, scenario, platform: process.platform, trustBefore });
  await writeFile(join(root, 'bin/ssh'), '#!/bin/sh\nexec /usr/bin/ssh -o HostKeyAlias=127.0.0.1 -o UpdateHostKeys=no "$@"\n', { mode: 0o700 });
  // The real OpenSSH client uses existing trust/auth. Only this private TCP relay is cut.
  proxy = net.createServer(front => {
    if (rejectTransports) { front.destroy(); return; }
    const back = net.createConnection({ host: '127.0.0.1', port: 22 });
    for (const s of [front, back]) { sockets.add(s); s.on('close', () => sockets.delete(s)); }
    front.on('error', e => log({ event: 'relay-error', message: e.message })); back.on('error', e => log({ event: 'relay-error', message: e.message }));
    front.once('close', () => back.destroy()); back.once('close', () => front.destroy()); front.pipe(back); back.pipe(front);
    front.qaTransport = true; back.qaTransport = true;
  });
  proxy.listen(0, '127.0.0.1'); await once(proxy, 'listening');
  const host = { id: `qa-${owner}`, label: 'owned loopback QA', hostname: '127.0.0.1', port: proxy.address().port, source: 'manual', authMethod: 'agent' };
  const helperPath = join(root, 'bin/ferryx-remote-helper'); await copyFile(process.env.FERRYX_QA_HELPER_BINARY ?? join(repo, 'remote-helper/target/debug/ferryx-remote-helper'), helperPath); await chmod(helperPath, 0o700);
  helper = child(helperPath, ['daemon', '--root', join(root, 'state'), '--host-id', host.id]); await waitLine(helper, '{"event":"ready"');
  log({ event: 'owned-helper-ready', pid: helper.pid });
  const projectPath = join(root, 'project'); const hostBytes = Buffer.from(host.id); const len = Buffer.alloc(8); len.writeBigUInt64LE(BigInt(hostBytes.length));
  const projectId = 'ssh:' + createHash('sha256').update(len).update(hostBytes).update(projectPath).digest('hex');
  const config = { host, environment: { platform: 'posix', executor: 'sh', version: 'qa', home: join(root, 'home'), temp: root, git: true }, helper: { executable: helperPath, root: join(root, 'state') }, projectId, projectPath, worktree: null, agentIdentity: null };
  await writeFile(join(root, 'remote-config.json'), JSON.stringify(config));
  await writeFile(join(root, 'ssh_hosts.json'), JSON.stringify({ hosts: [host], tombstones: [] }));
  await writeFile(join(root, 'remote_projects.json'), JSON.stringify({ [projectId]: { workspaceId: projectId, hostId: host.id, repoRoot: projectPath, gitRoot: null, platform: 'posix' } }));
  await writeFile(join(root, 'spawn.json'), JSON.stringify({ projectId, program: '/bin/sh', args: ['-c', `stty -echo; c=0; printf 'INIT:${owner}\\n'; while IFS= read -r line; do c=$((c+1)); printf 'ACK:${owner}:%s:%s\\n' "$c" "$line"; done`] }));
  const seed = daemonChild('seed'); const seeded = waitLine(seed, 'QA_SEEDED '); descriptor = JSON.parse(await seeded); const seedExit = await bounded(seed.done, 'seed runtime exit'); assert.equal(seedExit.code, 0, seed.err); log(seed.text);
  await writeFile(join(root, 'pane.json'), JSON.stringify({ paneId: 'qa-stable-pane', backendSessionId: descriptor.backendSessionId }));
  const paneHash = await hash(join(root, 'pane.json'));
  await startDaemon(); let stream = await attach(descriptor.backendSessionId); await stream.until(m => m.type === 'remoteStatus' && m.state === 'connected');
  const before = await details(); remotePid = before.pid; log({ event: 'initial-identity', descriptor, remotePid, paneHash }); await tick(stream, 'before', 1);
  if (scenario === 'daemon-restart') {
    const closed = once(stream.s, 'close'); const exited = daemon.done;
    const control = await connect(); control.send({ type: 'shutdown' });
    await bounded(exited, 'only QA daemon shutdown'); await bounded(closed, 'old attachment closed'); control.s.destroy();
    await startDaemon(); stream = await attach(descriptor.backendSessionId); await stream.until(m => m.type === 'remoteStatus' && m.state === 'connected');
  } else {
    rejectTransports = scenario === 'reconnect-safety';
    const reconnecting = stream.until(m => m.type === 'remoteStatus' && m.state === 'reconnecting');
    const transports = [...sockets].filter(s => s.qaTransport); assert(transports.length > 0);
    const closed = transports.map(s => once(s, 'close')); transports.forEach(s => s.destroy()); await Promise.all(closed); await reconnecting;
    if (scenario === 'reconnect-safety') {
      const d = await details(); const generation = d.generation;
      const rejected = await rpc({ type: 'remoteWrite', sessionId: descriptor.backendSessionId, generation, data: Buffer.from('OUTAGE-MUST-NOT-REPLAY\n').toString('base64') }); assert.equal(rejected.type, 'remoteSessionError', JSON.stringify(rejected));
      const deduped = await Promise.all(Array.from({ length: 8 }, () => rpc({ type: 'retryRemoteSession', sessionId: descriptor.backendSessionId })));
      assert(deduped.every(r => r.type === 'retryRemoteSessionOk'));
      log({ event: 'explicit-retry-actions', phase: 'already-reconnecting', requests: 8, expectedEffect: 'deduplicated-no-new-generation' });
      assert.equal((await details()).generation, generation, 'concurrent retry deduplication');
      rejectTransports = false;
    }
    await stream.until(m => m.type === 'remoteStatus' && m.state === 'connected');
  }
  const after = await details(); assert.equal(after.pid, remotePid); assert.deepEqual(after.descriptor.target, descriptor.target); assert.equal(await hash(join(root, 'pane.json')), paneHash);
  await tick(stream, 'after', 2); assert(!stream.output.includes('OUTAGE-MUST-NOT-REPLAY'));
  if (scenario === 'reconnect-safety') {
    rejectTransports = true;
    const exhausted = stream.until(m => m.type === 'remoteStatus' && m.state === 'disconnected');
    const transports = [...sockets].filter(s => s.qaTransport);
    const closed = transports.map(s => once(s, 'close')); transports.forEach(s => s.destroy()); await Promise.all(closed);
    await exhausted;
    const exhaustedDetails = await details(); assert.equal(exhaustedDetails.attempts, 5); assert.equal(exhaustedDetails.state, 'disconnected');
    log({ event: 'bounded-exhaustion', attempts: exhaustedDetails.attempts, failure: exhaustedDetails.failure });
    rejectTransports = false;
    const recovered = stream.until(m => m.type === 'remoteStatus' && m.state === 'connected');
    const retries = await Promise.all(Array.from({ length: 8 }, () => rpc({ type: 'retryRemoteSession', sessionId: descriptor.backendSessionId })));
    assert(retries.every(r => r.type === 'retryRemoteSessionOk')); await recovered;
    log({ event: 'explicit-retry-actions', phase: 'after-automatic-exhaustion', requests: 8, expectedEffect: 'one-new-generation' });
    assert.equal((await details()).generation, exhaustedDetails.generation + 1);
    await tick(stream, 'after-exhaustion', 3);
    // Canonical PTY EOF terminates only the owned counter, not the helper owner.
    const expired = stream.until(m => m.type === 'remoteStatus' && m.state === 'expired');
    const d = await details();
    const eof = await rpc({ type: 'remoteWrite', sessionId: descriptor.backendSessionId, generation: d.generation, data: Buffer.from([4]).toString('base64') });
    assert.equal(eof.type, 'writeOk'); await expired;
    const missing = await details(); assert.equal(missing.failure.kind, 'missing'); assert.deepEqual(missing.descriptor.target, descriptor.target);
    log({ event: 'missing-target-without-replacement', details: missing });
  }
  const closeReply = await rpc({ type: 'close', sessionId: descriptor.backendSessionId }); assert.equal(closeReply.type, 'closeOk', JSON.stringify(closeReply));
  log({ event: 'same-process-proof', remotePid, nonce: owner, target: descriptor.target, counter: scenario === 'reconnect-safety' ? [1, 2, 3] : [1, 2], paneHash, noRetryClick: scenario !== 'reconnect-safety', explicitRetryRequests: scenario === 'reconnect-safety' ? 16 : 0, automaticRecovery: scenario !== 'reconnect-safety' });
} catch (e) { failure = e; log({ status: 'INCOMPLETE', error: e.stack }); }
finally {
  try {
    // Snapshot exact-root commands plus transitive children before shutting down owners.
    // No process is signalled based on this audit; only owned Child handles are stopped.
    const beforeCleanup = root ? await fixtureProcesses() : [];
    log({ event: 'owned-processes-before-cleanup', processes: beforeCleanup });
    // Close exact-owned sockets before terminating owners, and await every child handle.
    const closing = [...sockets].map(s => { const done = once(s, 'close'); s.destroy(); return done; }); await bounded(Promise.all(closing), 'socket cleanup');
    await stop(daemon); await stop(helper);
    for (const p of children.toReversed()) await stop(p);
    if (proxy) await new Promise((resolve, reject) => proxy.close(e => e ? reject(e) : resolve()));
    if (remotePid) { try { process.kill(remotePid, 0); throw new Error(`QA PTY ${remotePid} still alive; retaining fixture`); } catch (e) { if (e.code !== 'ESRCH') throw e; } }
    const remainingProcesses = root ? await fixtureProcesses(beforeCleanup.map(p => p.pid)) : [];
    log({ event: 'exact-fixture-process-absence-audit', root: root ?? null, includesProductionSshDescendants: true, remainingProcesses });
    assert.deepEqual(remainingProcesses, [], 'exact-fixture processes remain; retaining fixture');
    assert.deepEqual(await Promise.all(trustPaths.map(hash)), trustBefore, 'host trust changed');
    if (root) { assert.equal(await readFile(join(root, 'owner'), 'utf8'), owner); await rm(root, { recursive: true }); }
    log({ event: 'cleanup-receipt', root: root ?? null, remainingOwnedChildren: children.filter(p => p.exitCode === null && p.signalCode === null).map(p => p.pid), remainingSockets: sockets.size, fixtureRemoved: Boolean(root), trustUnchanged: true });
  } catch (e) { failure ??= e; log({ event: 'cleanup-failure', error: e.stack, retainedFixture: root }); }
  log({ status: failure ? 'INCOMPLETE' : 'PASS', scenario, nativeDesktopCoverage: false, linuxRemoteCoverage: false, windowsRemoteCoverage: false });
  await writeFile(evidence, records.join('\n') + '\n');
}
if (failure) process.exitCode = 1;
