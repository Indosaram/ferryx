#!/usr/bin/env bun
// This process runs outside bwrap. Only recorded task identities cross its socket;
// no proc mount, process root link, namespace fd, or arbitrary host signal is exported.
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, readFileSync, readdirSync, readlinkSync, writeFileSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gitObjectsSource } from './f1-git-objects.mjs';
import { linuxProcess, sameProcess } from './f1-process-identity.mjs';

const SELF = fileURLToPath(import.meta.url);
const identityKey = (value) => `${value.pidNamespace}:${value.namespacePid}:${value.startStamp}`;
const validIdentity = (value) => value && Number.isSafeInteger(value.namespacePid) && value.namespacePid > 0
  && /^pid:\[\d+\]$/.test(value.pidNamespace) && /^\d+$/.test(value.startStamp);
const failure = (code, message) => Object.assign(new Error(message), { code });

function lines(stream, consume) {
  let buffer = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    buffer += chunk;
    if (buffer.length > 65_536) { stream.destroy(failure('FRAME_TOO_LARGE', 'helper frame exceeds 64KiB')); return; }
    for (let at = buffer.indexOf('\n'); at >= 0; at = buffer.indexOf('\n')) {
      const line = buffer.slice(0, at); buffer = buffer.slice(at + 1);
      if (line) consume(line);
    }
  });
}

// pidfd readiness is actual kernel exit evidence, including an unreaped zombie.
// The descriptor is opened before ARMED and before any caller-triggered signal.
const PIDFD = `import os,sys,select,signal,json
pid=int(sys.argv[1]); stamp=sys.argv[2]; timeout=float(sys.argv[3]); sig=sys.argv[4]
try:
 fd=os.pidfd_open(pid)
 stat=open('/proc/%d/stat'%pid).read().rsplit(') ',1)[1].split()
 if stat[19]!=stamp:
  print('GONE',flush=True); sys.exit(0)
 print('ARMED',flush=True)
 if sig!='none': signal.pidfd_send_signal(fd,getattr(signal,sig))
 print('EXIT' if select.select([fd],[],[],timeout)[0] else 'TIMEOUT',flush=True)
except ProcessLookupError:
 print('GONE',flush=True)
except FileNotFoundError:
 print('GONE',flush=True)
`;

function waitHost(identity, deadlineMs, armed, signal = 'none') {
  return new Promise((resolveWait, rejectWait) => {
    const child = spawn('python3', ['-u', '-c', PIDFD, String(identity.hostPid), identity.startStamp, String(deadlineMs / 1000), signal], { stdio: ['ignore', 'pipe', 'pipe'] });
    let result = null; let stderr = ''; let settled = false;
    const finish = (fn, value) => { if (settled) return; settled = true; clearTimeout(timer); fn(value); };
    const timer = setTimeout(() => { child.kill('SIGKILL'); finish(rejectWait, failure('EXIT_WATCH_TIMEOUT', 'pidfd helper failed to finish within its bound')); }, deadlineMs + 5_000);
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', (error) => finish(rejectWait, error));
    lines(child.stdout, (line) => { if (line === 'ARMED') armed(); else result = line; });
    child.once('close', (code) => {
      if (code !== 0 || !['EXIT', 'GONE', 'TIMEOUT'].includes(result)) finish(rejectWait, failure('EXIT_WATCH_FAILED', `pidfd watch failed: ${stderr || result || code}`));
      else finish(resolveWait, { exited: result !== 'TIMEOUT', via: 'pidfd' });
    });
  });
}

