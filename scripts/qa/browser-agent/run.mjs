#!/usr/bin/env bun
// Browser-agent integrated QA harness (plan maho-browser-merge, T8 "Concrete QA contract").
//
//   bun scripts/qa/browser-agent/run.mjs --config <host.json> --scenario Q1|...|Q9|all --evidence <dir>
//
// The config is validated against config.schema.json BEFORE anything is launched. Each scenario
// writes <evidence>/<os>/<Q>/{transcript.jsonl,assertions.json,receipt.json} (plus native
// before.png/after.png for visible changes). A scenario passes only when every recorded assertion
// passed, at least one assertion exists, and no step reported NOT_IMPLEMENTED. Steps that need
// `ferryx browser pipe|mcp|controllers` (T6) or the platform adapters (T7) are NOT_IMPLEMENTED in
// this skeleton and therefore FAIL; nothing here fabricates a pass. Native GUI actions go through
// MahoRD only (connectMahord); no web-only mock counts as a native pass.
//
// Exit code: 0 only if every selected scenario PASSED; 1 on any FAIL; 2 on usage/config errors.

import { appendFileSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { connectMahord } from '../daemon-lifetime/lib/mahord.mjs';
import { startFixtures, subscribeEvents, verifyPortsReleased } from './fixtures.mjs';
import { spawnPipe } from './protocol.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '../../..');
export const SCENARIOS = Object.freeze(['Q1', 'Q2', 'Q3', 'Q4', 'Q5', 'Q6', 'Q7', 'Q8', 'Q9']);
export const ENGINES = Object.freeze(['webview', 'chromium']);
export const NOT_IMPLEMENTED = 'NOT_IMPLEMENTED';
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

/** Schema + semantic checks that a JSON Schema cannot express. Throws UsageError on failure. */
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

/** First existing executable among candidates, or null. */
export function resolveCandidate(candidates) {
  for (const candidate of candidates) {
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
  const notImplemented = [];
  return {
    dir,
    transcript(entry) { appendFileSync(transcriptPath, `${JSON.stringify(redact({ at: new Date().toISOString(), ...entry }))}\n`); },
    assert(id, pass, { expected, observed } = {}) {
      const entry = { id, pass: pass === true, expected, observed: redact(observed) };
      assertions.push(entry);
      this.transcript({ assertion: entry });
      return entry.pass;
    },
    notImplemented(step, dependsOn) {
      const entry = { step, status: NOT_IMPLEMENTED, dependsOn };
      notImplemented.push(entry);
      assertions.push({ id: `${step}:${NOT_IMPLEMENTED}`, pass: false, expected: 'implemented step', observed: entry });
      this.transcript(entry);
    },
    writePng(name, bytes) {
      if (!bytes) throw new Error(`no PNG bytes for ${name}`);
      writeFileSync(path.join(dir, name), bytes);
    },
    finish(receipt, error) {
      if (error) assertions.push({ id: 'scenario:exception', pass: false, observed: String(error?.stack ?? error) });
      const status = assertions.length > 0 && assertions.every((a) => a.pass) && notImplemented.length === 0 ? 'PASS' : 'FAIL';
      writeFileSync(path.join(dir, 'assertions.json'), `${JSON.stringify({ scenario, status, notImplemented, assertions }, null, 2)}\n`);
      writeFileSync(path.join(dir, 'receipt.json'), `${JSON.stringify({ scenario, status, ...receipt, finishedAt: new Date().toISOString() }, null, 2)}\n`);
      return status;
    },
  };
}

async function gitSha() {
  try {
    const proc = Bun.spawn(['git', '-C', REPO, 'rev-parse', 'HEAD'], { stdout: 'pipe', stderr: 'pipe' });
    const [code, out] = await Promise.all([proc.exited, new Response(proc.stdout).text()]);
    return code === 0 ? out.trim() : null;
  } catch { return null; }
}

// ---------- scenario context ----------

function createContext({ config, binaries, fixtures, ev }) {
  let gui = null;
  return {
    config,
    binaries,
    fixtures,
    ev,
    /** Subscribe to fixture SSE BEFORE acting (bounded). */
    subscribe: (origin = fixtures.origins.a) => subscribeEvents(origin, { timeoutMs: config.timeouts?.readyMs ?? 10_000 }),
    /** Lazily connects the MahoRD native GUI driver (first native step only). */
    async gui() {
      gui ??= await connectMahord(
        { kind: 'mahord', host: config.mahord.host, pairingId: config.mahord.pairingId },
        { binary: config.mahord.binary ?? 'maho-client', log: (entry) => ev.transcript({ mahord: entry }) },
      );
      return gui;
    },
    /** Spawns `ferryx browser pipe` with the isolated environment (T6). */
    pipe(extraArgs = []) {
      return spawnPipe([binaries.cli, 'browser', 'pipe', ...extraArgs], {
        env: isolatedEnv(config),
        timeoutMs: config.timeouts?.requestMs ?? 15_000,
        onTranscript: (entry) => ev.transcript({ pipe: entry }),
      });
    },
    /** Options for runCli: isolated env (engine selection included) and a bound. */
    cliOptions(timeoutMs = config.timeouts?.requestMs ?? 15_000) {
      return { env: isolatedEnv(config), timeoutMs };
    },
    async close() { if (gui) await gui.close().catch(() => {}); },
  };
}

/**
 * Child env pointing every data/runtime root at the isolated QA roots; never mutates process env.
 * FERRYX_DATA_DIR isolates the controller store and the agent registry; the engine choice is
 * FERRYX_CEF_ENGINE_DIR (set only for the chromium engine).
 */
export function isolatedEnv(config) {
  const { runtimeRoot, dataRoot, profileRoot, tmpRoot } = config.isolation;
  return {
    PATH: process.env.PATH ?? '',
    ...(process.platform === 'win32' ? { SystemRoot: process.env.SystemRoot ?? '' } : {}),
    TMPDIR: tmpRoot ?? path.join(runtimeRoot, 'tmp'),
    TEMP: tmpRoot ?? path.join(runtimeRoot, 'tmp'),
    FERRYX_RUNTIME_DIR: runtimeRoot,
    FERRYX_DATA_DIR: dataRoot,
    FERRYX_QA_RUNTIME_DIR: runtimeRoot,
    FERRYX_QA_DATA_DIR: dataRoot,
    FERRYX_QA_PROFILE_DIR: profileRoot,
    ...(config.engine === 'chromium' ? { FERRYX_CEF_ENGINE_DIR: config.engineDir } : {}),
  };
}

/**
 * Runs one `ferryx browser ...` command in the isolated env and returns { code, stdout, stderr,
 * json } (json = parsed stdout when it is JSON). Bounded by `timeoutMs`; the child is killed on
 * timeout and that is reported as code null.
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
 * "message":"<SERVER_CODE>: <text>"}` on stderr; the server code is the part of `message`
 * before the first `: ` (the CLI formats it that way).
 */
export function cliErrorCode(result) {
  let error = null;
  try { error = JSON.parse(result.stderr.trim().split('\n').at(-1)); } catch { return null; }
  if (error?.type !== 'error' || typeof error.message !== 'string') return null;
  const head = error.message.split(': ', 1)[0];
  return /^[A-Z][A-Z0-9_]+$/.test(head) ? head : error.code ?? null;
}

// Common preflight recorded by every scenario: fixtures reachable on both origins and the SSE
// subscription is live before any action. These are harness assertions, never a scenario pass.
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
    ev.assert('preflight:gui-candidate', ctx.binaries.gui !== null, { expected: 'existing GUI candidate', observed: ctx.binaries.gui });
  } finally {
    for (const sub of subs) sub.close();
  }
}

