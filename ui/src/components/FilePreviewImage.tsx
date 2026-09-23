import { ExternalLink, Maximize2, Minus, Plus, RefreshCw, RotateCcw, Scan } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent, ReactNode, SyntheticEvent } from "react";

import { IconButton } from "./ui/IconButton";
import { cn } from "../lib/cn";
import { FILE_PREVIEW_ZOOM } from "../lib/filePreviewTypes";
import type { FilePreviewImageProps } from "../lib/filePreviewTypes";

/**
 * Bounded read-only image viewport (plan task 4).
 *
 * The renderer is deliberately incapable of widening the backend's decision:
 * it only displays a capability URL the backend already admitted, with an
 * allowlisted raster MIME type. It never sniffs bytes, never re-derives a size
 * or pixel bound (the payload carries no intrinsic dimensions — the 32 MiB /
 * 40 MP limits are enforced before `mediaUrl` exists), and never renders SVG as
 * an active document. Failures stay inside the modal with retry, reload and an
 * explicit external action; nothing is launched implicitly.
 *
 * Every visual state is driven by real `load`/`error` events on the actual
 * `img`. The element is keyed by generation+handle+attempt, so a replacement
 * preview (or a retry) unmounts the previous element with its in-flight load:
 * a late `load`/`error` from an image the modal already replaced can no longer
 * reach this component and corrupt the current view.
 */

/** Types this renderer draws with <img>. SVG stays an image, never a document. */
const IMAGE_MEDIA_TYPES: readonly string[] = [
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
  "image/bmp",
  "image/x-icon",
  "image/vnd.microsoft.icon",
  "image/svg+xml",
];

const ZOOM_MIN_PCT = Math.round(FILE_PREVIEW_ZOOM.min * 100);
const ZOOM_MAX_PCT = Math.round(FILE_PREVIEW_ZOOM.max * 100);
const ZOOM_STEP_PCT = Math.round(FILE_PREVIEW_ZOOM.step * 100);

type ViewState = {
  readonly mode: "fit" | "zoom";
  readonly zoom: number;
  readonly pan: { readonly x: number; readonly y: number };
};

type Natural = { readonly width: number; readonly height: number };

const INITIAL_VIEW: ViewState = { mode: "fit", zoom: 100, pan: { x: 0, y: 0 } };

