import { existsSync, readFileSync } from "node:fs";
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
    const originalCwd = process.cwd();
    const vite = await startFrontend({ build: false });

    try {
      const response = await fetch("http://127.0.0.1:5173/src/index.css");
      expect(response.status).toBe(200);
      const css = await response.text();
      expect(css).toContain(".h-screen");
      expect(css).toContain(".w-screen");
      expect(css).toContain(".bg-background");
    } finally {
      await vite.close();
      process.chdir(originalCwd);
    }
  },
  30_000,
);
