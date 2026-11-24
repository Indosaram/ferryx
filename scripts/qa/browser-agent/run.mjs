// Browser-agent integrated QA harness (plan maho-browser-merge, T8 "Concrete QA contract").
//
//   bun scripts/qa/browser-agent/run.mjs --config <host.json> --scenario Q1|...|Q9|all --evidence <dir>
//
// Every scenario drives real product surfaces only: `ferryx-cli browser ...` over the app socket,
// the `browser pipe` NDJSON adapter, the `browser mcp` JSON-RPC adapter, a raw socket envelope for
// the token-only admission path, and native GUI input through MahoRD (consent cards are clicked in
// the real app). A scenario PASSES only when it recorded at least one assertion, every assertion
// passed, and no step was skipped. A step whose product surface does not exist is executed anyway
// and recorded as a failing assertion carrying the observed refusal, so the run cannot report a
// pass it did not earn.
//
// Exit code: 0 only if every selected scenario PASSED; 1 on any FAIL; 2 on usage/config errors.

import { createHash } from 'node:crypto';
import { appendFileSync, createReadStream, existsSync, mkdirSync, readFileSync, rmSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { connect as netConnect } from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { connectMahord } from '../daemon-lifetime/lib/mahord.mjs';
import { startFixtures, subscribeEvents, verifyPortsReleased, FRAME_SENTINEL, PASSWORD_SENTINEL } from './fixtures.mjs';
import { spawnPipe, createLineDecoder } from './protocol.mjs';
import {
  CARD_LAYOUT, approveButtonPoint, decodePng, locateConsentCard, measureActionButtons, launchGui, killPid,
  ensureDir, liveWindowRect, ocrCardWords, confirmActionRow, resolveTesseract,
} from './gui.mjs';
import {
  CODES, actGeneration, approvalRequestId, callerControllerListed, cliErrorCode as cliErrorCodeFromSteps,
  dialogResolvedOnce, errorCode, fieldRef, findRef, frameBytes, framesAreOpaque, mcpToolError,
  noPasswordValue, noUploadsRecorded, pngSize, refsNamed,
  rpcErrorCode, scenarioStatus, sha256, textPostcondition, uploadMatches,
} from './steps.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '../../..');
export const SCENARIOS = Object.freeze(['Q1', 'Q2', 'Q3', 'Q4', 'Q5', 'Q6', 'Q7', 'Q8', 'Q9']);
export const ENGINES = Object.freeze(['webview', 'chromium']);
/** Largest pipe/MCP frame the app accepts (`browser/agent/pipe.rs::MAX_FRAME_BYTES`). */
export const MAX_FRAME_BYTES = 1024 * 1024;
const REDACT_KEYS = /^(secret|token|password|controllerauth|authorization|credential)s?$/i;

export class UsageError extends Error {}

export function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const [flag, inline] = arg.includes('=') ? arg.split(/=(.*)/s, 2) : [arg, undefined];
    if (!['--config', '--scenario', '--evidence'].includes(flag)) throw new UsageError(`unknown argument ${arg}`);
    const value = inline ?? argv[++i];
    if (value === undefined || value.startsWith('--')) throw new UsageError(`${flag} needs a value`);
    out[flag.slice(2)] = value;
  }
  for (const key of ['config', 'scenario', 'evidence']) if (!out[key]) throw new UsageError(`missing --${key}`);
  const wanted = out.scenario === 'all' ? [...SCENARIOS] : out.scenario.split(',').map((s) => s.trim().toUpperCase());
  for (const q of wanted) if (!SCENARIOS.includes(q)) throw new UsageError(`unknown scenario ${q} (expected Q1..Q9 or all)`);
  return { config: path.resolve(out.config), evidence: path.resolve(out.evidence), scenarios: [...new Set(wanted)] };
}

// ---------- minimal JSON Schema validator (the subset config.schema.json uses) ----------

function typeOf(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (Number.isInteger(value)) return 'integer';
  return typeof value;
}

function resolveRef(root, ref) {
  if (!ref.startsWith('#/')) throw new Error(`unsupported $ref ${ref}`);
  return ref.slice(2).split('/').reduce((node, key) => {
    if (!node || !(key in node)) throw new Error(`unresolved $ref ${ref}`);
    return node[key];
  }, root);
}

/** Returns a list of "path: message" errors; empty means valid. */
export function validateSchema(value, schema, root = schema, at = '$') {
  if (schema.$ref) return validateSchema(value, resolveRef(root, schema.$ref), root, at);
  const errors = [];
  const actual = typeOf(value);
  if (schema.type) {
    const ok = schema.type === 'number' ? actual === 'number' || actual === 'integer' : actual === schema.type;
    if (!ok) return [`${at}: expected ${schema.type}, got ${actual}`];
  }
  if (schema.enum && !schema.enum.includes(value)) errors.push(`${at}: must be one of ${schema.enum.join(', ')}`);
  if (actual === 'string') {
    if (schema.minLength !== undefined && value.length < schema.minLength) errors.push(`${at}: shorter than ${schema.minLength}`);
    if (schema.pattern && !new RegExp(schema.pattern, 'u').test(value)) errors.push(`${at}: does not match ${schema.pattern}`);
  }
  if (actual === 'integer' || actual === 'number') {
    if (schema.minimum !== undefined && value < schema.minimum) errors.push(`${at}: below ${schema.minimum}`);
    if (schema.maximum !== undefined && value > schema.maximum) errors.push(`${at}: above ${schema.maximum}`);
  }
  if (actual === 'array') {
    if (schema.minItems !== undefined && value.length < schema.minItems) errors.push(`${at}: needs at least ${schema.minItems} items`);
    if (schema.items) value.forEach((item, i) => errors.push(...validateSchema(item, schema.items, root, `${at}[${i}]`)));
  }
  if (actual === 'object') {
    for (const key of schema.required ?? []) if (!Object.hasOwn(value, key)) errors.push(`${at}: missing required ${key}`);
    const props = schema.properties ?? {};
    for (const [key, child] of Object.entries(value)) {
      if (Object.hasOwn(props, key)) errors.push(...validateSchema(child, props[key], root, `${at}.${key}`));
      else if (schema.additionalProperties === false) errors.push(`${at}: unexpected property ${key}`);
    }
  }
  return errors;
}

/** Schema + semantic checks a JSON Schema cannot express. Throws UsageError on failure. */
export function loadConfig(configPath) {
  const schema = JSON.parse(readFileSync(path.join(HERE, 'config.schema.json'), 'utf8'));
  let config;
  try { config = JSON.parse(readFileSync(configPath, 'utf8')); } catch (error) { throw new UsageError(`cannot read config ${configPath}: ${error.message}`); }
  const errors = validateSchema(config, schema);
  const roots = Object.entries(config.isolation ?? {}).map(([k, v]) => [k, path.resolve(String(v))]);
  for (let i = 0; i < roots.length; i += 1) {
    for (let j = i + 1; j < roots.length; j += 1) {
      const [a, pa] = roots[i];
      const [b, pb] = roots[j];
      if (pa === pb || pa.startsWith(pb + path.sep) || pb.startsWith(pa + path.sep)) errors.push(`$.isolation: ${a} and ${b} must be distinct, non-nested roots`);
    }
  }
  const home = process.env.HOME || process.env.USERPROFILE;
  for (const [k, p] of roots) if (home && path.resolve(home) === p) errors.push(`$.isolation.${k}: must not be the real home directory`);
  if (config.engine === 'chromium' && !config.engineDir) errors.push('$.engineDir: required when engine is chromium');
  if (config.engine !== 'chromium' && config.engineDir) errors.push('$.engineDir: only valid with engine chromium');
  if (errors.length) throw new UsageError(`config ${configPath} is invalid:\n  ${errors.join('\n  ')}`);
  return config;
}

/** Evidence directory label: one per OS and engine, so both engines' runs sit side by side. */
export function evidenceLabel(config) {
  return `${config.os}-${config.engine ?? 'webview'}`;
}

/** Windows publishes a TCP port file; Unix hosts publish a domain socket. */
export function browserEndpoint(config, platform = process.platform) {
  return path.join(config.isolation.runtimeRoot, platform === 'win32' || config.os === 'windows' ? 'browser.port' : 'browser.sock');
}

/** First existing executable among candidates, or null. */
export function resolveCandidate(candidates) {
  for (const candidate of [candidates].flat()) {
    if (!candidate) continue;
    try { if (statSync(candidate).isFile()) return candidate; } catch { /* missing */ }
  }
  return null;
}

// ---------- evidence writers ----------

export function redact(value) {
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, REDACT_KEYS.test(k) ? '[redacted]' : redact(v)]));
  }
  return value;
}

export function createEvidence(root, osLabel, scenario) {
  const dir = path.join(root, osLabel, scenario);
  mkdirSync(dir, { recursive: true });
  const transcriptPath = path.join(dir, 'transcript.jsonl');
  writeFileSync(transcriptPath, '');
  const assertions = [];
  const skipped = [];
  return {
    dir,
    transcript(entry) { appendFileSync(transcriptPath, `${JSON.stringify(redact({ at: new Date().toISOString(), ...entry }))}\n`); },
    assert(id, pass, { expected, observed } = {}) {
      const entry = { id, pass: pass === true, expected, observed: redact(observed) };
      assertions.push(entry);
      this.transcript({ assertion: entry });
      return entry.pass;
    },
    /** Records a step that could not run at all; it fails the scenario like an assertion. */
    skip(id, reason) {
      const entry = { id, reason };
      skipped.push(entry);
      assertions.push({ id: `${id}:SKIPPED`, pass: false, observed: entry });
      this.transcript({ skipped: entry });
    },
    writePng(name, bytes) {
      if (!bytes) return null;
      writeFileSync(path.join(dir, name), bytes);
      return path.join(dir, name);
    },
    diagnostics() { return { assertions: [...assertions], skipped: [...skipped] }; },
    finish(receipt, error) {
      if (error) assertions.push({ id: 'scenario:exception', pass: false, observed: String(error?.stack ?? error) });
      const status = scenarioStatus({ assertions, notImplemented: skipped, error: null });
      writeFileSync(path.join(dir, 'assertions.json'), `${JSON.stringify({ scenario, status, skipped, assertions }, null, 2)}\n`);
      writeFileSync(path.join(dir, 'receipt.json'), `${JSON.stringify({ scenario, status, ...receipt, finishedAt: new Date().toISOString() }, null, 2)}\n`);
      return status;
    },
  };
}

