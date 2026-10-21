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

type Terminal = {
  cursorCol: number;
  cursorRow: number;
  viewTop: number;
  /** Top row of the active screen (total_rows - rows). */
  bottomTop: number;
  /** True while the viewport follows active output, as libghostty does for Row(n >= bottomTop). */
  follow: boolean;
  pruned: number;
  lines: Record<number, string>;
};

const GRID: CopyModeGridSize = { cols: 10, rows: 6 };

/** A fake native terminal: live screen rows 100..105, cursor at screen row 102 col 2. */
function installTerminal(overrides: Partial<Terminal> = {}): Terminal {
  const term: Terminal = { cursorCol: 2, cursorRow: 102, viewTop: 100, bottomTop: 100, follow: true, pruned: 0, lines: {}, ...overrides };
  core.invoke.mockImplementation(async (cmd, args) => {
    switch (cmd) {
      case "cmd_native_terminal_cursor":
        return {
          col: term.cursorCol,
          row: term.cursorRow - term.bottomTop,
          screenRow: term.cursorRow,
          viewportTopRow: term.viewTop,
          scrollbackPrunedRows: term.pruned,
        };
      case "cmd_native_terminal_scroll":
        if (args?.behavior?.type === "row") {
          // libghostty-vt: Row(n) with n >= total_rows - rows means "follow active output".
          term.follow = args.behavior.offset >= term.bottomTop;
          term.viewTop = term.follow ? term.bottomTop : args.behavior.offset;
        }
        if (args?.behavior?.type === "bottom") {
          term.follow = true;
          term.viewTop = term.bottomTop;
        }
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

/** Appends output rows: the active screen and its cursor move down; a following viewport moves with them. */
function appendOutput(term: Terminal, rows: number) {
  term.bottomTop += rows;
  term.cursorRow += rows;
  if (term.follow) term.viewTop = term.bottomTop;
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

  it("viewport stays frozen when entered at the bottom while output streams", async () => {
    const term = installTerminal();
    const hook = await enterCopyMode();
    // Entry at the bottom pins Row(100), which the terminal treats as follow-active.
    expect(calls("cmd_native_terminal_scroll")).toEqual([{ sessionId: "sess-1", behavior: { type: "row", offset: 100 } }]);
    expect(term.follow).toBe(true);
    const screenRowBefore = 102 - term.viewTop;

    appendOutput(term, 3);
    expect(term.viewTop).toBe(103);
    act(() => hook.result.current.onOutput());
    await waitFor(() => expect(calls("cmd_native_terminal_scroll")).toHaveLength(2));
    expect(fromEnd(calls("cmd_native_terminal_scroll"), 1)).toEqual({ sessionId: "sess-1", behavior: { type: "row", offset: 100 } });
    expect(term.viewTop).toBe(100);
    expect(term.follow).toBe(false);

    // The copy cursor still sits on screen row 102, at the same row of the frozen viewport.
    expect(lastSelection()).toMatchObject({ startRow: 102, endRow: 102 });
    expect(lastSelection()!.startRow - term.viewTop).toBe(screenRowBefore);

    // Further output no longer drags the viewport, so no further re-pin is needed.
    appendOutput(term, 2);
    expect(term.viewTop).toBe(100);
    await press(hook, key("j"));
    expect(lastSelection()).toMatchObject({ startRow: 103, endRow: 103 });
    expect(calls("cmd_native_terminal_scroll")).toHaveLength(2);
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
