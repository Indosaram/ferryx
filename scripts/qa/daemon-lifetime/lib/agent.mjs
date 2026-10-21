#!/usr/bin/env node
// Host agent for the daemon-lifetime QA harness. run.mjs (the controller, on this Mac) starts one
// agent per target host: `node agent.mjs --platform <p> --run-id <id> --run-dir <dir> --allow-root <dir>...`
// locally, or over SSH on omarchy and maho-win. The agent speaks one JSON object per line on
// stdio: requests { id, method, params } and replies { id, ok, result | error }. Every OS-job
// command passes the guard, every kill goes through the ProcessRegistry, and every file write is
// confined to the run dir.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, realpathSync, rmSync, watch, writeFileSync, appendFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { createInterface } from 'node:readline';
import os from 'node:os';
import { GuardError, ProcessRegistry, isInside, assertRunId } from './guards.mjs';
import { run, runToFd, tailOfFile, LOG_TAIL_LINES, probeIdentity, signalPid, subscribeExit, processTable, withDeadline } from './proc.mjs';
import { connectData, connectControl } from './client.mjs';
import { data as D, DsrResponder, DSR_REPLY, visibleText } from './wire.mjs';
import { findSshHelpers, sshHostId } from './sshd.mjs';

/**
 * An empty marker waits for any output; with visible, for output a person would see (a prompt,
 * not a bare terminal query such as ConPTY's ESC[6n); otherwise a substring.
 */
export function outputMatches(text, marker, visible = false) {
  if (marker) return text.includes(marker);
  return visible ? visibleText(text).trim().length > 0 : text.length > 0;
}

/**
 * The stream handler for one data handle: stores output per session, answers ConPTY's cursor
 * query on Windows (one reply per query, sent as a data-protocol write on the request
 * connection), and wakes waiters. A failed reply write is kept in replyErrors, never dropped.
 */
export function createOutputRoute({ platform, output, waiters, responder = new DsrResponder(), send }) {
  const replyErrors = [];
  /** Answers the queries owed for a live frame at sequence, or for attach history (history: true). */
  const answer = (sessionId, text, sequence, { history = false } = {}) => {
    if (platform !== 'windows' || !sessionId) return 0;
    const owed = history ? responder.feedHistory(sessionId, text, sequence ?? null) : responder.feed(sessionId, text, sequence ?? null);
    for (let i = 0; i < owed; i += 1) {
      Promise.resolve(send(D.write(sessionId, DSR_REPLY))).catch((error) => replyErrors.push({ sessionId, error: String(error?.message ?? error) }));
    }
    return owed;
  };
  const route = (m) => {
    if (m.type !== 'output') return;
    const sid = m.sessionId ?? '*';
    const text = Buffer.from(m.data, 'base64').toString();
    output.set(sid, (output.get(sid) ?? '') + text);
    answer(m.sessionId, text, m.sequence);
    for (const w of [...waiters]) {
      const all = (output.get(w.sessionId) ?? output.get('*') ?? '').slice(w.since ?? 0);
      if (outputMatches(all, w.marker, w.visible)) { waiters.delete(w); w.resolve(all); }
    }
  };
  return { route, answer, responder, replyErrors };
}

export function parseAgentArgs(argv) {
  const out = { platform: null, runId: null, runDir: null, allowRoots: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--platform') out.platform = argv[++i];
    else if (a === '--run-id') out.runId = argv[++i];
    else if (a === '--run-dir') out.runDir = argv[++i];
    else if (a === '--allow-root') out.allowRoots.push(argv[++i]);
    else throw new Error(`unknown agent argument ${a}`);
  }
  if (!['mac', 'linux', 'windows'].includes(out.platform)) throw new Error('agent needs --platform mac|linux|windows');
  assertRunId(out.runId);
  if (!out.runDir) throw new Error('agent needs --run-dir');
  return out;
}

