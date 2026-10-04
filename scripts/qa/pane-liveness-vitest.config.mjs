import { defineConfig } from "../../ui/node_modules/vitest/dist/config.js";
import { fileURLToPath } from "node:url";
// Mirrors scripts/qa/ferryx-scope-vitest.config.mjs so the sole remote
// verifier can run the Task 3 runner unit suite with an exact command.
// Resolve includes from the repository root, independent of the gate's ui/ cwd and checkout path.
//
// Pass-19: the frozen gate command is `bun run --cwd ui test --config
// ../scripts/qa/pane-liveness-vitest.config.mjs` - ONE canonical invocation, and
// its argv is unchanged. The pass-18 delegation-stall/sink suite is listed HERE,
// in that same config, so the frozen command really executes it: a second config
// nothing invokes would leave the retry coverage outside every gate. The frozen
// suite (scripts/qa/pane-liveness.test.mjs) holds 73 tests after the pass-22
// audit repairs (the throwing read, the uncharged-window refusal, and the
// archive's pre-reap markers), and the pass-19/21 file adds
// 20, so the one frozen command runs 93.
export default defineConfig({ root: fileURLToPath(new URL("../../", import.meta.url)), test: { environment: "node", include: ["scripts/qa/pane-liveness.test.mjs", "scripts/qa/pane-liveness-delegation-retry.test.mjs"], fileParallelism: false, maxWorkers: 1, testTimeout: 10000 } });