// Scenario bodies. Each lists the steps of its plan row; steps whose product surface does not
// exist yet report NOT_IMPLEMENTED (a FAIL). Q1 and Q3 drive the CLI path (pairing, grants,
// observe/act) on the configured engine; consent is always clicked in the real app via MahoRD.
const PENDING = {
  Q1: [['pipe/MCP form of the same flow (`ferryx browser pipe`)', 'T6']],
  Q2: [['token-only legacy NDJSON, raw eval/get/cookies/state, malformed and >1 MiB frames, forged role, unauthorized RemoteAttach', 'T3,T6'],
    ['MCP tools/call without pairing; paired old-syntax CLI with default.json and --controller; missing default and revoke refusals', 'T6']],
  Q3: [['/redirect-other to 18762 stopped before commit (needs a navigation delegate check)', 'T4'],
    ['upload revoked via GUI before chooser completes; credentials after restart', 'T6,T7']],
  Q4: [['qa-a lease vs concurrent qa-b + remote claim behind a start barrier; GUI Take control / Resume; disconnect owner', 'T6']],
  Q5: [['/replace: stale ref after node replacement, ambiguous locator, password omitted, opaque iframe, tampered globals, old generation denied', 'T6']],
  Q6: [['real `browser pipe` two requests then EOF, malformed input, bounded exit, no stdout logs', 'T6'],
    ['real `browser mcp` initialize/initialized/list/call; impossible postcondition; /dialog blocks mutation; request_help', 'T6']],
  Q7: [['screenshot /form PNG decode + visible fixture', 'T7'],
    ['GUI-approved armed chooser upload to /upload; server SHA equals fixture', 'T7'],
    ['native key only if advertised (isTrusted=true), else typed refusal before input', 'T7']],
  Q8: [['existing CLI navigation/snapshot, popup, download cancel, picker feedback, pane drag/restore, remote view-only/control/revoke', 'T6,T7']],
  Q9: [['consent UI duplicate/timeout/revoke via MahoRD; desktop 1440x900 and remote 390x844 light/dark screenshots', 'T5,T6']],
};

