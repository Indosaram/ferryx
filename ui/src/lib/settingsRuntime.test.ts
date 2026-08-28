import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";

import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  APPEARANCE_SETTINGS_EVENT,
  applyAppearanceSettings,
  loadAppearanceSettings,
  resetAppearanceSettings,
  saveAppearanceSettings,
  useApplyAppearanceSettings,
} from "./appearanceSettings";
import {
  loadBrowserSettings,
  normalizeBrowserAddress,
  resetBrowserSettings,
  saveBrowserSettings,
} from "./browserSettings";
import {
  _resetSettingsRuntimeBridgeForTest,
  installSettingsRuntimeBridge,
} from "./settingsRuntimeBridge";

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-accent");
  document.documentElement.removeAttribute("data-density");
});

describe("settings runtime contracts", () => {
  it("persists and applies appearance theme, accent, and density", () => {
    const next = saveAppearanceSettings({ theme: "light", accentColor: "emerald", density: "comfortable" });
    expect(loadAppearanceSettings()).toEqual(next);

    applyAppearanceSettings(next);
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.accent).toBe("emerald");
    expect(document.documentElement.dataset.density).toBe("comfortable");
    expect(document.documentElement.style.colorScheme).toBe("light");

    resetAppearanceSettings();
    expect(loadAppearanceSettings()).toMatchObject({ theme: "charcoal", accentColor: "default", density: "compact" });
  });

  it("applies live appearance updates when useApplyAppearanceSettings is mounted", () => {
    function AppearanceWatcher() {
      useApplyAppearanceSettings();
      return null;
    }
    const { unmount } = render(createElement(AppearanceWatcher));

    act(() => {
      saveAppearanceSettings({ theme: "light", accentColor: "emerald", density: "comfortable" });
    });

    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.accent).toBe("emerald");
    expect(document.documentElement.dataset.density).toBe("comfortable");
    expect(document.documentElement.style.colorScheme).toBe("light");

    unmount();
  });

  it("wires useApplyAppearanceSettings into the App root component", () => {
    const appSource = readFileSync(resolve(__dirname, "../App.tsx"), "utf8");
    expect(appSource).toMatch(/useApplyAppearanceSettings\(\)/);
  });

  it("does not perform DOM text patching or arbitrary div scans in settingsRuntimeBridge", () => {
    const bridgeSource = readFileSync(resolve(__dirname, "./settingsRuntimeBridge.ts"), "utf8");
    expect(bridgeSource).not.toContain(".textContent =");
    expect(bridgeSource).not.toContain('querySelectorAll("div")');
    expect(bridgeSource).not.toContain('querySelectorAll("h2")');
  });

  it("reapplies persisted appearance through the runtime bridge after remount", () => {
    saveAppearanceSettings({ theme: "light", accentColor: "blue", density: "comfortable" });
    applyAppearanceSettings(loadAppearanceSettings());
    expect(document.documentElement.dataset.theme).toBe("light");

    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-accent");
    document.documentElement.removeAttribute("data-density");

    _resetSettingsRuntimeBridgeForTest();
    installSettingsRuntimeBridge();

    expect(loadAppearanceSettings()).toEqual({
      theme: "light",
      accentColor: "blue",
      density: "comfortable",
    });
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.accent).toBe("blue");
    expect(document.documentElement.dataset.density).toBe("comfortable");
  });

  it("dispatches appearance changes in the same webview", () => {
    let detail: unknown;
    const listener = (event: Event) => {
      detail = (event as CustomEvent).detail;
    };
    window.addEventListener(APPEARANCE_SETTINGS_EVENT, listener, { once: true });
    saveAppearanceSettings({ accentColor: "rose" });
    expect(detail).toMatchObject({ accentColor: "rose" });
  });

  it("uses the configured search provider for address-bar queries", () => {
    saveBrowserSettings({ searchEngine: "brave" });
    expect(normalizeBrowserAddress("rust tauri settings")).toBe(
      "https://search.brave.com/search?q=rust%20tauri%20settings",
    );
    expect(normalizeBrowserAddress("localhost:5173")).toBe("http://localhost:5173");
    expect(normalizeBrowserAddress("example.com/docs")).toBe("https://example.com/docs");
    expect(normalizeBrowserAddress("https://example.com")).toBe("https://example.com");
  });

  it("persists browser zoom and restore preference and resets them", () => {
    saveBrowserSettings({ defaultZoom: 125, restoreTabsOnLaunch: true });
    expect(loadBrowserSettings()).toMatchObject({ defaultZoom: 125, restoreTabsOnLaunch: true });
    resetBrowserSettings();
    expect(loadBrowserSettings()).toMatchObject({ defaultZoom: 100, restoreTabsOnLaunch: false });
  });

  describe("installSettingsRuntimeBridge", () => {
    beforeEach(() => {
      _resetSettingsRuntimeBridgeForTest();
    });

    it("does not attach a document-wide subtree MutationObserver on documentElement", () => {
      const observeSpy = vi.spyOn(MutationObserver.prototype, "observe");
      installSettingsRuntimeBridge();
      expect(observeSpy).not.toHaveBeenCalledWith(
        document.documentElement,
        expect.objectContaining({ subtree: true }),
      );
    });

    it("re-applies system appearance when OS color scheme flips", () => {
      saveAppearanceSettings({ theme: "system" });
      let mediaCallback: (() => void) | undefined;
      const matchMediaMock = vi.fn().mockImplementation((query: string) => ({
        matches: true,
        media: query,
        addEventListener: (_event: string, cb: () => void) => {
          mediaCallback = cb;
        },
        removeEventListener: vi.fn(),
      }));
      vi.stubGlobal("matchMedia", matchMediaMock);

      installSettingsRuntimeBridge();
      expect(document.documentElement.dataset.theme).toBe("light");

      matchMediaMock.mockImplementation((query: string) => ({
        matches: false,
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }));

      mediaCallback?.();
      expect(document.documentElement.dataset.theme).toBe("dark");

      vi.unstubAllGlobals();
    });
  });
});
