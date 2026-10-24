import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useMemo, useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { shortcutLabel, useShortcuts } from "../lib/shortcuts";
import { useTerminalCopyMode, type CopyModeGridSize } from "./TerminalCopyMode";

const core = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => core.invoke(cmd, args),
  isTauri: () => core.isTauri(),
}));

/** A native terminal with a live screen at rows 100..105 and the cursor at row 102, col 2. */
function installTerminal() {
  core.invoke.mockImplementation(async (cmd) => {
    switch (cmd) {
      case "cmd_native_terminal_cursor":
        return { col: 2, row: 2, screenRow: 102, viewportTopRow: 100, scrollbackPrunedRows: 0 };
      case "cmd_native_terminal_copy_selection":
        return "copied";
      default:
        return undefined;
    }
  });
}

function calls(cmd: string) {
  return core.invoke.mock.calls.filter(([name]) => name === cmd).map(([, args]) => args);
}

/** The pane's copy mode, entered through the real shortcut router the way App wires it. */
function useCopyModeViaShortcut(isMac: boolean) {
  const grid = useRef<CopyModeGridSize>({ cols: 10, rows: 6 });
  const copyMode = useTerminalCopyMode("sess-1", grid);
  const handlers = useMemo(() => ({ "terminal.copyMode": copyMode.toggle }), [copyMode.toggle]);
  useShortcuts(handlers, { isMac });
  return copyMode;
}

const key = (value: string) => ({ key: value, ctrlKey: false, metaKey: false, altKey: false });

beforeEach(() => {
  core.invoke.mockReset();
  core.isTauri.mockReturnValue(true);
});

afterEach(() => cleanup());

describe("keyboard copy mode", () => {
  it.each([
    ["non-Mac Ctrl+Shift+Space", false, { ctrlKey: true }],
    ["Mac Cmd+Shift+Space", true, { metaKey: true }],
  ] as const)("%s enters, v/l/j select, y copies and exits", async (_label, isMac, mods) => {
    installTerminal();
    expect(shortcutLabel("terminal.copyMode", isMac)).toBe(isMac ? "⌘⇧Space" : "Ctrl+Shift+Space");
    const hook = renderHook(() => useCopyModeViaShortcut(isMac));

    const chord = new KeyboardEvent("keydown", { key: " ", code: "Space", shiftKey: true, bubbles: true, cancelable: true, ...mods });
    act(() => {
      window.dispatchEvent(chord);
    });
    expect(chord.defaultPrevented).toBe(true);
    expect(hook.result.current.active).toBe(true);
    await waitFor(() => expect(calls("cmd_native_terminal_select")).toHaveLength(1));

    act(() => {
      for (const k of ["v", "l", "l", "j"]) hook.result.current.handleKey(key(k));
    });
    await waitFor(() => expect(calls("cmd_native_terminal_select")).toHaveLength(5));
    expect(calls("cmd_native_terminal_select").at(-1)?.mode).toEqual({
      type: "range", startCol: 2, startRow: 102, endCol: 4, endRow: 103, rectangle: false,
    });

    act(() => hook.result.current.handleKey(key("y")));
    await waitFor(() => expect(hook.result.current.active).toBe(false));
    expect(calls("cmd_native_terminal_copy_selection")).toEqual([{ sessionId: "sess-1" }]);
    expect(calls("cmd_native_terminal_select").at(-1)?.mode).toEqual({ type: "none" });
  });

  it.each(["q", "Escape"])("%s exits without copying", async (exitKey) => {
    installTerminal();
    const hook = renderHook(() => useCopyModeViaShortcut(false));
    act(() => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: " ", code: "Space", ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true }));
    });
    await waitFor(() => expect(calls("cmd_native_terminal_select")).toHaveLength(1));

    act(() => hook.result.current.handleKey(key("h")));
    await waitFor(() => expect(calls("cmd_native_terminal_select")).toHaveLength(2));
    act(() => hook.result.current.handleKey(key(exitKey)));
    await waitFor(() => expect(hook.result.current.active).toBe(false));
    expect(calls("cmd_native_terminal_copy_selection")).toEqual([]);
    expect(calls("cmd_native_terminal_select").at(-1)?.mode).toEqual({ type: "none" });
    expect(calls("cmd_native_terminal_scroll").at(-1)).toEqual({ sessionId: "sess-1", behavior: { type: "bottom" } });
  });
});
