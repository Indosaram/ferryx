import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createElement } from "react";

import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const terminalIpc = vi.hoisted(() => ({
  setTerminalAppearance: vi.fn(),
}));

vi.mock("./tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./tauri")>();
  return { ...actual, setTerminalAppearance: terminalIpc.setTerminalAppearance };
});

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
import { TERMINAL_BACKGROUND_STORAGE_KEY } from "./terminalSettings";

beforeEach(() => {
  localStorage.clear();
  terminalIpc.setTerminalAppearance.mockReset();
  terminalIpc.setTerminalAppearance.mockResolvedValue(null);
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

    describe("terminal appearance push (desktop shell)", () => {
      type MediaStub = { matches: boolean; fire: () => void };

      function stubColorScheme(prefersLight: boolean): MediaStub {
        const listeners: Array<() => void> = [];
        const stub: MediaStub = {
          matches: prefersLight,
          fire: () => listeners.forEach((listener) => listener()),
        };
        vi.stubGlobal("matchMedia", vi.fn().mockImplementation((query: string) => ({
          get matches() {
            return stub.matches;
          },
          media: query,
          addEventListener: (_event: string, cb: () => void) => listeners.push(cb),
          removeEventListener: (_event: string, cb: () => void) => {
            const index = listeners.indexOf(cb);
            if (index >= 0) listeners.splice(index, 1);
          },
        })));
        return stub;
      }

      beforeEach(() => {
        (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
        document.documentElement.style.removeProperty("--terminal");
        return () => {
          delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
          _resetSettingsRuntimeBridgeForTest();
          vi.unstubAllGlobals();
        };
      });

      it("pushes light to the terminal when the OS flips to light under system theme", async () => {
        saveAppearanceSettings({ theme: "system" });
        const media = stubColorScheme(false);
        installSettingsRuntimeBridge();
        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("dark");

        terminalIpc.setTerminalAppearance.mockResolvedValue({ theme: { background: "#fafafa" } });
        media.matches = true;
        media.fire();

        expect(document.documentElement.dataset.theme).toBe("light");
        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("light");
        await vi.waitFor(() => {
          expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#fafafa");
        });
        expect(localStorage.getItem(TERMINAL_BACKGROUND_STORAGE_KEY)).toBe("#fafafa");
      });

      it("does not push when theme is dark and the OS flips", () => {
        saveAppearanceSettings({ theme: "dark" });
        const media = stubColorScheme(false);
        installSettingsRuntimeBridge();
        expect(terminalIpc.setTerminalAppearance).toHaveBeenCalledTimes(1);
        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("dark");

        media.matches = true;
        media.fire();

        expect(terminalIpc.setTerminalAppearance).toHaveBeenCalledTimes(1);
      });

      it("charcoal resolves to dark", () => {
        saveAppearanceSettings({ theme: "light" });
        stubColorScheme(true);
        installSettingsRuntimeBridge();
        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("light");

        saveAppearanceSettings({ theme: "charcoal" });

        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("dark");
      });

      it("keeps the UI working when the terminal appearance push rejects", async () => {
        const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
        terminalIpc.setTerminalAppearance.mockRejectedValue(new Error("daemon unavailable"));
        saveAppearanceSettings({ theme: "system" });
        const media = stubColorScheme(false);
        installSettingsRuntimeBridge();
        await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(1));

        media.matches = true;
        media.fire();

        expect(document.documentElement.dataset.theme).toBe("light");
        expect(terminalIpc.setTerminalAppearance).toHaveBeenLastCalledWith("light");
        await vi.waitFor(() => expect(warn).toHaveBeenCalledTimes(2));
        warn.mockRestore();
      });

      it("never pushes outside the desktop shell", () => {
        delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
        saveAppearanceSettings({ theme: "system" });
        const media = stubColorScheme(false);
        installSettingsRuntimeBridge();
        media.matches = true;
        media.fire();
        saveAppearanceSettings({ theme: "light" });

        expect(terminalIpc.setTerminalAppearance).not.toHaveBeenCalled();
      });
    });
  });
});
