import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  getTerminalPreferences: vi.fn(),
}));

vi.mock("./tauri", () => ({ getTerminalPreferences: native.getTerminalPreferences }));

import {
  DEFAULT_TERMINAL_SETTINGS,
  TERMINAL_SETTINGS_STORAGE_KEY,
  applyTerminalSettings,
  loadTerminalSettings,
  resolveTerminalSettings,
  useTerminalSettings,
} from "./terminalSettings";

const ghosttyPreferences = {
  fontFamily: "Noto Sans KR",
  macosOptionAsAlt: true,
  source: "ghostty" as const,
  status: "imported" as const,
  sourcePath: "/Users/test/.config/ghostty/config",
};

beforeEach(() => {
  localStorage.clear();
  native.getTerminalPreferences.mockReset();
  native.getTerminalPreferences.mockResolvedValue(ghosttyPreferences);
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
      fontFamily: "monospace",
      macosOptionAsAlt: false,
      source: "defaults",
      status: "malformed",
      sourcePath: "/bad/config",
    });
    expect(fallback).toMatchObject({
      fontFamily: "monospace",
      macosOptionAsAlt: false,
      fontFamilySource: "fallback",
      macosOptionAsAltSource: "fallback",
    });
  });

  it("applies effective font, option-as-alt, font size and scrollback to live xterm options", () => {
    const terminal = {
      options: { fontFamily: "monospace", macOptionIsMeta: false, fontSize: 13, scrollback: 10_000 },
    };
    const settings = resolveTerminalSettings(DEFAULT_TERMINAL_SETTINGS, ghosttyPreferences);
    applyTerminalSettings(terminal, settings);
    expect(terminal.options).toEqual({
      fontFamily: "Noto Sans KR",
      macOptionIsMeta: true,
      fontSize: 13,
      scrollback: 10_000,
    });
  });
});