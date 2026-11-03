#!/usr/bin/env node
// F3 host helper: runs on a desktop host (macOS or Windows) under `ssh -T <host> node
// <checkout>/scripts/qa/supervisor-f3/host-helper.mjs --rpc`, next to the existing
// scripts/qa/fixtures/native-remote-agent.mjs that prepares the isolated profile and launches
// the candidate daemon and GUI. JSONL RPC: {id, method, args} in, {id, result|error} out.
//
// It does only what that agent does not: report the machine and checkout identity, speak the
// daemon protocol over the isolated profile's own endpoint, write the fixture pane wrappers,
// spawn the relay / CLI children this run owns, read files under the isolated profile, and list
// files that appeared in the profile's Downloads directory. Never a GUI action (MahoRD does
// those from the control host). Every path is refused outside the profile it was given; every
// child it kills is a ChildProcess handle it spawned itself.
import { execFileSync, spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { hostname, platform, release } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const CHECKOUT = resolve(HERE, '..', '..', '..');
const DAEMON_PROTOCOL_VERSION = 5;
const WIN = process.platform === 'win32';
const owned = new Map();
// Directories this helper process created a child under; only these may be removed.
const ownedDirs = new Set();
const fail = (code, message) => Object.assign(new Error(message), { code });

function inside(root, path) {
  const delta = relative(resolve(root), resolve(path));
  return delta === '' || (!delta.startsWith('..') && !isAbsolute(delta));
}
function requireProfile(profile, path) {
  if (!isAbsolute(profile ?? '') || !/[\\/]\.native-qa[\\/][a-z0-9-]+$/.test(profile)) throw fail('PROFILE_REFUSED', `not an isolated .native-qa profile: ${profile}`);
  if (path !== undefined && !inside(profile, path)) throw fail('PATH_REFUSED', `${path} is outside ${profile}`);
}
const git = (...args) => execFileSync('git', ['-C', CHECKOUT, ...args], { encoding: 'utf8' }).trim();

function machineId() {
  if (WIN) {
    const out = execFileSync('reg', ['query', 'HKLM\\SOFTWARE\\Microsoft\\Cryptography', '/v', 'MachineGuid'], { encoding: 'utf8' });
    return /MachineGuid\s+REG_SZ\s+(\S+)/.exec(out)?.[1] ?? null;
  }
  const out = execFileSync('ioreg', ['-rd1', '-c', 'IOPlatformExpertDevice'], { encoding: 'utf8' });
  return /"IOPlatformUUID" = "([^"]+)"/.exec(out)?.[1] ?? null;
}

function identity() {
  const status = git('status', '--porcelain', '--untracked-files=no');
  return {
    machine: { id: machineId(), hostname: hostname(), platform: platform(), release: release(), node: process.version },
    candidate: { checkout: CHECKOUT, headRevision: git('rev-parse', 'HEAD'), clean: status === '', status },
  };
}

/** The isolated daemon's endpoint: the unix socket, or Windows loopback port plus token. */
function endpoint(profile) {
  requireProfile(profile);
  const runtime = join(profile, 'runtime');
  if (!WIN) return { path: join(runtime, 'daemon.sock'), token: null };
  const port = Number(readFileSync(join(runtime, 'daemon.port'), 'utf8').trim());
  const token = readFileSync(join(runtime, 'daemon.token'), 'utf8').trim();
  if (!Number.isSafeInteger(port) || port <= 0) throw fail('DAEMON_ENDPOINT', 'daemon.port holds no port');
  return { host: '127.0.0.1', port, token };
}