/**
 * Answers the consent card in the real app. The card sits top-centre of the Ferryx window;
 * its Allow/Approve button position on this host comes from `config.consentCard.approve`
 * (screen pixels, read once from a screenshot). Without it the step is NOT_IMPLEMENTED and
 * the flow stops: nothing approves consent off the GUI.
 */
async function approveInGui(ctx, label) {
  const gui = await ctx.gui();
  const before = await gui.screenshot();
  if (before) ctx.ev.writePng(`approval-${label}-card.png`, before);
  const point = ctx.config.consentCard?.approve;
  if (!point) {
    ctx.ev.notImplemented(`approve consent card (${label}): set consentCard.approve for this host`, 'T5');
    return false;
  }
  await gui.click(point.x, point.y);
  return true;
}

/** Pairs qa-a through the CLI, approving the card in the GUI. Returns the controller id. */
async function pair(ctx, label) {
  const pending = runCli(ctx.binaries.cli, ['controllers', 'pair', '--name', label], ctx.cliOptions(65_000));
  await approveInGui(ctx, `pair-${label}`);
  // Unanswered, the request expires after 60 s and the CLI reports it.
  const result = await pending;
  ctx.ev.transcript({ cli: 'controllers pair', code: result.code, out: result.json, err: result.stderr.slice(0, 512) });
  ctx.ev.assert(`pair:${label}`, result.code === 0 && typeof result.json?.controllerId === 'string', { expected: 'paired', observed: result.json });
  return result.json?.controllerId ?? null;
}

async function grant(ctx, origin, scopes) {
  const pending = runCli(ctx.binaries.cli, ['controllers', 'grant', '--origin', origin, '--scopes', scopes.join(',')], ctx.cliOptions(65_000));
  await approveInGui(ctx, `grant-${scopes.join('-')}`);
  const result = await pending;
  ctx.ev.assert(`grant:${origin}:${scopes}`, result.json?.resolution === 'approveOnce', { expected: 'approveOnce', observed: result.json });
}

/** Opens the form, snapshots it and returns { browserId, generation, ref(name) }. */
async function openForm(ctx, origin) {
  const opened = await runCli(ctx.binaries.cli, ['open', '--url', `${origin}/form`], ctx.cliOptions());
  const browserId = opened.json?.browser?.browserId;
  ctx.ev.assert('open:/form', typeof browserId === 'string', { expected: 'opened tab', observed: opened.json ?? opened.stderr });
  const loaded = await runCli(ctx.binaries.cli, ['wait', '--browser-id', browserId, '--text', 'Name'], ctx.cliOptions());
  ctx.ev.assert('wait:/form', loaded.code === 0, { expected: 'form loaded', observed: loaded.stderr });
  const snap = await runCli(ctx.binaries.cli, ['snapshot', '--browser-id', browserId], ctx.cliOptions());
  const elements = snap.json?.elements ?? [];
  return {
    browserId,
    snapshot: snap,
    generation: snap.json?.generation,
    ref: (pred) => elements.find(pred)?.reference ?? null,
  };
}

