import { defineConfig } from '../../ui/node_modules/vitest/dist/config.js';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const uiNodeModules = fileURLToPath(new URL('../../ui/node_modules', import.meta.url));

// Canonical Vitest configuration for consistent session recovery QA contracts in scripts/qa.
// Extends the node-environment pattern from session-recovery-consistency-vitest.config.mjs
// with an exact test.include union covering all authored QA Vitest contract files.
// Resolves from candidate repository root with isolated single-worker execution.
export default defineConfig({
  root: fileURLToPath(new URL('../../', import.meta.url)),
  resolve: {
    alias: {
      vitest: resolve(uiNodeModules, 'vitest'),
    },
  },
  test: {
    environment: 'node',
    include: [
      'scripts/qa/session-recovery-consistency.test.mjs',
      'scripts/qa/session-recovery-provisioned.test.mjs',
      'scripts/qa/session-recovery-native.contract.test.mjs',
      'scripts/qa/fixtures/native-configured-adapters.contract.test.mjs',
      'scripts/qa/fixtures/native-forwarding-transport.contract.test.mjs',
      'scripts/qa/fixtures/fixture-pty-child.contract.test.mjs',
    ],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 10000,
  },
});

