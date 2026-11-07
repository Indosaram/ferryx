import { spawnSync } from 'node:child_process';

const REPOSITORY_ENV = [
  'GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY',
  'GIT_ALTERNATE_OBJECT_DIRECTORIES', 'GIT_CEILING_DIRECTORIES', 'GIT_COMMON_DIR', 'GIT_NAMESPACE',
];

export function gitEnv(extra = {}) {
  const env = { ...process.env, ...extra };
  for (const key of REPOSITORY_ENV) delete env[key];
  return env;
}

export function runGit(args, opts = {}) {
  return spawnSync('git', args, { ...opts, env: gitEnv(opts.env) });
}
