// Fixture-only process identities. A namespace-local pid is never a host pid: inside the isolated
// namespace an identity is a recorded {namespacePid, pidNamespace, startStamp}, and every host
// fact (host pid, group members, exit evidence) comes from the outer host helper over F1_HOST_HELPER.
import { exitWatchSource } from './f1-exit-watch.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { readFileSync, readdirSync, readlinkSync } from 'node:fs';
import { createConnection } from 'node:net';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const HELPER = join(dirname(fileURLToPath(import.meta.url)), 'f1-host-helper.mjs');
const failure = (code, message) => Object.assign(new Error(message), { code });
const helperSocket = () => process.env.F1_HOST_HELPER || null;
const keyOf = (value) => `${value.pidNamespace}:${value.namespacePid}:${value.startStamp}`;
// Only the recorded namespace identity crosses the helper socket: no pid, path or proc handle.
const wire = (value) => ({ namespacePid: value.namespacePid, pidNamespace: value.pidNamespace, startStamp: value.startStamp });
const own = (value) => ({ ...value, hostPid: value.pid, localPid: value.pid });

export function linuxProcess(pid, procRoot = '/proc') {
  try {
    const root = `${procRoot}/${pid}`;
    const stat = readFileSync(`${root}/stat`, 'utf8').replace(/^.*\) /s, '').trim().split(/\s+/);
    const status = readFileSync(`${root}/status`, 'utf8');
    const pids = status.match(/^NSpid:\s+(.+)$/m)?.[1].trim().split(/\s+/).map(Number) ?? [pid];
    const args = readFileSync(`${root}/cmdline`, 'utf8').split('\0').filter(Boolean);
    return { pid, namespacePid: pids.at(-1), pidNamespace: readlinkSync(`${root}/ns/pid`), startStamp: stat[19],
      pgrp: Number(stat[2]), args, procState: stat[0], live: !['Z', 'X', 'x'].includes(stat[0]) };
  } catch (error) {
    if (['ENOENT', 'ESRCH'].includes(error?.code)) return null;
    throw error;
  }
}

export function sameProcess(a, b) {
  return Boolean(a && b && a.startStamp && a.startStamp === b.startStamp && a.pidNamespace === b.pidNamespace && a.namespacePid === b.namespacePid);
}

export function mapHostProcess(local, candidates) {
  const matches = candidates.filter((candidate) => sameProcess(local, candidate));
  return matches.length === 1 ? matches[0] : null;
}

function portableProcess(pid) {
  const ps = spawnSync('ps', ['-o', 'lstart=', '-o', 'args=', '-p', String(pid)], { encoding: 'utf8', timeout: 5_000 });
  if (ps.status !== 0 || !ps.stdout.trim()) return null;
  const match = ps.stdout.trim().match(/^(\S+\s+\S+\s+\d+\s+\S+\s+\d+)\s+(.+)$/);
  return match ? { pid, namespacePid: pid, pidNamespace: null, startStamp: match[1], args: [match[2]], live: true } : null;
}

const ownNamespace = () => (process.platform === 'linux' ? readlinkSync('/proc/self/ns/pid') : null);

// ---- helper IPC: newline-JSON over a Unix socket; one request per connection ----

function readFrames(stream, consume) {
  let buffer = '';
  stream.setEncoding('utf8');
  stream.on('data', (chunk) => {
    buffer += chunk;
    for (let at = buffer.indexOf('\n'); at >= 0; at = buffer.indexOf('\n')) {
      const line = buffer.slice(0, at); buffer = buffer.slice(at + 1);
      if (line) consume(line);
    }
  });
}

const unwrap = (reply) => {
  if (!reply?.ok) throw failure(reply?.error?.code ?? 'HELPER_ERROR', reply?.error?.message ?? 'host helper refused the request');
  return reply.result;
};

