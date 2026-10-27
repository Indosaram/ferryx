// Scenario-agnostic primitives for scripts/qa/unified-supervisor.mjs:
// config isolation, evidence, secret redaction, recorded-PID ownership,
// subscribe-before-trigger barriers, bounded deadlines and receipts.
//
// Ownership model. Every process the runner starts is recorded (pid, absolute
// executable, argv, spawn time, owning runner) in an append-only ledger under
// the configured dataRoot immediately after spawn(), and its OS start stamp is
// appended as soon as it is read. Teardown signals only recorded processes:
// in-run through the unreaped child handle (its pid cannot have been reused);
// across runs (--cleanup) only when the recorded and observed OS start stamps
// are both present and equal and argv still matches the launch (or the
// interpreter-resolved argv observed at spawn, e.g. for #! scripts). Anything
// else is reported (mismatch / argv-changed / unverifiable) and never
// signalled; only a provably different process (unequal start stamps, or no
// recorded stamp and a start time outside the spawn window) is released.
// One run at a time per dataRoot: an exclusive run lock under
// <dataRoot>/.qa-unified-supervisor refuses a concurrent run (RUN_LOCKED).
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  appendFileSync, createReadStream, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync,
  readlinkSync, realpathSync, renameSync, rmSync, statSync, unlinkSync, watch, writeFileSync,
} from 'node:fs';
import { arch, homedir, hostname, release, tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { isDeepStrictEqual } from 'node:util';

export const PLATFORM = process.platform;
export const POSIX = PLATFORM !== 'win32';
export const SCHEMA = 'unified-supervisor/2';

export const EXIT = Object.freeze({ PASS: 0, FAIL: 1, USAGE: 2, NOT_IMPLEMENTED: 3, HARNESS: 4, CLEANUP: 5, INTERRUPTED: 130 });
export const STATUS_EXIT = Object.freeze({
  PASS: EXIT.PASS,
  FAIL: EXIT.FAIL,
  NOT_IMPLEMENTED: EXIT.NOT_IMPLEMENTED,
  DEADLINE_EXCEEDED: EXIT.HARNESS,
  HARNESS_ERROR: EXIT.HARNESS,
  CLEANUP_FAILED: EXIT.CLEANUP,
  INTERRUPTED: EXIT.INTERRUPTED,
});
const STATUS_PRECEDENCE = ['INTERRUPTED', 'HARNESS_ERROR', 'DEADLINE_EXCEEDED', 'CLEANUP_FAILED', 'FAIL', 'NOT_IMPLEMENTED'];
const USAGE_CODES = new Set([
  'USAGE', 'CONFIG_INVALID', 'PRODUCTION_PATH_REFUSED', 'FOREIGN_STATE_ROOT', 'EVIDENCE_DIR_NOT_EMPTY', 'UNKNOWN_SCENARIO',
  'INTERNAL_FLAG_REFUSED', 'STALE_OWNED_STATE', 'RUN_LOCKED',
]);

const FORCE_EXIT_MS = 5_000;
const MEMBER_GONE_MS = 3_000;
const PROBE_TIMEOUT_MS = 15_000;
const LOG_CAP_BYTES = 16 * 1024 * 1024;
const LINE_CAP = 64 * 1024;
const OUTPUT_CAP_BYTES = 4 * 1024 * 1024;
const LINEAGE_SLACK_MS = 5_000;
export const MAX_SOCKET_PATH_BYTES = 100;
export const RUN_ID_MAX = 24;
const RUN_ID_PATTERN = /^qa-[a-z0-9]+-[a-z0-9]+-\d+$/;
export const DEFAULT_SCENARIO_DEADLINE_MS = 15 * 60_000;
let runCounter = 0;

export class HarnessError extends Error {
  constructor(code, message, detail = undefined) {
    super(`${code}: ${message}`);
    this.name = 'HarnessError';
    this.code = code;
    this.detail = detail;
  }
}

export function errorInfo(error) {
  if (error instanceof HarnessError) return { code: error.code, message: error.message, detail: error.detail ?? null };
  return { code: 'UNEXPECTED', message: String(error?.stack ?? error) };
}

export function exitCodeForError(error) {
  if (USAGE_CODES.has(error?.code)) return EXIT.USAGE;
  if (error?.code === 'INTERRUPTED') return EXIT.INTERRUPTED;
  return EXIT.HARNESS;
}

// An empty result set is never a PASS; anything not PASS (including SKIPPED) is not a PASS.
export function aggregateExit(results) {
  if (results.length === 0) return { status: 'NOT_RUN', exitCode: EXIT.FAIL };
  if (results.every((result) => result.status === 'PASS')) return { status: 'PASS', exitCode: EXIT.PASS };
  for (const status of STATUS_PRECEDENCE) {
    if (results.some((result) => result.status === status)) return { status, exitCode: STATUS_EXIT[status] };
  }
  return { status: 'FAIL', exitCode: EXIT.FAIL };
}

// A fault (unhandled rejection / uncaught exception) anywhere in the process,
// including between scenarios or after one returned, forces HARNESS_ERROR
// unless the run was interrupted.
export function finalExitCode(exitCode, faults) {
  if (exitCode === EXIT.INTERRUPTED || !Array.isArray(faults) || faults.length === 0) return exitCode;
  return EXIT.HARNESS;
}

export const isSha256 = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
export const stamp = () => new Date().toISOString().replace(/[:.]/g, '-');
export const safeName = (value) => String(value).replace(/[^A-Za-z0-9._-]/g, '_');
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// Short and sun_path-friendly: qa-<base36 ms>-<base36 pid>-<n>.
export function newRunId() {
  const id = `qa-${Date.now().toString(36)}-${process.pid.toString(36)}-${++runCounter}`;
  if (id.length > RUN_ID_MAX) throw new HarnessError('HARNESS_ERROR', `run id ${id} exceeds ${RUN_ID_MAX} characters`);
  return id;
}

export function runtimeSocketPath(dataRoot, runId) {
  return join(dataRoot, 'runs', runId, 'runtime', 'daemon.sock');
}

// Resolves after `ms` with onTimeout(); the timer is always cleared.
export function withDeadline(promise, ms, onTimeout) {
  let timer;
  const expiry = new Promise((resolvePromise) => { timer = setTimeout(() => resolvePromise(onTimeout()), ms); });
  return Promise.race([promise, expiry]).finally(() => clearTimeout(timer));
}

// ---------------------------------------------------------------------------
// Secret redaction. Every byte written into evidence passes through here.

const SECRET_NAME = /(token|secret|passw(?:or)?d|api[-_]?key|authorization|cookie|credential|private[-_]?key|session[-_]?key)/i;
const SECRET_PATTERNS = [
  [/\bxox[a-z][.-][A-Za-z0-9.-]{6,}/g, '[REDACTED:slack-token]'],
  [/\bxapp-[A-Za-z0-9-]{6,}/g, '[REDACTED:slack-app-token]'],
  [/https:\/\/hooks\.slack\.com\/(?:services|workflows|triggers)\/[A-Za-z0-9/_-]+/g, '[REDACTED:slack-webhook]'],
  [/\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g, '[REDACTED:jwt]'],
  [/\bsk-(?:ant-)?[A-Za-z0-9_-]{12,}/g, '[REDACTED:api-key]'],
  [/\bgh[pousr]_[A-Za-z0-9]{20,}/g, '[REDACTED:github-token]'],
  [/\bAIza[0-9A-Za-z_-]{35}/g, '[REDACTED:google-key]'],
  [/\bAKIA[0-9A-Z]{16}\b/g, '[REDACTED:aws-key]'],
  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]{8,}/gi, '$1 [REDACTED]'],
  // JSON "…token…": "value"
  [/("[^"\\]*(?:token|secret|passw(?:or)?d|api[-_]?key|authorization|cookie|credential|private[-_]?key)[^"\\]*"\s*:\s*)"(?:[^"\\]|\\.)*"/gi, '$1"[REDACTED]"'],
  // CLI flags: --token v, --api-key=v (a flag starts a word, so prose such as "refresh-token expired" is kept)
  [/(^|[\s"'])(--?[A-Za-z0-9-]*(?:token|secret|passw(?:or)?d|api[-_]?key)[A-Za-z0-9-]*)(=|[ \t]+)(?!\[REDACTED)(\\"[^"\\\n]+\\"|"[^"\n,:{}[\]]+"|'[^'\n]+'|[^\s"'=\\]+)/gim, '$1$2$3[REDACTED]'],
  // env / query style: token=v, SLACK_BOT_TOKEN=v, ?token=v&, API_KEY="v", token='v', and the JSON-escaped API_KEY=\"v\".
  // A double-quoted value never spans JSON structure (, : { } [ ]), so redacting cannot break a JSON line.
  [/\b([A-Za-z0-9_]*(?:token|secret|passw(?:or)?d|api_?key)[A-Za-z0-9_]*)=(?!\[REDACTED)(\\"[^"\\\n]+\\"|"[^"\n,:{}[\]]+"|'[^'\n]+'|[^\s"',;&\\]+)/gi, '$1=[REDACTED]'],
  // YAML / header style: token: v, X-Api-Key: v (also inside a JSON string value)
  [/(^|[\s{,"])([A-Za-z0-9_-]*(?:token|secret|passw(?:or)?d|api[-_]?key)[A-Za-z0-9_-]*)([ \t]*:[ \t]+)(?!\[REDACTED)("[^"\n]*"|'[^'\n]*'|[^\s"',}\]]+)/gim, '$1$2$3[REDACTED]'],
];
let envSecrets = null;

function secretEnvValues() {
  envSecrets ??= Object.entries(process.env)
    .filter(([name, value]) => SECRET_NAME.test(name) && typeof value === 'string' && value.length >= 8)
    .sort((a, b) => b[1].length - a[1].length);
  return envSecrets;
}

export function redact(text) {
  let out = String(text);
  for (const [name, value] of secretEnvValues()) {
    if (out.includes(value)) out = out.split(value).join(`[REDACTED:env:${name}]`);
  }
  for (const [pattern, replacement] of SECRET_PATTERNS) out = out.replace(pattern, replacement);
  return out;
}

// Leaf-wise redaction before serialisation, so secrets nested in strings that
// would later be JSON-escaped are still caught.
export function redactValue(value, key = null) {
  if (typeof value === 'string') return key !== null && SECRET_NAME.test(key) ? '[REDACTED]' : redact(value);
  if (Array.isArray(value)) return value.map((item) => redactValue(item));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([name, item]) => [name, redactValue(item, name)]));
  }
  return value;
}

// ---------------------------------------------------------------------------
// Paths and production refusal.

const CASE_INSENSITIVE = PLATFORM === 'darwin' || PLATFORM === 'win32';

function norm(path) {
  let out = resolve(String(path).replace(/^\\\\\?\\/, ''));
  if (out.length > 1 && (out.endsWith('/') || out.endsWith('\\')) && !/^[A-Za-z]:[\\/]$/.test(out)) out = out.slice(0, -1);
  return CASE_INSENSITIVE ? out.toLowerCase() : out;
}

export function isInside(child, parent) {
  const c = norm(child);
  const p = norm(parent);
  if (c === p) return true;
  const rel = relative(p, c);
  return rel !== '' && rel.split(sep)[0] !== '..' && !isAbsolute(rel);
}

