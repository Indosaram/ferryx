import type { CSSProperties } from "react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { MobileKeyDock } from "../components/MobileKeyDock";
import { useTerminalSettings } from "../lib/terminalSettings";
import {
  applyGridFrame,
  decodeGridAttrs,
  estimateCellWidth,
  parseGridFrame,
  type GridColor,
  type GridCursor,
  type GridRun,
  type TerminalGridState,
} from "./terminalGridProtocol";

type RemoteTerminalProps = {
  readonly sessionId: string;
  readonly token: string;
  readonly title?: string;
  readonly onBack?: () => void;
  readonly embedded?: boolean;
  readonly activeTabId?: string | null;
  readonly onSwipeNextTab?: () => void;
  readonly onSwipePreviousTab?: () => void;
  readonly onSocketLifecycle?: (
    sessionId: string,
    state: "open" | "closed",
  ) => void;
};

export const MIN_TERMINAL_FONT_SIZE = 10;
export const MAX_TERMINAL_FONT_SIZE = 36;
export const SWIPE_THRESHOLD_PX = 40;

export const MIN_FONT_SIZE = MIN_TERMINAL_FONT_SIZE;
export const MAX_FONT_SIZE = MAX_TERMINAL_FONT_SIZE;

export function clampTerminalFontSize(fontSize: number): number {
  return Math.min(MAX_TERMINAL_FONT_SIZE, Math.max(MIN_TERMINAL_FONT_SIZE, Math.round(fontSize)));
}

type CellMetrics = {
  readonly width: number;
  readonly height: number;
};

type GridGeometry = {
  readonly cols: number;
  readonly rows: number;
};

type SocketRequest = {
  readonly sessionId: string;
  readonly token: string;
  readonly geometry: GridGeometry;
};

const MAX_GRID_COLS = 512;
const MAX_GRID_ROWS = 256;

const KEY_SEQUENCES = {
  tab: "\t",
  esc: "\u001b",
  up: "\u001b[A",
  down: "\u001b[B",
  left: "\u001b[D",
  right: "\u001b[C",
  pageup: "\u001b[5~",
  pagedown: "\u001b[6~",
  home: "\u001b[H",
  end: "\u001b[F",
  "ctrl-d": "\u0004",
  "ctrl-z": "\u001a",
  backspace: "\u007f",
  delete: "\u001b[3~",
} as const;

const BROWSER_KEY_NAMES: Record<string, keyof typeof KEY_SEQUENCES> = {
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  PageUp: "pageup",
  PageDown: "pagedown",
  Home: "home",
  End: "end",
  Escape: "esc",
  Tab: "tab",
  Backspace: "backspace",
  Delete: "delete",
};

const MODIFIED_NAVIGATION_FINALS: Record<string, string> = {
  ArrowUp: "A",
  ArrowDown: "B",
  ArrowRight: "C",
  ArrowLeft: "D",
  Home: "H",
  End: "F",
};

const DOCK_NAVIGATION_FINALS: Record<string, string> = {
  up: "A",
  down: "B",
  right: "C",
  left: "D",
  home: "H",
  end: "F",
};

function modifiedNavigationSequence(key: string, ctrlKey: boolean, altKey: boolean): string | undefined {
  const final = MODIFIED_NAVIGATION_FINALS[key];
  if (!final || (!ctrlKey && !altKey)) return undefined;
  const modifier = 1 + (altKey ? 2 : 0) + (ctrlKey ? 4 : 0);
  return `\u001b[1;${modifier}${final}`;
}

function modifiedDockNavigationSequence(key: string): string | undefined {
  const [modifier, direction] = key.split("-", 2);
  const final = direction ? DOCK_NAVIGATION_FINALS[direction] : undefined;
  if (!final) return undefined;
  const modifierCode = modifier === "ctrl" ? 5 : modifier === "alt" ? 3 : undefined;
  return modifierCode ? `\u001b[1;${modifierCode}${final}` : undefined;
}

