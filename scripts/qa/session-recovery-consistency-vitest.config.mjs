import { defineConfig } from '../../ui/node_modules/vitest/dist/config.js';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const uiNodeModules = fileURLToPath(new URL('../../ui/node_modules', import.meta.url));

export default defineConfig({
  root: fileURLToPath(new URL('../../', import.meta.url)),
  resolve: {
    alias: {
      vitest: resolve(uiNodeModules, 'vitest'),
    },
  },
  test: { environment: 'node', include: ['scripts/qa/session-recovery-consistency.test.mjs'], fileParallelism: false, maxWorkers: 1, testTimeout: 10000 },
});