// Real path of `path`, or of its nearest existing ancestor joined with the rest.
export function realish(path) {
  let current = resolve(String(path).replace(/^\\\\\?\\/, ''));
  const rest = [];
  for (;;) {
    try { return join(realpathSync(current), ...rest.reverse()); } catch { /* walk up */ }
    const parent = dirname(current);
    if (parent === current) return resolve(path);
    rest.push(basename(current));
    current = parent;
  }
}

// Every production root in both spellings: as written and symlink-resolved.
export function productionRoots({ env = process.env, home = homedir(), uid = process.getuid?.() ?? null } = {}) {
  const roots = [
    join(home, '.ferryx'),
    join(home, '.config', 'ferryx'), join(home, '.local', 'share', 'ferryx'), join(home, '.local', 'state', 'ferryx'), join(home, '.cache', 'ferryx'),
    join(home, '.config', 'com.ferryx.app'), join(home, '.local', 'share', 'com.ferryx.app'),
    join(home, 'Library', 'Application Support', 'ferryx'), join(home, 'Library', 'Application Support', 'com.ferryx.app'),
    join(home, 'Library', 'Application Support', 'com.ferryx.mobile'),
    join(home, '.maho'), join(home, '.mhc'), join(home, '.config', 'maho'), join(home, '.config', 'mhc'),
    join(home, '.local', 'share', 'maho'), join(home, '.local', 'share', 'mhc'),
  ];
  for (const base of [env.XDG_CONFIG_HOME, env.XDG_DATA_HOME, env.XDG_STATE_HOME, env.XDG_CACHE_HOME]) {
    if (base && isAbsolute(base)) for (const name of ['ferryx', 'com.ferryx.app', 'maho', 'mhc']) roots.push(join(base, name));
  }
  if (env.XDG_RUNTIME_DIR && isAbsolute(env.XDG_RUNTIME_DIR)) roots.push(join(env.XDG_RUNTIME_DIR, 'ferryx'));
  if (POSIX) {
    // Daemon runtime dirs (/tmp/ferryx-{uid}[-dev], legacy /tmp/rorca-{uid}) and the host endpoint dir.
    roots.push('/tmp/ferryx', join(env.TMPDIR && isAbsolute(env.TMPDIR) ? env.TMPDIR : '/tmp', 'ferryx'));
    if (uid !== null) for (const name of [`ferryx-${uid}`, `ferryx-${uid}-dev`, `rorca-${uid}`, `rorca-${uid}-dev`]) roots.push(join('/tmp', name));
  } else {
    roots.push(join(tmpdir(), 'ferryx'));
  }
  for (const base of [env.APPDATA, env.LOCALAPPDATA]) {
    if (base && isAbsolute(base)) roots.push(join(base, 'ferryx'), join(base, 'com.ferryx.app'), join(base, 'maho'), join(base, 'mhc'));
  }
  // The invoking shell's live Ferryx locations are production by definition.
  for (const name of ['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_HUB_DB', 'FERRYX_HUB_CONFIG', 'FERRYX_SESSION_DIR',
    'FERRYX_ACCOUNT_DATA_DIR', 'FERRYX_RELAY_DATA_DIR', 'FERRYX_LEGACY_RUNTIME_DIR']) {
    if (env[name] && isAbsolute(env[name])) roots.push(env[name]);
  }
  const all = new Set();
  for (const root of roots) { all.add(resolve(root)); all.add(realish(root)); }
  return [...all];
}

function namesHubDatabase(path) {
  return path.split(/[\\/]+/).some((segment) => segment.toLowerCase() === 'hub.sqlite');
}

export function assertNotProduction(path, label, context = {}) {
  const roots = productionRoots(context);
  for (const candidate of new Set([resolve(path), realish(path)])) {
    if (namesHubDatabase(candidate)) {
      throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} names a hub.sqlite database`);
    }
    for (const root of roots) {
      if (isInside(candidate, root)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} is inside production path ${root}`);
    }
  }
}

export function assertIsolatedRoot(path, label, context = {}) {
  assertNotProduction(path, label, context);
  const home = context.home ?? homedir();
  const roots = productionRoots(context);
  for (const candidate of new Set([resolve(path), realish(path)])) {
    for (const root of roots) {
      if (isInside(root, candidate)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} contains production path ${root}`);
    }
    for (const h of new Set([resolve(home), realish(home)])) {
      if (isInside(h, candidate)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} contains the home directory ${h}`);
    }
  }
}

// ---------------------------------------------------------------------------
// Config.

export const CONFIG_REQUIRED = Object.freeze(['ferryxCli', 'ferryxApp', 'mhcBin', 'dataRoot', 'memoryRoot', 'projectRoot', 'ownerPrincipal']);
export const CONFIG_OPTIONAL = Object.freeze(['pairedHostId', 'slackChannel', '$schema', '$comment']);
const BINARY_FIELDS = ['ferryxCli', 'ferryxApp', 'mhcBin'];
const ROOT_FIELDS = ['dataRoot', 'memoryRoot', 'projectRoot'];
const STATE_ROOT_ENTRIES = new Set(['.qa-unified-supervisor', 'runs']);

export function loadConfig(path, context = {}) {
  if (!path) throw new HarnessError('CONFIG_INVALID', '--config <json> is required');
  const file = resolve(path);
  let raw;
  try { raw = readFileSync(file); } catch (error) { throw new HarnessError('CONFIG_INVALID', `cannot read ${file}: ${error?.message ?? error}`); }
  let config;
  try { config = JSON.parse(raw.toString('utf8')); } catch (error) { throw new HarnessError('CONFIG_INVALID', `${file} is not JSON: ${error?.message ?? error}`); }
  if (!config || typeof config !== 'object' || Array.isArray(config)) throw new HarnessError('CONFIG_INVALID', `${file} must hold a JSON object`);

  const unknown = Object.keys(config).filter((key) => !CONFIG_REQUIRED.includes(key) && !CONFIG_OPTIONAL.includes(key));
  if (unknown.length > 0) throw new HarnessError('CONFIG_INVALID', `unknown field(s): ${unknown.join(', ')}`);
  const missing = CONFIG_REQUIRED.filter((key) => typeof config[key] !== 'string' || config[key].trim() === '');
  if (missing.length > 0) throw new HarnessError('CONFIG_INVALID', `missing required field(s): ${missing.join(', ')}`);
  for (const key of CONFIG_OPTIONAL) {
    if (config[key] === undefined || config[key] === null) continue;
    if (typeof config[key] !== 'string' || config[key].trim() === '') throw new HarnessError('CONFIG_INVALID', `${key} must be a non-empty string or null`);
  }
  for (const key of [...BINARY_FIELDS, ...ROOT_FIELDS]) {
    const value = config[key];
    if (value.startsWith('~')) throw new HarnessError('CONFIG_INVALID', `${key} must be absolute; ~ is never expanded`);
    if (!isAbsolute(value)) throw new HarnessError('CONFIG_INVALID', `${key} must be an absolute path, got ${value}`);
    if (value.split(/[\\/]+/).includes('..')) throw new HarnessError('CONFIG_INVALID', `${key} must not contain .. segments`);
  }
  for (const key of BINARY_FIELDS) assertNotProduction(config[key], key, context);
  for (const key of ROOT_FIELDS) assertIsolatedRoot(config[key], key, context);
  for (const a of ROOT_FIELDS) {
    for (const b of ROOT_FIELDS) {
      if (a !== b && (isInside(config[a], config[b]) || isInside(realish(config[a]), realish(config[b])))) {
        throw new HarnessError('CONFIG_INVALID', `${a} and ${b} must be disjoint roots`);
      }
    }
  }
  for (const key of BINARY_FIELDS) {
    let info;
    try { info = statSync(config[key]); } catch { throw new HarnessError('CONFIG_INVALID', `${key} ${config[key]} does not exist`); }
    if (key !== 'ferryxApp' && !info.isFile()) throw new HarnessError('CONFIG_INVALID', `${key} ${config[key]} is not a file`);
  }
  if (POSIX) {
    const socket = runtimeSocketPath(config.dataRoot, 'q'.repeat(RUN_ID_MAX));
    const bytes = Buffer.byteLength(socket);
    if (bytes > MAX_SOCKET_PATH_BYTES) {
      throw new HarnessError('CONFIG_INVALID', `dataRoot is too long: the isolated daemon socket path would be ${bytes} bytes (limit ${MAX_SOCKET_PATH_BYTES}); choose a shorter dataRoot`);
    }
  }
  for (const key of ['dataRoot', 'memoryRoot']) {
    if (!existsSync(config[key])) continue;
    if (!statSync(config[key]).isDirectory()) throw new HarnessError('FOREIGN_STATE_ROOT', `${key} ${config[key]} exists and is not a directory`);
    const foreign = readdirSync(config[key]).filter((name) => !STATE_ROOT_ENTRIES.has(name));
    if (foreign.length > 0) {
      throw new HarnessError('FOREIGN_STATE_ROOT', `${key} ${config[key]} already holds state this runner did not create (${foreign.slice(0, 5).join(', ')}); use an empty task-owned root`);
    }
  }
  if (!/^\S+$/.test(config.ownerPrincipal)) throw new HarnessError('CONFIG_INVALID', 'ownerPrincipal must not contain whitespace');
  if (config.slackChannel && !/^[CDG][A-Z0-9]{6,}$/.test(config.slackChannel)) {
    throw new HarnessError('CONFIG_INVALID', 'slackChannel must be a Slack channel id such as C0123456789');
  }
  return Object.freeze({ config: Object.freeze({ ...config }), path: file, sha256: sha256(raw), bytes: raw.length });
}

// ---------------------------------------------------------------------------
// Evidence. All writes are synchronous so an abrupt exit never loses a receipt.

export class Evidence {
  constructor(dir) {
    this.dir = resolve(dir);
    mkdirSync(join(this.dir, 'logs'), { recursive: true });
  }

  static prepare(dir, { force = false } = {}) {
    const target = resolve(dir);
    assertIsolatedRoot(target, 'evidence directory');
    let movedPrior = null;
    if (existsSync(target)) {
      if (!lstatSync(target).isDirectory()) throw new HarnessError('EVIDENCE_DIR_NOT_EMPTY', `${target} exists and is not a directory`);
      if (readdirSync(target).length > 0) {
        if (!force) throw new HarnessError('EVIDENCE_DIR_NOT_EMPTY', `${target} holds evidence from a prior run; choose a new directory or pass --force-evidence to move it aside`);
        movedPrior = `${target}.prior-${stamp()}`;
        renameSync(target, movedPrior);
      }
    }
    const evidence = new Evidence(target);
    evidence.writeJson('evidence-marker.json', { schema: SCHEMA, createdAt: new Date().toISOString(), pid: process.pid, movedPrior });
    return evidence;
  }

  child(name) { return new Evidence(join(this.dir, safeName(name))); }

  path(rel) {
    const target = resolve(this.dir, rel);
    if (!isInside(target, this.dir)) throw new HarnessError('HARNESS_ERROR', `evidence path ${rel} escapes ${this.dir}`);
    return target;
  }

