import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type KeyboardEvent, type PointerEvent } from "react";
import {
  mapClientPointToFrame,
  rdErrorMessage,
  rdMouseButton,
  wheelDeltaPixels,
  type RdConnectionState,
  type RdFrameMessage,
  type RdInputEvent,
  type RdVideoFrame,
} from "../lib/remoteDesktop";
import { remoteDesktopStore, type RemoteDesktopSession } from "../state/remoteDesktopStore";
import { Button } from "./ui/button";

export type RemoteDesktopPaneProps = {
  hostId: string;
  label: string;
  /** False while the owning tab is hidden; the session is kept alive but nothing is drawn or sent. */
  visible: boolean;
  /** True while pane drop feedback covers this pane; input is suspended so the drop wins. */
  occluded?: boolean;
};

const STATUS_LABEL: Record<RdConnectionState, string> = {
  connecting: "Connecting…",
  connected: "Connected",
  reconnecting: "Reconnecting…",
  ended: "Session ended",
};

function useSession(hostId: string): RemoteDesktopSession | null {
  return useSyncExternalStore(
    remoteDesktopStore.subscribe,
    () => remoteDesktopStore.getSession(hostId),
    () => null,
  );
}

/**
 * Canvas view of one paired host's desktop. The session itself belongs to `remoteDesktopStore`, so
 * mounting, hiding or unmounting this pane only attaches or detaches the canvas: the connection
 * survives worktree and tab switches. Only the explicit Disconnect button tears it down.
 */
export function RemoteDesktopPane({ hostId, label, visible, occluded = false }: RemoteDesktopPaneProps) {
  const session = useSession(hostId);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const frameSizeRef = useRef<{ width: number; height: number } | null>(null);
  const pendingFrameRef = useRef<RdVideoFrame | null>(null);
  const decodingRef = useRef(false);
  const pressedKeysRef = useRef(new Set<string>());
  const capturedPointersRef = useRef(new Set<number>());
  const visibleRef = useRef(visible);
  const [hasFrame, setHasFrame] = useState(false);

  visibleRef.current = visible;

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
        // A hidden pane keeps the session but must not paint: the stale frame is dropped.
        if (canvas && visibleRef.current) {
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

  // Attach to the host's session. Reusing an existing connected session is what keeps the
  // connection alive across mounts; nothing here may disconnect.
  useEffect(() => {
    void remoteDesktopStore.connect(hostId, label);
  }, [hostId, label]);

  useEffect(() => remoteDesktopStore.subscribeFrames(hostId, (message: RdFrameMessage) => {
    if (message.kind === "status") return;
    if (!visibleRef.current) return;
    // Only the newest frame is kept while one decodes; older ones are dropped.
    pendingFrameRef.current = message;
    drawLatestFrame();
  }), [hostId, drawLatestFrame]);

  const status = session?.status ?? "connecting";
  const wireState = session?.wireState ?? (status === "connected" ? "connected" : null);
  const inputEnabled = status === "connected" && session?.rdSessionId != null && visible && !occluded;

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

  // Hiding is not disconnecting: stop drawing, hand back the pointer, and let the far side know
  // about keys it still believes are held.
  useEffect(() => {
    if (visible) return;
    pendingFrameRef.current = null;
    releasePointerCapture();
    releasePressedKeys();
  }, [visible, releasePointerCapture, releasePressedKeys]);

  useEffect(() => {
    if (!inputEnabled) {
      releasePointerCapture();
      releasePressedKeys();
    }
  }, [inputEnabled, releasePointerCapture, releasePressedKeys]);

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

  const handlePointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    event.currentTarget.focus({ preventScroll: true });
    if (!inputEnabled) return;
    const button = rdMouseButton(event.button);
    if (button === null) return;
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
    sendPointerPosition(event.clientX, event.clientY);
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    capturedPointersRef.current.delete(event.pointerId);
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
  const showOverlay = status !== "connected" || !hasFrame;

  return (
    <div
      className="relative flex h-full w-full min-h-0 min-w-0 items-center justify-center overflow-hidden bg-black"
      data-testid="remote-desktop-pane"
      data-rd-state={status}
    >
      <canvas
        ref={canvasRef}
        tabIndex={0}
        role="img"
        aria-label={`Remote desktop of ${label}`}
        data-testid="remote-desktop-canvas"
        className="h-full w-full object-contain outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
        style={{
          pointerEvents: occluded || !visible ? "none" : undefined,
          visibility: hasFrame && visible ? "visible" : "hidden",
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
            role={status === "error" ? "alert" : "status"}
            data-testid="remote-desktop-status"
            className={`text-[12px] ${status === "error" ? "text-destructive" : "text-muted-foreground"}`}
          >
            {statusText}
          </p>
          {canRetry ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="pointer-events-auto"
              onClick={() => void remoteDesktopStore.connect(hostId, label)}
            >
              Reconnect
            </Button>
          ) : null}
          {status === "connected" || status === "connecting" ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="pointer-events-auto"
              onClick={() => void remoteDesktopStore.disconnect(hostId)}
            >
              Disconnect
            </Button>
          ) : null}
        </div>
      ) : (
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="absolute right-2 top-2"
          onClick={() => void remoteDesktopStore.disconnect(hostId)}
        >
          Disconnect
        </Button>
      )}
    </div>
  );
}
