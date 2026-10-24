// Isolation guards for the daemon-lifetime QA harness (plan todo 12).
//
// Everything this harness may touch is named after one run id: the QA bundle identifier, the QA
// launchd label / systemd unit / scheduled task, and the private data, runtime and session dirs.
// Every OS-job command passes classifyCommand before it runs, and every kill goes through
// ProcessRegistry, which kills only a pid recorded at spawn whose executable path (and start time,
// where the OS reports it) still match. Nothing here resolves a process by name or pattern.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

export const QA_PREFIX = 'ferryx-qa-';

/** UTC timestamp run id, lowercase so it is valid in launchd labels, unit names and task names. */
export function makeRunId(now = new Date()) {
  const iso = now.toISOString(); // 2026-10-19T06:45:12.345Z
  const id = `${iso.slice(0, 4)}${iso.slice(5, 7)}${iso.slice(8, 10)}t${iso.slice(11, 13)}${iso.slice(14, 16)}${iso.slice(17, 19)}z`;
  return id;
}

export function assertRunId(runId) {
  if (!/^\d{8}t\d{6}z$/.test(String(runId))) throw new GuardError('BAD_RUN_ID', `not a harness run id: ${runId}`);
  return runId;
}

export class GuardError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.code = code;
  }
}

/** FERRYX_DAEMON_SUPERVISOR_NAME for this run. Job names derive from it (todo 4-6). */
export function qaSupervisorName(runId) {
  return `${QA_PREFIX}${assertRunId(runId)}`;
}

export function qaBundleIdentifier(runId) {
  return `com.ferryx.app.qa-${assertRunId(runId)}`;
}

export function qaProductName(runId) {
  return `Ferryx QA ${assertRunId(runId)}`;
}

/** The --config overlay Tauri v2 merges over tauri.conf.json for the QA debug bundle. */
export function tauriQaConfig(runId) {
  return {
    identifier: qaBundleIdentifier(runId),
    productName: qaProductName(runId),
    plugins: { updater: { active: false } },
  };
}

/**
 * Whether an OS job name belongs to this run.
 * macOS label: <supervisor name> (legacy QA job) or <supervisor name>.<generation key>.
 * Linux unit: <supervisor name>[-<key>][.service].
 * Windows task: Ferryx\<supervisor name>[-<key>].
 */
export function isQaJobName(name, runId, platform) {
  const base = qaSupervisorName(runId);
  const value = String(name ?? '');
  if (platform === 'mac') return value === base || value.startsWith(`${base}.`);
  if (platform === 'linux') {
    const unit = value.replace(/\.(service|scope)$/, '');
    return unit === base || unit.startsWith(`${base}-`);
  }
  if (platform === 'windows') {
    const task = value.replace(/^\\/, '');
    return task === `Ferryx\\${base}` || task.startsWith(`Ferryx\\${base}-`);
  }
  return false;
}

/** Throws unless the bundle's CFBundleIdentifier is exactly this run's QA identifier. */
export function assertBundleIdentifier(actual, runId) {
  const expected = qaBundleIdentifier(runId);
  if (String(actual ?? '').trim() !== expected) {
    throw new GuardError('BUNDLE_ID_MISMATCH', `expected ${expected}, found ${JSON.stringify(actual)}`);
  }
  return expected;
}

/** Paths that belong to the user's real installation; the harness never runs or edits them. */
export function forbiddenRoots({ platform, home, uid = null, localAppData = null }) {
  const roots = [];
  if (platform === 'mac') {
    roots.push('/Applications', path.join(home, 'Applications'));
    if (uid !== null) roots.push(`/tmp/ferryx-${uid}`, `/tmp/ferryx-${uid}-dev`, `/tmp/rorca-${uid}`, `/tmp/rorca-${uid}-dev`);
    roots.push(path.join(home, 'Library/Application Support/Ferryx'), path.join(home, 'Library/LaunchAgents/com.ferryx.daemon'));
  }
  if (platform === 'linux') {
    roots.push('/usr/bin/ferryx', '/opt/ferryx', path.join(home, '.local/share/ferryx'), path.join(home, '.config/systemd/user/ferryx-daemon'));
    if (uid !== null) roots.push(`/tmp/ferryx-${uid}`, `/tmp/ferryx-${uid}-dev`, `/tmp/rorca-${uid}`, `/tmp/rorca-${uid}-dev`);
  }
  if (platform === 'windows') {
    const lad = localAppData ?? path.win32.join(home, 'AppData', 'Local');
    roots.push(path.win32.join(lad, 'Ferryx'), path.win32.join(lad, 'Programs', 'Ferryx'), 'C:\\Program Files\\Ferryx');
  }
  roots.push(path.join(home, 'ferryx-releases'));
  return roots;
}

