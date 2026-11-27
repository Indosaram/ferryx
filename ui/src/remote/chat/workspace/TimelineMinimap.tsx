import React, { useCallback, useRef, useState } from "react";
import { cn } from "../../../lib/cn";
import {
  deriveTimelineMinimapItems,
  resolveTimelineMinimapHeightStyle,
  resolveTimelineMinimapIndexFromPointer,
  resolveTimelineMinimapTopPercent,
  TIMELINE_MINIMAP_MIN_ITEMS,
  type TimelineMinimapItem,
} from "./timelineMinimapItems";

/**
 * Timeline minimap, ported from t3code `chat/TimelineMinimap.tsx` (MIT), in the workspace's
 * own idiom: a narrow rail on desktop widths (fine pointers only — a touch reader scrolls the
 * stream itself) with one marker per user turn. Hover names the turn; a press jumps to it.
 */
export interface TimelineMinimapProps {
  readonly messages: readonly { readonly id: string; readonly role: string; readonly content: string }[];
  /** Marker the viewport is reading, resolved by the owner from the last scroll snapshot. */
  readonly currentIndex: number | null;
  /** Jump the stream to this message id (the workspace scrolls its own container). */
  readonly onNavigate: (messageId: string) => void;
}

/** One line of a marker's preview: the turn's own words, never a paragraph. */
function previewText(item: TimelineMinimapItem): string {
  const source = item.userText && item.userText.trim().length > 0 ? item.userText : item.assistantText;
  const flat = (source ?? "").replace(/\s+/g, " ").trim();
  return flat.length > 60 ? `${flat.slice(0, 59)}…` : flat;
}

export const TimelineMinimap: React.FC<TimelineMinimapProps> = ({
  messages,
  currentIndex,
  onNavigate,
}) => {
  const items = deriveTimelineMinimapItems(messages);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const railRef = useRef<HTMLDivElement | null>(null);

  const updateActiveFromPointer = useCallback((clientY: number): void => {
    const rail = railRef.current;
    if (!rail) return;
    const rect = rail.getBoundingClientRect();
    setActiveIndex(
      resolveTimelineMinimapIndexFromPointer({
        itemCount: items.length,
        railTop: rect.top,
        railHeight: rect.height,
        pointerY: clientY,
      }),
    );
  }, [items.length]);

  if (items.length < TIMELINE_MINIMAP_MIN_ITEMS) return null;

  const activeItem = activeIndex === null ? null : items[activeIndex] ?? null;
  const resolvedCurrent = currentIndex !== null && currentIndex >= 0 && currentIndex < items.length
    ? currentIndex
    : null;

  return (
    <div
      data-testid="timeline-minimap"
      className={cn(
        "pointer-events-auto absolute right-0 top-0 z-30 hidden h-full w-10 select-none",
        "[@media(pointer:fine)_and_(min-width:1024px)]:block",
      )}
    >
      <div
        className="relative flex h-full w-full items-center justify-center"
        onMouseMove={(event) => updateActiveFromPointer(event.clientY)}
        onMouseLeave={() => setActiveIndex(null)}
      >
        <div
          ref={railRef}
          data-testid="timeline-minimap-rail"
          className="relative w-2 cursor-pointer rounded-full bg-chat-surface-raised"
          style={{ height: resolveTimelineMinimapHeightStyle(items.length) }}
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            const index = resolveTimelineMinimapIndexFromPointer({
              itemCount: items.length,
              railTop: rect.top,
              railHeight: rect.height,
              pointerY: event.clientY,
            });
            const item = items[index];
            if (item) onNavigate(item.id);
          }}
        >
          {items.map((item, index) => (
            <button
              key={item.id}
              type="button"
              data-testid="timeline-minimap-item"
              data-active={index === activeIndex ? "true" : undefined}
              data-current={index === resolvedCurrent ? "true" : undefined}
              aria-label={previewText(item) || `Turn ${index + 1}`}
              onClick={(event) => {
                event.stopPropagation();
                onNavigate(item.id);
              }}
              onMouseEnter={() => setActiveIndex(index)}
              className="absolute left-1/2 size-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-chat-foreground-secondary/60 transition-colors hover:bg-chat-foreground"
              style={{
                top: `${resolveTimelineMinimapTopPercent(index, items.length)}%`,
                ...(index === resolvedCurrent ? { backgroundColor: "var(--chat-foreground, currentColor)" } : {}),
              }}
            />
          ))}
        </div>
        {activeItem ? (
          <div
            data-testid="timeline-minimap-preview"
            role="status"
            className="pointer-events-none absolute right-10 max-w-56 truncate rounded-md border border-chat-border bg-chat-surface-raised px-2 py-1 text-xs text-chat-foreground"
          >
            {previewText(activeItem)}
          </div>
        ) : null}
      </div>
    </div>
  );
};