// Synchronous RPC: the helper CLI waits on the socket; its own network wait is event driven and bounded.
export function helperCallSync(request, socket = helperSocket()) {
  if (!socket) throw failure('HELPER_ABSENT', 'F1_HOST_HELPER is not set');
  const run = spawnSync(process.execPath, [HELPER, '--call', socket], { input: JSON.stringify(request), encoding: 'utf8', timeout: 70_000, maxBuffer: 1 << 20 });
  if (run.error) throw run.error;
  const frames = run.stdout.split('\n').filter(Boolean).map((line) => JSON.parse(line)).filter((frame) => !frame.armed);
  if (run.status !== 0 && frames.length === 0) throw failure('HELPER_CALL_FAILED', `helper call exited ${run.status}: ${run.stderr.trim().slice(-300)}`);
  return unwrap(frames.at(-1));
}

// Async RPC. `onArmed` runs when the helper reports its exit subscription is armed, before the final reply.
export function helperCall(request, { onArmed = null, socket = helperSocket(), timeoutMs = 15_000 } = {}) {
  if (!socket) return Promise.reject(failure('HELPER_ABSENT', 'F1_HOST_HELPER is not set'));
  return new Promise((resolveCall, rejectCall) => {
    const connection = createConnection(socket);
    let settled = false;
    let chain = Promise.resolve();
    const finish = (fn, value) => { if (settled) return; settled = true; clearTimeout(timer); connection.destroy(); fn(value); };
    const timer = setTimeout(() => finish(rejectCall, failure('RPC_TIMEOUT', `helper ${request.op} exceeded ${timeoutMs}ms`)), timeoutMs);
    connection.once('error', (error) => finish(rejectCall, error));
    connection.once('connect', () => connection.write(`${JSON.stringify(request)}\n`));
    readFrames(connection, (line) => {
      let frame;
      try { frame = JSON.parse(line); } catch (error) { finish(rejectCall, error); return; }
      if (frame.armed) { chain = chain.then(() => onArmed?.()).catch((error) => finish(rejectCall, error)); return; }
      chain = chain.then(() => { try { finish(resolveCall, unwrap(frame)); } catch (error) { finish(rejectCall, error); } });
    });
    connection.once('close', () => finish(rejectCall, failure('RPC_CLOSED', `helper closed before replying to ${request.op}`)));
  });
}

// ---- capture / map ----

// What a pane records about itself: local facts only. No host pid, no helper involvement.
export function captureLocalIdentity(pid = process.pid) {
  const local = process.platform === 'linux' ? linuxProcess(pid) : portableProcess(pid);
  return local ? { ...local, localPid: pid } : null;
}

// Host-side mapping of a recorded local identity. The host helper authors the map file; a pane's
// own pid is never promoted to a host pid. Without a helper (not isolated) local is host, but only
// inside the PID namespace the identity was recorded in.
export function mapRecordedIdentity(local) {
  if (!local) return null;
  if (helperSocket()) {
    try { return { ...local, ...helperCallSync({ op: 'register', identity: wire(local) }) }; } catch (error) {
      process.stderr.write(`f1 identity register refused: ${error.code ?? ''} ${error.message}\n`);
      return null;
    }
  }
  if (process.env.F1_PID_ISOLATED === '1') throw failure('HELPER_ABSENT', 'isolated fixture requires its host helper');
  if (local.pidNamespace !== ownNamespace()) return null;
  const pid = local.localPid ?? local.pid;
  return { ...local, hostPid: pid, localPid: pid };
}
export const hostIdentity = mapRecordedIdentity;

export function captureIdentity(pid = process.pid) {
  return mapRecordedIdentity(captureLocalIdentity(pid));
}

// ---- observation ----

function localObserve(identity) {
  if (identity.pidNamespace !== ownNamespace()) return { verdict: 'unknown', live: null, pid: null, error: 'identity belongs to another PID namespace' };
  const pid = identity.localPid ?? identity.pid;
  let host;
  try { host = process.platform === 'linux' ? linuxProcess(pid) : portableProcess(pid); } catch (error) {
    if (['EACCES', 'EPERM'].includes(error?.code)) return { verdict: 'unknown', live: null, pid: null };
    throw error;
  }
  if (!host || !host.live || !sameProcess(identity, host)) return { verdict: 'gone', live: false, pid: null };
  return { verdict: 'match', live: true, pid: host.pid, args: host.args, procState: host.procState };
}

