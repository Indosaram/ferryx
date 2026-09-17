/**
 * RemoteBrowser Component (§4.5, Phase 5)
 *
 * Renders live WKWebView screencast with letterbox-aware coordinate mapping
 * and a dedicated controls slot for Phase 6 composition.
 */

import React, { useEffect, useRef, useState } from "react";
import type {
  BrowserFrame,
  BrowserStateMessage,
  BrowserSubscribeOptions,
  DecodedBrowserFrame,
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
  const uncommittedFrameRef = useRef<DecodedBrowserFrame | null>(null);

  // Track the latest incoming frame in ref so onLoad can commit it to displayedFrame
  useEffect(() => {
    uncommittedFrameRef.current = frame;
  }, [frame]);

  // If imageUrl is cleared (e.g. backgrounded or disconnected), clear displayedFrame
  useEffect(() => {
    if (!imageUrl) {
      setDisplayedFrame(null);
      uncommittedFrameRef.current = null;
    }
  }, [imageUrl]);

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

    const {
      imageWidth,
      imageHeight,
      streamId,
      documentGeneration,
      viewportRevision,
      browserInstanceId,
    } = displayedFrame.metadata;

    if (imageWidth <= 0 || imageHeight <= 0) return;

    const containerAspect = rect.width / rect.height;
    const imageAspect = imageWidth / imageHeight;

    let renderedWidth: number;
    let renderedHeight: number;
    let offsetLeft = 0;
    let offsetTop = 0;

    if (containerAspect > imageAspect) {
      // Letterboxed horizontally (pillarbox)
      renderedHeight = rect.height;
      renderedWidth = rect.height * imageAspect;
      offsetLeft = (rect.width - renderedWidth) / 2;
    } else {
      // Letterboxed vertically
      renderedWidth = rect.width;
      renderedHeight = rect.width / imageAspect;
      offsetTop = (rect.height - renderedHeight) / 2;
    }

    const clickX = e.clientX - rect.left;
    const clickY = e.clientY - rect.top;

    // Discard clicks in letterbox margins (§4.5)
    if (
      clickX < offsetLeft ||
      clickX > offsetLeft + renderedWidth ||
      clickY < offsetTop ||
      clickY > offsetTop + renderedHeight
    ) {
      return;
    }

    // Normalized coordinates (u, v) in [0, 1] relative to the actual image
    const u = Math.min(Math.max((clickX - offsetLeft) / renderedWidth, 0), 1);
    const v = Math.min(Math.max((clickY - offsetTop) / renderedHeight, 0), 1);

    onPointClick({
      u,
      v,
      streamId,
      seq: displayedFrame.seq,
      sequenceNumber: displayedFrame.seq,
      documentGeneration,
      viewportRevision,
      browserInstanceId,
    });
  };

  const handleImageLoad = () => {
    const frameToCommit = uncommittedFrameRef.current ?? frame;
    if (frameToCommit) {
      setDisplayedFrame(frameToCommit);
      if (confirmPresented) {
        confirmPresented(frameToCommit.metadata.streamId, frameToCommit.seq);
      } else if (sendAck) {
        sendAck(frameToCommit.metadata.streamId, frameToCommit.seq);
      }
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
        className="relative flex-1 w-full h-full flex items-center justify-center overflow-hidden cursor-crosshair"
        onClick={handleViewportClick}
      >
        {imageUrl ? (
          <img
            src={imageUrl}
            alt="Remote browser stream"
            className="w-full h-full object-contain pointer-events-none"
            draggable={false}
            onLoad={handleImageLoad}
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