/** Fail rather than issue an unattributed receipt; packaged harnesses can supply their build SHA. */
export async function receiptProvenance(binaries, { env = process.env, repo = REPO } = {}) {
  let sha = env.FERRYX_QA_SHA?.trim();
  if (!sha) {
    const proc = Bun.spawn(['git', '-C', repo, 'rev-parse', 'HEAD'], { env, stdout: 'pipe', stderr: 'pipe' });
    const [code, out, err] = await Promise.all([
      proc.exited, new Response(proc.stdout).text(), new Response(proc.stderr).text(),
    ]);
    if (code !== 0) throw new Error(`cannot resolve receipt SHA: ${err.trim()}`);
    sha = out.trim();
  }
  if (!/^(?:[0-9a-f]{40}|[0-9a-f]{64})$/i.test(sha)) throw new Error('receipt SHA must be a full git commit hash');
  const hashCandidate = async (candidate) => {
    if (!candidate) return null;
    const hash = createHash('sha256');
    for await (const chunk of createReadStream(candidate)) hash.update(chunk);
    return hash.digest('hex');
  };
  const [cliSha256, guiSha256] = await Promise.all([hashCandidate(binaries.cli), hashCandidate(binaries.gui)]);
  return { sha, cliSha256, guiSha256 };
}

// ---------- child environment ----------

/**
 * Child env pointing every data/runtime root at the isolated QA roots; never mutates process env.
 * FERRYX_DATA_DIR isolates the controller store and agent registry; the engine choice is
 * FERRYX_CEF_ENGINE_DIR (set only for chromium). `pick` keeps only variables the host needs.
 */
export function isolatedEnv(config, extra = {}) {
  const { runtimeRoot, dataRoot, profileRoot, tmpRoot } = config.isolation;
  const tmp = tmpRoot ?? path.join(runtimeRoot, 'tmp');
  const base = {
    PATH: process.env.PATH ?? '',
    ...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}),
    ...(process.env.HOME ? { HOME: process.env.HOME } : {}),
    ...(process.env.USERPROFILE ? { USERPROFILE: process.env.USERPROFILE } : {}),
    ...(process.env.LOCALAPPDATA ? { LOCALAPPDATA: process.env.LOCALAPPDATA } : {}),
    TMPDIR: tmp,
    TEMP: tmp,
    TMP: tmp,
    FERRYX_RUNTIME_DIR: runtimeRoot,
    FERRYX_DATA_DIR: dataRoot,
    FERRYX_QA_RUNTIME_DIR: runtimeRoot,
    FERRYX_QA_DATA_DIR: dataRoot,
    FERRYX_QA_PROFILE_DIR: profileRoot,
    ...(config.engine === 'chromium' ? { FERRYX_CEF_ENGINE_DIR: config.engineDir } : {}),
    ...(config.env ?? {}),
  };
  return { ...base, ...extra };
}

/**
 * Runs one `ferryx browser ...` command in the isolated env and returns { code, stdout, stderr,
 * json }. Bounded by `timeoutMs`; the child is killed on timeout and that is reported as code null.
 */
export async function runCli(cli, args, { env, timeoutMs = 15_000, spawn = Bun.spawn } = {}) {
  const child = spawn([cli, 'browser', ...args], { env, stdout: 'pipe', stderr: 'pipe' });
  let timer;
  const timedOut = new Promise((resolve) => { timer = setTimeout(() => { child.kill(); resolve('timeout'); }, timeoutMs); });
  const [exit, stdout, stderr] = await Promise.all([
    Promise.race([child.exited, timedOut]),
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
  ]);
  clearTimeout(timer);
  let json = null;
  try { json = JSON.parse(stdout.trim()); } catch { /* not JSON */ }
  return { code: exit === 'timeout' ? null : exit, stdout, stderr, json };
}

/**
 * Server error code of a failed CLI call. The CLI prints `{"type":"error","code":<cli code>,
 * "message":"<SERVER_CODE>: <text>"}` on stderr; the server code precedes the first `: `.
 */
export function cliErrorCode(result) {
  return cliErrorCodeFromSteps(result);
}

// ---------- raw socket envelope (Q2: the token-only admission path) ----------

/**
 * One `BrowserCliEnvelope` line over the app's browser socket (unix socket, or the loopback TCP
 * port recorded in `browser.port` on Windows). The envelope is `{token, ...request}` with the
 * request tagged by `command`; there are no controller credentials, which is exactly the legacy
 * token-only caller Q2 exercises. Returns the parsed response, or { error } when nothing parsed.
 */
export async function sendEnvelope(config, payload, { timeoutMs = 10_000 } = {}) {
  const runtimeRoot = config.isolation.runtimeRoot;
  const unixPath = path.join(runtimeRoot, 'browser.sock');
  const portPath = path.join(runtimeRoot, 'browser.port');
  const line = `${JSON.stringify(payload)}\n`;
  return new Promise((resolve) => {
    let settled = false;
    const done = (value) => { if (!settled) { settled = true; socket.destroy(); resolve(value); } };
    const socket = existsSync(unixPath)
      ? netConnect(unixPath)
      : netConnect({ host: '127.0.0.1', port: Number(readFileSync(portPath, 'utf8').trim()) });
    const decoder = createLineDecoder();
    const timer = setTimeout(() => done({ error: 'timeout' }), timeoutMs);
    socket.setEncoding('utf8');
    socket.on('connect', () => socket.write(line));
    socket.on('data', (chunk) => {
      for (const item of decoder.push(chunk)) {
        if (item.frame) { clearTimeout(timer); done({ response: item.frame }); }
        else { clearTimeout(timer); done({ error: item.error?.code ?? 'malformed' }); }
      }
    });
    socket.on('error', (error) => { clearTimeout(timer); done({ error: String(error?.code ?? error?.message ?? error) }); });
    socket.on('close', () => { clearTimeout(timer); done({ error: 'closed' }); });
  });
}

/** The token this run's runtime dir owns; never printed by the harness. */
export function readBrowserToken(runtimeRoot) {
  for (const name of ['browser.sock.token', 'browser.port.token', 'browser.token']) {
    const file = path.join(runtimeRoot, name);
    if (existsSync(file)) {
      const token = readFileSync(file, 'utf8').trim();
      if (token) return token;
    }
  }
  return null;
}

// ---------- MCP client (Q6/Q2) ----------

/**
 * Speaks JSON-RPC to `ferryx browser mcp` over stdio: one JSON object per line, requests and
 * responses correlated by `id`. Nothing is printed to stdout by the client itself.
 */
export function createMcpClient({ env, cli, timeoutMs = 15_000, onTranscript = () => {} }) {
  const child = Bun.spawn([cli, 'browser', 'mcp'], { env, stdin: 'pipe', stdout: 'pipe', stderr: 'pipe' });
  const decoder = createLineDecoder();
  const pending = new Map();
  let nextId = 1;
  let stderr = '';
  (async () => { for await (const chunk of child.stderr) stderr += new TextDecoder().decode(chunk); })();
  (async () => {
    for await (const chunk of child.stdout) {
      for (const item of decoder.push(chunk)) {
        if (!item.frame) { onTranscript({ dir: 'in', error: item.error?.code }); continue; }
        const entry = pending.get(item.frame.id);
        onTranscript({ dir: 'in', frame: item.frame });
        if (entry) { pending.delete(entry.id); entry.resolve(item.frame); }
      }
    }
    for (const entry of pending.values()) entry.reject(new Error(`mcp exited: ${stderr.slice(-200)}`));
    pending.clear();
  })();
  const send = (method, params) => {
    const id = nextId++;
    const frame = { jsonrpc: '2.0', id, method, ...(params === undefined ? {} : { params }) };
    onTranscript({ dir: 'out', frame });
    const response = new Promise((resolve, reject) => {
      pending.set(id, { id, resolve, reject });
      setTimeout(() => { if (pending.delete(id)) reject(new Error(`mcp ${method} timed out`)); }, timeoutMs);
    });
    child.stdin.write(`${JSON.stringify(frame)}\n`);
    child.stdin.flush?.();
    return response;
  };
  return {
    send,
    callTool: (name, args) => send('tools/call', { name, arguments: args }),
    stderr: () => stderr,
    async close() {
      try { child.stdin.end(); } catch { /* closed */ }
      try { await Promise.race([child.exited, new Promise((r) => setTimeout(r, 5_000))]); } catch { /* ignore */ }
      try { child.kill(); } catch { /* ignore */ }
    },
  };
}

// ---------- GUI: consent approval in the real app ----------

/**
 * A screenshot decoded to pixels, retried while the driver has no encoded frame yet. Returns null
 * when no frame arrives within the bound (the caller records that as a failed step).
 */
