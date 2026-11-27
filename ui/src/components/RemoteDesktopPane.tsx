import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import {
  Check,
  Clipboard,
  Keyboard,
  Maximize2,
  Minimize2,
  Monitor,
  Power,
  RefreshCw,
  SlidersHorizontal,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useNativeTerminalVisibilityState } from "../lib/nativeTerminalVisibility";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import { toIpcError } from "../lib/tauri";
import {
  mapClientPointToFrame,
  onRdDisplayRefreshChanged,
  RD_QUALITY_PRESETS,
  RdAudioPlayer,
  rdDisplayRefresh,
  rdErrorMessage,
  rdMouseButton,
  rdRequestKeyframe,
  rdSendClipboard,
  rdSetAudio,
  rdSetAudioMuted,
  rdSetOptions,
  rdSetStreamOptions,
  rdSpecialKey,
  rdSurfaceAttach,
  rdSurfaceDetach,
  rdSurfaceUpdateBounds,
  registerRemoteDesktopShortcutCapture,
  wheelDeltaPixels,
  type RdConnectionState,
  type RdCursorFrame,
  type RdFitMode,
  type RdFrameMessage,
  type RdInputEvent,
  type RdLogicalBounds,
  type RdPlayoutMode,
  type RdQualityPresetId,
  type RdScaleMode,
  type RdSessionMeta,
  type RdSpecialKeyAction,
  type RdSurfaceAttachResponse,
  type RdVideoFrame,
} from "../lib/remoteDesktop";
import {
  remoteDesktopStore,
  useRemoteDesktopSession,
  type RemoteDesktopStatus,
} from "../state/remoteDesktopStore";
import { cn } from "../lib/cn";
import { RemoteDesktopStatsOverlay } from "./RemoteDesktopStatsOverlay";
import { readRdPreferences, RemoteDesktopToolbar, writeRdPreference } from "./RemoteDesktopToolbar";
import { Button } from "./ui/button";
import { Input } from "./ui/input";

export type RemoteDesktopPaneProps = {
  hostId: string;
  label: string;
  /** False while the owning tab is hidden; the session is kept alive but nothing is drawn or sent. */
  visible: boolean;
  /** True while pane drop feedback covers this pane; input is suspended so the drop wins. */
  occluded?: boolean;
};

type OpenMenu = "display" | "keys" | "quality" | "clipboard" | null;

type ToolbarNotice = { tone: "info" | "error"; text: string };

const lastMetaBySessionId = new Map<string, RdSessionMeta>();

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

const SPECIAL_KEY_ITEMS: ReadonlyArray<{ action: RdSpecialKeyAction; label: string; hint?: string }> = [
  { action: "ctrlAltDel", label: "Ctrl + Alt + Del", hint: "SAS" },
  { action: "altTab", label: "Alt + Tab" },
  { action: "cmdTab", label: "Cmd + Tab" },
  { action: "meta", label: "Win / Cmd" },
  { action: "altF4", label: "Alt + F4" },
  { action: "cmdW", label: "Cmd + W" },
  { action: "ctrlW", label: "Ctrl + W" },
  { action: "printScreen", label: "Print Screen" },
  { action: "escape", label: "Esc" },
];

function statusDotClass(status: RemoteDesktopStatus, wireState: RdConnectionState | null): string {
  if (status === "connected") return "bg-status-success";
  if (status === "connecting" || wireState === "reconnecting") return "bg-status-warning";
  if (status === "error") return "bg-destructive";
  return "bg-muted-foreground/40";
}

function formatTelemetry(meta: RdSessionMeta | null, frameSize: { width: number; height: number } | null): string {
  const parts: string[] = [];
  const w = meta?.width || frameSize?.width || 0;
  const h = meta?.height || frameSize?.height || 0;
  if (w > 0 && h > 0) parts.push(`${w}×${h}`);
  if (meta && meta.fps > 0) parts.push(`${meta.fps} fps`);
  if (meta?.rttMs !== null && meta?.rttMs !== undefined) parts.push(`${Math.round(meta.rttMs)} ms`);
  return parts.join(" · ");
}

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
 * Viewer of one paired host's desktop. The session itself belongs to `remoteDesktopStore`, so
 * mounting, hiding or unmounting this pane only attaches or detaches the presenter: the connection
 * survives worktree and tab switches, and only the explicit Disconnect button tears it down.
 *
 * The host prefers a native wgpu surface (`cmd_rd_surface_attach` answers `backend: "wgpu"`), which
 * draws beneath the transparent macOS WebView; when that is unavailable the pane falls back to JPEG
 * frames from the session channel drawn on a canvas scaled aspect-fit through a single rAF pump.
 * Either way pointer/keyboard input is mapped to normalized frame coordinates, or sent as relative
 * motion while Pointer Lock holds the mouse. A modal dialog suspends all input; on Windows/Linux it
 * also hides the native child surface, which would otherwise paint over the dialog.
 */