function norm(p, platform) {
  const mod = platform === 'windows' ? path.win32 : path.posix;
  const resolved = mod.resolve(String(p));
  return platform === 'windows' ? resolved.toLowerCase() : resolved;
}

export function isInside(child, parent, platform) {
  const c = norm(child, platform);
  const p = norm(parent, platform);
  const sep = platform === 'windows' ? '\\' : '/';
  return c === p || c.startsWith(p.endsWith(sep) ? p : p + sep);
}

/**
 * Positive allowlist: a QA executable must live under one of the run's own roots (the worktree
 * target dir or the run dir) and under none of the user's installation roots.
 */
export function assertQaExecutable(executable, { allowedRoots, forbidden, platform }) {
  for (const root of forbidden) {
    if (isInside(executable, root, platform)) throw new GuardError('RELEASE_PATH', `${executable} is inside ${root}`);
  }
  if (!allowedRoots.some((root) => isInside(executable, root, platform))) {
    throw new GuardError('NOT_A_QA_ARTIFACT', `${executable} is outside the QA roots ${allowedRoots.join(', ')}`);
  }
  return executable;
}

/**
 * Private directories and env for one run. Every key the daemon reads for its location is set.
 * runtimeDir places the socket dir outside runDir (short path on a host whose runDir is long).
 */
export function isolationLayout(runDir, platform, { runtimeDir = null } = {}) {
  const mod = platform === 'windows' ? path.win32 : path.posix;
  const dirs = {
    root: runDir,
    home: mod.join(runDir, 'home'),
    data: mod.join(runDir, 'data'),
    runtime: runtimeDir ?? mod.join(runDir, 'run'),
    sessions: mod.join(runDir, 'sessions'),
    config: mod.join(runDir, 'home', 'config'),
    xdgData: mod.join(runDir, 'home', 'data'),
    launchAgents: mod.join(runDir, 'LaunchAgents'),
    logs: mod.join(runDir, 'logs'),
    barriers: mod.join(runDir, 'barriers'),
    evidence: mod.join(runDir, 'evidence'),
  };
  return dirs;
}

/**
 * The private sshd's known_hosts (lib/sshd.mjs writes it). A file, not a layout dir: run.mjs
 * mkdirs every layout value.
 */
export function sshKnownHostsFile(layout) {
  const mod = /^[A-Za-z]:[\\/]/.test(layout.root) ? path.win32 : path.posix;
  return mod.join(layout.root, 'sshd', 'known_hosts');
}

/**
 * Linux keeps the real HOME/XDG_CONFIG_HOME: systemd --user reads units only from the user's own
 * ~/.config/systemd/user, so QA units must land there (named ferryx-qa-<runId>-*). Windows keeps
 * LOCALAPPDATA for the same reason (task definitions). Only macOS moves HOME, because launchd
 * plists go to FERRYX_DAEMON_PLIST_PATH and nothing else needs the real home.
 */
export function isolationEnv({ runId, layout, platform, base = {} }) {
  const env = {
    ...base,
    FERRYX_DATA_DIR: layout.data,
    FERRYX_RUNTIME_DIR: layout.runtime,
    FERRYX_SESSION_DIR: layout.sessions,
    FERRYX_DAEMON_SUPERVISOR_NAME: qaSupervisorName(runId),
    FERRYX_QA_RUN_ID: runId,
    FERRYX_QA_BARRIER_DIR: layout.barriers,
    // local-split-qa only (ssh/direct.rs): the daemon's ssh trusts exactly the private sshd's pinned
    // key. The file is written by startSshd; ssh reads it per connection, so it may appear later.
    FERRYX_QA_SSH_KNOWN_HOSTS: sshKnownHostsFile(layout),
  };
  if (platform === 'mac') env.FERRYX_DAEMON_PLIST_PATH = layout.launchAgents;
  if (platform === 'mac') env.HOME = layout.home;
  return env;
}