export async function supervise(root, args) {
  if (process.platform !== 'linux') throw failure('PLATFORM_UNSUPPORTED', 'F1 isolation requires Linux');
  root = resolve(root);
  const helperDir = join(root, 'host-helper');
  const socket = join(helperDir, 'helper.sock');
  if (Buffer.byteLength(socket) > 100) throw failure('SOCKET_PATH_TOO_LONG', socket);
  if (existsSync(helperDir)) throw failure('HELPER_STATE_EXISTS', `refusing existing helper state ${helperDir}`);
  mkdirSync(join(helperDir, 'map'), { recursive: true, mode: 0o700 });
  const ownNamespace = readlinkSync('/proc/self/ns/pid');
  const records = new Map(); const namespaces = new Set(); const groups = new Map();
  let child = null;

  function descendant(pid) {
    const seen = new Set();
    while (pid > 1 && !seen.has(pid)) {
      if (pid === child?.pid) return true;
      seen.add(pid);
      try { pid = Number(readFileSync(`/proc/${pid}/stat`, 'utf8').replace(/^.*\) /s, '').split(/\s+/)[1]); }
      catch (error) { if (['ENOENT', 'ESRCH'].includes(error.code)) return false; throw error; }
    }
    return false;
  }

  // Scan namespace links first: cmdline/stat of unrelated owner processes are not read.
  function candidates(namespace) {
    const values = []; let complete = true;
    for (const name of readdirSync('/proc').filter((entry) => /^\d+$/.test(entry))) {
      try {
        if (readlinkSync(`/proc/${name}/ns/pid`) !== namespace) continue;
        const value = linuxProcess(Number(name));
        if (value) values.push(value);
      } catch (error) {
        if (['ENOENT', 'ESRCH'].includes(error.code)) continue;
        if (['EACCES', 'EPERM'].includes(error.code)) {
          // Unrelated users may hide their namespace links. Only an unreadable task
          // descendant makes this task's group observation incomplete.
          if (descendant(Number(name))) complete = false;
          continue;
        }
        throw error;
      }
    }
    return { values, complete };
  }

  function stored(input) {
    if (!validIdentity(input)) throw failure('IDENTITY_INVALID', 'expected recorded namespace identity');
    const record = records.get(identityKey(input));
    if (!record) throw failure('IDENTITY_UNRECORDED', 'identity has not been mapped by this helper');
    return record;
  }
  function observed(record) {
    const value = linuxProcess(record.hostPid);
    if (!value || !sameProcess(record, value) || !value.live) return { verdict: 'gone', live: false, pid: null };
    return { verdict: 'match', live: true, pid: value.namespacePid, args: value.args, procState: value.procState };
  }
  function save(value) {
    const record = { namespacePid: value.namespacePid, pidNamespace: value.pidNamespace, startStamp: value.startStamp,
      hostPid: value.pid, hostPgrp: value.pgrp, pgrp: value.pgrp, localPid: value.namespacePid };
    records.set(identityKey(record), record);
    const inode = record.pidNamespace.match(/\d+/)[0];
    writeFileSync(join(helperDir, 'map', `map-${inode}-${record.namespacePid}-${record.startStamp}.json`), JSON.stringify(record), { mode: 0o600 });
    return record;
  }
  function groupMembers(record) {
    const key = identityKey(record);
    const members = groups.get(key) ?? new Map();
    const scan = { values: [], complete: true };
    // Once captured members exist, a matching PGID alone cannot claim newly reused groups.
    const anchors = [record, ...members.values()];
    const anchored = anchors.some((member) => observed(member).live === true);
    if (anchored) for (const name of readdirSync('/proc').filter((entry) => /^\d+$/.test(entry))) {
      try {
        const stat = readFileSync(`/proc/${name}/stat`, 'utf8').replace(/^.*\) /s, '').trim().split(/\s+/);
        if (Number(stat[2]) !== record.hostPgrp) continue;
        const value = linuxProcess(Number(name));
        if (value) scan.values.push(value);
      } catch (error) {
        if (['ENOENT', 'ESRCH'].includes(error.code)) continue;
        if (['EACCES', 'EPERM'].includes(error.code)) { scan.complete = false; continue; }
        throw error;
      }
    }
    if (anchored) for (const value of scan.values) {
      if (value.pgrp === record.hostPgrp && BigInt(value.startStamp) >= BigInt(record.startStamp)) {
        const member = save(value); members.set(identityKey(member), member);
      }
    }
    groups.set(key, members);
    return { members: [...members.values()], complete: scan.complete };
  }
  function groupAlive(record) {
    const scan = groupMembers(record);
    return scan.members.some((member) => observed(member).live === true) ? true : scan.complete ? false : null;
  }

  const server = createServer((connection) => {
    connection.on('error', (error) => { if (!['EPIPE', 'ECONNRESET'].includes(error.code)) process.stderr.write(`host helper socket: ${error.message}\n`); });
    let used = false;
    lines(connection, (line) => {
      if (used) return; used = true;
      const reply = (value) => connection.end(`${JSON.stringify(value)}\n`);
      void (async () => {
        const request = JSON.parse(line);
        if (request.op === 'ping') return { hostPid: process.pid };
        if (request.op === 'register') {
          const input = request.identity;
          if (!validIdentity(input) || input.pidNamespace === ownNamespace) throw failure('NAMESPACE_REFUSED', 'only an isolated task identity can register');
          const prior = records.get(identityKey(input));
          if (prior) return prior;
          const scan = candidates(input.pidNamespace);
          const matches = scan.values.filter((value) => sameProcess(input, value));
          if (!scan.complete || matches.length !== 1) throw failure('IDENTITY_UNRESOLVED', 'host mapping is incomplete or ambiguous');
          const value = matches[0];
          if (!descendant(value.pid)) throw failure('NAMESPACE_REFUSED', 'identity is not a descendant of this bwrap');
          if (namespaces.size && !namespaces.has(value.pidNamespace)) throw failure('NAMESPACE_REFUSED', 'nested PID namespaces are not allowed');
          namespaces.add(value.pidNamespace);
          return save(value);
        }
        const record = stored(request.identity);
        if (request.op === 'observe') return observed(record);
        if (request.op === 'groupAlive') return groupAlive(record);
        const deadlineMs = request.deadlineMs;
        if (!Number.isSafeInteger(deadlineMs) || deadlineMs < 1 || deadlineMs > 60_000) throw failure('DEADLINE_INVALID', 'deadline must be 1..60000 ms');
        if (request.op === 'wait') {
          if (observed(record).live !== true) return { exited: true, observation: observed(record), via: 'identity' };
          const result = await waitHost(record, deadlineMs, () => connection.write(`${JSON.stringify({ armed: true })}\n`));
          return { ...result, observation: observed(record) };
        }
        if (request.op === 'groupTerminate') {
          if (!['SIGTERM', 'SIGKILL'].includes(request.signal)) throw failure('SIGNAL_REFUSED', 'only TERM or KILL is supported');
          const scan = groupMembers(record);
          const members = await Promise.all(scan.members.map(async (member) => {
            if (observed(member).live !== true) return { identity: member, exited: true };
            return { identity: member, ...(await waitHost(member, deadlineMs, () => {}, request.signal)) };
          }));
          return { members, groupAlive: groupAlive(record), complete: scan.complete };
        }
        throw failure('OP_REFUSED', `unsupported helper operation ${request.op}`);
      })().then((result) => reply({ ok: true, result }), (error) => reply({ ok: false, error: { code: error.code ?? 'HELPER_ERROR', message: error.message } }));
    });
  });
  await new Promise((resolveListen, rejectListen) => { server.once('error', rejectListen); server.listen(socket, resolveListen); });
  chmodSync(socket, 0o600);
  const bun = process.env.F1_BUN ?? join(root, 'bun');
  const env = { ...process.env, F1_PID_ISOLATED: '1', F1_BUN: bun, F1_HOST_HELPER: socket,
    HOME: join(root, 'base-home'), TMPDIR: join(root, 'base-tmp'), FERRYX_DATA_DIR: join(root, 'base-data'), FERRYX_RUNTIME_DIR: join(root, 'base-runtime') };
  delete env.F1_HOST_PROC;
  const gitObjects = gitObjectsSource();
  if (!gitObjects.path) throw failure('GIT_OBJECTS_REFUSED', gitObjects.reason);
  const bwrapArgs = ['--unshare-pid', '--ro-bind', '/', '/', '--tmpfs', '/run', '--tmpfs', '/home',
    '--dir', '/home/indo/code/ferryx/.git', '--ro-bind', gitObjects.path, '/home/indo/code/ferryx/.git/objects',
    '--tmpfs', '/tmp', '--dir', root, '--bind', root, root, '--ro-bind', helperDir, helperDir,
    '--dev-bind', '/dev', '/dev', '--proc', '/proc', '--chdir', root, bun, ...args];
  child = spawn('/usr/bin/bwrap', bwrapArgs, { env, stdio: 'inherit' });
  const forwarders = ['SIGTERM', 'SIGINT', 'SIGHUP'].map((signal) => {
    const forward = () => { child.kill(signal); }; process.on(signal, forward); return [signal, forward];
  });
  try {
    return await new Promise((resolveExit, rejectExit) => {
      child.once('error', rejectExit);
      child.once('exit', (code, signal) => resolveExit(code ?? (signal === 'SIGTERM' ? 143 : 1)));
    });
  } finally {
    for (const [signal, forward] of forwarders) process.removeListener(signal, forward);
    server.close();
  }
}

async function call(socket) {
  let request = '';
  for await (const chunk of process.stdin) request += chunk;
  await new Promise((resolveCall, rejectCall) => {
    const connection = createConnection(socket);
    const timer = setTimeout(() => { connection.destroy(); rejectCall(failure('RPC_TIMEOUT', 'helper RPC deadline exceeded')); }, 65_000);
    connection.once('connect', () => connection.write(`${request.trim()}\n`));
    connection.once('error', rejectCall);
    connection.on('data', (chunk) => process.stdout.write(chunk));
    connection.once('close', () => { clearTimeout(timer); resolveCall(); });
  });
}

if (process.argv[1] && resolve(process.argv[1]) === SELF) {
  const args = process.argv.slice(2);
  try {
    if (args[0] === '--supervise') process.exitCode = await supervise(args[1], args.slice(3));
    else if (args[0] === '--call') await call(args[1]);
    else throw failure('USAGE', 'f1-host-helper --supervise ROOT -- args | --call SOCKET');
  } catch (error) { process.stderr.write(`f1-host-helper: ${error.code ?? 'ERROR'} ${error.message}\n`); process.exitCode = 1; }
}
