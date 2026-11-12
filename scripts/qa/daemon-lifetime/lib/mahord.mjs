// Minimal MCP stdio client for MahoRD (maho-client --host <ip> --pairing-id <id> --mcp). The only
// way the harness touches a remote GUI (omarchy, maho-win). Tool names match the adapters already
// used by scripts/qa/fixtures/native-configured-adapters.mjs.

import { spawn } from 'node:child_process';
import { statSync } from 'node:fs';
import { createInterface } from 'node:readline';
import { hjoin } from './hostpath.mjs';
import { withDeadline } from './proc.mjs';

export const TOOLS = Object.freeze({ click: 'remote_mouse_click', type: 'remote_type_text', key: 'remote_key_press', hotkey: 'remote_hotkey', screenshot: 'remote_take_screenshot' });

export function mahordArgs({ host, pairingId }) {
  return ['--host', host, '--pairing-id', pairingId, '--mcp'];
}

/**
 * A MahoRD client that can be replaced: reconnect() closes the current maho-client and opens a
 * fresh session. A host-side session can die (heartbeat timeout) while every tool call still
 * reports "Dispatched successfully" (only3 Q13: host-run.log 07:39:53 heartbeat timeout, then
 * g15/g16 Win+R reached nothing and both failure screenshots were the same stale frame).
 */
// MahoRD was renamed: its host is ferryx-rd-host and its client is ferryx-rd-client. The harness
// kept shelling out to the retired 'maho-client' name, which no longer exists on maho-mac at all
// (NO_MAHO_CLIENT) and is off PATH on omarchy (ENOENT), so Q11 could never type into a Mac GUI.
// ferryx-rd-client already exposes the same MCP surface (--mcp, --host, --pairing-id, and the
// remote_* tool names), so prefer it and keep maho-client only as a pre-rename fallback.
const RENAMED_CLIENT = 'ferryx-rd-client';
const LEGACY_CLIENT = 'maho-client';

function hasBinary(binary, env = process.env) {
  // PATH scan rather than Bun.which: the harness must behave the same when a caller imports this
  // module from plain node for its unit tests.
  const exts = process.platform === 'win32' ? String(env.PATHEXT ?? '.EXE;.CMD').split(';') : [''];
  for (const dir of String(env.PATH ?? '').split(process.platform === 'win32' ? ';' : ':')) {
    if (!dir) continue;
    for (const ext of exts) {
      try {
        if (statSync(hjoin(dir, binary + ext)).isFile()) return true;
      } catch {
        // not here
      }
    }
  }
  return false;
}

/**
 * Where a user-installed RD client usually lands but a service manager's PATH does not carry:
 * systemd --user and an ssh command both start with a minimal PATH, so a binary at
 * ~/.local/bin is invisible to a plain PATH scan (omarchy 2026-11-10: ENOENT for both names
 * while /home/indo/.local/bin/maho-client existed and was executable).
 */
function installDirs(env = process.env) {
  const home = env.HOME ?? '';
  return [
    home && hjoin(home, '.local/bin'),
    '/usr/local/bin',
    '/opt/homebrew/bin',
    home && hjoin(home, 'bin'),
  ].filter(Boolean);
}

/** Absolute path of an RD client that exists on this host, or null. */
export function findMahordBinary(env = process.env) {
  for (const name of [RENAMED_CLIENT, LEGACY_CLIENT]) {
    if (hasBinary(name, env)) return name;
    for (const dir of installDirs(env)) {
      try {
        if (statSync(hjoin(dir, name)).isFile()) return hjoin(dir, name);
      } catch {
        // not here
      }
    }
  }
  return null;
}

/**
 * The RD client to drive a remote GUI, preferring the shipped name over the retired one.
 * Returns null when the host has neither, so the caller reports an actionable blocker instead
 * of spawning a name that cannot resolve.
 */
export function resolveMahordBinary(explicit, env = process.env) {
  if (explicit) return explicit;
  if (env.FERRYX_RD_CLIENT) return env.FERRYX_RD_CLIENT;
  return findMahordBinary(env);
}

export async function connectMahord(gui, { binary = resolveMahordBinary(), log = () => {}, open = openMahord } = {}) {
  if (!binary) {
    throw new Error(
      'no RD client on PATH: install ferryx-rd-client (or the pre-rename maho-client) and retry; '
        + 'set FERRYX_RD_CLIENT to an absolute path to override',
    );
  }
  let current = await open(gui, { binary, log });
  let reconnects = 0;
  const via = (name) => (...args) => current[name](...args);
  return {
    click: via('click'), type: via('type'), key: via('key'), hotkey: via('hotkey'), screenshot: via('screenshot'),
    reconnect: async () => {
      await current.close().catch(() => {});
      current = await open(gui, { binary, log });
      reconnects += 1;
      log({ mahordReconnect: reconnects });
      return reconnects;
    },
    close: () => current.close(),
  };
}

async function openMahord(gui, { binary, log }) {
  if (gui.kind !== 'mahord') throw new Error(`not a MahoRD host: ${gui.kind}`);
  const child = spawn(binary, mahordArgs(gui), { stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  let nextId = 1;
  createInterface({ input: child.stdout }).on('line', (line) => {
    let msg;
    try { msg = JSON.parse(line); } catch { return; }
    const p = pending.get(msg.id);
    if (!p) return;
    pending.delete(msg.id);
    if (msg.error) p.reject(new Error(JSON.stringify(msg.error))); else p.resolve(msg.result);
  });
  child.stderr.on('data', (d) => log({ mahordStderr: String(d) }));
  const exited = new Promise((resolve) => child.once('exit', (code) => {
    for (const p of pending.values()) p.reject(new Error(`maho-client exited ${code}`));
    resolve(code);
  }));
  // Shared hosts hit build bursts that stall the RD encode loop for tens of seconds; a 30s
  // deadline turned that into a hard Q00 failure (run 20261111t050123z). 90s rides them out.
  const rpc = (method, params, deadlineMs = 90_000) => {
    const id = nextId++;
    const p = new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return withDeadline(p, deadlineMs, `mahord ${method}`);
  };
  // A host still tearing down a previous session answers EOF for ~6.5s; retry the handshake.
  let lastError;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      await rpc('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'ferryx-daemon-lifetime-qa', version: '1' } });
      lastError = null;
      break;
    } catch (error) {
      lastError = error;
    }
  }
  if (lastError) throw lastError;
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  const call = async (name, args) => {
    const result = await rpc('tools/call', { name, arguments: args });
    log({ mahord: name, args });
    return result;
  };
  return {
    // A timed-out action is unknown, never assumed delivered; callers verify its effect.
    click: (x, y) => call(TOOLS.click, { x, y, button: 'left', count: 1, normalized: false }),
    type: (text) => call(TOOLS.type, { text, delay_ms: 0 }),
    key: (key) => call(TOOLS.key, { key }),
    // Chords: remote_key_press accepts a single key name only (maho-client tools/list schema).
    hotkey: (keys) => call(TOOLS.hotkey, { keys }),
    screenshot: async () => {
      const r = await call(TOOLS.screenshot, { format: 'png' });
      const img = r?.content?.find((c) => c.type === 'image');
      return img ? Buffer.from(img.data, 'base64') : null;
    },
    close: async () => { child.stdin.end(); await exited; },
  };
}
