import { spawnSync } from "bun";
import { fileURLToPath } from "node:url";
import { createServer } from "../ui/node_modules/vite/dist/node/index.js";

const uiRoot = fileURLToPath(new URL("../ui", import.meta.url));

export async function startFrontend({ build = true, cacheDir } = {}) {
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
    ...(cacheDir ? { cacheDir } : {}),
  });

  await vite.listen();
  return vite;
}

if (import.meta.main) {
  const noBuild = process.env.FERRYX_DEV_FRONTEND_NO_BUILD === "1";
  const cacheDir = process.env.FERRYX_DEV_CACHE_DIR || undefined;
  await startFrontend({ build: !noBuild, cacheDir });
  console.log("FERRYX_FRONTEND_READY");
}
