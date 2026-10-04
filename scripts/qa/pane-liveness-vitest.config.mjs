import { defineConfig } from "../../ui/node_modules/vitest/dist/config.js";
import { fileURLToPath } from "node:url";
// Mirrors scripts/qa/ferryx-scope-vitest.config.mjs so the sole remote
// verifier can run the Task 3 runner unit suite with an exact command.
// Resolve includes from the repository root, independent of the gate's ui/ cwd and checkout path.
export default defineConfig({ root: fileURLToPath(new URL("../../", import.meta.url)), test: { environment: "node", include: ["scripts/qa/pane-liveness.test.mjs"], fileParallelism: false, maxWorkers: 1, testTimeout: 10000 } });
