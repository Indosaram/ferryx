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

  const handleViewportClick = (e: React.MouseEvent<HTMLDivElement>) => {
    // Strictly use displayedFrame - only clicks on committed, rendered frames are valid!
    if (!displayedFrame || !imageUrl || !viewportRef.current) {
      return;
    }

    const rect = viewportRef.current.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;

    const clickX = e.clientX - rect.left;
    const clickY = e.clientY - rect.top;

    const point = resolveViewportClickParams(
      clickX,
      clickY,
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
        className="relative flex-1 w-full h-full flex items-center justify-center overflow-hidden touch-manipulation [@media(pointer:fine)]:cursor-crosshair"
        onClick={handleViewportClick}
      >
        {imageUrl ? (
          <img
            src={imageUrl}
            alt="Remote browser stream"
            className="w-full h-full object-contain pointer-events-none"
            draggable={false}
            onLoad={() => handleImageLoad(imageUrl, frame)}
            onError={() => handleImageError(imageUrl)}
          />
        ) : (
          <div role="status" className="flex flex-col items-center justify-center text-chat-foreground-secondary text-sm">
            {status === "opening" && <span>Connecting to remote browser...</span>}
            {status === "ready" && <span>Waiting for screencast stream...</span>}
            {status === "paused" && (
              <span>
                Stream paused {browserState?.pauseReason ? `(${browserState.pauseReason})` : ""}
              </span>
            )}
            {status === "closed" && <span>Remote browser disconnected</span>}
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

        {status === "paused" && imageUrl && (
          <div
            data-testid="remote-browser-paused-badge"
            role="status"
            className="absolute top-2 right-2 px-2 py-1 bg-chat-surface-raised text-status-warning border border-chat-border text-xs rounded font-medium pointer-events-none"
          >
            Paused {browserState?.pauseReason ? `(${browserState.pauseReason})` : ""}
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
