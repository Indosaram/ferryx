import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test, expect } from "bun:test";

import { startFrontend } from "./dev-frontend.mjs";

const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url)));
const scriptUrl = new URL("./dev-frontend.mjs", import.meta.url);

test("Tauri owns one long-lived frontend runner", () => {
  expect(existsSync(scriptUrl)).toBe(true);
  const script = readFileSync(scriptUrl, "utf8");
  expect(config.build.beforeDevCommand).toBe("bun scripts/dev-frontend.mjs");
  expect(script).toContain('spawnSync(["bun", "run", "--cwd", "ui", "build"]');
  expect(script).toContain("process.chdir(uiRoot)");
  expect(script).toContain('createServer({');
  expect(script).toContain("root: uiRoot");
  expect(script).toContain('configFile: fileURLToPath(new URL("../ui/vite.config.ts", import.meta.url))');
  expect(script).toContain("await vite.listen()");
  expect(script).not.toContain('spawn(["bun", "run", "--cwd", "ui", "dev"]');
});

test(
  "the real runner serves Ferryx Tailwind utilities",
  async () => {
    const tempCacheDir = mkdtempSync(join(tmpdir(), "ferryx-vite-cache-"));
    const scriptPath = fileURLToPath(scriptUrl);
    const proc = Bun.spawn(["bun", scriptPath], {
      env: {
        ...process.env,
        FERRYX_DEV_FRONTEND_NO_BUILD: "1",
        FERRYX_DEV_CACHE_DIR: tempCacheDir,
      },
      stdout: "pipe",
      stderr: "pipe",
    });

    try {
      // Await FERRYX_FRONTEND_READY signal from the runner subprocess stdout
      const reader = proc.stdout.getReader();
      const stderrPromise = new Response(proc.stderr).text();
      const decoder = new TextDecoder();
      let output = "";
      try {
        while (!output.includes("FERRYX_FRONTEND_READY")) {
          const { value, done } = await reader.read();
          if (done) {
            const exitCode = await proc.exited;
            throw new Error(`frontend runner exited before readiness (exit ${exitCode}):\n${await stderrPromise}`);
          }
          output += decoder.decode(value, { stream: true });
        }
      } finally {
        reader.releaseLock();
      }
      expect(output).toContain("FERRYX_FRONTEND_READY");

      const response = await fetch("http://127.0.0.1:5173/src/index.css");
      expect(response.status).toBe(200);
      const css = await response.text();
      expect(css).toContain(".h-screen");
      expect(css).toContain(".w-screen");
      expect(css).toContain(".bg-background");
    } finally {
      proc.kill();
      await proc.exited;
      rmSync(tempCacheDir, { recursive: true, force: true });
    }
  },
  30_000,
);
