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
  // A client that dies at once must fail the handshake immediately, not sit until the RPC
  // deadline. Measured on maho-win 2026-11-18: the WinGet ferryx-rd-client.exe ships without its
  // FFmpeg DLLs, so it exits 0xC0000135 (STATUS_DLL_NOT_FOUND) in ~0 ms. 'exit' then fired before
  // the initialize entry existed in 'pending', so nothing was rejected and every run reported
  // 'deadline 90000ms: mahord initialize' - a transport-shaped message for a broken install.
  // A spawn-level failure (ENOENT, EACCES, bad image) emits 'error' and never 'exit'; that event
  // also had no listener, which is an unhandled error waiting to crash the run.
  let exitState = null;
  const exited = new Promise((resolve) => {
    child.once('error', (error) => {
      exitState = { error };
      for (const p of pending.values()) p.reject(new Error(`maho-client failed to start: ${error.message}`));
      resolve(null);
    });
    child.once('exit', (code, signal) => {
      exitState = { code, signal };
      for (const p of pending.values()) p.reject(new Error(`maho-client exited ${code ?? signal}`));
      resolve(code);
    });
  });
  const deathMessage = () => (exitState.error
    ? `maho-client failed to start: ${exitState.error.message}`
    : `maho-client exited ${exitState.code ?? exitState.signal} before answering`);
  // Shared hosts hit build bursts that stall the RD encode loop for tens of seconds; a 30s
  // deadline turned that into a hard Q00 failure (run 20261111t050123z). 90s rides them out.
  const rpc = (method, params, deadlineMs = 90_000) => {
    if (exitState) return Promise.reject(new Error(deathMessage()));
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

/** Local macOS System Events input adapter for Q11 when gui.kind === 'mac-local'. */
export function createMacLocalInput({ exec }) {
  const osa = async (script) => exec('/usr/bin/osascript', ['-e', script]);
  const escapeAs = (text) => String(text).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  // System Events only accepts the AppleScript modifier names; the chord vocabulary the scenarios
  // use is the cross-platform one (Meta/Control/Alt/Shift). Passing "meta down" straight through is
  // rejected by osascript, so Q10 never opened Settings on macOS (run 20261117t142623z).
  const MOD_NAMES = { meta: 'command', cmd: 'command', command: 'command', control: 'control', ctrl: 'control', alt: 'option', option: 'option', shift: 'shift' };
  // Named keys must be sent as key CODES. `keystroke "Tab"` types the four letters T-a-b; it never
  // presses the Tab key, so Q10's Tab/Space walk drove nothing and its opt-out marker was never
  // written (runs 20261117t234809z, and every earlier mac run). Every scenario key that is not a
  // single printable character belongs here; `Enter`/`Return` already did, which is why Q11 could
  // commit its line while its focus walk stayed inert.
  const KEY_CODES = {
    tab: 48,
    space: 49,
    escape: 53,
    enter: 36,
    return: 36,
    backspace: 51,
    delete: 117,
    forwarddelete: 117,
    arrowleft: 123,
    left: 123,
    arrowright: 124,
    right: 124,
    arrowdown: 125,
    down: 125,
    arrowup: 126,
    up: 126,
    home: 115,
    end: 119,
    pageup: 116,
    pagedown: 121,
  };
  const sendKey = async (k, mods = []) => {
    const using = mods.length ? ` using {${mods.map((m) => `${MOD_NAMES[String(m).toLowerCase()] ?? String(m).toLowerCase()} down`).join(', ')}}` : '';
    const code = KEY_CODES[String(k).toLowerCase()];
    if (code !== undefined) return osa(`tell application "System Events" to key code ${code}${using}`);
    return osa(`tell application "System Events" to keystroke "${escapeAs(k)}"${using}`);
  };
  return {
    // System Events' `click at` reports success without posting a real mouse event, so the native
    // terminal never received the click that sets has_focused_session() and Q11's keystrokes went
    // to a window that was frontmost but not focused. cliclick posts a genuine CGEvent; it is
    // present on maho-mac (/opt/homebrew/bin/cliclick). Fall back to System Events only when the
    // binary is missing so the adapter still works on a host without it.
    click: async (x, y) => {
      if (!Number.isFinite(x) || !Number.isFinite(y)) return { ok: false };
      const cx = Math.round(x);
      const cy = Math.round(y);
      const viaCliclick = await exec('/opt/homebrew/bin/cliclick', [`c:${cx},${cy}`]).catch(() => null);
      if (viaCliclick && viaCliclick.status === 0) return viaCliclick;
      return osa(`tell application "System Events" to click at {${cx}, ${cy}}`);
    },
    type: async (text) => osa(`tell application "System Events" to keystroke "${escapeAs(text)}"`),
    key: async (k) => sendKey(k),
    hotkey: async (keys) => {
      const mods = keys.slice(0, -1);
      const ch = keys.at(-1);
      return sendKey(ch, mods);
    },
    screenshot: async () => null,
    reconnect: async () => 0,
    close: async () => {},
  };
}
