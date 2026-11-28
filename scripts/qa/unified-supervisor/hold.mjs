import { mkdirSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { runSlackLeg } from './slack.mjs';
import { openHubReader, registerWorkspace, waitForChange, ADMISSION_WORKSPACE_ID } from './scenarios.mjs';

/**
 * Owner hold in a Slack lane thread is read back, survives a recovery tick,
 * and only explicit continue releases it; failed readback says not held.
 */
export async function runHold(ctx) {
  const hubDb = join(ctx.paths.data, 'hub', 'hub.sqlite');
  // The isolated projectRoot need not be a git repository: use a throwaway repo in the run's data dir.
  const repo = join(ctx.paths.data, 'hold-repo');
  mkdirSync(repo, { recursive: true });
  const init = spawnSync('git', ['-c', 'user.name=QA', '-c', 'user.email=qa@example.com', 'init', '-q', repo], { encoding: 'utf8', timeout: 30_000 });
  if (init.status !== 0) throw new Error(`hold fixture git init failed: ${init.stderr}`);
  const gitCommonDir = realpathSync(join(repo, '.git'));
  await runSlackLeg(ctx, {
    root: ctx.paths.data,
    dbPath: hubDb,
    hub: await openHubReader(hubDb),
    waitForChange,
    setupCli: (owner, env) => ctx.cli(
      ['supervisor', 'setup', '--owner', owner, '--project', 'qa-hold',
       '--repo-identity', 'qa:qa-hold', '--host', 'local', '--git-common-dir', gitCommonDir, '--json'],
      { label: 'hold-owner-setup', env },
    ),
    registerWorkspace: (env) => registerWorkspace(ctx, join(env.FERRYX_RUNTIME_DIR, 'daemon.sock'), ADMISSION_WORKSPACE_ID, realpathSync(repo)),
    mode: 'hold',
  });
}
