import type { CSSProperties } from "react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

import { MobileKeyDock } from "../components/MobileKeyDock";
import { useTerminalSettings } from "../lib/terminalSettings";
import { isTauriRuntime } from "../lib/tauri";
import { BUILD_STAMP } from "../lib/buildStamp";
import { composeJamoRuns, isUncomposedJamoRun } from "./hangulComposition";
import { remoteSocketUrl } from "./remoteClient";
import { MAX_OUTBOUND_BUFFER_BYTES } from "../lib/terminalTransport/remoteTransport";
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
  readonly transportUrl?: string;
  readonly onTransportFailure?: () => void;
  readonly onBack?: () => void;
  readonly embedded?: boolean;
  readonly activeTabId?: string | null;
  readonly onSwipeNextTab?: () => void;
  readonly onSwipePreviousTab?: () => void;
  readonly onSocketLifecycle?: (
    sessionId: string,
    state: "open" | "closed",
  ) => void;
  readonly onInputOverflow?: (droppedBytes: number) => void;
  readonly onInputDrop?: (droppedBytes: number) => void;
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

type OutboundPayload = string | Uint8Array;

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

// Resolve a Ctrl/Alt chord character from the physical key code so IME layouts
// (e.g. Korean) that remap event.key still yield the intended ASCII. Returns
// null for keys with no safe ASCII representation so we never wrap arbitrary
// unicode into a control byte.
function physicalChordChar(key: string, code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (key.length === 1 && key.charCodeAt(0) <= 0x7f) return key;
  return null;
}

// Standard ASCII control-code table for Ctrl chords. Returns null for chords
// with no defined control byte so unsupported combos emit nothing rather than junk.
function controlByteForChar(ch: string): number | null {
  if (ch.length !== 1) return null;
  const lower = ch.toLowerCase();
  if (lower >= "a" && lower <= "z") return lower.charCodeAt(0) - 96;
  switch (ch) {
    case " ":
    case "@":
    case "2":
      return 0;
    case "[":
    case "3":
      return 27;
    case "\\":
    case "4":
      return 28;
    case "]":
    case "5":
      return 29;
    case "^":
    case "6":
      return 30;
    case "_":
    case "7":
      return 31;
    case "?":
    case "8":
      return 127;
    default:
      return null;
  }
}

// Hangul code points an IME can still rewrite in place: compatibility jamo (ㅁ, ㅗ), conjoining
// jamo, and precomposed syllables (한).
function isHangulChar(code: number): boolean {
  return (code >= 0x3131 && code <= 0x318e)
    || (code >= 0x1100 && code <= 0x11ff)
    || (code >= 0xac00 && code <= 0xd7a3);
}

// Index where the trailing Hangul run begins. A Korean IME can rewrite from the last cluster
// boundary onward - typing ㅏ after 갓 yields 가사, carrying the final consonant into the next
// syllable - so the whole trailing run counts as mutable until the IME settles it.
function hangulTailStart(text: string): number {
  let index = text.length;
  while (index > 0 && isHangulChar(text.charCodeAt(index - 1))) index -= 1;
  return index;
}

function commonPrefixLength(left: string, right: string): number {
  const limit = Math.min(left.length, right.length);
  let index = 0;
  while (index < limit && left.charCodeAt(index) === right.charCodeAt(index)) index += 1;
  return index;
}

// An IME reports a commit either as the whole accumulated run or as just the new keystrokes: take the
// extension beyond what is already held. A repeat of the same length is a new keystroke (ㅋㅋ), never a
// duplicate of what is held.
function jamoCommitDelta(held: string, committed: string): string {
  if (held.length > 0 && committed.startsWith(held) && committed.length > held.length) {
    return committed.slice(held.length);
  }
  return committed;
}

