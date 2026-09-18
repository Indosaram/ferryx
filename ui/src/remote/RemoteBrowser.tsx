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
  onPointClick?: (point: RemoteBrowserPointClickEvent) => void;
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
    if (!displayedFrame || !imageUrl || !viewportRef.current || !onPointClick) {
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
    if (!point) return;

    onPointClick(point);
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

  return (
    <div
      className={`relative flex flex-col w-full h-full bg-neutral-950 text-white overflow-hidden select-none ${className}`}
    >
      {/* Controls slot for Phase 6 composition */}
      {controls && (
        <div data-testid="remote-browser-controls-slot" className="shrink-0 z-10">
          {controls}
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
          />
        ) : (
          <div className="flex flex-col items-center justify-center text-neutral-500 text-sm">
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
            className="absolute top-2 right-2 px-2 py-1 bg-amber-900/80 text-amber-200 text-xs rounded font-medium pointer-events-none"
          >
            Paused {browserState?.pauseReason ? `(${browserState.pauseReason})` : ""}
          </div>
        )}

        {status === "opening" && imageUrl && (
          <div className="absolute top-2 right-2 px-2 py-1 bg-blue-900/80 text-blue-200 text-xs rounded font-medium pointer-events-none">
            Reconnecting...
          </div>
        )}

        {status === "closed" && (
          <div className="absolute inset-0 bg-black/70 flex flex-col items-center justify-center p-4 z-20">
            <p className="text-neutral-300 mb-2">
              {error ? error.message : "Connection closed"}
            </p>
            <button
              type="button"
              onClick={reconnect}
              className="px-3 py-1.5 bg-neutral-800 hover:bg-neutral-700 text-white rounded text-sm font-medium transition"
            >
              Reconnect
            </button>
          </div>
        )}
      </div>
    </div>
  );
};
