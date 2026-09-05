import { test, expect } from '../../ui/node_modules/vitest/dist/index.js';
import { spawnSync } from 'node:child_process';
test('noninteractive child creation under the project test runner', () => {
  const child = spawnSync(process.execPath, ['--version'], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 5000 });
  expect(child.error).toBeUndefined();
  expect(child.status).toBe(0);
});