function terminalSocketUrl(sessionId: string, token: string, geometry: GridGeometry): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/v1/terminal/${sessionId}?token=${encodeURIComponent(token)}&render=grid&cols=${geometry.cols}&rows=${geometry.rows}`;
}

function geometriesEqual(left: GridGeometry | null, right: GridGeometry): boolean {
  return left?.cols === right.cols && left.rows === right.rows;
}

function socketRequestMatches(request: SocketRequest | null, sessionId: string, token: string): boolean {
  return request?.sessionId === sessionId && request.token === token;
}

function colorCss(color: GridColor): string {
  return `rgb(${color[0]}, ${color[1]}, ${color[2]})`;
}

function runStyle(
  run: GridRun,
  foreground: string,
  background: string,
  cellWidthPx: number | null,
): CSSProperties {
  const attrs = decodeGridAttrs(run.attrs);
  const explicitForeground = run.fg ? colorCss(run.fg) : null;
  const explicitBackground = run.bg ? colorCss(run.bg) : null;
  const style: CSSProperties = {
    color: attrs.inverse ? (explicitBackground ?? background) : (explicitForeground ?? undefined),
    backgroundColor: attrs.inverse ? (explicitForeground ?? foreground) : (explicitBackground ?? undefined),
    fontWeight: attrs.bold ? 700 : undefined,
    fontStyle: attrs.italic ? "italic" : undefined,
    textDecorationLine: attrs.underline ? "underline" : undefined,
  };
  // Snap the run to exactly its grid columns so run boundaries — and therefore
  // the cursor/preedit overlays positioned at cursor.x * cellWidth — stay
  // aligned with wide (CJK/Hangul) text rendered at natural glyph width.
  const cellCount = run.cells ?? estimateCellWidth(run.text);
  if (cellWidthPx !== null && cellWidthPx > 0 && cellCount > 0) {
    style.display = "inline-block";
    style.verticalAlign = "top";
    style.width = `${cellCount * cellWidthPx}px`;
  }
  return style;
}

function cursorOverlayStyle(cursor: GridCursor, cell: CellMetrics, color: string): CSSProperties {
  const left = cursor.x * cell.width;
  const top = cursor.y * cell.height;
  const base: CSSProperties = {
    position: "absolute",
    left: 0,
    top: 0,
    pointerEvents: "none",
    boxSizing: "border-box",
    zIndex: 2,
  };

  if (cursor.visualStyle === "bar") {
    return {
      ...base,
      width: Math.max(1, Math.round(cell.width * 0.12)),
      height: cell.height,
      backgroundColor: color,
      transform: `translate(${left}px, ${top}px)`,
    };
  }
  if (cursor.visualStyle === "underline") {
    const height = Math.max(1, Math.round(cell.height * 0.12));
    return {
      ...base,
      width: cell.width,
      height,
      backgroundColor: color,
      transform: `translate(${left}px, ${top + cell.height - height}px)`,
    };
  }
  if (cursor.visualStyle === "blockHollow") {
    return {
      ...base,
      width: cell.width,
      height: cell.height,
      border: `1px solid ${color}`,
      transform: `translate(${left}px, ${top}px)`,
    };
  }
  return {
    ...base,
    width: cell.width,
    height: cell.height,
    backgroundColor: color,
    opacity: 0.5,
    transform: `translate(${left}px, ${top}px)`,
  };
}

export function RemoteTerminal({
  sessionId,
  token,
  title,
  onBack,
  embedded = false,
  activeTabId,
  onSwipeNextTab,
  onSwipePreviousTab,
  onSocketLifecycle,
}: RemoteTerminalProps) {
  const socketRef = useRef<WebSocket | null>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const cellMeasureRef = useRef<HTMLSpanElement>(null);
  const inputSinkRef = useRef<HTMLTextAreaElement>(null);
  const isComposingRef = useRef(false);
  const requestResizeRef = useRef<() => void>(() => {});
  const lastSentGeometryRef = useRef<GridGeometry | null>(null);
  const scheduledSocketRequestRef = useRef<SocketRequest | null>(null);
  const activeSocketRequestRef = useRef<SocketRequest | null>(null);
  const [socketRequest, setSocketRequest] = useState<SocketRequest | null>(null);
  const [connected, setConnected] = useState(false);
  const [grid, setGrid] = useState<TerminalGridState | null>(null);
  const [cellMetrics, setCellMetrics] = useState<CellMetrics>({ width: 0, height: 0 });
  const [preedit, setPreedit] = useState<string | null>(null);
  const { settings, refreshNativePreferences } = useTerminalSettings();

  const [userFontSize, setUserFontSize] = useState<number | null>(null);
  const activeFontSize = clampTerminalFontSize(userFontSize ?? settings.fontSize);

  const touchStartRef = useRef<{ readonly x: number; readonly y: number } | null>(null);
  const touchLastRef = useRef<{ readonly x: number; readonly y: number } | null>(null);
  const touchGestureRef = useRef<"scroll" | "swipe" | null>(null);
  const accumulatedScrollDeltaYRef = useRef<number>(0);
  const lastScrollTimeRef = useRef<number>(0);
  const pinchStartRef = useRef<{ readonly distance: number; readonly initialFontSize: number } | null>(null);
  const isPinchActiveRef = useRef<boolean>(false);

  useEffect(() => {
    void refreshNativePreferences();
  }, [refreshNativePreferences]);

  const focusInput = useCallback(() => {
    inputSinkRef.current?.focus({ preventScroll: true });
  }, []);

  useLayoutEffect(() => {
    focusInput();
  }, [sessionId, activeTabId, focusInput]);

  useEffect(() => {
    if (connected) {
      focusInput();
    }
  }, [connected, focusInput]);

  useLayoutEffect(() => {
    const surface = surfaceRef.current;
    const cellMeasure = cellMeasureRef.current;
    if (!surface || !cellMeasure) return;

    const measureAndResize = () => {
      const surfaceRect = surface.getBoundingClientRect();
      const cellRect = cellMeasure.getBoundingClientRect();
      if (surfaceRect.width <= 0 || surfaceRect.height <= 0 || cellRect.width <= 0 || cellRect.height <= 0) return;

      const nextCellMetrics = { width: cellRect.width, height: cellRect.height };
      setCellMetrics((current) => (
        current.width === nextCellMetrics.width && current.height === nextCellMetrics.height
          ? current
          : nextCellMetrics
      ));

      const geometry = {
        cols: Math.min(MAX_GRID_COLS, Math.max(1, Math.floor(surfaceRect.width / cellRect.width))),
        rows: Math.min(MAX_GRID_ROWS, Math.max(1, Math.floor(surfaceRect.height / cellRect.height))),
      };
      if (!socketRequestMatches(scheduledSocketRequestRef.current, sessionId, token)) {
        const nextRequest = { sessionId, token, geometry };
        scheduledSocketRequestRef.current = nextRequest;
        setGrid(null);
        setConnected(false);
        lastSentGeometryRef.current = null;
        setSocketRequest(nextRequest);
        return;
      }

      const socket = socketRef.current;
      if (
        !socket ||
        socket.readyState !== WebSocket.OPEN ||
        !socketRequestMatches(activeSocketRequestRef.current, sessionId, token) ||
        geometriesEqual(lastSentGeometryRef.current, geometry)
      ) return;
      socket.send(JSON.stringify({ type: "resize", cols: geometry.cols, rows: geometry.rows }));
      lastSentGeometryRef.current = geometry;
    };

    requestResizeRef.current = measureAndResize;
    measureAndResize();

    if (typeof ResizeObserver === "undefined") {
      return () => {
        if (requestResizeRef.current === measureAndResize) requestResizeRef.current = () => {};
      };
    }

    const observer = new ResizeObserver(() => measureAndResize());
    observer.observe(surface);
    return () => {
      observer.disconnect();
      if (requestResizeRef.current === measureAndResize) requestResizeRef.current = () => {};
    };
  }, [sessionId, settings.fontFamily, activeFontSize, token]);

  useEffect(() => {
    if (!socketRequest) return;
    setGrid(null);
    setConnected(false);
    lastSentGeometryRef.current = socketRequest.geometry;

    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let backoffAttempt = 0;
    let disposed = false;

    const clearReconnectTimer = () => {
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    const dial = () => {
      clearReconnectTimer();
      if (disposed) return;
      const socket = new WebSocket(
        terminalSocketUrl(socketRequest.sessionId, socketRequest.token, socketRequest.geometry),
      );
      socket.binaryType = "arraybuffer";
      socketRef.current = socket;
      activeSocketRequestRef.current = socketRequest;
      socket.onopen = () => {
        if (disposed || socketRef.current !== socket) return;
        backoffAttempt = 0;
        setConnected(true);
        onSocketLifecycle?.(socketRequest.sessionId, "open");
        requestResizeRef.current();
      };
      socket.onclose = () => {
        if (disposed || socketRef.current !== socket || reconnectTimer !== null) return;
        setConnected(false);
        onSocketLifecycle?.(socketRequest.sessionId, "closed");

        const delay = Math.min(10000, 1000 * Math.pow(2, backoffAttempt));
        backoffAttempt += 1;
        clearReconnectTimer();
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null;
          dial();
        }, delay);
      };
      socket.onmessage = (event) => {
        if (disposed || socketRef.current !== socket) return;
        if (typeof event.data !== "string") return;
        const frame = parseGridFrame(event.data);
        if (!frame) return;
        setGrid((current) => applyGridFrame(current, frame));
      };
    };

    dial();

    return () => {
      disposed = true;
      clearReconnectTimer();
      const currentSocket = socketRef.current;
      if (socketRef.current === currentSocket) {
        socketRef.current = null;
        activeSocketRequestRef.current = null;
      }
      currentSocket?.close();
    };
  }, [onSocketLifecycle, socketRequest]);

  const handleWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    if (event.deltaY === 0) return;
    const rawRows = Math.trunc(event.deltaY / 20) || (event.deltaY > 0 ? 1 : -1);
    const rows = Math.min(10, Math.max(-10, rawRows));
    if (rows !== 0) {
      socket.send(JSON.stringify({ type: "scroll", rows }));
    }
  };

  const handleTouchStart = (event: React.TouchEvent<HTMLDivElement>) => {
    if (event.touches.length >= 2) {
      isPinchActiveRef.current = true;
      touchStartRef.current = null;
      touchLastRef.current = null;
      touchGestureRef.current = null;
      accumulatedScrollDeltaYRef.current = 0;
      const t1 = event.touches[0];
      const t2 = event.touches[1];
      const distance = Math.hypot(t1.clientX - t2.clientX, t1.clientY - t2.clientY);
      pinchStartRef.current = {
        distance,
        initialFontSize: activeFontSize,
      };
    } else if (event.touches.length === 1 && !isPinchActiveRef.current) {
      const touch = event.touches[0];
      touchStartRef.current = { x: touch.clientX, y: touch.clientY };
      touchLastRef.current = { x: touch.clientX, y: touch.clientY };
      touchGestureRef.current = null;
      accumulatedScrollDeltaYRef.current = 0;
      lastScrollTimeRef.current = 0;
    }
  };

  const handleTouchMove = (event: React.TouchEvent<HTMLDivElement>) => {
    if (event.touches.length >= 2 && pinchStartRef.current) {
      const t1 = event.touches[0];
      const t2 = event.touches[1];
      const distance = Math.hypot(t1.clientX - t2.clientX, t1.clientY - t2.clientY);
      if (pinchStartRef.current.distance > 0) {
        const scale = distance / pinchStartRef.current.distance;
        const targetSize = clampTerminalFontSize(pinchStartRef.current.initialFontSize * scale);
        setUserFontSize(targetSize);
      }
    } else if (event.touches.length === 1 && touchStartRef.current && touchLastRef.current) {
      const touch = event.touches[0];
      const totalDeltaX = touch.clientX - touchStartRef.current.x;
      const totalDeltaY = touch.clientY - touchStartRef.current.y;
      const stepDeltaY = touch.clientY - touchLastRef.current.y;
      touchLastRef.current = { x: touch.clientX, y: touch.clientY };

      if (touchGestureRef.current === null) {
        if (Math.abs(totalDeltaY) > 8 && Math.abs(totalDeltaY) > Math.abs(totalDeltaX)) {
          touchGestureRef.current = "scroll";
          accumulatedScrollDeltaYRef.current = totalDeltaY;
        } else if (Math.abs(totalDeltaX) > 8 && Math.abs(totalDeltaX) > Math.abs(totalDeltaY)) {
          touchGestureRef.current = "swipe";
        }
      } else if (touchGestureRef.current === "scroll") {
        accumulatedScrollDeltaYRef.current += stepDeltaY;
      }

      if (touchGestureRef.current === "scroll" && cellMetrics.height > 0) {
        const now = Date.now();
        if (now - lastScrollTimeRef.current >= 33) {
          const rawRows = Math.trunc(-accumulatedScrollDeltaYRef.current / cellMetrics.height);
          if (rawRows !== 0) {
            const rows = Math.min(10, Math.max(-10, rawRows));
            const socket = socketRef.current;
            if (socket && socket.readyState === WebSocket.OPEN) {
              socket.send(JSON.stringify({ type: "scroll", rows }));
            }
            accumulatedScrollDeltaYRef.current += rows * cellMetrics.height;
            lastScrollTimeRef.current = now;
          }
        }
      }
    }
  };

  const handleTouchEnd = (event: React.TouchEvent<HTMLDivElement>) => {
    if (event.touches.length === 0) {
      if (!isPinchActiveRef.current && touchStartRef.current && touchGestureRef.current !== "scroll") {
        const changedTouch = event.changedTouches[0];
        const endX = changedTouch ? changedTouch.clientX : (touchLastRef.current?.x ?? touchStartRef.current.x);
        const endY = changedTouch ? changedTouch.clientY : (touchLastRef.current?.y ?? touchStartRef.current.y);
        const deltaX = endX - touchStartRef.current.x;
        const deltaY = endY - touchStartRef.current.y;

        if (Math.abs(deltaX) >= SWIPE_THRESHOLD_PX && Math.abs(deltaX) > Math.abs(deltaY)) {
          if (deltaX < 0) {
            onSwipeNextTab?.();
          } else {
            onSwipePreviousTab?.();
          }
          focusInput();
        }
      }
      touchStartRef.current = null;
      touchLastRef.current = null;
      touchGestureRef.current = null;
      accumulatedScrollDeltaYRef.current = 0;
      pinchStartRef.current = null;
      isPinchActiveRef.current = false;
    } else if (event.touches.length === 1) {
      pinchStartRef.current = null;
    }
  };

  const handleTouchCancel = () => {
    touchStartRef.current = null;
    touchLastRef.current = null;
    touchGestureRef.current = null;
    accumulatedScrollDeltaYRef.current = 0;
    pinchStartRef.current = null;
    isPinchActiveRef.current = false;
  };

  const sendText = (text: string) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN || text.length === 0) return;
    socket.send(new TextEncoder().encode(text));
  };

  const commitComposition = (data: string) => {
    isComposingRef.current = false;
    setPreedit(null);
    const sink = inputSinkRef.current;
    const committed = data.length > 0 ? data : (sink?.value ?? "");
    if (sink) sink.value = "";
    sendText(committed);
  };

  const handleSinkInput = (event: React.FormEvent<HTMLTextAreaElement>) => {
    const sink = event.currentTarget;
    if (isComposingRef.current) {
      setPreedit(sink.value);
      return;
    }
    const text = sink.value;
    sink.value = "";
    sendText(text);
  };

  const sendKey = (key: string) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;

    if (key === "ctrl-c") {
      socket.send(JSON.stringify({ type: "signal", signal: "interrupt" }));
      return;
    }
    const modifiedDockSequence = modifiedDockNavigationSequence(key);
    if (modifiedDockSequence) {
      socket.send(new TextEncoder().encode(modifiedDockSequence));
      return;
    }
    if (key.startsWith("alt-")) {
      socket.send(new TextEncoder().encode(`\u001b${key.slice(4)}`));
      return;
    }
    if (key.startsWith("ctrl-") && key.length === 6) {
      socket.send(new Uint8Array([key.slice(5).toUpperCase().charCodeAt(0) - 64]));
      return;
    }
    const sequenceKey = BROWSER_KEY_NAMES[key] ?? key;
    const sequence = KEY_SEQUENCES[sequenceKey as keyof typeof KEY_SEQUENCES] ?? sequenceKey;
    socket.send(new TextEncoder().encode(sequence));
  };

  return (
    <div className={`flex min-h-0 flex-col overflow-hidden bg-terminal text-foreground ${embedded ? "h-full flex-1" : "h-[100dvh]"}`}>
      {!embedded ? (
        <div className="flex h-8 shrink-0 items-center justify-between border-b border-border bg-card px-2">
          <div className="flex min-w-0 items-center gap-2">
            {onBack ? (
              <button type="button" onClick={onBack} className="rounded-md border border-border bg-secondary px-2 py-1 text-xs font-medium text-secondary-foreground">
                Back
              </button>
            ) : null}
            <span className="truncate font-mono text-xs text-muted-foreground">{title ?? "Desktop terminal"}</span>
          </div>
          <span role="status" className="font-mono text-[10px] text-muted-foreground">
            {connected ? "Live" : "Connecting"}
          </span>
        </div>
      ) : null}
      <div
        ref={surfaceRef}
        data-testid="remote-terminal-grid"
        tabIndex={0}
        onPointerDown={() => {
          inputSinkRef.current?.focus();
        }}
        onWheel={handleWheel}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onTouchCancel={handleTouchCancel}
        onKeyDown={(event) => {
          // IME-composed keystrokes (Korean jamo, CJK, etc.) must flow through the
          // input sink's composition events. Sending them here shattered Hangul into
          // isolated jamo writes, one per physical keypress.
          if (event.nativeEvent.isComposing || event.key === "Process" || event.key === "Dead") {
            return;
          }
          if (event.ctrlKey && !event.metaKey && !event.altKey && event.key.length === 1) {
            event.preventDefault();
            sendKey(`ctrl-${event.key.toLowerCase()}`);
          } else if (!event.metaKey) {
            const modifiedSequence = modifiedNavigationSequence(event.key, event.ctrlKey, event.altKey);
            if (modifiedSequence) {
              event.preventDefault();
              sendKey(modifiedSequence);
            } else if (event.key === "Enter") {
              event.preventDefault();
              sendKey("\r");
            } else if (BROWSER_KEY_NAMES[event.key]) {
              event.preventDefault();
              sendKey(event.key);
            } else if (!event.altKey && event.key.length === 1) {
              if (event.key.charCodeAt(0) <= 0x7f) {
                event.preventDefault();
                sendKey(event.key);
              } else {
                // Non-ASCII printable key with the sink unfocused (keyboard-only tab
                // navigation): hand focus to the composition sink and let the IME
                // input path own this keystroke instead of sending the raw jamo.
                inputSinkRef.current?.focus();
              }
            }
          }
        }}
        onPaste={(event) => {
          event.preventDefault();
          sendKey(event.clipboardData.getData("text"));
        }}
        className="relative min-h-0 flex-1 overflow-hidden bg-terminal outline-none motion-reduce:transition-none"
        style={{
          backgroundColor: settings.theme.background,
          color: settings.theme.foreground,
          fontFamily: settings.fontFamily,
          fontSize: `${activeFontSize}px`,
          lineHeight: 1,
          whiteSpace: "pre",
        }}
      >
        {embedded && !connected ? (
          <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex justify-center pt-1.5">
            <span role="status" className="rounded-full border border-border bg-card/95 px-2 py-0.5 font-mono text-[10px] leading-tight text-muted-foreground shadow-sm">Connecting</span>
          </div>
        ) : null}
        <span
          ref={cellMeasureRef}
          data-terminal-cell-measure="true"
          aria-hidden="true"
          style={{
            position: "absolute",
            visibility: "hidden",
            display: "inline-block",
            width: "1ch",
            height: "1em",
            pointerEvents: "none",
          }}
        />
        <textarea
          ref={inputSinkRef}
          data-testid="remote-terminal-input-sink"
          aria-label="Remote terminal input"
          autoCapitalize="none"
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          className="pointer-events-none absolute resize-none overflow-hidden border-0 bg-transparent p-0 opacity-0 outline-none"
          style={{
            left: 0,
            top: 0,
            width: Math.max(1, cellMetrics.width),
            height: Math.max(1, cellMetrics.height),
            transform:
              cellMetrics.width > 0 && cellMetrics.height > 0
                ? `translate(${(grid?.cursor.x ?? 0) * cellMetrics.width}px, ${(grid?.cursor.y ?? 0) * cellMetrics.height}px)`
                : undefined,
            color: settings.theme.foreground,
            caretColor: "transparent",
            fontFamily: settings.fontFamily,
            fontSize: `${activeFontSize}px`,
            lineHeight: 1,
          }}
          onCompositionStart={() => {
            isComposingRef.current = true;
          }}
          onCompositionUpdate={(event) => {
            setPreedit(event.data);
          }}
          onCompositionEnd={(event) => {
            commitComposition(event.data);
          }}
          onInput={handleSinkInput}
          onBlur={() => {
            if (isComposingRef.current) {
              commitComposition("");
            }
            const sink = inputSinkRef.current;
            if (sink) sink.value = "";
          }}
        />
        {grid?.lines.map((line) => (
          <span
            key={line.index}
            data-grid-line={line.index}
            style={{
              display: "block",
              minWidth: cellMetrics.width > 0 ? `${grid.cols * cellMetrics.width}px` : `${grid.cols}ch`,
              height: cellMetrics.height > 0 ? `${cellMetrics.height}px` : "1em",
            }}
          >
            {line.runs.map((run, runIndex) => (
              <span
                key={runIndex}
                style={runStyle(run, settings.theme.foreground, settings.theme.background, cellMetrics.width > 0 ? cellMetrics.width : null)}
              >
                {run.text}
              </span>
            ))}
          </span>
        ))}
        {grid?.cursor.visible && cellMetrics.width > 0 && cellMetrics.height > 0 ? (
          <span
            aria-hidden="true"
            data-terminal-cursor="true"
            className={grid.cursor.blinking ? "animate-pulse" : undefined}
            style={cursorOverlayStyle(grid.cursor, cellMetrics, settings.theme.cursor)}
          />
        ) : null}
        {preedit !== null && preedit.length > 0 && cellMetrics.width > 0 && cellMetrics.height > 0 ? (
          <span
            aria-hidden="true"
            data-testid="remote-terminal-preedit"
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              zIndex: 2,
              pointerEvents: "none",
              whiteSpace: "pre",
              color: settings.theme.foreground,
              backgroundColor: settings.theme.background,
              textDecorationLine: "underline",
              transform: `translate(${(grid?.cursor.x ?? 0) * cellMetrics.width}px, ${(grid?.cursor.y ?? 0) * cellMetrics.height}px)`,
            }}
          >
            {preedit}
          </span>
        ) : null}
      </div>
      <MobileKeyDock onSendKey={sendKey} />
    </div>
  );
}