export function createAgent({ platform, runId, runDir, allowRoots = [], home = os.homedir(), table = () => processTable(platform), probe = (pid) => probeIdentity(pid, platform), signal = (pid, sig) => signalPid(pid, sig, platform) }) {
  const ctx = { runId, platform };
  const roots = [runDir, ...allowRoots];
  const registry = new ProcessRegistry({ platform, probe, signal });
  // Remote-helper daemons of this run's private sshd: pid (in the registry) and exact root dir.
  const sshHelpers = new Map();
  const handles = new Map();
  const children = new Map();
  // One cursor-query responder for every data handle: a session re-attached from a second
  // connection must not be answered again for a query the first one already answered.
  const dsr = new DsrResponder();
  let nextHandle = 1;

  const inRun = (p) => {
    if (!roots.some((root) => isInside(p, root, platform))) throw new GuardError('PATH_OUTSIDE_RUN', `${p} is outside ${roots.join(', ')}`);
    return p;
  };

  const methods = {
    ping: async () => ({ platform, runId, pid: process.pid, node: process.version }),
    exec: async ({ command, args = [], env, cwd, input = null, deadlineMs = 120_000 }) => {
      const r = await run(command, args, { ctx, env: env ? { ...process.env, ...env } : undefined, cwd, input, deadlineMs });
      return { status: r.status, stdout: r.stdout, stderr: r.stderr };
    },
    // Long, chatty commands (QA builds): stdout and stderr go straight to logFile on this host and
    // the real exit (status, signal, timedOut) is written to the statusFile sidecar. Only the exit,
    // the byte count and a bounded tail come back over the agent channel; the controller copies
    // the full log back as a file (run.mjs runBuildStep).
    execLogged: async ({ command, args = [], env, cwd, logFile, statusFile, tailLines = LOG_TAIL_LINES, deadlineMs = 120_000 }) => {
      inRun(logFile);
      inRun(statusFile);
      mkdirSync(path.dirname(logFile), { recursive: true });
      mkdirSync(path.dirname(statusFile), { recursive: true });
      const startedAt = new Date().toISOString();
      const fd = openSync(logFile, 'w');
      let outcome;
      try {
        outcome = await runToFd(command, args, { ctx, env: env ? { ...process.env, ...env } : undefined, cwd, fd, deadlineMs });
      } finally {
        closeSync(fd);
      }
      const { tail, bytes } = tailOfFile(logFile, tailLines);
      const record = { command, args, cwd: cwd ?? null, ...outcome, startedAt, finishedAt: new Date().toISOString(), logFile, bytes };
      writeFileSync(statusFile, `${JSON.stringify(record, null, 2)}\n`);
      return { status: outcome.status, signal: outcome.signal, timedOut: outcome.timedOut, error: outcome.error, pid: outcome.pid, bytes, tail, logFile, statusFile };
    },
    // Spawns a long-lived process this run owns, records it by pid + executable, and streams its
    // exit as an event. stdout lines are appended to logFile (inside the run dir).
    // record:false defers identity recording (a launcher that execs the real program); the caller
    // records the final pid with `record` once it is ready.
    spawn: async ({ command, args = [], env, cwd, role, executable = command, logFile, record = true }) => {
      inRun(logFile);
      mkdirSync(path.dirname(logFile), { recursive: true });
      const child = spawn(command, args, { env: { ...process.env, ...(env ?? {}) }, cwd, stdio: ['pipe', 'pipe', 'pipe'], detached: platform !== 'windows', windowsHide: false });
      const exited = new Promise((resolve) => child.once('exit', (code, signal) => resolve({ code, signal, at: Date.now() })));
      child.stdout.on('data', (d) => appendFileSync(logFile, d));
      child.stderr.on('data', (d) => appendFileSync(logFile, d));
      await new Promise((resolve, reject) => { child.once('spawn', resolve); child.once('error', reject); });
      children.set(child.pid, { child, exited });
      if (!record) return { pid: child.pid, role, recorded: false };
      // The OS reports the resolved image (/proc/<pid>/exe): on Arch /usr/sbin is a symlink to
      // /usr/bin, so sshd launched as /usr/sbin/sshd runs as /usr/bin/sshd. Record the canonical path.
      return registry.record(child.pid, { role, executable: existsSync(executable) ? realpathSync(executable) : executable });
    },
    // Writes one line to a spawned child's stdin (launcher control on Windows).
    stdin: async ({ pid, line }) => { children.get(pid)?.child.stdin?.write(`${line}\n`); return true; },
    // Records a pid this run caused to exist (e.g. the daemon started by the QA job), after
    // checking it runs an executable under the run's roots.
    record: async ({ pid, role, executable }) => {
      inRun(executable);
      return registry.record(pid, { role, executable });
    },
    check: async ({ pid }) => registry.check(pid),
    kill: async ({ pid, signal = 'SIGTERM' }) => registry.kill(pid, signal),
    recorded: async () => registry.list(),
    probe: async ({ pid }) => probeIdentity(pid, platform),
    table: async () => processTable(platform),
    // Resolves when pid exits (OS waiter event), or with { timedOut: true } after deadlineMs.
    waitExit: async ({ pid, deadlineMs = 60_000 }) => {
      const own = children.get(pid);
      const sub = own ? { exited: own.exited, cancel: () => {} } : subscribeExit(pid, platform);
      try {
        return await withDeadline(sub.exited, deadlineMs, `exit of ${pid}`);
      } catch {
        sub.cancel();
        return { timedOut: true };
      }
    },
    // Resolves when the file exists (fs.watch on its directory, checked once after subscribing).
    waitPath: async ({ file, deadlineMs = 30_000 }) => {
      const dir = path.dirname(file);
      mkdirSync(dir, { recursive: true });
      let watcher;
      const appeared = new Promise((resolve) => {
        watcher = watch(dir, () => { if (existsSync(file)) resolve(true); });
        if (existsSync(file)) resolve(true);
      });
      try {
        return await withDeadline(appeared, deadlineMs, `path ${file}`);
      } catch {
        return false;
      } finally {
        watcher?.close();
      }
    },
    writeFile: async ({ file, content, mode }) => { inRun(file); mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, content, mode ? { mode } : undefined); return true; },
    readFile: async ({ file }) => (existsSync(file) ? readFileSync(file, 'utf8') : null),
    exists: async ({ file }) => existsSync(file),
    mkdir: async ({ dir, mode }) => { inRun(dir); mkdirSync(dir, { recursive: true, mode }); return true; },
    remove: async ({ target }) => { inRun(target); rmSync(target, { recursive: true, force: true }); return !existsSync(target); },
    sha256: async ({ file }) => createHash('sha256').update(readFileSync(file)).digest('hex'),
    stat: async ({ file }) => { const s = statSync(file); return { size: s.size, mtimeMs: s.mtimeMs }; },
    // ---- daemon clients; each connection is a handle ----
    dataOpen: async ({ runtimeDir, version = 5, allowMismatch = false }) => {
      const { conn, handshake } = await connectData(runtimeDir, platform, { version, allowMismatch });
      const handle = nextHandle++;
      const output = new Map();
      const waiters = new Set();
      const { route, answer, responder, replyErrors } = createOutputRoute({ platform, output, waiters, responder: dsr, send: (frame) => conn.call(frame) });
      conn.onStream(route);
      handles.set(handle, { conn, kind: 'data', output, waiters, responder, replyErrors, answer, runtimeDir, version, route, streams: new Map() });
      return { handle, handshake };
    },
    controlOpen: async ({ runtimeDir }) => {
      const conn = await connectControl(runtimeDir, platform);
      if (!conn) return { handle: null };
      const handle = nextHandle++;
      handles.set(handle, { conn, kind: 'control' });
      return { handle };
    },
    call: async ({ handle, frame, deadlineMs = 30_000 }) => {
      const h = handles.get(handle);
      if (!h) throw new Error(`no handle ${handle}`);
      // The daemon turns a data connection into an output stream once it answers Attach, and closes
      // it on any further client bytes (server.rs pump_session_stream). Each attach therefore gets
      // its own stream connection, routed into this handle's output; requests stay on h.conn.
      let resp;
      if (h.kind === 'data' && frame?.type === 'attach') {
        h.streams.get(frame.sessionId)?.close();
        const { conn: stream } = await connectData(h.runtimeDir, platform, { version: h.version });
        stream.onStream(h.route);
        h.streams.set(frame.sessionId, stream);
        resp = await stream.call(frame, deadlineMs);
      } else {
        resp = await h.conn.call(frame, deadlineMs);
      }
      if (h.kind === 'data' && resp?.type === 'attachOk' && frame.sessionId) {
        const prior = h.output.get(frame.sessionId) ?? '';
        const history = Buffer.from(resp.history ?? '', 'base64').toString();
        h.output.set(frame.sessionId, history + prior);
        h.answer(frame.sessionId, history, resp.endSequence ?? null, { history: true });
        // A visible-output waiter may already be satisfied by the history just stored.
        for (const w of [...h.waiters]) {
          const all = h.output.get(w.sessionId) ?? h.output.get('*') ?? '';
          if (outputMatches(all, w.marker, w.visible)) { h.waiters.delete(w); w.resolve(all); }
        }
      }
      return resp;
    },
    // since: match only output after that many characters (outputOf(...).length taken earlier).
    waitOutput: async ({ handle, sessionId, marker, visible = false, since = 0, deadlineMs = 15_000 }) => {
      const h = handles.get(handle);
      const text = () => (h.output.get(sessionId) ?? h.output.get('*') ?? '').slice(since);
      const extra = () => ({ dsrReplies: h.responder.replies(sessionId), replyErrors: h.replyErrors.filter((e) => e.sessionId === sessionId) });
      if (outputMatches(text(), marker, visible)) return { text: text(), count: marker ? text().split(marker).length - 1 : 0, ...extra() };
      try {
        await withDeadline(new Promise((resolve) => h.waiters.add({ sessionId, marker, visible, since, resolve })), deadlineMs, `output ${marker}`);
      } catch {
        return { timedOut: true, text: text(), count: marker ? text().split(marker).length - 1 : 0, ...extra() };
      }
      return { text: text(), count: marker ? text().split(marker).length - 1 : 0, ...extra() };
    },
    outputOf: async ({ handle, sessionId }) => handles.get(handle)?.output.get(sessionId) ?? '',
    // Resolves when the file's content matches pattern (fs.watch on its directory).
    waitLog: async ({ file, pattern, flags = '', deadlineMs = 30_000 }) => {
      const re = new RegExp(pattern, flags);
      const dir = path.dirname(file);
      mkdirSync(dir, { recursive: true });
      const read = () => (existsSync(file) ? readFileSync(file, 'utf8') : '');
      let watcher;
      const found = new Promise((resolve) => {
        const test = () => { const m = re.exec(read()); if (m) resolve(m[0]); };
        watcher = watch(dir, test);
        test();
      });
      try {
        return { matched: await withDeadline(found, deadlineMs, `log ${pattern}`) };
      } catch {
        return { matched: null, timedOut: true };
      } finally {
        watcher?.close();
      }
    },
    // Resolves when <runtimeDir>/owner.json names a pid other than fromPid; reports every pid seen.
    waitOwnerChange: async ({ runtimeDir, fromPid, deadlineMs = 60_000 }) => {
      const file = path.join(runtimeDir, 'owner.json');
      const seen = [];
      const read = () => { try { return JSON.parse(readFileSync(file, 'utf8')); } catch { return null; } };
      let watcher;
      const changed = new Promise((resolve) => {
        const test = () => {
          const rec = read();
          if (rec && !seen.includes(rec.pid)) seen.push(rec.pid);
          if (rec && rec.pid !== fromPid) resolve(rec);
        };
        watcher = watch(runtimeDir, test);
        test();
      });
      try {
        return { owner: await withDeadline(changed, deadlineMs, 'owner change'), seen };
      } catch {
        return { owner: null, timedOut: true, seen };
      } finally {
        watcher?.close();
      }
    },
    // Resolves when the peer closes the connection (the daemon exited or dropped us).
    waitClosed: async ({ handle, deadlineMs = 60_000 }) => {
      const h = handles.get(handle);
      try { return await withDeadline(h.conn.closedPromise, deadlineMs, 'connection close'); } catch { return { timedOut: true }; }
    },
    close: async ({ handle }) => {
      const h = handles.get(handle);
      for (const s of h?.streams?.values() ?? []) s.close();
      h?.conn.close();
      handles.delete(handle);
      return true;
    },
    teardownRecorded: async () => {
      const out = { killed: [], refused: [], gone: [] };
      for (const entry of registry.list()) {
        try {
          const killed = await registry.kill(entry.pid, 'SIGTERM');
          (killed ? out.killed : out.gone).push(entry);
        } catch (error) {
          out.refused.push({ ...entry, reason: error.message });
        }
      }
      return out;
    },
    // Records the remote-helper daemon(s) started for this run's private sshd host id: the pid by
    // its exact executable, the root by its exact path. Only this run's host id is accepted.
    recordSshHelper: async ({ hostId }) => {
      if (hostId !== sshHostId(runId)) throw new GuardError('SSH_HELPER_NOT_OURS', `${hostId} is not this run's sshd host id`);
      const out = [];
      for (const h of findSshHelpers(await table(), { platform, home, hostId })) {
        try {
          if (!registry.has(h.pid)) await registry.record(h.pid, { role: 'ssh-helper', executable: h.executable });
        } catch (error) {
          if (error?.code !== 'PID_GONE') throw error;
        }
        sshHelpers.set(h.root, h.pid);
        out.push(h);
      }
      return out;
    },
    // After teardownRecorded: removes each recorded helper root once its recorded pid has exited.
    removeSshHelperRoots: async ({ deadlineMs = 15_000 } = {}) => {
      const out = { removed: [], kept: [] };
      for (const [root, pid] of sshHelpers) {
        const sub = subscribeExit(pid, platform);
        // The waiter's exit only wakes us; the pid's own probe decides that it is gone.
        if ((await probe(pid)) !== null) await withDeadline(sub.exited, deadlineMs, `exit of ssh helper ${pid}`).catch(() => {});
        sub.cancel();
        if ((await probe(pid)) !== null) { out.kept.push({ root, pid, reason: 'helper pid still running' }); continue; }
        rmSync(root, { recursive: true, force: true });
        (existsSync(root) ? out.kept : out.removed).push({ root, pid });
      }
      return out;
    },
  };
  return { methods, registry };
}

async function main() {
  const opts = parseAgentArgs(process.argv.slice(2));
  const { methods } = createAgent(opts);
  const rl = createInterface({ input: process.stdin });
  const reply = (obj) => process.stdout.write(`${JSON.stringify(obj)}\n`);
  reply({ id: 0, ok: true, result: { ready: true, pid: process.pid } });
  rl.on('line', async (line) => {
    let req;
    try { req = JSON.parse(line); } catch { return; }
    const fn = methods[req.method];
    if (!fn) return reply({ id: req.id, ok: false, error: { code: 'NO_METHOD', message: req.method } });
    try {
      reply({ id: req.id, ok: true, result: await fn(req.params ?? {}) });
    } catch (error) {
      reply({ id: req.id, ok: false, error: { code: error.code ?? 'ERROR', message: error.message } });
    }
  });
  rl.on('close', () => process.exit(0));
}

if (import.meta.url === `file://${process.argv[1]}` || process.argv[1]?.endsWith('agent.mjs')) {
  main().catch((error) => { process.stderr.write(`${error.stack}\n`); process.exit(2); });
}
