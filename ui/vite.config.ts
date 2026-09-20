import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";

const DEV_HOST = "127.0.0.1";
const DEV_PORT = 5173;

/**
 * Build identity stamped into the bundle: version, source revision (dirty-marked), build clock.
 * A phone that shows an older stamp than the host serves is running a stale client.
 */
function buildStamp(): string {
  const version = JSON.parse(readFileSync(path.resolve(__dirname, "package.json"), "utf8")).version as string;
  let revision = "unknown";
  try {
    const sha = execFileSync("git", ["rev-parse", "--short", "HEAD"], { cwd: __dirname }).toString().trim();
    const dirty = execFileSync("git", ["status", "--porcelain"], { cwd: __dirname }).toString().trim().length > 0;
    revision = `${sha}${dirty ? "+dirty" : ""}`;
  } catch {
    revision = "unknown";
  }
  const builtAt = new Date().toISOString().slice(0, 16).replace("T", " ");
  return `${version} ${revision} ${builtAt}`;
}

export default defineConfig({
  plugins: [react()],
  define: {
    __FERRYX_BUILD__: JSON.stringify(buildStamp()),
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    host: DEV_HOST,
    port: DEV_PORT,
    strictPort: true,
    hmr: {
      protocol: "ws",
      host: DEV_HOST,
      clientPort: DEV_PORT,
    },
    watch: {
      usePolling: true,
      interval: 100,
      ignored: ["**/src-tauri/**", "**/target/**", "**/.git/**"],
    },
  },
});
