/**
 * RemoteBrowser Component (§4.5, Phase 5)
 *
 * Renders live WKWebView screencast with letterbox-aware coordinate mapping
 * and a dedicated controls slot for Phase 6 composition.
 */

import React, { useEffect, useRef, useState } from "react";
import { CircleAlert, X } from "lucide-react";
import {
  buildPointClickParams,
  type BrowserCaptureRect,
  type BrowserErrorDetails,
  type BrowserFrame,
  type BrowserFrameMetadata,
  type BrowserStateMessage,
  type BrowserSubscribeOptions,
  type DecodedBrowserFrame,
} from "./browserProtocol";
import {
  useRemoteBrowser,
  type RemoteBrowserStatus,
  type UseRemoteBrowserResult,
} from "./useRemoteBrowser";
import { userFacingError } from "./userFacingError";

export const BROWSER_FOCUS_RING = "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

const COARSE_POINTER_QUERY = "(pointer: coarse)";

/** True on touch-first devices (phones, tablets); tracks the media query live. */
export function useCoarsePointer(): boolean {
  const [coarse, setCoarse] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
    return window.matchMedia(COARSE_POINTER_QUERY)?.matches === true;
  });
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(COARSE_POINTER_QUERY);
    if (!query || typeof query.addEventListener !== "function") return;
    setCoarse(query.matches === true);
    const onChange = (event: MediaQueryListEvent) => setCoarse(event.matches === true);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return coarse;
}

export interface RemoteBrowserPointClickEvent {
  u: number;
  v: number;
  streamId: number;
  seq: number;
  sequenceNumber: number;
  documentGeneration: string;
  viewportRevision: string;
  browserInstanceId: string;
  captureRect?: BrowserCaptureRect;
  geometrySource?: "wkSnapshot";
  x?: number;
  y?: number;
}

export interface RemoteInputRefusal {
  code: string;
  reason: string;
  inputClass: "point" | "key" | "fill" | "eval" | "unsupported";
  target?: string;
  remediation?: string;
}

/**
 * Builds the refusal banner payload from a rejected point-click command.
 *
 * When the transport carried a structured `details` payload the banner shows the server's
 * own reason / input class / remediation. Without one it keeps the transport code and
 * message and leaves the remediation unset — the component never infers a cause from the
 * message text.
 */
export function buildRemoteInputRefusal(err: unknown): RemoteInputRefusal {
  const reason = err instanceof Error ? err.message : String(err);
  const code = (err as { code?: string })?.code || "UNSUPPORTED";
  const details = (err as { details?: BrowserErrorDetails })?.details;
  if (!details) {
    return { code, reason, inputClass: "point" };
  }
  return {
    code: details.code || code,
    reason: details.reason || reason,
    inputClass: (details.inputClass as RemoteInputRefusal["inputClass"]) || "point",
    remediation: details.remediation,
  };
}

const REFUSAL_COPY: Record<string, string> = {
  LETTERBOX_MARGIN: "That tap was outside the page.",
  BROWSER_INVALID_REFERENCE: "That page element is no longer there.",
  PAYLOAD_TOO_LARGE: "That is too much to send at once.",
  KEY_NOT_ALLOWED: "That key can't be sent to the page.",
  PERMISSION_DENIED: "Running a script on this page needs approval on the computer.",
  UNSUPPORTED: "The page can't take that kind of input.",
  COMMAND_FAILED: "That didn't go through.",
};

/** One plain sentence for a refusal; the raw code and server reason belong in diagnostics only. */
export function describeRemoteInputRefusal(refusal: RemoteInputRefusal): string {
  return REFUSAL_COPY[refusal.code] ?? userFacingError({ code: refusal.code });
}

