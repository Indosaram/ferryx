import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useNativeTerminalVisibilityState } from "../lib/nativeTerminalVisibility";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import { toIpcError } from "../lib/tauri";
import {
  mapClientPointToFrame,
  onRdDisplayRefreshChanged,
  rdConnect,
  rdDisconnect,
  rdDisplayRefresh,
  rdErrorMessage,
  rdInput,
  rdSetAudio,
  rdSetOptions,
  rdSurfaceAttach,
  rdSurfaceDetach,
  rdSurfaceUpdateBounds,
  RdInputSender,
  rdMouseButton,
  wheelDeltaPixels,
  type RdConnectOptions,
  type RdConnectionState,
  type RdCursorFrame,
  type RdFrameMessage,
  type RdInputEvent,
  type RdLogicalBounds,
  type RdPlayoutMode,
  type RdScaleMode,
  type RdSurfaceAttachResponse,
  type RdVideoFrame,
} from "../lib/remoteDesktop";
import { RemoteDesktopStatsOverlay } from "./RemoteDesktopStatsOverlay";
import { readRdPreferences, RemoteDesktopToolbar, writeRdPreference } from "./RemoteDesktopToolbar";
import { Button } from "./ui/button";

export type RemoteDesktopPaneProps = {
  hostId: string;
  label: string;
  /** False while the owning tab is hidden. */
  visible: boolean;
  /** True while pane drop feedback covers this pane; input is suspended so the drop wins. */
  occluded?: boolean;
};

type PaneStatus =
  | { phase: RdConnectionState; reason: string | null }
  | { phase: "error"; message: string };

/** Which presenter owns this pane's pixels: the native wgpu surface or the in-DOM Canvas 2D. */
type RdPresentBackend = "wgpu" | "canvas2d";

/**
 * Delay before a layout change is reported to the host as a new physical viewport. Every report
 * rescales the remote stream, so a drag-resize must settle first. Bounds are never debounced:
 * the native surface has to track the pane continuously.
 */
const VIEWPORT_OPTIONS_DEBOUNCE_MS = 150;

/** Largest remote cursor image turned into a CSS cursor; browsers ignore bigger ones anyway. */
const MAX_CURSOR_EDGE_PX = 256;

const STATUS_LABEL: Record<RdConnectionState, string> = {
  connecting: "Connecting…",
  connected: "Connected",
  reconnecting: "Reconnecting…",
  ended: "Session ended",
};

/**
 * The host command takes `Option<LogicalBounds>`: `null` hides the native child surface without
 * claiming any geometry, which is how a modal or a hidden tab takes the pixels back.
 */
const updateSurfaceBounds = rdSurfaceUpdateBounds as (
  rdSessionId: string,
  bounds: RdLogicalBounds | null,
  visible: boolean,
) => Promise<void>;

/** `Ctrl+Shift+Alt+S` toggles the stats HUD and is never forwarded to the remote host. */
function isStatsShortcut(event: KeyboardEvent<HTMLElement>): boolean {
  return event.ctrlKey && event.shiftKey && event.altKey && !event.metaKey && event.nativeEvent.code === "KeyS";
}

let crcTable: Uint32Array | null = null;