function clampZoom(percent: number): number {
  return Math.min(ZOOM_MAX_PCT, Math.max(ZOOM_MIN_PCT, percent));
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`;
}

export function FilePreviewImage({
  payload,
  generation,
  onReload,
  onExternalOpen,
  onFailure,
}: FilePreviewImageProps) {
  const [view, setView] = useState<ViewState>(INITIAL_VIEW);
  const [natural, setNatural] = useState<Natural | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [attempt, setAttempt] = useState(0);
  const dragRef = useRef<{ pointerId: number; startX: number; startY: number; panX: number; panY: number } | null>(null);
  const reportedRef = useRef<string | null>(null);

  const mediaUrl = payload.mediaUrl;
  const supported =
    payload.kind === "image" &&
    typeof mediaUrl === "string" &&
    mediaUrl.length > 0 &&
    typeof payload.mediaType === "string" &&
    IMAGE_MEDIA_TYPES.includes(payload.mediaType);

  /** Element identity: a new request or a retry is a new DOM element, never a reused one. */
  const requestKey = `${generation}::${payload.handle}`;
  const sourceKey = `${requestKey}::${attempt}`;

  const failureRef = useRef(onFailure);
  failureRef.current = onFailure;

  // A replacement preview is a new view: fit, unpanned, not yet loaded.
  useEffect(() => {
    setView(INITIAL_VIEW);
    setNatural(null);
    setStatus("loading");
    setAttempt(0);
    dragRef.current = null;
  }, [requestKey]);

  // Metadata the renderer cannot display is reported once per request, and no
  // `img` element is ever created for it.
  useEffect(() => {
    if (supported || reportedRef.current === requestKey) return;
    reportedRef.current = requestKey;
    failureRef.current({
      reason: "UnsupportedFormat",
      message: "This file cannot be displayed as an image in the preview.",
      details: {
        reason: "UnsupportedFormat",
        displayName: payload.displayName,
        byteLength: payload.byteLength,
      },
    });
  }, [supported, requestKey, payload.displayName, payload.byteLength]);

  const handleLoad = useCallback((event: SyntheticEvent<HTMLImageElement>) => {
    const img = event.currentTarget;
    setNatural({ width: img.naturalWidth, height: img.naturalHeight });
    setStatus("ready");
  }, []);

  const handleError = useCallback(
    () => {
      setStatus("error");
      failureRef.current({
        reason: null,
        message: "The image could not be displayed. It may be corrupt, or the preview may no longer be valid.",
        details: null,
      });
    },
    [],
  );

  const setZoomPercent = useCallback((next: number) => {
    setView((current) => ({ mode: "zoom", zoom: clampZoom(next), pan: current.pan }));
  }, []);

  const handlePointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (view.mode !== "zoom" || event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        panX: view.pan.x,
        panY: view.pan.y,
      };
    },
    [view],
  );

  const handlePointerMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const x = drag.panX + (event.clientX - drag.startX);
    const y = drag.panY + (event.clientY - drag.startY);
    setView((current) => ({ ...current, pan: { x, y } }));
  }, []);

  const endDrag = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    dragRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }, []);

  const retry = useCallback(() => {
    setStatus("loading");
    setNatural(null);
    setAttempt((value) => value + 1);
  }, []);

  if (!supported) {
    return (
      <div
        data-testid="file-preview-image-unsupported"
        className="flex h-full w-full flex-col items-center justify-center gap-3 bg-background p-6 text-center"
      >
        <p className="text-sm font-medium text-foreground">This image format cannot be previewed</p>
        <p className="max-w-sm text-xs text-muted-foreground">
          {payload.displayName} ({formatBytes(payload.byteLength)}) is not one of the supported PNG, JPEG, GIF or
          WebP images. Open it externally to view it.
        </p>
        <div className="flex items-center gap-2">
          <ActionButton label="Open externally" icon={<ExternalLink className="size-3.5" />} onClick={onExternalOpen} />
          <ActionButton label="Reload file" icon={<RefreshCw className="size-3.5" />} onClick={onReload} />
        </div>
      </div>
    );
  }

  const zoomed = view.mode === "zoom";
  const panned = view.pan.x !== 0 || view.pan.y !== 0;
  const imageWidth = zoomed && natural ? `${(natural.width * view.zoom) / 100}px` : undefined;

  return (
    <div data-testid="file-preview-image" className="flex h-full w-full flex-col bg-background">
      <div
        data-testid="file-preview-image-viewport"
        role="group"
        aria-label={`${payload.displayName} image viewport`}
        className={cn(
          "relative flex flex-1 items-center justify-center overflow-hidden",
          zoomed && status === "ready" ? "cursor-grab touch-none active:cursor-grabbing" : null,
        )}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {status === "error" ? (
          <div
            data-testid="file-preview-image-error"
            className="flex flex-col items-center gap-3 p-6 text-center"
          >
            <p className="text-sm font-medium text-foreground">This image could not be displayed</p>
            <p className="max-w-sm text-xs text-muted-foreground">
              {payload.displayName} ({formatBytes(payload.byteLength)}) failed to decode, or its preview is no longer
              valid. Retry the image, reload the file, or open it externally.
            </p>
            <div className="flex items-center gap-2">
              <ActionButton label="Retry" icon={<RefreshCw className="size-3.5" />} onClick={retry} />
              <ActionButton label="Reload file" icon={<RotateCcw className="size-3.5" />} onClick={onReload} />
              <ActionButton
                label="Open externally"
                icon={<ExternalLink className="size-3.5" />}
                onClick={onExternalOpen}
              />
            </div>
          </div>
        ) : (
          <div
            data-testid="file-preview-image-canvas"
            className="flex max-h-full max-w-full items-center justify-center"
            style={panned ? { transform: `translate(${view.pan.x}px, ${view.pan.y}px)` } : undefined}
          >
            <img
              key={sourceKey}
              src={mediaUrl ?? undefined}
              alt={payload.displayName}
              draggable={false}
              decoding="async"
              onLoad={handleLoad}
              onError={handleError}
              className={cn(
                "select-none",
                zoomed ? "max-w-none" : "max-h-full max-w-full object-contain",
              )}
              style={imageWidth ? { width: imageWidth, height: "auto" } : undefined}
            />
          </div>
        )}
      </div>

      <div className="flex h-9 shrink-0 items-center gap-2 border-t border-border bg-card px-2">
        <span data-testid="file-preview-image-zoom" className="min-w-10 font-mono text-[11px] text-muted-foreground">
          {zoomed ? `${view.zoom}%` : "Fit"}
        </span>
        {natural ? (
          <span data-testid="file-preview-image-dimensions" className="font-mono text-[11px] text-muted-foreground/70">
            {natural.width} &#215; {natural.height}
          </span>
        ) : null}
        <span className="flex-1" />
        <IconButton
          label="Zoom out"
          size="sm"
          disabled={status !== "ready" || (zoomed && view.zoom <= ZOOM_MIN_PCT)}
          onClick={() => setZoomPercent((zoomed ? view.zoom : 100) - ZOOM_STEP_PCT)}
        >
          <Minus className="size-3.5" />
        </IconButton>
        <IconButton
          label="Zoom in"
          size="sm"
          disabled={status !== "ready" || (zoomed && view.zoom >= ZOOM_MAX_PCT)}
          onClick={() => setZoomPercent((zoomed ? view.zoom : 100) + ZOOM_STEP_PCT)}
        >
          <Plus className="size-3.5" />
        </IconButton>
        <IconButton
          label="Fit"
          size="sm"
          disabled={status !== "ready" || !zoomed}
          onClick={() => setView(INITIAL_VIEW)}
        >
          <Maximize2 className="size-3.5" />
        </IconButton>
        <IconButton
          label="Actual size"
          size="sm"
          disabled={status !== "ready"}
          onClick={() => setView({ mode: "zoom", zoom: 100, pan: { x: 0, y: 0 } })}
        >
          <Scan className="size-3.5" />
        </IconButton>
        <IconButton label="Reset view" size="sm" onClick={() => setView(INITIAL_VIEW)}>
          <RotateCcw className="size-3.5" />
        </IconButton>
      </div>
    </div>
  );
}

function ActionButton({
  label,
  icon,
  onClick,
}: {
  label: string;
  icon: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 rounded-md border border-border px-2.5 py-1 text-xs text-foreground",
        "hover:bg-accent focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
      )}
    >
      {icon}
      <span>{label}</span>
    </button>
  );
}
