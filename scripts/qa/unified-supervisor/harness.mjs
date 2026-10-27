// Scenario-agnostic primitives for scripts/qa/unified-supervisor.mjs:
// config isolation, evidence, secret redaction, recorded-PID ownership,
// subscribe-before-trigger barriers, bounded deadlines and receipts.
//
// Ownership model. Every process the runner starts is recorded (pid, absolute
// executable, argv, OS start stamp, owning runner) in an append-only ledger under
// the configured dataRoot before anything waits on it. Teardown signals only
// recorded processes: in-run through the unreaped child handle (its pid cannot
// have been reused), across runs (--cleanup) only after the OS start stamp and
// argv still match the record. A mismatch is reported and never signalled.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import {
  appendFileSync, createReadStream, existsSync, lstatSync, mkdirSync, readdirSync, readFileSync,
  readlinkSync, realpathSync, renameSync, rmSync, statSync, watch, writeFileSync,
} from 'node:fs';
import { arch, homedir, hostname, release } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { isDeepStrictEqual } from 'node:util';

export const PLATFORM = process.platform;
export const POSIX = PLATFORM !== 'win32';
export const SCHEMA = 'unified-supervisor/1';

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
const USAGE_CODES = new Set(['USAGE', 'CONFIG_INVALID', 'PRODUCTION_PATH_REFUSED', 'EVIDENCE_DIR_NOT_EMPTY', 'UNKNOWN_SCENARIO']);

const FORCE_EXIT_MS = 5_000;
const MEMBER_GONE_MS = 3_000;
const PROBE_TIMEOUT_MS = 15_000;
const LOG_CAP_BYTES = 16 * 1024 * 1024;
const LINE_CAP = 64 * 1024;
const OUTPUT_CAP_BYTES = 4 * 1024 * 1024;
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

export function aggregateExit(results) {
  if (results.every((result) => result.status === 'PASS')) return { status: 'PASS', exitCode: EXIT.PASS };
  for (const status of STATUS_PRECEDENCE) {
    if (results.some((result) => result.status === status)) return { status, exitCode: STATUS_EXIT[status] };
  }
  return { status: 'FAIL', exitCode: EXIT.FAIL };
}

