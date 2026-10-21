#!/usr/bin/env node
// Isolated debug-bundle daemon lifecycle QA (plan daemon-independent-lifetime-20261017, todo 12).
//
//   node scripts/qa/daemon-lifetime/run.mjs --platform mac|linux|windows --all
//   node scripts/qa/daemon-lifetime/run.mjs --platform linux --only Q01,Q08
//   node scripts/qa/daemon-lifetime/run.mjs --platform mac --self-check     (no build, no QA: guards + hooks)
//   node scripts/qa/daemon-lifetime/run.mjs --platform mac --self-test-reload
//       (harness failure self-test: builds a throwaway worktree with self-test-reload.patch, which
//        re-adds supervisor::reload in the client, runs Q00+Q04, and exits 0 only if Q04 FAILS)
//
// Runs on this Mac. mac targets this Mac's own QA bundle; linux targets omarchy and windows targets
// maho-win over SSH, with GUI actions through MahoRD only. It builds only the plan's QA debug
// bundles (cargo tauri build --debug --features local-split-qa --config <runDir>/tauri.qa.json,
// CARGO_TARGET_DIR private), never a release build, never the installed app, never the live
// daemon. Teardown kills only pids it recorded (re-checked by executable path and start time),
// removes only jobs named ferryx-qa-<runId>*, and only its own run dir.

import { mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeRunId, tauriQaConfig, isolationLayout, isolationEnv, assertIsolatedEnv, forbiddenRoots, assertQaExecutable, qaSupervisorName, GuardError } from './lib/guards.mjs';
import { newSummary, selectScenarios, settle, block, applyQ00Gate, computeExitCode, validateSummary, scenarioById } from './lib/summary.mjs';
import { hostFor, buildPlan, assertQaBuild, worktreePaths } from './lib/hosts.mjs';
import { probeHooks, missingHooksFor } from './lib/hooks.mjs';
import { generationKey, assertDistinctKeys } from './lib/wire.mjs';
import { startAgent, fetchHostFile } from './lib/remote.mjs';
import { createJobs } from './lib/jobs.mjs';
import { mahordTypable, createGuiDriver, macGuiExecutable } from './lib/gui.mjs';
import { assembleDaemonHelper } from '../../macos-daemon-helper.mjs';
import { finalizeMacosBundle } from '../../lib/release-platforms.mjs';
import { connectMahord } from './lib/mahord.mjs';
import { sshHostId } from './lib/sshd.mjs';
import { run } from './lib/proc.mjs';
import * as g1 from './scenarios/q00-q03.mjs';
import * as g2 from './scenarios/q04-q06.mjs';
import * as g3 from './scenarios/q07-q09.mjs';
import * as g4 from './scenarios/q10-q13.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '../../..');
export const RUNNERS = Object.freeze({ ...g1, ...g2, ...g3, ...g4 });

const syncExec = (cmd, args) => {
  const r = spawnSync(cmd, args, { encoding: 'utf8' });
  const out = { status: r.status === null ? -1 : r.status, stdout: r.stdout ?? '', stderr: r.stderr ?? '' };
  if (out.status !== 0) throw new Error(`${cmd} ${args.join(' ')} exited ${out.status}: ${out.stderr}`);
  return out;
};

/** todo 13 on a QA bundle: nested signed helper, then inside-out ad-hoc re-seal, no notarization. */
export function signMacQaBundle({ appPath, entitlements, workspaceDir, exec = syncExec }) {
  const helper = assembleDaemonHelper({ appPath, identity: '-', entitlements, exec });
  const sealed = finalizeMacosBundle({ appPath, workspaceDir, signingIdentity: '-', approveNotarization: false, entitlements, exec });
  return { helper, sealed };
}

/** QA worktree under the host's worktree root (AGENTS.md); --worktree-slug picks another one. */
export const DEFAULT_WORKTREE_SLUG = 'daemon-lifetime-w5-t12-20261018';

