import type { CSSProperties } from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { MobileKeyDock } from "../components/MobileKeyDock";
import { useTerminalSettings } from "../lib/terminalSettings";
import {
  applyGridFrame,
  decodeGridAttrs,
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

function runStyle(run: GridRun, foreground: string, background: string): CSSProperties {
  const attrs = decodeGridAttrs(run.attrs);
  const explicitForeground = run.fg ? colorCss(run.fg) : null;
  const explicitBackground = run.bg ? colorCss(run.bg) : null;
  return {
    color: attrs.inverse ? (explicitBackground ?? background) : (explicitForeground ?? undefined),
    backgroundColor: attrs.inverse ? (explicitForeground ?? foreground) : (explicitBackground ?? undefined),
    fontWeight: attrs.bold ? 700 : undefined,
    fontStyle: attrs.italic ? "italic" : undefined,
    textDecorationLine: attrs.underline ? "underline" : undefined,
  };
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
  onSwipeNextTab,
  onSwipePreviousTab,
  onSocketLifecycle,
}: RemoteTerminalProps) {
  const socketRef = useRef<WebSocket | null>(null);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const cellMeasureRef = useRef<HTMLSpanElement>(null);
  const requestResizeRef = useRef<() => void>(() => {});
  const lastSentGeometryRef = useRef<GridGeometry | null>(null);
  const scheduledSocketRequestRef = useRef<SocketRequest | null>(null);
  const activeSocketRequestRef = useRef<SocketRequest | null>(null);
  const [socketRequest, setSocketRequest] = useState<SocketRequest | null>(null);
  const [connected, setConnected] = useState(false);
  const [grid, setGrid] = useState<TerminalGridState | null>(null);
  const [cellMetrics, setCellMetrics] = useState<CellMetrics>({ width: 0, height: 0 });
  const { settings, refreshNativePreferences } = useTerminalSettings();

  const [userFontSize, setUserFontSize] = useState<number | null>(null);
  const activeFontSize = clampTerminalFontSize(userFontSize ?? settings.fontSize);

  const touchStartRef = useRef<{ readonly x: number; readonly y: number } | null>(null);
  const touchLastRef = useRef<{ readonly x: number; readonly y: number } | null>(null);
  const pinchStartRef = useRef<{ readonly distance: number; readonly initialFontSize: number } | null>(null);
  const isPinchActiveRef = useRef<boolean>(false);

  useEffect(() => {
    void refreshNativePreferences();
  }, [refreshNativePreferences]);

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
    const socket = new WebSocket(
      terminalSocketUrl(socketRequest.sessionId, socketRequest.token, socketRequest.geometry),
    );
    socket.binaryType = "arraybuffer";
    socketRef.current = socket;
    activeSocketRequestRef.current = socketRequest;
    socket.onopen = () => {
      if (socketRef.current !== socket) return;
      setConnected(true);
      onSocketLifecycle?.(socketRequest.sessionId, "open");
      requestResizeRef.current();
    };
    socket.onclose = () => {
      if (socketRef.current !== socket) return;
      setConnected(false);
      onSocketLifecycle?.(socketRequest.sessionId, "closed");
    };
    socket.onmessage = (event) => {
      if (socketRef.current !== socket) return;
      if (typeof event.data !== "string") return;
      const frame = parseGridFrame(event.data);
      if (!frame) return;
      setGrid((current) => applyGridFrame(current, frame));
    };

    return () => {
      if (socketRef.current === socket) {
        socketRef.current = null;
        activeSocketRequestRef.current = null;
      }
      socket.close();
    };
  }, [onSocketLifecycle, socketRequest]);

  const handleTouchStart = (event: React.TouchEvent<HTMLDivElement>) => {
    if (event.touches.length >= 2) {
      isPinchActiveRef.current = true;
      touchStartRef.current = null;
      touchLastRef.current = null;
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
    } else if (event.touches.length === 1 && touchStartRef.current) {
      const touch = event.touches[0];
      touchLastRef.current = { x: touch.clientX, y: touch.clientY };
    }
  };

  const handleTouchEnd = (event: React.TouchEvent<HTMLDivElement>) => {
    if (event.touches.length === 0) {
      if (!isPinchActiveRef.current && touchStartRef.current) {
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
        }
      }
      touchStartRef.current = null;
      touchLastRef.current = null;
      pinchStartRef.current = null;
      isPinchActiveRef.current = false;
    } else if (event.touches.length === 1) {
      pinchStartRef.current = null;
    }
  };

  const handleTouchCancel = () => {
    touchStartRef.current = null;
    touchLastRef.current = null;
    pinchStartRef.current = null;
    isPinchActiveRef.current = false;
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
      <div className="flex h-8 shrink-0 items-center justify-between border-b border-border bg-card px-2">
        <div className="flex min-w-0 items-center gap-2">
          {!embedded && onBack ? (
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
      <div
        ref={surfaceRef}
        data-testid="remote-terminal-grid"
        tabIndex={0}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onTouchCancel={handleTouchCancel}
        onKeyDown={(event) => {
          if (event.ctrlKey && !event.metaKey && !event.altKey && event.key.length === 1) {
            event.preventDefault();
            sendKey(`ctrl-${event.key.toLowerCase()}`);
          } else if (!event.metaKey) {
            const modifiedSequence = modifiedNavigationSequence(event.key, event.ctrlKey, event.altKey);
            if (modifiedSequence) {
              event.preventDefault();
              sendKey(modifiedSequence);
            } else if (!event.altKey && event.key.length === 1) {
              event.preventDefault();
              sendKey(event.key);
            } else if (event.key === "Enter") {
              event.preventDefault();
              sendKey("\r");
            } else if (BROWSER_KEY_NAMES[event.key]) {
              event.preventDefault();
              sendKey(event.key);
            }
          }
        }}
        onPaste={(event) => {
          event.preventDefault();
          sendKey(event.clipboardData.getData("text"));
        }}
        className="relative min-h-0 flex-1 overflow-auto bg-terminal outline-none motion-reduce:transition-none"
        style={{
          backgroundColor: settings.theme.background,
          color: settings.theme.foreground,
          fontFamily: settings.fontFamily,
          fontSize: `${activeFontSize}px`,
          lineHeight: 1,
          whiteSpace: "pre",
        }}
      >
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
              <span key={runIndex} style={runStyle(run, settings.theme.foreground, settings.theme.background)}>
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
      </div>
      <MobileKeyDock onSendKey={sendKey} />
    </div>
  );
}
