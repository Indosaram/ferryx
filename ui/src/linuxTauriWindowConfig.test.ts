import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

type WindowConfig = Record<string, unknown> & { label: string };
type TauriConfig = { app: { windows: WindowConfig[] } };

const PLATFORM_OVERRIDES = {
  hiddenTitle: false,
  titleBarStyle: "Visible",
  transparent: false,
} as const;

async function readMainWindow(fileName: string): Promise<WindowConfig | undefined> {
  const path = resolve(import.meta.dirname, "../../src-tauri", fileName);
  const config = JSON.parse(await readFile(path, "utf8")) as TauriConfig;
  return config.app.windows.find(({ label }) => label === "main");
}

describe("Linux Tauri window configuration", () => {
  it("overrides only the macOS chrome contract and keeps every base window setting", async () => {
    // Given: the base window contract and the Linux platform override.
    const [base, linux] = await Promise.all([
      readMainWindow("tauri.conf.json"),
      readMainWindow("tauri.linux.conf.json"),
    ]);

    // When: Tauri replaces `app.windows` wholesale with the Linux override array.
    // Then: the Linux window must still carry the base identity and geometry.
    expect(linux).toEqual({ ...base, ...PLATFORM_OVERRIDES });
  });
});