export function parseArgs(argv) {
  const out = { platform: null, all: false, only: [], selfCheck: false, selfTestReload: false, evidenceDir: null, runId: null, keepRunDir: false, v1Worktree: null, baselineWorktree: null, worktreeSlug: DEFAULT_WORKTREE_SLUG };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--platform') out.platform = argv[++i];
    else if (a === '--all') out.all = true;
    else if (a === '--only') out.only = argv[++i].split(',').map((s) => s.trim()).filter(Boolean);
    else if (a === '--self-check') out.selfCheck = true;
    else if (a === '--self-test-reload') out.selfTestReload = true;
    else if (a === '--evidence-dir') out.evidenceDir = argv[++i];
    else if (a === '--run-id') out.runId = argv[++i];
    else if (a === '--v1-worktree') out.v1Worktree = argv[++i];
    else if (a === '--baseline-worktree') out.baselineWorktree = argv[++i];
    else if (a === '--worktree-slug') out.worktreeSlug = argv[++i];
    else throw new Error(`unknown argument ${a}`);
  }
  if (!['mac', 'linux', 'windows'].includes(out.platform)) throw new Error('--platform mac|linux|windows is required');
  if (!/^[a-z0-9][a-z0-9-]*$/.test(out.worktreeSlug ?? '')) throw new Error('--worktree-slug must be lowercase letters, digits and -');
  if (out.selfTestReload) {
    if (out.selfCheck || out.all || out.only.length) throw new Error('--self-test-reload runs Q04 only; it takes no --all, --only or --self-check');
    out.only = ['Q04'];
    return out;
  }
  if (!out.selfCheck && !out.all && out.only.length === 0) throw new Error('--all or --only Qxx,... is required');
  return out;
}

/** The patch that re-adds supervisor::reload in the client (plan todo 12 failure scenario). */
export const SELF_TEST_PATCH = 'self-test-reload.patch';

/** Exit code of a --self-test-reload run: 0 only when Q04 ran and FAILED against the patched build. */
export function selfTestExitCode(summary) {
  const q04 = summary.scenarios.find((s) => s.id === 'Q04');
  return q04?.status === 'FAIL' ? 0 : 1;
}

/**
 * Throwaway worktree for the self-test: `git worktree add` of this checkout's HEAD under the
 * host's worktree root, then `git apply` of the patch. Returns the worktree path and a remover.
 * The handle goes to `register` as soon as the worktree exists, so a later setup failure (submodule,
 * apply, bun install) still leaves the caller's teardown a worktree to remove.
 */
export async function prepareSelfTestWorktree({ host, agent, runId, source, register }) {
  const slug = `selftest-reload-${runId.toLowerCase().replace(/[^a-z0-9-]/g, '-')}`;
  const worktree = worktreePaths(host, slug).worktree;
  const head = (await agent.call('exec', { command: 'git', args: ['-C', source, 'rev-parse', 'HEAD'] })).stdout.trim();
  const git = async (args) => {
    const r = await agent.call('exec', { command: 'git', args, deadlineMs: 900_000 });
    if (r.status !== 0) throw new GuardError('SELF_TEST_SETUP_FAILED', `git ${args.join(' ')} exited ${r.status}: ${r.stderr}`);
  };
  await git(['-C', host.canonical, 'worktree', 'add', '--detach', worktree, head]);
  const handle = {
    worktree,
    head,
    remove: async () => {
      for (const args of [['-C', host.canonical, 'worktree', 'remove', '--force', worktree], ['-C', host.canonical, 'worktree', 'prune']]) {
        const r = await agent.call('exec', { command: 'git', args, deadlineMs: 900_000 });
        if (r.status !== 0) throw new Error(`git ${args.join(' ')} exited ${r.status}: ${r.stderr}`);
      }
    },
  };
  register(handle);
  // The ghostty submodule is cloned from the canonical clone's local module dir (file transport).
  await git(['-C', worktree, '-c', 'protocol.file.allow=always', 'submodule', 'update', '--init', '--recursive']);
  await git(['-C', worktree, 'apply', joinFor(host.platform, worktree, 'scripts', 'qa', 'daemon-lifetime', SELF_TEST_PATCH)]);
  // A fresh worktree has no ui/node_modules; tauri's beforeBuildCommand (bun run --cwd ui build) needs them.
  const deps = await agent.call('exec', { command: 'bun', args: ['install', '--cwd', joinFor(host.platform, worktree, 'ui'), '--frozen-lockfile'], deadlineMs: 900_000 });
  if (deps.status !== 0) throw new GuardError('SELF_TEST_SETUP_FAILED', `bun install exited ${deps.status}: ${deps.stderr}`);
  return handle;
}

/** Every runner must exist for every scenario id, and the summary must list them all. */
export function assertRunnersComplete(runners = RUNNERS, ids) {
  const missing = ids.filter((id) => typeof runners[id] !== 'function');
  if (missing.length) throw new Error(`no runner for ${missing.join(', ')}`);
  return true;
}

function joinFor(platform, ...parts) {
  return platform === 'windows' ? path.win32.join(...parts) : path.posix.join(...parts);
}