/** Refuses an env that would point a QA process at the user's real daemon. */
export function assertIsolatedEnv(env, { runId, forbidden, platform }) {
  for (const key of ['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_SESSION_DIR']) {
    if (!env[key]) throw new GuardError('ENV_NOT_ISOLATED', `${key} is unset`);
    for (const root of forbidden) {
      if (isInside(env[key], root, platform)) throw new GuardError('ENV_NOT_ISOLATED', `${key}=${env[key]} is inside ${root}`);
    }
  }
  if (env.FERRYX_DAEMON_SUPERVISOR_NAME !== qaSupervisorName(runId)) {
    throw new GuardError('ENV_NOT_ISOLATED', 'FERRYX_DAEMON_SUPERVISOR_NAME is not this run\'s QA name');
  }
  return env;
}

const LAUNCHCTL_READ = new Set(['print', 'list', 'print-disabled', 'blame']);
const LAUNCHCTL_QA_MUTATIONS = new Set(['bootstrap', 'bootout', 'kickstart', 'enable', 'disable']);
const SYSTEMCTL_READ = new Set(['show', 'status', 'is-active', 'is-enabled', 'list-units', 'list-unit-files', 'cat']);
const SYSTEMCTL_QA_MUTATIONS = new Set(['start', 'stop', 'kill', 'disable', 'enable', 'daemon-reload', 'reset-failed']);
const SCHTASKS_QA_MUTATIONS = new Set(['/run', '/end', '/delete', '/create', '/change']);

/**
 * Decides whether an OS-job command may run. Read-only queries are always allowed. A mutation is
 * allowed only when every job it names belongs to this run; kickstart -k is never allowed.
 * Returns { allowed, reason }.
 */
export function classifyCommand(command, args, { runId, platform }) {
  const cmd = path.basename(String(command)).toLowerCase().replace(/\.exe$/, '');
  const list = args.map(String);
  const deny = (reason) => ({ allowed: false, reason });
  if (['kill', 'pkill', 'killall', 'taskkill'].includes(cmd)) return deny('process kills go through ProcessRegistry only');
  if (cmd === 'launchctl') {
    const verb = list[0];
    if (LAUNCHCTL_READ.has(verb)) return { allowed: true, reason: 'read-only' };
    if (!LAUNCHCTL_QA_MUTATIONS.has(verb)) return deny(`launchctl ${verb} is not a harness operation`);
    if (list.includes('-k')) return deny('kickstart -k is forbidden');
    const targets = list.slice(1).filter((a) => !a.startsWith('-'));
    const named = targets.map((t) => {
      if (t.endsWith('.plist')) return path.basename(t, '.plist');
      const m = /^gui\/\d+\/(.+)$/.exec(t);
      return m ? m[1] : null;
    }).filter((t) => t !== null);
    if (named.length === 0) return deny(`launchctl ${verb} names no job`);
    if (!named.every((n) => isQaJobName(n, runId, 'mac'))) return deny(`launchctl ${verb} targets a non-QA job`);
    return { allowed: true, reason: 'qa job' };
  }
  if (cmd === 'systemctl') {
    const rest = list.filter((a) => a !== '--user' && a !== '--no-block' && a !== '--no-pager' && !a.startsWith('-p') && !a.startsWith('--property'));
    if (!list.includes('--user')) return deny('systemctl without --user');
    const verb = rest[0];
    if (SYSTEMCTL_READ.has(verb)) return { allowed: true, reason: 'read-only' };
    if (!SYSTEMCTL_QA_MUTATIONS.has(verb)) return deny(`systemctl ${verb} is not a harness operation`);
    if (list.includes('--now') && verb === 'disable') return deny('disable --now stops a unit');
    if (verb === 'daemon-reload') return { allowed: true, reason: 'reload of unit files only' };
    // Option flags (e.g. --signal=SIGKILL for Q03's scope kill) are not unit names.
    const units = rest.slice(1).filter((a) => !a.startsWith('-'));
    if (units.length === 0 || !units.every((u) => isQaJobName(u, runId, 'linux'))) return deny(`systemctl ${verb} targets a non-QA unit`);
    return { allowed: true, reason: 'qa unit' };
  }
  if (cmd === 'schtasks') {
    const lower = list.map((a) => a.toLowerCase());
    if (lower.includes('/query')) return { allowed: true, reason: 'read-only' };
    const verb = lower.find((a) => SCHTASKS_QA_MUTATIONS.has(a));
    if (!verb) return deny('unknown schtasks operation');
    const tn = list[lower.indexOf('/tn') + 1];
    if (lower.indexOf('/tn') < 0 || !isQaJobName(tn, runId, 'windows')) return deny(`schtasks ${verb} targets a non-QA task`);
    return { allowed: true, reason: 'qa task' };
  }
  return { allowed: true, reason: 'not an OS-job command' };
}