export function RemoteDesktopPane({ hostId, label, visible, occluded = false }: RemoteDesktopPaneProps) {
  const session = useRemoteDesktopSession(hostId);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const audioRef = useRef<RdAudioPlayer | null>(null);
  const frameSizeRef = useRef<{ width: number; height: number } | null>(null);
  const pendingFrameRef = useRef<RdVideoFrame | null>(null);
  const decodingRef = useRef(false);
  const rafIdRef = useRef<number | null>(null);
  const pressedKeysRef = useRef(new Set<string>());
  const capturedPointersRef = useRef(new Set<number>());
  const visibleRef = useRef(visible);
  const presentBackendRef = useRef<RdPresentBackend>("canvas2d");
  const surfaceVisibleRef = useRef(visible && !occluded);
  const reportedViewportRef = useRef("");
  const optionsTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleFrameDrawRef = useRef<() => void>(() => undefined);
  const cursorShapeRef = useRef<CursorShape>({ shapeId: null, url: null, css: undefined });
  const pushedPreferencesRef = useRef<string | null>(null);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const rdSessionId = session?.rdSessionId ?? null;

  const [initialPreferences] = useState(readRdPreferences);
  const [playoutMode, setPlayoutMode] = useState<RdPlayoutMode>(initialPreferences.playoutMode);
  const [scaleMode, setScaleMode] = useState<RdScaleMode>(initialPreferences.scaleMode);
  const [audioMuted, setAudioMuted] = useState<boolean>(initialPreferences.audioMuted);
  const [audioVolume, setAudioVolume] = useState<number>(initialPreferences.audioVolume);
  const [statsOpen, setStatsOpen] = useState(initialPreferences.statsOpen);
  const [refreshHz, setRefreshHz] = useState<number | null>(null);
  const [pointerLocked, setPointerLocked] = useState(false);
  const [remoteCursor, setRemoteCursor] = useState<string | undefined>(undefined);
  const [presentBackend, setPresentBackend] = useState<RdPresentBackend>("canvas2d");
  const [nativePresented, setNativePresented] = useState(false);
  const [hasFrame, setHasFrame] = useState(false);
  const [frameSize, setFrameSize] = useState<{ width: number; height: number } | null>(null);
  const [meta, setMeta] = useState<RdSessionMeta | null>(() =>
    rdSessionId ? (lastMetaBySessionId.get(rdSessionId) ?? null) : null,
  );
  const [openMenu, setOpenMenu] = useState<OpenMenu>(null);
  const [fitMode, setFitMode] = useState<RdFitMode>("fit");
  const [qualityPreset, setQualityPreset] = useState<RdQualityPresetId>("balanced");
  const [captureShortcuts, setCaptureShortcuts] = useState(true);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [nativeFullscreen, setNativeFullscreen] = useState(false);
  const [remoteClipboard, setRemoteClipboard] = useState<string | null>(null);
  const [hasUnreadClipboard, setHasUnreadClipboard] = useState(false);
  const [clipboardDraft, setClipboardDraft] = useState("");
  const [notice, setNotice] = useState<ToolbarNotice | null>(null);
  const connectOptionsRef = useRef({ playoutMode, scaleMode, audioMuted, audioVolume });

  const showNotice = useCallback((tone: ToolbarNotice["tone"], text: string) => {
    if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
    setNotice({ tone, text });
    noticeTimerRef.current = setTimeout(() => {
      setNotice(null);
      noticeTimerRef.current = null;
    }, 3500);
  }, []);

  useEffect(() => () => {
    if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
  }, []);

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

  // A channel message arrives outside React's render cycle, so the frame pump and the surface
  // plumbing read the pane's current session, backend and visibility from refs instead of a
  // stale effect closure.
  useEffect(() => {
    sessionIdRef.current = rdSessionId;
    visibleRef.current = visible;
    surfaceVisibleRef.current = surfaceVisible;
    presentBackendRef.current = presentBackend;
  }, [rdSessionId, visible, surfaceVisible, presentBackend]);

  useEffect(() => {
    const audio = new RdAudioPlayer();
    audioRef.current = audio;
    return () => {
      audio.close();
      if (audioRef.current === audio) audioRef.current = null;
    };
  }, [hostId]);

  useEffect(() => {
    if (rdSessionId) {
      const cached = lastMetaBySessionId.get(rdSessionId);
      if (cached) {
        setMeta(cached);
        setAudioMuted(cached.audioMuted);
        audioRef.current?.setMuted(cached.audioMuted);
      }
      return;
    }
    setHasFrame(false);
    setMeta(null);
    setAudioMuted(initialPreferences.audioMuted);
  }, [rdSessionId, initialPreferences.audioMuted]);

  // Attach to the host's session. Reusing an existing connected session is what keeps the
  // connection alive across mounts; nothing here may disconnect.
  useEffect(() => {
    void remoteDesktopStore.connect(hostId, label);
  }, [hostId, label]);

  /**
   * Pane geometry in logical pixels. The native surface is placed from this rect, so it is
   * measured from the viewport container rather than from the (aspect-fit) canvas bitmap.
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
   * Preferences chosen before the session existed (or restored from storage) must reach the host
   * as soon as the session id resolves: the toolbar only re-sends on an explicit user change.
   */
  useEffect(() => {
    if (!rdSessionId || pushedPreferencesRef.current === rdSessionId) return;
    pushedPreferencesRef.current = rdSessionId;
    const preferences = connectOptionsRef.current;
    const bounds = measureBounds();
    const viewport =
      bounds.width > 0 && bounds.height > 0
        ? {
          viewportWidth: Math.max(1, Math.round(bounds.width * bounds.scaleFactor)),
          viewportHeight: Math.max(1, Math.round(bounds.height * bounds.scaleFactor)),
        }
        : {};
    void rdSetOptions(rdSessionId, {
      playoutMode: preferences.playoutMode,
      scaleMode: preferences.scaleMode,
      ...viewport,
    }).catch((error: unknown) => {
      console.warn("Remote desktop initial options failed", error);
    });
    void rdSetAudio(rdSessionId, preferences.audioMuted, preferences.audioVolume).catch((error: unknown) => {
      console.warn("Remote desktop initial audio settings failed", error);
    });
  }, [rdSessionId, measureBounds]);

  /**
   * Pushes the pane's current geometry to the native surface. Bounds go out on every layout
   * change so the surface tracks the pane; the host-facing viewport options are debounced and
   * deduplicated because each one rescales the remote stream. A surface that must not paint is
   * hidden with `null` bounds and no viewport report.
   */
  const reportSurfaceGeometry = useCallback(() => {
    const activeSessionId = sessionIdRef.current;
    if (!activeSessionId || presentBackendRef.current !== "wgpu") return;
    if (!surfaceVisibleRef.current) {
      void updateSurfaceBounds(activeSessionId, null, false).catch((error: unknown) => {
        console.warn("Remote desktop surface hide failed", error);
      });
      return;
    }
    const bounds = measureBounds();
    void updateSurfaceBounds(activeSessionId, bounds, true).catch((error: unknown) => {
      console.warn("Remote desktop surface bounds update failed", error);
    });
    const viewportWidth = Math.max(1, Math.round(bounds.width * bounds.scaleFactor));
    const viewportHeight = Math.max(1, Math.round(bounds.height * bounds.scaleFactor));
    const viewportKey = `${viewportWidth}x${viewportHeight}`;
    if (viewportKey === reportedViewportRef.current) return;
    if (optionsTimerRef.current !== null) clearTimeout(optionsTimerRef.current);
    optionsTimerRef.current = setTimeout(() => {
      optionsTimerRef.current = null;
      if (sessionIdRef.current !== activeSessionId || presentBackendRef.current !== "wgpu") return;
      if (viewportKey === reportedViewportRef.current) return;
      reportedViewportRef.current = viewportKey;
      void rdSetOptions(activeSessionId, { viewportWidth, viewportHeight }).catch((error: unknown) => {
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
        // A hidden pane keeps the session but must not paint: the stale frame is dropped.
        if (canvas && visibleRef.current) {
          if (canvas.width !== frame.width) canvas.width = frame.width;
          if (canvas.height !== frame.height) canvas.height = frame.height;
          canvas.getContext("2d")?.drawImage(bitmap, 0, 0, frame.width, frame.height);
          const prev = frameSizeRef.current;
          frameSizeRef.current = { width: frame.width, height: frame.height };
          if (!prev || prev.width !== frame.width || prev.height !== frame.height) {
            setFrameSize({ width: frame.width, height: frame.height });
          }
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
  // so it reads the current pump through this ref. Assigned before the frame subscription runs
  // so a frame that lands on the first tick still schedules.
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

  // Every frame of the session fans out here: status lives in the store snapshot, everything the
  // presenter needs (video, meta, audio, clipboard, cursor, native presentation) lands here.
  useEffect(() => remoteDesktopStore.subscribeFrames(hostId, (message: RdFrameMessage) => {
    if (message.kind === "status") return;
    if (message.kind === "meta") {
      const sid = sessionIdRef.current;
      if (sid) lastMetaBySessionId.set(sid, message.meta);
      setMeta(message.meta);
      setAudioMuted(message.meta.audioMuted);
      audioRef.current?.setMuted(message.meta.audioMuted);
      return;
    }
    if (message.kind === "audio") {
      if (visibleRef.current) audioRef.current?.push(message);
      return;
    }
    if (message.kind === "clipboard") {
      setRemoteClipboard(message.text);
      setHasUnreadClipboard(true);
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
    if (!visibleRef.current) return;
    // Only the newest frame is kept; the rAF pump draws it at most once per display frame.
    pendingFrameRef.current = message;
    scheduleFrameDrawRef.current();
  }), [hostId, applyCursorFrame]);

  // The native surface is attached for exactly as long as the session is presented here: detaching
  // releases the child window while the session that owns it is still alive.
  useEffect(() => {
    if (!rdSessionId) return;
    let cancelled = false;
    setPresentBackend("canvas2d");
    presentBackendRef.current = "canvas2d";
    setNativePresented(false);
    frameSizeRef.current = null;
    reportedViewportRef.current = "";
    void rdSurfaceAttach(rdSessionId, measureBounds()).then(
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
    return () => {
      cancelled = true;
      if (optionsTimerRef.current !== null) {
        clearTimeout(optionsTimerRef.current);
        optionsTimerRef.current = null;
      }
      reportedViewportRef.current = "";
      presentBackendRef.current = "canvas2d";
      void rdSurfaceDetach(rdSessionId).catch((error: unknown) => {
        console.warn("Remote desktop surface detach failed", error);
      });
    };
  }, [rdSessionId, measureBounds]);

  // Unmounting drops the presenter state the next mount must rebuild from scratch.
  useEffect(() => () => {
    cancelPendingFrame();
    pendingFrameRef.current = null;
    resetCursorShape();
  }, [cancelPendingFrame, resetCursorShape]);

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
    if (!rdSessionId || presentBackend !== "wgpu") return;
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
  }, [rdSessionId, presentBackend, reportSurfaceGeometry]);

  useEffect(() => {
    if (!openMenu) return;
    const onPointerDown = (event: globalThis.PointerEvent) => {
      if (toolbarRef.current && event.target instanceof Node && !toolbarRef.current.contains(event.target)) {
        setOpenMenu(null);
      }
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") setOpenMenu(null);
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [openMenu]);

  useEffect(() => {
    const onFsChange = () => {
      const active = document.fullscreenElement === rootRef.current;
      setNativeFullscreen(active);
      setIsFullscreen(active);
    };
    document.addEventListener("fullscreenchange", onFsChange);
    return () => document.removeEventListener("fullscreenchange", onFsChange);
  }, []);

  const status = session?.status ?? "connecting";
  const wireState = session?.wireState ?? (status === "connected" ? "connected" : null);
  const inputEnabled = status === "connected" && rdSessionId !== null && paneInteractive;
  const capturingKeys = inputEnabled && captureShortcuts && !openMenu;

  // Forward native menu shortcuts (e.g. macOS AppKit Cmd+W) to the remote host while capturing.
  useEffect(() => {
    if (!capturingKeys) return;
    return registerRemoteDesktopShortcutCapture((action) => {
      const canvas = canvasRef.current;
      const sid = sessionIdRef.current;
      if (!sid || !canvas || document.activeElement !== canvas) return false;
      void rdSpecialKey(sid, action).catch((error: unknown) => {
        showNotice("error", rdErrorMessage(toIpcError(error)));
      });
      return true;
    });
  }, [capturingKeys, showNotice]);

  const send = useCallback((event: RdInputEvent) => {
    remoteDesktopStore.sendInput(hostId, event);
  }, [hostId]);

  const releasePressedKeys = useCallback(() => {
    for (const code of pressedKeysRef.current) send({ kind: "key", code, down: false });
    pressedKeysRef.current.clear();
  }, [send]);

  const releasePointerCapture = useCallback(() => {
    const canvas = canvasRef.current;
    for (const pointerId of capturedPointersRef.current) {
      try {
        canvas?.releasePointerCapture?.(pointerId);
      } catch {
        // The pointer is already gone; nothing to release.
      }
    }
    capturedPointersRef.current.clear();
  }, []);

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

  // Hiding is not disconnecting: stop drawing, hand back the pointer and the mouse, and let the far
  // side know about keys it still believes are held. `inputEnabled` already folds `visible`,
  // `occluded` and modal ownership in, so this one effect covers every yielding case.
  useEffect(() => {
    if (inputEnabled) return;
    pendingFrameRef.current = null;
    exitPointerLock();
    releasePointerCapture();
    releasePressedKeys();
  }, [inputEnabled, exitPointerLock, releasePointerCapture, releasePressedKeys]);

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
    const sid = sessionIdRef.current;
    if (!sid) throw new Error("remote desktop session is not connected");
    await rdSetOptions(sid, { playoutMode: mode });
    setPlayoutMode(mode);
  }, []);

  const changeScaleMode = useCallback(async (mode: RdScaleMode) => {
    const sid = sessionIdRef.current;
    if (!sid) throw new Error("remote desktop session is not connected");
    await rdSetOptions(sid, { scaleMode: mode });
    setScaleMode(mode);
  }, []);

  const changeAudio = useCallback(async (muted: boolean, volume: number) => {
    const sid = sessionIdRef.current;
    if (!sid) throw new Error("remote desktop session is not connected");
    await rdSetAudio(sid, muted, volume);
    setAudioMuted(muted);
    setAudioVolume(volume);
  }, []);

  const sendPointerPosition = useCallback((clientX: number, clientY: number): boolean => {
    const canvas = canvasRef.current;
    const size = frameSizeRef.current;
    if (!canvas || !size) return false;
    const point = mapClientPointToFrame(clientX, clientY, canvas.getBoundingClientRect(), size.width, size.height, fitMode);
    if (!point) return false;
    send({ kind: "mouseMove", x: point.x, y: point.y });
    return true;
  }, [fitMode, send]);

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

  const handlePointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.focus({ preventScroll: true });
    audioRef.current?.resume();
    if (openMenu) setOpenMenu(null);
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
    capturedPointersRef.current.add(event.pointerId);
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
    capturedPointersRef.current.delete(event.pointerId);
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

  const handleSendSpecialKey = (action: RdSpecialKeyAction, actionLabel: string) => {
    const sid = sessionIdRef.current;
    setOpenMenu(null);
    if (!sid) return;
    void rdSpecialKey(sid, action).then(
      () => {
        showNotice("info", `Sent ${actionLabel}`);
        canvasRef.current?.focus({ preventScroll: true });
      },
      (error: unknown) => showNotice("error", rdErrorMessage(toIpcError(error))),
    );
  };

  const handleSelectDisplay = (displayIndex: number) => {
    const sid = sessionIdRef.current;
    setOpenMenu(null);
    if (!sid) return;
    void rdSetStreamOptions(sid, { displayIndex }).then(
      (updated) => {
        lastMetaBySessionId.set(sid, updated);
        setMeta(updated);
        showNotice("info", `Switched to display ${displayIndex + 1}`);
        canvasRef.current?.focus({ preventScroll: true });
      },
      (error: unknown) => showNotice("error", rdErrorMessage(toIpcError(error))),
    );
  };

  const handleSelectQuality = (presetId: RdQualityPresetId) => {
    const preset = RD_QUALITY_PRESETS.find((item) => item.id === presetId);
    if (!preset) return;
    setQualityPreset(presetId);
    const sid = sessionIdRef.current;
    if (!sid) return;
    void rdSetStreamOptions(sid, preset.options).then(
      (updated) => {
        lastMetaBySessionId.set(sid, updated);
        setMeta(updated);
        showNotice("info", `Quality: ${preset.label}`);
      },
      (error: unknown) => showNotice("error", rdErrorMessage(toIpcError(error))),
    );
  };

  const handleRefreshFrame = () => {
    const sid = sessionIdRef.current;
    setOpenMenu(null);
    if (!sid) return;
    void rdRequestKeyframe(sid).then(
      () => showNotice("info", "Keyframe requested"),
      (error: unknown) => showNotice("error", rdErrorMessage(toIpcError(error))),
    );
  };

  const handleSendClipboardText = (text: string) => {
    const sid = sessionIdRef.current;
    if (!sid || !text) return;
    void rdSendClipboard(sid, text).then(
      () => {
        setClipboardDraft("");
        showNotice("info", "Sent to remote clipboard");
      },
      (error: unknown) => showNotice("error", rdErrorMessage(toIpcError(error))),
    );
  };

  const handleSendLocalClipboard = () => {
    if (!navigator.clipboard?.readText) {
      showNotice("error", "Local clipboard read is unavailable; paste text below.");
      return;
    }
    void navigator.clipboard.readText().then(
      (text) => {
        if (!text) {
          showNotice("info", "Local clipboard is empty");
          return;
        }
        handleSendClipboardText(text);
      },
      () => showNotice("error", "Could not read local clipboard; paste text below."),
    );
  };

  const handleCopyRemoteClipboard = () => {
    if (!remoteClipboard) return;
    if (!navigator.clipboard?.writeText) {
      showNotice("error", "Local clipboard write is unavailable.");
      return;
    }
    void navigator.clipboard.writeText(remoteClipboard).then(
      () => {
        setHasUnreadClipboard(false);
        showNotice("info", "Copied remote clipboard");
      },
      () => showNotice("error", "Could not write to local clipboard."),
    );
  };

  const handleToggleAudio = () => {
    const nextMuted = !audioMuted;
    setAudioMuted(nextMuted);
    audioRef.current?.setMuted(nextMuted);
    const sid = sessionIdRef.current;
    if (!sid) return;
    void rdSetAudioMuted(sid, nextMuted).catch((error: unknown) => {
      showNotice("error", rdErrorMessage(toIpcError(error)));
    });
  };

  const handleToggleFullscreen = () => {
    setOpenMenu(null);
    if (isFullscreen) {
      if (nativeFullscreen && typeof document.exitFullscreen === "function") {
        void document.exitFullscreen().catch(() => {
          setNativeFullscreen(false);
          setIsFullscreen(false);
        });
      } else {
        setIsFullscreen(false);
      }
      return;
    }
    const root = rootRef.current;
    if (root && typeof root.requestFullscreen === "function") {
      void root.requestFullscreen().then(
        () => {
          setNativeFullscreen(true);
          setIsFullscreen(true);
        },
        () => {
          setNativeFullscreen(false);
          setIsFullscreen(true);
        },
      );
    } else {
      setNativeFullscreen(false);
      setIsFullscreen(true);
    }
  };

  const handleDisconnect = () => {
    setOpenMenu(null);
    releasePointerCapture();
    releasePressedKeys();
    const sid = sessionIdRef.current;
    if (sid) lastMetaBySessionId.delete(sid);
    void remoteDesktopStore.disconnect(hostId);
  };

  const handleReconnect = () => {
    setOpenMenu(null);
    if (status === "connected" || status === "connecting") {
      const sid = sessionIdRef.current;
      if (sid) lastMetaBySessionId.delete(sid);
      void remoteDesktopStore.disconnect(hostId).then(() => remoteDesktopStore.connect(hostId, label));
      return;
    }
    void remoteDesktopStore.connect(hostId, label);
  };

  const statusText = status === "error"
    ? rdErrorMessage(session?.error ?? { code: "UNKNOWN", message: "Remote desktop failed." })
    : status === "idle" && !wireState
      ? "Disconnected"
      : wireState && wireState !== "connected" && session?.reason
        ? `${STATUS_LABEL[wireState]} ${session.reason}`
        : wireState
          ? STATUS_LABEL[wireState]
          : "Connecting…";
  const canRetry = status === "error" || status === "idle" || wireState === "ended";
  // A presented native surface already shows the desktop, so the pane must not cover it with a
  // "Connected" placeholder that only exists because no JPEG frame was ever decoded.
  const showOverlay = status !== "connected" || (!hasFrame && !nativePresented);
  // A frozen frame is arbitrary image content, so the overlay needs its own opaque scrim for
  // readable status text; without a frame the pane is already black and needs none.
  const scrim = hasFrame;
  const statusColor = status === "error"
    ? scrim ? "text-red-300" : "text-destructive"
    : scrim ? "text-white" : "text-muted-foreground";
  const isConnected = status === "connected";
  const displays = meta?.displays ?? [];
  const clipboardAllowed = meta?.clipboardAllowed ?? true;
  const telemetry = formatTelemetry(meta, frameSize);
  // The interactive canvas stays hit-testable from the moment the session connects: in wgpu
  // mode it is a transparent event surface over the native view and never gets a bitmap.
  const canvasVisible = hasFrame || isConnected;
  // macOS places the native view beneath a transparent WKWebView, so an opaque pane background
  // would hide it; index.css paints var(--terminal) instead until the surface presents.
  const nativeSurfaceOwnsPixels = presentBackend === "wgpu" && nativePresented;

  const toolbarBtn = (active = false) =>
    `inline-flex h-6 items-center gap-1 rounded px-1.5 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-45 ${
      active
        ? "bg-accent text-accent-foreground"
        : "text-muted-foreground hover:bg-surface-hover hover:text-foreground"
    }`;

  return (
    <div
      ref={rootRef}
      className={`${
        isFullscreen && !nativeFullscreen
          ? "fixed inset-0 z-50 flex min-h-0 min-w-0 flex-col overflow-hidden"
          : "relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden"
      }${nativeSurfaceOwnsPixels ? "" : " bg-black"}`}
      data-testid="remote-desktop-pane"
      data-rd-state={status}
      data-rd-fullscreen={isFullscreen ? "true" : "false"}
      data-rd-native={presentBackend === "wgpu" ? "true" : "false"}
      data-rd-presented={nativePresented ? "true" : "false"}
      data-rd-interactive={paneInteractive ? "true" : "false"}
      data-rd-surface-visible={surfaceVisible ? "true" : "false"}
      data-rd-pointer-locked={pointerLocked ? "true" : "false"}
      data-rd-cursor={remoteCursor}
    >
      <RemoteDesktopToolbar
        rdSessionId={rdSessionId}
        connected={isConnected}
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
        ref={toolbarRef}
        data-testid="remote-desktop-stream-toolbar"
        className="relative z-30 flex h-[34px] shrink-0 items-center justify-between gap-2 border-b border-border bg-card px-2.5 text-[11px]"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${statusDotClass(status, wireState)}`}
            aria-hidden
          />
          <span className="truncate font-medium text-foreground">
            {meta?.hostName || label}
          </span>
          {meta?.route ? (
            <span
              data-testid="rd-route-badge"
              className={`shrink-0 rounded border border-border px-1.5 py-0.5 text-[11px] leading-none ${
                meta.route === "direct" ? "text-status-success" : "text-status-warning"
              }`}
            >
              {meta.route === "direct" ? "Direct" : "Relay"}
            </span>
          ) : null}
          {isConnected && telemetry ? (
            <span
              data-testid="rd-telemetry"
              className="hidden truncate font-mono text-[11px] text-muted-foreground sm:inline"
            >
              {telemetry}
            </span>
          ) : null}
          {notice ? (
            <span
              data-testid="rd-toolbar-notice"
              role={notice.tone === "error" ? "alert" : "status"}
              className={`truncate text-[11px] ${
                notice.tone === "error" ? "text-destructive" : "text-muted-foreground"
              }`}
            >
              {notice.text}
            </span>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          <div className="relative">
            <button
              type="button"
              data-testid="rd-display-trigger"
              aria-label="Select display"
              aria-expanded={openMenu === "display"}
              disabled={!isConnected}
              title={
                displays.length > 1
                  ? `Switch display (${displays.length} available)`
                  : "Display info"
              }
              className={toolbarBtn(openMenu === "display")}
              onClick={() => setOpenMenu((cur) => (cur === "display" ? null : "display"))}
            >
              <Monitor className="h-3.5 w-3.5" aria-hidden />
              <span className="hidden md:inline">
                {displays.length > 1 ? `Display ${(meta?.activeDisplay ?? 0) + 1}` : "Display"}
              </span>
            </button>
            {openMenu === "display" ? (
              <div
                data-testid="rd-display-menu"
                className="absolute right-0 top-full z-40 mt-1 w-56 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-elevation-2"
              >
                <div className="px-2 py-1 text-[11px] font-medium text-muted-foreground">
                  Remote displays
                </div>
                {displays.length === 0 ? (
                  <div className="px-2 py-1.5 text-[11px] text-muted-foreground">
                    Primary display ({frameSize ? `${frameSize.width}×${frameSize.height}` : "active"})
                  </div>
                ) : (
                  displays.map((d) => {
                    const active = (meta?.activeDisplay ?? 0) === d.index;
                    return (
                      <button
                        key={d.index}
                        type="button"
                        data-testid={`rd-display-option-${d.index}`}
                        disabled={!meta?.supportsStreamOptions && !active}
                        className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover disabled:opacity-50"
                        onClick={() => handleSelectDisplay(d.index)}
                      >
                        <span className="truncate">
                          {d.name || `Display ${d.index + 1}`}
                          {d.primary ? " (Primary)" : ""}
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5 font-mono text-[11px] text-muted-foreground">
                          {d.width}×{d.height}
                          {active ? <Check className="h-3.5 w-3.5 text-foreground" aria-hidden /> : null}
                        </span>
                      </button>
                    );
                  })
                )}
              </div>
            ) : null}
          </div>

          <div className="relative">
            <button
              type="button"
              data-testid="rd-keys-trigger"
              aria-label="Send special keys"
              aria-expanded={openMenu === "keys"}
              disabled={!isConnected}
              title="Special keys & shortcut capture"
              className={toolbarBtn(openMenu === "keys")}
              onClick={() => setOpenMenu((cur) => (cur === "keys" ? null : "keys"))}
            >
              <Keyboard className="h-3.5 w-3.5" aria-hidden />
              <span className="hidden md:inline">Keys</span>
            </button>
            {openMenu === "keys" ? (
              <div
                data-testid="rd-keys-menu"
                className="absolute right-0 top-full z-40 mt-1 w-52 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-elevation-2"
              >
                <button
                  type="button"
                  data-testid="rd-capture-shortcuts-toggle"
                  className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                  onClick={() => setCaptureShortcuts((cur) => !cur)}
                >
                  <span>Capture app shortcuts</span>
                  {captureShortcuts ? <Check className="h-3.5 w-3.5 text-status-success" aria-hidden /> : null}
                </button>
                <div className="my-1 h-px bg-border" />
                {SPECIAL_KEY_ITEMS.map((item) => (
                  <button
                    key={item.action}
                    type="button"
                    data-testid={`rd-key-${item.action}`}
                    className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                    onClick={() => handleSendSpecialKey(item.action, item.label)}
                  >
                    <span>{item.label}</span>
                    {item.hint ? (
                      <span className="font-mono text-[11px] text-muted-foreground">{item.hint}</span>
                    ) : null}
                  </button>
                ))}
              </div>
            ) : null}
          </div>

          <div className="relative">
            <button
              type="button"
              data-testid="rd-quality-trigger"
              aria-label="View and quality settings"
              aria-expanded={openMenu === "quality"}
              disabled={!isConnected}
              title="Scale & stream quality"
              className={toolbarBtn(openMenu === "quality")}
              onClick={() => setOpenMenu((cur) => (cur === "quality" ? null : "quality"))}
            >
              <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden />
              <span className="hidden lg:inline">{fitMode === "fit" ? "Fit" : "1:1"}</span>
            </button>
            {openMenu === "quality" ? (
              <div
                data-testid="rd-quality-menu"
                className="absolute right-0 top-full z-40 mt-1 w-56 rounded-md border border-border bg-popover p-1 text-popover-foreground shadow-elevation-2"
              >
                <div className="px-2 py-1 text-[11px] font-medium text-muted-foreground">View scale</div>
                <button
                  type="button"
                  data-testid="rd-scale-fit"
                  className="flex w-full items-center justify-between rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                  onClick={() => {
                    setFitMode("fit");
                    setOpenMenu(null);
                  }}
                >
                  <span>Fit to pane</span>
                  {fitMode === "fit" ? <Check className="h-3.5 w-3.5" aria-hidden /> : null}
                </button>
                <button
                  type="button"
                  data-testid="rd-scale-actual"
                  className="flex w-full items-center justify-between rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                  onClick={() => {
                    setFitMode("actual");
                    setOpenMenu(null);
                  }}
                >
                  <span>1:1 Actual size</span>
                  {fitMode === "actual" ? <Check className="h-3.5 w-3.5" aria-hidden /> : null}
                </button>
                <div className="my-1 h-px bg-border" />
                <div className="px-2 py-1 text-[11px] font-medium text-muted-foreground">Stream quality</div>
                {RD_QUALITY_PRESETS.map((preset) => (
                  <button
                    key={preset.id}
                    type="button"
                    data-testid={`rd-quality-${preset.id}`}
                    className="flex w-full items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                    onClick={() => {
                      handleSelectQuality(preset.id);
                      setOpenMenu(null);
                    }}
                  >
                    <span className="flex flex-col">
                      <span>{preset.label}</span>
                      <span className="text-[11px] text-muted-foreground">{preset.description}</span>
                    </span>
                    {qualityPreset === preset.id ? <Check className="h-3.5 w-3.5 shrink-0" aria-hidden /> : null}
                  </button>
                ))}
                <div className="my-1 h-px bg-border" />
                <button
                  type="button"
                  data-testid="rd-request-keyframe"
                  className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-[12px] text-foreground hover:bg-surface-hover"
                  onClick={handleRefreshFrame}
                >
                  <RefreshCw className="h-3.5 w-3.5" aria-hidden />
                  <span>Refresh keyframe</span>
                </button>
              </div>
            ) : null}
          </div>

          <div className="relative">
            <button
              type="button"
              data-testid="rd-clipboard-trigger"
              aria-label="Clipboard sync"
              aria-expanded={openMenu === "clipboard"}
              disabled={!isConnected}
              title={clipboardAllowed ? "Clipboard sync" : "Clipboard disabled by host"}
              className={toolbarBtn(openMenu === "clipboard")}
              onClick={() => {
                setHasUnreadClipboard(false);
                setOpenMenu((cur) => (cur === "clipboard" ? null : "clipboard"));
              }}
            >
              <span className="relative inline-flex">
                <Clipboard className="h-3.5 w-3.5" aria-hidden />
                {hasUnreadClipboard ? (
                  <span
                    data-testid="rd-clipboard-unread"
                    className="absolute -right-0.5 -top-0.5 h-1.5 w-1.5 rounded-full bg-status-success"
                  />
                ) : null}
              </span>
            </button>
            {openMenu === "clipboard" ? (
              <div
                data-testid="rd-clipboard-menu"
                className="absolute right-0 top-full z-40 mt-1 w-72 rounded-md border border-border bg-popover p-2.5 text-popover-foreground shadow-elevation-2"
              >
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-[12px] font-medium text-foreground">Clipboard</span>
                  {!clipboardAllowed ? (
                    <span className="text-[11px] text-muted-foreground">Disabled on host</span>
                  ) : null}
                </div>
                <div className="flex flex-col gap-2">
                  <Button
                    type="button"
                    size="sm"
                    variant="outline"
                    data-testid="rd-clipboard-send-local"
                    disabled={!clipboardAllowed}
                    className="w-full justify-center"
                    onClick={handleSendLocalClipboard}
                  >
                    Send local clipboard
                  </Button>
                  <div className="flex items-center gap-1.5">
                    <Input
                      type="text"
                      data-testid="rd-clipboard-input"
                      aria-label="Text to send to remote clipboard"
                      placeholder="Paste or type text…"
                      disabled={!clipboardAllowed}
                      value={clipboardDraft}
                      onChange={(event) => setClipboardDraft(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter" && clipboardDraft.trim()) {
                          event.preventDefault();
                          handleSendClipboardText(clipboardDraft);
                        }
                      }}
                      className="h-7 text-[12px]"
                    />
                    <Button
                      type="button"
                      size="sm"
                      data-testid="rd-clipboard-send-text"
                      disabled={!clipboardAllowed || !clipboardDraft}
                      onClick={() => handleSendClipboardText(clipboardDraft)}
                    >
                      Send
                    </Button>
                  </div>
                  {remoteClipboard !== null ? (
                    <div className="rounded border border-border bg-background p-2">
                      <div className="mb-1 flex items-center justify-between gap-2">
                        <span className="text-[11px] font-medium text-muted-foreground">From remote</span>
                        <button
                          type="button"
                          data-testid="rd-clipboard-copy-remote"
                          className="text-[11px] font-medium text-foreground underline-offset-2 hover:underline"
                          onClick={handleCopyRemoteClipboard}
                        >
                          Copy
                        </button>
                      </div>
                      <p
                        data-testid="rd-clipboard-remote-preview"
                        className="max-h-20 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] text-foreground"
                      >
                        {remoteClipboard}
                      </p>
                    </div>
                  ) : null}
                </div>
              </div>
            ) : null}
          </div>

          <button
            type="button"
            data-testid="rd-audio-toggle"
            aria-label={audioMuted ? "Unmute remote audio" : "Mute remote audio"}
            aria-pressed={audioMuted}
            disabled={!isConnected}
            title={audioMuted ? "Unmute remote audio" : "Mute remote audio"}
            className={toolbarBtn(audioMuted)}
            onClick={handleToggleAudio}
          >
            {audioMuted ? <VolumeX className="h-3.5 w-3.5" aria-hidden /> : <Volume2 className="h-3.5 w-3.5" aria-hidden />}
          </button>

          <button
            type="button"
            data-testid="rd-fullscreen-toggle"
            aria-label={isFullscreen ? "Exit fullscreen" : "Enter fullscreen"}
            aria-pressed={isFullscreen}
            title={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
            className={toolbarBtn(isFullscreen)}
            onClick={handleToggleFullscreen}
          >
            {isFullscreen ? <Minimize2 className="h-3.5 w-3.5" aria-hidden /> : <Maximize2 className="h-3.5 w-3.5" aria-hidden />}
          </button>

          <span className="mx-1 h-3.5 w-px bg-border" aria-hidden />

          <button
            type="button"
            data-testid="rd-reconnect-btn"
            aria-label="Reconnect remote desktop"
            title="Reconnect"
            className={toolbarBtn(false)}
            onClick={handleReconnect}
          >
            <RefreshCw className="h-3.5 w-3.5" aria-hidden />
          </button>

          {!canRetry ? (
            <button
              type="button"
              data-testid="rd-disconnect-btn"
              aria-label="Disconnect remote desktop"
              title="Disconnect"
              className="inline-flex h-6 items-center rounded px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
              onClick={handleDisconnect}
            >
              <Power className="h-3.5 w-3.5" aria-hidden />
            </button>
          ) : null}
        </div>
      </div>

      <div
        ref={containerRef}
        className={
          fitMode === "actual"
            ? "relative flex min-h-0 min-w-0 flex-1 overflow-auto"
            : "relative flex min-h-0 min-w-0 flex-1 items-center justify-center overflow-hidden"
        }
      >
        <canvas
          ref={canvasRef}
          tabIndex={0}
          role="img"
          aria-label={`Remote desktop of ${label}`}
          data-testid="remote-desktop-canvas"
          data-rd-capture-keys={capturingKeys ? "true" : "false"}
          data-rd-fit-mode={fitMode}
          className={
            fitMode === "actual"
              ? "shrink-0 outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
              : "h-full w-full object-contain outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          }
          style={{
            pointerEvents: occluded || !visible ? "none" : undefined,
            visibility: canvasVisible ? "visible" : "hidden",
            cursor: pointerLocked ? undefined : remoteCursor,
            ...(fitMode === "actual" && frameSize
              ? { width: `${frameSize.width}px`, height: `${frameSize.height}px` }
              : {}),
          }}
          onPointerDown={handlePointerDown}
          onPointerUp={handlePointerUp}
          onPointerMove={handlePointerMove}
          onContextMenu={(event) => event.preventDefault()}
          onKeyDown={(event) => handleKey(event, true)}
          onKeyUp={(event) => handleKey(event, false)}
          onBlur={releasePressedKeys}
        />
        <RemoteDesktopStatsOverlay rdSessionId={rdSessionId} open={statsOpen} visible={surfaceVisible} />
        {showOverlay ? (
          <div
            data-testid="remote-desktop-overlay"
            data-scrim={scrim ? "true" : "false"}
            className={cn(
              "pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 p-4 text-center",
              scrim && "bg-black/75 text-white backdrop-blur-sm",
            )}
          >
            <p
              role={status === "error" ? "alert" : "status"}
              data-testid="remote-desktop-status"
              className={cn("text-[12px]", statusColor)}
            >
              {statusText}
            </p>
            {canRetry ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className={cn(
                  "pointer-events-auto",
                  // The outline variant paints bg-background, which is white in the light theme; over a
                  // dark scrim that leaves white-on-white text, so pin the scrim palette explicitly.
                  scrim && "border-white/40 bg-black/60 text-white hover:bg-black/80 hover:text-white",
                )}
                onClick={handleReconnect}
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