/**
 * The baseline step is a plain cargo build (no tauri beforeBuildCommand), but tauri_build still
 * requires frontendDist (../ui/dist) to exist. The daemon-only baseline binary needs no real UI,
 * so create an empty, gitignored <worktree>/ui/dist on the host (mkdir -p: idempotent, never
 * touches tracked files or an existing dist). Runs through exec because the agent's mkdir is
 * confined to the run dir.
 */
export async function prepareBaselineDist({ host, agent, worktree }) {
  const dir = joinFor(host.platform, worktree, 'ui', 'dist');
  const r = await agent.call('exec', { command: 'node', args: ['-e', 'require("fs").mkdirSync(process.argv[1],{recursive:true})', dir] });
  if (r.status !== 0) throw new GuardError('BASELINE_DIST_FAILED', `mkdir ${dir} exited ${r.status}: ${r.stderr}`);
  return dir;
}

/**
 * One QA build. Output never crosses the agent channel: the agent writes stdout+stderr to
 * <runDir>/build-logs/build-<name>.log on the host and the real exit (status, signal, timedOut) to
 * build-<name>.status.json beside it, and returns only that exit and a bounded tail. Both files
 * are then copied to the evidence dir as files (build-<name>.log keeps its name). A failed copy
 * leaves the returned tail in build-<name>.log instead, marked as such, and is recorded on the
 * result; the build outcome is still decided by the recorded exit.
 */
export async function runBuildStep({ host, agent, name, step, runDir, localEvidence, fetch = fetchHostFile }) {
  const logFile = joinFor(host.platform, runDir, 'build-logs', `build-${name}.log`);
  const statusFile = joinFor(host.platform, runDir, 'build-logs', `build-${name}.status.json`);
  const built = await agent.call('execLogged', { command: step.command, args: step.args, cwd: step.cwd, env: step.env, logFile, statusFile, deadlineMs: 4 * 3_600_000 });
  const localLog = path.join(localEvidence, `build-${name}.log`);
  const localStatus = path.join(localEvidence, `build-${name}.status.json`);
  const fetchErrors = [];
  try { await fetch(host, logFile, localLog); } catch (error) {
    fetchErrors.push(`log: ${error?.message ?? error}`);
    writeFileSync(localLog, `[full log copy failed: ${error?.message ?? error}; last lines returned by the agent follow]\n${built.tail}\n`);
  }
  try { await fetch(host, statusFile, localStatus); } catch (error) {
    fetchErrors.push(`status: ${error?.message ?? error}`);
    writeFileSync(localStatus, `${JSON.stringify({ ...built, tail: undefined, copyFailed: true }, null, 2)}\n`);
  }
  const ok = built.status === 0 && !built.signal && !built.timedOut && !built.error;
  return { ...built, ok, localLog, localStatus, fetchErrors };
}

/** BUILD_FAILED detail: the recorded exit code, signal, timeout or spawn error. */
export function buildFailureDetail(name, built) {
  const parts = [`exit=${built.status}`, `signal=${built.signal ?? 'none'}`];
  if (built.timedOut) parts.push('timedOut');
  if (built.error) parts.push(`error=${built.error}`);
  return `${name} ${parts.join(' ')}`;
}

/** Artifact descriptor for a built QA bundle (per platform layout of cargo tauri build --debug). */
export function artifactPaths({ platform, targetDir, productName, name }) {
  const dbg = joinFor(platform, targetDir, 'debug');
  if (platform === 'mac') {
    const appPath = joinFor(platform, dbg, 'bundle', 'macos', `${productName}.app`);
    return { name, appPath, guiExecutable: macGuiExecutable(appPath), cliExecutable: joinFor(platform, dbg, 'ferryx-cli'), daemonSource: macGuiExecutable(appPath) };
  }
  const exe = platform === 'windows' ? 'ferryx.exe' : 'ferryx';
  const cli = platform === 'windows' ? 'ferryx-cli.exe' : 'ferryx-cli';
  return { name, appPath: null, guiExecutable: joinFor(platform, dbg, exe), cliExecutable: joinFor(platform, dbg, cli), daemonSource: joinFor(platform, dbg, exe) };
}

/**
 * The owner record a release daemon writes: server.rs get_runtime_dir() of a release build
 * (/tmp/rorca-<uid>, or %LOCALAPPDATA%\Ferryx\runtime with the product's TEMP fallback) + owner.json.
 */
export function releaseOwnerFile(platform, { uid, localAppData }) {
  if (platform !== 'windows') return `/tmp/rorca-${uid}/owner.json`;
  if (!localAppData) throw new GuardError('RELEASE_OWNER_PATH', 'the host reported no LOCALAPPDATA/TEMP directory');
  return path.win32.join(localAppData, 'Ferryx', 'runtime', 'owner.json');
}

