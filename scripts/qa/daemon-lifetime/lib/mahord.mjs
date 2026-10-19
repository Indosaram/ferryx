// Minimal MCP stdio client for MahoRD (maho-client --host <ip> --pairing-id <id> --mcp). The only
// way the harness touches a remote GUI (omarchy, maho-win). Tool names match the adapters already
// used by scripts/qa/fixtures/native-configured-adapters.mjs.

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { withDeadline } from './proc.mjs';

export const TOOLS = Object.freeze({ click: 'remote_mouse_click', type: 'remote_type_text', key: 'remote_key_press', hotkey: 'remote_hotkey', screenshot: 'remote_take_screenshot' });

export function mahordArgs({ host, pairingId }) {
  return ['--host', host, '--pairing-id', pairingId, '--mcp'];
}

export async function connectMahord(gui, { binary = 'maho-client', log = () => {} } = {}) {
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
  const rpc = (method, params, deadlineMs = 30_000) => {
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