  writeText(rel, text) {
    const target = this.path(rel);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, redact(text));
  }

  appendText(rel, text) {
    const target = this.path(rel);
    mkdirSync(dirname(target), { recursive: true });
    appendFileSync(target, redact(text));
  }

  writeJson(rel, value) { this.writeText(rel, `${JSON.stringify(redactValue(value), null, 2)}\n`); }
  appendJsonl(rel, value) { this.appendText(rel, `${JSON.stringify(redactValue(value))}\n`); }
  log(message) { this.appendText('logs/runner.log', `${new Date().toISOString()} ${message}\n`); }
}

// ---------------------------------------------------------------------------
// Ownership ledger (append-only JSONL under <dataRoot>/.qa-unified-supervisor).
// Not redacted: exact argv is the identity evidence, and no secret is ever argv.

export class Ledger {
  constructor(dir) {
    this.dir = resolve(dir);
    this.file = join(this.dir, 'owned.jsonl');
  }

  static forDataRoot(dataRoot) { return new Ledger(join(dataRoot, '.qa-unified-supervisor')); }

  append(record) {
    mkdirSync(this.dir, { recursive: true });
    appendFileSync(this.file, `${JSON.stringify({ ...record, at: new Date().toISOString() })}\n`);
  }

  read() {
    if (!existsSync(this.file)) return { records: [], invalidLines: 0 };
    const records = [];
    let invalidLines = 0;
    for (const line of readFileSync(this.file, 'utf8').split('\n')) {
      if (!line.trim()) continue;
      try { records.push(JSON.parse(line)); } catch { invalidLines += 1; }
    }
    return { records, invalidLines };
  }

  // Open spawn/path records, with the start stamp (and the interpreter-resolved
  // argv observed at spawn) appended after spawn merged in.
  open() {
    const { records } = this.read();
    const closed = new Set(records.filter((r) => r.type === 'released' || r.type === 'path-released').map((r) => r.key));
    const stamps = new Map(records.filter((r) => r.type === 'stamp' && r.startStamp).map((r) => [r.key, r]));
    return records
      .filter((r) => (r.type === 'spawn' || r.type === 'path') && !closed.has(r.key))
      .map((r) => {
        const s = r.type === 'spawn' && !r.startStamp ? stamps.get(r.key) : null;
        if (!s) return r;
        return { ...r, startStamp: s.startStamp, observedArgv: r.observedArgv ?? s.observedArgv ?? null, observedCommand: r.observedCommand ?? s.observedCommand ?? null };
      });
  }
}

// ---------------------------------------------------------------------------
// Bounded capture for read-only probes (ps, PowerShell, git). Not ledgered: the
// child is killed through its own handle at the deadline. Detached on POSIX so a
// terminal or group signal aimed at the runner cannot kill a probe mid-read.

export function capture(file, args, { timeoutMs = 10_000, cwd, env, hash = false } = {}) {
  return new Promise((resolvePromise) => {
    let child;
    try {
      child = spawn(file, args, { cwd, env, detached: POSIX, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    } catch (error) {
      resolvePromise({ code: null, signal: null, stdout: '', stderr: '', timedOut: false, error: String(error?.message ?? error) });
      return;
    }
    const out = [];
    const err = [];
    const hasher = hash ? createHash('sha256') : null;
    let hashedBytes = 0;
    let size = 0;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolvePromise({
        stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8'),
        stdoutSha256: hasher ? hasher.digest('hex') : null, stdoutBytes: hasher ? hashedBytes : size, ...result,
      });
    };
    const timer = setTimeout(() => {
      try { child.kill('SIGKILL'); } catch { /* already gone */ }
      finish({ code: null, signal: 'SIGKILL', timedOut: true });
    }, timeoutMs);
    child.stdout?.on('data', (chunk) => {
      if (hasher) { hasher.update(chunk); hashedBytes += chunk.length; return; }
      if (size < OUTPUT_CAP_BYTES) { out.push(chunk); size += chunk.length; }
    });
    child.stderr?.on('data', (chunk) => { if (err.length < 64) err.push(chunk); });
    child.on('error', (error) => finish({ code: null, signal: null, timedOut: false, error: String(error?.message ?? error) }));
    child.on('close', (code, signal) => finish({ code, signal, timedOut: false }));
  });
}

// ---------------------------------------------------------------------------
// Process identity.

function linuxStat(pid) {
  const raw = readFileSync(`/proc/${pid}/stat`, 'utf8');
  const fields = raw.slice(raw.lastIndexOf(')') + 2).trim().split(/\s+/);
  // fields[0] is stat field 3 (state); field N is fields[N - 3].
  return { state: fields[0], ppid: Number(fields[1]), pgid: Number(fields[2]), startStamp: `linux:${fields[19]}` };
}

function lastJsonLine(text) {
  const lines = String(text).split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    try { return { ok: true, value: JSON.parse(lines[index]) }; } catch { /* keep looking */ }
  }
  return { ok: false, value: null };
}

const powershell = (script) => capture('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { timeoutMs: PROBE_TIMEOUT_MS });

// kill(pid, 0): ESRCH is authoritative "gone"; EPERM means the pid exists.
export function posixExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error?.code === 'ESRCH') return false;
    if (error?.code === 'EPERM') return true;
    return null;
  }
}

// Interpret `ps -o state= -o lstart= -p PID`. Only kill(0) can declare a pid
// gone; a ps that was signalled, timed out or failed while the pid still
// exists is 'unknown', never 'gone'.
export function interpretDarwinPs(result, existsAfter) {
  if (existsAfter === false) return { state: 'gone' };
  const line = String(result?.stdout ?? '').trim();
  if (!result?.timedOut && !result?.error && result?.code === 0 && line) {
    const match = /^(\S+)\s+(.+)$/.exec(line);
    if (!match) return { state: 'unknown', reason: `unparsed ps output: ${line}` };
    const startStamp = `darwin:${match[2].replace(/\s+/g, ' ')}`;
    return match[1].includes('Z') ? { state: 'zombie', startStamp } : { state: 'alive', startStamp };
  }
  return { state: 'unknown', reason: `ps ended code=${result?.code ?? null} signal=${result?.signal ?? null} timedOut=${Boolean(result?.timedOut)} while the pid ${existsAfter ? 'still exists' : 'state is unknown'}` };
}

