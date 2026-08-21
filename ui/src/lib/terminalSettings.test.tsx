import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_TERMINAL_SETTINGS,
  TERMINAL_SETTINGS_STORAGE_KEY,
  applyTerminalSettings,
  loadTerminalSettings,
  useTerminalSettings,
} from "./terminalSettings";

beforeEach(() => localStorage.clear());

describe("terminal settings", () => {
  it("loads defaults, persists updates, and synchronizes hook state", () => {
    expect(loadTerminalSettings()).toEqual(DEFAULT_TERMINAL_SETTINGS);
    const { result } = renderHook(() => useTerminalSettings());

    act(() => result.current.updateSettings({ fontSize: 16, scrollback: 24_000 }));

    expect(result.current.settings).toEqual({ fontSize: 16, scrollback: 24_000 });
    expect(JSON.parse(localStorage.getItem(TERMINAL_SETTINGS_STORAGE_KEY)!)).toEqual({ fontSize: 16, scrollback: 24_000 });
  });

  it("applies persisted values to live xterm options", () => {
    const terminal = { options: { fontSize: 13, scrollback: 10_000 } };
    applyTerminalSettings(terminal, { fontSize: 18, scrollback: 50_000 });
    expect(terminal.options).toEqual({ fontSize: 18, scrollback: 50_000 });
  });
});