function crc32(bytes: Uint8Array, start: number, end: number): number {
  if (!crcTable) {
    crcTable = new Uint32Array(256);
    for (let n = 0; n < 256; n += 1) {
      let c = n;
      for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      crcTable[n] = c >>> 0;
    }
  }
  let crc = 0xffffffff;
  for (let index = start; index < end; index += 1) crc = crcTable[(crc ^ bytes[index]) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

/**
 * Encodes straight RGBA pixels as a PNG using stored (uncompressed) deflate blocks. Cursor images
 * are a few KiB, so skipping compression keeps this synchronous, canvas-free and dependency-free.
 */
function encodeRgbaPng(width: number, height: number, rgba: Uint8Array) {
  const rowBytes = width * 4 + 1;
  const raw = new Uint8Array(rowBytes * height);
  for (let row = 0; row < height; row += 1) {
    raw[row * rowBytes] = 0;
    raw.set(rgba.subarray(row * width * 4, (row + 1) * width * 4), row * rowBytes + 1);
  }
  const blockCount = Math.max(1, Math.ceil(raw.length / 0xffff));
  const zlib = new Uint8Array(2 + blockCount * 5 + raw.length + 4);
  zlib[0] = 0x78;
  zlib[1] = 0x01;
  let offset = 2;
  for (let block = 0; block < blockCount; block += 1) {
    const chunk = raw.subarray(block * 0xffff, Math.min(raw.length, (block + 1) * 0xffff));
    zlib[offset] = block === blockCount - 1 ? 1 : 0;
    zlib[offset + 1] = chunk.length & 0xff;
    zlib[offset + 2] = chunk.length >>> 8;
    zlib[offset + 3] = ~chunk.length & 0xff;
    zlib[offset + 4] = (~chunk.length >>> 8) & 0xff;
    zlib.set(chunk, offset + 5);
    offset += 5 + chunk.length;
  }
  let a = 1;
  let b = 0;
  for (const byte of raw) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  new DataView(zlib.buffer).setUint32(offset, ((b << 16) | a) >>> 0);

  const chunks: Array<[string, Uint8Array]> = [];
  const header = new Uint8Array(13);
  const headerView = new DataView(header.buffer);
  headerView.setUint32(0, width);
  headerView.setUint32(4, height);
  header.set([8, 6, 0, 0, 0], 8);
  chunks.push(["IHDR", header], ["IDAT", zlib], ["IEND", new Uint8Array(0)]);

  const total = 8 + chunks.reduce((sum, [, data]) => sum + 12 + data.length, 0);
  const png = new Uint8Array(total);
  const view = new DataView(png.buffer);
  png.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], 0);
  let cursor = 8;
  for (const [type, data] of chunks) {
    view.setUint32(cursor, data.length);
    for (let index = 0; index < 4; index += 1) png[cursor + 4 + index] = type.charCodeAt(index);
    png.set(data, cursor + 8);
    view.setUint32(cursor + 8 + data.length, crc32(png, cursor + 4, cursor + 8 + data.length));
    cursor += 12 + data.length;
  }
  return png;
}

/**
 * Turns a cursor frame's RGBA shape into an object URL, or null when the payload is malformed or
 * the runtime cannot mint object URLs (the pane then keeps the browser's default cursor).
 */
function cursorShapeUrl(message: RdCursorFrame): string | null {
  const { rgbaBase64, width, height } = message;
  if (!rgbaBase64 || !width || !height) return null;
  if (width > MAX_CURSOR_EDGE_PX || height > MAX_CURSOR_EDGE_PX) return null;
  if (typeof URL === "undefined" || typeof URL.createObjectURL !== "function" || typeof Blob === "undefined") return null;
  let binary: string;
  try {
    binary = atob(rgbaBase64);
  } catch {
    return null;
  }
  if (binary.length !== width * height * 4) return null;
  const rgba = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) rgba[index] = binary.charCodeAt(index);
  return URL.createObjectURL(new Blob([encodeRgbaPng(width, height, rgba)], { type: "image/png" }));
}

function currentWindowLabel(): string | null {
  if (typeof window === "undefined") return null;
  const internals = (window as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: unknown } } } })
    .__TAURI_INTERNALS__;
  const label = internals?.metadata?.currentWindow?.label;
  return typeof label === "string" ? label : null;
}

type CursorShape = { shapeId: number | null; url: string | null; css: string | undefined };

/**
 * Viewer of one paired host's desktop. The host prefers a native wgpu surface
 * (`cmd_rd_surface_attach` answers `backend: "wgpu"`), which draws beneath the transparent
 * macOS WebView; when that is unavailable the pane falls back to JPEG frames from the
 * `cmd_rd_connect` channel drawn on a canvas scaled aspect-fit. Either way pointer/keyboard
 * input is mapped to normalized frame coordinates, or sent as relative motion while Pointer Lock
 * holds the mouse. A modal dialog suspends all input; on Windows/Linux it also hides the native
 * child surface, which would otherwise paint over the dialog. The viewer session lives only as
 * long as this component; unmount detaches the surface and disconnects it.
 */