async function captureFrame(gui, { deadlineMs = 20_000, intervalMs = 500 } = {}) {
  const started = Date.now();
  for (;;) {
    let bytes = null;
    try { bytes = await gui.screenshot(); } catch { bytes = null; }
    if (bytes && bytes.length > 0) {
      try { return { bytes, image: decodePng(bytes) }; } catch { return { bytes, image: null }; }
    }
    if (Date.now() - started >= deadlineMs) return null;
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}

/**
 * The app window's rectangle inside the screenshot, in screenshot pixels. Screenshots are
 * full-output, so the window's own box is what keeps card detection from matching another
 * monitor's same-coloured panel. Null when the host cannot report it, which only widens the search.
 */
export function windowRectFromClients(clients, pid) {
  const mine = (Array.isArray(clients) ? clients : []).find((c) => c?.mapped === true && (pid === null || Number(c.pid) === Number(pid)));
  if (!mine || !Array.isArray(mine.at) || !Array.isArray(mine.size)) return null;
  const [x, y] = mine.at.map(Number);
  const [width, height] = mine.size.map(Number);
  if (![x, y, width, height].every(Number.isFinite) || width <= 0 || height <= 0) return null;
  return { left: x, top: y, right: x + width, bottom: y + height, width, height };
}

/**
 * Finds the consent card in a live screenshot and answers it. The card is located by its own
 * popover fill inside the app window - every theme's popover fill, because the card is `bg-popover`
 * and a light-theme window renders a white card - so a moved, resized or re-themed window still
 * works. The click point is then chosen from OCR word boxes when a tesseract binary is available
 * (the card title opens with the primary's own verb, so a topmost-word guess clicks the title) and
 * from measured button runs inside the located card otherwise. `action` is 'approve' or 'deny'.
 * Returns { answered, card, point, shot, via } and never throws: a card it cannot find is a failed
 * step, reported by the caller.
 */
async function answerConsentCard(ctx, { action = 'approve', label = 'consent', within = null, deadlineMs = null } = {}) {
  const gui = await ctx.gui();
  const bound = deadlineMs ?? ctx.config.timeouts?.approvalMs ?? 45_000;
  const started = Date.now();
  // The card can only be found inside the app window, and that window's rectangle is only known
  // from the live compositor; a configured rectangle is a fallback for hosts that cannot report it.
  const area = within ?? await liveWindowRect({ env: isolatedEnv(ctx.config, ctx.config.guiEnv ?? {}) }) ?? ctx.guiWindow;
  const tesseract = resolveTesseract(ctx.config.tesseract ?? null);
  let last = null;
  for (;;) {
    const shot = await captureFrame(gui, { deadlineMs: 15_000 });
    if (!shot) return { answered: false, reason: 'no screenshot frame', shot: null };
    last = shot;
    // A card is only usable if its action row is inside the frame: the window may be larger than
    // the captured output, and OCR boxes are frame pixels.
    const card = shot.image ? locateConsentCard(shot.image, { within: area, tolerance: 12 }) : null;
    if (card) {
      const buttons = card.buttons ?? (shot.image ? measureActionButtons(shot.image, card) : { allow: null, deny: null });
      let target = action === 'deny' ? buttons.deny : buttons.allow;
      // The measured runs give the geometry; OCR only has to corroborate that they are the action
      // row. The primary's own label is the least reliable word on the card (see confirmActionRow),
      // so it is never the thing the click is chosen on: a garbled `Allow` must not veto a card that
      // is plainly there, and a lone `Allow` on the title line must not be clicked either.
      let via = target ? 'pixels' : null;
      let proof = null;
      if (shot.image && tesseract && target) {
        const ocr = await ocrCardWords(shot.image, card, { binary: tesseract, dir: ctx.ev.dir });
        ctx.ev.transcript({ ocrCard: { label, passes: ocr.passes, words: ocr.words.length, crop: ocr.crop } });
        proof = confirmActionRow(ocr.words, buttons, { box: card });
        ctx.ev.transcript({ ocrActionRow: { label, ...proof } });
        if (proof.confirmed) via = `pixels (action row confirmed by '${proof.sibling}')`;
      }
      if (action === 'deny') {
        // Deny is the left of the pair `justify-end` lays out, and on the measured host it is
        // demonstrably its own button; nothing else in the card reads as a control.
        target = buttons.deny;
        via = target ? (via ?? 'pixels') : null;
      }
      if (!target) return { answered: false, reason: 'card has no action row', card, buttons, shot };
      ctx.ev.writePng(`consent-${label}-before.png`, shot.bytes);
      ctx.ev.transcript({ consentClick: { label, action, point: target, via, card, buttons } });
      await gui.click(target.x, target.y);
      const after = await captureFrame(gui, { deadlineMs: 10_000 });
      if (after) ctx.ev.writePng(`consent-${label}-after.png`, after.bytes);
      return { answered: true, card, buttons, point: target, shot, after, via };
    }
    if (Date.now() - started >= bound) return { answered: false, reason: `no consent card within ${bound}ms`, shot: last };
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

// ---------- scenario context ----------

function createContext({ config, binaries, fixtures, ev, guiWindow }) {
  let gui = null;
  return {
    config,
    binaries,
    fixtures,
    ev,
    guiWindow,
    /** Pids this run launched and therefore may stop; nothing else is ever killed. */
    ownedGuis: [],
    /** Transient user units this run created (Linux), stopped by name on teardown. */
    ownedUnits: [],
    socket: browserEndpoint(config),
    subscribe: (origin = fixtures.origins.a) => subscribeEvents(origin, { timeoutMs: config.timeouts?.readyMs ?? 10_000 }),
    async gui() {
      gui ??= await connectMahord(
        { kind: 'mahord', host: config.mahord.host, pairingId: config.mahord.pairingId, port: config.mahord.port },
        { binary: config.mahord.rdClient ?? 'maho-client', log: (entry) => ev.transcript({ mahord: entry }) },
      );
      return gui;
    },
    pipe(extraArgs = []) {
      return spawnPipe([binaries.cli, 'browser', 'pipe', ...extraArgs], {
        env: isolatedEnv(config),
        timeoutMs: config.timeouts?.requestMs ?? 20_000,
        onTranscript: (entry) => ev.transcript({ pipe: entry }),
      });
    },
    mcp(timeoutMs = config.timeouts?.requestMs ?? 20_000) {
      return createMcpClient({
        cli: binaries.cli,
        env: isolatedEnv(config),
        timeoutMs,
        onTranscript: (entry) => ev.transcript({ mcp: entry }),
      });
    },
    cli(args, timeoutMs = config.timeouts?.requestMs ?? 20_000) {
      return runCli(binaries.cli, args, { env: isolatedEnv(config), timeoutMs });
    },
    async close() {
      if (gui) await gui.close().catch(() => {});
      // Stop only GUIs this run launched; an adopted app belongs to the host, not to the suite.
      for (const unit of this.ownedUnits.splice(0)) await stopUnit(unit);
      for (const pid of this.ownedGuis.splice(0)) await killPid(pid).catch(() => {});
    },
  };
}

// ---------- shared steps ----------

/** Preflight: fixtures reachable, candidates present, and the app socket answering. */
/**
 * Why the app cannot be reached on its own socket, as recorded evidence: whether the socket file is
 * there, whether anything listens on it, which files the runtime dir holds, and what the CLI itself
 * answers under this run's environment. This turns "the CLI says unavailable" into a cause.
 */
async function diagnoseEndpoint(socket, { cli, env } = {}) {
  const run = (cmd) => new Promise((resolve) => {
    // Diagnostic only, and `sh` does not exist on every QA host: report nothing rather than throw.
    if (process.platform === 'win32') { resolve(''); return; }
    const child = Bun.spawn(['sh', '-c', cmd], { stdout: 'pipe', stderr: 'ignore' });
    new Response(child.stdout).text().then((out) => child.exited.then(() => resolve(out.trim()))).catch(() => resolve(''));
  });
  const dir = socket ? path.dirname(socket) : null;
  let cliAnswer = '';
  if (cli && env) {
    const child = Bun.spawn([cli, 'browser', 'list'], { env, stdout: 'pipe', stderr: 'pipe' });
    cliAnswer = ((await new Response(child.stdout).text()) || (await new Response(child.stderr).text())).trim().slice(0, 200);
    await child.exited;
  }
  return {
    socket,
    socketExists: Boolean(socket && existsSync(socket)),
    listening: socket ? Number(await run(`ss -xl 2>/dev/null | grep -c -- ${JSON.stringify(socket)}`)) > 0 : false,
    files: dir ? await run(`ls -1 ${JSON.stringify(dir)} 2>/dev/null | tr '\\n' ' '`) : '',
    cliAnswer,
  };
}

/** Stops one transient user unit this run created, bounded; never a name pattern. */
async function stopUnit(unit) {
  if (!unit) return;
  try {
    const child = Bun.spawn(['systemctl', '--user', 'stop', unit], { stdout: 'ignore', stderr: 'ignore' });
    await child.exited;
  } catch { /* already gone */ }
}

/**
 * Clears this run's own isolated state so a launch starts clean. Only the declared roots are
 * touched, and a broad one (/, the home directory itself) is refused: a misconfigured root must
 * never be able to delete a real profile.
 */
export function resetRuntimeState(isolation, { log = () => {} } = {}) {
  const all = [isolation.runtimeRoot, isolation.dataRoot, isolation.profileRoot, isolation.tmpRoot ?? path.join(isolation.runtimeRoot, 'tmp')].filter(Boolean);
  for (const root of all) mkdirSync(root, { recursive: true });
  for (const root of [isolation.runtimeRoot, isolation.dataRoot]) {
    if (!root) continue;
    const resolved = path.resolve(root);
    if (resolved === '/' || resolved === path.resolve(process.env.HOME ?? '/nonexistent') || resolved.split(path.sep).filter(Boolean).length < 3) {
      log({ skipped: resolved, reason: 'root is too broad to clear' });
      continue;
    }
    try {
      rmSync(resolved, { recursive: true, force: true });
      log({ cleared: resolved });
    } catch (error) {
      log({ failed: resolved, error: String(error?.message ?? error) });
    }
  }
  for (const root of all) mkdirSync(root, { recursive: true });
}

/**
 * Makes sure the candidate GUI is running on this run's runtime dir. An app already serving the
 * socket is adopted (a host may keep one alive across runs); otherwise one is launched with the
 * run's isolated roots. Returns `{ launched, pid, socket, adopted }` and never throws, so preflight
 * can report the real state instead of failing later for an unrelated reason.
 */
async function ensureGui(ctx, { socket, logFile }) {
  const { config, binaries, ev } = ctx;
  // Adopt only an app that actually answers: a socket file outlives a killed app, and adopting one
  // would report a launch-free run that cannot talk to anything. This runs before any reset, so a
  // healthy adopted app keeps its state.
  if (socket && existsSync(socket)) {
    const probe = await ctx.cli(['list'], 5_000).catch(() => null);
    if (probe && cliErrorCode(probe) !== 'BROWSER_CLI_INVALID_OR_UNAVAILABLE') {
      return { launched: false, adopted: true, socket, pid: null, ready: true, firstAnswer: cliErrorCode(probe) ?? 'ok' };
    }
    ev.transcript({ staleSocket: socket, action: 'launching a fresh app' });
  }
  if (!binaries.gui) return { launched: false, adopted: false, socket: null, pid: null, reason: 'no GUI candidate on this host' };
  // Start from a clean state. A leftover `owner.json` (or its daemon) makes the new GUI decide "a
  // daemon owner exists" and start nothing, so `control.sock` never appears and every CLI call
  // reports the app as not running. A host may opt out when it deliberately shares one daemon.
  if (config.isolation.resetOnLaunch !== false) {
    resetRuntimeState(config.isolation, { log: (entry) => ev.transcript({ reset: entry }) });
  }
  // The isolated roots must exist before launch: GTK aborts outright (icon-theme pixbuf write
  // failure) when TMPDIR points at a missing directory, which would look like a product crash.
  for (const dir of [config.isolation.runtimeRoot, config.isolation.dataRoot, config.isolation.profileRoot, config.isolation.tmpRoot ?? path.join(config.isolation.runtimeRoot, 'tmp')]) {
    ensureDir(dir);
  }
  const launchOnce = async () => {
    const started = await launchGui(binaries.gui, {
      env: isolatedEnv(config, config.guiEnv ?? {}),
      socket,
      args: config.guiLaunch?.args ?? [],
      cwd: config.guiLaunch?.cwd ?? null,
      logFile,
      unit: config.guiLaunch?.unit ?? 'ferryx-qa-gui',
      deadlineMs: config.timeouts?.readyMs ? config.timeouts.readyMs * 6 : 90_000,
    });
    ctx.ownedGuis.push(started.pid);
    if (started.unit) ctx.ownedUnits.push(started.unit);
    if (!started.socket) return { ...started, ready: false, firstAnswer: 'no socket' };
    // The socket file exists before the app can answer on it: `browser.sock` appears while the
    // daemon is still coming up, and a CLI call in that window reports
    // BROWSER_CLI_INVALID_OR_UNAVAILABLE as if no app were running. Wait for a real answer, which is
    // normally BROWSER_CONTROLLER_PAIRING_REQUIRED (the requirement this suite then exercises).
    // The socket file exists before the app can answer on it: `browser.sock` appears while the
    // daemon is still coming up, and a CLI call in that window reports
    // BROWSER_CLI_INVALID_OR_UNAVAILABLE as if no app were running. A cold WebKit app on this host
    // takes longer than the 15 s a scenario tolerates per request, so readiness gets its own,
    // larger bound (`timeouts.startupMs`) instead of reusing `readyMs`.
    const deadline = Date.now() + (config.timeouts?.startupMs ?? 60_000);
    for (;;) {
      const probe = await ctx.cli(['list'], 5_000);
      const code = cliErrorCode(probe) ?? 'ok';
      if (code !== 'BROWSER_CLI_INVALID_OR_UNAVAILABLE') return { ...started, ready: true, firstAnswer: code };
      if (Date.now() >= deadline) {
        // Record why it is unreachable, so the evidence names the cause instead of a bare code.
        const diag = await diagnoseEndpoint(socket, { cli: binaries.cli, env: isolatedEnv(config) });
        ev.transcript({ guiUnreachable: diag });
        return { ...started, ready: false, firstAnswer: code, diag };
      }
      await new Promise((resolve) => setTimeout(resolve, 300));
    }
  };

  let attempt = await launchOnce();
  // One retry after a full reset: a second app instance for the same roots can find the first one's
  // `owner.json` and start no daemon, which is a harness-ordering artifact, not a product failure.
  if (!attempt.ready && config.isolation.resetOnLaunch !== false) {
    ev.transcript({ guiRetry: { reason: attempt.firstAnswer, previousUnit: attempt.unit ?? null } });
    if (attempt.unit) await stopUnit(attempt.unit);
    if (attempt.pid) await killPid(attempt.pid).catch(() => {});
    resetRuntimeState(config.isolation, { log: (entry) => ev.transcript({ reset: entry }) });
    attempt = await launchOnce();
  }
  return { ...attempt, launched: true, adopted: false };
}

/**
 * Ensures the candidate app's frontend origin answers. A debug build loads `devUrl`
 * (`src-tauri/tauri.conf.json`) rather than `frontendDist`, so without a server there the window
 * renders `404 File not found` and no consent card can ever appear -- a launch failure that would
 * otherwise be reported as a pairing failure. When `guiFrontend` is configured and nothing answers,
 * the run starts one for its own lifetime and stops it on teardown.
 */
async function ensureFrontend(ctx) {
  const spec = ctx.config.guiFrontend;
  if (!spec) return { configured: false, serving: null };
  const origin = `http://${spec.host}:${spec.port}`;
  // Plain fetch, not curl: the QA hosts include Windows, where `curl` is an alias for a cmdlet whose
  // flags are nothing like the real tool's.
  const probe = async () => {
    try {
      const response = await fetch(`${origin}/`, { signal: AbortSignal.timeout(5_000) });
      return String(response.status);
    } catch {
      return '000';
    }
  };
  const before = await probe();
  if (before === '200') return { configured: true, serving: null, status: before, adopted: true };
  const logFile = path.join(ctx.ev.dir, 'frontend.log');
  const unit = `${ctx.config.guiLaunch?.unit ?? 'ferryx-qa'}-frontend-${process.pid}-${Math.random().toString(36).slice(2, 8)}`;
  const child = Bun.spawn([
    ...(process.platform === 'linux'
      ? ['systemd-run', '--user', `--unit=${unit}`, '--collect',
        `--property=StandardOutput=append:${logFile}`, `--property=StandardError=append:${logFile}`, '--']
      : []),
    spec.command, ...(spec.args ?? []),
  ], { stdout: 'pipe', stderr: 'pipe', cwd: spec.cwd ?? undefined });
  // Without systemd the harness owns the server directly, so it belongs to the pids teardown kills.
  const directly = process.platform !== 'linux';
  const started = directly ? 0 : await child.exited;
  if (started !== 0) {
    const err = (await new Response(child.stderr).text()).trim().slice(0, 300);
    return { configured: true, serving: null, status: before, error: err, unit };
  }
  const deadline = Date.now() + (ctx.config.timeouts?.startupMs ?? 60_000);
  let status = before;
  while (Date.now() < deadline) {
    status = await probe();
    if (status === '200') break;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  if (status === '200') {
    if (directly) ctx.ownedGuis.push(child.pid);
    else ctx.ownedUnits.push(unit);
  }
  return { configured: true, serving: directly ? `pid ${child.pid}` : unit, status, adopted: false, logFile };
}

async function preflight(ctx) {
  const { ev, fixtures } = ctx;
  const subs = [];
  try {
    for (const origin of [fixtures.origins.a, fixtures.origins.b]) {
      const sub = await ctx.subscribe(origin);
      subs.push(sub);
      ev.assert(`preflight:sse-ready:${origin}`, Number.isInteger(sub.readySeq), { expected: 'ready frame', observed: { readySeq: sub.readySeq } });
    }
    ev.assert('preflight:cli-candidate', ctx.binaries.cli !== null, { expected: 'existing CLI candidate', observed: ctx.binaries.cli });
    // A debug build renders `devUrl`, so the frontend origin must answer before the app is judged.
    const frontend = await ensureFrontend(ctx);
    ev.transcript({ frontend });
    if (frontend.configured) {
      ev.assert('preflight:frontend-serving', frontend.status === '200', {
        expected: 'the frontend origin the app loads answers 200',
        observed: { status: frontend.status, unit: frontend.serving ?? null, error: frontend.error ?? null },
      });
    }
    // Bring the app up before asking it anything: a suite that silently tested a dead app would
    // report pairing failures that are really launch failures.
    const gui = await ensureGui(ctx, {
      socket: ctx.socket,
      logFile: path.join(ctx.ev.dir, 'gui.log'),
    });
    ev.transcript({ guiLaunch: { launched: gui.launched, adopted: gui.adopted, pid: gui.pid, socket: gui.socket, ready: gui.ready ?? null, firstAnswer: gui.firstAnswer ?? null, reason: gui.reason ?? null } });
    ev.assert('preflight:gui-up', Boolean(gui.socket), {
      expected: 'the candidate app serves this run\'s runtime dir',
      observed: { socket: gui.socket, launched: gui.launched, adopted: gui.adopted, pid: gui.pid, logFile: gui.logFile ?? null },
    });
    // The app must be serving this run's runtime dir; an isolated instance that died would make
    // every later step fail for the wrong reason. `BROWSER_CLI_INVALID_OR_UNAVAILABLE` is exactly
    // "no app on this socket", so any other answer (including a pairing refusal) proves it is up.
    const listed = await ctx.cli(['list']);
    const listedCode = cliErrorCode(listed) ?? null;
    ev.assert('preflight:app-serving', listedCode !== 'BROWSER_CLI_INVALID_OR_UNAVAILABLE', {
      expected: 'the GUI answers on this run\'s runtime dir',
      observed: { code: listed.code, code0: listedCode },
    });
  } finally {
    for (const sub of subs) sub.close();
  }
}

/**
 * Pairs a controller through the CLI and answers the card in the GUI. Returns the controller id,
 * or null after recording why it failed.
 */
async function pairController(ctx, label) {
  const { ev } = ctx;
  const pending = ctx.cli(['controllers', 'pair', '--name', label], 90_000);
  const answered = await answerConsentCard(ctx, { action: 'approve', label: `pair-${label}`, within: ctx.guiWindow });
  const result = await pending;
  ev.transcript({ step: 'controllers pair', code: result.code, stdout: result.json, stderr: result.stderr.slice(0, 400) });
  if (!answered.answered) {
    ev.assert(`pair:${label}:consent-card`, false, { expected: 'Allow clicked on the pairing card', observed: answered.reason });
    return null;
  }
  ev.assert(`pair:${label}:consent-card`, true, { expected: 'Allow clicked', observed: answered.point });
  const id = result.json?.controllerId ?? null;
  ev.assert(`pair:${label}`, result.code === 0 && typeof id === 'string', { expected: '{"type":"paired"}', observed: result.json ?? result.stderr.slice(0, 300) });
  return id;
}

/** Requests a grant and answers its card. Returns the resolution string or null. */
async function grantScope(ctx, { origin = null, scopes = [], tool = null, path: grantPath = null } = {}) {
  const { ev } = ctx;
  const args = ['controllers', 'grant'];
  if (origin) args.push('--origin', origin);
  if (scopes.length) args.push('--scopes', scopes.join(','));
  if (tool) args.push('--tool', tool);
  if (grantPath) args.push('--path', grantPath);
  const pending = ctx.cli(args, 90_000);
  const answered = await answerConsentCard(ctx, { action: 'approve', label: `grant-${tool ?? scopes.join('-')}`, within: ctx.guiWindow });
  const result = await pending;
  ev.transcript({ step: args.join(' '), code: result.code, stdout: result.json, stderr: result.stderr.slice(0, 300) });
  const dc = origin ? `${origin}` : `${tool}:${grantPath}`;
  if (!answered.answered) {
    ev.assert(`grant:${dc}`, false, { expected: 'Allow clicked on the grant card', observed: answered.reason });
    return null;
  }
  const resolution = result.json?.resolution ?? null;
  ev.assert(`grant:${dc}`, typeof resolution === 'string' && resolution.startsWith('approve'), { expected: 'approve*', observed: result.json ?? result.stderr.slice(0, 300) });
  return resolution;
}

/** Opens the form fixture, waits for it, snapshots, and returns the observe plus ref lookup. */
async function openForm(ctx, origin) {
  const { ev } = ctx;
  const opened = await ctx.cli(['open', '--url', `${origin}/form`]);
  const browserId = opened.json?.browser?.browserId ?? null;
  ev.assert('open:/form', typeof browserId === 'string', { expected: 'opened session', observed: opened.json ?? opened.stderr.slice(0, 300) });
  if (!browserId) return null;
  const waited = await ctx.cli(['wait', '--browser-id', browserId, '--text', 'Form fixture']);
  ev.assert('wait:/form', waited.code === 0, { expected: 'waited', observed: waited.stdout || waited.stderr.slice(0, 200) });
  const snap = await ctx.cli(['snapshot', '--browser-id', browserId]);
  const snapshot = snap.json ?? null;
  ev.assert('observe:/form', Boolean(snapshot?.browserId), { expected: 'snapshot', observed: snap.stderr.slice(0, 200) });
  return {
    browserId,
    snapshot,
    generation: actGeneration(snapshot),
    ref: (pred) => findRef(snapshot, pred),
  };
}

/** Names of the frames in a snapshot (opaque boundary check). */
function frameCount(snapshot) {
  return Array.isArray(snapshot?.frames) ? snapshot.frames.length : 0;
}

// ---------- scenario bodies ----------

const SCENARIO_BODIES = {
  // Q1: pair, grant read+interact, observe, fill, submit; the fixture must record exactly one
  // submission carrying the typed name. Also: opening a tab must not steal desktop focus.
  async Q1(ctx) {
    const { ev, fixtures } = ctx;
    const events = await ctx.subscribe(fixtures.origins.a);
    try {
      const focusBefore = await ctx.focusWindow?.() ?? null;
      if (!(await pairController(ctx, 'qa-a'))) return;
      await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

      const form = await openForm(ctx, fixtures.origins.a);
      if (!form) return;
      const focusAfter = await ctx.focusWindow?.() ?? null;
      if (focusBefore !== null && focusAfter !== null) {
        ev.assert('q1:open-does-not-steal-focus', focusBefore === focusAfter, { expected: focusBefore, observed: focusAfter });
      }

      const nameRef = fieldRef(form.snapshot, 'name');
      const submitRef = refsNamed(form.snapshot, 'Submit', { role: 'button' })[0]?.reference ?? null;
      ev.assert('q1:observe-refs', Boolean(nameRef && submitRef), { expected: 'name and submit refs', observed: { nameRef, submitRef } });

      const filled = await ctx.cli(['fill', '--browser-id', form.browserId, '--generation', String(form.generation), '--ref', nameRef, '--value', 'Ferryx']);
      ev.assert('q1:fill', filled.code === 0 && filled.json?.dispatched === true, { expected: 'dispatched', observed: filled.json ?? filled.stderr.slice(0, 200) });

      const submitted = await ctx.cli(['click', '--browser-id', form.browserId, '--generation', String(form.generation), '--ref', submitRef, '--expect-text', 'Saved', '--expect-timeout-ms', '15000']);
      ev.assert('q1:submit-applied', submitted.code === 0 && submitted.json?.applied === true, { expected: 'applied true', observed: submitted.json ?? submitted.stderr.slice(0, 300) });

      const record = await events.waitFor((r) => r.kind === 'form-submit', { label: 'form submit', timeoutMs: 15_000 }).catch((error) => ({ error: String(error?.message ?? error) }));
      ev.assert('q1:fixture-submission', record?.name === 'Ferryx' && fixtures.state().formSubmits.length === 1, { expected: 'one submission named Ferryx', observed: record?.error ?? fixtures.state().formSubmits });

      const shot = await captureFrame(await ctx.gui(), { deadlineMs: 10_000 }).catch(() => null);
      if (shot) ev.writePng('q1-after.png', shot.bytes);
    } finally {
      events.close();
    }
  },

  // Q2: authority on every ingress. Token-only envelope refused; wrong token refused; a paired
  // CLI works; a missing/!default selection refused; revoke makes further calls report revoked;
  // MCP without pairing is refused per tool.
  async Q2(ctx) {
    const { ev, fixtures } = ctx;
    const runtimeRoot = ctx.config.isolation.runtimeRoot;
    const token = readBrowserToken(runtimeRoot);
    ev.assert('q2:token-present', typeof token === 'string', { expected: 'runtime token file', observed: Boolean(token) });

    const noAuth = await sendEnvelope(ctx.config, { token, command: 'list' });
    ev.assert('q2:token-only-list-refused', errorCode(noAuth.response) === CODES.pairingRequired, { expected: CODES.pairingRequired, observed: noAuth.response ?? noAuth.error });

    const forged = await sendEnvelope(ctx.config, { token, command: 'list', controllerAuth: { id: 'forged', secret: 'forged' } });
    ev.assert('q2:forged-controller-refused', errorCode(forged.response) !== null && errorCode(forged.response) !== undefined, { expected: 'a typed refusal', observed: forged.response ?? forged.error });

    const badToken = await sendEnvelope(ctx.config, { token: 'not-the-token', command: 'list' });
    ev.assert('q2:wrong-token-refused', errorCode(badToken.response) === CODES.unauthorized, { expected: CODES.unauthorized, observed: badToken.response ?? badToken.error });

    // `Snapshot` is the one arm of `BrowserCliRequest` with no `rename_all` (browser_cli.rs:31), so
    // its field really is `browser_id` on the wire; sending `browserId` was refused as `missing
    // field browser_id` before authority was ever consulted, which proves the harness wrong rather
    // than the daemon.
    const snapshotNoAuth = await sendEnvelope(ctx.config, { token, command: 'snapshot', browser_id: 'any' });
    ev.assert('q2:token-only-snapshot-refused', errorCode(snapshotNoAuth.response) === CODES.pairingRequired, { expected: CODES.pairingRequired, observed: snapshotNoAuth.response ?? snapshotNoAuth.error });

    // MCP without pairing: the adapter runs, but every browser tool is refused by authority.
    const mcp = ctx.mcp();
    try {
      const init = await mcp.send('initialize', { protocolVersion: '2025-03-26', capabilities: {} });
      ev.assert('q2:mcp-initialize', init?.result?.serverInfo?.name === 'ferryx-browser', { expected: 'ferryx-browser', observed: init?.result?.serverInfo ?? init?.error });
      const tools = await mcp.send('tools/list', {});
      const names = (tools?.result?.tools ?? []).map((t) => t.name).sort();
      ev.assert('q2:mcp-tools-listed', names.length >= 6, { expected: '>=6 tools', observed: names });
      const refused = await mcp.callTool('browser_list', {});
      ev.assert('q2:mcp-unpaired-refused', mcpToolError(refused, CODES.pairingRequired) === CODES.pairingRequired, { expected: CODES.pairingRequired, observed: refused?.result ?? refused?.error });
    } finally {
      await mcp.close();
    }

    // Now pair and prove the same surfaces work, then revoke.
    if (!(await pairController(ctx, 'qa-legacy'))) return;
    const paired = await ctx.cli(['list']);
    ev.assert('q2:paired-cli-list', paired.code === 0 && Array.isArray(paired.json), { expected: 'array of sessions', observed: paired.json ?? paired.stderr.slice(0, 200) });

    // `<data>/browser/controllers/default.json` (browser/agent/client.rs::store_dir + load_auth).
    const defaultPath = path.join(ctx.config.isolation.dataRoot, 'browser', 'controllers', 'default.json');
    const stash = `${defaultPath}.qa-stash`;
    let moved = false;
    if (existsSync(defaultPath)) { writeFileSync(stash, readFileSync(defaultPath)); unlinkSync(defaultPath); moved = true; }
    const withoutDefault = await ctx.cli(['list']);
    ev.assert('q2:missing-default-refused', withoutDefault.code !== 0, { expected: 'refused without default.json', observed: { code: withoutDefault.code, err: withoutDefault.stderr.slice(0, 200) } });
    if (moved) { writeFileSync(defaultPath, readFileSync(stash)); unlinkSync(stash); }

    const revoked = await ctx.cli(['controllers', 'revoke']);
    ev.assert('q2:revoke', revoked.code === 0, { expected: 'revoked', observed: revoked.json ?? revoked.stderr.slice(0, 200) });
    const afterRevoke = await ctx.cli(['list']);
    ev.assert('q2:revoked-refused', cliErrorCode(afterRevoke) === CODES.revoked, { expected: CODES.revoked, observed: { code: afterRevoke.code, err: afterRevoke.stderr.slice(0, 200) } });

    // A pair request the desktop never answers expires (60 s) instead of hanging forever.
    const expired = await ctx.cli(['controllers', 'pair', '--name', 'qa-expire'], 120_000);
    ev.assert('q2:unanswered-pair-expires', expired.code !== 0, { expected: 'nonzero (expired or denied)', observed: { code: expired.code, err: expired.stderr.slice(0, 200) } });

    const sessions = await ctx.cli(['list']).catch(() => ({ json: null }));
    ev.assert('q2:revoked-list-is-typed', sessions.json === null || !Array.isArray(sessions.json), { expected: 'no privileged listing while revoked', observed: sessions.json });
    void fixtures;
  },

  // Q3: a grant for origin A does not authorise origin B; the refusal is typed; after revoke the
  // controller is refused. Restarting the CLI keeps the credential (it is on disk, not in memory).
  async Q3(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pairController(ctx, 'qa-a'))) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

    const other = await ctx.cli(['open', '--url', `${fixtures.origins.b}/form`]);
    ev.assert('q3:other-origin-refused', other.code !== 0 && cliErrorCode(other) === CODES.forbidden, { expected: CODES.forbidden, observed: { code: other.code, err: other.stderr.slice(0, 240) } });
    ev.assert('q3:redirect-target-not-loaded', fixtures.state().requests.filter((r) => r.origin === fixtures.origins.b && r.path === '/form').length === 0, { expected: 'origin B never fetched', observed: fixtures.state().requests.filter((r) => r.origin === fixtures.origins.b).slice(0, 5) });

    const form = await openForm(ctx, fixtures.origins.a);
    if (form) {
      const submitRef = refsNamed(form.snapshot, 'Submit', { role: 'button' })[0]?.reference ?? null;
      const acted = await ctx.cli(['click', '--browser-id', form.browserId, '--generation', String(form.generation), '--ref', submitRef]);
      ev.assert('q3:granted-origin-act', acted.code === 0 && acted.json?.dispatched === true, { expected: 'dispatched', observed: acted.json ?? acted.stderr.slice(0, 240) });
    }

    const revoked = await ctx.cli(['controllers', 'revoke']);
    ev.assert('q3:controller-revoked', revoked.code === 0, { expected: 'revoked', observed: revoked.json ?? revoked.stderr.slice(0, 200) });
    const after = await ctx.cli(['list']);
    ev.assert('q3:revoked-still-refused-after-restart', cliErrorCode(after) === CODES.revoked, { expected: CODES.revoked, observed: { code: after.code, err: after.stderr.slice(0, 200) } });

    // Upload-revoke sub-step of the plan cannot run: no upload surface exists in this build.
    const pipe = ctx.pipe();
    try {
      const upload = await pipe.session.send({ op: 'upload', tab_id: 'any', path: '/etc/hostname', ref: 'any' });
      ev.assert('q3:upload-revoke-step', errorCode(upload) !== CODES.unsupported, {
        expected: 'an upload op to revoke (product gap: no upload op in PIPE_OPS, browser/agent/pipe.rs:295)',
        observed: { ok: upload.ok, error: upload.error },
      });
    } finally {
      await pipe.close({ timeout: 10_000 }).catch(() => {});
    }
  },

  // Q4: two controllers are separate identities; a second controller cannot use the first's
  // grant; revoking one leaves the other intact.
  async Q4(ctx) {
    const { ev, fixtures } = ctx;
    const first = await pairController(ctx, 'qa-a');
    if (!first) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

    const second = await pairController(ctx, 'qa-b');
    if (!second) return;
    ev.assert('q4:distinct-controllers', first !== second, { expected: 'distinct ids', observed: { first, second } });

    // qa-b is the default selection now (pairing replaces default.json); it holds no grant.
    const asSecond = await ctx.cli(['open', '--url', `${fixtures.origins.a}/form`]);
    ev.assert('q4:second-controller-needs-own-grant', asSecond.code !== 0, { expected: 'refused without its own grant', observed: { code: asSecond.code, err: asSecond.stderr.slice(0, 240) } });

    // `controllers list` is the calling controller's own record by design
    // (agent/service.rs::controller_summary filters to `summary.id == id`), so two pairings do not
    // mean two rows here: the whole workspace list is the desktop-only `cmd_browser_agent_overview`.
    // The row must be qa-b's own identity, not an empty list or a different controller.
    const list = await ctx.cli(['controllers', 'list']);
    const controllers = Array.isArray(list.json) ? list.json : [];
    ev.assert('q4:both-listed', list.code === 0 && callerControllerListed(controllers, second), {
      expected: 'the caller sees only its own record, once',
      observed: controllers.map((c) => c.id ?? c),
    });

    const explicit = await ctx.cli(['list', '--controller', first]);
    ev.assert('q4:explicit-controller-selects-granted', explicit.code === 0, { expected: 'qa-a works by --controller', observed: { code: explicit.code, err: explicit.stderr.slice(0, 200) } });

    const revoked = await ctx.cli(['controllers', 'revoke']);
    ev.assert('q4:revoke-is-per-controller', revoked.code === 0, { expected: 'revoked', observed: revoked.json ?? revoked.stderr.slice(0, 200) });
    const otherAlive = await ctx.cli(['list', '--controller', first]);
    ev.assert('q4:other-controller-unaffected', otherAlive.code === 0, { expected: 'qa-a still authorised', observed: { code: otherAlive.code, err: otherAlive.stderr.slice(0, 200) } });
  },

  // Q5: hostile page. Stale ref after node replacement, duplicate locators, password values never
  // observed, cross-origin iframe stays opaque, tampered globals do not break the agent, and an
  // act carrying a superseded generation is refused.
  async Q5(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pairController(ctx, 'qa-a'))) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

    const events = await ctx.subscribe(fixtures.origins.a);
    try {
      // Subscribe to the page's control-stream readiness BEFORE opening /replace so an early
      // EventSource open cannot be missed before we await it.
      const controlReady = events.waitFor((r) => r.kind === 'page:control-ready', { label: 'replace control ready', timeoutMs: 30_000 });
      const opened = await ctx.cli(['open', '--url', `${fixtures.origins.a}/replace`]);
      const browserId = opened.json?.browser?.browserId ?? null;
      ev.assert('q5:open-/replace', typeof browserId === 'string', { expected: 'opened', observed: opened.json ?? opened.stderr.slice(0, 200) });
      if (!browserId) return;
      await ctx.cli(['wait', '--browser-id', browserId, '--text', 'Replace fixture']);
      const first = await ctx.cli(['snapshot', '--browser-id', browserId]);
      const snapshot = first.json;

      ev.assert('q5:password-not-observed', noPasswordValue(snapshot, PASSWORD_SENTINEL), { expected: 'no password value in snapshot', observed: String(first.stdout).includes(PASSWORD_SENTINEL) ? 'sentinel leaked' : 'absent' });
      ev.assert('q5:iframe-opaque', framesAreOpaque(snapshot, FRAME_SENTINEL), { expected: 'frame present, sentinel absent', observed: { frames: frameCount(snapshot), sentinel: String(first.stdout).includes(FRAME_SENTINEL) } });
      ev.assert('q5:tampered-globals-survived', Array.isArray(snapshot?.elements) && snapshot.elements.length > 0, { expected: 'elements observed despite tampering', observed: { count: snapshot?.elements?.length } });

      const duplicates = refsNamed(snapshot, 'Delete', { role: 'button' });
      ev.assert('q5:duplicate-locators-distinct', new Set(duplicates.map((d) => d.reference)).size === duplicates.length && duplicates.length >= 2, { expected: 'distinct refs for equal names', observed: duplicates.map((d) => d.reference) });

      const targetRef = findRef(snapshot, (e) => String(e.name ?? '').trim() === 'Target action');
      const gen = actGeneration(snapshot);
      await controlReady;
      await Promise.all([
        events.waitFor((r) => r.kind === 'page:node-replaced' && r.reason === 'control', { label: 'node replaced', timeoutMs: 15_000 }),
        fetch(`${fixtures.origins.a}/__control`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ action: 'replace' }) }),
      ]);
      const stale = await ctx.cli(['click', '--browser-id', browserId, '--generation', String(gen), '--ref', targetRef]);
      ev.assert('q5:stale-ref-refused', cliErrorCode(stale) === 'BROWSER_STALE_REF', { expected: 'BROWSER_STALE_REF', observed: stale.json ?? stale.stderr.slice(0, 240) });

      const oldGeneration = await ctx.cli(['click', '--browser-id', browserId, '--generation', String(Math.max(0, gen - 1)), '--ref', targetRef]);
      ev.assert('q5:old-generation-denied', oldGeneration.code !== 0 || oldGeneration.json?.applied === false, { expected: 'denied', observed: oldGeneration.json ?? oldGeneration.stderr.slice(0, 240) });
    } finally {
      events.close();
    }
  },

  // Q6: pipe lifecycle and MCP contract. Two requests answered in order, EOF exits bounded,
  // malformed input answered then the stream continues, a pipe op outside the advertised set is
  // refused by name, an impossible postcondition reports applied:false, and a blocking dialog
  // prevents mutation until it is answered.
  async Q6(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pairController(ctx, 'qa-a'))) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

    const pipe = ctx.pipe();
    let exitCode = null;
    try {
      const listed = await pipe.session.send({ op: 'list' });
      ev.assert('q6:pipe-list', listed.ok === true, { expected: 'ok', observed: listed.error ?? listed.result });

      pipe.session.sendRaw('{not json\n');
      const afterMalformed = await pipe.session.send({ op: 'list' });
      ev.assert('q6:malformed-then-next-served', afterMalformed.ok === true, { expected: 'next frame served', observed: afterMalformed.error ?? 'ok' });
      const malformed = pipe.session.unsolicited.find((u) => u.frame?.error?.code === CODES.frameMalformed);
      ev.assert('q6:malformed-typed', Boolean(malformed), { expected: CODES.frameMalformed, observed: pipe.session.unsolicited.map((u) => u.frame?.error?.code ?? 'frame') });

      const oversized = await pipe.session.send({ op: 'list', pad: 'x'.repeat(MAX_FRAME_BYTES + 1024) }, { allowOversize: true, timeout: 20_000 });
      ev.assert('q6:oversize-refused', errorCode(oversized) === CODES.frameTooLarge, { expected: CODES.frameTooLarge, observed: oversized.error ?? oversized });

      const unknown = await pipe.session.send({ op: 'teleport' });
      ev.assert('q6:unknown-op-typed', errorCode(unknown) === CODES.unknownOp, { expected: CODES.unknownOp, observed: unknown.error ?? unknown });

      // request_help is in the plan but not in this build's op list (product gap, pipe.rs:295).
      const help = await pipe.session.send({ op: 'request_help', tab_id: 'any' });
      ev.assert('q6:request_help-step', errorCode(help) !== CODES.unknownOp, {
        expected: 'a request_help op (product gap: absent from PIPE_OPS, browser/agent/pipe.rs:295)',
        observed: { ok: help.ok, error: help.error },
      });

      const opened = await pipe.session.send({ op: 'open', url: `${fixtures.origins.a}/form` });
      const browserId = opened.result?.browser?.browserId ?? null;
      ev.assert('q6:pipe-open', typeof browserId === 'string', { expected: 'session', observed: opened.error ?? opened.result });

      // `open` is answered when the session exists, before its first document commits (the same race
      // Q8 documents for `navigate`), so an immediate `observe` is refused with
      // "webview evaluation was cancelled". The pipe's own `wait` op is the documented way to wait
      // for that, and the page is then observed in a settled state.
      await pipe.session.send({ op: 'wait', tab_id: browserId, condition: { condition: 'loadState', state: 'load' } }, { timeout: 30_000 });
      const snapshot = await pipe.session.send({ op: 'observe', tab_id: browserId });
      // The pipe serializes `BrowserCliResponse::Snapshot{snapshot}` (browser_cli.rs:1664), so the
      // observe payload is `result.snapshot`; accept the bare form too, never a missing object.
      const observed = snapshot.result?.snapshot ?? snapshot.result ?? null;
      const submitRef = refsNamed(observed, 'Submit', { role: 'button' })[0]?.reference ?? null;
      const gen = actGeneration(observed);
      ev.assert('q6:pipe-observe', Boolean(observed?.browserId) && typeof gen === 'number', { expected: 'snapshot with generation', observed: { browserId: observed?.browserId, generation: gen, error: snapshot.error } });
      // A postcondition the page can never satisfy must report applied:false, not a pass.
      const impossible = await pipe.session.send({ op: 'act', tab_id: browserId, generation: gen, action: { kind: 'click', ref: submitRef }, postcondition: textPostcondition('never-appears-9f3c', 2_000) }, { timeout: 30_000 });
      ev.assert('q6:impossible-postcondition', impossible.ok === true && impossible.result?.applied === false, { expected: 'applied:false', observed: impossible.error ?? impossible.result });

      // Real request_help handoff on the opened tab: must succeed and block subsequent agent mutations.
      const handedOff = await pipe.session.send({ op: 'request_help', tab_id: browserId, reason: 'qa handoff verification' });
      ev.assert('q6:request_help-handoff', handedOff.ok === true && handedOff.result?.handedOff === true, { expected: 'handedOff:true', observed: handedOff.error ?? handedOff.result });
      const afterHandoff = await pipe.session.send({ op: 'act', tab_id: browserId, generation: gen, action: { kind: 'click', ref: submitRef } });
      ev.assert('q6:act-blocked-after-handoff', afterHandoff.ok === false, { expected: 'refusal after handoff', observed: afterHandoff.error ?? afterHandoff.result });
    } finally {
      exitCode = await pipe.close({ timeout: 15_000 }).catch(() => null);
      ev.assert('q6:stdout-frames-only', pipe.session.errors.length === 0, { expected: 'zero stdout decoder errors, including at EOF', observed: { errors: pipe.session.errors.map((e) => e.code) } });
    }
    ev.assert('q6:eof-exits-bounded', typeof exitCode === 'number', { expected: 'exit code after EOF', observed: exitCode });

    // Subscribe before triggering the dialog so its result cannot race the fixture-state read.
    const events = await ctx.subscribe(fixtures.origins.a);
    try {
      const dialogOpen = await ctx.cli(['open', '--url', `${fixtures.origins.a}/dialog`]);
      const dialogBrowser = dialogOpen.json?.browser?.browserId ?? null;
      ev.assert('q6:dialog-open', typeof dialogBrowser === 'string', { expected: 'session', observed: dialogOpen.json ?? dialogOpen.stderr.slice(0, 200) });
      if (dialogBrowser) {
        await ctx.cli(['wait', '--browser-id', dialogBrowser, '--load-state', 'load'], 30_000);
        const policySet = await ctx.cli(['dialog', 'policy', '--browser-id', dialogBrowser, '--policy', 'auto-accept']);
        ev.assert('q6:dialog-accept', policySet.code === 0, { expected: 'dialog policy accept set', observed: policySet.json ?? policySet.stderr.slice(0, 200) });
        const dSnap = await ctx.cli(['snapshot', '--browser-id', dialogBrowser]);
        const openBtn = refsNamed(dSnap.json, 'Open confirm', { role: 'button' })[0]?.reference ?? null;
        const dGen = actGeneration(dSnap.json);
        const dialogEvent = events.waitFor((r) => r.kind === 'dialog', { label: 'dialog resolved', timeoutMs: 15_000 });
        await ctx.cli(['click', '--browser-id', dialogBrowser, '--generation', String(dGen), '--ref', openBtn]);
        await dialogEvent;
        const pending = await ctx.cli(['dialog', 'list', '--browser-id', dialogBrowser]);
        ev.assert('q6:dialog-pending-listed', pending.code === 0 && Array.isArray(pending.json?.dialogs) && pending.json.dialogs.length === 1, { expected: 'dialog list answered with 1 entry', observed: pending.json ?? pending.stderr.slice(0, 200) });
        const record = await fetch(`${fixtures.origins.a}/__state`).then((r) => r.json());
        ev.assert('q6:dialog-resolved-once', dialogResolvedOnce(record.dialogs), { expected: 'exactly one accepted dialog resolution', observed: record.dialogs });
      }
    } finally {
      events.close();
    }
    void ev;
  },

  // Q7: screenshot PNG is real and shows the fixture; upload and native input do not exist in this
  // build, so the steps run and their typed refusal is the finding.
  async Q7(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pairController(ctx, 'qa-a'))) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });
    const form = await openForm(ctx, fixtures.origins.a);
    if (!form) return;

    const outPath = path.join(ctx.config.isolation.dataRoot, 'q7-shot.png');
    // `browser.screenshot` carries `Obligation::FileCapability` (agent/authority.rs:634), so the
    // desktop user must approve this exact path once before the tool may write it. The harness used
    // to skip that grant and read the resulting refusal as a broken screenshot; the refusal is the
    // security contract working, and the run has to ask for the capability the way a real agent
    // would.
    await grantScope(ctx, { tool: 'browser.screenshot', path: outPath });
    const shot = await ctx.cli(['screenshot', '--browser-id', form.browserId, '--out', outPath], 40_000);
    ev.assert('q7:screenshot-written', shot.code === 0 && existsSync(outPath), { expected: 'PNG path', observed: { stdout: shot.stdout.trim(), err: shot.stderr.slice(0, 200) } });
    if (existsSync(outPath)) {
      const bytes = readFileSync(outPath);
      const size = pngSize(bytes);
      ev.assert('q7:screenshot-is-png', Boolean(size && size.width > 100 && size.height > 100), { expected: 'PNG with real dimensions', observed: size });
      ev.writePng('q7-page.png', bytes);
      ev.assert('q7:screenshot-nonblank', sha256(bytes).length === 64 && bytes.length > 5_000, { expected: 'more than a blank frame', observed: { bytes: bytes.length, sha: sha256(bytes).slice(0, 16) } });
    } else {
      ev.assert('q7:screenshot-is-png', false, { expected: 'file written', observed: shot.stderr.slice(0, 300) });
    }

    const pipe = ctx.pipe();
    try {
      const listed = await pipe.session.send({ op: 'list' });
      ev.assert('q7:pipe-alive', listed.ok === true, { expected: 'ok', observed: listed.error ?? 'ok' });
      const upload = await pipe.session.send({ op: 'upload', tab_id: form.browserId, path: outPath, ref: 'any' });
      ev.assert('q7:upload-supported', errorCode(upload) !== CODES.unsupported, {
        expected: 'an upload op (product gap: no upload/native surface, browser/agent/pipe.rs:295)',
        observed: { ok: upload.ok, error: upload.error },
      });
      const native = await pipe.session.send({ op: 'native', tab_id: form.browserId, action: { kind: 'key', key: 'Tab' } });
      ev.assert('q7:native-supported', errorCode(native) !== CODES.unsupported, {
        expected: 'a native input op (product gap: browser/agent/pipe.rs:295)',
        observed: { ok: native.ok, error: native.error },
      });
    } finally {
      await pipe.close({ timeout: 10_000 }).catch(() => {});
    }

    // Unauthorized upload bytes never reached the fixture server before file-capability approval.
    const uploadsBefore = fixtures.state().uploads;
    ev.assert('q7:no-upload-recorded', noUploadsRecorded(uploadsBefore), { expected: 'an empty upload ledger before approval', observed: uploadsBefore });

    // Real approved upload on /upload: approve one-shot browser.upload file capability, attach
    // known bytes via pipe `upload`, submit, and verify exact SHA-256 and size on the fixture server.
    const uploadPath = path.join(ctx.config.isolation.dataRoot, 'q7-upload.txt');
    const uploadBytes = Buffer.from(`ferryx-q7-upload-proof-${Date.now()}\n`, 'utf8');
    writeFileSync(uploadPath, uploadBytes);
    const expectedSha = sha256(uploadBytes);
    const uploadOpened = await ctx.cli(['open', '--url', `${fixtures.origins.a}/upload`]);
    const uploadTab = uploadOpened.json?.browser?.browserId ?? null;
    if (uploadTab) {
      await ctx.cli(['wait', '--browser-id', uploadTab, '--load-state', 'load'], 30_000);
      const snap = await ctx.cli(['snapshot', '--browser-id', uploadTab]);
      const uploadSnap = snap.json?.snapshot ?? snap.json ?? null;
      const uploadGen = uploadSnap?.generation ?? null;
      const uploadEls = Array.isArray(uploadSnap?.elements) ? uploadSnap.elements : [];
      const fileInputRef = uploadEls.find((e) => String(e?.tagName || '').toLowerCase() === 'input')?.reference ?? null;
      const uploadBtnRef = refsNamed(uploadSnap, 'Upload', { role: 'button' })[0]?.reference ?? null;
      ev.assert('q7:upload-form-observed', Boolean(fileInputRef && uploadBtnRef && typeof uploadGen === 'number'), { expected: 'file input and Upload button refs', observed: { fileInputRef, uploadBtnRef, uploadGen } });
      if (fileInputRef && uploadBtnRef && typeof uploadGen === 'number') {
        await grantScope(ctx, { tool: 'browser.upload', path: uploadPath });
        const pipe2 = ctx.pipe();
        try {
          const uploaded = await pipe2.session.send({ op: 'upload', tab_id: uploadTab, path: uploadPath, ref: fileInputRef, generation: uploadGen }, { timeout: 30_000 });
          ev.assert('q7:approved-upload-attached', uploaded.ok === true && uploaded.result?.sha256 === expectedSha, { expected: { ok: true, sha256: expectedSha }, observed: uploaded.error ?? uploaded.result });
          const submitted = await pipe2.session.send({ op: 'act', tab_id: uploadTab, generation: uploadGen, action: { kind: 'click', ref: uploadBtnRef }, postcondition: textPostcondition('Uploaded', 15_000) }, { timeout: 30_000 });
          ev.assert('q7:approved-upload-submitted', submitted.ok === true, { expected: 'submitted', observed: submitted.error ?? submitted.result });
        } finally {
          await pipe2.close({ timeout: 10_000 }).catch(() => {});
        }
        const uploadsAfter = fixtures.state().uploads;
        ev.assert('q7:approved-upload-sha-matches', Array.isArray(uploadsAfter) && uploadsAfter.length === 1 && uploadMatches(uploadsAfter[0], expectedSha, uploadBytes.length), {
          expected: { sha256: expectedSha, size: uploadBytes.length },
          observed: uploadsAfter,
        });
      }
    }

    // Real approved native input: approve one-shot browser.native_input capability, dispatch Key Tab.
    // On Chromium engine tabs this delivers trusted input; on system webviews it refuses honestly
    // with BROWSER_PLATFORM_UNSUPPORTED (never fabricating trusted input).
    await grantScope(ctx, { tool: 'browser.native_input' });
    const pipe3 = ctx.pipe();
    try {
      const nativeApproved = await pipe3.session.send({ op: 'native', tab_id: form.browserId, action: { kind: 'key', key: 'Tab' } }, { timeout: 20_000 });
      const engineTab = (ctx.config.engine ?? 'webview') === 'chromium';
      const nativeOk = engineTab
        ? (nativeApproved.ok === true && nativeApproved.result?.inputMode === 'trusted')
        : (nativeApproved.ok === true || errorCode(nativeApproved) === 'BROWSER_PLATFORM_UNSUPPORTED' || errorCode(nativeApproved) === 'UNSUPPORTED');
      ev.assert('q7:approved-native-honest', nativeOk, {
        expected: engineTab ? 'ok with inputMode=trusted' : 'trusted dispatch or typed platform unsupported',
        observed: nativeApproved.error ?? nativeApproved.result,
      });
    } finally {
      await pipe3.close({ timeout: 10_000 }).catch(() => {});
    }
  },

  // Q8: adjacent regressions on the surfaces this build does have: CLI navigation/snapshot stay
  // correct, download cancel is typed, dialog policy round-trips, and tab close works.
  async Q8(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pairController(ctx, 'qa-a'))) return;
    await grantScope(ctx, { origin: fixtures.origins.a, scopes: ['read', 'interact'] });

    const opened = await ctx.cli(['open', '--url', `${fixtures.origins.a}/`]);
    const browserId = opened.json?.browser?.browserId ?? null;
    ev.assert('q8:open-index', typeof browserId === 'string', { expected: 'session', observed: opened.json ?? opened.stderr.slice(0, 200) });
    if (!browserId) return;

    const navigated = await ctx.cli(['navigate', '--browser-id', browserId, '--url', `${fixtures.origins.a}/form`]);
    ev.assert('q8:navigate', navigated.code === 0, { expected: 'navigated', observed: navigated.stdout || navigated.stderr.slice(0, 200) });
    const url = await ctx.cli(['url', '--browser-id', browserId]);
    // `url` prints the raw snapshot URL (cli.rs:3979), but a `navigate` is answered before the new
    // document commits, so reading it once raced the commit and saw the previous URL. Waiting on
    // the documented condition first is what `wait --url-contains` exists for.
    if (!url.stdout.trim().endsWith('/form')) {
      await ctx.cli(['wait', '--browser-id', browserId, '--url-contains', '/form'], 15_000);
    }
    const settledUrl = await ctx.cli(['url', '--browser-id', browserId]);
    ev.assert('q8:url-reflects-navigation', settledUrl.stdout.trim().endsWith('/form'), { expected: '/form', observed: settledUrl.stdout.trim().slice(0, 160) });

    const snap = await ctx.cli(['snapshot', '--browser-id', browserId]);
    ev.assert('q8:snapshot-after-navigation', Boolean(snap.json?.browserId) && Array.isArray(snap.json?.elements), { expected: 'elements', observed: { count: snap.json?.elements?.length } });

    const back = await ctx.cli(['back', '--browser-id', browserId]);
    ev.assert('q8:back', back.code === 0, { expected: 'back', observed: back.stdout || back.stderr.slice(0, 200) });

    const cancelled = await ctx.cli(['download', 'cancel', '--browser-id', browserId, '--id', 'no-such-download']);
    ev.assert('q8:download-cancel-typed', cancelled.code !== 0 || cancelled.json?.cancelled === false, { expected: 'typed refusal or cancelled:false', observed: { code: cancelled.code, out: (cancelled.stdout || cancelled.stderr).slice(0, 200) } });

    // `dialog policy <auto-dismiss|auto-accept> --browser-id <id>`; the read is the act of setting.
    const policy = await ctx.cli(['dialog', 'policy', 'auto-dismiss', '--browser-id', browserId]);
    ev.assert('q8:dialog-policy-set', policy.code === 0, { expected: 'policy set', observed: policy.stdout.slice(0, 200) || policy.stderr.slice(0, 240) });

    const tabList = await ctx.cli(['tab', 'list']);
    ev.assert('q8:tab-list', tabList.code === 0, { expected: 'list', observed: tabList.stdout.slice(0, 200) || tabList.stderr.slice(0, 200) });

    const closed = await ctx.cli(['close', '--browser-id', browserId]);
    ev.assert('q8:close', closed.code === 0, { expected: 'closed', observed: closed.stdout || closed.stderr.slice(0, 200) });
  },

  // Q9: the consent card itself. A pairing request puts a card on screen; the harness finds it in a
  // live screenshot, proves it names the controller, approves it, and (for a second request) proves
  // deny also resolves exactly once.
  async Q9(ctx) {
    const { ev } = ctx;
    const gui = await ctx.gui().catch(() => null);
    if (!gui) {
      ev.skip('q9:gui-connection', 'MahoRD driver unavailable for this host');
      return;
    }
    const focusBefore = await ctx.focusWindow?.() ?? null;
    if (focusBefore !== null) ev.assert('q9:window-focused', true, { expected: 'app window', observed: focusBefore });

    // One pairing request, approved by clicking the card.
    const pendingPair = ctx.cli(['controllers', 'pair', '--name', 'qa-q9'], 90_000);
    const answered = await answerConsentCard(ctx, { action: 'approve', label: 'q9-pair', within: ctx.guiWindow });
    const paired = await pendingPair;
    ev.assert('q9:card-located', answered.answered, { expected: 'consent card found in the app window', observed: answered.reason ?? answered.card });
    if (answered.answered) {
      ev.assert('q9:card-top-of-window', answered.card.top <= (ctx.guiWindow?.top ?? 0) + 220, { expected: 'card near the window top', observed: answered.card });
      ev.assert('q9:approve-resolves-pair', paired.code === 0 && paired.json?.type === 'paired', { expected: 'paired', observed: paired.json ?? paired.stderr.slice(0, 240) });
    } else {
      ev.assert('q9:approve-resolves-pair', false, { expected: 'paired', observed: paired.stderr.slice(0, 300) });
    }

    // A second request answered with Deny must resolve as denied, not as a pairing.
    if (paired.code === 0) {
      const secondPair = ctx.cli(['controllers', 'pair', '--name', 'qa-q9-deny'], 90_000);
      const denied = await answerConsentCard(ctx, { action: 'deny', label: 'q9-deny', within: ctx.guiWindow });
      const second = await secondPair;
      ev.assert('q9:deny-card-located', denied.answered, { expected: 'second card found', observed: denied.reason ?? denied.card });
      ev.assert('q9:deny-resolves-without-pairing', second.code !== 0, { expected: 'pair not granted', observed: { code: second.code, err: second.stderr.slice(0, 240) } });
    } else {
      ev.skip('q9:deny-path', 'pairing was never approved, so the deny path could not be exercised');
    }

    const shot = await captureFrame(gui, { deadlineMs: 10_000 }).catch(() => null);
    if (shot) ev.writePng('q9-final.png', shot.bytes);
  },
};

