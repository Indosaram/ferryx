import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { copyModeSelectionRange, copyModeWordMotion, useTerminalCopyMode, type CopyModeGridSize } from "./TerminalCopyMode";

const core = vi.hoisted(() => ({
  invoke: vi.fn<(cmd: string, args?: any) => Promise<any>>(async () => undefined),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => core.invoke(cmd, args),
  isTauri: () => core.isTauri(),
}));

vi.mock("../lib/shortcuts", () => ({ isMacShortcutPlatform: () => true }));

type Terminal = { cursorCol: number; cursorRow: number; viewTop: number; pruned: number; lines: Record<number, string> };

const GRID: CopyModeGridSize = { cols: 10, rows: 6 };

/** A fake native terminal: live screen rows 100..105, cursor at screen row 102 col 2. */
function installTerminal(overrides: Partial<Terminal> = {}): Terminal {
  const term: Terminal = { cursorCol: 2, cursorRow: 102, viewTop: 100, pruned: 0, lines: {}, ...overrides };
  core.invoke.mockImplementation(async (cmd, args) => {
    switch (cmd) {
      case "cmd_native_terminal_cursor":
        return {
          col: term.cursorCol,
          row: term.cursorRow - 100,
          screenRow: term.cursorRow,
          viewportTopRow: term.viewTop,
          scrollbackPrunedRows: term.pruned,
        };
      case "cmd_native_terminal_scroll":
        if (args?.behavior?.type === "row") term.viewTop = args.behavior.offset;
        if (args?.behavior?.type === "bottom") term.viewTop = 100;
        return undefined;
      case "cmd_native_terminal_line_at":
        return { text: term.lines[term.viewTop + args.row] ?? "", col: args.col, row: args.row };
      case "cmd_native_terminal_copy_selection":
        return "copied";
      default:
        return undefined;
    }
  });
  return term;
}

function calls(cmd: string) {
  return core.invoke.mock.calls.filter(([name]) => name === cmd).map(([, args]) => args);
}

function lastSelection() {
  return fromEnd(calls("cmd_native_terminal_select"), 1)?.mode;
}

function fromEnd<T>(items: readonly T[], n: number): T | undefined {
  return items[items.length - n];
}

const key = (value: string, mods: { ctrlKey?: boolean } = {}) => ({ key: value, ctrlKey: false, metaKey: false, altKey: false, ...mods });

async function enterCopyMode() {
  const grid = { current: { ...GRID } };
  const hook = renderHook(() => useTerminalCopyMode("sess-1", grid));
  act(() => hook.result.current.toggle());
  await waitFor(() => expect(calls("cmd_native_terminal_select")).toHaveLength(1));
  return hook;
}

async function press(hook: Awaited<ReturnType<typeof enterCopyMode>>, ...keys: ReturnType<typeof key>[]) {
  const before = calls("cmd_native_terminal_select").length;
  const selecting = keys.filter((k) => !["y", "Escape", "q"].includes(k.key) && !(k.key === "g" && !k.ctrlKey)).length;
  act(() => {
    for (const k of keys) hook.result.current.handleKey(k);
  });
  if (selecting > 0) {
    await waitFor(() => expect(calls("cmd_native_terminal_select").length).toBeGreaterThanOrEqual(before + selecting));
  }
}