export const isSha256 = (value) => typeof value === 'string' && /^[0-9a-f]{64}$/.test(value);
export const stamp = () => new Date().toISOString().replace(/[:.]/g, '-');
export const safeName = (value) => String(value).replace(/[^A-Za-z0-9._-]/g, '_');
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

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
  [/\bxox[abposr]-[A-Za-z0-9-]{6,}/g, '[REDACTED:slack-token]'],
  [/\bxapp-[A-Za-z0-9-]{6,}/g, '[REDACTED:slack-app-token]'],
  [/\bsk-(?:ant-)?[A-Za-z0-9_-]{12,}/g, '[REDACTED:api-key]'],
  [/\bgh[pousr]_[A-Za-z0-9]{20,}/g, '[REDACTED:github-token]'],
  [/\bAKIA[0-9A-Z]{16}\b/g, '[REDACTED:aws-key]'],
  [/\b(Bearer|Basic)\s+[A-Za-z0-9._~+/=-]{8,}/gi, '$1 [REDACTED]'],
  [/("[^"\\]*(?:token|secret|passw(?:or)?d|api[-_]?key|authorization|cookie|credential|private[-_]?key)[^"\\]*"\s*:\s*)"(?:[^"\\]|\\.)*"/gi, '$1"[REDACTED]"'],
  [/\b([A-Za-z_][A-Za-z0-9_]*(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|APIKEY)[A-Za-z0-9_]*)=([^\s"',;\\]+)/gi, '$1=[REDACTED]'],
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
  let out = resolve(path);
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
function realish(path) {
  let current = resolve(path);
  const rest = [];
  for (;;) {
    try { return join(realpathSync(current), ...rest.reverse()); } catch { /* walk up */ }
    const parent = dirname(current);
    if (parent === current) return resolve(path);
    rest.push(basename(current));
    current = parent;
  }
}

export function productionRoots({ env = process.env, home = homedir() } = {}) {
  const roots = [
    join(home, '.ferryx'),
    join(home, '.config', 'ferryx'), join(home, '.local', 'share', 'ferryx'), join(home, '.local', 'state', 'ferryx'), join(home, '.cache', 'ferryx'),
    join(home, '.config', 'com.ferryx.app'), join(home, '.local', 'share', 'com.ferryx.app'),
    join(home, 'Library', 'Application Support', 'ferryx'), join(home, 'Library', 'Application Support', 'com.ferryx.app'),
    join(home, 'Library', 'Application Support', 'com.ferryx.mobile'),
    join(home, '.maho'), join(home, '.mhc'), join(home, '.config', 'maho'), join(home, '.config', 'mhc'),
    join(home, '.local', 'share', 'maho'), join(home, '.local', 'share', 'mhc'),
  ];
  for (const base of [env.APPDATA, env.LOCALAPPDATA]) {
    if (base && isAbsolute(base)) roots.push(join(base, 'ferryx'), join(base, 'com.ferryx.app'), join(base, 'maho'), join(base, 'mhc'));
  }
  // The invoking shell's live Ferryx locations are production by definition.
  for (const name of ['FERRYX_DATA_DIR', 'FERRYX_RUNTIME_DIR', 'FERRYX_HUB_DB', 'FERRYX_HUB_CONFIG']) {
    if (env[name] && isAbsolute(env[name])) roots.push(env[name]);
  }
  return roots;
}

function namesHubDatabase(path) {
  return path.split(/[\\/]+/).some((segment) => segment.toLowerCase() === 'hub.sqlite');
}

export function assertNotProduction(path, label, context = {}) {
  for (const candidate of new Set([resolve(path), realish(path)])) {
    if (namesHubDatabase(candidate)) {
      throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} names a hub.sqlite database`);
    }
    for (const root of productionRoots(context)) {
      if (isInside(candidate, root)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} is inside production path ${root}`);
    }
  }
}

export function assertIsolatedRoot(path, label, context = {}) {
  assertNotProduction(path, label, context);
  const home = context.home ?? homedir();
  for (const candidate of new Set([resolve(path), realish(path)])) {
    for (const root of productionRoots(context)) {
      if (isInside(root, candidate)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} contains production path ${root}`);
    }
    if (isInside(home, candidate)) throw new HarnessError('PRODUCTION_PATH_REFUSED', `${label} ${candidate} contains the home directory ${home}`);
  }
}

// ---------------------------------------------------------------------------
// Config.

export const CONFIG_REQUIRED = Object.freeze(['ferryxCli', 'ferryxApp', 'mhcBin', 'dataRoot', 'memoryRoot', 'projectRoot', 'ownerPrincipal']);
export const CONFIG_OPTIONAL = Object.freeze(['pairedHostId', 'slackChannel', '$schema', '$comment']);
const BINARY_FIELDS = ['ferryxCli', 'ferryxApp', 'mhcBin'];
const ROOT_FIELDS = ['dataRoot', 'memoryRoot', 'projectRoot'];

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
  for (const key of ['pairedHostId', 'slackChannel', '$schema', '$comment']) {
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
      if (a !== b && isInside(config[a], config[b])) throw new HarnessError('CONFIG_INVALID', `${a} and ${b} must be disjoint roots`);
    }
  }
  for (const key of BINARY_FIELDS) {
    let info;
    try { info = statSync(config[key]); } catch { throw new HarnessError('CONFIG_INVALID', `${key} ${config[key]} does not exist`); }
    if (key !== 'ferryxApp' && !info.isFile()) throw new HarnessError('CONFIG_INVALID', `${key} ${config[key]} is not a file`);
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
        if (!force) throw new HarnessError('EVIDENCE_DIR_NOT_EMPTY', `${target} holds evidence from a prior run; choose a new directory or pass --force to move it aside`);
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

  open() {
    const { records } = this.read();
    const closed = new Set(records.filter((r) => r.type === 'released' || r.type === 'path-released').map((r) => r.key));
    return records.filter((r) => (r.type === 'spawn' || r.type === 'path') && !closed.has(r.key));
  }
}

// ---------------------------------------------------------------------------
// Bounded capture for read-only probes (ps, PowerShell, git). Not ledgered: the
// child is killed through its own handle at the deadline.

export function capture(file, args, { timeoutMs = 10_000, cwd, env } = {}) {
  return new Promise((resolvePromise) => {
    let child;
    try {
      child = spawn(file, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
    } catch (error) {
      resolvePromise({ code: null, signal: null, stdout: '', stderr: '', timedOut: false, error: String(error?.message ?? error) });
      return;
    }
    const out = [];
    const err = [];
    let size = 0;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolvePromise({ stdout: Buffer.concat(out).toString('utf8'), stderr: Buffer.concat(err).toString('utf8'), ...result });
    };
    const timer = setTimeout(() => {
      try { child.kill('SIGKILL'); } catch { /* already gone */ }
      finish({ code: null, signal: 'SIGKILL', timedOut: true });
    }, timeoutMs);
    const collect = (sink) => (chunk) => { if (size < OUTPUT_CAP_BYTES) { sink.push(chunk); size += chunk.length; } };
    child.stdout?.on('data', collect(out));
    child.stderr?.on('data', collect(err));
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
    if (result.timedOut || result.error) return { state: 'unknown', reason: result.error ?? 'process probe timed out' };
    const parsed = lastJsonLine(result.stdout);
    if (!parsed.ok) return { state: 'unknown', reason: 'unparsed process probe output' };
    if (parsed.value === null) return { state: 'gone' };
    return { state: 'alive', startStamp: `win32:${parsed.value.created}`, ppid: parsed.value.ppid, command: parsed.value.cmd || null, executable: parsed.value.exe || null };
  }
  const st = await capture('ps', ['-o', 'state=', '-o', 'lstart=', '-p', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
  if (st.timedOut || st.error) return { state: 'unknown', reason: st.error ?? 'ps timed out' };
  const line = st.stdout.trim();
  if (st.code !== 0 || !line) return { state: 'gone' };
  const match = /^(\S+)\s+(.+)$/.exec(line);
  if (!match) return { state: 'unknown', reason: `unparsed ps output: ${line}` };
  const startStamp = `darwin:${match[2].replace(/\s+/g, ' ')}`;
  if (match[1].includes('Z')) return { state: 'zombie', startStamp };
  const cmd = await capture('ps', ['-ww', '-o', 'command=', '-p', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
  return { state: 'alive', startStamp, command: cmd.stdout.trim() || null };
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
    return rows;
  }
  if (PLATFORM === 'win32') {
    const result = await powershell([
      "$ErrorActionPreference = 'SilentlyContinue';",
      '$rows = @(Get-CimInstance Win32_Process | ForEach-Object { [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; created = [string]$(if ($_.CreationDate) { $_.CreationDate.ToUniversalTime().ToString(\'o\') }) } });',
      'ConvertTo-Json -InputObject $rows -Compress -Depth 3',
    ].join('\n'));
    if (result.timedOut || result.error) return null;
    const parsed = lastJsonLine(result.stdout);
    if (!parsed.ok) return null;
    const rows = Array.isArray(parsed.value) ? parsed.value : [parsed.value];
    return rows.filter(Boolean).map((row) => ({ pid: Number(row.pid), ppid: Number(row.ppid), pgid: null, state: 'alive', startStamp: row.created ? `win32:${row.created}` : null }));
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
  return rows;
}

// Descendants by parent links from one table read, plus (POSIX) members of the
// root's own process group. The root itself is excluded.
export function familyOf(rows, rootPid) {
  const byParent = new Map();
  for (const row of rows) {
    if (!byParent.has(row.ppid)) byParent.set(row.ppid, []);
    byParent.get(row.ppid).push(row);
  }
  const seen = new Set([rootPid]);
  const out = [];
  const queue = [rootPid];
  while (queue.length > 0) {
    for (const row of byParent.get(queue.shift()) ?? []) {
      if (seen.has(row.pid)) continue;
      seen.add(row.pid);
      out.push(row);
      queue.push(row.pid);
    }
  }
  if (POSIX) {
    for (const row of rows) {
      if (row.pgid === rootPid && !seen.has(row.pid)) { seen.add(row.pid); out.push(row); }
    }
  }
  return out;
}

function stem(path) { return basename(String(path)).replace(/\.(exe|cmd|bat)$/i, '').toLowerCase(); }

function argvVerdict(record, probe) {
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

// match: positively the recorded process. mismatch: positively not (pid reused).
// gone: exited (zombies count). unknown: unobservable, never signalled.
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
  if (argv === false) return { verdict: 'mismatch', reason: 'argv differs from the recorded launch' };
  if (argv === null && !stamps) return { verdict: 'unknown', reason: 'neither argv nor start stamp observable' };
  return { verdict: 'match', strength: stamps ? (argv ? 'start-stamp+argv' : 'start-stamp') : 'argv-only' };
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
    const result = await capture('taskkill', [...(group ? ['/T'] : []), ...(signal === 'SIGKILL' ? ['/F'] : []), '/PID', String(pid)], { timeoutMs: PROBE_TIMEOUT_MS });
    return { signal, target: group ? 'tree' : 'pid', code: result.code, error: result.error ?? null };
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
  const { verdict, reason } = identityVerdict(record, await probeProcess(member.pid));
  const entry = { pid: member.pid, startStamp: member.startStamp, initial: verdict, reason: reason ?? null, signals: [], finalState: null };
  if (verdict === 'match') {
    entry.signals.push(await killPid(member.pid, 'SIGKILL'));
    entry.finalState = (await awaitGone(record, MEMBER_GONE_MS)) ? 'gone' : 'alive';
  } else {
    entry.finalState = verdict === 'unknown' ? 'unknown' : 'gone';
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

  async #spawn(id, executable, args, { cwd, env = process.env, graceMs = 3_000, stdin = 'ignore', onLine } = {}) {
    if (!isAbsolute(executable)) throw new HarnessError('SPAWN_FAILED', `${id}: executable must be absolute, got ${executable}`);
    const key = `${this.runId}/proc/${safeName(id)}#${++this.counter}`;
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
    this.#pipe(handle, child.stdout, 'stdout', onLine);
    this.#pipe(handle, child.stderr, 'stderr', onLine);
    const probe = await probeProcess(child.pid);
    handle.record = {
      type: 'spawn', key, runId: this.runId, id, pid: child.pid, executable, args: [...args], cwd: cwd ?? process.cwd(),
      detached: POSIX, graceMs, startStamp: probe.startStamp ?? null, runnerPid: process.pid, runnerStartStamp: this.runnerStamp,
    };
    this.ledger.append(handle.record);
    this.evidence.appendJsonl('owned.jsonl', handle.record);
    this.handles.push(handle);
    this.journal.push({ source: id, kind: 'spawned', pid: child.pid, executable, args });
    return handle;
  }

  #pipe(handle, stream, kind, onLine) {
    if (!stream) return;
    const logRel = `logs/${safeName(handle.id)}.${kind}.log`;
    let buffer = '';
    let written = 0;
    const emit = (raw) => {
      const line = raw.length > LINE_CAP ? `${raw.slice(0, LINE_CAP)}[line truncated]` : raw;
      if (written < LOG_CAP_BYTES) {
        this.evidence.appendText(logRel, `${line}\n`);
        written += line.length + 1;
        if (written >= LOG_CAP_BYTES) this.evidence.appendText(logRel, '[log capped]\n');
      }
      let json = null;
      const trimmed = line.trim();
      if (trimmed.startsWith('{')) { try { json = JSON.parse(trimmed); } catch { /* plain text */ } }
      this.journal.push({ source: handle.id, kind, line, json, pid: handle.pid });
      onLine?.(kind, line, json);
    };
    stream.setEncoding('utf8');
    stream.on('data', (chunk) => {
      buffer += chunk;
      let index;
      while ((index = buffer.indexOf('\n')) >= 0) {
        emit(buffer.slice(0, index).replace(/\r$/, ''));
        buffer = buffer.slice(index + 1);
      }
      if (buffer.length > LINE_CAP * 4) { emit(buffer); buffer = ''; }
    });
    stream.on('end', () => { if (buffer) emit(buffer); buffer = ''; });
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
    if (!POSIX) {
      receipt.signals.push({ ...(await killPid(handle.pid, signal, { group: true })), at });
      return;
    }
    const grouped = await killPid(handle.pid, signal, { group: true });
    receipt.signals.push({ ...grouped, at });
    if (!grouped.error) return;
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
      signals: [], alreadyExited: false, exited: false, exitCode: null, exitSignal: null, descendantsObservable: false, descendants: [], ok: false,
    };
    const before = await listProcesses();
    receipt.descendantsObservable = before !== null;
    let members = [];
    if (handle.exitInfo) {
      receipt.alreadyExited = true;
      receipt.identity = 'exited-before-teardown';
      // A row now holding the leader's pid is a reused pid: its group is not ours.
      if (before && !before.some((row) => row.pid === handle.pid)) members = familyOf(before, handle.pid);
    } else {
      // An unreaped child cannot have had its pid reused: the handle is the identity proof.
      receipt.identity = 'child-handle';
      const probe = await probeProcess(handle.pid);
      receipt.probe = { state: probe.state, ...identityVerdict(handle.record ?? {}, probe) };
      members = before ? familyOf(before, handle.pid) : [];
      await this.#signal(handle, 'SIGTERM', receipt);
      if (!(await this.#exitWithin(handle, handle.graceMs))) {
        await this.#signal(handle, 'SIGKILL', receipt);
        await this.#exitWithin(handle, FORCE_EXIT_MS);
      }
    }
    receipt.exited = handle.exitInfo !== null;
    receipt.exitCode = handle.exitInfo?.code ?? null;
    receipt.exitSignal = handle.exitInfo?.signal ?? null;
    if (POSIX && receipt.exited) {
      const after = await listProcesses();
      if (after && !after.some((row) => row.pid === handle.pid)) {
        members = dedupeRows([...members, ...after.filter((row) => row.pgid === handle.pid)]);
      }
    }
    for (const member of members) receipt.descendants.push(await reapMember(member));
    receipt.ok = receipt.exited && receipt.descendants.every((entry) => entry.finalState === 'gone');
    receipt.finishedAt = new Date().toISOString();
    if (receipt.ok) this.ledger.append({ type: 'released', key: handle.key, reason, exited: receipt.exited });
    return receipt;
  }

  async teardown(reason) {
    this.closed = true;
    await withDeadline(Promise.allSettled([...this.pending]), 15_000, () => null);
    const receipts = [];
    for (const handle of [...this.handles].reverse()) {
      try {
        receipts.push(await this.terminate(handle, reason));
      } catch (error) {
        receipts.push({ key: handle.key, id: handle.id, pid: handle.pid, ok: false, error: errorInfo(error) });
      }
    }
    return receipts.reverse();
  }
}

// Cross-run reclaim for --cleanup: identity-verified, never by name or port.
export async function reclaimProcess(record, { ledger, force = false }) {
  const entry = {
    kind: 'process', key: record.key, runId: record.runId, id: record.id, pid: record.pid, executable: record.executable, args: record.args,
    startStamp: record.startStamp ?? null, status: null, identity: null, killed: false, signals: [], descendants: [], ok: false,
  };
  if (record.pid === process.pid || record.pid === process.ppid) {
    entry.status = 'refused-self';
    return entry;
  }
  if (!force && record.runnerPid) {
    const runner = identityVerdict({ pid: record.runnerPid, startStamp: record.runnerStartStamp ?? null }, await probeProcess(record.runnerPid));
    if (runner.verdict === 'match' || runner.verdict === 'unknown') {
      entry.status = runner.verdict === 'match' ? 'owner-runner-alive' : 'owner-runner-unverifiable';
      entry.detail = 'the run that owns this process may still be active and tears down its own resources; pass --force to override';
      return entry;
    }
  }
  entry.identity = identityVerdict(record, await probeProcess(record.pid));
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
  const rows = await listProcesses();
  let members = rows ? familyOf(rows, record.pid) : [];
  const group = Boolean(record.detached) || !POSIX;
  entry.killed = true;
  entry.signals.push(await killPid(record.pid, 'SIGTERM', { group }));
  let gone = await awaitGone(record, record.graceMs ?? 3_000);
  if (!gone) {
    entry.signals.push(await killPid(record.pid, 'SIGKILL', { group }));
    gone = await awaitGone(record, FORCE_EXIT_MS);
  }
  if (POSIX && gone) {
    const after = await listProcesses();
    if (after && !after.some((row) => row.pid === record.pid)) members = dedupeRows([...members, ...after.filter((row) => row.pgid === record.pid)]);
  }
  for (const member of members) entry.descendants.push(await reapMember(member));
  entry.status = gone ? 'terminated' : 'kill-failed';
  entry.ok = gone && entry.descendants.every((member) => member.finalState === 'gone');
  if (entry.ok) ledger.append({ type: 'released', key: record.key, reason: 'cleanup:terminated' });
  return entry;
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

export function releaseRunRoot(ledger, record, evidence) {
  const entry = { kind: 'path', key: record.key, label: record.label, path: record.path, status: null, preserved: [], removed: false, ok: false };
  const runsDir = join(record.base, 'runs');
  if (!isInside(record.path, runsDir) || norm(record.path) === norm(runsDir)) {
    entry.status = 'refused-outside-owned-runs';
    return entry;
  }
  try { assertNotProduction(record.path, 'run root'); } catch (error) {
    entry.status = 'refused-production';
    entry.error = errorInfo(error);
    return entry;
  }
  try {
    if (existsSync(record.path)) {
      entry.preserved = preserveLogs(record.path, evidence, `logs/run-roots/${safeName(record.label)}`);
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
// FERRYX_*, SLACK_* and provider variables never reach product binaries, and
// HOME points inside the run root so no default ~/.ferryx path is reachable.

const ENV_ALLOW = ['PATH', 'USER', 'LOGNAME', 'SHELL', 'LANG', 'LC_ALL', 'LC_CTYPE', 'TERM', 'TMPDIR', 'TZ',
  'SystemRoot', 'SYSTEMROOT', 'windir', 'WINDIR', 'ComSpec', 'COMSPEC', 'PATHEXT', 'TEMP', 'TMP', 'ProgramData', 'ProgramFiles', 'ProgramFiles(x86)'];

export function productEnv({ dataRun, extra = {} }) {
  const env = {};
  for (const key of ENV_ALLOW) if (process.env[key] !== undefined) env[key] = process.env[key];
  const home = join(dataRun, 'home');
  mkdirSync(home, { recursive: true });
  Object.assign(env, {
    HOME: home,
    FERRYX_DATA_DIR: join(dataRun, 'data'),
    FERRYX_RUNTIME_DIR: join(dataRun, 'runtime'),
    FERRYX_HUB_DB: join(dataRun, 'hub', 'hub.sqlite'),
    FERRYX_HUB: 'on',
    FERRYX_HUB_QUIET_HOURS: 'off',
  });
  if (!POSIX) Object.assign(env, { USERPROFILE: home, APPDATA: join(home, 'AppData', 'Roaming'), LOCALAPPDATA: join(home, 'AppData', 'Local') });
  return { ...env, ...extra };
}

// ---------------------------------------------------------------------------
// Receipts: source revision, binary and config hashes, host and runtime.

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

async function gitReceipt(dir) {
  const rev = await capture('git', ['-C', dir, 'rev-parse', 'HEAD'], { timeoutMs: PROBE_TIMEOUT_MS });
  if (rev.code !== 0) return { dir, rev: null, error: (rev.error ?? rev.stderr.trim()) || 'git rev-parse failed' };
  const branch = await capture('git', ['-C', dir, 'rev-parse', '--abbrev-ref', 'HEAD'], { timeoutMs: PROBE_TIMEOUT_MS });
  const status = await capture('git', ['-C', dir, 'status', '--porcelain=v1'], { timeoutMs: PROBE_TIMEOUT_MS });
  const dirty = status.code === 0 ? status.stdout.split('\n').filter(Boolean) : null;
  return {
    dir, rev: rev.stdout.trim(), branch: branch.code === 0 ? branch.stdout.trim() : null,
    dirtyEntries: dirty ? dirty.length : null, statusSha256: dirty ? sha256(status.stdout) : null,
  };
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
    project: config ? { root: config.projectRoot, exists: existsSync(config.projectRoot), git: existsSync(config.projectRoot) ? await gitReceipt(config.projectRoot) : null } : null,
    binaries: config ? {
      ferryxCli: await describeArtifact(config.ferryxCli),
      ferryxApp: await describeArtifact(config.ferryxApp),
      mhcBin: await describeArtifact(config.mhcBin),
    } : null,
    config: configInfo ? { path: configInfo.path, sha256: configInfo.sha256, bytes: configInfo.bytes } : null,
  };
}

// ---------------------------------------------------------------------------
// One scenario run: evidence, deadline, assertions, guaranteed teardown.

function makeContext({ name, runId, configInfo, evidence, journal, owned, signal, assertions, dataRoot, memoryRoot }) {
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
    assert: (assertion, ok, detail = null) => {
      const entry = { name: assertion, ok: ok === true, detail };
      assertions.push(entry);
      evidence.log(`assert ${assertion} ${entry.ok ? 'ok' : 'FAILED'}`);
      return entry.ok;
    },
    spawn: (id, executable, args, options = {}) => owned.spawn(id, executable, args, options),
    run: (id, executable, args, options = {}) => owned.run(id, executable, args, { signal, ...options }),
    terminate: (handle, reason = 'scenario') => owned.terminate(handle, reason),
    waitFor: (label, predicate, options = {}) => journal.waitFor(label, predicate, { signal, ...options }),
    waitForPath: (target, options = {}) => waitForPath(target, { signal, ...options }),
    productEnv: (extra = {}) => productEnv({ dataRun: dataRoot.path, extra }),
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
        readiness = waitForPath(join(runtimeDir, 'daemon.sock'), { deadlineMs, signal, label: 'daemon socket' });
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
  const runId = `qa-${stamp()}-${process.pid}-${++runCounter}`;
  const startedAt = new Date().toISOString();
  const ledger = Ledger.forDataRoot(config.dataRoot);
  const journal = new Journal(evidence);
  const runnerStamp = (await probeProcess(process.pid)).startStamp ?? null;
  const owned = new Owned({ runId, ledger, evidence, journal, runnerStamp });
  const controller = new AbortController();
  const forward = () => controller.abort(interrupts.signal.reason);
  if (interrupts.signal.aborted) forward(); else interrupts.signal.addEventListener('abort', forward, { once: true });
  const deadlineMs = scenario.deadlineMs ?? DEFAULT_SCENARIO_DEADLINE_MS;
  const deadlineTimer = setTimeout(() => controller.abort(new HarnessError('DEADLINE_EXCEEDED', `scenario ${name} exceeded its ${deadlineMs}ms deadline`)), deadlineMs);
  const runInfo = { schema: SCHEMA, runId, scenario: name, task: scenario.task ?? null, summary: scenario.summary ?? null, startedAt, pid: process.pid, argv, deadlineMs };
  evidence.writeJson('run.json', runInfo);
  evidence.writeJson('receipts.json', receipts);
  evidence.writeJson('config.redacted.json', { path: configInfo.path, sha256: configInfo.sha256, config });
  for (const file of ['requests.jsonl', 'responses.jsonl', 'events.jsonl']) evidence.appendText(file, '');
  evidence.log(`scenario ${name} run ${runId} started`);

  const assertions = [];
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
      const ctx = makeContext({ name, runId, configInfo, evidence, journal, owned, signal: controller.signal, assertions, dataRoot, memoryRoot });
      const work = Promise.resolve().then(() => scenario.run(ctx));
      work.catch(() => { /* raced below */ });
      const aborted = new Promise((_, rejectAbort) => {
        if (controller.signal.aborted) rejectAbort(controller.signal.reason);
        else controller.signal.addEventListener('abort', () => rejectAbort(controller.signal.reason), { once: true });
      });
      aborted.catch(() => { /* raced below */ });
      await Promise.race([work, aborted]);
      if (controller.signal.aborted) throw controller.signal.reason;
      if (assertions.length === 0) {
        status = 'FAIL';
        error = { code: 'NO_ASSERTIONS', message: 'the scenario recorded no assertions; PASS requires named assertions' };
      } else {
        status = assertions.every((assertion) => assertion.ok) ? 'PASS' : 'FAIL';
      }
    }
  } catch (caught) {
    error = errorInfo(caught);
    status = caught?.code === 'INTERRUPTED' ? 'INTERRUPTED' : caught?.code === 'DEADLINE_EXCEEDED' ? 'DEADLINE_EXCEEDED' : 'HARNESS_ERROR';
  } finally {
    clearTimeout(deadlineTimer);
    interrupts.signal.removeEventListener('abort', forward);
  }

  interrupts.phase = 'cleanup';
  process.stdout.write(`${JSON.stringify({ unifiedSupervisor: { phase: 'cleanup', scenario: name, runId } })}\n`);
  evidence.log(`scenario ${name} teardown started (status so far ${status})`);
  const cleanupStarted = new Date().toISOString();
  const processes = await owned.teardown(status === 'INTERRUPTED' ? 'interrupted' : 'teardown');
  const paths = roots.map((record) => releaseRunRoot(ledger, record, evidence));
  const cleanup = {
    schema: SCHEMA, runId, scenario: name, ledger: ledger.file, startedAt: cleanupStarted, finishedAt: new Date().toISOString(),
    interrupts: interrupts.count, interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received,
    processes, paths, ok: processes.every((entry) => entry.ok) && paths.every((entry) => entry.ok),
  };
  evidence.writeJson('cleanup.json', cleanup);
  if (!cleanup.ok && status === 'PASS') status = 'CLEANUP_FAILED';
  if (interrupts.count > 0) status = 'INTERRUPTED';
  const exitCode = STATUS_EXIT[status];
  evidence.writeJson('assertions.json', {
    schema: SCHEMA, scenario: name, runId, status, exitCode,
    passRule: 'PASS only when at least one assertion is recorded, every assertion is ok, no error occurred and cleanup.json is ok',
    assertions, error, cleanupOk: cleanup.ok,
  });
  evidence.writeJson('run.json', { ...runInfo, finishedAt: new Date().toISOString(), status, exitCode });
  evidence.log(`scenario ${name} ${status} exit=${exitCode}`);
  interrupts.phase = 'run';
  return { name, runId, status, exitCode };
}

export async function cleanupMode({ configInfo, evidence, interrupts, force }) {
  interrupts.phase = 'cleanup';
  const startedAt = new Date().toISOString();
  const ledger = Ledger.forDataRoot(configInfo.config.dataRoot);
  const { invalidLines } = ledger.read();
  const open = ledger.open();
  const entries = [];
  for (const record of open.filter((r) => r.type === 'spawn')) {
    try { entries.push(await reclaimProcess(record, { ledger, force })); } catch (error) {
      entries.push({ kind: 'process', key: record.key, pid: record.pid, status: 'error', ok: false, error: errorInfo(error) });
    }
  }
  for (const record of open.filter((r) => r.type === 'path')) entries.push(releaseRunRoot(ledger, record, evidence));
  const ok = entries.every((entry) => entry.ok);
  evidence.writeJson('cleanup.json', {
    schema: SCHEMA, mode: 'cleanup', ledger: ledger.file, invalidLedgerLines: invalidLines, startedAt, finishedAt: new Date().toISOString(),
    interrupts: interrupts.count, interruptsDuringCleanup: interrupts.duringCleanup, received: interrupts.received, force, ok, entries,
  });
  evidence.log(`cleanup ${ok ? 'ok' : 'INCOMPLETE'} entries=${entries.length}`);
  if (!ok) return EXIT.CLEANUP;
  return interrupts.count > 0 ? EXIT.INTERRUPTED : EXIT.PASS;
}
