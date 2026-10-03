import { defineConfig } from "../../ui/node_modules/vitest/dist/config.js";
// Mirrors scripts/qa/ferryx-scope-vitest.config.mjs so the sole remote
// verifier can run the Task 3 runner unit suite with an exact command.
export default defineConfig({ test: { environment: "node", include: ["scripts/qa/pane-liveness.test.mjs"], fileParallelism: false, maxWorkers: 1, testTimeout: 10000 } });
