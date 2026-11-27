/**
 * Timeline minimap model, ported from t3code `chat/timelineMinimapItems.ts` and the
 * positioning helpers in `chat/MessagesTimeline.logic.ts` (MIT). The minimap shows one
 * marker per user turn and navigates by marker; only the pure model lives here.
 */

/** A minimap marker: one user turn with the assistant answer that follows it. */
export interface TimelineMinimapItem {
  readonly id: string;
  readonly userText: string | null;
  readonly assistantText: string | null;
}

/** User turns in order, each carrying the last assistant text before the next user turn. */
export function deriveTimelineMinimapItems(
  messages: readonly { readonly id: string; readonly role: string; readonly content: string }[],
): readonly TimelineMinimapItem[] {
  const items: TimelineMinimapItem[] = [];
  for (let index = 0; index < messages.length; index += 1) {
    const message = messages[index];
    if (!message || message.role !== "user") continue;
    let assistantText: string | null = null;
    for (let next = index + 1; next < messages.length; next += 1) {
      const later = messages[next];
      if (!later) continue;
      if (later.role === "user") break;
      if (later.role === "assistant" && later.content) assistantText = later.content;
    }
    items.push({ id: message.id, userText: message.content, assistantText });
  }
  return items;
}

/** t3code shows the rail from two markers on; fewer would be noise. */
export const TIMELINE_MINIMAP_MIN_ITEMS = 2;

const TIMELINE_MINIMAP_ITEM_SPACING = 8;
const TIMELINE_MINIMAP_MAX_HEIGHT_PX = 320;

/** The rail's height: one gap per marker pair, capped so a long thread stays usable. */
export function resolveTimelineMinimapHeightStyle(itemCount: number): string {
  const naturalHeight = Math.max(1, (itemCount - 1) * TIMELINE_MINIMAP_ITEM_SPACING);
  return `${Math.min(naturalHeight, TIMELINE_MINIMAP_MAX_HEIGHT_PX)}px`;
}

/** Marker i of n sits at an even share of the rail; the ends keep their half-gap. */
export function resolveTimelineMinimapTopPercent(index: number, itemCount: number): number {
  if (itemCount <= 1) return 0;
  const clamped = Math.min(Math.max(0, Math.trunc(index)), itemCount - 1);
  return (clamped / (itemCount - 1)) * 100;
}

/** The marker a pointer names: nearest share of the rail height, clamped into range. */
export function resolveTimelineMinimapIndexFromPointer(input: {
  readonly itemCount: number;
  readonly railTop: number;
  readonly railHeight: number;
  readonly pointerY: number;
}): number {
  if (input.itemCount <= 0 || input.railHeight <= 0) return 0;
  const fraction = (input.pointerY - input.railTop) / input.railHeight;
  const unclamped = Math.round(fraction * (input.itemCount - 1));
  return Math.min(Math.max(0, unclamped), input.itemCount - 1);
}

/**
 * The marker the viewport is reading: the last item whose user message starts at or above
 * the viewport's middle. An empty or unmeasured range reads as "at the latest turn".
 */
export function resolveTimelineMinimapCurrentIndex(input: {
  readonly items: readonly TimelineMinimapItem[];
  /** scrollTop of the message element for each item id; absent means not measured. */
  readonly topById: ReadonlyMap<string, number>;
  readonly scrollTop: number;
  readonly viewportHeight: number;
}): number | null {
  const { items, topById, scrollTop, viewportHeight } = input;
  if (items.length === 0) return null;
  const line = scrollTop + viewportHeight / 2;
  let current: number | null = null;
  for (let index = 0; index < items.length; index += 1) {
    const top = topById.get(items[index]!.id);
    if (typeof top !== "number" || !Number.isFinite(top)) continue;
    if (top <= line) current = index;
  }
  return current;
}
