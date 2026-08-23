import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const projectRoot = resolve(import.meta.dirname, "../..");

describe("Ferryx development runtime contract", () => {
  it("keeps bun tauri dev wired to the Tauri CLI", () => {
    const rootPackage: unknown = JSON.parse(readFileSync(resolve(projectRoot, "package.json"), "utf8"));

    expect(rootPackage).toMatchObject({
      name: "ferryx",
      private: true,
      scripts: {
        tauri: "cargo tauri",
        dev: "cargo tauri dev",
      },
    });
  });

  it("keeps the Tauri dev URL aligned with strict Vite HMR", () => {
    const tauriConfig: unknown = JSON.parse(
      readFileSync(resolve(projectRoot, "src-tauri/tauri.conf.json"), "utf8"),
    );
    const viteConfig = readFileSync(resolve(projectRoot, "ui/vite.config.ts"), "utf8");

    expect(tauriConfig).toMatchObject({
      build: {
        beforeDevCommand: "bun run --cwd ui dev",
        devUrl: "http://127.0.0.1:5173",
      },
    });
    expect(viteConfig).toContain('const DEV_HOST = "127.0.0.1";');
    expect(viteConfig).toContain("const DEV_PORT = 5173;");
    expect(viteConfig).toContain("strictPort: true");
  });
});
