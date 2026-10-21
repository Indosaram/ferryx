// Process identity, guarded command execution and event-driven exit waits.
//
// Exit is observed through an OS waiter child, never a pid polling loop: macOS
// `caffeinate -w <pid>`, Linux `tail --pid=<pid> -f /dev/null`, Windows
// `Wait-Process -Id <pid>`. The waiter exits when the target does; its 'exit' event is the signal.

import { spawn } from 'node:child_process';
import { closeSync, fstatSync, openSync, readFileSync, readSync, readlinkSync } from 'node:fs';
import { assertCommandAllowed } from './guards.mjs';

export function withDeadline(promise, ms, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`deadline ${ms}ms: ${label}`)), ms); }),
  ]).finally(() => clearTimeout(timer));
}

/** Runs a command to completion with a deadline; OS-job commands pass the guard first. */
export function run(command, args, { ctx = null, env = undefined, cwd = undefined, input = null, deadlineMs = 60_000, log = null, allowFailure = true } = {}) {
  if (ctx) assertCommandAllowed(command, args, ctx);
  return withDeadline(new Promise((resolve, reject) => {
    const child = spawn(command, args, { env, cwd, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.once('error', reject);
    child.once('close', (status) => {
      const result = { command, args, status, stdout, stderr };
      if (log) log(result);
      if (status !== 0 && !allowFailure) reject(new Error(`${command} ${args.join(' ')} exited ${status}: ${stderr || stdout}`));
      else resolve(result);
    });
    if (input !== null) child.stdin.end(input); else child.stdin.end();
  }), deadlineMs, `${command} ${args.join(' ')}`);
}

/**
 * Runs a command with stdout and stderr both written straight to an open file descriptor, so the
 * output never passes through this process's memory (a build emits ~1.3 MB per run). Resolves with
 * the real exit: { status, signal, pid, timedOut, error }. Past deadlineMs the child is killed
 * (SIGKILL) and the outcome says timedOut. A spawn failure (ENOENT) resolves with error set.
 */
export function runToFd(command, args, { ctx = null, env = undefined, cwd = undefined, fd, deadlineMs = 60_000 } = {}) {
  if (ctx) assertCommandAllowed(command, args, ctx);
  return new Promise((resolve) => {
    let timedOut = false;
    let settled = false;
    const child = spawn(command, args, { env, cwd, stdio: ['ignore', fd, fd], windowsHide: true });
    const done = (outcome) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ pid: child.pid ?? null, timedOut, error: null, ...outcome });
    };
    const timer = setTimeout(() => { timedOut = true; try { child.kill('SIGKILL'); } catch { /* already gone */ } }, deadlineMs);
    child.once('error', (error) => done({ status: null, signal: null, error: String(error?.message ?? error) }));
    child.once('exit', (status, signal) => done({ status, signal }));
  });
}

/** Default number of log lines a file-backed exec returns over the agent channel. */
export const LOG_TAIL_LINES = 200;

/**
 * The last n lines of a file, reading at most maxBytes from its end; the rest of the file is never
 * loaded. When the read starts mid-file its first (possibly partial) line is dropped.
 */
export function tailOfFile(file, n = LOG_TAIL_LINES, maxBytes = 512 * 1024) {
  const fd = openSync(file, 'r');
  try {
    const bytes = fstatSync(fd).size;
    const len = Math.min(bytes, maxBytes);
    const buf = Buffer.alloc(len);
    if (len > 0) readSync(fd, buf, 0, len, bytes - len);
    const lines = buf.toString('utf8').split(/\r?\n/);
    if (lines.at(-1) === '') lines.pop();
    if (len < bytes) lines.shift();
    return { tail: lines.slice(-n).join('\n'), bytes };
  } finally {
    closeSync(fd);
  }
}

function psQuote(value) {
  return `'${String(value).replace(/'/g, "''")}'`;
}

/**
 * Windows probe script. CreationTime FILETIME (~1.34e17) exceeds 2^53: emitted as a JSON string
 * so JSON.parse never rounds it (owner.json records the exact u64; equality must be exact).
 */
export function windowsProbeScript(pid) {
  return `$p = Get-Process -Id ${Number(pid)} -ErrorAction SilentlyContinue; if ($p) { [pscustomobject]@{ path = $p.Path; start = [string]$p.StartTime.ToFileTimeUtc() } | ConvertTo-Json -Compress }`;
}

/** Parses windowsProbeScript stdout; a non-string or non-decimal start is refused, never rounded. */
export function parseWindowsProbe(stdout) {
  const text = String(stdout ?? '').trim();
  if (!text) return null;
  const v = JSON.parse(text);
  if (!v.path) return null;
  if (typeof v.start !== 'string' || !/^\d+$/.test(v.start)) throw new Error(`windows probe start is not an exact decimal string: ${JSON.stringify(v.start)}`);
  return { executable: v.path, startTime: v.start };
}

