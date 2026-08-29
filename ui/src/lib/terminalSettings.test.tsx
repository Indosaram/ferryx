import { DEFAULT_TERMINAL_FONT_STACK } from "./tauri";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  getTerminalPreferences: vi.fn(),
  applyTerminalOverrides: vi.fn(),
}));

vi.mock("./tauri", () => ({
  DEFAULT_TERMINAL_FONT_STACK: 'MesloLGS NF, "Noto Sans KR", monospace',
  getTerminalPreferences: native.getTerminalPreferences,
  applyTerminalOverrides: native.applyTerminalOverrides,
}));

import {
  DEFAULT_TERMINAL_SETTINGS,
  TERMINAL_BACKGROUND_STORAGE_KEY,
  TERMINAL_SETTINGS_STORAGE_KEY,
  applyCachedTerminalBackground,
  loadTerminalSettings,
  resetTerminalPreferencesCache,
  resolveTerminalSettings,
  saveTerminalSettings,
  syncTerminalBackground,
  useTerminalSettings,
} from "./terminalSettings";

const ghosttyPreferences = {
  fontFamily: "Noto Sans KR",
  fontSize: 13,
  macosOptionAsAlt: true,
  cursorStyle: "block",
  theme: {
    background: "#282c34",
    foreground: "#ffffff",
    cursor: "#ffffff",
    cursorAccent: "#282c34",
    selectionBackground: "#52525299",
    black: "#1d1f21",
    red: "#cc6666",
    green: "#b5bd68",
    yellow: "#f0c674",
    blue: "#81a2be",
    magenta: "#b294bb",
    cyan: "#8abeb7",
    white: "#c5c8c6",
    brightBlack: "#666666",
    brightRed: "#d54e53",
    brightGreen: "#b9ca4a",
    brightYellow: "#e7c547",
    brightBlue: "#7aa6da",
    brightMagenta: "#c397d8",
    brightCyan: "#70c0b1",
    brightWhite: "#eaeaea",
    extendedAnsi: [],
  },
  source: "ghostty" as const,
  status: "imported" as const,
  sourcePath: "/Users/test/.config/ghostty/config",
};

beforeEach(() => {
  localStorage.clear();
  document.documentElement.style.removeProperty("--terminal");
  native.getTerminalPreferences.mockReset();
  native.getTerminalPreferences.mockResolvedValue(ghosttyPreferences);
  native.applyTerminalOverrides.mockReset();
  native.applyTerminalOverrides.mockResolvedValue(ghosttyPreferences);
  resetTerminalPreferencesCache();
});

