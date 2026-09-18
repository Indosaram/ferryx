import { useCallback, useEffect, useRef, useState } from "react";

import type { FilePreviewFailure, FilePreviewVideoProps } from "../lib/filePreviewTypes";

/**
 * Plan task 5: native `<video>` lifecycle for the file-preview modal.
 *
 * The renderer owns exactly one media element and nothing else: no transcoder,
 * no codec shim, no Media Source pipeline. The browser engine plays the
 * capability URL directly with its own controls, so a recognised container that
 * the engine cannot decode is a normal, recoverable outcome rather than a crash
 * — it surfaces as `UnsupportedFormat` with reload / external-open in place.
 *
 * Lifecycle rules:
 * - Nothing is fetched beyond metadata until the user presses play (no autoplay).
 * - Closing the preview or replacing the payload pauses the element, detaches
 *   `src` and calls `load()`, which aborts the in-flight range requests instead
 *   of leaving the capability stream open.
 * - Teardown makes the engine emit late `error` events on the element that is
 *   going away; those (and any error without a `MediaError`) are ignored, and a
 *   generation/handle guard keeps stale events from an older request out of the
 *   modal's failure channel.
 */

const MEDIA_ERR_ABORTED = 1;
const MEDIA_ERR_NETWORK = 2;
const MEDIA_ERR_DECODE = 3;
const MEDIA_ERR_SRC_NOT_SUPPORTED = 4;

/** Pause, detach the capability URL and reset the engine. Safe on a detached element. */
function releaseMedia(media: HTMLVideoElement | null) {
  if (!media) return;
  media.pause();
  media.removeAttribute("src");
  media.load();
}

function unsupportedFormatFailure(displayName: string, message: string): FilePreviewFailure {
  return {
    reason: "UnsupportedFormat",
    message,
    details: { reason: "UnsupportedFormat", displayName },
  };
}

/**
 * Maps a `MediaError` onto the frozen failure contract.
 *
 * Returns `null` for events the modal must not see: teardown aborts and error
 * events that carry no `MediaError` at all.
 */
function mediaFailure(displayName: string, error: MediaError | null): FilePreviewFailure | null {
  if (!error) return null;
  switch (error.code) {
    case MEDIA_ERR_ABORTED:
      return null;
    case MEDIA_ERR_NETWORK:
      return {
        reason: null,
        message: `Playback of ${displayName} stopped because the preview stream could not be read.`,
        details: null,
      };
    case MEDIA_ERR_DECODE:
    case MEDIA_ERR_SRC_NOT_SUPPORTED:
      return unsupportedFormatFailure(
        displayName,
        `${displayName} cannot be played here: this container or codec is not supported by the preview.`,
      );
    default:
      return {
        reason: null,
        message: `Playback of ${displayName} failed.`,
        details: null,
      };
  }
}

function formatBytes(byteLength: number): string {
  if (byteLength < 1024) return `${byteLength} B`;
  const units = ["KiB", "MiB", "GiB"];
  let value = byteLength / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

export function FilePreviewVideo({
  payload,
  generation,
  onReload,
  onExternalOpen,
  onFailure,
}: FilePreviewVideoProps) {
  const { displayName, mediaUrl, mediaType, byteLength, handle } = payload;
  /** One media element per request: a new generation or handle remounts it. */
  const instanceKey = `${generation}:${handle}`;
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const activeKeyRef = useRef(instanceKey);
  const onFailureRef = useRef(onFailure);
  const [failure, setFailure] = useState<FilePreviewFailure | null>(null);

  useEffect(() => {
    onFailureRef.current = onFailure;
  });

  useEffect(() => {
    activeKeyRef.current = instanceKey;
    setFailure(null);
    const media = videoRef.current;
    return () => releaseMedia(media);
  }, [instanceKey]);

  useEffect(() => {
    if (mediaUrl) return;
    const missing = unsupportedFormatFailure(
      displayName,
      `${displayName} has no playable media stream.`,
    );
    setFailure(missing);
    onFailureRef.current(missing);
  }, [instanceKey, mediaUrl, displayName]);

  const handleError = useCallback(
    (event: React.SyntheticEvent<HTMLVideoElement>) => {
      if (activeKeyRef.current !== instanceKey) return;
      const next = mediaFailure(displayName, event.currentTarget.error);
      if (!next) return;
      setFailure(next);
      onFailure(next);
    },
    [displayName, instanceKey, onFailure],
  );

  return (
    <div
      data-testid="file-preview-video"
      data-generation={generation}
      className="flex h-full min-h-0 flex-1 flex-col gap-2"
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-xs font-medium text-foreground" title={displayName}>
            {displayName}
          </p>
          <p className="text-[11px] text-muted-foreground">
            {mediaType ?? "unknown type"} · {formatBytes(byteLength)}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={onReload}
            data-testid="file-preview-video-reload"
            className="rounded-md border border-border px-2 py-1 text-xs font-medium hover:bg-accent/50"
          >
            Reload
          </button>
          <button
            type="button"
            onClick={onExternalOpen}
            data-testid="file-preview-video-external"
            className="rounded-md border border-border px-2 py-1 text-xs font-medium hover:bg-accent/50"
          >
            Open externally
          </button>
        </div>
      </div>

      {mediaUrl ? (
        <video
          key={instanceKey}
          ref={videoRef}
          data-testid="file-preview-video-element"
          className="min-h-0 w-full flex-1 rounded-md bg-black object-contain"
          src={mediaUrl}
          controls
          preload="metadata"
          playsInline
          aria-label={`Video preview of ${displayName}`}
          onError={handleError}
        />
      ) : null}

      {failure ? (
        <div
          role="alert"
          data-testid="file-preview-video-error"
          className="flex items-center justify-between gap-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-foreground"
        >
          <span className="min-w-0">{failure.message}</span>
          <button
            type="button"
            onClick={onReload}
            data-testid="file-preview-video-retry"
            className="shrink-0 rounded-md border border-border px-2 py-1 text-xs font-medium hover:bg-accent/50"
          >
            Try again
          </button>
        </div>
      ) : null}
    </div>
  );
}
