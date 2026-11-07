import { execFileSync } from 'node:child_process';
import { readdirSync, realpathSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gitEnv } from './git-env.mjs';

export function gitObjectsSource({ override = process.env.F1_GIT_OBJECTS, scriptDir = dirname(fileURLToPath(import.meta.url)) } = {}) {
  try {
    const path = override !== undefined ? override : execFileSync('git', ['-C', scriptDir, 'rev-parse', '--path-format=absolute', '--git-path', 'objects'], { encoding: 'utf8', env: gitEnv() }).trim();
    if (!isAbsolute(path)) throw new Error('Git objects source must be absolute');
    const canonical = realpathSync(path);
    const populated = readdirSync(canonical).some((name) => {
      if (!/^[0-9a-f]{2}$/.test(name) && name !== 'pack') return false;
      return readdirSync(join(canonical, name)).some((entry) => (name === 'pack' ? /^pack-[0-9a-f]+\.pack$/ : /^[0-9a-f]{38,62}$/).test(entry) && statSync(join(canonical, name, entry)).isFile());
    });
    if (!populated) throw new Error('Git objects source contains no objects');
    return { path: canonical, reason: null };
  } catch (error) { return { path: null, reason: error.message }; }
}
