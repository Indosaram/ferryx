import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  OPEN_BROWSER_URL_EVENT,
  OPEN_SETTINGS_EVENT,
  getBrowserEnginePaths,
  listBrowserExtensions,
  openBrowserExtensionPage,
  openBrowserUrlInNewTab,
  openSettingsSection,
} from "./browserExtensions";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => vi.resetAllMocks());

describe("browserExtensions bindings", () => {
  it("lists extensions for a profile", async () => {
    vi.mocked(invoke).mockResolvedValue({ profile: "work", engineInstalled: true, supported: true, extensions: [] });
    await listBrowserExtensions("work");
    expect(invoke).toHaveBeenCalledWith("cmd_browser_extensions_list", { profile: "work" });
  });

  it("opens extension pages with the page descriptor and profile", async () => {
    vi.mocked(invoke).mockResolvedValue({});
    await openBrowserExtensionPage({ kind: "details", id: "abc" }, "work");
    expect(invoke).toHaveBeenCalledWith("cmd_browser_extensions_open_page", {
      profile: "work",
      page: { kind: "details", id: "abc" },
    });
    await openBrowserExtensionPage({ kind: "manage" });
    expect(invoke).toHaveBeenLastCalledWith("cmd_browser_extensions_open_page", {
      profile: undefined,
      page: { kind: "manage" },
    });
  });

  it("reads the engine paths", async () => {
    vi.mocked(invoke).mockResolvedValue({ installDir: "/e", profilesDir: "/p" });
    await expect(getBrowserEnginePaths()).resolves.toEqual({ installDir: "/e", profilesDir: "/p" });
    expect(invoke).toHaveBeenCalledWith("cmd_browser_engine_paths");
  });

  it("dispatches the open-settings and open-url window events", () => {
    const seen: Array<{ type: string; detail: unknown }> = [];
    const record = (event: Event) => seen.push({ type: event.type, detail: (event as CustomEvent).detail });
    window.addEventListener(OPEN_SETTINGS_EVENT, record);
    window.addEventListener(OPEN_BROWSER_URL_EVENT, record);
    openSettingsSection("browser", "browser-engine");
    openBrowserUrlInNewTab("https://example.test", "work");
    window.removeEventListener(OPEN_SETTINGS_EVENT, record);
    window.removeEventListener(OPEN_BROWSER_URL_EVENT, record);
    expect(seen).toEqual([
      { type: OPEN_SETTINGS_EVENT, detail: { section: "browser", anchor: "browser-engine" } },
      { type: OPEN_BROWSER_URL_EVENT, detail: { url: "https://example.test", profileId: "work" } },
    ]);
  });
});
