import { defineConfig } from "../../ui/node_modules/vitest/dist/config.js";
export default defineConfig({ test: { environment: "node", include: ["scripts/qa/ferryx-scope-*.test.mjs"], fileParallelism: false, maxWorkers: 1, testTimeout: 10000 } });