/** Handshake plus `frames`, one reply per frame, bounded. */
function daemon({ profile, frames, deadlineMs = 30_000 }) {
  const target = endpoint(profile);
  const all = [{ type: 'handshake', version: DAEMON_PROTOCOL_VERSION, ...(target.token ? { token: target.token } : {}) }, ...frames];
  return new Promise((resolveExchange, reject) => {
    const replies = [];
    let buffer = '';
    let next = 0;
    const connection = createConnection(target.path ? { path: target.path } : { host: target.host, port: target.port });
    const timer = setTimeout(() => { connection.destroy(); reject(fail('DAEMON_DEADLINE', `no reply within ${deadlineMs} ms after ${replies.length}`)); }, deadlineMs);
    const finish = () => { clearTimeout(timer); connection.destroy(); resolveExchange(replies); };
    const send = () => { if (next >= all.length) finish(); else connection.write(`${JSON.stringify(all[next++])}\n`); };
    connection.setEncoding('utf8');
    connection.on('connect', send);
    connection.on('error', (error) => { clearTimeout(timer); reject(fail('DAEMON_IO', error.message)); });
    connection.on('data', (chunk) => {
      buffer += chunk;
      for (let at = buffer.indexOf('\n'); at >= 0; at = buffer.indexOf('\n')) {
        const line = buffer.slice(0, at);
        buffer = buffer.slice(at + 1);
        let json;
        try { json = JSON.parse(line); } catch { json = { unparsed: line }; }
        replies.push(json);
        if (json.type === 'protocolMismatch' || json.type === 'error') { finish(); return; }
        send();
      }
    });
  });
}

/** One wrapper per fixture mode, so a spawn's `shell` names exactly one fixture agent. */
function writeWrappers({ profile }) {
  requireProfile(profile);
  const agent = join(HERE, 'fixture-agent.mjs');
  const logs = join(profile, 'evidence', 'f3-agents');
  const bin = join(profile, 'bin');
  mkdirSync(bin, { recursive: true });
  mkdirSync(logs, { recursive: true });
  const wrappers = {};
  // `focus` and `away` are per desktop (F3-D6): two instrumented panes whose titles name the
  // host, so each desktop's rows are unique; both run the fixture's `focus` mode.
  const host = WIN ? 'win' : 'mac';
  for (const [key, mode, title] of [['decision', 'decision', 'f3decision'], ['held', 'held', 'f3push'], ['upstream', 'upstream', 'f3upstream'],
    ['focus', 'focus', `f3focus${host}`], ['away', 'focus', `f3away${host}`]]) {
    const file = join(bin, `f3-${key}${WIN ? '.cmd' : ''}`);
    const body = WIN
      ? `@echo off\r\n"${process.execPath}" "${agent}" "${logs}" ${mode} ${title}\r\nexit /b %ERRORLEVEL%\r\n`
      : `#!/bin/sh\nexec '${process.execPath}' '${agent}' '${logs}' ${mode} ${title}\n`;
    writeFileSync(file, body, { mode: 0o755 });
    if (!WIN) chmodSync(file, 0o755);
    wrappers[key] = { shell: file, title };
  }
  // The panel titles a session by the last segment of its cwd (fleet.rs), so each of these panes
  // runs in its own directory inside the profile (and so inside the registered worktree, removed
  // with the profile): one row on this computer carries its title.
  for (const key of ['focus', 'away']) {
    const cwd = join(profile, 'panes', wrappers[key].title);
    mkdirSync(cwd, { recursive: true });
    wrappers[key].cwd = resolve(cwd);
  }
  return { wrappers, logs };
}