// `pid` is always a pid valid in the caller's own namespace (the one a signal may use).
export function observeIdentity(identity) {
  if (!identity) return { verdict: 'unknown', live: null, pid: null };
  if (!helperSocket() && process.env.F1_PID_ISOLATED === '1') return { verdict: 'unknown', live: null, pid: null, error: 'isolated fixture helper absent' };
  if (!helperSocket()) return localObserve(identity);
  try { return helperCallSync({ op: 'observe', identity: wire(identity) }); } catch (error) {
    return { verdict: 'unknown', live: null, pid: null, error: `${error.code ?? 'ERROR'}: ${error.message}` };
  }
}

const portableWatch = exitWatchSource();

// Local wait: kernel pidfd, kqueue NOTE_EXIT, or bounded identity-stamp polling.
// The watched pid is re-verified after arming, closing the subscription reuse window.
function localWait(identity, deadlineMs, trigger) {
  return new Promise((resolveWait, rejectWait) => {
    const child = spawn('python3', ['-u', '-c', portableWatch, String(identity.localPid ?? identity.pid), identity.startStamp, String(deadlineMs / 1000), 'none'], { stdio: ['ignore', 'pipe', 'pipe'] });
    let result = null; let stderr = ''; let settled = false; let chain = Promise.resolve();
    const finish = (fn, value) => { if (settled) return; settled = true; clearTimeout(timer); child.kill('SIGKILL'); fn(value); };
    const timer = setTimeout(() => finish(rejectWait, failure('EXIT_WATCH_TIMEOUT', 'exit watcher did not finish within its bound')), deadlineMs + 5_000);
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', (error) => finish(rejectWait, error));
    readFrames(child.stdout, (line) => {
      if (line === 'ARMED') {
        chain = chain.then(() => {
          if (localObserve(identity).verdict !== 'match') return null;
          return trigger?.();
        }).catch((error) => finish(rejectWait, error));
      } else result = line;
    });
    child.once('close', (code) => {
      void chain.then(() => {
        if (code !== 0 || !['EXIT', 'GONE', 'TIMEOUT'].includes(result)) finish(rejectWait, failure('EXIT_WATCH_FAILED', `exit watcher failed: ${stderr.trim() || result || code}`));
        else finish(resolveWait, { exited: result !== 'TIMEOUT', via: stderr.match(/^WATCH_VIA=(\w+)$/m)?.[1] ?? 'identity' });
      });
    });
  });
}

// Bounded, event-based wait for the identity's exit. `trigger` runs only after the exit subscription
// is armed (if the process is already gone there is nothing to arm and it does not run). Resolves
// with a fresh observation plus {exited, via}; a timeout resolves with the still-live observation.
export async function waitIdentityExit(identity, deadlineMs, trigger = null) {
  const bound = Math.min(60_000, Math.max(1, Math.ceil(deadlineMs)));
  if (!identity) return { verdict: 'unknown', live: null, pid: null, exited: false, via: 'none' };
  if (!helperSocket()) {
    if (localObserve(identity).live !== true) return { ...localObserve(identity), exited: localObserve(identity).live === false, via: 'identity' };
    const result = await localWait(identity, bound, trigger);
    return { ...localObserve(identity), ...result };
  }
  const reply = await helperCall({ op: 'wait', identity: wire(identity), deadlineMs: bound }, { onArmed: trigger, timeoutMs: bound + 10_000 });
  return { ...reply.observation, exited: reply.exited, via: reply.via };
}

// ---- process-group membership ----

const localGroups = new Map();

function scanLocal(pgrp) {
  const values = []; let complete = true;
  for (const name of readdirSync('/proc').filter((entry) => /^\d+$/.test(entry))) {
    try { const value = linuxProcess(Number(name)); if (value) values.push(value); } catch (error) {
      if (!['EACCES', 'EPERM'].includes(error?.code)) throw error;
      try {
        // Unreadable namespace link: only a member of the identity's own group makes the scan incomplete.
        if (Number(readFileSync(`/proc/${name}/stat`, 'utf8').replace(/^.*\) /s, '').split(/\s+/)[2]) === pgrp) complete = false;
      } catch (inner) { if (!['ENOENT', 'ESRCH'].includes(inner?.code)) complete = false; }
    }
  }
  return { values, complete };
}

