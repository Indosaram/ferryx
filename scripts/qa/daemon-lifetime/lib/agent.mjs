#!/usr/bin/env node
// Host agent for the daemon-lifetime QA harness. run.mjs (the controller, on this Mac) starts one
// agent per target host: `node agent.mjs --platform <p> --run-id <id> --run-dir <dir> --allow-root <dir>...`
// locally, or over SSH on omarchy and maho-win. The agent speaks one JSON object per line on
// stdio: requests { id, method, params } and replies { id, ok, result | error }. Every OS-job
// command passes the guard, every kill goes through the ProcessRegistry, and every file write is
// confined to the run dir.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, watch, writeFileSync, appendFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { GuardError, ProcessRegistry, isInside, assertRunId } from './guards.mjs';
import { run, probeIdentity, signalPid, subscribeExit, processTable, withDeadline } from './proc.mjs';
import { connectData, connectControl } from './client.mjs';

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

export function createAgent({ platform, runId, runDir, allowRoots = [] }) {
  const ctx = { runId, platform };
  const roots = [runDir, ...allowRoots];
  const registry = new ProcessRegistry({ platform, probe: (pid) => probeIdentity(pid, platform), signal: (pid, sig) => signalPid(pid, sig, platform) });
  const handles = new Map();
  const children = new Map();
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
      return registry.record(child.pid, { role, executable });
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
      const route = (m) => {
        if (m.type !== 'output') return;
        const sid = m.sessionId ?? '*';
        output.set(sid, (output.get(sid) ?? '') + Buffer.from(m.data, 'base64').toString());
        for (const w of [...(handles.get(handle)?.waiters ?? [])]) {
          const text = output.get(w.sessionId) ?? output.get('*') ?? '';
          if (text.includes(w.marker)) { handles.get(handle).waiters.delete(w); w.resolve(text); }
        }
      };
      conn.onStream(route);
      handles.set(handle, { conn, kind: 'data', output, waiters: new Set(), runtimeDir, version, route, streams: new Map() });
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
        h.output.set(frame.sessionId, Buffer.from(resp.history ?? '', 'base64').toString() + prior);
      }
      return resp;
    },
    waitOutput: async ({ handle, sessionId, marker, deadlineMs = 15_000 }) => {
      const h = handles.get(handle);
      const text = () => h.output.get(sessionId) ?? h.output.get('*') ?? '';
      if (text().includes(marker)) return { text: text(), count: text().split(marker).length - 1 };
      try {
        await withDeadline(new Promise((resolve) => h.waiters.add({ sessionId, marker, resolve })), deadlineMs, `output ${marker}`);
      } catch {
        return { timedOut: true, text: text(), count: text().split(marker).length - 1 };
      }
      return { text: text(), count: text().split(marker).length - 1 };
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