export function resolveViewportClickParams(
  clickX: number,
  clickY: number,
  containerWidth: number,
  containerHeight: number,
  displayedFrame: { seq: number; metadata: BrowserFrameMetadata } | null,
  lastAckedSeq?: number | null,
): RemoteBrowserPointClickEvent | null {
  if (!displayedFrame || !displayedFrame.metadata) return null;
  if (containerWidth <= 0 || containerHeight <= 0) return null;

  const { imageWidth, imageHeight } = displayedFrame.metadata;
  if (imageWidth <= 0 || imageHeight <= 0) return null;

  const containerAspect = containerWidth / containerHeight;
  const imageAspect = imageWidth / imageHeight;

  let renderedWidth: number;
  let renderedHeight: number;
  let offsetLeft = 0;
  let offsetTop = 0;

  if (containerAspect > imageAspect) {
    // Letterboxed horizontally (pillarbox)
    renderedHeight = containerHeight;
    renderedWidth = containerHeight * imageAspect;
    offsetLeft = (containerWidth - renderedWidth) / 2;
  } else {
    // Letterboxed vertically
    renderedWidth = containerWidth;
    renderedHeight = containerWidth / imageAspect;
    offsetTop = (containerHeight - renderedHeight) / 2;
  }

  // Discard clicks in letterbox margins (§4.5)
  if (
    clickX < offsetLeft ||
    clickX > offsetLeft + renderedWidth ||
    clickY < offsetTop ||
    clickY > offsetTop + renderedHeight
  ) {
    return null;
  }

  // Normalized coordinates (u, v) in [0, 1] relative to the actual image
  const u = Math.min(Math.max((clickX - offsetLeft) / renderedWidth, 0), 1);
  const v = Math.min(Math.max((clickY - offsetTop) / renderedHeight, 0), 1);

  const params = buildPointClickParams({
    u,
    v,
    frame: displayedFrame,
    lastAckedSeq,
  });

  return {
    ...params,
    seq: displayedFrame.seq,
  };
}

/** Pinch-zoom state of the phone viewport: the frame layer is drawn at `translate(x, y) scale(scale)`. */
export interface BrowserViewportZoom {
  scale: number;
  x: number;
  y: number;
}

export const BROWSER_ZOOM_IDENTITY: BrowserViewportZoom = { scale: 1, x: 0, y: 0 };
export const BROWSER_ZOOM_MAX = 4;
/** A finger that travels less than this is still a tap, not a pan. */
const BROWSER_PAN_SLOP_PX = 8;

/** Keeps the zoomed layer covering the viewport: no gap ever opens at an edge. */
export function clampBrowserZoom(zoom: BrowserViewportZoom, width: number, height: number): BrowserViewportZoom {
  const scale = Math.min(Math.max(zoom.scale, 1), BROWSER_ZOOM_MAX);
  if (scale === 1) return BROWSER_ZOOM_IDENTITY;
  const x = Math.min(0, Math.max(width - width * scale, zoom.x));
  const y = Math.min(0, Math.max(height - height * scale, zoom.y));
  return { scale, x, y };
}

/** A point on screen (viewport-local px) mapped back to the unzoomed viewport it covers. */
export function unzoomViewportPoint(x: number, y: number, zoom: BrowserViewportZoom): { x: number; y: number } {
  return { x: (x - zoom.x) / zoom.scale, y: (y - zoom.y) / zoom.scale };
}

type TouchGesture =
  | { kind: "pinch"; distance: number; zoom: BrowserViewportZoom; anchorX: number; anchorY: number }
  | { kind: "pan"; startX: number; startY: number; zoom: BrowserViewportZoom; moved: boolean };

export interface RemoteBrowserProps {
  baseUrl?: string;
  browserId?: string | null;
  deviceToken?: string;
  options?: BrowserSubscribeOptions;
  session?: UseRemoteBrowserResult;
  controls?: React.ReactNode;
  className?: string;
  onFrame?: (frame: BrowserFrame) => void;
  onStateChange?: (state: BrowserStateMessage) => void;
  onStatusChange?: (status: RemoteBrowserStatus) => void;
  onPointClick?: (point: RemoteBrowserPointClickEvent) => Promise<unknown> | void;
  refusal?: RemoteInputRefusal | null;
  onRefusal?: (refusal: RemoteInputRefusal) => void;
  onClearRefusal?: () => void;
}

export const RemoteBrowser: React.FC<RemoteBrowserProps> = (props) => {
  if (props.session) {
    return <RemoteBrowserView {...props} session={props.session} />;
  }
  return <RemoteBrowserWithSelfSession {...props} />;
};

const RemoteBrowserWithSelfSession: React.FC<RemoteBrowserProps> = (props) => {
  const session = useRemoteBrowser({
    baseUrl: props.baseUrl ?? "",
    browserId: props.browserId ?? null,
    deviceToken: props.deviceToken ?? "",
    options: props.options,
  });

  return <RemoteBrowserView {...props} session={session} />;
};

