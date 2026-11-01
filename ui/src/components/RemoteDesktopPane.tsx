import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { toIpcError } from "../lib/tauri";
import {
  mapClientPointToFrame,
  rdConnect,
  rdDisconnect,
  rdErrorMessage,
  RdInputSender,
  rdMouseButton,
  wheelDeltaPixels,
  type RdConnectionState,
  type RdFrameMessage,
  type RdInputEvent,
  type RdVideoFrame,
} from "../lib/remoteDesktop";
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

const STATUS_LABEL: Record<RdConnectionState, string> = {
  connecting: "Connecting…",
  connected: "Connected",
  reconnecting: "Reconnecting…",
  ended: "Session ended",
};

/**
 * Viewer of one paired host's desktop: JPEG frames from the `cmd_rd_connect` channel are drawn on
 * a canvas scaled aspect-fit, and pointer/keyboard input is mapped to normalized frame coordinates.
 * The viewer session lives only as long as this component; unmount disconnects it.
 */
export function RemoteDesktopPane({ hostId, label, visible, occluded = false }: RemoteDesktopPaneProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const senderRef = useRef<RdInputSender | null>(null);
  const frameSizeRef = useRef<{ width: number; height: number } | null>(null);
  const pendingFrameRef = useRef<RdVideoFrame | null>(null);
  const decodingRef = useRef(false);
  const pressedKeysRef = useRef(new Set<string>());
  const [status, setStatus] = useState<PaneStatus>({ phase: "connecting", reason: null });
  const [hasFrame, setHasFrame] = useState(false);
  const [attempt, setAttempt] = useState(0);

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
        drawLatestFrame();
      });
  }, []);

  useEffect(() => {
    let cancelled = false;
    let rdSessionId: string | null = null;
    setStatus({ phase: "connecting", reason: null });
    const onFrame = (message: RdFrameMessage) => {
      if (cancelled) return;
      if (message.kind === "status") {
        setStatus({ phase: message.state, reason: message.reason });
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
      senderRef.current?.close();
      senderRef.current = null;
      pendingFrameRef.current = null;
      pressedKeysRef.current.clear();
      if (rdSessionId) {
        void rdDisconnect(rdSessionId).catch((error: unknown) => {
          console.warn("Remote desktop disconnect failed", error);
        });
      }
    };
  }, [hostId, attempt, drawLatestFrame]);

  const inputEnabled = status.phase === "connected" && !occluded;

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

  useEffect(() => {
    if (!inputEnabled || !visible) releasePressedKeys();
  }, [inputEnabled, visible, releasePressedKeys]);

  const handlePointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.focus({ preventScroll: true });
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

  const statusText = status.phase === "error"
    ? status.message
    : status.reason && status.phase !== "connected"
      ? `${STATUS_LABEL[status.phase]} ${status.reason}`
      : STATUS_LABEL[status.phase];
  const canRetry = status.phase === "error" || status.phase === "ended";
  const showOverlay = status.phase !== "connected" || !hasFrame;

  return (
    <div
      className="relative flex h-full w-full min-h-0 min-w-0 items-center justify-center overflow-hidden bg-black"
      data-testid="remote-desktop-pane"
      data-rd-state={status.phase}
    >
      <canvas
        ref={canvasRef}
        tabIndex={0}
        role="img"
        aria-label={`Remote desktop of ${label}`}
        data-testid="remote-desktop-canvas"
        className="h-full w-full object-contain outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
        style={{ pointerEvents: occluded ? "none" : undefined, visibility: hasFrame ? "visible" : "hidden" }}
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
              onClick={() => setAttempt((value) => value + 1)}
            >
              Reconnect
            </Button>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
