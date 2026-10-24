import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  setTerminalAppearance: vi.fn(),
  themeListeners: [] as Array<(event: { payload: string }) => void>,
}));

vi.mock("../../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../lib/tauri")>();
  return { ...actual, setTerminalAppearance: native.setTerminalAppearance };
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onThemeChanged: async (callback: (event: { payload: string }) => void) => {
      native.themeListeners.push(callback);
      return () => {
        const index = native.themeListeners.indexOf(callback);
        if (index >= 0) native.themeListeners.splice(index, 1);
      };
    },
  }),
}));

import {
  _resetNativeOsThemeForTest,
  loadAppearanceSettings,
  saveAppearanceSettings,
  useApplyAppearanceSettings,
} from "../../lib/appearanceSettings";
import {
  _resetSettingsRuntimeBridgeForTest,
  installSettingsRuntimeBridge,
} from "../../lib/settingsRuntimeBridge";
import { AppearanceSection } from "./AppearanceSection";

type MediaStub = { matches: boolean; fire: () => void };

function stubColorScheme(prefersLight: boolean): MediaStub {
  const listeners: Array<() => void> = [];
  const stub: MediaStub = { matches: prefersLight, fire: () => listeners.forEach((listener) => listener()) };
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

/** What App renders: the hook that applies the theme, plus the settings section. */
function Harness() {
  useApplyAppearanceSettings();
  return <AppearanceSection />;
}

function fireOsTheme(payload: "light" | "dark") {
  act(() => {
    for (const listener of [...native.themeListeners]) listener({ payload });
  });
}

describe("AppearanceSection system theme", () => {
  beforeEach(() => {
    localStorage.clear();
    native.themeListeners.length = 0;
    native.setTerminalAppearance.mockReset();
    native.setTerminalAppearance.mockResolvedValue(null);
    _resetNativeOsThemeForTest();
    _resetSettingsRuntimeBridgeForTest();
    document.documentElement.removeAttribute("data-theme");
    (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    cleanup();
    _resetSettingsRuntimeBridgeForTest();
    delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.unstubAllGlobals();
  });

  it("offers a System option that persists and follows prefers-color-scheme", async () => {
    const media = stubColorScheme(true);
    render(<Harness />);

    const trigger = screen.getByRole("combobox", { name: "Theme mode" });
    fireEvent.pointerDown(trigger, { pointerId: 1, button: 0 });
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("option", { name: "System (follow OS)" }));

    await waitFor(() => expect(loadAppearanceSettings().theme).toBe("system"));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.dataset.themePreference).toBe("system");
    expect(screen.getByText(/Currently Light/)).toBeInTheDocument();

    media.matches = false;
    act(() => media.fire());
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByText(/Currently Dark/)).toBeInTheDocument();
  });

  it("follows the native OS theme event even when the media query does not change", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "system" });
    render(<Harness />);
    expect(document.documentElement.dataset.theme).toBe("dark");
    await waitFor(() => expect(native.themeListeners.length).toBeGreaterThan(0));

    fireOsTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(screen.getByText(/Currently Light/)).toBeInTheDocument();

    fireOsTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(screen.getByText(/Currently Dark/)).toBeInTheDocument();
  });

  it("switches the terminal to its light palette when the OS turns light", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "system" });
    installSettingsRuntimeBridge();
    expect(native.setTerminalAppearance).toHaveBeenLastCalledWith("dark");
    await waitFor(() => expect(native.themeListeners.length).toBeGreaterThan(0));

    fireOsTheme("light");
    expect(native.setTerminalAppearance).toHaveBeenLastCalledWith("light");
  });

  it("ignores OS theme events while a fixed theme is chosen", async () => {
    stubColorScheme(false);
    saveAppearanceSettings({ theme: "charcoal" });
    installSettingsRuntimeBridge();
    render(<Harness />);
    await waitFor(() => expect(native.themeListeners.length).toBeGreaterThan(0));

    fireOsTheme("light");
    expect(document.documentElement.dataset.theme).toBe("charcoal");
    expect(native.setTerminalAppearance).not.toHaveBeenCalledWith("light");
  });
});
