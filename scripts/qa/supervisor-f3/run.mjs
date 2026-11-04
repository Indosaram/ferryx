#!/usr/bin/env bun
// F3 real-surface acceptance runner (supervisor-chat-redesign F3). Runs on the control host
// (the MacBook that holds MahoRD, tesseract, SSH keys and the WDA endpoint); builds and tests
// nothing locally. See docs/qa/supervisor-f3-real-surfaces.md for the command sequence.
//
//   bun scripts/qa/supervisor-f3/run.mjs --config <f3.json> --revision <40-hex> --evidence <new dir>
//   bun scripts/qa/supervisor-f3/run.mjs --validate <evidence dir>
//
// Topology: the session host (`sessionHost`, session.mjs; omarchy on Linux by default) runs the
// run-owned relay with its embedded account service behind a run-owned `tailscale serve` HTTPS
// endpoint, and an isolated candidate daemon holding the fixture panes. The macOS and Windows
// desktops join the same account through that relay and the iPhone signs in to it in Safari.
//
// Exit: 0 pass, 1 fail, 2 blocked, 5 usage, 130 interrupted. Evidence: <evidence>/run.json,
// verdict.json, manifest.json, <lane>/**. --validate re-reads every artifact from disk.
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { EXIT, LANES, RUN_SCHEMA, scenariosForLane } from './contract.mjs';
import { runDesktopLane } from './desktop-lane.mjs';
import { runIphoneLane } from './iphone-lane.mjs';
import { configProblems } from './seams.mjs';
import { startSession } from './session.mjs';
import { LaneBlocked, markNotRun, newLane } from './steps.mjs';
import { controlIdentity, manifestFiles, verifyEvidence } from './verify.mjs';

const USAGE = 'usage: run.mjs --config <f3.json> --revision <40-hex> --evidence <new dir> | --validate <evidence dir>';
function parseArgs(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 1) {
    const value = () => { const next = argv[i + 1]; if (!next || next.startsWith('--')) throw new Error(`${argv[i]} needs a value`); i += 1; return next; };
    switch (argv[i]) {
      case '--config': opts.config = value(); break;
      case '--revision': opts.revision = value(); break;
      case '--evidence': opts.evidence = value(); break;
      case '--validate': opts.validate = value(); break;
      default: throw new Error(`unknown argument ${argv[i]}`);
    }
  }
  if (opts.validate) return opts;
  if (!opts.config || !opts.evidence || !/^[0-9a-f]{40}$/.test(opts.revision ?? '')) throw new Error(USAGE);
  return opts;
}

function loadConfig(path) {
  const config = JSON.parse(readFileSync(path, 'utf8'));
  const problems = configProblems(config);
  if (problems.length) throw new Error(`config ${path}: ${problems.join('; ')}`);
  return config;
}

function crossChecks(shared, lanes) {
  const cross = [];
  const d1 = shared.observed['F3-D1'] ?? {};
  cross.push({ scenarioId: 'F3-D1', id: 'same-card-both-desktops', ok: Boolean(d1.macos && d1.windows) && JSON.stringify(d1.macos) === JSON.stringify(d1.windows), observed: d1 });
  const d2 = shared.observed['F3-D2'] ?? {};
  cross.push({ scenarioId: 'F3-D2', id: 'same-prompt-and-reply-both-desktops', ok: Boolean(d2.macos && d2.windows) && d2.macos.owner === d2.windows.owner && d2.macos.reply === d2.windows.reply, observed: d2 });
  for (const id of ['F3-D3', 'F3-D4', 'F3-D5', 'F3-D6', 'F3-D7']) {
    const status = (name) => lanes[name].receipt.steps.find((s) => s.scenarioId === id)?.status;
    cross.push({ scenarioId: id, id: 'both-desktops-pass', ok: status('macos') === 'pass' && status('windows') === 'pass', observed: { macos: status('macos'), windows: status('windows') } });
  }
  return cross;
}

