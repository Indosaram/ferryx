import { useCallback, useEffect, useRef, useState } from "react";
import type { MutableRefObject, ReactElement } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

/** Window event App dispatches to toggle copy mode in the pane owning `detail.sessionId`. */
export const TERMINAL_COPY_MODE_EVENT = "ferryx:terminal-copy-mode";

export type TerminalCopyModeEventDetail = { readonly sessionId: string };

export type CopyModeSelectionMode = "none" | "char" | "line" | "block";

/** A cell in screen (scrollback-inclusive) coordinates. */
export type CopyModePoint = { col: number; row: number };

export type CopyModeState = {
  cursor: CopyModePoint;
  anchor: CopyModePoint | null;
  mode: CopyModeSelectionMode;
};

export type CopyModeGridSize = { cols: number; rows: number };

/** Mirrors `NativeTerminalCursorReceipt` in src-tauri/src/ipc/native_terminal.rs. */
type CursorReceipt = {
  readonly col: number;
  readonly row: number;
  readonly screenRow: number;
  readonly viewportTopRow: number;
  readonly scrollbackPrunedRows: number;
};

type LineReceipt = { readonly text: string; readonly col: number; readonly row: number };

export type CopyModeRange = {
  type: "range";
  startCol: number;
  startRow: number;
  endCol: number;
  endRow: number;
  rectangle: boolean;
};

export type CopyModeKey = {
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
};

/** The selection a copy-mode state shows: the cursor cell alone until an anchor is set. */
export function copyModeSelectionRange(state: CopyModeState, cols: number): CopyModeRange {
  const { cursor, anchor, mode } = state;
  if (!anchor || mode === "none") {
    return { type: "range", startCol: cursor.col, startRow: cursor.row, endCol: cursor.col, endRow: cursor.row, rectangle: false };
  }
  const topRow = Math.min(anchor.row, cursor.row);
  const bottomRow = Math.max(anchor.row, cursor.row);
  if (mode === "line") {
    return { type: "range", startCol: 0, startRow: topRow, endCol: Math.max(0, cols - 1), endRow: bottomRow, rectangle: false };
  }
  if (mode === "block") {
    return {
      type: "range",
      startCol: Math.min(anchor.col, cursor.col),
      startRow: topRow,
      endCol: Math.max(anchor.col, cursor.col),
      endRow: bottomRow,
      rectangle: true,
    };
  }
  const anchorFirst = anchor.row < cursor.row || (anchor.row === cursor.row && anchor.col <= cursor.col);
  const start = anchorFirst ? anchor : cursor;
  const end = anchorFirst ? cursor : anchor;
  return { type: "range", startCol: start.col, startRow: start.row, endCol: end.col, endRow: end.row, rectangle: false };
}

type CharClass = 0 | 1 | 2;

function charClass(ch: string | undefined): CharClass {
  if (ch === undefined || /\s/.test(ch)) return 0;
  return /\w/.test(ch) ? 1 : 2;
}

/** vi `w` / `b` / `e` within one line of text; returns the new column. */
export function copyModeWordMotion(text: string, col: number, motion: "w" | "b" | "e"): number {
  const last = text.length - 1;
  if (last < 0) return col;
  let i = Math.min(Math.max(col, 0), last);
  if (motion === "w") {
    const start = charClass(text[i]);
    while (i <= last && start !== 0 && charClass(text[i]) === start) i += 1;
    while (i <= last && charClass(text[i]) === 0) i += 1;
    return i > last ? col : i;
  }
  if (motion === "e") {
    i += 1;
    while (i <= last && charClass(text[i]) === 0) i += 1;
    if (i > last) return col;
    const cls = charClass(text[i]);
    while (i + 1 <= last && charClass(text[i + 1]) === cls) i += 1;
    return i;
  }
  i -= 1;
  while (i >= 0 && charClass(text[i]) === 0) i -= 1;
  if (i < 0) return 0;
  const cls = charClass(text[i]);
  while (i - 1 >= 0 && charClass(text[i - 1]) === cls) i -= 1;
  return i;
}