export const RUNNERS = Object.freeze(Object.fromEntries(SCENARIOS.map((q) => [q, async (ctx) => {
  await preflight(ctx);
  if (!SCENARIO_BODIES[q]) throw new Error(`no body for ${q}`);
  return SCENARIO_BODIES[q](ctx);
}])));

// ---------- main ----------

export async function main(argv = process.argv.slice(2)) {
  let args;
  let config;
  try {
    args = parseArgs(argv);
    config = loadConfig(args.config);
  } catch (error) {
    if (!(error instanceof UsageError)) throw error;
    process.stderr.write(`${error.message}\nusage: bun scripts/qa/browser-agent/run.mjs --config <host.json> --scenario Q1..Q9|all --evidence <dir>\n`);
    return 2;
  }
  const binaries = { gui: resolveCandidate(config.binaries.gui), cli: resolveCandidate(config.binaries.cli) };
  const receiptBase = {
    os: config.os,
    engine: config.engine ?? 'webview',
    host: config.hostLabel ?? null,
    ...await receiptProvenance(binaries),
    runtime: { bun: Bun.version, platform: process.platform, arch: process.arch },
    binaries,
    fixtures: null,
    startedAt: new Date().toISOString(),
  };

  let fixtures;
  try {
    fixtures = await startFixtures();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 1;
  }
  receiptBase.fixtures = fixtures.origins;

  const guiWindow = config.windowRect ?? null;
  const results = {};
  try {
    for (const q of args.scenarios) {
      const ev = createEvidence(args.evidence, evidenceLabel(config), q);
      const ctx = createContext({ config, binaries, fixtures, ev, guiWindow });
      fixtures.reset();
      let failure = null;
      try {
        await RUNNERS[q](ctx);
      } catch (error) {
        failure = error;
      } finally {
        await ctx.close();
      }
      ev.transcript({ fixtureState: fixtures.state() });
      results[q] = ev.finish(receiptBase, failure);
      process.stderr.write(`${q}: ${results[q]}\n`);
    }
  } finally {
    await fixtures.stop();
  }
  const teardown = { ports: verifyPortsReleased(), results };
  mkdirSync(path.join(args.evidence, evidenceLabel(config)), { recursive: true });
  writeFileSync(path.join(args.evidence, evidenceLabel(config), 'teardown.json'), `${JSON.stringify(teardown, null, 2)}\n`);
  const portsOk = teardown.ports.every((p) => p.released);
  if (!portsOk) process.stderr.write(`fixture ports not released: ${JSON.stringify(teardown.ports)}\n`);
  return portsOk && Object.values(results).every((s) => s === 'PASS') ? 0 : 1;
}

if (import.meta.main) {
  process.exitCode = await main();
}
