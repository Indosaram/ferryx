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

/**
 * The cliclick command that releases every modifier at once. cliclick's own usage says
 * `ku:keys` takes "a comma-separated list of modifier keys", and `cliclick -m test 'ku:alt,ku:cmd,...'`
 * rejects it with 'Invalid key "ku:cmd" given as argument to command "ku"' and exit 1, while
 * `ku:alt,cmd,ctrl,shift` prints one 'Release ... key' line per key and exits 0. One command, one
 * release per named key - the prefixes belong to the command identifier only.
 */
export const CLICLICK_RELEASE_KEYS = Object.freeze(['alt', 'cmd', 'ctrl', 'shift']);

/** cliclick argv for releasing the held modifiers, as `exec` receives it. */
export function cliclickReleaseArgv(keys = CLICLICK_RELEASE_KEYS) {
  return [`ku:${keys.join(',')}`];
}

/**
 * In the isolated maho-mac receiver, System Events' first plain `keystroke "u"` carried Option,
 * while cliclick's `t:u` did not. Both delivered U+3155 with the two-set Hangul input source.
 * Choosing cliclick avoids the observed modifier, not the layout or terminal-focus requirements.
 */
export function cliclickTypeArgv(text) {
  const s = String(text ?? '');
  // Keep comma-containing text on the existing path; this adapter only routes the tested subset.
  if (!s || !/^[\x20-\x7e]+$/.test(s) || s.includes(',')) return null;
  return [`t:${s}`];
}

/**
 * Raised when a cliclick delivery may already have posted input, so re-sending would duplicate it.
 *
 * `exec` is proc.run, whose three outcomes are distinguishable at the call site:
 *  - a child that never started rejects with an errno in `error.code` (ENOENT, EACCES), so nothing
 *    reached the desktop and a System Events fallback is safe;
 *  - a child that started always resolves, with a numeric status, or null when it died from a signal;
 *  - a deadline rejects with a plain Error carrying no `code`.
 * Only the first outcome proves that nothing was delivered. A non-zero exit after cliclick had
 * already parsed and posted part of its argv, a signalled child, and a timed-out child all leave an
 * unknown amount of text typed, so they surface here instead of being retried.
 */
export class MacInputDeliveryError extends Error {
  constructor(detail) {
    super(`MAC_INPUT_DELIVERY_UNKNOWN: ${JSON.stringify(detail)}`);
    this.name = 'MacInputDeliveryError';
    this.code = 'MAC_INPUT_DELIVERY_UNKNOWN';
    this.detail = detail;
  }
}

/** Local macOS System Events input adapter for Q11 when gui.kind === 'mac-local'. */
export function createMacLocalInput({ exec }) {
  const osa = async (script) => exec('/usr/bin/osascript', ['-e', script]);
  const escapeAs = (text) => String(text).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  // Only a spawn failure proves cliclick posted nothing: proc.run attaches the errno ('ENOENT') to
  // that rejection, while a deadline rejects with a plain Error that has no `code`.
  const neverStarted = (error) => typeof error?.code === 'string' && error.code.length > 0;
  // Types one cliclick argv, or returns null when the binary never started so the caller may still
  // fall back. Any other failure is recorded and rethrown: replaying the text could double the input.
  const typeViaCliclick = async (argv) => {
    let outcome;
    try {
      outcome = await exec('/opt/homebrew/bin/cliclick', argv);
    } catch (error) {
      if (neverStarted(error)) return null;
      throw new MacInputDeliveryError({ argv, stage: 'exec', error: String(error?.message ?? error) });
    }
    if (outcome && outcome.status === 0) return outcome;
    throw new MacInputDeliveryError({
      argv,
      stage: 'exit',
      status: outcome?.status ?? null,
      stderr: String(outcome?.stderr ?? '').trim(),
    });
  };
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
    // Alternate text delivery is only for text that IS the intended literal character: an unmodified
    // printable character, or Shift+letter, whose product is that capital letter. Every other chord
    // keeps its real modifiers verbatim. Shift+1 means "!" and Shift+/ means "?"; typing `t:1` or
    // `t:/` would post the unshifted character, so those chords must stay chords.
    const key = String(k);
    const lower = mods.map((m) => String(m).toLowerCase());
    const shiftOnly = lower.length === 1 && lower[0] === 'shift';
    const literal = mods.length === 0
      ? key
      : (shiftOnly && /^[A-Za-z]$/.test(key) ? key.toUpperCase() : null);
    const argv = literal === null ? null : cliclickTypeArgv(literal);
    if (argv) {
      const delivered = await typeViaCliclick(argv);
      if (delivered) return delivered;
      // cliclick is not installed: nothing was sent, so System Events can still type the character.
    }
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
    // Release held modifiers separately: success here does not establish the flags of the next key.
    releaseModifiers: async () => {
      const keys = CLICLICK_RELEASE_KEYS;
      // cliclick posts real key-up CGEvents for every named modifier in one call.
      const viaCliclick = await exec('/opt/homebrew/bin/cliclick', cliclickReleaseArgv(keys)).catch(() => null);
      if (viaCliclick && viaCliclick.status === 0) return { ok: true, via: 'cliclick', keys };
      // Without cliclick, release them one script at a time: a name System Events rejects must not
      // hide the others. Report the failure that actually happened - the exit status and stderr of
      // the key-up that did not land - so focus.json shows which modifier stayed down instead of a
      // bare ok:false.
      const results = [];
      for (const name of keys) {
        let r = null;
        let failure = null;
        try {
          r = await osa(`tell application "System Events" to key up ${MOD_NAMES[name] ?? name}`);
        } catch (error) {
          failure = String(error?.detail ?? error?.message ?? error);
        }
        // proc.run resolves status null when the child died from a signal (its 'close' handler takes
        // the exit code only), so a key-up is successful only when the exit status IS 0: a null
        // status means the osascript was killed and never released the modifier.
        const ok = !failure && r?.status === 0;
        results.push({ name, ok, ...(ok ? {} : { status: r?.status ?? null, error: failure ?? (String(r?.stderr ?? '').trim() || null) }) });
      }
      return { ok: results.every((r) => r.ok), via: 'osascript', keys, results, cliclick: viaCliclick ? { status: viaCliclick.status, stderr: String(viaCliclick.stderr ?? '').trim() } : null };
    },
    type: async (text) => {
      const argv = cliclickTypeArgv(text);
      if (argv) {
        const delivered = await typeViaCliclick(argv);
        if (delivered) return delivered;
        // cliclick is not installed: nothing was sent, so System Events can still type the text.
      }
      return osa(`tell application "System Events" to keystroke "${escapeAs(text)}"`);
    },
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
