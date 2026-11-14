#!/usr/bin/env node
// Footprint benchmark (adoption todo 17): GUI and daemon memory, open descriptors and the staged
// daemon-runtime size at 1, 10 and 30 live sessions, against an isolated QA instance only.
//
//   node bench/footprint/run.mjs --platform macos --gui-exe <QA bundle>/Contents/MacOS/ferryx
//   node bench/footprint/run.mjs --platform linux --isolated-daemon --daemon-exe <debug ferryx>
//   node bench/footprint/run.mjs --platform linux --isolated-daemon --daemon-exe <exe> --kill-gui-mid-run
//
// Modes:
//   --gui-exe <path>   launches the QA GUI with private FERRYX_DATA_DIR/RUNTIME_DIR/SESSION_DIR;
//                      the GUI starts its own daemon there (supervisor.rs installs no OS job under
//                      an isolation override). The daemon pid comes from the data handshake.
//   --isolated-daemon  headless harness (omarchy, CI): stages --daemon-exe into the private
//                      daemon-runtime/<version>-<sha8>/ dir, runs it with --daemon, and spawns a
//                      companion client process (this script with --companion) that stands in for
//                      the GUI: it holds its own data connection attached to every session, so
//                      guiRssBytes is the companion's resident size.
// Either way nothing outside the run's own temp dirs is read or written, and no user daemon is
// contacted. For every N in --sessions (cumulative), sessions are opened up to N; each new one runs
// `yes | head -c <payload>` and then stays an idle shell. Once every payload finished, GUI and
// daemon RSS, the daemon's open descriptor count and the daemon-runtime dir size are sampled.
//
// --kill-gui-mid-run (failure scenario): kills the sampled GUI/companion after the first stage's
// sessions are loaded. The benchmark detects that a sampled process died, writes no
// footprint.json and exits non-zero.
//
// footprint.json: { schema, commit, platform, mode, timestamp, payloadBytes, fdLimit, samples:
// [{ sessions, guiRssBytes, daemonRssBytes, fdCount, daemonRuntimeBytes }] } with no null field.

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, watch, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

import { connectData } from '../../scripts/qa/daemon-lifetime/lib/client.mjs';
import { data as D, generationKey } from '../../scripts/qa/daemon-lifetime/lib/wire.mjs';
import { withDeadline } from '../../scripts/qa/daemon-lifetime/lib/proc.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '../..');

export const FOOTPRINT_SCHEMA = 'ferryx-footprint/v1';
export const PLATFORMS = Object.freeze(['linux', 'macos', 'windows']);
export const DEFAULT_SESSIONS = Object.freeze([1, 10, 30]);
export const DEFAULT_PAYLOAD_BYTES = 50 * 1024 * 1024;
/** Open descriptors the daemon may hold at 30 sessions (plan todo 17). */
export const FD_LIMIT = 4096;
export const SAMPLE_FIELDS = Object.freeze(['sessions', 'guiRssBytes', 'daemonRssBytes', 'fdCount', 'daemonRuntimeBytes']);

/** Typed failure; code FOOTPRINT_SAMPLED_PROCESS_DIED marks a sampled process that exited. */
export class FootprintError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = 'FootprintError';
    this.code = code;
  }
}

/** The daemon-lifetime QA libs name macOS "mac". */
export const qaPlatform = (platform) => (platform === 'macos' ? 'mac' : platform);

export function hostPlatform(nodePlatform = process.platform) {
  return { darwin: 'macos', linux: 'linux', win32: 'windows' }[nodePlatform] ?? null;
}

