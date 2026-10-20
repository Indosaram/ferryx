import { defineConfig } from '../../ui/node_modules/vitest/dist/config.js';
import { fileURLToPath } from 'node:url';

// Canonical Vitest configuration for consistent session recovery QA contracts in scripts/qa.
// Extends the node-environment pattern from session-recovery-consistency-vitest.config.mjs
// with an exact test.include union covering all authored QA Vitest contract files.
// Resolves from candidate repository root with isolated single-worker execution.
export default defineConfig({
  root: fileURLToPath(new URL('../../', import.meta.url)),
  test: {
    environment: 'node',
    include: [
      'scripts/qa/session-recovery-consistency.test.mjs',
      'scripts/qa/session-recovery-provisioned.test.mjs',
      'scripts/qa/session-recovery-native.contract.test.mjs',
      'scripts/qa/fixtures/native-configured-adapters.contract.test.mjs',
    ],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 10000,
  },
});

