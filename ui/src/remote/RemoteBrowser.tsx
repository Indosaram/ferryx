/**
 * RemoteBrowser Component (§4.5, Phase 5)
 *
 * Renders live WKWebView screencast with letterbox-aware coordinate mapping
 * and a dedicated controls slot for Phase 6 composition.
 */

import React, { useEffect, useRef, useState } from "react";
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
      // Discard clicks in letterbox margins (§4.5), but explain why no action was taken
      const refusal: RemoteInputRefusal = {
        code: "LETTERBOX_MARGIN",
        reason: "Click was outside active browser viewport (in letterbox margin)",
        inputClass: "point",
        remediation: "Click within the active webpage stream area.",
      };
      setLocalRefusal(refusal);
      onRefusal?.(refusal);
      return;
    }

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
    if (confirmPresented) {
      confirmPresented(failed.metadata.streamId, failed.seq);
    } else if (sendAck) {
      sendAck(failed.metadata.streamId, failed.seq);
    }
  };

  return (
    <div
      className={`relative flex flex-col w-full h-full bg-[#0a0a0a] text-[#f5f5f5] overflow-hidden select-none ${className}`}
    >
      {/* Controls slot for Phase 6 composition */}
      {controls && (
        <div data-testid="remote-browser-controls-slot" className="shrink-0 z-10">
          {controls}
        </div>
      )}

      {/* Refused Input Explanation Banner (T22, GAP-7) */}
      {activeRefusal && (
        <div
          data-testid="remote-browser-refusal-banner"
          role="alert"
          className="absolute top-2 left-4 right-4 z-30 max-w-xl mx-auto flex items-center justify-between gap-3 px-3 py-2 bg-[#ff6467]/15 border border-[#ff6467]/40 rounded backdrop-blur-sm text-xs text-[#ff6467] shadow-lg animate-in fade-in slide-in-from-top-2 duration-150"
        >
          <div className="flex items-start gap-2 flex-1 min-w-0">
            <span
              data-testid="remote-browser-refusal-code"
              className="shrink-0 px-1.5 py-0.5 rounded bg-[#ff6467]/20 border border-[#ff6467]/30 text-[10px] font-semibold uppercase tracking-wider"
            >
              {activeRefusal.code || "REFUSED"}
            </span>
            <div className="flex flex-col flex-1 min-w-0">
              <span
                data-testid="remote-browser-refusal-reason"
                className="font-medium text-[#f5f5f5] break-words"
              >
                {activeRefusal.reason}
              </span>
              {activeRefusal.remediation && (
                <span
                  data-testid="remote-browser-refusal-remediation"
                  className="text-[11px] text-[#ff6467]/80 mt-0.5"
                >
                  {activeRefusal.remediation}
                </span>
              )}
            </div>
          </div>
          <button
            type="button"
            data-testid="remote-browser-refusal-dismiss"
            onClick={() => {
              setLocalRefusal(null);
              onClearRefusal?.();
            }}
            className="shrink-0 text-[#838383] hover:text-[#f5f5f5] text-sm leading-none p-1 rounded hover:bg-[#ff6467]/10 transition"
            aria-label="Dismiss refusal notice"
          >
            ×
          </button>
        </div>
      )}

      {/* Main Viewport */}
      <div
        ref={viewportRef}
        data-testid="remote-browser-viewport"
        data-retained-frames={framesByUrlRef.current.size}
        className="relative flex-1 w-full h-full flex items-center justify-center overflow-hidden cursor-crosshair"
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
          <div className="flex flex-col items-center justify-center text-[#818181] text-sm">
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

        {/* Status Overlays */}
        {status === "paused" && imageUrl && (
          <div
            data-testid="remote-browser-paused-badge"
            className="absolute top-2 right-2 px-2 py-1 bg-[#ffb900]/15 text-[#ffb900] border border-[#ffb900]/30 text-xs rounded font-medium pointer-events-none"
          >
            Paused {browserState?.pauseReason ? `(${browserState.pauseReason})` : ""}
          </div>
        )}

        {status === "opening" && imageUrl && (
          <div className="absolute top-2 right-2 px-2 py-1 bg-[#346bf1]/15 text-[#346bf1] border border-[#346bf1]/30 text-xs rounded font-medium pointer-events-none">
            Reconnecting...
          </div>
        )}

        {status === "closed" && (
          <div className="absolute inset-0 bg-black/70 flex flex-col items-center justify-center p-4 z-20">
            <p className="text-[#f5f5f5] mb-2">
              {error ? error.message : "Connection closed"}
            </p>
            <button
              type="button"
              onClick={reconnect}
              className="px-3 py-1.5 bg-[#1a1b1b] hover:bg-[#141414] text-[#f5f5f5] rounded text-sm font-medium transition"
            >
              Reconnect
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