// One process: state alive | zombie | gone | unknown, plus start stamp and argv.
export async function probeProcess(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return { state: 'gone', reason: 'invalid pid' };
  if (PLATFORM === 'linux') {
    let stat;
    try { stat = linuxStat(pid); } catch (error) {
      return error?.code === 'ENOENT' || error?.code === 'ESRCH' ? { state: 'gone' } : { state: 'unknown', reason: String(error?.message ?? error) };
    }
    if (stat.state === 'Z' || stat.state === 'X' || stat.state === 'x') return { state: 'zombie', startStamp: stat.startStamp };
    let argv = null;
    let executable = null;
    try {
      argv = readFileSync(`/proc/${pid}/cmdline`).toString('utf8').split('\0');
      if (argv.at(-1) === '') argv.pop();
    } catch { /* unreadable argv leaves identity to the start stamp */ }
    try { executable = readlinkSync(`/proc/${pid}/exe`); } catch { /* not ours to read */ }
    return { state: 'alive', startStamp: stat.startStamp, ppid: stat.ppid, pgid: stat.pgid, argv, executable };
  }
  if (PLATFORM === 'win32') {
    const result = await powershell([
      "$ErrorActionPreference = 'Stop';",
      `$p = Get-CimInstance Win32_Process -Filter "ProcessId=${pid}";`,
      "if ($null -eq $p) { 'null' } else { [ordered]@{ pid = [int]$p.ProcessId; ppid = [int]$p.ParentProcessId; exe = [string]$p.ExecutablePath; cmd = [string]$p.CommandLine; created = $p.CreationDate.ToUniversalTime().ToString('o') } | ConvertTo-Json -Compress }",
    ].join('\n'));
    if (result.timedOut || result.error || result.code !== 0) return { state: 'unknown', reason: result.error ?? `process probe ended code=${result.code}` };
    const parsed = lastJsonLine(result.stdout);
    if (!parsed.ok) return { state: 'unknown', reason: 'unparsed process probe output' };
    if (parsed.value === null) return { state: 'gone' };
    return { state: 'alive', startStamp: `win32:${parsed.value.created}`, ppid: parsed.value.ppid, command: parsed.value.cmd || null, executable: parsed.value.exe || null };
  }
  if (posixExists(pid) === false) return { state: 'gone' };
  const st = await capture('ps', ['-o', 'state=', '-o', 'lstart=', '-p', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
  const verdict = interpretDarwinPs(st, posixExists(pid));
  if (verdict.state !== 'alive') return verdict;
  const cmd = await capture('ps', ['-ww', '-o', 'command=', '-p', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
  return { ...verdict, command: cmd.code === 0 ? cmd.stdout.trim() || null : null };
}

// One process-table read: [{ pid, ppid, pgid, state, startStamp }] or null when unobservable.
export async function listProcesses() {
  if (PLATFORM === 'linux') {
    const rows = [];
    let entries;
    try { entries = readdirSync('/proc'); } catch { return null; }
    for (const name of entries) {
      if (!/^\d+$/.test(name)) continue;
      try {
        const stat = linuxStat(Number(name));
        rows.push({ pid: Number(name), ppid: stat.ppid, pgid: stat.pgid, state: stat.state === 'Z' ? 'zombie' : 'alive', startStamp: stat.startStamp });
      } catch { /* exited during the scan */ }
    }
    return rows.length > 0 ? rows : null;
  }
  if (PLATFORM === 'win32') {
    const result = await powershell([
      "$ErrorActionPreference = 'SilentlyContinue';",
      '$rows = @(Get-CimInstance Win32_Process | ForEach-Object { [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; created = [string]$(if ($_.CreationDate) { $_.CreationDate.ToUniversalTime().ToString(\'o\') }) } });',
      'ConvertTo-Json -InputObject $rows -Compress -Depth 3',
    ].join('\n'));
    if (result.timedOut || result.error || result.code !== 0) return null;
    const parsed = lastJsonLine(result.stdout);
    if (!parsed.ok) return null;
    const rows = (Array.isArray(parsed.value) ? parsed.value : [parsed.value]).filter(Boolean)
      .map((row) => ({ pid: Number(row.pid), ppid: Number(row.ppid), pgid: null, state: 'alive', startStamp: row.created ? `win32:${row.created}` : null }));
    return rows.length > 0 ? rows : null;
  }
  const result = await capture('ps', ['-A', '-o', 'pid=', '-o', 'ppid=', '-o', 'pgid=', '-o', 'state=', '-o', 'lstart='], { timeoutMs: PROBE_TIMEOUT_MS });
  if (result.timedOut || result.error || result.code !== 0) return null;
  const rows = [];
  for (const line of result.stdout.split('\n')) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(.+?)\s*$/.exec(line);
    if (!match) continue;
    rows.push({
      pid: Number(match[1]), ppid: Number(match[2]), pgid: Number(match[3]),
      state: match[4].includes('Z') ? 'zombie' : 'alive', startStamp: `darwin:${match[5].replace(/\s+/g, ' ')}`,
    });
  }
  return rows.length > 0 ? rows : null;
}

let linuxBootMs;
// Absolute start time (ms) of a start stamp, or null when it cannot be derived.
export function stampEpochMs(startStamp) {
  if (typeof startStamp !== 'string') return null;
  const index = startStamp.indexOf(':');
  if (index < 0) return null;
  const kind = startStamp.slice(0, index);
  const value = startStamp.slice(index + 1);
  if (kind === 'linux') {
    if (linuxBootMs === undefined) {
      try {
        const match = /^btime\s+(\d+)/m.exec(readFileSync('/proc/stat', 'utf8'));
        linuxBootMs = match ? Number(match[1]) * 1000 : null;
      } catch { linuxBootMs = null; }
    }
    const ticks = Number(value);
    // USER_HZ is 100 on every Linux ABI.
    return linuxBootMs === null || !Number.isFinite(ticks) ? null : linuxBootMs + ticks * 10;
  }
  if (kind === 'darwin' || kind === 'win32') {
    const ms = Date.parse(value);
    return Number.isFinite(ms) ? ms : null;
  }
  return null;
}

// Descendants of rootPid from one table read, plus (POSIX) members of its own
// process group; the root itself is excluded. Windows never updates a dead
// parent's ParentProcessId, so there a row only counts as a child when it was
// created at or after its parent (applied transitively).
export function familyOf(rows, rootPid, { platform = PLATFORM, rootStartStamp = null } = {}) {
  const posix = platform !== 'win32';
  const byParent = new Map();
  for (const row of rows) {
    if (!byParent.has(row.ppid)) byParent.set(row.ppid, []);
    byParent.get(row.ppid).push(row);
  }
  const rootRow = rows.find((row) => row.pid === rootPid);
  const epochs = new Map([[rootPid, stampEpochMs(rootRow?.startStamp ?? rootStartStamp)]]);
  const seen = new Set([rootPid]);
  const out = [];
  const queue = [rootPid];
  while (queue.length > 0) {
    const parent = queue.shift();
    for (const row of byParent.get(parent) ?? []) {
      if (seen.has(row.pid)) continue;
      if (!posix) {
        const parentEpoch = epochs.get(parent);
        const childEpoch = stampEpochMs(row.startStamp);
        if (parentEpoch === null || parentEpoch === undefined || childEpoch === null || childEpoch < parentEpoch) continue;
        epochs.set(row.pid, childEpoch);
      }
      seen.add(row.pid);
      out.push(row);
      queue.push(row.pid);
    }
  }
  if (posix) {
    for (const row of rows) {
      if (row.pgid === rootPid && !seen.has(row.pid)) { seen.add(row.pid); out.push(row); }
    }
  }
  return out;
}

function stem(path) { return basename(String(path)).replace(/\.(exe|cmd|bat)$/i, '').toLowerCase(); }

function launchVerdict(record, probe) {
  if (Array.isArray(probe.argv) && probe.argv.length > 0) {
    return stem(probe.argv[0]) === stem(record.executable) && isDeepStrictEqual(probe.argv.slice(1), record.args);
  }
  if (typeof probe.command === 'string' && probe.command) {
    if (PLATFORM === 'win32') {
      const command = probe.command.toLowerCase();
      const exeOk = !probe.executable || stem(probe.executable) === stem(record.executable);
      return exeOk && record.args.every((arg) => command.includes(String(arg).toLowerCase()));
    }
    const tail = record.args.join(' ');
    if (probe.command === [record.executable, ...record.args].join(' ')) return true;
    return stem(probe.command.split(' ')[0]) === stem(record.executable) && (tail === '' || probe.command.endsWith(` ${tail}`));
  }
  return null;
}

// true / false / null (unobservable). A launch that the kernel resolved through
// an interpreter (#! scripts show argv[0] = /bin/sh) matches the argv observed
// for this very process at spawn time.
function argvVerdict(record, probe) {
  const launched = launchVerdict(record, probe);
  if (launched !== false) return launched;
  if (Array.isArray(probe.argv) && Array.isArray(record.observedArgv)) return isDeepStrictEqual(probe.argv, record.observedArgv);
  if (typeof probe.command === 'string' && typeof record.observedCommand === 'string') return probe.command === record.observedCommand;
  return false;
}

// match: consistent with the record (see strength). mismatch: provably not the
// recorded process (unequal start stamps). argv-changed: argv differs from the
// launch; with equal stamps it is the same process after exec/retitle, without
// stamps it is undecided - never signalled, never released on argv alone.
// gone: exited (zombies count). unknown: unobservable.
// Signalling across runs additionally requires sameStartStamp() below.
export function identityVerdict(record, probe) {
  if (probe.state === 'gone' || probe.state === 'zombie') return { verdict: 'gone', reason: probe.state };
  if (probe.state !== 'alive') return { verdict: 'unknown', reason: probe.reason ?? 'process state unobservable' };
  const stamps = Boolean(record.startStamp && probe.startStamp);
  if (stamps && record.startStamp !== probe.startStamp) {
    return { verdict: 'mismatch', reason: `start stamp ${probe.startStamp} differs from recorded ${record.startStamp}` };
  }
  if (!Array.isArray(record.args)) {
    return stamps ? { verdict: 'match', strength: 'start-stamp' } : { verdict: 'unknown', reason: 'no start stamp and no argv recorded' };
  }
  const argv = argvVerdict(record, probe);
  if (argv === false) {
    return { verdict: 'argv-changed', reason: stamps ? 'equal start stamp but argv differs from the recorded launch (exec or retitle)' : 'argv differs from the recorded launch and no start stamp pins the process' };
  }
  if (argv === null && !stamps) return { verdict: 'unknown', reason: 'neither argv nor start stamp observable' };
  return { verdict: 'match', strength: stamps ? (argv ? 'start-stamp+argv' : 'start-stamp') : 'argv-only' };
}

export function sameStartStamp(record, probe) {
  return Boolean(record.startStamp && probe.startStamp && record.startStamp === probe.startStamp);
}

// A termination receipt is ok only when the leader exited, the process table
// was observable before signalling and (POSIX) again after exit, and every
// recorded descendant is gone.
export function settledOk(receipt) {
  return receipt.exited === true && receipt.descendantsObservable === true && receipt.postExitObservable === true
    && Array.isArray(receipt.descendants) && receipt.descendants.every((entry) => entry.finalState === 'gone');
}

// Cross-process absence check. There is no exit event for a process this runner
// did not spawn, so absence is re-probed with backoff inside a hard bound; the
// verdict is the observed state, never the elapsed time.
export async function awaitGone(record, ms) {
  const deadline = performance.now() + ms;
  let interval = 20;
  for (;;) {
    const { verdict } = identityVerdict(record, await probeProcess(record.pid));
    if (verdict === 'gone' || verdict === 'mismatch') return true;
    const left = deadline - performance.now();
    if (left <= 0) return false;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, Math.min(interval, left)));
    interval = Math.min(interval * 2, 250);
  }
}

async function killPid(pid, signal, { group = false } = {}) {
  if (!POSIX) {
    // Never 'taskkill /T': its tree walk trusts stale ParentProcessId values.
    // Descendants are reaped one by one from familyOf (creation-order lineage).
    const result = await capture('taskkill', [...(signal === 'SIGKILL' ? ['/F'] : []), '/PID', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
    return { signal, target: 'pid', code: result.code, error: result.error ?? null };
  }
  try {
    process.kill(group ? -pid : pid, signal);
    return { signal, target: group ? 'group' : 'pid', error: null };
  } catch (error) {
    return { signal, target: group ? 'group' : 'pid', error: error?.code ?? String(error?.message ?? error) };
  }
}

// SIGKILL one recorded descendant only while its start stamp still matches.
export async function reapMember(member) {
  const record = { pid: member.pid, startStamp: member.startStamp };
  const probe = await probeProcess(member.pid);
  const { verdict, reason } = identityVerdict(record, probe);
  const entry = { pid: member.pid, startStamp: member.startStamp, initial: verdict, reason: reason ?? null, signals: [], finalState: null };
  if (verdict === 'match' && sameStartStamp(record, probe)) {
    entry.signals.push(await killPid(member.pid, 'SIGKILL'));
    entry.finalState = (await awaitGone(record, MEMBER_GONE_MS)) ? 'gone' : 'alive';
  } else {
    entry.finalState = verdict === 'gone' || verdict === 'mismatch' ? 'gone' : 'unknown';
  }
  return entry;
}

function dedupeRows(rows) {
  const seen = new Set();
  return rows.filter((row) => (seen.has(row.pid) ? false : (seen.add(row.pid), true)));
}

// ---------------------------------------------------------------------------
// Event journal: one subscription per source, installed when the source is
// created; waits reconcile already-journaled events by cursor before waiting.

export class Journal {
  constructor(evidence) {
    this.evidence = evidence;
    this.events = [];
    this.waiters = new Set();
    this.seq = 0;
  }

  cursor() { return this.seq; }

  push(event) {
    const entry = { seq: ++this.seq, at: new Date().toISOString(), ...event };
    this.events.push(entry);
    this.evidence?.appendJsonl('events.jsonl', entry);
    for (const waiter of [...this.waiters]) {
      if (entry.seq <= waiter.after) continue;
      let matched = false;
      try { matched = waiter.predicate(entry); } catch (error) { waiter.reject(error); continue; }
      if (matched) waiter.resolve(entry);
    }
    return entry;
  }

  waitFor(label, predicate, { after = 0, deadlineMs, signal } = {}) {
    if (!Number.isFinite(deadlineMs) || deadlineMs <= 0) {
      return Promise.reject(new HarnessError('UNBOUNDED_WAIT', `${label} needs a positive deadlineMs`));
    }
    if (signal?.aborted) return Promise.reject(signal.reason);
    const found = this.events.find((event) => event.seq > after && predicate(event));
    if (found) return Promise.resolve(found);
    return new Promise((resolvePromise, rejectPromise) => {
      let timer = null;
      const waiter = { after, predicate };
      const settle = (fn, value) => {
        if (!this.waiters.has(waiter)) return;
        this.waiters.delete(waiter);
        clearTimeout(timer);
        signal?.removeEventListener('abort', onAbort);
        fn(value);
      };
      const onAbort = () => settle(rejectPromise, signal.reason);
      waiter.resolve = (value) => settle(resolvePromise, value);
      waiter.reject = (error) => settle(rejectPromise, error);
      this.waiters.add(waiter);
      timer = setTimeout(() => settle(rejectPromise, new HarnessError('DEADLINE_EXCEEDED', `${label} was not observed within ${deadlineMs}ms`, { after })), deadlineMs);
      signal?.addEventListener('abort', onAbort, { once: true });
    });
  }
}

// Watch first, then reconcile: the path may already exist when the watch arms.
export function waitForPath(target, { deadlineMs, signal, label = `path ${target}` } = {}) {
  if (!Number.isFinite(deadlineMs) || deadlineMs <= 0) return Promise.reject(new HarnessError('UNBOUNDED_WAIT', `${label} needs a positive deadlineMs`));
  if (signal?.aborted) return Promise.reject(signal.reason);
  return new Promise((resolvePromise, rejectPromise) => {
    let watcher = null;
    let timer = null;
    let settled = false;
    const finish = (fn, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try { watcher?.close(); } catch { /* already closed */ }
      signal?.removeEventListener('abort', onAbort);
      fn(value);
    };
    const onAbort = () => finish(rejectPromise, signal.reason);
    const check = () => { if (existsSync(target)) finish(resolvePromise, target); };
    try {
      watcher = watch(dirname(target), check);
      watcher.on?.('error', (error) => finish(rejectPromise, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`)));
    } catch (error) {
      finish(rejectPromise, new HarnessError('WATCH_FAILED', `${label}: ${error?.message ?? error}`));
      return;
    }
    timer = setTimeout(() => finish(rejectPromise, new HarnessError('DEADLINE_EXCEEDED', `${label} did not appear within ${deadlineMs}ms`)), deadlineMs);
    signal?.addEventListener('abort', onAbort, { once: true });
    check();
  });
}

// ---------------------------------------------------------------------------
// Owned processes.

export class Owned {
  constructor({ runId, ledger, evidence, journal, runnerStamp = null }) {
    this.runId = runId;
    this.ledger = ledger;
    this.evidence = evidence;
    this.journal = journal;
    this.runnerStamp = runnerStamp;
    this.handles = [];
    this.pending = new Set();
    this.closed = false;
    this.counter = 0;
  }

  async spawn(id, executable, args, options = {}) {
    if (this.closed) throw new HarnessError('OWNERSHIP_CLOSED', `refusing to start ${id} after teardown began`);
    const work = this.#spawn(id, executable, args, options);
    this.pending.add(work);
    try { return await work; } finally { this.pending.delete(work); }
  }

  // `env` is mandatory: callers choose productEnv() or an explicit inheritance.
  async #spawn(id, executable, args, { cwd, env, graceMs = 3_000, stdin = 'ignore', onLine } = {}) {
    if (!isAbsolute(executable)) throw new HarnessError('SPAWN_FAILED', `${id}: executable must be absolute, got ${executable}`);
    if (!env || typeof env !== 'object') throw new HarnessError('SPAWN_FAILED', `${id}: an explicit env is required`);
    const key = `${this.runId}/proc/${safeName(id)}#${++this.counter}`;
    const spawnedAtMs = Date.now();
    let child;
    try {
      child = spawn(executable, args, { cwd, env, detached: POSIX, stdio: [stdin, 'pipe', 'pipe'], windowsHide: true });
    } catch (error) {
      throw new HarnessError('SPAWN_FAILED', `${id}: ${error?.message ?? error}`);
    }
    const handle = { id, key, child, pid: child.pid, graceMs, exitInfo: null, record: null, termination: null };
    const errored = new Promise((resolveError) => {
      child.on('error', (error) => {
        this.journal.push({ source: id, kind: 'error', message: String(error?.message ?? error) });
        resolveError(error);
      });
    });
    handle.exited = new Promise((resolveExit) => {
      child.on('exit', (code, signal) => {
        handle.exitInfo = { code, signal, at: new Date().toISOString() };
        this.journal.push({ source: id, kind: 'exit', code, signal, pid: handle.pid });
        resolveExit(handle.exitInfo);
      });
    });
    handle.closed = new Promise((resolveClose) => child.on('close', () => resolveClose(true)));
    if (!Number.isSafeInteger(child.pid)) {
      const error = await withDeadline(errored, 5_000, () => null);
      throw new HarnessError('SPAWN_FAILED', `${id}: ${error?.message ?? 'no pid was assigned'}`);
    }
    // Owned from this instant: teardown sees the handle even while the probe below runs.
    handle.record = {
      type: 'spawn', key, runId: this.runId, id, pid: child.pid, executable, args: [...args], cwd: cwd ?? process.cwd(),
      detached: POSIX, graceMs, spawnedAtMs, startStamp: null, runnerPid: process.pid, runnerStartStamp: this.runnerStamp,
    };
    this.handles.push(handle);
    this.ledger.append(handle.record);
    this.evidence.appendJsonl('owned.jsonl', handle.record);
    this.#pipe(handle, child.stdout, 'stdout', onLine);
    this.#pipe(handle, child.stderr, 'stderr', onLine);
    const probe = await probeProcess(child.pid);
    if (probe.startStamp) {
      handle.record.startStamp = probe.startStamp;
      const stampRecord = { type: 'stamp', key, startStamp: probe.startStamp };
      // Our own unreaped child: whatever argv the kernel shows now (e.g. the #!
      // interpreter first) is this launch, so --cleanup can recognise it later.
      if (launchVerdict(handle.record, probe) === false) {
        if (Array.isArray(probe.argv)) stampRecord.observedArgv = probe.argv;
        else if (typeof probe.command === 'string') stampRecord.observedCommand = probe.command;
        handle.record.observedArgv = stampRecord.observedArgv ?? null;
        handle.record.observedCommand = stampRecord.observedCommand ?? null;
      }
      this.ledger.append(stampRecord);
      this.evidence.appendJsonl('owned.jsonl', stampRecord);
    }
    this.journal.push({ source: id, kind: 'spawned', pid: child.pid, executable, args });
    return handle;
  }

  // Lines are redacted before truncation (a cut can never expose half a
  // secret); an over-long line is truncated and the rest dropped up to the
  // next newline. onLine receives the raw line for in-memory use only.
  #pipe(handle, stream, kind, onLine) {
    if (!stream) return;
    const logRel = `logs/${safeName(handle.id)}.${kind}.log`;
    let buffer = '';
    let dropping = false;
    let written = 0;
    const emit = (raw) => {
      const safe = redact(raw);
      const line = safe.length > LINE_CAP ? `${safe.slice(0, LINE_CAP)}[line truncated]` : safe;
      if (written < LOG_CAP_BYTES) {
        this.evidence.appendText(logRel, `${line}\n`);
        written += line.length + 1;
        if (written >= LOG_CAP_BYTES) this.evidence.appendText(logRel, '[log capped]\n');
      }
      let json = null;
      const trimmed = raw.trim();
      if (trimmed.startsWith('{') && trimmed.length <= LINE_CAP) { try { json = JSON.parse(trimmed); } catch { /* plain text */ } }
      this.journal.push({ source: handle.id, kind, line, json, pid: handle.pid });
      onLine?.(kind, raw, json);
    };
    stream.setEncoding('utf8');
    stream.on('data', (chunk) => {
      buffer += chunk;
      let index;
      while ((index = buffer.indexOf('\n')) >= 0) {
        const piece = buffer.slice(0, index).replace(/\r$/, '');
        buffer = buffer.slice(index + 1);
        if (dropping) { dropping = false; continue; }
        emit(piece);
      }
      if (!dropping && buffer.length > LINE_CAP * 4) { emit(buffer); buffer = ''; dropping = true; }
      if (dropping) buffer = '';
    });
    stream.on('end', () => { if (buffer && !dropping) emit(buffer); buffer = ''; });
    stream.on('error', () => { /* the exit event decides */ });
  }

  // Bounded command: kills the owned process at the deadline (hung commands)
  // and returns its termination receipt instead of waiting forever.
  async run(id, executable, args, { deadlineMs, input, signal, ...options } = {}) {
    if (!Number.isFinite(deadlineMs) || deadlineMs <= 0) throw new HarnessError('UNBOUNDED_WAIT', `${id} needs a positive deadlineMs`);
    if (signal?.aborted) throw signal.reason;
    const started = performance.now();
    const stdout = [];
    const stderr = [];
    let size = 0;
    const onLine = (kind, line) => {
      if (size >= OUTPUT_CAP_BYTES) return;
      (kind === 'stdout' ? stdout : stderr).push(line);
      size += line.length + 1;
    };
    const handle = await this.spawn(id, executable, args, { ...options, onLine, stdin: input === undefined ? 'ignore' : 'pipe' });
    if (input !== undefined) {
      handle.child.stdin.on('error', () => { /* the exit event decides */ });
      handle.child.stdin.end(input);
    }
    let timer = null;
    let onAbort = null;
    const outcome = await new Promise((resolveOutcome) => {
      timer = setTimeout(() => resolveOutcome({ timedOut: true }), deadlineMs);
      onAbort = () => resolveOutcome({ aborted: signal.reason });
      signal?.addEventListener('abort', onAbort, { once: true });
      handle.exited.then((exit) => resolveOutcome({ exit }));
    });
    clearTimeout(timer);
    signal?.removeEventListener('abort', onAbort);
    let receipt = null;
    if (!outcome.exit) receipt = await this.terminate(handle, outcome.timedOut ? 'deadline' : 'aborted');
    // 'exit' can precede the final stdio chunk; drain within a bound.
    await withDeadline(handle.closed, 2_000, () => false);
    if (outcome.aborted) throw outcome.aborted;
    return {
      pid: handle.pid, code: handle.exitInfo?.code ?? null, signal: handle.exitInfo?.signal ?? null, timedOut: Boolean(outcome.timedOut),
      stdout: stdout.join('\n'), stderr: stderr.join('\n'), durationMs: Math.round(performance.now() - started), receipt,
    };
  }

  terminate(handle, reason) {
    handle.termination ??= this.#terminate(handle, reason);
    return handle.termination;
  }

  async #exitWithin(handle, ms) {
    if (handle.exitInfo) return true;
    return withDeadline(handle.exited.then(() => true), ms, () => false);
  }

  async #signal(handle, signal, receipt) {
    const at = new Date().toISOString();
    const grouped = await killPid(handle.pid, signal, { group: true });
    receipt.signals.push({ ...grouped, at });
    if (!POSIX || !grouped.error) return;
    try {
      handle.child.kill(signal);
      receipt.signals.push({ signal, target: 'leader', error: null, at });
    } catch (error) {
      receipt.signals.push({ signal, target: 'leader', error: String(error?.message ?? error), at });
    }
  }

  async #terminate(handle, reason) {
    const receipt = {
      key: handle.key, id: handle.id, pid: handle.pid, executable: handle.record?.executable ?? null, args: handle.record?.args ?? null,
      startStamp: handle.record?.startStamp ?? null, reason, startedAt: new Date().toISOString(), identity: null, probe: null,
      signals: [], alreadyExited: false, exited: false, exitCode: null, exitSignal: null,
      descendantsObservable: false, postExitObservable: false, descendants: [], ok: false,
    };
    const lineage = { rootStartStamp: handle.record?.startStamp ?? null };
    const before = await listProcesses();
    receipt.descendantsObservable = before !== null;
    let members = [];
    if (handle.exitInfo) {
      receipt.alreadyExited = true;
      receipt.identity = 'exited-before-teardown';
      // A row now holding the leader's pid is a reused pid: its group is not ours.
      if (before && !before.some((row) => row.pid === handle.pid)) members = familyOf(before, handle.pid, lineage);
    } else {
      // An unreaped child cannot have had its pid reused: the handle is the identity proof.
      receipt.identity = 'child-handle';
      const probe = await probeProcess(handle.pid);
      receipt.probe = { state: probe.state, ...identityVerdict(handle.record ?? {}, probe) };
      members = before ? familyOf(before, handle.pid, lineage) : [];
      await this.#signal(handle, 'SIGTERM', receipt);
      if (!(await this.#exitWithin(handle, handle.graceMs))) {
        await this.#signal(handle, 'SIGKILL', receipt);
        await this.#exitWithin(handle, FORCE_EXIT_MS);
      }
    }
    receipt.exited = handle.exitInfo !== null;
    receipt.exitCode = handle.exitInfo?.code ?? null;
    receipt.exitSignal = handle.exitInfo?.signal ?? null;
    if (!POSIX) {
      receipt.postExitObservable = receipt.descendantsObservable;
    } else if (receipt.exited) {
      const after = await listProcesses();
      receipt.postExitObservable = after !== null;
      if (after && !after.some((row) => row.pid === handle.pid)) {
        members = dedupeRows([...members, ...after.filter((row) => row.pgid === handle.pid)]);
      }
    }
    for (const member of members) receipt.descendants.push(await reapMember(member));
    receipt.ok = settledOk(receipt);
    receipt.finishedAt = new Date().toISOString();
    if (receipt.ok) this.ledger.append({ type: 'released', key: handle.key, reason, exited: receipt.exited });
    return receipt;
  }

  async teardown(reason) {
    this.closed = true;
    // Handles are registered right after spawn(), so a spawn still probing is torn down too.
    await withDeadline(Promise.allSettled([...this.pending]), 2 * PROBE_TIMEOUT_MS + 5_000, () => null);
    const receipts = [];
    for (const handle of [...this.handles].reverse()) {
      try {
        receipts.push(await this.terminate(handle, reason));
      } catch (error) {
        receipts.push({ key: handle.key, id: handle.id, pid: handle.pid, ok: false, error: errorInfo(error) });
      }
    }
    if (this.pending.size > 0) receipts.push({ key: `${this.runId}/pending`, ok: false, error: { code: 'SPAWN_STILL_PENDING', message: `${this.pending.size} spawn(s) did not settle` } });
    return receipts.reverse();
  }
}

// Cross-run reclaim for --cleanup: never by name or port, and never without
// equal recorded and observed OS start stamps.
export async function reclaimProcess(record, { ledger, forceOwner = false }) {
  const entry = {
    kind: 'process', key: record.key, runId: record.runId, id: record.id, pid: record.pid, executable: record.executable, args: record.args,
    startStamp: record.startStamp ?? null, status: null, identity: null, killed: false, signals: [],
    descendantsObservable: false, postExitObservable: false, descendants: [], exited: false, ok: false,
  };
  if (record.pid === process.pid || record.pid === process.ppid) {
    entry.status = 'refused-self';
    return entry;
  }
  if (!forceOwner && record.runnerPid) {
    const runner = identityVerdict({ pid: record.runnerPid, startStamp: record.runnerStartStamp ?? null }, await probeProcess(record.runnerPid));
    if (runner.verdict === 'match' || runner.verdict === 'unknown') {
      entry.status = runner.verdict === 'match' ? 'owner-runner-alive' : 'owner-runner-unverifiable';
      entry.detail = 'the run that owns this process may still be active and tears down its own resources; pass --force-owner to override';
      return entry;
    }
  }
  const probe = await probeProcess(record.pid);
  entry.identity = identityVerdict(record, probe);
  if (entry.identity.verdict === 'gone' || entry.identity.verdict === 'mismatch') {
    entry.status = entry.identity.verdict === 'gone' ? 'already-gone' : 'identity-mismatch';
    entry.ok = true;
    ledger.append({ type: 'released', key: record.key, reason: `cleanup:${entry.status}` });
    return entry;
  }
  if (entry.identity.verdict === 'unknown') {
    entry.status = 'unverifiable';
    return entry;
  }
  if (!sameStartStamp(record, probe)) {
    // No recorded stamp: a live process that provably started at another time
    // is a reused pid (released, untouched); otherwise it stays unverifiable.
    const started = stampEpochMs(probe.startStamp);
    if (started !== null && Number.isFinite(record.spawnedAtMs) && Math.abs(started - record.spawnedAtMs) > LINEAGE_SLACK_MS) {
      entry.status = 'identity-mismatch';
      entry.reason = `no recorded start stamp; the live process started ${Math.round((started - record.spawnedAtMs) / 1000)}s from the recorded spawn time`;
      entry.ok = true;
      ledger.append({ type: 'released', key: record.key, reason: 'cleanup:identity-mismatch' });
      return entry;
    }
    entry.status = 'unverifiable';
    entry.reason = 'start stamps are not both present and equal; a process is never signalled on argv alone';
    return entry;
  }
  if (entry.identity.verdict === 'argv-changed') {
    // Same pid and start time, different argv: the owned process after exec or
    // a retitle. It may still be ours, so it is neither signalled nor released.
    entry.status = 'argv-changed';
    entry.reason = 'equal start stamps but argv differs from the recorded launch; not signalled and left open (inspect and stop it by hand)';
    return entry;
  }
  const rows = await listProcesses();
  entry.descendantsObservable = rows !== null;
  let members = rows ? familyOf(rows, record.pid, { rootStartStamp: record.startStamp }) : [];
  const group = Boolean(record.detached) || !POSIX;
  entry.killed = true;
  entry.signals.push(await killPid(record.pid, 'SIGTERM', { group }));
  let gone = await awaitGone(record, record.graceMs ?? 3_000);
  if (!gone) {
    entry.signals.push(await killPid(record.pid, 'SIGKILL', { group }));
    gone = await awaitGone(record, FORCE_EXIT_MS);
  }
  entry.exited = gone;
  if (!POSIX) {
    entry.postExitObservable = entry.descendantsObservable;
  } else if (gone) {
    const after = await listProcesses();
    entry.postExitObservable = after !== null;
    if (after && !after.some((row) => row.pid === record.pid)) members = dedupeRows([...members, ...after.filter((row) => row.pgid === record.pid)]);
  }
  for (const member of members) entry.descendants.push(await reapMember(member));
  entry.status = gone ? 'terminated' : 'kill-failed';
  entry.ok = settledOk(entry);
  if (entry.ok) ledger.append({ type: 'released', key: record.key, reason: 'cleanup:terminated' });
  return entry;
}

// Before a run: refuse when an earlier, no-longer-running runner left owned
// processes that may still be alive; they would pollute this run's census.
export async function preflightOwnedState(configInfo) {
  const ledger = Ledger.forDataRoot(configInfo.config.dataRoot);
  const stale = [];
  for (const record of ledger.open().filter((r) => r.type === 'spawn')) {
    if (record.runnerPid) {
      const runner = identityVerdict({ pid: record.runnerPid, startStamp: record.runnerStartStamp ?? null }, await probeProcess(record.runnerPid));
      if (runner.verdict === 'match') continue;
    }
    const { verdict } = identityVerdict(record, await probeProcess(record.pid));
    if (verdict === 'match' || verdict === 'unknown' || verdict === 'argv-changed') stale.push({ key: record.key, pid: record.pid, verdict });
  }
  if (stale.length > 0) {
    throw new HarnessError('STALE_OWNED_STATE', `${stale.length} owned process(es) from an earlier run may still be alive; run --cleanup with this config first`, { stale });
  }
}

// ---------------------------------------------------------------------------
// Exclusive run lock (<dataRoot>/.qa-unified-supervisor/run.lock). A lock whose
// holder is provably gone (exited, or its pid now has another start stamp) is
// moved aside and taken over; a live or unverifiable holder refuses the run.

export async function acquireRunLock(dataRoot, { runnerStamp = null, mode = null } = {}) {
  const dir = join(dataRoot, '.qa-unified-supervisor');
  mkdirSync(dir, { recursive: true });
  const file = join(dir, 'run.lock');
  const mine = { pid: process.pid, startStamp: runnerStamp, mode, at: new Date().toISOString(), nonce: Math.random().toString(36).slice(2) };
  const takenOver = [];
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      writeFileSync(file, `${JSON.stringify(mine)}\n`, { flag: 'wx' });
      return { file, mine, takenOver };
    } catch (error) {
      if (error?.code !== 'EEXIST') throw new HarnessError('HARNESS_ERROR', `cannot create run lock ${file}: ${error?.message ?? error}`);
    }
    let raw = null;
    try { raw = readFileSync(file, 'utf8'); } catch (error) { if (error?.code === 'ENOENT') continue; }
    let holder = null;
    try { holder = JSON.parse(raw); } catch { /* refused below */ }
    if (!holder || !Number.isSafeInteger(holder.pid)) {
      throw new HarnessError('RUN_LOCKED', `run lock ${file} is unreadable; if no run is active on this dataRoot, delete it by hand`, { raw });
    }
    const { verdict } = identityVerdict({ pid: holder.pid, startStamp: holder.startStamp ?? null }, await probeProcess(holder.pid));
    if (verdict !== 'gone' && verdict !== 'mismatch') {
      throw new HarnessError('RUN_LOCKED', `another run (pid ${holder.pid}, ${holder.mode ?? 'unknown mode'}, since ${holder.at ?? '?'}) holds ${file}; runs on one dataRoot are exclusive`, { holder, verdict });
    }
    const aside = `${file}.stale-${process.pid}-${attempt}`;
    try { renameSync(file, aside); } catch (error) { if (error?.code === 'ENOENT') continue; throw error; }
    let movedRaw = null;
    try { movedRaw = readFileSync(aside, 'utf8'); } catch { /* checked below */ }
    if (movedRaw !== raw) {
      throw new HarnessError('RUN_LOCKED', `run lock ${file} changed while a stale lock was being taken over; retry`, { aside });
    }
    takenOver.push({ holder, verdict, movedTo: aside });
  }
  throw new HarnessError('RUN_LOCKED', `could not take run lock ${file}`);
}

export function releaseRunLock(lock) {
  if (!lock) return false;
  try {
    const current = JSON.parse(readFileSync(lock.file, 'utf8'));
    if (current.pid !== lock.mine.pid || current.nonce !== lock.mine.nonce) return false;
    unlinkSync(lock.file);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Owned run roots (<dataRoot>/runs/<runId>, <memoryRoot>/runs/<runId>).

export function claimRunRoot(ledger, runId, label, base) {
  const record = { type: 'path', key: `${runId}/path/${label}`, runId, label, base, path: join(base, 'runs', runId) };
  ledger.append(record);
  mkdirSync(record.path, { recursive: true });
  return record;
}

function preserveLogs(root, evidence, rel) {
  const preserved = [];
  const walk = (dir, prefix, depth) => {
    if (depth > 8 || preserved.length >= 200) return;
    let entries;
    try { entries = readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const full = join(dir, entry.name);
      const relPath = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(full, relPath, depth + 1);
      else if (entry.isFile() && /\.(log|jsonl)$/.test(entry.name)) {
        try {
          if (statSync(full).size > 8 * 1024 * 1024) continue;
          evidence.writeText(`${rel}/${relPath}`, readFileSync(full, 'utf8'));
          preserved.push(relPath);
        } catch { /* best-effort copy; removal still proceeds */ }
      }
    }
  };
  walk(root, '', 0);
  return preserved;
}

// Removes only <configured base>/runs/<well-formed runId> exactly as recorded.
export function releaseRunRoot(ledger, record, evidence, allowedBases) {
  const entry = { kind: 'path', key: record.key, label: record.label ?? null, path: record.path, status: null, preserved: [], removed: false, ok: false };
  if (!allowedBases.some((base) => typeof record.base === 'string' && norm(base) === norm(record.base))) {
    entry.status = 'refused-outside-configured-roots';
    return entry;
  }
  const runsDir = join(record.base, 'runs');
  if (!RUN_ID_PATTERN.test(String(record.runId)) || typeof record.path !== 'string' || norm(record.path) !== norm(join(runsDir, record.runId))) {
    entry.status = 'refused-unexpected-run-path';
    return entry;
  }
  try { assertNotProduction(record.path, 'run root'); } catch (error) {
    entry.status = 'refused-production';
    entry.error = errorInfo(error);
    return entry;
  }
  try {
    if (existsSync(record.path)) {
      entry.preserved = preserveLogs(record.path, evidence, `logs/run-roots/${safeName(record.label ?? 'root')}`);
      rmSync(record.path, { recursive: true, force: true });
    }
  } catch (error) {
    entry.error = errorInfo(error);
  }
  entry.removed = !existsSync(record.path);
  entry.ok = entry.removed;
  entry.status = entry.removed ? 'removed' : 'remove-failed';
  if (entry.ok) ledger.append({ type: 'path-released', key: record.key });
  return entry;
}

// ---------------------------------------------------------------------------
// Product environment: an allowlist plus isolated Ferryx locations. Inherited
// FERRYX_*, SLACK_*, XDG_* and provider variables never reach product binaries;
// HOME and TMPDIR point inside the run root so no default ~/.ferryx path or
// shared $TMPDIR/ferryx endpoint is reachable.

const ENV_ALLOW = ['PATH', 'USER', 'LOGNAME', 'SHELL', 'LANG', 'LC_ALL', 'LC_CTYPE', 'TERM', 'TZ',
  'SystemRoot', 'SYSTEMROOT', 'windir', 'WINDIR', 'ComSpec', 'COMSPEC', 'PATHEXT', 'ProgramData', 'ProgramFiles', 'ProgramFiles(x86)'];

export function productEnv({ dataRun, extra = {} }) {
  const env = {};
  for (const key of ENV_ALLOW) if (process.env[key] !== undefined) env[key] = process.env[key];
  const home = join(dataRun, 'home');
  const tmp = join(dataRun, 'tmp');
  mkdirSync(home, { recursive: true });
  mkdirSync(tmp, { recursive: true });
  Object.assign(env, {
    HOME: home,
    TMPDIR: tmp,
    FERRYX_DATA_DIR: join(dataRun, 'data'),
    FERRYX_RUNTIME_DIR: join(dataRun, 'runtime'),
    FERRYX_HUB_DB: join(dataRun, 'hub', 'hub.sqlite'),
    FERRYX_HUB: 'on',
    FERRYX_HUB_QUIET_HOURS: 'off',
  });
  if (!POSIX) Object.assign(env, { USERPROFILE: home, TEMP: tmp, TMP: tmp, APPDATA: join(home, 'AppData', 'Roaming'), LOCALAPPDATA: join(home, 'AppData', 'Local') });
  return { ...env, ...extra };
}

// ---------------------------------------------------------------------------
// Receipts: source revision (including dirty content), binary and config
// hashes, the repositories that contain the binaries, host and runtime.

export function hashFile(path) {
  return new Promise((resolvePromise, rejectPromise) => {
    const hash = createHash('sha256');
    const stream = createReadStream(path);
    stream.on('error', rejectPromise);
    stream.on('data', (chunk) => hash.update(chunk));
    stream.on('end', () => resolvePromise(hash.digest('hex')));
  });
}

async function hashTree(root) {
  const hash = createHash('sha256');
  let files = 0;
  let bytes = 0;
  const walk = async (dir, prefix) => {
    const entries = readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      const full = join(dir, entry.name);
      const relPath = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isSymbolicLink()) hash.update(`L ${relPath} -> ${readlinkSync(full)}\n`);
      else if (entry.isDirectory()) { hash.update(`D ${relPath}\n`); await walk(full, relPath); }
      else if (entry.isFile()) {
        const size = statSync(full).size;
        files += 1;
        bytes += size;
        hash.update(`F ${relPath} ${size} ${await hashFile(full)}\n`);
      }
    }
  };
  await walk(root, '');
  return { sha256: hash.digest('hex'), files, bytes };
}

const artifactCache = new Map();

export async function describeArtifact(path) {
  try {
    const real = realpathSync(path);
    const info = statSync(real);
    const cacheKey = `${real}\0${info.size}\0${info.mtimeMs}`;
    if (!artifactCache.has(cacheKey)) {
      artifactCache.set(cacheKey, info.isDirectory()
        ? hashTree(real).then((tree) => ({ kind: 'directory', ...tree }))
        : hashFile(real).then((digest) => ({ kind: 'file', sha256: digest, bytes: info.size })));
    }
    return { path, realpath: real, mtime: info.mtime.toISOString(), ...(await artifactCache.get(cacheKey)) };
  } catch (error) {
    return { path, error: String(error?.message ?? error) };
  }
}

const UNTRACKED_LIMIT = 5_000;

export async function gitReceipt(dir, { full = true } = {}) {
  const top = await capture('git', ['-C', dir, 'rev-parse', '--show-toplevel'], { timeoutMs: PROBE_TIMEOUT_MS });
  if (top.code !== 0) return { dir, toplevel: null, rev: null, error: (top.error ?? top.stderr.trim()) || 'not inside a git work tree' };
  const toplevel = top.stdout.trim();
  const rev = await capture('git', ['-C', toplevel, 'rev-parse', 'HEAD'], { timeoutMs: PROBE_TIMEOUT_MS });
  const receipt = { dir, toplevel, rev: rev.code === 0 ? rev.stdout.trim() : null };
  if (!full) return receipt;
  const branch = await capture('git', ['-C', toplevel, 'rev-parse', '--abbrev-ref', 'HEAD'], { timeoutMs: PROBE_TIMEOUT_MS });
  const status = await capture('git', ['-C', toplevel, 'status', '--porcelain=v1'], { timeoutMs: PROBE_TIMEOUT_MS });
  const dirty = status.code === 0 ? status.stdout.split('\n').filter(Boolean) : null;
  receipt.branch = branch.code === 0 ? branch.stdout.trim() : null;
  receipt.dirtyEntries = dirty ? dirty.length : null;
  receipt.statusSha256 = dirty ? sha256(status.stdout) : null;
  // Content of the dirty tree, not just its path list.
  const diff = await capture('git', ['-C', toplevel, 'diff', '--binary', 'HEAD'], { timeoutMs: 60_000, hash: true });
  receipt.diffSha256 = diff.code === 0 && !diff.timedOut ? diff.stdoutSha256 : null;
  receipt.diffBytes = diff.code === 0 ? diff.stdoutBytes : null;
  const untracked = await capture('git', ['-C', toplevel, 'ls-files', '--others', '--exclude-standard', '-z'], { timeoutMs: PROBE_TIMEOUT_MS });
  if (untracked.code === 0) {
    const names = untracked.stdout.split('\0').filter(Boolean).sort();
    const hash = createHash('sha256');
    for (const name of names.slice(0, UNTRACKED_LIMIT)) {
      let digest;
      try { digest = await hashFile(join(toplevel, name)); } catch (error) { digest = `unreadable:${error?.code ?? 'error'}`; }
      hash.update(`${name}\0${digest}\n`);
    }
    receipt.untrackedCount = names.length;
    receipt.untrackedTruncated = names.length > UNTRACKED_LIMIT;
    receipt.untrackedSha256 = hash.digest('hex');
  } else {
    receipt.untrackedSha256 = null;
  }
  return receipt;
}

// Repository containing a binary (or app bundle), when it lives inside a work tree.
async function containingRepo(path) {
  try {
    const real = realpathSync(path);
    const dir = statSync(real).isDirectory() ? real : dirname(real);
    return await gitReceipt(dir, { full: false });
  } catch (error) {
    return { dir: path, toplevel: null, rev: null, error: String(error?.message ?? error) };
  }
}

export async function collectReceipts({ repoRoot, configInfo = null, scripts = [] }) {
  const config = configInfo?.config ?? null;
  const scriptHashes = {};
  for (const file of scripts) {
    try { scriptHashes[relative(repoRoot, file).split(sep).join('/')] = await hashFile(file); } catch (error) { scriptHashes[file] = `error: ${error?.message ?? error}`; }
  }
  return {
    schema: SCHEMA,
    createdAt: new Date().toISOString(),
    host: { hostname: hostname(), platform: PLATFORM, release: release(), arch: arch() },
    runner: { bun: globalThis.Bun?.version ?? process.versions?.bun ?? null, execPath: process.execPath, exec: await describeArtifact(process.execPath), scripts: scriptHashes },
    git: await gitReceipt(repoRoot),
    project: config ? { root: config.projectRoot, exists: existsSync(config.projectRoot), git: existsSync(config.projectRoot) ? await gitReceipt(config.projectRoot, { full: false }) : null } : null,
    binaries: config ? {
      ferryxCli: await describeArtifact(config.ferryxCli),
      ferryxApp: await describeArtifact(config.ferryxApp),
      mhcBin: await describeArtifact(config.mhcBin),
    } : null,
    binaryRepos: config ? {
      ferryxCli: await containingRepo(config.ferryxCli),
      ferryxApp: await containingRepo(config.ferryxApp),
      mhcBin: await containingRepo(config.mhcBin),
    } : null,
    config: configInfo ? { path: configInfo.path, sha256: configInfo.sha256, bytes: configInfo.bytes } : null,
  };
}

// ---------------------------------------------------------------------------
// One scenario run: evidence, deadline, frozen assertions, guaranteed teardown.

function makeContext({ name, runId, configInfo, evidence, journal, owned, signal, assertions, state, dataRoot, memoryRoot }) {
  const config = configInfo.config;
  let requestSeq = 0;
  const ctx = {
    name, runId, config, signal, journal, evidence,
    paths: Object.freeze({ data: dataRoot.path, memory: memoryRoot.path, project: config.projectRoot, evidence: evidence.dir }),
    log: (message) => evidence.log(`[${name}] ${message}`),
    announce: (value) => {
      const text = redact(typeof value === 'string' ? value : JSON.stringify(redactValue(value)));
      process.stdout.write(`${text}\n`);
      evidence.log(`[announce] ${text}`);
    },
    // Frozen once the scenario settles: a late assertion is recorded, throws and fails the run.
    assert: (assertion, ok, detail = null) => {
      const entry = { name: assertion, ok: ok === true, detail };
      if (state.frozen) {
        state.late.push(entry);
        evidence.log(`late assert ${assertion} after the scenario settled`);
        throw new HarnessError('LATE_ASSERTION', `assertion ${assertion} was recorded after the scenario settled`);
      }
      assertions.push(entry);
      evidence.log(`assert ${assertion} ${entry.ok ? 'ok' : 'FAILED'}`);
      return entry.ok;
    },
    productEnv: (extra = {}) => productEnv({ dataRun: dataRoot.path, extra }),
    // Default env is productEnv(); pass `env: process.env` only to opt in to inheritance.
    spawn: (id, executable, args, options = {}) => owned.spawn(id, executable, args, { ...options, env: options.env ?? ctx.productEnv() }),
    run: (id, executable, args, options = {}) => owned.run(id, executable, args, { signal, ...options, env: options.env ?? ctx.productEnv() }),
    terminate: (handle, reason = 'scenario') => owned.terminate(handle, reason),
    waitFor: (label, predicate, options = {}) => journal.waitFor(label, predicate, { signal, ...options }),
    waitForPath: (target, options = {}) => waitForPath(target, { signal, ...options }),
    // Production CLI surface; every call lands in requests.jsonl / responses.jsonl.
    cli: async (args, { deadlineMs = 30_000, input, label = args[0] ?? 'cli', env } = {}) => {
      const seq = ++requestSeq;
      evidence.appendJsonl('requests.jsonl', { seq, label, surface: 'ferryx-cli', executable: config.ferryxCli, args, input: input ?? null, at: new Date().toISOString() });
      const result = await owned.run(`cli-${label}`, config.ferryxCli, args, { signal, deadlineMs, input, env: env ?? ctx.productEnv(), cwd: dataRoot.path });
      let json = null;
      try { json = result.stdout.trim() ? JSON.parse(result.stdout) : null; } catch { /* not JSON */ }
      evidence.appendJsonl('responses.jsonl', {
        seq, label, code: result.code, signal: result.signal, timedOut: result.timedOut, durationMs: result.durationMs,
        stdout: result.stdout, stderr: result.stderr, json, at: new Date().toISOString(),
      });
      return { ...result, json };
    },
    // Isolated daemon; readiness is a barrier armed before the spawn.
    startDaemon: async ({ deadlineMs = 60_000, env, ready } = {}) => {
      const runtimeDir = join(dataRoot.path, 'runtime');
      mkdirSync(runtimeDir, { recursive: true, mode: 0o700 });
      let readiness = null;
      if (!ready) {
        if (!POSIX) throw new HarnessError('DAEMON_READINESS_UNDEFINED', 'Windows daemon readiness needs a scenario-supplied ready(ctx, handle) barrier');
        const socket = join(runtimeDir, 'daemon.sock');
        if (Buffer.byteLength(socket) > MAX_SOCKET_PATH_BYTES) throw new HarnessError('CONFIG_INVALID', `daemon socket path ${socket} exceeds ${MAX_SOCKET_PATH_BYTES} bytes`);
        readiness = waitForPath(socket, { deadlineMs, signal, label: 'daemon socket' });
        readiness.catch(() => { /* raced below */ });
      }
      const cursor = journal.cursor();
      const handle = await owned.spawn('daemon', config.ferryxCli, ['--daemon'], { env: env ?? ctx.productEnv(), cwd: dataRoot.path, graceMs: 10_000 });
      const local = new AbortController();
      const forward = () => local.abort(signal.reason);
      signal.addEventListener('abort', forward, { once: true });
      const died = journal.waitFor('daemon exit', (e) => e.source === 'daemon' && e.kind === 'exit', { after: cursor, deadlineMs, signal: local.signal })
        .then((e) => { throw new HarnessError('DAEMON_EXITED', `daemon exited before ready (code ${e.code}, signal ${e.signal})`); });
      died.catch(() => { /* raced below */ });
      try {
        await Promise.race([ready ? ready(ctx, handle) : readiness, died]);
      } finally {
        signal.removeEventListener('abort', forward);
        local.abort(new HarnessError('SUPERSEDED', 'daemon readiness settled'));
      }
      return handle;
    },
  };
  return ctx;
}

export async function runScenario({ name, scenario, configInfo, evidence, receipts, interrupts, argv }) {
  const config = configInfo.config;
  const runId = newRunId();
  const startedAt = new Date().toISOString();
  const ledger = Ledger.forDataRoot(config.dataRoot);
  const journal = new Journal(evidence);
  const runnerStamp = (await probeProcess(process.pid)).startStamp ?? null;
  const owned = new Owned({ runId, ledger, evidence, journal, runnerStamp });
  const controller = new AbortController();
  const forward = () => controller.abort(interrupts.signal.reason);
  if (interrupts.signal.aborted) forward(); else interrupts.signal.addEventListener('abort', forward, { once: true });
  const faultsAtStart = interrupts.faults.length;
  const onFault = (kind, message) => {
    journal.push({ source: 'runner', kind: 'harness-fault', fault: kind, message });
    if (!controller.signal.aborted) controller.abort(new HarnessError('HARNESS_ERROR', `${kind}: ${message}`));
  };
  interrupts.faultListeners.add(onFault);
  const deadlineMs = scenario.deadlineMs ?? DEFAULT_SCENARIO_DEADLINE_MS;
  const deadlineTimer = setTimeout(() => controller.abort(new HarnessError('DEADLINE_EXCEEDED', `scenario ${name} exceeded its ${deadlineMs}ms deadline`)), deadlineMs);
  const runInfo = { schema: SCHEMA, runId, scenario: name, task: scenario.task ?? null, summary: scenario.summary ?? null, startedAt, pid: process.pid, argv, deadlineMs };
  evidence.writeJson('run.json', runInfo);
  evidence.writeJson('receipts.json', receipts);
  evidence.writeJson('config.redacted.json', { path: configInfo.path, sha256: configInfo.sha256, config });
  for (const file of ['requests.jsonl', 'responses.jsonl', 'events.jsonl']) evidence.appendText(file, '');
  evidence.log(`scenario ${name} run ${runId} started`);

  const assertions = [];
  const state = { frozen: false, late: [] };
  const roots = [];
  let status;
  let error = null;
  try {
    if (!scenario.implemented) {
      assertions.push({ name: 'scenario-implemented', ok: false, detail: `scenario ${name} is registered for plan task ${scenario.task} but not implemented; it can never PASS` });
      status = 'NOT_IMPLEMENTED';
    } else {
      const dataRoot = claimRunRoot(ledger, runId, 'data', config.dataRoot);
      roots.push(dataRoot);
      const memoryRoot = claimRunRoot(ledger, runId, 'memory', config.memoryRoot);
      roots.push(memoryRoot);
      const ctx = makeContext({ name, runId, configInfo, evidence, journal, owned, signal: controller.signal, assertions, state, dataRoot, memoryRoot });
      const work = Promise.resolve().then(() => scenario.run(ctx));
      work.catch(() => { /* raced below */ });
      const aborted = new Promise((_, rejectAbort) => {
        if (controller.signal.aborted) rejectAbort(controller.signal.reason);
        else controller.signal.addEventListener('abort', () => rejectAbort(controller.signal.reason), { once: true });
      });
      aborted.catch(() => { /* raced below */ });
      await Promise.race([work, aborted]);
      if (controller.signal.aborted) throw controller.signal.reason;
      state.frozen = true;
      const missing = (scenario.requiredAssertions ?? []).filter((required) => !assertions.some((assertion) => assertion.name === required));
      if (assertions.length === 0) {
        status = 'FAIL';
        error = { code: 'NO_ASSERTIONS', message: 'the scenario recorded no assertions; PASS requires named assertions' };
      } else if (!assertions.every((assertion) => assertion.ok)) {
        status = 'FAIL';
      } else if (missing.length > 0) {
        status = 'FAIL';
        error = { code: 'MISSING_ASSERTIONS', message: `required assertion(s) never recorded: ${missing.join(', ')}`, detail: missing };
      } else {
        status = 'PASS';
      }
    }
  } catch (caught) {
    error = errorInfo(caught);
    status = caught?.code === 'INTERRUPTED' ? 'INTERRUPTED' : caught?.code === 'DEADLINE_EXCEEDED' ? 'DEADLINE_EXCEEDED' : 'HARNESS_ERROR';
  } finally {
    state.frozen = true;
    clearTimeout(deadlineTimer);
    interrupts.signal.removeEventListener('abort', forward);
  }

  interrupts.phase = 'cleanup';
  process.stdout.write(`${JSON.stringify({ unifiedSupervisor: { phase: 'cleanup', scenario: name, runId } })}\n`);
  evidence.log(`scenario ${name} teardown started (status so far ${status})`);
  const cleanupStarted = new Date().toISOString();
  const processes = await owned.teardown(status === 'INTERRUPTED' ? 'interrupted' : 'teardown');
  const paths = roots.map((record) => releaseRunRoot(ledger, record, evidence, [config.dataRoot, config.memoryRoot]));
  interrupts.faultListeners.delete(onFault);
  const faults = interrupts.faults.slice(faultsAtStart);
  const cleanup = {
    schema: SCHEMA, runId, scenario: name, ledger: ledger.file, startedAt: cleanupStarted, finishedAt: new Date().toISOString(),
    interrupts: interrupts.count, interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received, faults,
    processes, paths, ok: processes.every((entry) => entry.ok) && paths.every((entry) => entry.ok),
  };
  evidence.writeJson('cleanup.json', cleanup);

  // Final status is recomputed from everything observed, never only from the settle-time view.
  if (faults.length > 0 && status !== 'INTERRUPTED') {
    status = 'HARNESS_ERROR';
    if (error?.code !== 'HARNESS_ERROR') error = { code: 'HARNESS_ERROR', message: `uncaught fault during the run: ${faults[0].kind}`, detail: faults };
  }
  if (status === 'PASS' && state.late.length > 0) {
    status = 'FAIL';
    error = { code: 'LATE_ASSERTION', message: 'assertions were recorded after the scenario settled', detail: state.late };
  }
  if (status === 'PASS' && !assertions.every((assertion) => assertion.ok)) status = 'FAIL';
  if (!cleanup.ok && status === 'PASS') status = 'CLEANUP_FAILED';
  if (interrupts.count > 0) status = 'INTERRUPTED';
  const exitCode = STATUS_EXIT[status];
  evidence.writeJson('assertions.json', {
    schema: SCHEMA, scenario: name, runId, status, exitCode,
    passRule: 'PASS only when at least one assertion is recorded, every assertion (and every required one) is ok, none arrived late, no error or uncaught fault occurred and cleanup.json is ok',
    assertions, lateAssertions: state.late, requiredAssertions: scenario.requiredAssertions ?? [], error, cleanupOk: cleanup.ok,
  });
  evidence.writeJson('run.json', { ...runInfo, finishedAt: new Date().toISOString(), status, exitCode });
  evidence.log(`scenario ${name} ${status} exit=${exitCode}`);
  interrupts.phase = 'run';
  return { name, runId, status, exitCode };
}

export async function cleanupMode({ configInfo, evidence, interrupts, forceOwner = false }) {
  interrupts.phase = 'cleanup';
  const startedAt = new Date().toISOString();
  const { config } = configInfo;
  const ledger = Ledger.forDataRoot(config.dataRoot);
  const { invalidLines } = ledger.read();
  const open = ledger.open();
  const entries = [];
  for (const record of open.filter((r) => r.type === 'spawn')) {
    try { entries.push(await reclaimProcess(record, { ledger, forceOwner })); } catch (error) {
      entries.push({ kind: 'process', key: record.key, pid: record.pid, status: 'error', ok: false, error: errorInfo(error) });
    }
  }
  for (const record of open.filter((r) => r.type === 'path')) entries.push(releaseRunRoot(ledger, record, evidence, [config.dataRoot, config.memoryRoot]));
  const ok = entries.every((entry) => entry.ok);
  evidence.writeJson('cleanup.json', {
    schema: SCHEMA, mode: 'cleanup', ledger: ledger.file, invalidLedgerLines: invalidLines, startedAt, finishedAt: new Date().toISOString(),
    interrupts: interrupts.count, interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received, faults: interrupts.faults,
    forceOwner, ok, entries,
  });
  evidence.log(`cleanup ${ok ? 'ok' : 'INCOMPLETE'} entries=${entries.length}`);
  if (!ok || interrupts.faults.length > 0) return EXIT.CLEANUP;
  return interrupts.count > 0 ? EXIT.INTERRUPTED : EXIT.PASS;
}