/** Session host first (relay, daemon, panes); then both desktops and the phone together. */
async function runLanes({ config, run, lanes, session, shared, signal }) {
  try {
    const who = await shared.probe();
    Object.assign(session.receipt, { machine: who.machine, candidate: who.candidate });
    session.write('identity', 'identity.json', who);
    await shared.start();
    await shared.spawnPanes();
  } catch (error) {
    if (error instanceof LaneBlocked) session.receipt.blocked = { code: error.code, detail: error.detail, at: new Date().toISOString() };
    else session.receipt.failure = { code: error.code ?? 'SESSION_HOST', detail: String(error.detail ?? error.stack ?? error).slice(0, 4000) };
    return;
  }
  await Promise.allSettled([
    runDesktopLane(lanes.windows, { host: config.hosts.windows, run, shared, signal }),
    runDesktopLane(lanes.macos, { host: config.hosts.macos, run, shared, signal }),
    runIphoneLane(lanes.iphone, { device: config.iphone, run, shared }),
  ]);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.validate) {
    const verdict = verifyEvidence(resolve(opts.validate));
    process.stdout.write(`${JSON.stringify(verdict, null, 2)}\n`);
    return verdict.exitCode;
  }
  const config = loadConfig(opts.config);
  const control = controlIdentity();
  const evidenceDir = resolve(opts.evidence);
  if (existsSync(evidenceDir) && readdirSync(evidenceDir).length > 0) throw new Error(`${evidenceDir} is not empty; give a new evidence directory`);
  mkdirSync(evidenceDir, { recursive: true });
  const controller = new AbortController();
  process.on('SIGINT', () => controller.abort(new Error('interrupted')));
  const runId = `f3-${new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14)}-${randomUUID().slice(0, 6)}`;
  const run = { runId, revision: opts.revision, evidenceDir, signal: controller.signal, mahordExecutable: config.mahordExecutable, tesseract: config.tesseract, sshExecutable: config.sshExecutable };
  const lanes = Object.fromEntries(LANES.map((name) => [name, newLane(name, evidenceDir)]));
  const startedAt = new Date().toISOString();
  const session = newLane('session', evidenceDir);
  const shared = startSession({ config, run, lane: session });
  let runError = null;
  try {
    await runLanes({ config, run, lanes, session, shared, signal: controller.signal });
  } catch (error) { // no-excuse-ok: catch (recorded; run.json is still written and fails)
    runError = String(error.stack ?? error).slice(0, 4000);
  } finally {
    session.receipt.cleanup.push(...(await shared.stop()));
    shared.close();
  }
  const cause = session.receipt.blocked ?? session.receipt.failure;
  for (const name of LANES) {
    if (lanes[name].receipt.steps.length > 0) continue;
    lanes[name].receipt.blocked = { code: 'SESSION_HOST_UNAVAILABLE', detail: `the session host ${config.sessionHost.hostId} did not provide the relay, daemon and fixture panes: ${JSON.stringify(cause)}`, at: new Date().toISOString() };
    markNotRun(lanes[name], scenariosForLane(name), 'SESSION_HOST_UNAVAILABLE');
  }
  const receipt = { schema: RUN_SCHEMA, runId, revision: opts.revision, startedAt, finishedAt: new Date().toISOString(), config: opts.config, control, error: runError,
    session: { ...session.receipt, finishedAt: new Date().toISOString() },
    lanes: Object.fromEntries(LANES.map((name) => [name, { ...lanes[name].receipt, finishedAt: new Date().toISOString() }])), cross: crossChecks(shared, lanes) };
  writeFileSync(join(evidenceDir, 'run.json'), `${JSON.stringify(receipt, null, 2)}\n`);
  const verdict = verifyEvidence(evidenceDir, { withManifest: false });
  writeFileSync(join(evidenceDir, 'verdict.json'), `${JSON.stringify(verdict, null, 2)}\n`);
  writeFileSync(join(evidenceDir, 'manifest.json'), `${JSON.stringify({ runId, revision: opts.revision, files: manifestFiles(evidenceDir) }, null, 2)}\n`);
  process.stdout.write(`${JSON.stringify({ f3: { runId, verdict: verdict.verdict, exitCode: verdict.exitCode, evidence: evidenceDir } })}\n`);
  return controller.signal.aborted ? EXIT.INTERRUPTED : verdict.exitCode;
}

if (import.meta.main) {
  let code;
  try { code = await main(); } catch (error) { // no-excuse-ok: catch (CLI boundary)
    process.stderr.write(`supervisor-f3: ${error.stack ?? error}\n${USAGE}\n`);
    code = EXIT.USAGE;
  }
  process.exit(code);
}
