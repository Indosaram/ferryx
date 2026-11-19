import { mkdir, rmdir } from 'node:fs/promises';
import path from 'node:path';
import { assertRunId, GuardError, isInside, qaProductName } from './guards.mjs';
import { run } from './proc.mjs';

const bad = (detail) => { throw new GuardError('QA_ARTIFACT_OPTIONS', detail); };
const oid = /^[0-9a-f]{40}$/;

export function validateArtifactOptions(opts) {
  if (!opts.buildOnly && !opts.reuseArtifacts) {
    if (opts.manifestSha256 || opts.v1Rev || opts.baselineRev) bad('artifact pins require --reuse-artifacts');
    return;
  }
  if (opts.buildOnly && opts.reuseArtifacts) bad('--build-only and --reuse-artifacts are exclusive');
  if (opts.selfTestReload) bad('self-test builds cannot produce or consume reusable artifacts');
  if (opts.runId) assertRunId(opts.runId);
  if (opts.buildOnly) {
    if (opts.only.length || opts.all || opts.fault || opts.selfCheck || opts.keepRunDir || opts.evidenceDir || opts.manifestSha256 || opts.v1Rev || opts.baselineRev) {
      bad('--build-only DIR takes no scenarios, evidence directory, or consumer options');
    }
  } else {
    if (!opts.runId || !/^[0-9a-f]{64}$/.test(opts.manifestSha256 ?? '')) bad('--reuse-artifacts requires its baked --run-id and the producer\'s --manifest-sha256');
    if (opts.v1Worktree || opts.baselineWorktree) bad('use --v1-rev or --baseline-rev instead of a build worktree for reuse');
    for (const rev of [opts.v1Rev, opts.baselineRev]) if (rev && !oid.test(rev)) bad('optional revision pins must be full commit IDs');
    if (opts.selfCheck && (opts.all || opts.only.length || opts.fault || opts.keepRunDir)) bad('artifact --self-check verifies bytes without running scenarios');
  }
}

export function requireNativeArtifactHost(host) {
  if (host.transport.kind !== 'local' || { mac: 'darwin', linux: 'linux', windows: 'win32' }[host.platform] !== process.platform) {
    bad(`artifact modes must run on their native ${host.platform} host`);
  }
}

export function separateArtifactPaths(directory, otherPaths, platform) {
  for (const other of otherPaths.filter(Boolean)) {
    if (isInside(directory, other, platform) || isInside(other, directory, platform)) {
      throw new GuardError('QA_ARTIFACT_PATH_OVERLAP', `${directory} overlaps ${other}`);
    }
  }
}

export async function assertIdleArtifactRun({ jobs, agent, runDirs, platform, runId }) {
  const names = await jobs.listQaJobs({ strict: true });
  const xml = await jobs.listQaTaskXml();
  const table = await agent.call('table');
  if (!Array.isArray(table) || !table.length) throw new GuardError('QA_ARTIFACT_INVENTORY', 'process table unavailable');
  const appSuffix = `/${qaProductName(runId)}.app/Contents/MacOS/ferryx`;
  const holders = table.filter((p) => p.executable && (runDirs.some((d) => isInside(p.executable, d, platform))
    || (platform === 'mac' && p.executable.endsWith(appSuffix))));
  if (names.length || xml.length || holders.length) {
    throw new GuardError('QA_ARTIFACT_RUN_IN_USE', JSON.stringify({ jobs: names, xml, pids: holders.map((p) => p.pid) }));
  }
}

// mkdir without recursive on the leaf is the ownership claim. Never adopt an existing run:
// bundles bake the run ID into their identity, so a parallel or unfinished attempt can share it.
export async function claimFreshDirectories(dirs) {
  const claimed = [];
  try {
    for (const dir of new Set(dirs)) {
      await mkdir(path.dirname(dir), { recursive: true, mode: 0o700 });
      await mkdir(dir, { mode: 0o700 });
      claimed.push(dir);
    }
  } catch (error) {
    for (const dir of claimed.reverse()) await rmdir(dir); // Only empty directories we just created.
    throw error;
  }
}

async function gitOutput(dir, args) {
  const result = await run('git', ['-C', dir, ...args]);
  if (result.status !== 0) throw new GuardError('QA_ARTIFACT_REVISION', `${dir}: git ${args[0]} exited ${result.status}: ${result.stderr}`);
  return result.stdout.trim();
}

async function cleanHead(dir) {
  const head = await gitOutput(dir, ['rev-parse', '--verify', 'HEAD^{commit}']);
  const dirty = await gitOutput(dir, ['status', '--porcelain=v1', '--untracked-files=normal', '--ignore-submodules=none']);
  if (!oid.test(head) || dirty) throw new GuardError('QA_ARTIFACT_REVISION', `a clean, identifiable checkout is required: ${dir}: ${dirty || head}`);
  return head;
}

export async function artifactIdentity({ repo, platform, runId, v1Worktree, baselineWorktree, v1Rev, baselineRev }) {
  const head = await cleanHead(repo);
  const tree = await gitOutput(repo, ['rev-parse', '--verify', `${head}^{tree}`]);
  const cargo = await gitOutput(repo, ['show', `${head}:src-tauri/Cargo.toml`]);
  const version = cargo.match(/^version = "([^"]+)"/m)?.[1];
  if (!oid.test(tree) || !version) throw new GuardError('QA_ARTIFACT_REVISION', `missing tree or product version at ${head}`);
  return { platform, arch: process.arch, head, tree, runId, version,
    v1Head: v1Worktree ? await cleanHead(v1Worktree) : v1Rev ?? null,
    baselineHead: baselineWorktree ? await cleanHead(baselineWorktree) : baselineRev ?? null };
}
