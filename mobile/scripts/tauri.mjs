import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
// Use an absolute CLI path so the generated Gradle callback works from src-tauri.
const cli = fileURLToPath(new URL("../../node_modules/@tauri-apps/cli/tauri.js", import.meta.url));
const result = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], {
  cwd: fileURLToPath(new URL("..", import.meta.url)),
  stdio: "inherit",
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