export function parseArgs(argv, { host = hostPlatform() } = {}) {
  const out = {
    platform: host, sessions: [...DEFAULT_SESSIONS], out: 'footprint.json', killGuiMidRun: false,
    isolatedDaemon: false, daemonExe: null, guiExe: null, payloadBytes: DEFAULT_PAYLOAD_BYTES,
    commit: null, keep: false, stageDeadlineMs: 600_000, companion: false, runtimeDir: null,
  };
  const value = (i, flag) => {
    if (i >= argv.length || String(argv[i]).startsWith('--')) throw new FootprintError('FOOTPRINT_USAGE', `${flag} needs a value`);
    return argv[i];
  };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--platform') out.platform = value(++i, a);
    else if (a === '--sessions') out.sessions = value(++i, a).split(',').map((s) => s.trim()).filter(Boolean).map(Number);
    else if (a === '--out') out.out = value(++i, a);
    else if (a === '--kill-gui-mid-run') out.killGuiMidRun = true;
    else if (a === '--isolated-daemon') out.isolatedDaemon = true;
    else if (a === '--daemon-exe') out.daemonExe = value(++i, a);
    else if (a === '--gui-exe') out.guiExe = value(++i, a);
    else if (a === '--payload-bytes') out.payloadBytes = Number(value(++i, a));
    else if (a === '--commit') out.commit = value(++i, a);
    else if (a === '--stage-deadline-ms') out.stageDeadlineMs = Number(value(++i, a));
    else if (a === '--keep') out.keep = true;
    else if (a === '--companion') out.companion = true;
    else if (a === '--runtime-dir') out.runtimeDir = value(++i, a);
    else throw new FootprintError('FOOTPRINT_USAGE', `unknown argument ${a}`);
  }
  if (!PLATFORMS.includes(out.platform)) throw new FootprintError('FOOTPRINT_USAGE', `--platform must be one of ${PLATFORMS.join('|')}`);
  if (out.companion) {
    if (!out.runtimeDir) throw new FootprintError('FOOTPRINT_USAGE', '--companion needs --runtime-dir');
    return out;
  }
  if (!out.sessions.length || out.sessions.some((n) => !Number.isInteger(n) || n < 1)) throw new FootprintError('FOOTPRINT_USAGE', '--sessions takes positive integers, e.g. 1,10,30');
  out.sessions = [...new Set(out.sessions)].sort((a, b) => a - b);
  if (!Number.isInteger(out.payloadBytes) || out.payloadBytes < 1) throw new FootprintError('FOOTPRINT_USAGE', '--payload-bytes must be a positive integer');
  if (!Number.isInteger(out.stageDeadlineMs) || out.stageDeadlineMs < 1) throw new FootprintError('FOOTPRINT_USAGE', '--stage-deadline-ms must be a positive integer');
  if (out.isolatedDaemon === Boolean(out.guiExe)) throw new FootprintError('FOOTPRINT_USAGE', 'pick exactly one mode: --gui-exe <QA GUI> or --isolated-daemon --daemon-exe <exe>');
  if (out.isolatedDaemon && !out.daemonExe) throw new FootprintError('FOOTPRINT_USAGE', '--isolated-daemon needs --daemon-exe');
  if (out.platform !== host) throw new FootprintError('FOOTPRINT_USAGE', `--platform ${out.platform} must run on that host (this is ${host})`);
  return out;
}

// ---- samplers: pure parsers plus thin OS calls ----

const UNIT = { B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3 };

/** `footprint -p <pid>` header: "ferryx [123]: 64-bit    Footprint: 1616 KB (...)". */
export function parseMacFootprint(text) {
  const m = /Footprint:\s*([\d.]+)\s*(B|KB|MB|GB)\b/.exec(String(text ?? ''));
  return m ? Math.round(Number(m[1]) * UNIT[m[2]]) : null;
}

/** `ps -o rss= -p <pid>`: kilobytes. */
export function parsePsRss(text) {
  const t = String(text ?? '').trim();
  return /^\d+$/.test(t) ? Number(t) * 1024 : null;
}