/**
 * Release receipt from the owner record read before and after the run: absent (no release owner
 * either time; nothing to compare), continuous (the same record), or changed.
 */
export function releaseContinuity(before, after) {
  if (before === null && after === null) return { state: 'absent', unchanged: true };
  if (before === after) return { state: 'continuous', unchanged: true };
  return { state: 'changed', unchanged: false };
}

/** Teardown steps for the run's optional resources, read when teardown runs (they may appear mid-setup). */
export function teardownExtras(live, summary) {
  return [
    ['self-test-worktree', async () => {
      if (!live.selfTest) return;
      await live.selfTest.remove();
      if (summary.selfTestReload) summary.selfTestReload.removed = true;
    }],
    ['mahord', async () => { if (live.mahord) await live.mahord.close(); }],
  ];
}

/**
 * Runs `work`, then `teardown` and `stop` on every exit path. A work failure is kept in
 * summary.error and rethrown after teardown with exit code 2; a failing teardown or stop is recorded
 * beside it and never replaces it.
 */
export async function runWithTeardown({ summary, work, teardown, stop, writeSummary, log = () => {} }) {
  let runError = null;
  try {
    await work();
  } catch (error) {
    runError = error;
    summary.error = String(error?.stack ?? error);
    writeSummary();
    log({ runError: String(error?.message ?? error) });
  }
  for (const [key, fn] of [['teardownError', teardown], ['stopError', stop]]) {
    try {
      await fn();
    } catch (error) {
      if (!runError) throw error;
      summary[key] = String(error?.stack ?? error);
    }
  }
  if (runError) {
    summary.finishedAt = new Date().toISOString();
    summary.exitCode = 2;
    writeSummary();
    throw runError;
  }
}

/**
 * Teardown for every exit path of a run, including a build or setup failure before any scenario.
 * Recorded pids only, QA-named jobs only, the run's own dirs only. A failing step is recorded in
 * teardown.errors (and as a leftover, so the run is never clean) and does not stop later steps;
 * the run dirs are removed only when nothing is left behind and no step failed.
 */
