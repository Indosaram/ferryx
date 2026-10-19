// Controller side of the host agent: starts it locally or over SSH and exposes call(method, params).

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { withDeadline } from './proc.mjs';

export function agentCommand(host, { agentPath, runId, runDir, allowRoots = [] }) {
  const args = [agentPath, '--platform', host.platform, '--run-id', runId, '--run-dir', runDir];
  for (const r of allowRoots) args.push('--allow-root', r);
  if (host.transport.kind === 'local') return { command: host.node, args };
  const quote = (s) => (host.platform === 'windows' ? `"${String(s).replace(/"/g, '\\"')}"` : `'${String(s).replace(/'/g, "'\\''")}'`);
  const remote = [host.node, ...args].map(quote).join(' ');
  return { command: 'ssh', args: ['-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', '-o', 'ServerAliveInterval=15', host.transport.target, host.platform === 'windows' ? `& ${remote}` : remote] };
}

export async function startAgent(host, opts, { log = () => {} } = {}) {
  const { command, args } = agentCommand(host, opts);
  const child = spawn(command, args, { stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  let nextId = 1;
  let ready;
  const readyPromise = new Promise((resolve, reject) => { ready = { resolve, reject }; });
  const rl = createInterface({ input: child.stdout });
  rl.on('line', (line) => {
    let msg;
    try { msg = JSON.parse(line); } catch { log({ agentNoise: line }); return; }
    if (msg.id === 0) return ready.resolve(msg.result);
    const p = pending.get(msg.id);
    if (!p) return;
    pending.delete(msg.id);
    if (msg.ok) p.resolve(msg.result);
    else { const e = new Error(msg.error.message); e.code = msg.error.code; p.reject(e); }
  });
  child.stderr.on('data', (d) => log({ agentStderr: String(d) }));
  const exited = new Promise((resolve) => child.once('exit', (code) => {
    for (const p of pending.values()) p.reject(new Error(`agent exited ${code}`));
    pending.clear();
    ready.reject(new Error(`agent exited ${code} before ready`));
    resolve(code);
  }));
  const info = await withDeadline(readyPromise, 60_000, `agent on ${host.name}`);
  const call = (method, params = {}, deadlineMs = 300_000) => {
    const id = nextId++;
    const p = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
    child.stdin.write(`${JSON.stringify({ id, method, params })}\n`);
    return withDeadline(p, deadlineMs, `agent ${method}`);
  };
  return { info, call, stop: async () => { child.stdin.end(); await exited; }, exited };
}