export function RemoteDesktopPane({ hostId, label, visible, occluded = false }: RemoteDesktopPaneProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const senderRef = useRef<RdInputSender | null>(null);
  const frameSizeRef = useRef<{ width: number; height: number } | null>(null);
  const pendingFrameRef = useRef<RdVideoFrame | null>(null);
  const decodingRef = useRef(false);
  const rafIdRef = useRef<number | null>(null);
  const pressedKeysRef = useRef(new Set<string>());
  const sessionIdRef = useRef<string | null>(null);
  const presentBackendRef = useRef<RdPresentBackend>("canvas2d");
  const visibleRef = useRef(visible);
  const surfaceVisibleRef = useRef(visible && !occluded);
  const reportedViewportRef = useRef("");
  const optionsTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleFrameDrawRef = useRef<() => void>(() => undefined);
  const cursorShapeRef = useRef<CursorShape>({ shapeId: null, url: null, css: undefined });
  const [initialPreferences] = useState(readRdPreferences);
  const [playoutMode, setPlayoutMode] = useState<RdPlayoutMode>(initialPreferences.playoutMode);
  const [scaleMode, setScaleMode] = useState<RdScaleMode>(initialPreferences.scaleMode);
  const [audioMuted, setAudioMuted] = useState(initialPreferences.audioMuted);
  const [audioVolume, setAudioVolume] = useState(initialPreferences.audioVolume);
  const [statsOpen, setStatsOpen] = useState(initialPreferences.statsOpen);
  const connectOptionsRef = useRef({ playoutMode, scaleMode, audioMuted, audioVolume });
  const [refreshHz, setRefreshHz] = useState<number | null>(null);
  const [pointerLocked, setPointerLocked] = useState(false);
  const [remoteCursor, setRemoteCursor] = useState<string | undefined>(undefined);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [presentBackend, setPresentBackend] = useState<RdPresentBackend>("canvas2d");
  const [nativePresented, setNativePresented] = useState(false);
  const [status, setStatus] = useState<PaneStatus>({ phase: "connecting", reason: null });
  const [hasFrame, setHasFrame] = useState(false);
  const [attempt, setAttempt] = useState(0);

  const visibilityState = useNativeTerminalVisibilityState();
  const isMac = isMacShortcutPlatform();
  /** False while the tab is hidden, a drop covers the pane, or a modal owns input. */
  const paneInteractive = visible && !occluded && visibilityState.interactive;
  /**
   * macOS keeps the native view beneath the transparent WKWebView, so a modal paints over it and
   * the surface may stay up. Elsewhere the child window sits above the WebView and must hide.
   */
  const surfaceVisible = visible && !occluded && (isMac || visibilityState.visible);

  useEffect(() => {
    connectOptionsRef.current = { playoutMode, scaleMode, audioMuted, audioVolume };
  }, [playoutMode, scaleMode, audioMuted, audioVolume]);

  /**
   * Pane geometry in logical pixels. The native surface is placed from this rect, so it is
   * measured from the pane container rather than from the (aspect-fit) canvas bitmap.
   */
  const measureBounds = useCallback((): RdLogicalBounds => {
    const rect = containerRef.current?.getBoundingClientRect();
    const scaleFactor =
      typeof window !== "undefined" && typeof window.devicePixelRatio === "number" && window.devicePixelRatio > 0
        ? window.devicePixelRatio
        : 1;
    return {
      x: rect?.left ?? 0,
      y: rect?.top ?? 0,
      width: rect?.width ?? 0,
      height: rect?.height ?? 0,
      scaleFactor,
    };
  }, []);

  /**
   * Pushes the pane's current geometry to the native surface. Bounds go out on every layout
   * change so the surface tracks the pane; the host-facing viewport options are debounced and
   * deduplicated because each one rescales the remote stream. A surface that must not paint is
   * hidden with `null` bounds and no viewport report.
   */
  const reportSurfaceGeometry = useCallback(() => {
    const rdSessionId = sessionIdRef.current;
    if (!rdSessionId || presentBackendRef.current !== "wgpu") return;
    if (!surfaceVisibleRef.current) {
      void updateSurfaceBounds(rdSessionId, null, false).catch((error: unknown) => {
        console.warn("Remote desktop surface hide failed", error);
      });
      return;
    }
    const bounds = measureBounds();
    void updateSurfaceBounds(rdSessionId, bounds, true).catch((error: unknown) => {
      console.warn("Remote desktop surface bounds update failed", error);
    });
    const viewportWidth = Math.max(1, Math.round(bounds.width * bounds.scaleFactor));
    const viewportHeight = Math.max(1, Math.round(bounds.height * bounds.scaleFactor));
    const viewportKey = `${viewportWidth}x${viewportHeight}`;
    if (viewportKey === reportedViewportRef.current) return;
    if (optionsTimerRef.current !== null) clearTimeout(optionsTimerRef.current);
    optionsTimerRef.current = setTimeout(() => {
      optionsTimerRef.current = null;
      if (sessionIdRef.current !== rdSessionId || presentBackendRef.current !== "wgpu") return;
      if (viewportKey === reportedViewportRef.current) return;
      reportedViewportRef.current = viewportKey;
      void rdSetOptions(rdSessionId, { viewportWidth, viewportHeight }).catch((error: unknown) => {
        console.warn("Remote desktop viewport options update failed", error);
      });
    }, VIEWPORT_OPTIONS_DEBOUNCE_MS);
  }, [measureBounds]);

  const cancelPendingFrame = useCallback(() => {
    if (rafIdRef.current === null) return;
    cancelAnimationFrame(rafIdRef.current);
    rafIdRef.current = null;
  }, []);

  /**
   * The single rAF pump of the Canvas 2D path: it pops whatever frame is newest at draw time,
   * so a burst of JPEG frames costs one decode and one paint instead of one per frame.
   */
  const drawPendingFrame = useCallback(() => {
    rafIdRef.current = null;
    if (presentBackendRef.current !== "canvas2d" || !visibleRef.current) return;
    // A decode in flight re-schedules from its own completion; the newest frame still wins.
    if (decodingRef.current) return;
    const frame = pendingFrameRef.current;
    if (!frame) return;
    pendingFrameRef.current = null;
    if (typeof createImageBitmap !== "function") return;
    decodingRef.current = true;
    void createImageBitmap(new Blob([frame.jpeg.slice()], { type: "image/jpeg" }))
      .then((bitmap) => {
        const canvas = canvasRef.current;
        if (canvas) {
          if (canvas.width !== frame.width) canvas.width = frame.width;
          if (canvas.height !== frame.height) canvas.height = frame.height;
          canvas.getContext("2d")?.drawImage(bitmap, 0, 0, frame.width, frame.height);
          frameSizeRef.current = { width: frame.width, height: frame.height };
          setHasFrame(true);
        }
        bitmap.close();
      })
      .catch((error: unknown) => {
        console.warn("Remote desktop frame decode failed", error);
      })
      .finally(() => {
        decodingRef.current = false;
        if (pendingFrameRef.current) scheduleFrameDrawRef.current();
      });
  }, []);

  const scheduleFrameDraw = useCallback(() => {
    if (rafIdRef.current !== null) return;
    if (presentBackendRef.current !== "canvas2d" || !visibleRef.current) return;
    rafIdRef.current = requestAnimationFrame(() => drawPendingFrame());
  }, [drawPendingFrame]);

  // The decode completion path cannot reference `scheduleFrameDraw` directly without a cycle,
  // so it reads the current pump through this ref. Assigned before the connect effect runs so a
  // frame that lands on the first tick still schedules.
  useEffect(() => {
    scheduleFrameDrawRef.current = scheduleFrameDraw;
  }, [scheduleFrameDraw]);

  /** Drops the cached remote cursor image; its object URL is revoked so nothing leaks. */
  const resetCursorShape = useCallback(() => {
    const previous = cursorShapeRef.current;
    if (previous.url) URL.revokeObjectURL(previous.url);
    cursorShapeRef.current = { shapeId: null, url: null, css: undefined };
  }, []);

  /**
   * Applies one cursor frame. A shape is cached by `shapeId`, so the per-move frames that repeat
   * it reuse the existing URL; only a new shape mints a URL, revoking the one it replaces.
   */
  const applyCursorFrame = useCallback((message: RdCursorFrame) => {
    const cached = cursorShapeRef.current;
    if (message.shapeId !== null && message.shapeId === cached.shapeId) {
      setRemoteCursor(message.visible ? cached.css : "none");
      return;
    }
    const url = cursorShapeUrl(message);
    if (url !== null || message.shapeId !== cached.shapeId) {
      resetCursorShape();
      if (url !== null) {
        const width = message.width ?? 1;
        const height = message.height ?? 1;
        const hotspotX = Math.min(width - 1, Math.max(0, Math.round(message.hotspotX ?? 0)));
        const hotspotY = Math.min(height - 1, Math.max(0, Math.round(message.hotspotY ?? 0)));
        cursorShapeRef.current = {
          shapeId: message.shapeId,
          url,
          css: `url("${url}") ${hotspotX} ${hotspotY}, auto`,
        };
      } else {
        cursorShapeRef.current = { shapeId: message.shapeId, url: null, css: undefined };
      }
    }
    setRemoteCursor(message.visible ? cursorShapeRef.current.css : "none");
  }, [resetCursorShape]);

  useEffect(() => resetCursorShape, [resetCursorShape]);

  useEffect(() => {
    let cancelled = false;
    let rdSessionId: string | null = null;
    // Captured here so the cleanup closure below does not read the ref after it may have changed.
    const pressedKeys = pressedKeysRef.current;
    setStatus({ phase: "connecting", reason: null });
    setPresentBackend("canvas2d");
    setNativePresented(false);
    setHasFrame(false);
    setSessionId(null);
    setRemoteCursor(undefined);
    sessionIdRef.current = null;
    frameSizeRef.current = null;
    reportedViewportRef.current = "";
    const onFrame = (message: RdFrameMessage) => {
      if (cancelled) return;
      if (message.kind === "status") {
        setStatus({ phase: message.state, reason: message.reason });
        return;
      }
      if (message.kind === "nativePresented") {
        // B2: every presentation message carries the geometry the surface is showing, so a live
        // rescale, reconnect or reattach refreshes the input mapping instead of freezing it on
        // the first frame's aspect ratio.
        frameSizeRef.current = { width: message.width, height: message.height };
        setNativePresented(true);
        return;
      }
      if (message.kind === "cursor") {
        applyCursorFrame(message);
        return;
      }
      // Only the newest frame is kept; the rAF pump draws it at most once per display frame.
      pendingFrameRef.current = message;
      scheduleFrameDrawRef.current();
    };
    const preferences = connectOptionsRef.current;
    const options: RdConnectOptions = { ...preferences };
    const bounds = measureBounds();
    if (bounds.width > 0 && bounds.height > 0) {
      options.viewportWidth = Math.max(1, Math.round(bounds.width * bounds.scaleFactor));
      options.viewportHeight = Math.max(1, Math.round(bounds.height * bounds.scaleFactor));
    }
    rdConnect(hostId, onFrame, (error) => console.warn("Remote desktop frame rejected", error), options).then(
      (result) => {
        if (cancelled) {
          void rdDisconnect(result.rdSessionId).catch(() => undefined);
          return;
        }
        rdSessionId = result.rdSessionId;
        sessionIdRef.current = result.rdSessionId;
        setSessionId(result.rdSessionId);
        senderRef.current = new RdInputSender(result.rdSessionId, undefined, (error) => {
          console.warn("Remote desktop input failed", error);
        });
        void rdSurfaceAttach(result.rdSessionId, measureBounds()).then(
          (surface: RdSurfaceAttachResponse | undefined) => {
            if (cancelled) return;
            // A declined native surface is not a failure: Canvas 2D is the documented fallback,
            // so it switches over silently and never raises a banner.
            presentBackendRef.current = surface?.backend === "wgpu" ? "wgpu" : "canvas2d";
            setPresentBackend(presentBackendRef.current);
          },
          (error: unknown) => {
            if (cancelled) return;
            console.warn("Remote desktop native surface unavailable, using Canvas 2D", error);
            presentBackendRef.current = "canvas2d";
            setPresentBackend("canvas2d");
          },
        );
      },
      (error: unknown) => {
        if (cancelled) return;
        setStatus({ phase: "error", message: rdErrorMessage(toIpcError(error)) });
      },
    );
    return () => {
      cancelled = true;
      cancelPendingFrame();
      pendingFrameRef.current = null;
      if (optionsTimerRef.current !== null) {
        clearTimeout(optionsTimerRef.current);
        optionsTimerRef.current = null;
      }
      reportedViewportRef.current = "";
      sessionIdRef.current = null;
      presentBackendRef.current = "canvas2d";
      // A key held at unmount would stay down on the remote host for the life of that session.
      // The release goes out directly: the sender is about to close and may still be mid-flight,
      // where a queued event would be dropped with it.
      const heldKeys = [...pressedKeys];
      pressedKeys.clear();
      if (rdSessionId && heldKeys.length > 0) {
        void rdInput(rdSessionId, heldKeys.map((code) => ({ kind: "key" as const, code, down: false }))).catch(
          (error: unknown) => {
            console.warn("Remote desktop key release failed", error);
          },
        );
      }
      senderRef.current?.close();
      senderRef.current = null;
      resetCursorShape();
      if (rdSessionId) {
        const session = rdSessionId;
        // Detach first: the surface must be released while the session that owns it is alive.
        void rdSurfaceDetach(session)
          .catch((error: unknown) => {
            console.warn("Remote desktop surface detach failed", error);
          })
          .finally(() => rdDisconnect(session).catch((error: unknown) => {
            console.warn("Remote desktop disconnect failed", error);
          }));
      }
    };
  }, [hostId, attempt, measureBounds, cancelPendingFrame, applyCursorFrame, resetCursorShape]);

  // The refresh badge follows the monitor this window is on, including live mode switches.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void rdDisplayRefresh().then(
      (info) => {
        if (!cancelled && info && Number.isFinite(info.refreshHz)) setRefreshHz(info.refreshHz);
      },
      () => undefined,
    );
    const windowLabel = currentWindowLabel();
    void onRdDisplayRefreshChanged((change) => {
      if (cancelled) return;
      if (windowLabel !== null && change.windowLabel !== windowLabel) return;
      setRefreshHz(change.refreshHz);
    }).then(
      (stop) => {
        if (typeof stop !== "function") return;
        if (cancelled) stop();
        else unlisten = stop;
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // A channel message arrives outside React's render cycle, so the frame pump reads the pane's
  // current backend and visibility from refs instead of a stale effect closure.
  useEffect(() => {
    visibleRef.current = visible;
    surfaceVisibleRef.current = surfaceVisible;
    presentBackendRef.current = presentBackend;
  }, [visible, surfaceVisible, presentBackend]);

  // Zero idle rAF burn: nothing may stay armed while frames are not being drawn.
  useEffect(() => {
    if (presentBackend === "canvas2d" && visible) return;
    cancelPendingFrame();
    if (presentBackend === "wgpu") pendingFrameRef.current = null;
  }, [presentBackend, visible, cancelPendingFrame]);

  // Visibility is a surface contract, not only an input gate: a hidden tab, a drop over the pane
  // or (off macOS) a modal must hide the native surface immediately, before any resize arrives.
  useEffect(() => {
    reportSurfaceGeometry();
  }, [surfaceVisible, presentBackend, reportSurfaceGeometry]);

  // The native surface tracks layout through ResizeObserver and monitor-DPR changes through
  // `resize` (ResizeObserver does not fire when only the pixel density changes).
  useEffect(() => {
    if (!sessionId || presentBackend !== "wgpu") return;
    const container = containerRef.current;
    let observer: ResizeObserver | null = null;
    if (container && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => reportSurfaceGeometry());
      observer.observe(container);
    }
    window.addEventListener("resize", reportSurfaceGeometry);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", reportSurfaceGeometry);
    };
  }, [sessionId, presentBackend, reportSurfaceGeometry]);

  const connected = status.phase === "connected";
  const inputEnabled = connected && paneInteractive;

  const send = useCallback((event: RdInputEvent) => {
    senderRef.current?.push(event);
  }, []);

  const sendPointerPosition = useCallback((clientX: number, clientY: number): boolean => {
    const canvas = canvasRef.current;
    const size = frameSizeRef.current;
    if (!canvas || !size) return false;
    const point = mapClientPointToFrame(clientX, clientY, canvas.getBoundingClientRect(), size.width, size.height);
    if (!point) return false;
    send({ kind: "mouseMove", x: point.x, y: point.y });
    return true;
  }, [send]);

  // React registers wheel listeners as passive, so preventDefault needs a native listener.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !inputEnabled) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const page = canvas.clientHeight || 800;
      send({
        kind: "wheel",
        dx: wheelDeltaPixels(event.deltaX, event.deltaMode, page),
        dy: wheelDeltaPixels(event.deltaY, event.deltaMode, page),
      });
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [inputEnabled, send]);

  const releasePressedKeys = useCallback(() => {
    for (const code of pressedKeysRef.current) send({ kind: "key", code, down: false });
    pressedKeysRef.current.clear();
  }, [send]);

  const exitPointerLock = useCallback(() => {
    const canvas = canvasRef.current;
    if (typeof document === "undefined" || !canvas || document.pointerLockElement !== canvas) return;
    document.exitPointerLock?.();
  }, []);

  const requestPointerLock = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas || !inputEnabled) return;
    canvas.focus({ preventScroll: true });
    try {
      const result: unknown = canvas.requestPointerLock?.();
      if (result instanceof Promise) {
        result.catch((error: unknown) => {
          console.warn("Remote desktop pointer lock was refused", error);
        });
      }
    } catch (error) {
      console.warn("Remote desktop pointer lock was refused", error);
    }
  }, [inputEnabled]);

  // Pointer Lock state is owned by the browser: a lock lost for any reason (Escape, focus loss,
  // a system dialog) also lifts every held key so nothing sticks on the remote host.
  useEffect(() => {
    if (typeof document === "undefined") return;
    const sync = () => {
      const locked = canvasRef.current !== null && document.pointerLockElement === canvasRef.current;
      setPointerLocked(locked);
      if (!locked) releasePressedKeys();
    };
    const onError = () => setPointerLocked(false);
    document.addEventListener("pointerlockchange", sync);
    document.addEventListener("pointerlockerror", onError);
    return () => {
      document.removeEventListener("pointerlockchange", sync);
      document.removeEventListener("pointerlockerror", onError);
    };
  }, [releasePressedKeys]);

  useEffect(() => {
    const onWindowBlur = () => {
      exitPointerLock();
      releasePressedKeys();
    };
    window.addEventListener("blur", onWindowBlur);
    return () => window.removeEventListener("blur", onWindowBlur);
  }, [exitPointerLock, releasePressedKeys]);

  // A hidden tab, a drop over the pane, a modal, or a lost session yields the mouse and keyboard.
  useEffect(() => {
    if (inputEnabled) return;
    exitPointerLock();
    releasePressedKeys();
  }, [inputEnabled, exitPointerLock, releasePressedKeys]);

  // Unmounting while locked must hand the mouse back to the app. React detaches refs before
  // passive cleanups run, so the canvas is captured while it is still mounted.
  useEffect(() => {
    const canvas = canvasRef.current;
    return () => {
      if (canvas && typeof document !== "undefined" && document.pointerLockElement === canvas) {
        document.exitPointerLock?.();
      }
    };
  }, []);

  const toggleStats = useCallback(() => {
    setStatsOpen((open) => !open);
  }, []);

  const togglePointerLock = useCallback(() => {
    if (pointerLocked) exitPointerLock();
    else requestPointerLock();
  }, [pointerLocked, exitPointerLock, requestPointerLock]);

  const changePlayoutMode = useCallback(async (mode: RdPlayoutMode) => {
    const rdSessionId = sessionIdRef.current;
    if (!rdSessionId) throw new Error("remote desktop session is not connected");
    await rdSetOptions(rdSessionId, { playoutMode: mode });
    setPlayoutMode(mode);
  }, []);

  const changeScaleMode = useCallback(async (mode: RdScaleMode) => {
    const rdSessionId = sessionIdRef.current;
    if (!rdSessionId) throw new Error("remote desktop session is not connected");
    await rdSetOptions(rdSessionId, { scaleMode: mode });
    setScaleMode(mode);
  }, []);

  const changeAudio = useCallback(async (muted: boolean, volume: number) => {
    const rdSessionId = sessionIdRef.current;
    if (!rdSessionId) throw new Error("remote desktop session is not connected");
    await rdSetAudio(rdSessionId, muted, volume);
    setAudioMuted(muted);
    setAudioVolume(volume);
  }, []);

  const handlePointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.focus({ preventScroll: true });
    if (!inputEnabled) return;
    const button = rdMouseButton(event.button);
    if (button === null) return;
    if (pointerLocked) {
      // A locked pointer has no meaningful absolute position; the host keeps its own.
      event.preventDefault();
      send({ kind: "mouseButton", button, down: true });
      return;
    }
    if (!sendPointerPosition(event.clientX, event.clientY)) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    send({ kind: "mouseButton", button, down: true });
  };

  const handlePointerUp = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!inputEnabled) return;
    const button = rdMouseButton(event.button);
    if (button === null) return;
    if (pointerLocked) {
      send({ kind: "mouseButton", button, down: false });
      return;
    }
    sendPointerPosition(event.clientX, event.clientY);
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    send({ kind: "mouseButton", button, down: false });
  };

  const handlePointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!inputEnabled) return;
    if (pointerLocked) {
      const dx = event.movementX;
      const dy = event.movementY;
      if (Number.isFinite(dx) && Number.isFinite(dy) && (dx !== 0 || dy !== 0)) {
        send({ kind: "relativeMove", dx, dy });
      }
      return;
    }
    sendPointerPosition(event.clientX, event.clientY);
  };

  const handleKey = (event: KeyboardEvent<HTMLCanvasElement>, down: boolean) => {
    if (isStatsShortcut(event)) {
      event.preventDefault();
      event.stopPropagation();
      if (down && !event.nativeEvent.repeat) {
        writeRdPreference("statsOpen", !statsOpen);
        toggleStats();
      }
      return;
    }
    if (!inputEnabled || !event.nativeEvent.code || event.nativeEvent.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    const code = event.nativeEvent.code;
    if (pointerLocked && code === "Escape") {
      // Escape is the user's way out of the lock; it is never forwarded while locked.
      if (down) {
        exitPointerLock();
        releasePressedKeys();
      }
      return;
    }
    if (down) {
      if (event.nativeEvent.repeat && pressedKeysRef.current.has(code)) {
        send({ kind: "key", code, down: true });
        return;
      }
      pressedKeysRef.current.add(code);
    } else {
      pressedKeysRef.current.delete(code);
    }
    send({ kind: "key", code, down });
  };

  const statusText = status.phase === "error"
    ? status.message
    : status.reason && status.phase !== "connected"
      ? `${STATUS_LABEL[status.phase]} ${status.reason}`
      : STATUS_LABEL[status.phase];
  const canRetry = status.phase === "error" || status.phase === "ended";
  // A presented native surface already shows the desktop, so the pane must not cover it with a
  // "Connected" placeholder that only exists because no JPEG frame was ever decoded.
  const showOverlay = status.phase !== "connected" || (!hasFrame && !nativePresented);
  // The interactive canvas stays hit-testable from the moment the session connects: in wgpu
  // mode it is a transparent event surface over the native view and never gets a bitmap.
  const canvasVisible = hasFrame || status.phase === "connected";
  // macOS places the native view beneath a transparent WKWebView, so an opaque pane background
  // would hide it; index.css paints var(--terminal) instead until the surface presents.
  const nativeSurfaceOwnsPixels = presentBackend === "wgpu" && nativePresented;

  return (
    <div className="flex h-full w-full min-h-0 min-w-0 flex-col" data-testid="remote-desktop-viewer">
      <RemoteDesktopToolbar
        rdSessionId={sessionId}
        connected={connected}
        playoutMode={playoutMode}
        scaleMode={scaleMode}
        audioMuted={audioMuted}
        audioVolume={audioVolume}
        statsOpen={statsOpen}
        pointerLocked={pointerLocked}
        refreshHz={refreshHz}
        onPlayoutModeChange={changePlayoutMode}
        onScaleModeChange={changeScaleMode}
        onAudioChange={changeAudio}
        onToggleStats={toggleStats}
        onTogglePointerLock={togglePointerLock}
      />
      <div
        ref={containerRef}
        className={`relative flex min-h-0 min-w-0 flex-1 items-center justify-center overflow-hidden${
          nativeSurfaceOwnsPixels ? "" : " bg-black"
        }`}
        data-testid="remote-desktop-pane"
        data-rd-state={status.phase}
        data-rd-native={presentBackend === "wgpu" ? "true" : "false"}
        data-rd-presented={nativePresented ? "true" : "false"}
        data-rd-interactive={paneInteractive ? "true" : "false"}
        data-rd-surface-visible={surfaceVisible ? "true" : "false"}
        data-rd-pointer-locked={pointerLocked ? "true" : "false"}
        data-rd-cursor={remoteCursor}
      >
        <canvas
          ref={canvasRef}
          tabIndex={0}
          role="img"
          aria-label={`Remote desktop of ${label}`}
          data-testid="remote-desktop-canvas"
          className="h-full w-full object-contain outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          style={{
            pointerEvents: occluded ? "none" : undefined,
            visibility: canvasVisible ? "visible" : "hidden",
            cursor: pointerLocked ? undefined : remoteCursor,
          }}
          onPointerDown={handlePointerDown}
          onPointerUp={handlePointerUp}
          onPointerMove={handlePointerMove}
          onContextMenu={(event) => event.preventDefault()}
          onKeyDown={(event) => handleKey(event, true)}
          onKeyUp={(event) => handleKey(event, false)}
          onBlur={releasePressedKeys}
        />
        <RemoteDesktopStatsOverlay rdSessionId={sessionId} open={statsOpen} visible={surfaceVisible} />
        {showOverlay ? (
          <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 p-4 text-center">
            <p
              role={status.phase === "error" ? "alert" : "status"}
              data-testid="remote-desktop-status"
              className={`text-[12px] ${status.phase === "error" ? "text-destructive" : "text-muted-foreground"}`}
            >
              {statusText}
            </p>
            {canRetry ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="pointer-events-auto"
                onClick={() => setAttempt((value) => value + 1)}
              >
                Reconnect
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
    </div>
  );
}
