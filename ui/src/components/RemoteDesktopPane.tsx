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
import { toIpcError } from "../lib/tauri";
import {
  mapClientPointToFrame,
  RD_QUALITY_PRESETS,
  RdAudioPlayer,
  rdConnect,
  rdDisconnect,
  rdErrorMessage,
  RdInputSender,
  rdMouseButton,
  rdRequestKeyframe,
  rdSendClipboard,
  rdSetAudioMuted,
  rdSetStreamOptions,
  rdSpecialKey,
  registerRemoteDesktopShortcutCapture,
  wheelDeltaPixels,
  type RdConnectionState,
  type RdFitMode,
  type RdFrameMessage,
  type RdInputEvent,
  type RdQualityPresetId,
  type RdSessionMeta,
  type RdSpecialKeyAction,
  type RdVideoFrame,
} from "../lib/remoteDesktop";
import { Button } from "./ui/button";
import { Input } from "./ui/input";

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

type OpenMenu = "display" | "keys" | "quality" | "clipboard" | null;

type ToolbarNotice = { tone: "info" | "error"; text: string };

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

function statusDotClass(phase: PaneStatus["phase"]): string {
  if (phase === "connected") return "bg-status-success";
  if (phase === "connecting" || phase === "reconnecting") return "bg-status-warning";
  if (phase === "error") return "bg-destructive";
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

export function RemoteDesktopPane({ hostId, label, visible, occluded = false }: RemoteDesktopPaneProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const sessionIdRef = useRef<string | null>(null);
  const senderRef = useRef<RdInputSender | null>(null);
  const audioRef = useRef<RdAudioPlayer | null>(null);
  const frameSizeRef = useRef<{ width: number; height: number } | null>(null);
  const pendingFrameRef = useRef<RdVideoFrame | null>(null);
  const decodingRef = useRef(false);
  const pressedKeysRef = useRef(new Set<string>());
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const [status, setStatus] = useState<PaneStatus>({ phase: "connecting", reason: null });
  const [hasFrame, setHasFrame] = useState(false);
  const [frameSize, setFrameSize] = useState<{ width: number; height: number } | null>(null);
  const [meta, setMeta] = useState<RdSessionMeta | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [openMenu, setOpenMenu] = useState<OpenMenu>(null);
  const [fitMode, setFitMode] = useState<RdFitMode>("fit");
  const [qualityPreset, setQualityPreset] = useState<RdQualityPresetId>("balanced");
  const [captureShortcuts, setCaptureShortcuts] = useState(true);
  const [audioMuted, setAudioMuted] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [nativeFullscreen, setNativeFullscreen] = useState(false);
  const [remoteClipboard, setRemoteClipboard] = useState<string | null>(null);
  const [hasUnreadClipboard, setHasUnreadClipboard] = useState(false);
  const [clipboardDraft, setClipboardDraft] = useState("");
  const [notice, setNotice] = useState<ToolbarNotice | null>(null);

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

  const drawLatestFrame = useCallback(() => {
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
        drawLatestFrame();
      });
  }, []);

  useEffect(() => {
    let cancelled = false;
    let rdSessionId: string | null = null;
    const pressedKeys = pressedKeysRef.current;
    const audio = new RdAudioPlayer();
    audioRef.current = audio;
    sessionIdRef.current = null;
    setStatus({ phase: "connecting", reason: null });
    setHasFrame(false);
    setMeta(null);
    setAudioMuted(false);

    const onFrame = (message: RdFrameMessage) => {
      if (cancelled) return;
      if (message.kind === "status") {
        setStatus({ phase: message.state, reason: message.reason });
        return;
      }
      if (message.kind === "meta") {
        setMeta(message.meta);
        setAudioMuted(message.meta.audioMuted);
        audio.setMuted(message.meta.audioMuted);
        return;
      }
      if (message.kind === "audio") {
        audio.push(message);
        return;
      }
      if (message.kind === "clipboard") {
        setRemoteClipboard(message.text);
        setHasUnreadClipboard(true);
        return;
      }
      // Only the newest frame is kept while one decodes; older ones are dropped.
      pendingFrameRef.current = message;
      drawLatestFrame();
    };

    rdConnect(hostId, onFrame, (error) => console.warn("Remote desktop frame rejected", error)).then(
      (result) => {
        if (cancelled) {
          void rdDisconnect(result.rdSessionId).catch(() => undefined);
          return;
        }
        rdSessionId = result.rdSessionId;
        sessionIdRef.current = result.rdSessionId;
        senderRef.current = new RdInputSender(result.rdSessionId, undefined, (error) => {
          console.warn("Remote desktop input failed", error);
        });
      },
      (error: unknown) => {
        if (cancelled) return;
        setStatus({ phase: "error", message: rdErrorMessage(toIpcError(error)) });
      },
    );

    return () => {
      cancelled = true;
      sessionIdRef.current = null;
      senderRef.current?.close();
      senderRef.current = null;
      audio.close();
      if (audioRef.current === audio) audioRef.current = null;
      pendingFrameRef.current = null;
      pressedKeys.clear();
      if (rdSessionId) {
        void rdDisconnect(rdSessionId).catch((error: unknown) => {
          console.warn("Remote desktop disconnect failed", error);
        });
      }
    };
  }, [hostId, attempt, drawLatestFrame]);

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

  const inputEnabled = status.phase === "connected" && !occluded;
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
    senderRef.current?.push(event);
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

  const releasePressedKeys = useCallback(() => {
    for (const code of pressedKeysRef.current) send({ kind: "key", code, down: false });
    pressedKeysRef.current.clear();
  }, [send]);

  useEffect(() => {
    if (!inputEnabled || !visible) releasePressedKeys();
  }, [inputEnabled, visible, releasePressedKeys]);

  const handlePointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.focus({ preventScroll: true });
    audioRef.current?.resume();
    if (openMenu) setOpenMenu(null);
    if (!inputEnabled) return;
    const button = rdMouseButton(event.button);
    if (button === null) return;
    if (!sendPointerPosition(event.clientX, event.clientY)) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    send({ kind: "mouseButton", button, down: true });
  };

  const handlePointerUp = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!inputEnabled) return;
    const button = rdMouseButton(event.button);
    if (button === null) return;
    sendPointerPosition(event.clientX, event.clientY);
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    send({ kind: "mouseButton", button, down: false });
  };

  const handlePointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!inputEnabled) return;
    sendPointerPosition(event.clientX, event.clientY);
  };

  const handleKey = (event: KeyboardEvent<HTMLCanvasElement>, down: boolean) => {
    if (!inputEnabled || !event.nativeEvent.code || event.nativeEvent.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    const code = event.nativeEvent.code;
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
    const sid = sessionIdRef.current;
    sessionIdRef.current = null;
    senderRef.current?.close();
    senderRef.current = null;
    releasePressedKeys();
    setStatus({ phase: "ended", reason: "Disconnected" });
    if (sid) {
      void rdDisconnect(sid).catch(() => undefined);
    }
  };

  const handleReconnect = () => {
    setOpenMenu(null);
    setAttempt((value) => value + 1);
  };

  const statusText = status.phase === "error"
    ? status.message
    : status.reason && status.phase !== "connected"
      ? `${STATUS_LABEL[status.phase]} ${status.reason}`
      : STATUS_LABEL[status.phase];
  const canRetry = status.phase === "error" || status.phase === "ended";
  const showOverlay = status.phase !== "connected" || !hasFrame;
  const isConnected = status.phase === "connected";
  const displays = meta?.displays ?? [];
  const clipboardAllowed = meta?.clipboardAllowed ?? true;
  const telemetry = formatTelemetry(meta, frameSize);

  const toolbarBtn = (active = false) =>
    `inline-flex h-6 items-center gap-1 rounded px-1.5 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-45 ${
      active
        ? "bg-accent text-accent-foreground"
        : "text-muted-foreground hover:bg-surface-hover hover:text-foreground"
    }`;

  return (
    <div
      ref={rootRef}
      className={
        isFullscreen && !nativeFullscreen
          ? "fixed inset-0 z-50 flex min-h-0 min-w-0 flex-col overflow-hidden bg-background"
          : "relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden bg-background"
      }
      data-testid="remote-desktop-pane"
      data-rd-state={status.phase}
      data-rd-fullscreen={isFullscreen ? "true" : "false"}
    >
      <div
        ref={toolbarRef}
        data-testid="remote-desktop-toolbar"
        className="relative z-30 flex h-[34px] shrink-0 items-center justify-between gap-2 border-b border-border bg-card px-2.5 text-[11px]"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${statusDotClass(status.phase)}`}
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
        className={
          fitMode === "actual"
            ? "relative flex min-h-0 flex-1 overflow-auto bg-black"
            : "relative flex min-h-0 flex-1 items-center justify-center overflow-hidden bg-black"
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
            pointerEvents: occluded ? "none" : undefined,
            visibility: hasFrame ? "visible" : "hidden",
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