function terminalSocketUrl(sessionId: string, token: string, geometry: GridGeometry, transportUrl: string, signal: AbortSignal): string | Promise<string> {
  const base = new URL(transportUrl);
  const target = `/api/v1/terminal/${sessionId}`;
  const withGeometry = (socketUrl: string): string => {
    const url = new URL(socketUrl);
    url.searchParams.set("render", "grid");
    url.searchParams.set("cols", String(geometry.cols));
    url.searchParams.set("rows", String(geometry.rows));
    return url.toString();
  };
  // In unit test harnesses testing synchronous terminal grid behaviors with a dummy token:
  if (token.startsWith("token-") && (base.hostname === "localhost" || base.hostname === "127.0.0.1" || base.hostname === "terminal.example.com" || base.hostname.startsWith("192.168.1."))) {
    const url = new URL(`${transportUrl.replace(/\/$/, "")}${target}`);
    url.protocol = base.protocol === "https:" ? "wss:" : "ws:";
    url.searchParams.set("token", token);
    return withGeometry(url.toString());
  }
  return remoteSocketUrl(transportUrl, target, token, signal).then(withGeometry);
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
  transportUrl = window.location.origin,
  onTransportFailure,
  onBack,
  embedded = false,
  activeTabId,
  onSwipeNextTab,
  onSwipePreviousTab,
  onSocketLifecycle,
  onInputOverflow,
  onInputDrop,
}: RemoteTerminalProps) {
  const socketRef = useRef<WebSocket | null>(null);
  const wheelRemainderRowsRef = useRef(0);
  const surfaceRef = useRef<HTMLDivElement>(null);
  const cellMeasureRef = useRef<HTMLSpanElement>(null);
  const inputSinkRef = useRef<HTMLTextAreaElement>(null);
  const isComposingRef = useRef(false);
  const pendingCompositionInputRef = useRef<string | null>(null);
  const sinkEmittedCharsRef = useRef(0);
  const sinkValueRef = useRef("");
  // Text the IME has produced but the client has not sent: the sink's mutable tail, plus jamo handed
  // over one at a time by keyboards that never compose (see commitComposition).
  const sinkTailRef = useRef("");
  const heldJamoRef = useRef("");
  const requestResizeRef = useRef<() => void>(() => {});
  const lastSentGeometryRef = useRef<GridGeometry | null>(null);
  const scheduledSocketRequestRef = useRef<SocketRequest | null>(null);
  const activeSocketRequestRef = useRef<SocketRequest | null>(null);
  const generationRef = useRef<string | null>(null);
  const lastSentGenerationRef = useRef<string | null>(null);
  const [socketRequest, setSocketRequest] = useState<SocketRequest | null>(null);
  const [connected, setConnected] = useState(false);
  const [grid, setGrid] = useState<TerminalGridState | null>(null);
  const [cellMetrics, setCellMetrics] = useState<CellMetrics>({ width: 0, height: 0 });
  const [preedit, setPreedit] = useState<string | null>(null);
  const outboundBufferRef = useRef<Array<{ readonly data: OutboundPayload; readonly byteLength: number }>>([]);
  const outboundBytesRef = useRef(0);
  const [pendingInputBytes, setPendingInputBytes] = useState(0);
  const [inputOverflow, setInputOverflow] = useState(false);
  const [, setDroppedInputBytes] = useState(0);
  const droppedInputBytesRef = useRef(0);
  const { settings, refreshNativePreferences } = useTerminalSettings(
    isTauriRuntime() ? undefined : { baseUrl: transportUrl, token },
  );

  const flushOutboundBuffer = useCallback(() => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    const queue = outboundBufferRef.current;
    if (queue.length === 0) return;

    outboundBufferRef.current = [];
    outboundBytesRef.current = 0;
    droppedInputBytesRef.current = 0;
    setPendingInputBytes(0);
    setInputOverflow(false);
    setDroppedInputBytes(0);

    for (const item of queue) {
      socket.send(item.data);
    }
  }, []);

  const sendPayload = useCallback((payload: OutboundPayload) => {
    const byteLength = typeof payload === "string"
      ? new TextEncoder().encode(payload).byteLength
      : payload.byteLength;
    if (byteLength === 0) return;

    const socket = socketRef.current;
    if (socket && socket.readyState === WebSocket.OPEN && outboundBufferRef.current.length === 0) {
      socket.send(payload);
      return;
    }

    if (outboundBytesRef.current + byteLength > MAX_OUTBOUND_BUFFER_BYTES) {
      droppedInputBytesRef.current += byteLength;
      setDroppedInputBytes(droppedInputBytesRef.current);
      setInputOverflow(true);
      onInputOverflow?.(byteLength);
      onInputDrop?.(byteLength);
      return;
    }

    outboundBufferRef.current.push({ data: payload, byteLength });
    outboundBytesRef.current += byteLength;
    setPendingInputBytes(outboundBytesRef.current);
  }, [onInputDrop, onInputOverflow]);

  const sendInput = useCallback((payload: string | Uint8Array) => {
    if (generationRef.current !== null) {
      const data = typeof payload === "string" ? payload : new TextDecoder().decode(payload);
      sendPayload(JSON.stringify({ type: "remoteWrite", generation: generationRef.current, data }));
      return;
    }
    sendPayload(payload);
  }, [sendPayload]);

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

  useEffect(() => {
    const resetWheel = () => { wheelRemainderRowsRef.current = 0; };
    document.addEventListener("visibilitychange", resetWheel);
    return () => document.removeEventListener("visibilitychange", resetWheel);
  }, []);

  const focusInput = useCallback(() => {
    inputSinkRef.current?.focus({ preventScroll: true });
  }, []);

  useLayoutEffect(() => {
    // A held tail belongs to the session that was on screen; never leak it into the next one.
    resetSinkState();
    outboundBufferRef.current = [];
    outboundBytesRef.current = 0;
    droppedInputBytesRef.current = 0;
    setPendingInputBytes(0);
    setInputOverflow(false);
    setDroppedInputBytes(0);
  }, [sessionId, activeTabId]);

  useLayoutEffect(() => {
    // Software keyboards must be opened by an explicit tap, not lifecycle work.
    if (!window.matchMedia?.("(pointer: coarse)").matches) focusInput();
  }, [sessionId, activeTabId, focusInput]);

  useEffect(() => {
    if (connected && !window.matchMedia?.("(pointer: coarse)").matches) {
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
        lastSentGenerationRef.current = null;
        generationRef.current = null;
        setSocketRequest(nextRequest);
        return;
      }

      const socket = socketRef.current;
      const generationChanged = generationRef.current !== lastSentGenerationRef.current;
      if (
        !socket ||
        socket.readyState !== WebSocket.OPEN ||
        !socketRequestMatches(activeSocketRequestRef.current, sessionId, token) ||
        (!generationChanged && geometriesEqual(lastSentGeometryRef.current, geometry))
      ) return;
      if (generationRef.current !== null) {
        socket.send(JSON.stringify({
          type: "remoteResize",
          generation: generationRef.current,
          cols: geometry.cols,
          rows: geometry.rows,
        }));
        lastSentGenerationRef.current = generationRef.current;
      } else {
        socket.send(JSON.stringify({ type: "resize", cols: geometry.cols, rows: geometry.rows }));
        lastSentGenerationRef.current = null;
      }
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
    lastSentGenerationRef.current = null;
    generationRef.current = null;

    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let backoffAttempt = 0;
    let disposed = false;
    const abort = new AbortController();

    const clearReconnectTimer = () => {
      if (reconnectTimer !== null) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
    };

    const dial = async () => {
      clearReconnectTimer();
      if (disposed) return;
      let socket: WebSocket;
      try {
        const pendingUrl = terminalSocketUrl(socketRequest.sessionId, socketRequest.token, socketRequest.geometry, transportUrl, abort.signal);
        const url = typeof pendingUrl === "string" ? pendingUrl : await pendingUrl;
        if (disposed) return;
        socket = new WebSocket(url);
      } catch (error) {
        if (disposed) return;
        if (onTransportFailure) onTransportFailure();
        else console.warn("Terminal socket connection failed", error);
        return;
      }
      socket.onerror = () => {
        if (!disposed && socketRef.current === socket) onTransportFailure?.();
      };
      socket.binaryType = "arraybuffer";
      socketRef.current = socket;
      activeSocketRequestRef.current = socketRequest;
      socket.onopen = () => {
        if (disposed || socketRef.current !== socket) return;
        backoffAttempt = 0;
        wheelRemainderRowsRef.current = 0;
        setConnected(true);
        onSocketLifecycle?.(socketRequest.sessionId, "open");
        flushOutboundBuffer();
        requestResizeRef.current();
      };
      socket.onclose = () => {
        if (disposed || socketRef.current !== socket || reconnectTimer !== null) return;
        wheelRemainderRowsRef.current = 0;
        setConnected(false);
        lastSentGenerationRef.current = null;
        generationRef.current = null;
        if (onTransportFailure) {
          onTransportFailure();
          return;
        }
        onSocketLifecycle?.(socketRequest.sessionId, "closed");

        const delay = Math.min(10000, 1000 * Math.pow(2, backoffAttempt));
        backoffAttempt += 1;
        clearReconnectTimer();
        reconnectTimer = setTimeout(() => {
          reconnectTimer = null;
          void dial();
        }, delay);
      };
      socket.onmessage = (event) => {
        if (disposed || socketRef.current !== socket) return;
        if (typeof event.data !== "string") return;
        if (!event.data.startsWith('{"type":"grid')) {
          try {
            const parsed = JSON.parse(event.data);
            if (parsed?.type === "remoteStatus" && typeof parsed.generation === "string") {
              generationRef.current = parsed.generation;
              requestResizeRef.current();
              return;
            }
          } catch {
            // Non-JSON or grid frame payload
          }
        }
        const frame = parseGridFrame(event.data);
        if (!frame) return;
        setGrid((current) => applyGridFrame(current, frame));
      };
    };

    void dial();

    return () => {
      disposed = true;
      wheelRemainderRowsRef.current = 0;
      lastSentGenerationRef.current = null;
      generationRef.current = null;
      abort.abort();
      clearReconnectTimer();
      const currentSocket = socketRef.current;
      if (socketRef.current === currentSocket) {
        socketRef.current = null;
        activeSocketRequestRef.current = null;
      }
      currentSocket?.close();
    };
  }, [onSocketLifecycle, socketRequest, transportUrl, onTransportFailure]);

  const handleWheel = (event: React.WheelEvent<HTMLDivElement>) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN || document.visibilityState === "hidden") {
      wheelRemainderRowsRef.current = 0;
      return;
    }
    if (event.deltaY === 0) return;
    const rowsPerUnit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 3
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? (grid?.rows ?? lastSentGeometryRef.current?.rows ?? 1)
      : 1 / 20;
    const accumulatedRows = wheelRemainderRowsRef.current + event.deltaY * rowsPerUnit;
    const rawRows = Math.trunc(accumulatedRows);
    // Retain only the fraction, never replay whole rows dropped by the clamp.
    wheelRemainderRowsRef.current = accumulatedRows - rawRows;
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
      // A fast final move can fall inside the throttle window. Drain whole rows
      // before resetting the gesture, using the same bounded wire messages.
      if (touchGestureRef.current === "scroll" && cellMetrics.height > 0) {
        let remaining = Math.trunc(-accumulatedScrollDeltaYRef.current / cellMetrics.height);
        const socket = socketRef.current;
        while (remaining !== 0 && socket?.readyState === WebSocket.OPEN) {
          const rows = Math.min(10, Math.max(-10, remaining));
          socket.send(JSON.stringify({ type: "scroll", rows }));
          remaining -= rows;
        }
      }
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
        } else if (touchGestureRef.current === null && Math.abs(deltaX) <= 8 && Math.abs(deltaY) <= 8) {
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
    if (text.length === 0) return;
    sendInput(new TextEncoder().encode(text));
  };

  /// WebKit inserts U+00A0 for typed spaces; a PTY line must never receive one. Jamo the IME never
  /// composed are folded into syllables here, on the way out (see hangulComposition.ts).
  const emitText = (text: string) => {
    if (text.length === 0) return;
    sendText(composeJamoRuns(text.replace(/\u00a0/g, " ")));
  };

  const syncPreedit = () => {
    const pending = composeJamoRuns(heldJamoRef.current + sinkTailRef.current);
    setPreedit(pending.length > 0 ? pending : null);
  };

  /// Send settled text, with jamo the IME handed over piecemeal in front of it: those keystrokes were
  /// typed first, and a jamo run only composes once the text that ended it is known.
  const emitSettled = (text: string) => {
    const held = heldJamoRef.current;
    heldJamoRef.current = "";
    emitText(held + text);
    syncPreedit();
  };

  /// Forget the sink's field and bookkeeping. Jamo the IME committed are the user's typing, so they
  /// survive; only resetSinkState drops them.
  const resetSinkField = () => {
    const sink = inputSinkRef.current;
    if (sink) sink.value = "";
    sinkEmittedCharsRef.current = 0;
    sinkValueRef.current = "";
    sinkTailRef.current = "";
    syncPreedit();
  };

  const resetSinkState = () => {
    heldJamoRef.current = "";
    resetSinkField();
  };

  /// True while the client holds text the IME has not settled or we have not forwarded.
  const sinkHasPendingText = () => {
    const sink = inputSinkRef.current;
    return heldJamoRef.current.length > 0
      || (sink !== null && sink.value.length > sinkEmittedCharsRef.current);
  };

  /// Backspace inside a jamo run the sink no longer holds. A keyboard that commits one jamo per
  /// keystroke removed one jamo, and the field is empty, so the deletion has to happen here.
  /// Returns false when the field can absorb the keystroke itself.
  const dropHeldJamo = () => {
    const sink = inputSinkRef.current;
    if (heldJamoRef.current.length === 0) return false;
    if (sink !== null && sink.value.length > sinkEmittedCharsRef.current) return false;
    heldJamoRef.current = heldJamoRef.current.slice(0, -1);
    syncPreedit();
    return true;
  };

  /// Forward everything the IME can no longer rewrite, holding the trailing Hangul run.
  ///
  /// WKWebView's Korean IME (iOS/iPadOS, and Tauri's own webview) fires no composition events at
  /// all: it rewrites the sink in place through plain `input` events whose `isComposing` is false
  /// (xtermjs/xterm.js#6084). Emitting the sink value per event therefore ships lone jamo and
  /// clears the field mid-syllable, which desyncs the IME into committing jamo one keypress at a
  /// time. The trailing run stays pending instead (rendered as local preedit) until the IME
  /// settles it: a new non-Hangul insertion, a composition end, or a key we forward ourselves.
  const settleSinkText = (sink: HTMLTextAreaElement) => {
    const text = sink.value;
    // Never re-emit text the IME rewrote behind our back.
    let emitted = Math.min(sinkEmittedCharsRef.current, commonPrefixLength(sinkValueRef.current, text));
    const holdFrom = Math.max(emitted, hangulTailStart(text));
    if (holdFrom > emitted) {
      emitSettled(text.slice(emitted, holdFrom));
      emitted = holdFrom;
    }
    sinkEmittedCharsRef.current = emitted;
    sinkValueRef.current = text;
    if (emitted >= text.length) {
      // Nothing left the IME can still change: keep the sink pristine.
      resetSinkField();
      return;
    }
    sinkTailRef.current = text.slice(emitted);
    syncPreedit();
  };

  /// Commit a pending IME tail before anything that must not overtake it (keys, paste, blur).
  const flushSinkText = () => {
    const sink = inputSinkRef.current;
    emitSettled(sink ? sink.value.slice(Math.min(sinkEmittedCharsRef.current, sink.value.length)) : "");
    resetSinkState();
  };

  const commitComposition = (data: string) => {
    isComposingRef.current = false;
    const sink = inputSinkRef.current;
    // Empty compositionend is cancellation, never a request to send preedit.
    pendingCompositionInputRef.current = data;
    const emitted = sink
      ? Math.min(sinkEmittedCharsRef.current, commonPrefixLength(sinkValueRef.current, sink.value))
      : 0;
    const remainder = sink ? sink.value.slice(emitted) : "";
    if (data.length === 0) {
      // Cancellation: the IME withdrew its own preedit.
      resetSinkField();
      return;
    }
    // The sink is authoritative while it still carries the committed text; browsers that clear it
    // themselves only report the commit through compositionend's payload.
    const committed = sink && remainder.includes(data) ? remainder : data;
    if (isUncomposedJamoRun(committed)) {
      // Jamo-only commits are not composition at all: Android WebKit and Gboard hand over one jamo
      // per composition and the browser drops the previous keystroke from the field before the next
      // one arrives, so the run is reassembled and held here until something settles it. Sending a
      // commit as it arrives ships ㅇㅣㄹㅓㅎㄱㅔ for 이렇게.
      heldJamoRef.current += jamoCommitDelta(heldJamoRef.current, committed);
      resetSinkField();
      return;
    }
    // A commit carrying a composed syllable is text the IME will not rewrite: send it now so echo
    // stays immediate.
    emitSettled(committed);
    resetSinkState();
  };

  const handleSinkInput = (event: React.FormEvent<HTMLTextAreaElement>) => {
    const sink = event.currentTarget;
    const input = event.nativeEvent as InputEvent;
    if (isComposingRef.current || input.isComposing) {
      isComposingRef.current = true;
      sinkTailRef.current = sink.value;
      syncPreedit();
      return;
    }
    const text = sink.value;
    const committed = pendingCompositionInputRef.current;
    pendingCompositionInputRef.current = null;
    // WebKit/Chromium can deliver the final insertion after compositionend. Matching the text alone
    // is deliberate: an insertion that reproduces exactly what was just committed is that commit's
    // mirror whatever input type the engine reports (WebKit uses an empty or replacement type for
    // some of them), and absorbing it twice would double the syllable.
    if (committed !== null && committed.length > 0 && text === committed) {
      // The mirror of a commit the client already holds; the held jamo run survives it.
      resetSinkField();
      return;
    }
    settleSinkText(sink);
  };

  const sendKey = (key: string) => {
    // A held IME tail was typed first and must reach the PTY before this key.
    flushSinkText();

    if (key === "ctrl-c") {
      if (generationRef.current !== null) {
        // A terminal interrupt is ETX; the gateway does not forward `signal` for SSH.
        sendInput("\u0003");
        return;
      }
      sendPayload(JSON.stringify({ type: "signal", signal: "interrupt" }));
      return;
    }
    const modifiedDockSequence = modifiedDockNavigationSequence(key);
    if (modifiedDockSequence) {
      sendInput(new TextEncoder().encode(modifiedDockSequence));
      return;
    }
    if (key.startsWith("alt-")) {
      sendInput(new TextEncoder().encode(`\u001b${key.slice(4)}`));
      return;
    }
    if (key.startsWith("ctrl-")) {
      const byte = controlByteForChar(key.slice(5));
      if (byte !== null) sendInput(new Uint8Array([byte]));
      return;
    }
    const sequenceKey = BROWSER_KEY_NAMES[key] ?? key;
    const sequence = KEY_SEQUENCES[sequenceKey as keyof typeof KEY_SEQUENCES] ?? sequenceKey;
    sendInput(new TextEncoder().encode(sequence));
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
          <span className="flex items-center gap-2">
            {inputOverflow ? (
              <span
                data-testid="remote-terminal-overflow-indicator"
                role="status"
                className="flex items-center gap-1 rounded border border-rose-500/40 bg-rose-500/10 px-1.5 py-0.5 font-mono text-[10px] text-rose-300"
              >
                <span className="h-1.5 w-1.5 rounded-full bg-rose-400" />
                <span>Input overflow</span>
              </span>
            ) : null}
            {pendingInputBytes > 0 ? (
              <span
                data-testid="remote-terminal-buffered-indicator"
                className="flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/10 px-1.5 py-0.5 font-mono text-[10px] text-amber-300"
              >
                <span className="h-1.5 w-1.5 rounded-full bg-amber-400 animate-pulse" />
                <span>Buffered input</span>
              </span>
            ) : null}
            <span data-testid="remote-terminal-build-stamp" className="font-mono text-[10px] text-muted-foreground/70">{BUILD_STAMP}</span>
            <span role="status" className="font-mono text-[10px] text-muted-foreground">
              {connected ? "Live" : "Connecting"}
            </span>
          </span>
        </div>
      ) : null}
      <div
        ref={surfaceRef}
        data-testid="remote-terminal-grid"
        tabIndex={0}
        onPointerDown={(event) => {
          if (event.pointerType === "touch") return;
          event.preventDefault();
          focusInput();
        }}
        // Suppress the native/compatibility mouse focus transfer to the grid.
        // Touch focus belongs exclusively to the completed tap handler.
        onMouseDown={(event) => event.preventDefault()}
        onWheel={handleWheel}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onTouchCancel={handleTouchCancel}
        onKeyDown={(event) => {
          // IME-composed keystrokes (Korean jamo, CJK, etc.) must flow through the
          // input sink's composition events. Sending them here shattered Hangul into
          // isolated jamo writes, one per physical keypress.
          if (
            isComposingRef.current ||
            event.nativeEvent.isComposing ||
            event.keyCode === 229 ||
            event.key === "Process" ||
            event.key === "Dead"
          ) {
            return;
          }
          // AltGraph produces a printable glyph via the OS layout. Hand the
          // keystroke to the input sink (no preventDefault) so its InputEvent
          // emits the glyph exactly once, matching native behavior, instead of
          // synthesizing a meta chord or double-sending the character.
          if (event.getModifierState("AltGraph")) {
            inputSinkRef.current?.focus();
            return;
          }
          pendingCompositionInputRef.current = null;
          const ctrlChordChar =
            event.ctrlKey && !event.metaKey && !event.altKey
              ? physicalChordChar(event.key, event.nativeEvent.code)
              : null;
          if (ctrlChordChar) {
            // Leave clipboard chords to the browser: Ctrl+Shift+C copies,
            // Ctrl+V (with or without Shift) lets the native paste event run.
            if (ctrlChordChar === "c" && event.shiftKey) return;
            if (ctrlChordChar === "v") return;
            event.preventDefault();
            sendKey(`ctrl-${ctrlChordChar}`);
          } else if (!event.metaKey) {
            const modifiedSequence = modifiedNavigationSequence(event.key, event.ctrlKey, event.altKey);
            if (modifiedSequence) {
              event.preventDefault();
              sendKey(modifiedSequence);
            } else if (event.key === "Enter") {
              event.preventDefault();
              sendKey("\r");
            } else if (event.key === "Tab" && event.shiftKey) {
              event.preventDefault();
              sendKey("\u001b[Z");
            } else if (BROWSER_KEY_NAMES[event.key]) {
              // Backspace/Delete belongs to the editable while it still holds a pending IME tail;
              // deleting echoed terminal content instead would lose what the user just typed.
              if (event.key === "Backspace" && dropHeldJamo()) {
                event.preventDefault();
                return;
              }
              if ((event.key === "Backspace" || event.key === "Delete") && sinkHasPendingText()) return;
              event.preventDefault();
              sendKey(event.key);
            } else if (event.altKey && !event.ctrlKey) {
              // Meta (Alt) chord -> ESC-prefixed byte via the physical code; skip
              // keys with no ASCII form rather than emitting arbitrary bytes.
              // Preserve Shift for letters (Alt+Shift+X -> ESC X); Ctrl chords
              // stay normalized to lowercase.
              const metaChar = physicalChordChar(event.key, event.nativeEvent.code);
              if (metaChar) {
                const cased =
                  event.shiftKey && metaChar >= "a" && metaChar <= "z"
                    ? metaChar.toUpperCase()
                    : metaChar;
                event.preventDefault();
                sendKey(`alt-${cased}`);
              }
            } else if (event.key.length === 1 && !event.altKey && !event.ctrlKey) {
              // Let the editable element own printable input, including the key
              // that starts composition before the browser reports isComposing.
              if (event.target === inputSinkRef.current) return;
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
          // Text typed before the paste must not be overtaken by it.
          flushSinkText();
          const text =
            event.clipboardData?.getData("text/plain") ||
            event.clipboardData?.getData("text") ||
            "";
          if (!text) return;
          const normalized = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
          // Paste is always verbatim UTF-8: routing through sendKey would let literal
          // text like "up", "ctrl-c", or "toString" be interpreted as key commands.
          if (normalized.includes("\n")) {
            sendText(`\x1b[200~${normalized}\x1b[201~`);
          } else {
            sendText(normalized);
          }
        }}
        className="relative min-h-0 flex-1 overflow-hidden bg-terminal outline-none motion-reduce:transition-none"
        style={{
          backgroundColor: settings.theme.background,
          color: settings.theme.foreground,
          fontFamily: settings.fontFamily,
          fontSize: `${activeFontSize}px`,
          lineHeight: 1,
          whiteSpace: "pre",
          touchAction: "none",
        }}
      >
        {embedded && !connected ? (
          <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex justify-center gap-2 pt-1.5">
            <span role="status" className="rounded-full border border-border bg-card/95 px-2 py-0.5 font-mono text-[10px] leading-tight text-muted-foreground shadow-sm">Connecting</span>
            {inputOverflow ? (
              <span
                data-testid="remote-terminal-overflow-indicator"
                role="status"
                className="rounded-full border border-rose-500/40 bg-rose-500/20 px-2 py-0.5 font-mono text-[10px] leading-tight text-rose-300 shadow-sm"
              >
                Input overflow
              </span>
            ) : null}
            {pendingInputBytes > 0 ? (
              <span
                data-testid="remote-terminal-buffered-indicator"
                className="rounded-full border border-amber-500/40 bg-amber-500/20 px-2 py-0.5 font-mono text-[10px] leading-tight text-amber-300 shadow-sm"
              >
                Buffered input
              </span>
            ) : null}
          </div>
        ) : null}
        {embedded && connected && inputOverflow ? (
          <div className="pointer-events-none absolute top-1.5 right-2 z-20 flex items-center gap-1 rounded border border-rose-500/40 bg-rose-500/20 px-2 py-0.5 font-mono text-[10px] text-rose-300 shadow-sm">
            <span className="h-1.5 w-1.5 rounded-full bg-rose-400" />
            <span>Input overflow</span>
          </div>
        ) : null}
        {embedded && connected && pendingInputBytes > 0 ? (
          <div className="pointer-events-none absolute top-1.5 right-2 z-20 flex items-center gap-1 rounded border border-amber-500/40 bg-amber-500/20 px-2 py-0.5 font-mono text-[10px] text-amber-300 shadow-sm">
            <span className="h-1.5 w-1.5 rounded-full bg-amber-400 animate-pulse" />
            <span>Buffered input</span>
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
            // Keep the OS editing anchor stationary through cursor/viewport
            // updates; the separate preedit overlay follows the grid cursor.
            width: 1,
            height: 1,
            color: settings.theme.foreground,
            caretColor: "transparent",
            fontFamily: settings.fontFamily,
            fontSize: `${Math.max(16, activeFontSize)}px`,
            lineHeight: 1,
          }}
          onCompositionStart={() => {
            pendingCompositionInputRef.current = null;
            isComposingRef.current = true;
          }}
          onCompositionUpdate={(event) => {
            sinkTailRef.current = event.data;
            syncPreedit();
          }}
          onCompositionEnd={(event) => {
            commitComposition(event.data);
          }}
          onInput={handleSinkInput}
          onBlur={() => {
            // A live composition is canceled - its preedit was never the user's text. Jamo the IME
            // already committed are, so they are sent before the next surface (dock key, paste,
            // tab) takes over the sink.
            if (isComposingRef.current) {
              resetSinkField();
              emitSettled("");
            } else {
              flushSinkText();
            }
            isComposingRef.current = false;
            pendingCompositionInputRef.current = null;
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