export async function teardownRun({ summary, agent, jobs, host, layout, runDirs, qaRoots, localEvidence, prodSnapshot, extra = [] }) {
  const errors = [];
  const step = async (name, fn) => {
    try { await fn(); } catch (error) { errors.push({ step: name, error: String(error?.message ?? error) }); }
  };
  // The run dir (and its logs/) is removed below: copy every log into the evidence first, so a
  // FAIL keeps the GUI and daemon logs that explain it (r3 Q00 lost gui-g1.log this way).
  await step('copy-logs', async () => {
    const listed = await agent.call('exec', { command: 'node', args: ['-e', 'try{console.log(JSON.stringify(require("fs").readdirSync(process.argv[1])))}catch{console.log("[]")}', layout.logs] });
    for (const name of JSON.parse(listed.stdout || '[]')) {
      const text = await agent.call('readFile', { file: joinFor(host.platform, layout.logs, name) });
      if (text !== null) writeFileSync(path.join(localEvidence, `runlog-${name}`), text);
    }
  });
  // The private sshd's remote-helper daemon outlives the session that started it (it daemonises
  // outside the run dirs): record any not yet recorded before the kill, remove its root after.
  await step('record-ssh-helper', async () => {
    await agent.call('recordSshHelper', { hostId: sshHostId(summary.runId) });
  });
  await step('kill-recorded', async () => {
    const t = await agent.call('teardownRecorded');
    summary.teardown.killed = t.killed.map((e) => ({ pid: e.pid, role: e.role }));
    summary.teardown.refused = t.refused;
  });
  await step('remove-ssh-helper-roots', async () => {
    const r = await agent.call('removeSshHelperRoots');
    summary.teardown.dirsRemoved.push(...(r?.removed ?? []).map((h) => h.root));
    summary.teardown.leftovers.push(...(r?.kept ?? []).map((h) => ({ kind: 'ssh-helper-root', ...h })));
  });
  await step('remove-jobs', async () => {
    for (const name of await jobs.listQaJobs()) {
      await jobs.remove(name);
      summary.teardown.jobsRemoved.push(name);
    }
    if (host.platform === 'linux') await agent.call('exec', { command: 'systemctl', args: ['--user', 'daemon-reload'] });
  });
  await step('leftovers', async () => {
    summary.teardown.leftovers = [
      ...(await jobs.listQaJobs()).map((j) => ({ kind: 'job', name: j })),
      ...(await agent.call('table')).filter((r) => r.executable && qaRoots.some((root) => r.executable.toLowerCase().startsWith(root.toLowerCase()))).map((r) => ({ kind: 'process', pid: r.pid, executable: r.executable })),
    ];
  });
  if (summary.teardown.leftovers.length === 0 && errors.length === 0) {
    await step('remove-run-dirs', async () => {
      for (const dir of runDirs) {
        await agent.call('remove', { target: dir });
        summary.teardown.dirsRemoved.push(dir);
        if (await agent.call('exists', { file: dir })) summary.teardown.leftovers.push({ kind: 'dir', path: dir });
      }
    });
  }
  summary.releaseDaemon.state = 'unverified';
  summary.releaseDaemon.unchanged = null;
  await step('release-daemon-snapshot', async () => {
    summary.releaseDaemon.after = await prodSnapshot();
    Object.assign(summary.releaseDaemon, releaseContinuity(summary.releaseDaemon.before, summary.releaseDaemon.after));
  });
  for (const [name, fn] of extra) await step(name, fn);
  summary.teardown.errors = errors;
  summary.teardown.leftovers.push(...errors.map((e) => ({ kind: 'teardown-error', ...e })));
  return summary.teardown;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const host = hostFor(opts.platform);
  const runId = opts.runId ?? makeRunId();
  const selected = opts.selfCheck ? [] : selectScenarios({ all: opts.all, only: opts.only }, host.platform);
  assertRunnersComplete(RUNNERS, selected);
  // Sockets stay under runRoot (short path); bulk storage goes under storageRoot when the host sets one.
  const runtimeRunDir = joinFor(host.platform, host.runRoot, runId);
  const runDir = host.storageRoot ? joinFor(host.platform, host.storageRoot, runId) : runtimeRunDir;
  const runDirs = [...new Set([runDir, runtimeRunDir])];
  const layout = isolationLayout(runDir, host.platform, runDir === runtimeRunDir ? {} : { runtimeDir: joinFor(host.platform, runtimeRunDir, 'run') });
  // Evidence always lands on this Mac: <evidenceRoot>/q12-<platform>/<runId>.
  const localEvidence = opts.evidenceDir ?? path.join(hostFor('mac').evidenceRoot, `q12-${host.platform}`, runId);
  mkdirSync(localEvidence, { recursive: true });
  const summary = newSummary({ runId, platform: host.platform, host: host.name });
  const summaryPath = path.join(localEvidence, 'summary.json');
  const writeSummary = () => writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
  const log = (o) => console.log(JSON.stringify({ t: new Date().toISOString(), ...o }));

  summary.revisions.head = (await run('git', ['-C', REPO, 'rev-parse', 'HEAD'])).stdout.trim();
  summary.revisions.tree = (await run('git', ['-C', REPO, 'rev-parse', 'HEAD^{tree}'])).stdout.trim();
  summary.revisions.dirty = (await run('git', ['-C', REPO, 'status', '--porcelain', '--untracked-files=no'])).stdout.trim().split('\n').filter(Boolean);
  summary.hooks = probeHooks(REPO, host.platform);
  writeSummary();

  const agentPath = joinFor(host.platform, host.transport.kind === 'local' ? HERE : '', 'lib', 'agent.mjs');
  const remoteHarness = host.transport.kind === 'local' ? HERE : joinFor(host.platform, worktreePaths(host, opts.worktreeSlug).worktree, 'scripts', 'qa', 'daemon-lifetime');
  const agent = await startAgent(host, { agentPath: host.transport.kind === 'local' ? agentPath : joinFor(host.platform, remoteHarness, 'lib', 'agent.mjs'), runId, runDir, allowRoots: [...runDirs.slice(1), host.cargoTarget, `${host.cargoTarget}-baseline`] }, { log });
  const uid = host.platform === 'windows' ? null : Number((await agent.call('exec', { command: 'id', args: ['-u'] })).stdout.trim());
  const home = host.transport.kind === 'local' ? os.homedir() : (await agent.call('exec', host.platform === 'windows' ? { command: 'powershell.exe', args: ['-NoProfile', '-Command', '$env:USERPROFILE'] } : { command: 'sh', args: ['-c', 'printf %s "$HOME"'] })).stdout.trim();
  const forbidden = forbiddenRoots({ platform: host.platform, home, uid });
  const env = isolationEnv({ runId, layout, platform: host.platform });
  assertIsolatedEnv(env, { runId, forbidden, platform: host.platform });

  // Release daemon owner record: recorded, compared at the end, never touched. readFile returns
  // null only for an absent file; an unreadable one throws instead of passing for absent.
  const localAppData = host.platform === 'windows' ? (await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-Command', "if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } elseif ($env:TEMP) { $env:TEMP } else { 'C:\\ProgramData' }"] })).stdout.trim() : null;
  summary.releaseDaemon.path = releaseOwnerFile(host.platform, { uid, localAppData });
  const prodSnapshot = () => agent.call('readFile', { file: summary.releaseDaemon.path });
  summary.releaseDaemon.before = await prodSnapshot();

  if (opts.selfCheck) {
    summary.selfCheck = { env, forbidden, layout, supervisor: qaSupervisorName(runId), hooks: summary.hooks };
    summary.exitCode = 0;
    summary.finishedAt = new Date().toISOString();
    writeSummary();
    await agent.stop();
    log({ selfCheck: summaryPath });
    return 0;
  }

  // Everything from here on can leave QA processes, jobs or dirs behind, so every exit path - a
  // build or setup failure before any scenario included - runs the same teardown below. The
  // original error is kept in summary.error and rethrown after teardown (exit 2), never masked.
  const qaRoots = [...runDirs, host.cargoTarget, `${host.cargoTarget}-baseline`];
  const jobs = createJobs({ host, agent, runId, layout, uid });
  const live = { selfTest: null, mahord: null };
  const work = async () => {
    for (const dir of Object.values(layout)) await agent.call('mkdir', { dir, mode: 0o700 });
    await agent.call('writeFile', { file: joinFor(host.platform, runDir, 'tauri.qa.json'), content: JSON.stringify(tauriQaConfig(runId), null, 2) });
    const worktree = host.transport.kind === 'local' ? REPO : worktreePaths(host, opts.worktreeSlug).worktree;
    const remoteGit = async (dir) => ({ dir, head: (await agent.call('exec', { command: 'git', args: ['-C', dir, 'rev-parse', 'HEAD'] })).stdout.trim(), dirty: (await agent.call('exec', { command: 'git', args: ['-C', dir, 'status', '--porcelain', '--untracked-files=no'] })).stdout.trim().split('\n').filter(Boolean) });
    summary.revisions.buildTree = await remoteGit(worktree);
    if (opts.v1Worktree) summary.revisions.handoverV1Rev = await remoteGit(opts.v1Worktree);
    if (opts.baselineWorktree) summary.revisions.baselineRev = await remoteGit(opts.baselineWorktree);
    writeSummary();
    // --self-test-reload: every QA artifact is built from the patched throwaway worktree.
    const selfTest = opts.selfTestReload ? await prepareSelfTestWorktree({ host, agent, runId, source: worktree, register: (h) => {
      live.selfTest = h;
      summary.selfTestReload = { worktree: h.worktree, head: h.head, patch: SELF_TEST_PATCH, expect: 'Q04 FAIL', removed: false };
    } }) : null;
    const buildFrom = selfTest ? selfTest.worktree : worktree;
    const plan = buildPlan({ host, runDir, worktree: buildFrom, v1Worktree: opts.v1Worktree, baselineWorktree: opts.baselineWorktree });
    const productName = tauriQaConfig(runId).productName;
    const artifacts = {};
    const keys = {};
    const version = (await agent.call('exec', { command: 'git', args: ['-C', buildFrom, 'show', 'HEAD:src-tauri/Cargo.toml'] })).stdout.match(/^version = "([^"]+)"/m)?.[1];
    for (const name of ['A', 'B', 'C', 'bump', 'v1bump', 'baseline']) {
      const step = plan[name];
      if (!step) continue;
      assertQaBuild(step);
      if (name === 'baseline') await prepareBaselineDist({ host, agent, worktree: step.cwd });
      const built = await runBuildStep({ host, agent, name, step, runDir, localEvidence });
      summary.builds = { ...(summary.builds ?? {}), [name]: { status: built.status, signal: built.signal, timedOut: built.timedOut, error: built.error, bytes: built.bytes, log: built.localLog, statusFile: built.localStatus, fetchErrors: built.fetchErrors } };
      writeSummary();
      if (!built.ok) throw new GuardError('BUILD_FAILED', buildFailureDetail(name, built));
      if (name === 'baseline') {
        artifacts.baseline = { name, daemonExecutable: joinFor(host.platform, step.env.CARGO_TARGET_DIR, 'debug', host.platform === 'windows' ? 'ferryx.exe' : 'ferryx') };
        continue;
      }
      // Each build overwrites the same bundle path: move it to a per-artifact dir under the run dir.
      const fresh = artifactPaths({ platform: host.platform, targetDir: host.cargoTarget, productName, name });
      const destRoot = joinFor(host.platform, runDir, 'artifacts', name);
      await agent.call('mkdir', { dir: destRoot });
      // Host paths are parsed with the host's path flavour: posix basename/dirname do not split C:\ paths.
      const hostPath = host.platform === 'windows' ? path.win32 : path.posix;
      const src = fresh.appPath ?? hostPath.dirname(fresh.guiExecutable);
      if (host.platform === 'mac') await agent.call('exec', { command: 'ditto', args: [src, joinFor(host.platform, destRoot, path.basename(src))] });
      else await agent.call('exec', host.platform === 'windows'
        ? { command: 'powershell.exe', args: ['-NoProfile', '-Command', `Copy-Item -LiteralPath '${fresh.guiExecutable}','${fresh.cliExecutable}' -Destination '${destRoot}'`] }
        : { command: 'cp', args: [fresh.guiExecutable, fresh.cliExecutable, destRoot] });
      const art = host.platform === 'mac'
        ? { name, appPath: joinFor(host.platform, destRoot, path.basename(src)), guiExecutable: macGuiExecutable(joinFor(host.platform, destRoot, path.basename(src))), cliExecutable: fresh.cliExecutable }
        : { name, appPath: null, guiExecutable: joinFor(host.platform, destRoot, hostPath.basename(fresh.guiExecutable)), cliExecutable: joinFor(host.platform, destRoot, hostPath.basename(fresh.cliExecutable)) };
      if (host.platform === 'mac') {
        // todo 13: each QA artifact carries its own signed helper and a re-sealed outer signature.
        // mac artifacts live on this Mac: assemble + ad-hoc re-seal in-process (scripts/build-notarized-local.mjs order).
        const helper = signMacQaBundle({ appPath: art.appPath, entitlements: joinFor(host.platform, destRoot, 'outer-entitlements.plist'), workspaceDir: destRoot });
        writeFileSync(path.join(localEvidence, `helper-${name}.log`), `${JSON.stringify(helper, null, 2)}\n`);
        for (const target of [art.appPath, joinFor(host.platform, art.appPath, 'Contents', 'Helpers', 'Ferryx Daemon.app')]) {
          const v = await run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', target]);
          writeFileSync(path.join(localEvidence, `codesign-${name}-${path.basename(target)}.log`), `exit ${v.status}\n${v.stderr}`);
          if (v.status !== 0) throw new GuardError('CODESIGN_FAILED', target);
        }
        art.daemonSource = joinFor(host.platform, art.appPath, 'Contents', 'Helpers', 'Ferryx Daemon.app', 'Contents', 'MacOS', 'ferryx');
      } else {
        art.daemonSource = art.guiExecutable;
      }
      art.sha256 = await agent.call('sha256', { file: art.daemonSource });
      keys[name] = generationKey(version, art.sha256);
      artifacts[name] = art;
    }
    assertDistinctKeys({ A: keys.A, B: keys.B, C: keys.C });
    summary.artifacts = Object.fromEntries(Object.entries(artifacts).map(([k, v]) => [k, { ...v, key: keys[k] ?? null }]));
    writeSummary();

    const allowedRoots = [runDir, host.cargoTarget, `${host.cargoTarget}-baseline`];
    for (const art of Object.values(artifacts)) {
      for (const exe of [art.guiExecutable, art.daemonExecutable].filter(Boolean)) assertQaExecutable(exe, { allowedRoots, forbidden, platform: host.platform });
    }
    const mahord = live.mahord = host.gui.kind === 'mahord' ? await connectMahord(host.gui, { log }) : null;
    const gui = createGuiDriver({ host, agent, runId, layout, allowedRoots, forbidden, mahord, guiEnv: { ...(host.guiEnv ?? {}), ...env } });
    if (host.platform === 'windows') {
      const launcher = await import('node:fs').then((fs) => fs.readFileSync(path.join(HERE, 'fixtures', 'windows-gui-launcher.ps1'), 'utf8'));
      await agent.call('writeFile', { file: joinFor(host.platform, runDir, 'launcher.ps1'), content: launcher });
    }
    const repoDir = joinFor(host.platform, runDir, 'repo');
    await agent.call('mkdir', { dir: repoDir });
    await agent.call('exec', { command: 'git', args: ['init', '-q', repoDir] });

    const ctx = {
      platform: host.platform, runId, host, agent, jobs, gui, mahord, layout, uid, artifacts, keys, hooks: summary.hooks,
      revisions: summary.revisions, isolationEnv: env, repoDir, workspaceId: 'qa-daemon-lifetime', workspaceRegistered: false,
      stagedRoot: joinFor(host.platform, layout.data, 'daemon-runtime'),
      qaRoots, guiExecutables: Object.values(artifacts).map((a) => a.guiExecutable).filter(Boolean),
      hostUser: host.transport.user ?? os.userInfo().username, sshPort: 20000 + (Number(runId.slice(-5, -1)) % 20000),
      evidenceDir: localEvidence.split(path.sep).join('/'), state: {}, carry: [], sshd: null,
      writeEvidence: async (file, content) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, content); },
      replaceAppOnDisk: async (from, to) => {
        // The on-disk app at A's path is replaced by B's bytes (same version, different build).
        if (host.platform === 'mac') {
          await agent.call('exec', { command: 'rm', args: ['-rf', from.appPath] });
          await agent.call('exec', { command: 'ditto', args: [to.appPath, from.appPath] });
        } else {
          await agent.call('exec', host.platform === 'windows'
            ? { command: 'powershell.exe', args: ['-NoProfile', '-Command', `Copy-Item -Force '${to.guiExecutable}' '${from.guiExecutable}'`] }
            : { command: 'cp', args: ['-f', to.guiExecutable, from.guiExecutable] });
        }
        return { artifact: { ...from, name: to.name, sha256: await agent.call('sha256', { file: from.guiExecutable }) } };
      },
      typeIntoSession: async (guiProc, sessionId, text) => {
        // macOS: no desktop input on the user's MacBook; the data-protocol write is the GUI-equivalent
        // client write. Remote hosts type through MahoRD into the focused QA window.
        if (mahord) {
          // MahoRD drops '-' and '_' on maho-win and types no newline: refuse untypable text instead
          // of letting a marker arrive mangled, and send Enter as a key.
          const line = text.replace(/\r?\n$/, '');
          if (host.platform === 'windows' && !mahordTypable(line)) throw new Error(`MahoRD cannot type '-' or '_' on maho-win: ${line}`);
          await mahord.type(line);
          return text.endsWith('\n') ? mahord.key('Enter') : true;
        }
        const { conn } = { conn: ctx.state.conn };
        return conn.call({ type: 'write', sessionId, data: Buffer.from(text).toString('base64') });
      },
    };

    const ordered = summary.scenarios.filter((s) => selected.includes(s.id));
    for (const result of ordered) {
      if (!result.applicable) continue;
      if (result.id !== 'Q00' && applyQ00Gate(summary)) break;
      const missing = missingHooksFor(result.id, summary.hooks);
      if (missing.length) {
        block(result, 'HOOK_MISSING', missing.map((m) => `${m.id}: ${m.owner}`).join('; '));
        writeSummary();
        continue;
      }
      result.startedAt = new Date().toISOString();
      const t0 = Date.now();
      log({ scenario: result.id, title: scenarioById(result.id).title });
      try {
        await RUNNERS[result.id](ctx, result);
      } catch (error) {
        result.assertions.push({ name: 'scenario completed without an exception', ok: false, detail: String(error?.stack ?? error) });
        writeFileSync(path.join(localEvidence, `${result.id}-exception.log`), String(error?.stack ?? error));
        result.evidence.push(path.join(localEvidence, `${result.id}-exception.log`));
      }
      result.durationMs = Date.now() - t0;
      settle(result);
      writeSummary();
      if (result.id === 'Q00') applyQ00Gate(summary);
    }
    summary.carryover = ctx.carry;
  };

  await runWithTeardown({
    summary, work, writeSummary, log, stop: () => agent.stop(),
    teardown: () => teardownRun({ summary, agent, jobs, host, layout, runDirs, qaRoots, localEvidence, prodSnapshot, extra: teardownExtras(live, summary) }),
  });
  summary.finishedAt = new Date().toISOString();
  summary.exitCode = computeExitCode(summary, selected);
  // The self-test inverts the verdict: a FAIL of Q04 against the reload build is the pass, as long
  // as teardown was clean and the release daemon untouched.
  if (live.selfTest && summary.teardown.leftovers.length === 0 && summary.releaseDaemon.unchanged) summary.exitCode = selfTestExitCode(summary);
  const problems = validateSummary(summary);
  if (problems.length) { summary.schemaProblems = problems; summary.exitCode = 2; }
  writeSummary();
  log({ summary: summaryPath, exitCode: summary.exitCode });
  return summary.exitCode;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then((code) => process.exit(code), (error) => {
    console.error(error?.stack ?? error);
    process.exit(2);
  });
}
