#!/usr/bin/env node
// Isolated debug-bundle daemon lifecycle QA (plan daemon-independent-lifetime-20261017, todo 12).
//
//   node scripts/qa/daemon-lifetime/run.mjs --platform mac|linux|windows --all
//   node scripts/qa/daemon-lifetime/run.mjs --platform linux --only Q01,Q08
//   node scripts/qa/daemon-lifetime/run.mjs --platform mac --self-check     (no build, no QA: guards + hooks)
//
// Runs on this Mac. mac targets this Mac's own QA bundle; linux targets omarchy and windows targets
// maho-win over SSH, with GUI actions through MahoRD only. It builds only the plan's QA debug
// bundles (cargo tauri build --debug --features local-split-qa --config <runDir>/tauri.qa.json,
// CARGO_TARGET_DIR private), never a release build, never the installed app, never the live
// daemon. Teardown kills only pids it recorded (re-checked by executable path and start time),
// removes only jobs named ferryx-qa-<runId>*, and only its own run dir.

import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { makeRunId, tauriQaConfig, isolationLayout, isolationEnv, assertIsolatedEnv, forbiddenRoots, assertQaExecutable, qaSupervisorName, GuardError } from './lib/guards.mjs';
import { newSummary, selectScenarios, settle, block, applyQ00Gate, computeExitCode, validateSummary, scenarioById } from './lib/summary.mjs';
import { hostFor, buildPlan, assertQaBuild, worktreePaths } from './lib/hosts.mjs';
import { probeHooks, missingHooksFor } from './lib/hooks.mjs';
import { generationKey, assertDistinctKeys } from './lib/wire.mjs';
import { startAgent } from './lib/remote.mjs';
import { createJobs } from './lib/jobs.mjs';
import { createGuiDriver, macGuiExecutable } from './lib/gui.mjs';
import { connectMahord } from './lib/mahord.mjs';
import { run } from './lib/proc.mjs';
import * as g1 from './scenarios/q00-q03.mjs';
import * as g2 from './scenarios/q04-q06.mjs';
import * as g3 from './scenarios/q07-q09.mjs';
import * as g4 from './scenarios/q10-q13.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, '../../..');
export const RUNNERS = Object.freeze({ ...g1, ...g2, ...g3, ...g4 });

