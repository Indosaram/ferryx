import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';

// Own only this fresh directory. Never stage into the checkout, inherit a
// runtime endpoint, or remove a fixed/shared root as the historical probe did.
const root = realpathSync(mkdtempSync(join(tmpdir(), 'ferryx-edge-owned-')));
const driver = join(root, 'probe-daemon-edges.mjs');
const nonce = randomUUID();
try {
  copyFileSync(fileURLToPath(new URL('./probe-daemon-edges.mjs', import.meta.url)), driver);
  const child = spawnSync(process.execPath, [driver, nonce], { encoding: 'utf8', timeout: 10000 });
  assert.ifError(child.error);
  assert.equal(child.status, 0, child.stderr);
  assert.deepEqual(JSON.parse(child.stdout), { driver: realpathSync(driver), nonce });
} finally {
  rmSync(root, { recursive: true });
  assert.equal(existsSync(root), false);
}
console.log('EDGE_FIXTURE_STAGED_EXECUTED_CLEANED');