describe("terminal settings", () => {
  it("loads safe local defaults and synchronizes persisted overrides", async () => {
    expect(loadTerminalSettings()).toEqual(DEFAULT_TERMINAL_SETTINGS);
    const { result } = renderHook(() => useTerminalSettings());
    await waitFor(() => expect(native.getTerminalPreferences).toHaveBeenCalled());

    act(() => result.current.updateSettings({ fontSize: 16, scrollback: 24_000, fontFamily: "JetBrains Mono" }));

    expect(result.current.settings).toMatchObject({ fontSize: 16, scrollback: 24_000, fontFamily: "JetBrains Mono" });
    expect(JSON.parse(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)!)).toMatchObject({
      fontSize: 16,
      scrollback: 24_000,
      fontFamily: "JetBrains Mono",
    });
  });

  it("resolves local override above Ghostty and Ghostty above fallback", () => {
    const imported = resolveTerminalSettings(DEFAULT_TERMINAL_SETTINGS, ghosttyPreferences);
    expect(imported).toMatchObject({
      fontFamily: "Noto Sans KR",
      macosOptionAsAlt: true,
      fontFamilySource: "ghostty",
      macosOptionAsAltSource: "ghostty",
    });

    const local = resolveTerminalSettings(
      { ...DEFAULT_TERMINAL_SETTINGS, fontFamily: "JetBrains Mono", macosOptionAsAlt: false },
      ghosttyPreferences,
    );
    expect(local).toMatchObject({
      fontFamily: "JetBrains Mono",
      macosOptionAsAlt: false,
      fontFamilySource: "local",
      macosOptionAsAltSource: "local",
    });

    const fallback = resolveTerminalSettings(DEFAULT_TERMINAL_SETTINGS, {
      fontFamily: DEFAULT_TERMINAL_FONT_STACK,
      fontSize: 13,
      macosOptionAsAlt: false,
      cursorStyle: "block",
      theme: ghosttyPreferences.theme,
      source: "defaults",
      status: "malformed",
      sourcePath: "/bad/config",
    });
    expect(fallback).toMatchObject({
      fontFamily: DEFAULT_TERMINAL_FONT_STACK,
      macosOptionAsAlt: false,
      fontFamilySource: "fallback",
      macosOptionAsAltSource: "fallback",
    });
  });

  it("restores the cached terminal background before terminal settings finish loading", () => {
    localStorage.setItem(TERMINAL_BACKGROUND_STORAGE_KEY, "#282c34");

    expect(applyCachedTerminalBackground()).toBe("#282c34");
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#282c34");
  });

  it("synchronizes the terminal surface token and cached first-paint background", () => {
    syncTerminalBackground("#282c34");

    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#282c34");
    expect(localStorage.getItem(TERMINAL_BACKGROUND_STORAGE_KEY)).toBe("#282c34");
  });

  it("updates the shared terminal surface after native preferences resolve", async () => {
    renderHook(() => useTerminalSettings());

    await waitFor(() => expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#282c34"));
    expect(localStorage.getItem(TERMINAL_BACKGROUND_STORAGE_KEY)).toBe("#282c34");
  });

  it("pushes local font overrides to the native runtime that owns the renderer", async () => {
    const { result } = renderHook(() => useTerminalSettings());
    await waitFor(() => expect(native.applyTerminalOverrides).toHaveBeenCalled());
    expect(native.applyTerminalOverrides).toHaveBeenLastCalledWith({
      fontFamily: null,
      fontSize: null,
      macosOptionAsAlt: null,
      shell: null,
    });

    act(() => result.current.updateSettings({ fontSize: 17 }));
    await waitFor(() =>
      expect(native.applyTerminalOverrides).toHaveBeenLastCalledWith({
        fontFamily: null,
        fontSize: 17,
        macosOptionAsAlt: null,
        shell: null,
      }),
    );

    act(() => result.current.updateSettings({ shell: "pwsh" }));
    await waitFor(() =>
      expect(native.applyTerminalOverrides).toHaveBeenLastCalledWith({
        fontFamily: null,
        fontSize: 17,
        macosOptionAsAlt: null,
        shell: "pwsh",
      }),
    );

    const callsAfterChange = native.applyTerminalOverrides.mock.calls.length;
    act(() => result.current.updateSettings({ fontSize: 17 }));
    await waitFor(() => expect(result.current.settings.fontSize).toBe(17));
    expect(native.applyTerminalOverrides).toHaveBeenCalledTimes(callsAfterChange);
  });

  it("normalizes empty and whitespace shell strings to null", () => {
    act(() => {
      const saved = saveTerminalSettings({ shell: "   " });
      expect(saved.shell).toBeNull();
    });
    expect(loadTerminalSettings().shell).toBeNull();
  });

  it("reports a font-size-only override as a local override", () => {
    const resolved = resolveTerminalSettings(
      { ...DEFAULT_TERMINAL_SETTINGS, fontSize: 17 },
      ghosttyPreferences,
    );

    expect(resolved).toMatchObject({
      fontSize: 17,
      fontSizeSource: "local",
      fontFamilySource: "ghostty",
      macosOptionAsAltSource: "ghostty",
    });
  });
});