/** /proc/<pid>/smaps_rollup: { rss, pss } in bytes ("Rss:  1234 kB"). */
export function parseSmapsRollup(text) {
  const field = (name) => {
    const m = new RegExp(`^${name}:\\s+(\\d+)\\s+kB$`, 'm').exec(String(text ?? ''));
    return m ? Number(m[1]) * 1024 : null;
  };
  return { rss: field('Rss'), pss: field('Pss') };
}

/** `lsof -p <pid>`: header, then one row per entry; only numeric FD rows (e.g. "12u") are descriptors. */
export function parseLsofCount(text) {
  const lines = String(text ?? '').split('\n').filter((l) => l.trim());
  if (!lines.length || !/^COMMAND\s/.test(lines[0])) return null;
  return lines.slice(1).filter((l) => /^\S+\s+\d+\s+\S+\s+\d+[rwu]?\s/.test(l)).length;
}

/** PowerShell probe output: "<WorkingSet64> <HandleCount>". */
export function parseWindowsProcess(text) {
  const m = /^(\d+)\s+(\d+)$/.exec(String(text ?? '').trim());
  return m ? { workingSet: Number(m[1]), handles: Number(m[2]) } : null;
}

function exec(command, args) {
  const r = spawnSync(command, args, { encoding: 'utf8', windowsHide: true, maxBuffer: 64 * 1024 * 1024 });
  if (r.error) throw r.error;
  return { status: r.status, stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
}

function windowsProbe(pid) {
  const r = exec('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `$p = Get-Process -Id ${Number(pid)} -ErrorAction Stop; "$($p.WorkingSet64) $($p.HandleCount)"`]);
  return parseWindowsProcess(r.stdout);
}

/** Resident bytes of a live pid. Throws (never returns null) when the pid cannot be sampled. */
export function sampleRssBytes(pid, platform) {
  let bytes = null;
  if (platform === 'linux') {
    try { bytes = parseSmapsRollup(readFileSync(`/proc/${pid}/smaps_rollup`, 'utf8')).rss; } catch { bytes = null; }
  } else if (platform === 'macos') {
    bytes = parseMacFootprint(exec('/usr/bin/footprint', ['-p', String(pid)]).stdout);
    if (bytes === null) bytes = parsePsRss(exec('/bin/ps', ['-o', 'rss=', '-p', String(pid)]).stdout);
  } else {
    bytes = windowsProbe(pid)?.workingSet ?? null;
  }
  if (!Number.isInteger(bytes) || bytes <= 0) throw new FootprintError('FOOTPRINT_SAMPLE_FAILED', `no resident size for pid ${pid} on ${platform}`);
  return bytes;
}

/** Open descriptor (Windows: handle) count of a live pid. Throws when it cannot be sampled. */
export function sampleFdCount(pid, platform) {
  let count = null;
  if (platform === 'linux') {
    try { count = readdirSync(`/proc/${pid}/fd`).length; } catch { count = null; }
  } else if (platform === 'macos') {
    count = parseLsofCount(exec('/usr/sbin/lsof', ['-n', '-P', '-p', String(pid)]).stdout);
  } else {
    count = windowsProbe(pid)?.handles ?? null;
  }
  if (!Number.isInteger(count) || count <= 0) throw new FootprintError('FOOTPRINT_SAMPLE_FAILED', `no descriptor count for pid ${pid} on ${platform}`);
  return count;
}

/** Bytes of every file and symlink under dir (links not followed); 0 when the dir is absent. */
export function dirSizeBytes(dir) {
  let total = 0;
  const visit = (p) => {
    const st = lstatSync(p);
    if (st.isDirectory()) for (const name of readdirSync(p)) visit(path.join(p, name));
    else total += st.size;
  };
  if (existsSync(dir)) visit(dir);
  return total;
}

/** Throws unless every field of the document (recursively) is present and finite. */
export function assertNoNulls(value, where = 'footprint') {
  if (value === null || value === undefined || (typeof value === 'number' && !Number.isFinite(value))) {
    throw new FootprintError('FOOTPRINT_NULL_FIELD', `${where} is ${value}`);
  }
  if (Array.isArray(value)) value.forEach((v, i) => assertNoNulls(v, `${where}[${i}]`));
  else if (typeof value === 'object') for (const [k, v] of Object.entries(value)) assertNoNulls(v, `${where}.${k}`);
  return value;
}

/** Validates a footprint document: required fields, sample shape, no nulls, descriptor ceiling. */
export function validateFootprint(doc) {
  assertNoNulls(doc);
  for (const key of ['commit', 'platform', 'timestamp']) {
    if (typeof doc[key] !== 'string' || !doc[key]) throw new FootprintError('FOOTPRINT_INVALID', `${key} must be a non-empty string`);
  }
  if (!Array.isArray(doc.samples) || !doc.samples.length) throw new FootprintError('FOOTPRINT_INVALID', 'samples must be a non-empty array');
  for (const s of doc.samples) {
    for (const f of SAMPLE_FIELDS) {
      if (!Number.isInteger(s[f]) || s[f] < 0) throw new FootprintError('FOOTPRINT_INVALID', `sample ${s.sessions}: ${f} must be a non-negative integer`);
    }
    if (s.fdCount >= FD_LIMIT) throw new FootprintError('FOOTPRINT_FD_LIMIT', `${s.fdCount} open descriptors at ${s.sessions} sessions (must stay below ${FD_LIMIT})`);
  }
  return doc;
}

// ---- isolated instance ----

function shQuote(s) { return `'${String(s).replace(/'/g, `'\\''`)}'`; }
function psQuote(s) { return `'${String(s).replace(/'/g, "''")}'`; }

/** Line a session runs: the payload, then a done marker file; the shell then stays idle. */
export function loadCommand(platform, payloadBytes, doneFile) {
  if (platform === 'windows') {
    const chunk = 65536;
    const rounds = Math.ceil(payloadBytes / (2 * chunk));
    return `1..${rounds} | ForEach-Object { [Console]::Out.Write("y\`n" * ${chunk}) }; New-Item -ItemType File -Force ${psQuote(doneFile)} | Out-Null\r`;
  }
  // `head -c` takes a plain byte count on both GNU and BSD; BSD head rejects the 50M suffix.
  return `yes | head -c ${payloadBytes}; : > ${shQuote(doneFile)}\n`;
}

function gitCommit() {
  const r = spawnSync('git', ['-C', REPO, 'rev-parse', 'HEAD'], { encoding: 'utf8' });
  const sha = (r.stdout ?? '').trim();
  return r.status === 0 && /^[0-9a-f]{40}$/.test(sha) ? sha : null;
}

function appVersion() {
  return JSON.parse(readFileSync(path.join(REPO, 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
}

/** Copies the daemon executable into <data>/daemon-runtime/<version>-<sha8>/ like runtime_stage.rs. */
export function stageDaemon(daemonExe, dataDir, version) {
  const sha = createHash('sha256').update(readFileSync(daemonExe)).digest('hex');
  const dir = path.join(dataDir, 'daemon-runtime', generationKey(version, sha));
  mkdirSync(dir, { recursive: true });
  const staged = path.join(dir, path.basename(daemonExe));
  copyFileSync(daemonExe, staged);
  if (process.platform !== 'win32') chmodSync(staged, 0o755);
  writeFileSync(path.join(dir, '.staged-at'), String(Date.now()));
  return staged;
}

function isolatedLayout() {
  const root = mkdtempSync(path.join(os.tmpdir(), 'ferryx-footprint-'));
  // Unix socket paths are capped near 104 bytes and the macOS tmpdir is long: keep runtime short.
  const runtime = process.platform === 'win32' ? path.join(root, 'run') : mkdtempSync('/tmp/ffp-');
  const layout = { root, runtime, data: path.join(root, 'data'), sessions: path.join(root, 'sessions'), done: path.join(root, 'done'), repo: path.join(root, 'repo') };
  for (const dir of Object.values(layout)) mkdirSync(dir, { recursive: true });
  return layout;
}

/** Every location key the daemon reads points into the run's own dirs (supervisor.rs isolation). */
export function isolationEnv(layout, platform, base = process.env) {
  const env = {
    ...base,
    FERRYX_DATA_DIR: layout.data,
    FERRYX_RUNTIME_DIR: layout.runtime,
    FERRYX_SESSION_DIR: layout.sessions,
    FERRYX_DAEMON_SUPERVISOR_NAME: `ferryx-qa-footprint-${path.basename(layout.root).replace(/[^a-z0-9]/gi, '').toLowerCase()}`,
  };
  if (platform === 'macos') env.FERRYX_DAEMON_PLIST_PATH = path.join(layout.root, 'LaunchAgents');
  return env;
}

/** A child whose exit is a benchmark failure while armed. Exit is observed by event. */
export function watchChild(child, role) {
  const state = { role, pid: child.pid, armed: true, exit: null, child };
  let rejectDied;
  state.died = new Promise((_, reject) => { rejectDied = reject; });
  state.died.catch(() => {});
  state.exited = new Promise((resolve) => {
    child.once('exit', (code, signal) => {
      state.exit = { code, signal };
      if (state.armed) rejectDied(diedError(state));
      resolve(state.exit);
    });
  });
  return state;
}

function diedError(state) {
  return new FootprintError('FOOTPRINT_SAMPLED_PROCESS_DIED', `${state.role} pid ${state.pid} exited mid-run (code ${state.exit.code}, signal ${state.exit.signal})`);
}

/** Throws for any armed process that has exited. */
export function assertAlive(procs) {
  for (const p of procs) if (p.armed && p.exit) throw diedError(p);
}

function waitLine(child, prefix, deadlineMs) {
  const lines = readline.createInterface({ input: child.stdout });
  return withDeadline(new Promise((resolve, reject) => {
    lines.on('line', (line) => { if (line.startsWith(prefix)) { lines.close(); resolve(line); } });
    child.once('exit', (code, signal) => reject(new FootprintError('FOOTPRINT_LAUNCH_FAILED', `exited before ${prefix} (code ${code}, signal ${signal})`)));
  }), deadlineMs, prefix);
}

/** Connects once the data endpoint accepts, retrying on each runtime-dir change event. */
function connectWhenReady(layout, platform, deadlineMs) {
  let watcher;
  return withDeadline(new Promise((resolve, reject) => {
    let busy = false;
    let again = false;
    let done = false;
    const attempt = async () => {
      if (done) return;
      if (busy) { again = true; return; }
      busy = true;
      try {
        const c = await connectData(layout.runtime, qaPlatform(platform));
        done = true;
        resolve(c);
      } catch (error) {
        if (!['ENOENT', 'ECONNREFUSED'].includes(error.code)) { done = true; reject(error); }
      } finally {
        busy = false;
        if (again && !done) { again = false; attempt(); }
      }
    };
    watcher = watch(layout.runtime, attempt);
    attempt();
  }), deadlineMs, 'daemon data endpoint').finally(() => watcher?.close());
}

/** Resolves when every named file exists in dir, by fs.watch events (no polling). */
function waitForFiles(dir, names, deadlineMs) {
  const pending = new Set(names);
  let watcher;
  return withDeadline(new Promise((resolve) => {
    const check = () => {
      for (const n of [...pending]) if (existsSync(path.join(dir, n))) pending.delete(n);
      if (!pending.size) resolve();
    };
    watcher = watch(dir, check);
    check();
  }), deadlineMs, `payload done markers (${pending.size} pending)`).finally(() => watcher?.close());
}

async function call(conn, frame, expect, deadlineMs = 60_000) {
  const resp = await conn.call(frame, deadlineMs);
  if (resp?.type !== expect) throw new FootprintError('FOOTPRINT_PROTOCOL', `${frame.type}: expected ${expect}, got ${JSON.stringify(resp)}`);
  return resp;
}

function killChild(state) {
  if (!state || state.exit) return Promise.resolve();
  state.child.kill('SIGKILL');
  return state.exited;
}

/** Runs the benchmark; writes and returns the footprint document. */
export async function runBenchmark(opts, { log = (line) => console.error(line) } = {}) {
  const platform = opts.platform;
  const commit = opts.commit ?? gitCommit();
  if (!commit) throw new FootprintError('FOOTPRINT_NO_COMMIT', 'cannot resolve the commit (git rev-parse HEAD failed); pass --commit');
  const layout = isolatedLayout();
  const env = isolationEnv(layout, platform);
  const procs = [];
  let daemon = null;
  let gui = null;
  let conn = null;
  const launchDeadline = 120_000;
  try {
    if (opts.isolatedDaemon) {
      const staged = stageDaemon(path.resolve(opts.daemonExe), layout.data, appVersion());
      daemon = watchChild(spawn(staged, ['--daemon'], { env, cwd: layout.root, stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true }), 'daemon');
      procs.push(daemon);
      await Promise.race([waitLine(daemon.child, 'FERRYX_DAEMON_READY', launchDeadline), daemon.died]);
    } else {
      gui = watchChild(spawn(path.resolve(opts.guiExe), [], { env, cwd: layout.root, stdio: ['ignore', 'ignore', 'inherit'] }), 'gui');
      procs.push(gui);
    }
    const connected = await Promise.race([connectWhenReady(layout, platform, launchDeadline), ...procs.map((p) => p.died)]);
    conn = connected.conn;
    const daemonPid = connected.handshake.pid;
    if (!Number.isInteger(daemonPid) || daemonPid <= 0) throw new FootprintError('FOOTPRINT_PROTOCOL', `handshake has no daemon pid: ${JSON.stringify(connected.handshake)}`);
    if (opts.isolatedDaemon && daemonPid !== daemon.pid) throw new FootprintError('FOOTPRINT_NOT_ISOLATED', `handshake pid ${daemonPid} is not the launched daemon ${daemon.pid}`);
    // The daemon's own exit closes the data socket: that is the death signal in GUI mode.
    const daemonGone = conn.closedPromise.then(() => { throw new FootprintError('FOOTPRINT_SAMPLED_PROCESS_DIED', `daemon pid ${daemonPid} closed the data connection mid-run`); });
    daemonGone.catch(() => {});
    if (opts.isolatedDaemon) {
      gui = watchChild(spawn(process.execPath, [fileURLToPath(import.meta.url), '--companion', '--platform', platform, '--runtime-dir', layout.runtime], { env, stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true }), 'gui companion');
      procs.push(gui);
      await Promise.race([waitLine(gui.child, 'COMPANION_READY', launchDeadline), gui.died]);
    }
    const died = Promise.race([daemonGone, ...procs.map((p) => p.died)]);
    const guarded = (promise) => Promise.race([promise, died]);
    log(`footprint: ${opts.isolatedDaemon ? 'isolated daemon + companion' : 'QA GUI'}; gui pid ${gui.pid}, daemon pid ${daemonPid}, data ${layout.data}`);

    const workspaceId = 'qa-footprint';
    await guarded(call(conn, D.registerWorkspace(workspaceId, layout.repo), 'registerWorkspaceOk'));
    const sessionIds = [];
    const samples = [];
    for (const [stage, target] of opts.sessions.entries()) {
      const fresh = [];
      while (sessionIds.length < target) {
        const n = sessionIds.length;
        const spawned = await guarded(call(conn, D.spawnLocal({ clientRequestId: `qa-footprint-${n}`, workspaceId, cwd: layout.repo, shell: platform === 'windows' ? 'powershell' : null }), 'spawnOk'));
        sessionIds.push(spawned.sessionId);
        fresh.push(String(n));
        if (opts.isolatedDaemon) gui.child.stdin.write(`attach ${spawned.sessionId}\n`);
        await guarded(call(conn, D.write(spawned.sessionId, loadCommand(platform, opts.payloadBytes, path.join(layout.done, String(n)))), 'writeOk'));
      }
      await guarded(waitForFiles(layout.done, fresh, opts.stageDeadlineMs));
      if (stage === 0 && opts.killGuiMidRun) {
        log(`footprint: --kill-gui-mid-run: killing ${gui.role} pid ${gui.pid}`);
        await killChild(gui);
      }
      assertAlive(procs);
      const sample = {
        sessions: target,
        guiRssBytes: sampleRssBytes(gui.pid, platform),
        daemonRssBytes: sampleRssBytes(daemonPid, platform),
        fdCount: sampleFdCount(daemonPid, platform),
        daemonRuntimeBytes: dirSizeBytes(path.join(layout.data, 'daemon-runtime')),
      };
      // A process that died while it was sampled must not leave a plausible-looking row.
      assertAlive(procs);
      if (conn.closed) throw new FootprintError('FOOTPRINT_SAMPLED_PROCESS_DIED', `daemon pid ${daemonPid} closed the data connection mid-run`);
      samples.push(sample);
      log(`footprint: ${JSON.stringify(sample)}`);
    }
    const doc = validateFootprint({
      schema: FOOTPRINT_SCHEMA,
      commit,
      platform,
      mode: opts.isolatedDaemon ? 'isolated-daemon' : 'qa-gui',
      timestamp: new Date().toISOString(),
      payloadBytes: opts.payloadBytes,
      fdLimit: FD_LIMIT,
      samples,
    });
    writeFileSync(opts.out, `${JSON.stringify(doc, null, 2)}\n`);
    return doc;
  } finally {
    for (const p of procs) p.armed = false;
    if (conn) {
      if (!conn.closed) {
        try { await conn.call(D.shutdown(), 10_000); } catch { /* the daemon closes the socket as it exits */ }
      }
      conn.close();
    }
    await killChild(gui);
    await killChild(daemon);
    if (opts.keep) log(`footprint: kept ${layout.root} and ${layout.runtime}`);
    else for (const dir of new Set([layout.root, layout.runtime])) rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * Companion client (headless stand-in for the GUI): attaches to every session named on stdin.
 *
 * Each attach needs its own data connection: the daemon answers Attach and then hands that socket
 * to the session stream (server.rs DaemonRequest::Attach ends with `return` after
 * serve_attach_stream), so a second Attach on the same connection can never be read. The control
 * connection below stays attached to nothing; it exists to report the daemon's death.
 */
async function runCompanion(opts) {
  const platform = qaPlatform(opts.platform);
  const control = await connectData(opts.runtimeDir, platform);
  control.conn.closedPromise.then(() => process.exit(0));
  const streams = [];
  process.stdout.write('COMPANION_READY\n');
  for await (const line of readline.createInterface({ input: process.stdin })) {
    const m = /^attach (\S+)$/.exec(line.trim());
    if (!m) continue;
    const { conn } = await connectData(opts.runtimeDir, platform);
    streams.push(conn);
    conn.call(D.attach(m[1], 0)).catch(() => {});
  }
  process.exit(0);
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exit(2);
  }
  if (opts.companion) return runCompanion(opts);
  try {
    const doc = await runBenchmark(opts);
    console.log(`footprint: wrote ${opts.out} (${doc.samples.length} samples)`);
    process.exit(0);
  } catch (error) {
    console.error(error instanceof FootprintError ? error.message : `FOOTPRINT_FAILED: ${error?.stack ?? error}`);
    process.exit(1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