describe("useTerminalCopyMode", () => {
  beforeEach(() => {
    core.invoke.mockReset();
    core.isTauri.mockReturnValue(true);
  });

  it("j/k move the cursor and scroll at the edge", async () => {
    const term = installTerminal({ cursorRow: 100 });
    const hook = await enterCopyMode();
    expect(hook.result.current.active).toBe(true);
    // Entry pins the viewport where it is instead of following output.
    expect(calls("cmd_native_terminal_scroll")[0]).toEqual({ sessionId: "sess-1", behavior: { type: "row", offset: 100 } });
    expect(lastSelection()).toEqual({ type: "range", startCol: 2, startRow: 100, endCol: 2, endRow: 100, rectangle: false });

    await press(hook, key("j"));
    expect(lastSelection()).toMatchObject({ startRow: 101, endRow: 101 });
    expect(calls("cmd_native_terminal_scroll")).toHaveLength(1);

    await press(hook, key("k"), key("k"));
    // Row 99 is above the viewport top (100): the viewport follows the cursor up one row.
    expect(lastSelection()).toMatchObject({ startRow: 99, endRow: 99 });
    expect(fromEnd(calls("cmd_native_terminal_scroll"), 1)).toEqual({ sessionId: "sess-1", behavior: { type: "row", offset: 99 } });
    expect(term.viewTop).toBe(99);
  });

  it("v then y copies the range and exits", async () => {
    installTerminal();
    const hook = await enterCopyMode();
    await press(hook, key("v"), key("l"), key("l"), key("j"));
    expect(lastSelection()).toEqual({ type: "range", startCol: 2, startRow: 102, endCol: 4, endRow: 103, rectangle: false });

    act(() => hook.result.current.handleKey(key("y")));
    await waitFor(() => expect(hook.result.current.active).toBe(false));
    expect(calls("cmd_native_terminal_copy_selection")).toEqual([{ sessionId: "sess-1" }]);
    const selectCalls = calls("cmd_native_terminal_select");
    // The yank re-applies the range, then exit clears it and returns to the bottom.
    expect(fromEnd(selectCalls, 2)?.mode).toEqual({ type: "range", startCol: 2, startRow: 102, endCol: 4, endRow: 103, rectangle: false });
    expect(fromEnd(selectCalls, 1)?.mode).toEqual({ type: "none" });
    expect(fromEnd(calls("cmd_native_terminal_scroll"), 1)).toEqual({ sessionId: "sess-1", behavior: { type: "bottom" } });
  });

  it("V selects whole lines", async () => {
    installTerminal();
    const hook = await enterCopyMode();
    await press(hook, key("V"), key("k"));
    expect(lastSelection()).toEqual({ type: "range", startCol: 0, startRow: 101, endCol: 9, endRow: 102, rectangle: false });
  });

  it("Ctrl-v selects a block", async () => {
    installTerminal({ cursorCol: 5 });
    const hook = await enterCopyMode();
    await press(hook, key("v", { ctrlKey: true }), key("h"), key("h"), key("j"));
    expect(lastSelection()).toEqual({ type: "range", startCol: 3, startRow: 102, endCol: 5, endRow: 103, rectangle: true });
  });

  it("Esc exits without copying", async () => {
    installTerminal();
    const hook = await enterCopyMode();
    await press(hook, key("v"), key("l"));
    act(() => hook.result.current.handleKey(key("Escape")));
    await waitFor(() => expect(hook.result.current.active).toBe(false));
    expect(calls("cmd_native_terminal_copy_selection")).toEqual([]);
    expect(lastSelection()).toEqual({ type: "none" });
    expect(fromEnd(calls("cmd_native_terminal_scroll"), 1)).toEqual({ sessionId: "sess-1", behavior: { type: "bottom" } });
  });

  it("shifts the cursor and anchor by rows pruned from scrollback", async () => {
    const term = installTerminal();
    const hook = await enterCopyMode();
    await press(hook, key("v"));
    term.pruned = 1;
    term.viewTop = 99;
    await press(hook, key("l"));
    expect(lastSelection()).toEqual({ type: "range", startCol: 2, startRow: 101, endCol: 3, endRow: 101, rectangle: false });
  });
});

describe("copy mode pure helpers", () => {
  it("orders a char selection whose anchor is after the cursor", () => {
    expect(copyModeSelectionRange({ cursor: { col: 1, row: 3 }, anchor: { col: 4, row: 5 }, mode: "char" }, 10)).toEqual({
      type: "range", startCol: 1, startRow: 3, endCol: 4, endRow: 5, rectangle: false,
    });
  });

  it("moves by vi words within a line", () => {
    const text = "foo bar.baz  qux";
    expect(copyModeWordMotion(text, 0, "w")).toBe(4);
    expect(copyModeWordMotion(text, 4, "w")).toBe(7);
    expect(copyModeWordMotion(text, 4, "e")).toBe(6);
    expect(copyModeWordMotion(text, 13, "b")).toBe(8);
  });
});