// Members stay recorded after the leader exits; the group id alone is never trusted once nothing
// verified is left alive (a reused PGID is not ours).
function localGroup(identity) {
  const key = keyOf(identity);
  const members = localGroups.get(key) ?? new Map();
  const scan = scanLocal(identity.pgrp);
  const anchored = [identity, ...members.values()].some((member) => localObserve(member).live === true);
  if (anchored) {
    for (const value of scan.values) {
      if (value.pgrp === identity.pgrp && value.pidNamespace === identity.pidNamespace && BigInt(value.startStamp) >= BigInt(identity.startStamp)) members.set(keyOf(value), own(value));
    }
  }
  localGroups.set(key, members);
  return { members: [...members.values()], complete: scan.complete };
}

// true: a verified member is alive; false: observed completely and none alive; null: unknown.
export function daemonGroupAlive(identity) {
  if (!identity) return null;
  if (helperSocket()) {
    try { return helperCallSync({ op: 'groupAlive', identity: wire(identity) }); } catch { return null; }
  }
  if (process.platform !== 'linux') {
    try { process.kill(-(identity.localPid ?? identity.pid), 0); return true; } catch (error) {
      return error?.code === 'ESRCH' ? false : null;
    }
  }
  if (identity.pidNamespace !== ownNamespace()) return null;
  const group = localGroup(identity);
  return group.members.some((member) => localObserve(member).live === true) ? true : group.complete ? false : null;
}

// Signal every verified recorded member (snapshot taken by daemonGroupAlive/captureIdentity before the
// leader is signalled), each with its exit subscription armed first. Never signals a bare PGID.
export async function teardownIdentityGroup(identity, signal, deadlineMs) {
  if (!['SIGTERM', 'SIGKILL'].includes(signal)) throw failure('SIGNAL_REFUSED', 'only SIGTERM or SIGKILL');
  const bound = Math.min(60_000, Math.max(1, Math.ceil(deadlineMs)));
  if (helperSocket()) return helperCall({ op: 'groupTerminate', identity: wire(identity), signal, deadlineMs: bound }, { timeoutMs: bound + 15_000 });
  if (process.platform !== 'linux') {
    const exit = await waitIdentityExit(identity, bound, () => { try { process.kill(-(identity.localPid ?? identity.pid), signal); } catch (error) { if (error?.code !== 'ESRCH') throw error; } });
    return { members: [{ identity, exited: exit.exited, via: exit.via }], groupAlive: daemonGroupAlive(identity), complete: true };
  }
  if (identity.pidNamespace !== ownNamespace()) throw failure('NAMESPACE_REFUSED', 'identity belongs to another PID namespace');
  const group = localGroup(identity);
  const members = await Promise.all(group.members.map(async (member) => {
    if (localObserve(member).live !== true) return { identity: member, exited: true, via: 'identity' };
    const exit = await waitIdentityExit(member, bound, () => {
      if (localObserve(member).verdict !== 'match') return;
      try { process.kill(member.localPid, signal); } catch (error) { if (error?.code !== 'ESRCH') throw error; }
    });
    return { identity: member, exited: exit.exited, via: exit.via };
  }));
  return { members, groupAlive: daemonGroupAlive(identity), complete: group.complete };
}

export const PANE_END_KINDS = new Set(['stdin-closed', 'hangup', 'terminated']);

export function paneTeardownState(records, observation, script, logDir) {
  const ended = records.some((record) => PANE_END_KINDS.has(record.kind));
  const argvMatches = observation.args?.includes(script) && observation.args?.includes(logDir);
  // portable ps yields one argv string rather than NUL-delimited arguments.
  const portableMatches = process.platform !== 'linux' && observation.args?.[0]?.includes(script) && observation.args?.[0]?.includes(logDir);
  const owned = observation.verdict === 'match' && Boolean(argvMatches || portableMatches);
  const alive = observation.live !== false;
  return { ended, owned, alive, clean: !alive, signalPid: owned && observation.live ? observation.pid : null };
}

export function fleetTeardownClean(receipts) {
  return receipts.every((receipt) => !receipt.error && (receipt.daemons ?? []).every((daemon) => Boolean(daemon.exited) && daemon.groupAlive === false)
    && (receipt.panes ?? []).every((pane) => pane.clean === true));
}
