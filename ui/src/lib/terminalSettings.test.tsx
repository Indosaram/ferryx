import { DEFAULT_TERMINAL_FONT_STACK } from "./tauri";
import { act, renderHook, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

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
  fetchCachedNativePreferences,
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

  it("dedupes native preference fetches across concurrently mounted panes via the shared cache", async () => {
    await act(async () => {
      renderHook(() => useTerminalSettings());
      renderHook(() => useTerminalSettings());
    });
    expect(native.getTerminalPreferences).toHaveBeenCalledTimes(1);
  });

  it("still forces a fresh native fetch when a refresh is explicitly requested", async () => {
    const { result } = renderHook(() => useTerminalSettings());
    const initial = native.getTerminalPreferences.mock.calls.length;
    await act(async () => {
      await result.current.refreshNativePreferences();
    });
    expect(native.getTerminalPreferences.mock.calls.length).toBe(initial + 1);
  });

  it("does not flash the fallback background onto the shared surface when a later pane mounts", async () => {
    const distinct = {
      ...ghosttyPreferences,
      theme: { ...ghosttyPreferences.theme, background: "#123456" },
    };
    const pending = deferred<typeof distinct>();
    native.getTerminalPreferences.mockReset();
    native.getTerminalPreferences.mockReturnValue(pending.promise);

    renderHook(() => useTerminalSettings());
    await act(async () => {
      pending.resolve(distinct);
      await pending.promise;
    });
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#123456");

    renderHook(() => useTerminalSettings());
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#123456");
    await act(async () => {});
  });

  it("keeps the forced-refresh result when an earlier stale fetch resolves afterward", async () => {
    const older = { ...ghosttyPreferences, theme: { ...ghosttyPreferences.theme, background: "#aaaaaa" } };
    const newer = { ...ghosttyPreferences, theme: { ...ghosttyPreferences.theme, background: "#bbbbbb" } };
    const first = deferred<typeof older>();
    const second = deferred<typeof newer>();
    native.getTerminalPreferences.mockReset();
    native.getTerminalPreferences
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const pane = renderHook(() => useTerminalSettings());
    const sibling = renderHook(() => useTerminalSettings());
    await act(async () => {
      const forced = pane.result.current.refreshNativePreferences();
      second.resolve(newer);
      await forced;
    });
    await act(async () => {
      first.resolve(older);
      await first.promise;
    });
    expect(pane.result.current.nativePreferences.theme.background).toBe("#bbbbbb");
    expect(sibling.result.current.nativePreferences.theme.background).toBe("#bbbbbb");
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#bbbbbb");

    const late = renderHook(() => useTerminalSettings());
    expect(late.result.current.nativePreferences.theme.background).toBe("#bbbbbb");
    await act(async () => {});
  });

  it("preserves the cached startup background until native preferences resolve", async () => {
    const pending = deferred<typeof ghosttyPreferences>();
    native.getTerminalPreferences.mockReturnValue(pending.promise);
    syncTerminalBackground("#123456");

    renderHook(() => useTerminalSettings());
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe("#123456");
    await act(async () => {
      pending.resolve(ghosttyPreferences);
    });
    expect(document.documentElement.style.getPropertyValue("--terminal")).toBe(ghosttyPreferences.theme.background);
  });

  it("does not publish a fetch that resolves after the cache was reset", async () => {
    const stale = { ...ghosttyPreferences, theme: { ...ghosttyPreferences.theme, background: "#cccccc" } };
    const first = deferred<typeof stale>();
    native.getTerminalPreferences.mockReset();
    native.getTerminalPreferences
      .mockReturnValueOnce(first.promise)
      .mockReturnValue(new Promise<typeof stale>(() => {}));

    void fetchCachedNativePreferences(false);
    resetTerminalPreferencesCache();
    await act(async () => {
      first.resolve(stale);
      await first.promise;
    });

    const pane = renderHook(() => useTerminalSettings());
    expect(pane.result.current.nativePreferences.theme.background).toBe("#282c34");
  });

  it("dispatches a single settings event per update under StrictMode", () => {
    const events = vi.fn();
    window.addEventListener("orca:terminal-settings", events);
    const { result } = renderHook(() => useTerminalSettings(), { wrapper: StrictMode });
    act(() => result.current.updateSettings({ fontSize: 18 }));
    window.removeEventListener("orca:terminal-settings", events);
    expect(events).toHaveBeenCalledTimes(1);
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