/** Spawns a run-owned child under the profile; resolves once `ready` appears on its output. */
function spawnOwned({ profile, key, executable, args = [], env = {}, ready, deadlineMs = 60_000 }) {
  requireProfile(profile);
  if (owned.has(key)) throw fail('SPAWN_REFUSED', `${key} already spawned`);
  const logFile = join(profile, 'evidence', `f3-${key}.log`);
  if (!existsSync(profile)) ownedDirs.add(resolve(profile));
  mkdirSync(dirname(logFile), { recursive: true });
  const child = spawn(executable, args, { cwd: profile, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  const entry = { key, child, executable, args, pid: child.pid, startedAt: new Date().toISOString(), exit: null, logFile, output: '' };
  owned.set(key, entry);
  const exited = new Promise((resolveExit) => child.once('exit', (code, signal) => { entry.exit = { code, signal }; resolveExit(entry.exit); }));
  entry.exited = exited;
  return new Promise((resolveReady, reject) => {
    const timer = ready ? setTimeout(() => reject(fail('SPAWN_NOT_READY', `${key} printed no ${ready} within ${deadlineMs} ms`)), deadlineMs) : null;
    const take = (chunk) => {
      entry.output += chunk;
      writeFileSync(logFile, entry.output);
      if (ready && entry.output.includes(ready)) { clearTimeout(timer); resolveReady(describe(entry)); }
    };
    child.stdout.on('data', take);
    child.stderr.on('data', take);
    child.once('error', (error) => { clearTimeout(timer); reject(fail('SPAWN_FAILED', error.message)); });
    exited.then((exit) => { if (ready) { clearTimeout(timer); reject(fail('SPAWN_EXITED', `${key} exited ${JSON.stringify(exit)} before ready`)); } });
    if (!ready) resolveReady(describe(entry));
  });
}
const describe = (entry) => ({ key: entry.key, pid: entry.pid, executable: entry.executable, args: entry.args, startedAt: entry.startedAt, exit: entry.exit });

async function awaitExit({ key, deadlineMs = 120_000 }) {
  const entry = owned.get(key);
  if (!entry) throw fail('NOT_OWNED', key);
  const exit = await Promise.race([entry.exited, new Promise((_, reject) => setTimeout(() => reject(fail('EXIT_DEADLINE', key)), deadlineMs))]);
  return { ...describe(entry), exit, output: entry.output.slice(-8000) };
}

async function cleanup() {
  const receipts = [];
  for (const entry of owned.values()) {
    if (entry.exit) { receipts.push({ resource: entry.key, key: entry.key, pid: entry.pid, action: 'already-exited', exit: entry.exit }); continue; }
    entry.child.kill();
    const exit = await Promise.race([entry.exited, new Promise((r) => setTimeout(() => r(null), 10_000))]);
    receipts.push({ resource: entry.key, key: entry.key, pid: entry.pid, action: exit ? 'killed' : 'reported-not-killed', exit });
  }
  return receipts;
}

function readB64({ profile, path }) {
  requireProfile(profile, path);
  const bytes = readFileSync(path);
  return { path, bytes: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex'), base64: bytes.toString('base64') };
}

function listFiles({ profile, dir }) {
  requireProfile(profile, dir);
  if (!existsSync(dir)) return [];
  return readdirSync(dir).map((name) => {
    const info = statSync(join(dir, name));
    return { name, path: join(dir, name), bytes: info.size, mtimeMs: info.mtimeMs, file: info.isFile() };
  });
}

/** Removes a directory this helper itself created (the run-owned relay root), after cleanup. */
function removeOwnedDir({ profile }) {
  const target = resolve(profile);
  if (!ownedDirs.has(target)) {
    return existsSync(target)
      ? { resource: 'dir', path: target, action: 'reported-not-killed', reason: 'exists but was not created by this helper' }
      : { resource: 'dir', path: target, action: 'absent' };
  }
  if ([...owned.values()].some((entry) => !entry.exit)) throw fail('DIR_BUSY', `${target} still has a live child`);
  rmSync(target, { recursive: true, force: false });
  ownedDirs.delete(target);
  return { resource: 'dir', path: target, action: 'removed' };
}

/**
 * A free loopback-and-wildcard TCP port for this run's gateway, found by binding port 0 and
 * releasing it; never the production gateway port 43821 and never a listener someone else owns.
 * The daemon binds it right after, and the lane proves the bind from the daemon's own status.
 */
function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once('error', (error) => reject(fail('PORT_UNAVAILABLE', error.message)));
    server.listen({ port: 0, host: '0.0.0.0', exclusive: true }, () => {
      const { port } = server.address();
      server.close(() => (port === 43821 ? freePort().then(resolvePort, reject) : resolvePort({ port })));
    });
  });
}

const METHODS = {
  identity, daemon, 'write-wrappers': writeWrappers, 'spawn-owned': spawnOwned, 'await-exit': awaitExit,
  cleanup, 'read-b64': readB64, 'list-files': listFiles, 'remove-owned-dir': removeOwnedDir, 'free-port': freePort,
};

if (process.argv.includes('--rpc')) {
  const out = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);
  createInterface({ input: process.stdin })
    .on('line', (line) => {
      let request;
      try { request = JSON.parse(line); } catch (error) { out({ error: { code: 'BAD_JSON', message: error.message } }); return; }
      const method = METHODS[request.method];
      if (!method) { out({ id: request.id, error: { code: 'UNKNOWN_METHOD', message: request.method } }); return; }
      Promise.resolve().then(() => method(request.args ?? {})).then(
        (result) => out({ id: request.id, result }),
        (error) => out({ id: request.id, error: { code: error.code ?? 'HELPER_ERROR', message: error.message } }),
      );
    })
    .on('close', () => { void cleanup().finally(() => process.exit(0)); });
}