const SCENARIO_BODIES = {
  // Q1: pair, grant, observe, fill, submit on the configured engine; the fixture must record
  // exactly one Name=Ferryx submission.
  async Q1(ctx) {
    const { ev, fixtures } = ctx;
    const events = await ctx.subscribe(fixtures.origins.a);
    try {
      if (!(await pair(ctx, 'qa-a'))) return;
      await grant(ctx, fixtures.origins.a, ['read', 'interact']);
      const form = await openForm(ctx, fixtures.origins.a);
      const name = form.ref((e) => e.tagName === 'input' && /name/i.test(e.name));
      const submit = form.ref((e) => e.tagName === 'button' && /submit/i.test(e.name));
      ev.assert('observe:refs', Boolean(name && submit), { expected: 'Name and Submit refs', observed: form.snapshot.json });
      const gen = String(form.generation);
      const fill = await runCli(ctx.binaries.cli, ['fill', '--browser-id', form.browserId, '--generation', gen, '--ref', name, '--value', 'Ferryx'], ctx.cliOptions());
      ev.assert('act:fill', fill.code === 0, { expected: 'acted', observed: fill.json ?? fill.stderr });
      const click = await runCli(ctx.binaries.cli, ['click', '--browser-id', form.browserId, '--generation', gen, '--ref', submit, '--expect-text', 'Saved', '--expect-timeout-ms', '10000'], ctx.cliOptions());
      ev.assert('act:click-applied', click.code === 0 && click.json?.dispatched === true && click.json?.applied === true, { expected: 'dispatched and applied (Saved shown)', observed: click.json ?? click.stderr });
      const submitted = await events.waitFor((r) => r.kind === 'form-submit', { label: 'form submit' });
      ev.assert('fixture:one-submission', submitted.name === 'Ferryx' && fixtures.state().formSubmits.length === 1, { expected: 'one Name=Ferryx', observed: fixtures.state().formSubmits });
      const saved = await runCli(ctx.binaries.cli, ['wait', '--browser-id', form.browserId, '--text', 'Saved'], ctx.cliOptions());
      ev.assert('page:saved', saved.code === 0, { expected: 'Saved shown', observed: saved.stderr });
      const shot = await (await ctx.gui()).screenshot();
      if (shot) ev.writePng('after.png', shot);
    } finally {
      events.close();
    }
    for (const [step, dependsOn] of PENDING.Q1) ev.notImplemented(step, dependsOn);
  },

  // Q3: a grant for one origin does not cover the other; a revoked grant blocks act; a revoked
  // controller is refused outright.
  async Q3(ctx) {
    const { ev, fixtures } = ctx;
    if (!(await pair(ctx, 'qa-a'))) return;
    await grant(ctx, fixtures.origins.a, ['read', 'interact']);
    const other = await runCli(ctx.binaries.cli, ['open', '--url', `${fixtures.origins.b}/form`], ctx.cliOptions());
    ev.assert('grant:other-origin-refused', other.code !== 0 && cliErrorCode(other) === 'BROWSER_FORBIDDEN', { expected: 'BROWSER_FORBIDDEN', observed: other.stderr });

    const form = await openForm(ctx, fixtures.origins.a);
    const submit = form.ref((e) => e.tagName === 'button');
    // The desktop user withdraws the grant in Settings > Browser > Agent access ("Revoke
    // access"). Reaching that row takes host-specific navigation the harness does not script
    // yet, so the step stays pending and the act check below runs only after a real revoke.
    const revokePoint = ctx.config.consentCard?.revokeGrant;
    if (revokePoint) {
      await (await ctx.gui()).click(revokePoint.x, revokePoint.y);
      const blocked = await runCli(ctx.binaries.cli, ['click', '--browser-id', form.browserId, '--generation', String(form.generation), '--ref', submit], ctx.cliOptions());
      ev.assert('revoked-grant:act-blocked', blocked.code !== 0 && cliErrorCode(blocked) === 'BROWSER_FORBIDDEN', { expected: 'BROWSER_FORBIDDEN', observed: blocked.stderr });
      ev.assert('revoked-grant:no-submission', fixtures.state().formSubmits.length === 0, { expected: 'no submit', observed: fixtures.state().formSubmits });
    } else {
      ev.notImplemented('revoke the origin grant in Settings > Browser > Agent access: set consentCard.revokeGrant (Settings open on that row)', 'QA');
    }

    const revoke = await runCli(ctx.binaries.cli, ['controllers', 'revoke'], ctx.cliOptions());
    ev.assert('controller:revoked', revoke.code === 0, { expected: 'revoked', observed: revoke.json ?? revoke.stderr });
    const after = await runCli(ctx.binaries.cli, ['list'], ctx.cliOptions());
    ev.assert('controller:revoked-refused', after.code !== 0 && cliErrorCode(after) === 'BROWSER_CONTROLLER_REVOKED', { expected: 'BROWSER_CONTROLLER_REVOKED', observed: after.stderr });
    for (const [step, dependsOn] of PENDING.Q3) ev.notImplemented(step, dependsOn);
  },
};

export const RUNNERS = Object.freeze(Object.fromEntries(SCENARIOS.map((q) => [q, async (ctx) => {
  await preflight(ctx);
  if (SCENARIO_BODIES[q]) return SCENARIO_BODIES[q](ctx);
  for (const [step, dependsOn] of PENDING[q]) ctx.ev.notImplemented(step, dependsOn);
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
    sha: await gitSha(),
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

  const results = {};
  try {
    for (const q of args.scenarios) {
      const ev = createEvidence(args.evidence, evidenceLabel(config), q);
      const ctx = createContext({ config, binaries, fixtures, ev });
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
