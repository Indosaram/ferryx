// @vitest-environment node

import { describe, expect, it, vi } from "vitest";
import type { UserConfig } from "vite";

// Assert the real config module, but neutralize what makes importing it expensive.
// The module is loaded through Vite's own pipeline, so importing it drags in the whole
// `vite` package plus @vitejs/plugin-react and instantiates the React plugin, and
// `buildStamp()` shells out to `git rev-parse` and `git status --porcelain` (2.4s over
// this repository on macOS with a cold cache). None of that has anything to do with the
// server endpoint and watcher settings asserted below, yet it made this file outlast the
// 5s test deadline when the suite ran it under load while it passed alone.
//
// Mocking the two heavy modules to their identities and pinning the two git calls keeps
// vite.config.ts itself as the source under test, so a regression in how it assembles
// `server` is still caught, while the import stays cheap and cannot vary with the tree.
vi.mock("vite", () => ({ defineConfig: (config: unknown) => config, mergeConfig: (a: object, b: object) => ({ ...a, ...b }) }));
vi.mock("@vitejs/plugin-react", () => ({ default: () => ({ name: "vite:react-babel" }) }));
vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:child_process")>();
  return {
    ...actual,
    execFileSync: (_file: string, args: readonly string[]) =>
      args[0] === "status" ? "" : "test-revision\n",
  };
});

describe("Tauri Vite HMR configuration", () => {
  it("uses one deterministic IPv4 loopback endpoint and polling file watching", async () => {
    const configPath = "../../vite.config";
    const configModule = await import(/* @vite-ignore */ configPath);
    const config = configModule.default as UserConfig;

    expect(config.server?.host).toBe("127.0.0.1");
    expect(config.server?.port).toBe(5173);
    expect(config.server?.strictPort).toBe(true);
    expect(config.server?.hmr).toMatchObject({
      protocol: "ws",
      host: "127.0.0.1",
      clientPort: 5173,
    });
    expect(config.server?.watch).toMatchObject({
      usePolling: true,
      interval: 100,
    });
  });
});