/** Throws a GuardError unless classifyCommand allows the command. */
export function assertCommandAllowed(command, args, ctx) {
  const verdict = classifyCommand(command, args, ctx);
  if (!verdict.allowed) throw new GuardError('COMMAND_REFUSED', `${command} ${args.join(' ')}: ${verdict.reason}`);
  return verdict;
}

export function sha256File(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

/**
 * Pids this run spawned or observed as its own, each with the identity captured at record time.
 * kill() re-reads the identity through the injected probe and refuses on any difference or on a
 * pid it never recorded. The probe returns { executable, startTime } or null when the pid is gone.
 */
export class ProcessRegistry {
  constructor({ probe, signal, platform }) {
    this.probe = probe;
    this.signal = signal;
    this.platform = platform;
    this.entries = new Map();
    this.log = [];
  }

  async record(pid, { role, executable, startTime = null, sha256 = null }) {
    if (!Number.isInteger(pid) || pid <= 0) throw new GuardError('BAD_PID', `refusing to record ${pid}`);
    const live = await this.probe(pid);
    if (!live) throw new GuardError('PID_GONE', `${role} pid ${pid} exited before it was recorded`);
    if (norm(live.executable, this.platform) !== norm(executable, this.platform)) {
      throw new GuardError('IDENTITY_MISMATCH', `${role} pid ${pid} runs ${live.executable}, expected ${executable}`);
    }
    const entry = { pid, role, executable, startTime: startTime ?? live.startTime ?? null, sha256 };
    this.entries.set(pid, entry);
    this.log.push({ op: 'record', ...entry });
    return entry;
  }

  has(pid) {
    return this.entries.has(pid);
  }

  /**
   * Re-validates a recorded pid. Returns 'match' | 'gone' | 'reused' | 'mismatch' | 'unrecorded'.
   * 'reused': the pid now names a process with a different exact start time than the recorded one.
   * A pid names one process at a time, so the recorded process has exited (all3: g9's pid 7716
   * was g28's launcher nine minutes later). Without both start times there is no such proof, and
   * the same start under another executable stays 'mismatch'.
   */
  async check(pid) {
    const entry = this.entries.get(pid);
    if (!entry) return 'unrecorded';
    const live = await this.probe(pid);
    if (!live) return 'gone';
    const knownStarts = entry.startTime !== null && live.startTime !== null && live.startTime !== undefined;
    if (knownStarts && String(live.startTime) !== String(entry.startTime)) return 'reused';
    if (norm(live.executable, this.platform) !== norm(entry.executable, this.platform)) return 'mismatch';
    return 'match';
  }

  /** Signals a recorded pid that still matches; false when the recorded process is gone or its pid was reused (never signalled). */
  async kill(pid, sig = 'SIGTERM') {
    const state = await this.check(pid);
    this.log.push({ op: 'kill', pid, sig, state });
    if (state === 'unrecorded') throw new GuardError('KILL_REFUSED', `pid ${pid} was never recorded by this run`);
    if (state === 'mismatch') throw new GuardError('KILL_REFUSED', `pid ${pid} no longer runs its recorded executable`);
    if (state === 'gone' || state === 'reused') return false;
    await this.signal(pid, sig);
    return true;
  }

  forget(pid) {
    this.entries.delete(pid);
  }

  list() {
    return [...this.entries.values()];
  }
}