const RemoteBrowserView: React.FC<RemoteBrowserProps & { session: UseRemoteBrowserResult }> = ({
  session,
  controls,
  className = "",
  onFrame,
  onStateChange,
  onStatusChange,
  onPointClick,
  refusal: propRefusal,
  onRefusal,
  onClearRefusal,
}) => {
  const {
    status,
    frame,
    imageUrl,
    browserState,
    error,
    reconnect,
    confirmPresented,
    sendAck,
  } = session;

  const [displayedFrame, setDisplayedFrame] = useState<DecodedBrowserFrame | null>(null);
  const [localRefusal, setLocalRefusal] = useState<RemoteInputRefusal | null>(null);
  const activeRefusal = propRefusal ?? localRefusal;
  const [tapMarker, setTapMarker] = useState<{ id: number; x: number; y: number } | null>(null);
  const tapCounterRef = useRef(0);
  // Pinch-zoom and pan are touch-only; a fine pointer keeps the identity transform.
  const coarsePointer = useCoarsePointer();
  const [zoom, setZoom] = useState<BrowserViewportZoom>(BROWSER_ZOOM_IDENTITY);
  const zoomRef = useRef<BrowserViewportZoom>(BROWSER_ZOOM_IDENTITY);
  zoomRef.current = zoom;
  const gestureRef = useRef<TouchGesture | null>(null);
  // the click a browser may synthesise after a pinch or pan is not a tap on the page
  const suppressClickRef = useRef(false);
  const reconnectButtonRef = useRef<HTMLButtonElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const displayedFrameRef = useRef<DecodedBrowserFrame | null>(null);
  displayedFrameRef.current = displayedFrame;
  const framesByUrlRef = useRef<Map<string, DecodedBrowserFrame>>(new Map());
  const activeStreamIdRef = useRef<number | null>(null);

  // Bind each incoming frame to its exact image URL identity and evict stale/cancelled entries (R4-11)
  useEffect(() => {
    if (imageUrl && frame) {
      // Clear cache on stream or instance identity change
      const streamId = frame.metadata.streamId;
      if (activeStreamIdRef.current !== null && activeStreamIdRef.current !== streamId) {
        framesByUrlRef.current.clear();
      }
      activeStreamIdRef.current = streamId;

      framesByUrlRef.current.set(imageUrl, frame);

      // Evict superseded / cancelled frames: retain at most displayed and loading frames
      const displayedSeq = displayedFrameRef.current?.seq ?? -1;
      for (const [url, cachedFrame] of framesByUrlRef.current.entries()) {
        if (url !== imageUrl) {
          // If frame is older than displayed or older than current pending frame when cache exceeds cap
          if (cachedFrame.seq < displayedSeq || (cachedFrame.seq < frame.seq && framesByUrlRef.current.size > 2)) {
            framesByUrlRef.current.delete(url);
          }
        }
      }

      // Hard cap to at most 2 retained frames (1 displayed + 1 pending loading)
      if (framesByUrlRef.current.size > 2) {
        const sorted = Array.from(framesByUrlRef.current.entries()).sort((a, b) => a[1].seq - b[1].seq);
        while (sorted.length > 2) {
          const oldest = sorted.shift();
          if (oldest && oldest[0] !== imageUrl) {
            framesByUrlRef.current.delete(oldest[0]);
          }
        }
      }
    }
  }, [imageUrl, frame]);

  // If imageUrl is cleared (e.g. backgrounded or disconnected), clear displayedFrame & URL cache
  useEffect(() => {
    if (!imageUrl) {
      setDisplayedFrame(null);
      setZoom(BROWSER_ZOOM_IDENTITY);
      framesByUrlRef.current.clear();
      activeStreamIdRef.current = null;
    }
  }, [imageUrl]);

  // Clear URL frame cache on unmount or disconnect
  useEffect(() => {
    return () => {
      framesByUrlRef.current.clear();
      activeStreamIdRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (status === "closed") reconnectButtonRef.current?.focus();
  }, [status]);

  useEffect(() => {
    if (!tapMarker) return;
    const timer = setTimeout(() => setTapMarker(null), 600);
    return () => clearTimeout(timer);
  }, [tapMarker]);

  useEffect(() => {
    if (frame && onFrame) {
      onFrame(frame);
    }
  }, [frame, onFrame]);

  useEffect(() => {
    if (browserState && onStateChange) {
      onStateChange(browserState);
    }
  }, [browserState, onStateChange]);

  useEffect(() => {
    if (onStatusChange) {
      onStatusChange(status);
    }
  }, [status, onStatusChange]);

  const viewportPoint = (touch: { clientX: number; clientY: number }, rect: DOMRect) => ({
    x: touch.clientX - rect.left,
    y: touch.clientY - rect.top,
  });

  const handleTouchStart = (e: React.TouchEvent<HTMLDivElement>) => {
    if (!coarsePointer || !viewportRef.current) return;
    suppressClickRef.current = false;
    const rect = viewportRef.current.getBoundingClientRect();
    if (e.touches.length >= 2) {
      const a = viewportPoint(e.touches[0], rect);
      const b = viewportPoint(e.touches[1], rect);
      const distance = Math.hypot(b.x - a.x, b.y - a.y);
      if (distance <= 0) return;
      // the frame point under the fingers' midpoint stays under it while they spread
      const anchor = unzoomViewportPoint((a.x + b.x) / 2, (a.y + b.y) / 2, zoomRef.current);
      gestureRef.current = { kind: "pinch", distance, zoom: zoomRef.current, anchorX: anchor.x, anchorY: anchor.y };
      suppressClickRef.current = true;
    } else if (e.touches.length === 1) {
      const p = viewportPoint(e.touches[0], rect);
      gestureRef.current = { kind: "pan", startX: p.x, startY: p.y, zoom: zoomRef.current, moved: false };
    }
  };

  const handleTouchMove = (e: React.TouchEvent<HTMLDivElement>) => {
    const gesture = gestureRef.current;
    if (!coarsePointer || !gesture || !viewportRef.current) return;
    const rect = viewportRef.current.getBoundingClientRect();
    if (gesture.kind === "pinch" && e.touches.length >= 2) {
      const a = viewportPoint(e.touches[0], rect);
      const b = viewportPoint(e.touches[1], rect);
      const scale = gesture.zoom.scale * (Math.hypot(b.x - a.x, b.y - a.y) / gesture.distance);
      const midX = (a.x + b.x) / 2;
      const midY = (a.y + b.y) / 2;
      setZoom(clampBrowserZoom(
        { scale, x: midX - gesture.anchorX * scale, y: midY - gesture.anchorY * scale },
        rect.width,
        rect.height,
      ));
    } else if (gesture.kind === "pan" && e.touches.length === 1) {
      const p = viewportPoint(e.touches[0], rect);
      const dx = p.x - gesture.startX;
      const dy = p.y - gesture.startY;
      if (!gesture.moved && Math.hypot(dx, dy) < BROWSER_PAN_SLOP_PX) return;
      gesture.moved = true;
      suppressClickRef.current = true;
      // nothing to pan at 1x: the whole frame is already on screen
      if (gesture.zoom.scale === 1) return;
      setZoom(clampBrowserZoom(
        { scale: gesture.zoom.scale, x: gesture.zoom.x + dx, y: gesture.zoom.y + dy },
        rect.width,
        rect.height,
      ));
    }
  };

  const handleTouchEnd = (e: React.TouchEvent<HTMLDivElement>) => {
    if (!coarsePointer) return;
    // lifting one finger of a pinch ends the pinch; the remaining finger does not start a pan
    if (e.touches.length === 0) gestureRef.current = null;
  };

  const handleViewportClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    // Strictly use displayedFrame - only clicks on committed, rendered frames are valid!
    if (!displayedFrame || !imageUrl || !viewportRef.current) {
      return;
    }

    const rect = viewportRef.current.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;

    const clickX = e.clientX - rect.left;
    const clickY = e.clientY - rect.top;
    // a zoomed tap lands on the frame point under the finger, not the unzoomed one
    const unzoomed = unzoomViewportPoint(clickX, clickY, zoom);

    const point = resolveViewportClickParams(
      unzoomed.x,
      unzoomed.y,
      rect.width,
      rect.height,
      displayedFrame,
      displayedFrame.seq,
    );
    if (!point) {
      return;
    }

    tapCounterRef.current += 1;
    setTapMarker({ id: tapCounterRef.current, x: clickX, y: clickY });

    if (!onPointClick) {
      return;
    }

    try {
      const res = onPointClick(point);
      if (res && typeof (res as Promise<unknown>).catch === "function") {
        (res as Promise<unknown>).catch((err: unknown) => {
          const refusal = buildRemoteInputRefusal(err);
          setLocalRefusal(refusal);
          onRefusal?.(refusal);
        });
      }
    } catch (err: unknown) {
      const refusal = buildRemoteInputRefusal(err);
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
    }
  };

  const handleImageLoad = (boundUrl: string, boundFrame: DecodedBrowserFrame | null) => {
    // Resolve the exact frame corresponding to this image identity
    const frameToCommit = (boundUrl ? framesByUrlRef.current.get(boundUrl) : null) ?? boundFrame;
    if (!frameToCommit) return;

    // Discard late-loading frames that arrive out of order
    if (displayedFrameRef.current && frameToCommit.seq < displayedFrameRef.current.seq) {
      return;
    }

    setDisplayedFrame(frameToCommit);

    // Evict any frames older than the newly committed displayed frame (R4-11)
    for (const [url, cachedFrame] of framesByUrlRef.current.entries()) {
      if (cachedFrame.seq < frameToCommit.seq) {
        framesByUrlRef.current.delete(url);
      }
    }

    if (confirmPresented) {
      confirmPresented(frameToCommit.metadata.streamId, frameToCommit.seq);
    } else if (sendAck) {
      sendAck(frameToCommit.metadata.streamId, frameToCommit.seq);
    }
  };

  const handleImageError = (boundUrl: string) => {
    // A frame that cannot be decoded is never committed, but it must still be acknowledged: an unconfirmed
    // frame would let the ACK-deadline sweeper close the whole session over one bad image.
    const failed = framesByUrlRef.current.get(boundUrl);
    framesByUrlRef.current.delete(boundUrl);
    if (!failed) return;
    if (displayedFrameRef.current && failed.seq > displayedFrameRef.current.seq) {
      setDisplayedFrame(null);
    }
    if (confirmPresented) {
      confirmPresented(failed.metadata.streamId, failed.seq);
    } else if (sendAck) {
      sendAck(failed.metadata.streamId, failed.seq);
    }
  };

  return (
    <div
      className={`relative flex flex-col w-full h-full bg-chat-screen text-chat-foreground overflow-hidden select-none ${className}`}
    >
      {controls && (
        <div data-testid="remote-browser-controls-slot" className="shrink-0 z-10">
          {controls}
        </div>
      )}

      {activeRefusal && (
        <div
          data-testid="remote-browser-refusal-banner"
          role="alert"
          className="absolute top-2 left-4 right-4 z-30 max-w-xl mx-auto flex items-start justify-between gap-2 pl-3 pr-1 py-1 bg-chat-surface-raised border border-chat-border rounded text-sm text-chat-foreground animate-in fade-in slide-in-from-top-2 duration-150 motion-reduce:animate-none"
        >
          <div className="flex items-start gap-2 flex-1 min-w-0 py-2">
            <CircleAlert className="size-4 shrink-0 mt-0.5 text-chat-danger" aria-hidden="true" />
            <div className="flex flex-col flex-1 min-w-0 gap-1">
              <span data-testid="remote-browser-refusal-message" className="font-medium break-words">
                {describeRemoteInputRefusal(activeRefusal)}
              </span>
              {activeRefusal.remediation && (
                <span
                  data-testid="remote-browser-refusal-remediation"
                  className="text-xs text-chat-foreground-secondary"
                >
                  {activeRefusal.remediation}
                </span>
              )}
              <details className="text-xs text-chat-foreground-secondary">
                <summary className={`cursor-pointer w-fit rounded ${BROWSER_FOCUS_RING}`}>Details</summary>
                <div className="mt-1 flex flex-col gap-0.5 font-mono break-words">
                  <span data-testid="remote-browser-refusal-code">{activeRefusal.code || "REFUSED"}</span>
                  <span data-testid="remote-browser-refusal-reason">{activeRefusal.reason}</span>
                </div>
              </details>
            </div>
          </div>
          <button
            type="button"
            data-testid="remote-browser-refusal-dismiss"
            onClick={() => {
              setLocalRefusal(null);
              onClearRefusal?.();
            }}
            className={`shrink-0 flex size-11 items-center justify-center rounded text-chat-foreground-secondary hover:text-chat-foreground hover:bg-chat-surface-hover transition-colors ${BROWSER_FOCUS_RING}`}
            aria-label="Dismiss notice"
          >
            <X className="size-4" aria-hidden="true" />
          </button>
        </div>
      )}

      <div
        ref={viewportRef}
        data-testid="remote-browser-viewport"
        data-retained-frames={framesByUrlRef.current.size}
        data-zoom-scale={zoom.scale}
        className={`relative flex-1 w-full h-full flex items-center justify-center overflow-hidden ${coarsePointer ? "touch-none" : "touch-manipulation"} [@media(pointer:fine)]:cursor-crosshair`}
        onClick={handleViewportClick}
        onTouchStart={handleTouchStart}
        onTouchMove={handleTouchMove}
        onTouchEnd={handleTouchEnd}
        onTouchCancel={handleTouchEnd}
      >
        {imageUrl ? (
          <div
            data-testid="remote-browser-zoom-layer"
            className="w-full h-full pointer-events-none"
            style={
              zoom.scale === 1
                ? undefined
                : { transform: `translate(${zoom.x}px, ${zoom.y}px) scale(${zoom.scale})`, transformOrigin: "0 0" }
            }
          >
            <img
              src={imageUrl}
              alt="Remote browser stream"
              className="w-full h-full object-contain pointer-events-none"
              draggable={false}
              onLoad={() => handleImageLoad(imageUrl, frame)}
              onError={() => handleImageError(imageUrl)}
            />
          </div>
        ) : (
          <div role="status" className="flex flex-col items-center justify-center text-chat-foreground-secondary text-sm">
            {status === "opening" && <span>Connecting to the browser...</span>}
            {status === "ready" && <span>Waiting for the page...</span>}
            {status === "paused" && (
              <span>
                {browserState?.pauseReason === "hidden" ? "Paused while the page is hidden on your computer" : "Paused"}
              </span>
            )}
            {status === "closed" && <span>The browser disconnected</span>}
          </div>
        )}

        {tapMarker && (
          <span
            key={tapMarker.id}
            data-testid="remote-browser-tap-marker"
            aria-hidden="true"
            style={{ left: tapMarker.x, top: tapMarker.y }}
            className="absolute z-10 size-8 -translate-x-1/2 -translate-y-1/2 rounded-full bg-chat-primary/40 pointer-events-none animate-out fade-out duration-500 fill-mode-forwards motion-reduce:animate-none"
          />
        )}

        {zoom.scale > 1 && (
          <button
            type="button"
            data-testid="remote-browser-zoom-reset"
            onClick={(event) => {
              event.stopPropagation();
              setZoom(BROWSER_ZOOM_IDENTITY);
            }}
            className={`absolute bottom-2 right-2 z-20 min-h-11 px-4 rounded-full bg-chat-surface-raised border border-chat-border text-sm font-medium text-chat-foreground hover:bg-chat-surface-hover transition-colors ${BROWSER_FOCUS_RING}`}
          >
            Fit page
          </button>
        )}

        {status === "paused" && imageUrl && (
          <div
            data-testid="remote-browser-paused-badge"
            role="status"
            className="absolute top-2 right-2 px-2 py-1 bg-chat-surface-raised text-status-warning border border-chat-border text-xs rounded font-medium pointer-events-none"
          >
            {browserState?.pauseReason === "hidden" ? "Paused while hidden on your computer" : "Paused"}
          </div>
        )}

        {status === "opening" && imageUrl && (
          <div
            role="status"
            className="absolute top-2 right-2 px-2 py-1 bg-chat-surface-raised text-chat-foreground-secondary border border-chat-border text-xs rounded font-medium pointer-events-none"
          >
            Reconnecting...
          </div>
        )}

        {status === "closed" && (
          <div className="absolute inset-0 bg-chat-screen/80 flex flex-col items-center justify-center gap-3 p-4 z-20">
            <p role="status" className="text-chat-foreground text-center">
              {error ? userFacingError(error) : "The connection to your computer dropped."}
            </p>
            {error ? (
              <details className="max-w-full text-xs text-chat-foreground-secondary">
                <summary className={`cursor-pointer w-fit mx-auto rounded ${BROWSER_FOCUS_RING}`}>Details</summary>
                <span data-testid="remote-browser-closed-reason" className="mt-1 block font-mono break-words">{error.message}</span>
              </details>
            ) : null}
            <button
              ref={reconnectButtonRef}
              type="button"
              onClick={reconnect}
              className={`min-h-11 px-5 bg-chat-primary text-chat-primary-foreground hover:brightness-110 rounded text-sm font-medium transition-colors ${BROWSER_FOCUS_RING}`}
            >
              Reconnect
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
