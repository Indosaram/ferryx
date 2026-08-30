import { spawnSync } from "bun";
import { fileURLToPath } from "node:url";
import { createServer } from "../ui/node_modules/vite/dist/node/index.js";

const uiRoot = fileURLToPath(new URL("../ui", import.meta.url));

export async function startFrontend({ build = true } = {}) {
  if (build) {
    const result = spawnSync(["bun", "run", "--cwd", "ui", "build"], {
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
    });

    if (result.exitCode !== 0) {
      process.exit(result.exitCode);
    }
  }

  process.chdir(uiRoot);

  const vite = await createServer({
    root: uiRoot,
    configFile: fileURLToPath(new URL("../ui/vite.config.ts", import.meta.url)),
  });

  await vite.listen();
  return vite;
}

if (import.meta.main) {
  await startFrontend();
  console.log("FERRYX_FRONTEND_READY");
}
