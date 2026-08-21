// @vitest-environment node

import { describe, expect, it } from "vitest";
import type { UserConfig } from "vite";

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