/** { executable, startTime } of a live pid, or null when it is gone. exec is injectable for tests. */
export async function probeIdentity(pid, platform, { exec = run } = {}) {
  if (platform === 'linux') {
    try {
      const executable = readlinkSync(`/proc/${pid}/exe`).replace(/ \(deleted\)$/, '');
      const stat = readFileSync(`/proc/${pid}/stat`, 'utf8');
      const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
      return { executable, startTime: fields[19] }; // field 22 overall = starttime ticks
    } catch {
      return null;
    }
  }
  if (platform === 'mac') {
    const r = await run('/bin/ps', ['-o', 'lstart=,comm=', '-p', String(pid)], { deadlineMs: 10_000 });
    if (r.status !== 0 || !r.stdout.trim()) return null;
    const line = r.stdout.trim();
    const m = /^(\w{3} \w{3}\s+\d+ [\d:]+ \d{4})\s+(.*)$/.exec(line);
    if (!m) return null;
    return { executable: m[2], startTime: m[1] };
  }
  if (platform === 'windows') {
    const r = await exec('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', windowsProbeScript(pid)], { deadlineMs: 20_000 });
    return parseWindowsProbe(r.stdout);
  }
  throw new Error(`unknown platform ${platform}`);
}

export async function signalPid(pid, sig, platform) {
  if (platform === 'windows') {
    await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `Stop-Process -Id ${Number(pid)} -Force`], { deadlineMs: 20_000 });
    return;
  }
  process.kill(pid, sig);
}

/**
 * Subscribes to the exit of a pid this process did not spawn. Resolve() of the returned promise
 * happens when the OS waiter exits. Call before acting, await after.
 */
export function subscribeExit(pid, platform) {
  let command;
  let args;
  if (platform === 'mac') { command = '/usr/bin/caffeinate'; args = ['-w', String(pid)]; }
  else if (platform === 'linux') { command = 'tail'; args = [`--pid=${pid}`, '-f', '/dev/null']; }
  else { command = 'powershell.exe'; args = ['-NoProfile', '-NonInteractive', '-Command', `Wait-Process -Id ${Number(pid)} -ErrorAction SilentlyContinue`]; }
  const waiter = spawn(command, args, { stdio: 'ignore', windowsHide: true });
  const exited = new Promise((resolve, reject) => {
    waiter.once('error', reject);
    waiter.once('exit', () => resolve({ pid, exitedAt: Date.now() }));
  });
  return { exited, cancel: () => { try { waiter.kill(); } catch { /* already gone */ } } };
}

/** Process table rows { pid, ppid, executable, commandLine } with no name matching. */
export async function processTable(platform) {
  if (platform === 'windows') {
    const script = "Get-CimInstance Win32_Process | ForEach-Object { [ordered]@{ pid = [int]$_.ProcessId; ppid = [int]$_.ParentProcessId; executable = [string]$_.ExecutablePath; commandLine = [string]$_.CommandLine } } | ConvertTo-Json -Compress -Depth 3";
    const r = await run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { deadlineMs: 60_000 });
    const v = JSON.parse(r.stdout.trim() || '[]');
    return (Array.isArray(v) ? v : [v]).map((x) => ({ pid: x.pid, ppid: x.ppid, executable: x.executable || null, commandLine: x.commandLine || '' }));
  }
  const r = await run('/bin/ps', ['-Ao', 'pid=,ppid=,command='], { deadlineMs: 20_000 });
  const rows = [];
  for (const line of r.stdout.split('\n')) {
    const m = /^\s*(\d+)\s+(\d+)\s+(.*)$/.exec(line);
    if (!m) continue;
    const pid = Number(m[1]);
    let executable = null;
    if (platform === 'linux') {
      try { executable = readlinkSync(`/proc/${pid}/exe`).replace(/ \(deleted\)$/, ''); } catch { executable = null; }
    } else {
      const argv0 = m[3].trim();
      executable = argv0.startsWith('/') ? argv0 : null;
    }
    rows.push({ pid, ppid: Number(m[2]), executable, commandLine: m[3] });
  }
  return rows;
}

/** Rows whose executable sits under any of the given roots (prefix by directory). */
export function rowsUnder(rows, roots, platform) {
  const n = (p) => (platform === 'windows' ? String(p).toLowerCase() : String(p));
  return rows.filter((r) => r.executable && roots.some((root) => n(r.executable).startsWith(n(root))));
}

export { psQuote };