export function parseArgs(argv) {
  const out = { platform: null, all: false, only: [], selfCheck: false, evidenceDir: null, runId: null, keepRunDir: false, v1Worktree: null, baselineWorktree: null };
  for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    if (a === '--platform') out.platform = argv[++i];
    else if (a === '--all') out.all = true;
    else if (a === '--only') out.only = argv[++i].split(',').map((s) => s.trim()).filter(Boolean);
    else if (a === '--self-check') out.selfCheck = true;
    else if (a === '--evidence-dir') out.evidenceDir = argv[++i];
    else if (a === '--run-id') out.runId = argv[++i];
    else if (a === '--v1-worktree') out.v1Worktree = argv[++i];
    else if (a === '--baseline-worktree') out.baselineWorktree = argv[++i];
    else throw new Error(`unknown argument ${a}`);
  }
  if (!['mac', 'linux', 'windows'].includes(out.platform)) throw new Error('--platform mac|linux|windows is required');
  if (!out.selfCheck && !out.all && out.only.length === 0) throw new Error('--all or --only Qxx,... is required');
  return out;
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

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const host = hostFor(opts.platform);
  const runId = opts.runId ?? makeRunId();
  const selected = opts.selfCheck ? [] : selectScenarios({ all: opts.all, only: opts.only }, host.platform);
  assertRunnersComplete(RUNNERS, selected);
  const runDir = joinFor(host.platform, host.runRoot, runId);
  const layout = isolationLayout(runDir, host.platform);
  // Evidence always lands on this Mac: <evidenceRoot>/q12-<platform>/<runId>.
  const localEvidence = opts.evidenceDir ?? path.join(hostFor('mac').evidenceRoot, `q12-${host.platform}`, runId);
  mkdirSync(localEvidence, { recursive: true });
  const summary = newSummary({ runId, platform: host.platform, host: host.name });
  const summaryPath = path.join(localEvidence, 'summary.json');
  const writeSummary = () => writeFileSync(summaryPath, `${JSON.stringify(summary, null, 2)}\n`);
  const log = (o) => console.log(JSON.stringify({ t: new Date().toISOString(), ...o }));

  summary.revisions.head = (await run('git', ['-C', REPO, 'rev-parse', 'HEAD'])).stdout.trim();
  summary.hooks = probeHooks(REPO, host.platform);
  writeSummary();

  const agentPath = joinFor(host.platform, host.transport.kind === 'local' ? HERE : '', 'lib', 'agent.mjs');
  const remoteHarness = host.transport.kind === 'local' ? HERE : joinFor(host.platform, worktreePaths(host, opts.worktreeSlug ?? 'daemon-lifetime-w5-t12-20261018').worktree, 'scripts', 'qa', 'daemon-lifetime');
  const agent = await startAgent(host, { agentPath: host.transport.kind === 'local' ? agentPath : joinFor(host.platform, remoteHarness, 'lib', 'agent.mjs'), runId, runDir, allowRoots: [host.cargoTarget] }, { log });
  const uid = host.platform === 'windows' ? null : Number((await agent.call('exec', { command: 'id', args: ['-u'] })).stdout.trim());
  const home = host.transport.kind === 'local' ? os.homedir() : (await agent.call('exec', host.platform === 'windows' ? { command: 'powershell.exe', args: ['-NoProfile', '-Command', '$env:USERPROFILE'] } : { command: 'sh', args: ['-c', 'printf %s "$HOME"'] })).stdout.trim();
  const forbidden = forbiddenRoots({ platform: host.platform, home, uid });
  const env = isolationEnv({ runId, layout, platform: host.platform });
  assertIsolatedEnv(env, { runId, forbidden, platform: host.platform });

  // Production daemon snapshot: recorded, compared at the end, never touched.
  const prodRuntime = host.platform === 'windows' ? null : `/tmp/rorca-${uid}`;
  const prodSnapshot = async () => {
    if (!prodRuntime) return (await agent.call('exec', { command: 'powershell.exe', args: ['-NoProfile', '-Command', "Get-Content -Raw (Join-Path $env:LOCALAPPDATA 'Ferryx\\owner.json') -ErrorAction SilentlyContinue"] })).stdout.trim() || null;
    return agent.call('readFile', { file: `${prodRuntime}/owner.json` });
  };
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

  for (const dir of Object.values(layout)) await agent.call('mkdir', { dir, mode: 0o700 });
  await agent.call('writeFile', { file: joinFor(host.platform, runDir, 'tauri.qa.json'), content: JSON.stringify(tauriQaConfig(runId), null, 2) });
  const worktree = host.transport.kind === 'local' ? REPO : worktreePaths(host, 'daemon-lifetime-w5-t12-20261018').worktree;
  const plan = buildPlan({ host, runDir, worktree, v1Worktree: opts.v1Worktree, baselineWorktree: opts.baselineWorktree });
  const productName = tauriQaConfig(runId).productName;
  const artifacts = {};
  const keys = {};
  const version = (await agent.call('exec', { command: 'git', args: ['-C', worktree, 'show', 'HEAD:src-tauri/Cargo.toml'] })).stdout.match(/^version = "([^"]+)"/m)?.[1];
  for (const name of ['A', 'B', 'C', 'bump', 'v1bump', 'baseline']) {
    const step = plan[name];
    if (!step) continue;
    assertQaBuild(step);
    const built = await agent.call('exec', { command: step.command, args: step.args, cwd: step.cwd, env: step.env, deadlineMs: 3_600_000 });
    writeFileSync(path.join(localEvidence, `build-${name}.log`), `exit ${built.status}\n${built.stdout}\n${built.stderr}`);
    if (built.status !== 0) throw new GuardError('BUILD_FAILED', `${name} exited ${built.status}`);
    if (name === 'baseline') {
      artifacts.baseline = { name, daemonExecutable: joinFor(host.platform, step.env.CARGO_TARGET_DIR, 'debug', host.platform === 'windows' ? 'ferryx.exe' : 'ferryx') };
      continue;
    }
    // Each build overwrites the same bundle path: move it to a per-artifact dir under the run dir.
    const fresh = artifactPaths({ platform: host.platform, targetDir: host.cargoTarget, productName, name });
    const destRoot = joinFor(host.platform, runDir, 'artifacts', name);
    await agent.call('mkdir', { dir: destRoot });
    const src = fresh.appPath ?? path.dirname(fresh.guiExecutable);
    if (host.platform === 'mac') await agent.call('exec', { command: 'ditto', args: [src, joinFor(host.platform, destRoot, path.basename(src))] });
    else await agent.call('exec', host.platform === 'windows'
      ? { command: 'powershell.exe', args: ['-NoProfile', '-Command', `Copy-Item -LiteralPath '${fresh.guiExecutable}','${fresh.cliExecutable}' -Destination '${destRoot}'`] }
      : { command: 'cp', args: [fresh.guiExecutable, fresh.cliExecutable, destRoot] });
    const art = host.platform === 'mac'
      ? { name, appPath: joinFor(host.platform, destRoot, path.basename(src)), guiExecutable: macGuiExecutable(joinFor(host.platform, destRoot, path.basename(src))), cliExecutable: fresh.cliExecutable }
      : { name, appPath: null, guiExecutable: joinFor(host.platform, destRoot, path.basename(fresh.guiExecutable)), cliExecutable: joinFor(host.platform, destRoot, path.basename(fresh.cliExecutable)) };
    if (host.platform === 'mac') {
      // todo 13: each QA artifact carries its own signed helper and a re-sealed outer signature.
      const helper = await run('node', ['-e', `import('${path.join(REPO, 'scripts/macos-daemon-helper.mjs')}').then(async (h) => { const r = await import('${path.join(REPO, 'scripts/lib/release-platforms.mjs')}'); const a = h.assembleDaemonHelper({ appPath: process.argv[1], identity: '-', entitlements: process.argv[2] }); r.finalizeMacosBundle({ appPath: process.argv[1], workspaceDir: process.argv[3], signingIdentity: '-', approveNotarization: false, entitlements: process.argv[2] }); console.log(JSON.stringify(a)); })`, art.appPath, joinFor(host.platform, destRoot, 'outer-entitlements.plist'), destRoot], { deadlineMs: 600_000 });
      writeFileSync(path.join(localEvidence, `helper-${name}.log`), `exit ${helper.status}\n${helper.stdout}\n${helper.stderr}`);
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

  const allowedRoots = [runDir, host.cargoTarget];
  for (const art of Object.values(artifacts)) {
    for (const exe of [art.guiExecutable, art.daemonExecutable].filter(Boolean)) assertQaExecutable(exe, { allowedRoots, forbidden, platform: host.platform });
  }
  const mahord = host.gui.kind === 'mahord' ? await connectMahord(host.gui, { log }) : null;
  const jobs = createJobs({ host, agent, runId, layout, uid });
  const gui = createGuiDriver({ host, agent, runId, layout, allowedRoots, forbidden, mahord, guiEnv: env });
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
    qaRoots: [runDir, host.cargoTarget], guiExecutables: Object.values(artifacts).map((a) => a.guiExecutable).filter(Boolean),
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
      if (mahord) return mahord.type(text);
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

  // ---- teardown: recorded pids only, QA-named jobs only, own run dir only ----
  const t = await agent.call('teardownRecorded');
  summary.teardown.killed = t.killed.map((e) => ({ pid: e.pid, role: e.role }));
  summary.teardown.refused = t.refused;
  for (const name of await jobs.listQaJobs()) {
    await jobs.remove(name);
    summary.teardown.jobsRemoved.push(name);
  }
  if (host.platform === 'linux') await agent.call('exec', { command: 'systemctl', args: ['--user', 'daemon-reload'] });
  summary.teardown.leftovers = [
    ...(await jobs.listQaJobs()).map((j) => ({ kind: 'job', name: j })),
    ...(await agent.call('table')).filter((r) => r.executable && ctx.qaRoots.some((root) => r.executable.toLowerCase().startsWith(root.toLowerCase()))).map((r) => ({ kind: 'process', pid: r.pid, executable: r.executable })),
  ];
  if (summary.teardown.leftovers.length === 0) {
    await agent.call('remove', { target: runDir });
    summary.teardown.dirsRemoved.push(runDir);
    if (await agent.call('exists', { file: runDir })) summary.teardown.leftovers.push({ kind: 'dir', path: runDir });
  }
  summary.releaseDaemon.after = await prodSnapshot();
  summary.releaseDaemon.unchanged = summary.releaseDaemon.before === summary.releaseDaemon.after;
  if (mahord) await mahord.close();
  await agent.stop();
  summary.finishedAt = new Date().toISOString();
  summary.exitCode = computeExitCode(summary, selected);
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