function firstNonBlank(text: string): number {
  const index = text.search(/\S/);
  return index < 0 ? 0 : index;
}

function lastNonBlank(text: string): number {
  const trimmed = text.replace(/\s+$/, "");
  return Math.max(0, trimmed.length - 1);
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

export type TerminalCopyModeController = {
  readonly active: boolean;
  /** True from the moment copy mode is requested; key paths read it synchronously. */
  readonly activeRef: MutableRefObject<boolean>;
  readonly toggle: () => void;
  readonly exit: () => void;
  readonly handleKey: (key: CopyModeKey) => void;
  /** Called on terminal output while active: re-reads pruning without moving the viewport. */
  readonly onOutput: () => void;
};

/**
 * Keyboard copy mode for a native terminal pane. While active the pane forwards no key to the
 * PTY; the viewport stays frozen where it was (output keeps flowing, the cursor tracks the
 * scrollback pruning count) and motions move a cell cursor shown as a native selection.
 */
export function useTerminalCopyMode(
  sessionId: string | null,
  gridSizeRef: MutableRefObject<CopyModeGridSize>,
): TerminalCopyModeController {
  const [active, setActive] = useState(false);
  const activeRef = useRef(false);
  const stateRef = useRef<CopyModeState | null>(null);
  const prunedRef = useRef(0);
  /** Viewport top row copy mode holds; re-pinned on output since Row(n) at the bottom follows output. */
  const frozenTopRef = useRef(0);
  const bottomRowRef = useRef(0);
  const pendingGRef = useRef(false);
  const chainRef = useRef<Promise<void>>(Promise.resolve());
  const sessionRef = useRef(sessionId);
  sessionRef.current = sessionId;

  const enqueue = useCallback((task: () => Promise<void>) => {
    chainRef.current = chainRef.current.then(task).catch((error: unknown) => {
      console.error("Terminal copy mode command failed", error);
    });
  }, []);

  const readCursor = useCallback(async (sid: string): Promise<CursorReceipt | null> => {
    const receipt = await invoke<CursorReceipt | null>("cmd_native_terminal_cursor", { sessionId: sid });
    return receipt ?? null;
  }, []);

  /** Shifts stored rows by rows pruned since the last reading; drops a pruned anchor. */
  const syncReceipt = useCallback((receipt: CursorReceipt, state: CopyModeState): boolean => {
    const delta = Math.max(0, receipt.scrollbackPrunedRows - prunedRef.current);
    prunedRef.current = receipt.scrollbackPrunedRows;
    bottomRowRef.current = Math.max(0, receipt.screenRow - receipt.row + gridSizeRef.current.rows - 1);
    if (delta === 0) return false;
    frozenTopRef.current = Math.max(0, frozenTopRef.current - delta);
    state.cursor = { col: state.cursor.col, row: Math.max(0, state.cursor.row - delta) };
    if (state.anchor) {
      if (state.anchor.row - delta < 0) {
        state.anchor = null;
        state.mode = "none";
      } else {
        state.anchor = { col: state.anchor.col, row: state.anchor.row - delta };
      }
    }
    return true;
  }, [gridSizeRef]);

  const applySelection = useCallback(async (sid: string, state: CopyModeState) => {
    await invoke("cmd_native_terminal_select", {
      sessionId: sid,
      mode: copyModeSelectionRange(state, gridSizeRef.current.cols),
    });
  }, [gridSizeRef]);

  const finish = useCallback(async (sid: string | null) => {
    activeRef.current = false;
    stateRef.current = null;
    pendingGRef.current = false;
    setActive(false);
    if (!sid || !isTauri()) return;
    await invoke("cmd_native_terminal_select", { sessionId: sid, mode: { type: "none" } });
    await invoke("cmd_native_terminal_scroll", { sessionId: sid, behavior: { type: "bottom" } });
  }, []);

  const enter = useCallback(() => {
    const sid = sessionRef.current;
    if (!sid || !isTauri() || activeRef.current) return;
    activeRef.current = true;
    setActive(true);
    enqueue(async () => {
      if (!activeRef.current || sessionRef.current !== sid) return;
      let receipt: CursorReceipt | null = null;
      try {
        receipt = await readCursor(sid);
      } finally {
        // Never leave the keyboard captured when the pane cannot report a cursor.
        if (!receipt) {
          activeRef.current = false;
          setActive(false);
        }
      }
      if (!receipt) return;
      prunedRef.current = receipt.scrollbackPrunedRows;
      bottomRowRef.current = Math.max(0, receipt.screenRow - receipt.row + gridSizeRef.current.rows - 1);
      const state: CopyModeState = { cursor: { col: receipt.col, row: receipt.screenRow }, anchor: null, mode: "none" };
      stateRef.current = state;
      // Pin the viewport at its current top. At the bottom libghostty treats Row(top) as
      // "follow active output", so onOutput re-pins frozenTop once output has moved it.
      frozenTopRef.current = receipt.viewportTopRow;
      await invoke("cmd_native_terminal_scroll", { sessionId: sid, behavior: { type: "row", offset: receipt.viewportTopRow } });
      await applySelection(sid, state);
    });
  }, [applySelection, enqueue, gridSizeRef, readCursor]);

  const exit = useCallback(() => {
    if (!activeRef.current) return;
    const sid = sessionRef.current;
    enqueue(() => finish(sid));
  }, [enqueue, finish]);

  const toggle = useCallback(() => {
    if (activeRef.current) exit();
    else enter();
  }, [enter, exit]);

  const yank = useCallback(async (sid: string, state: CopyModeState) => {
    await applySelection(sid, state);
    // The native copy command writes the system clipboard on every platform.
    await invoke("cmd_native_terminal_copy_selection", { sessionId: sid });
    await finish(sid);
  }, [applySelection, finish]);

  const handleKey = useCallback((input: CopyModeKey) => {
    if (!activeRef.current) return;
    const key = { key: input.key, ctrlKey: input.ctrlKey, metaKey: input.metaKey, altKey: input.altKey };
    enqueue(async () => {
      const sid = sessionRef.current;
      const state = stateRef.current;
      if (!activeRef.current || !sid || !state) return;
      if (key.metaKey || key.altKey) return;
      if (key.key === "Escape" || (key.key === "q" && !key.ctrlKey)) {
        await finish(sid);
        return;
      }
      const receipt = await readCursor(sid);
      if (!receipt) return;
      syncReceipt(receipt, state);
      if (key.key === "y" && !key.ctrlKey) {
        await yank(sid, state);
        return;
      }

      const { cols, rows } = gridSizeRef.current;
      const viewTop = receipt.viewportTopRow;
      const lastRow = Math.max(bottomRowRef.current, 0);
      const half = Math.max(1, Math.floor(rows / 2));
      const cursor = { ...state.cursor };
      const toggleMode = (mode: CopyModeSelectionMode) => {
        if (state.mode === mode) {
          state.mode = "none";
          state.anchor = null;
        } else {
          if (!state.anchor) state.anchor = { ...state.cursor };
          state.mode = mode;
        }
      };
      const lineText = async (): Promise<string | null> => {
        const viewRow = cursor.row - viewTop;
        if (viewRow < 0 || viewRow >= rows) return null;
        const line = await invoke<LineReceipt | null>("cmd_native_terminal_line_at", {
          sessionId: sid,
          col: cursor.col,
          row: viewRow,
        });
        return line?.text ?? null;
      };

      const isG = key.key === "g" && !key.ctrlKey;
      if (!isG) pendingGRef.current = false;
      if (key.ctrlKey) {
        const lower = key.key.toLowerCase();
        if (lower === "u") cursor.row -= half;
        else if (lower === "d") cursor.row += half;
        else if (lower === "v") toggleMode("block");
        else return;
      } else {
        switch (key.key) {
          case "h":
          case "ArrowLeft":
            cursor.col -= 1;
            break;
          case "l":
          case "ArrowRight":
            cursor.col += 1;
            break;
          case "j":
          case "ArrowDown":
            cursor.row += 1;
            break;
          case "k":
          case "ArrowUp":
            cursor.row -= 1;
            break;
          case "0":
            cursor.col = 0;
            break;
          case "^": {
            const text = await lineText();
            if (text !== null) cursor.col = firstNonBlank(text);
            break;
          }
          case "$": {
            const text = await lineText();
            cursor.col = text !== null ? lastNonBlank(text) : cols - 1;
            break;
          }
          case "w":
          case "b":
          case "e": {
            const text = await lineText();
            if (text !== null) cursor.col = copyModeWordMotion(text, cursor.col, key.key as "w" | "b" | "e");
            break;
          }
          case "g":
            if (!pendingGRef.current) {
              pendingGRef.current = true;
              return;
            }
            pendingGRef.current = false;
            cursor.row = 0;
            break;
          case "G":
            cursor.row = lastRow;
            break;
          case "H":
            cursor.row = viewTop;
            break;
          case "M":
            cursor.row = viewTop + Math.floor((rows - 1) / 2);
            break;
          case "L":
            cursor.row = viewTop + rows - 1;
            break;
          case "v":
            toggleMode("char");
            break;
          case "V":
            toggleMode("line");
            break;
          default:
            return;
        }
      }

      cursor.row = clamp(cursor.row, 0, lastRow);
      cursor.col = clamp(cursor.col, 0, cols - 1);
      state.cursor = cursor;

      let nextTop = viewTop;
      if (cursor.row < viewTop) nextTop = cursor.row;
      else if (cursor.row > viewTop + rows - 1) nextTop = cursor.row - rows + 1;
      if (nextTop !== viewTop) {
        frozenTopRef.current = nextTop;
        await invoke("cmd_native_terminal_scroll", { sessionId: sid, behavior: { type: "row", offset: nextTop } });
      }
      await applySelection(sid, state);
    });
  }, [applySelection, enqueue, finish, gridSizeRef, readCursor, syncReceipt, yank]);

  const onOutput = useCallback(() => {
    if (!activeRef.current) return;
    enqueue(async () => {
      const sid = sessionRef.current;
      const state = stateRef.current;
      if (!activeRef.current || !sid || !state) return;
      const receipt = await readCursor(sid);
      if (!receipt) return;
      const shifted = syncReceipt(receipt, state);
      if (receipt.viewportTopRow !== frozenTopRef.current) {
        await invoke("cmd_native_terminal_scroll", {
          sessionId: sid,
          behavior: { type: "row", offset: frozenTopRef.current },
        });
      }
      if (shifted) {
        await applySelection(sid, state);
      }
    });
  }, [applySelection, enqueue, readCursor, syncReceipt]);

  // A rebind to another backend session ends copy mode; its rows mean nothing there.
  useEffect(() => () => {
    activeRef.current = false;
    stateRef.current = null;
    pendingGRef.current = false;
    setActive(false);
  }, [sessionId]);

  return { active, activeRef, toggle, exit, handleKey, onOutput };
}

export function TerminalCopyModeBadge(): ReactElement {
  return (
    <div
      data-testid="terminal-copy-mode-badge"
      aria-live="polite"
      className="pointer-events-none absolute top-2 left-2 z-40 rounded border border-primary/40 bg-card/95 px-1.5 py-0.5 font-mono text-[10px] font-semibold tracking-wide text-primary shadow-sm"
    >
      COPY
    </div>
  );
}

