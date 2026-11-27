/**
 * `MediaPartView` (lane H5): a tool result's picture, fetched on demand.
 *
 * A `mediaRef` part carries no bytes: it names one content item of one tool call, and the bytes
 * are a separate `media` read (`get_media` over the IPC command, `GET .../media` over the route).
 * So the transcript never inlines base64 - it draws this view, which asks for the item once when
 * it scrolls into view and then hands the bytes to the lightbox every other picture in this app
 * already uses.
 *
 * The load is lazy for a reason that is not cosmetic: a session with fifty screenshots would
 * otherwise make fifty media reads on mount, each of them a request the gateway has to serve
 * before the reader sees the newest answer.
 */
import React, { useCallback, useEffect, useRef, useState } from "react";
import { ImageOff, Loader2 } from "lucide-react";
import { cn } from "../../../lib/cn";
import { ZoomableImage } from "../messages/imageLightbox";
import type { StructuredMediaBlob, StructuredMediaRef } from "../../../lib/structuredChatTypes";

/** A base64 payload as the `src` a browser can draw, or `null` when it is not an image at all. */
export function structuredMediaDataUrl(blob: StructuredMediaBlob): string | null {
  const mime = blob.mimeType.trim();
  // An empty or non-image type would make `<img>` guess, and a guess on a text payload draws a
  // broken thumbnail instead of saying the item is not a picture.
  if (!mime.startsWith("image/")) return null;
  if (blob.dataBase64.length === 0) return null;
  return `data:${mime};base64,${blob.dataBase64}`;
}

export interface MediaPartViewProps {
  readonly mediaRef: StructuredMediaRef;
  /** The bytes the lane already holds, if it read this item before. */
  readonly blob?: StructuredMediaBlob;
  /** Read one item; a second call for the same item is the same read. */
  readonly fetchMedia: (toolCallId: string, contentIndex: number) => Promise<StructuredMediaBlob | null>;
  readonly className?: string;
}

/**
 * The view. It draws the loaded picture, a spinner while the read is in flight, or a named
 * refusal - never an empty box. `alt` comes from the part when the provider supplied one, else
 * from the mime type, so a screen reader says what the picture is.
 */
export const MediaPartView: React.FC<MediaPartViewProps> = ({ mediaRef, blob, fetchMedia, className }) => {
  const [loaded, setLoaded] = useState<StructuredMediaBlob | null>(blob ?? null);
  const [failed, setFailed] = useState(false);
  const [pending, setPending] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  // One read per item: a re-render (a new part landing) must not re-ask for bytes already held.
  const requested = useRef(false);

  const key = `${mediaRef.toolCallId}:${mediaRef.contentIndex}`;

  const load = useCallback(async (): Promise<void> => {
    if (requested.current) return;
    requested.current = true;
    setPending(true);
    try {
      const next = await fetchMedia(mediaRef.toolCallId, mediaRef.contentIndex);
      if (next === null) {
        setFailed(true);
        return;
      }
      setLoaded(next);
    } finally {
      setPending(false);
    }
  }, [fetchMedia, mediaRef.contentIndex, mediaRef.toolCallId]);

  // A blob the lane already holds wins over a read: it is the same bytes, already paid for.
  useEffect(() => {
    if (blob !== undefined) {
      setLoaded(blob);
      setFailed(false);
      requested.current = true;
    }
  }, [blob]);

  // Load when the item is actually on screen. `IntersectionObserver` is absent in some test
  // environments and in older WebViews, where the picture is simply read at once - the read is
  // the point, laziness is the optimisation.
  useEffect(() => {
    if (loaded !== null || requested.current) return;
    const node = containerRef.current;
    if (node === null || typeof IntersectionObserver === "undefined") {
      void load();
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        observer.disconnect();
        void load();
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [load, loaded]);

  const src = loaded === null ? null : structuredMediaDataUrl(loaded);
  const alt = mediaRef.alt ?? mediaRef.mimeType;

  return (
    <div
      ref={containerRef}
      data-testid="structured-media"
      data-media-key={key}
      className={cn("flex max-w-[12.5rem] flex-col gap-1", className)}
    >
      {src !== null ? (
        <ZoomableImage src={src} alt={alt} thumbnailTestId="structured-media-thumbnail" />
      ) : pending ? (
        <div
          data-testid="structured-media-loading"
          className="flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface px-2.5 py-1.5 text-xs text-chat-foreground-secondary"
        >
          <Loader2 aria-hidden="true" className="size-3.5 shrink-0 animate-spin" />
          <span>Loading {mediaRef.mimeType}</span>
        </div>
      ) : (
        <div
          data-testid="structured-media-unavailable"
          className="flex items-center gap-2 rounded-lg border border-chat-border bg-chat-surface px-2.5 py-1.5 text-xs text-chat-foreground-secondary"
        >
          <ImageOff aria-hidden="true" className="size-3.5 shrink-0" />
          <span className="truncate">{failed ? "Couldn't load this attachment" : mediaRef.mimeType}</span>
        </div>
      )}
    </div>
  );
};
