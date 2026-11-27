/**
 * Page scrolling for the timeline, ported from t3code `chat/pageScrollController.ts` (MIT):
 * a bare PageUp/PageDown (no modifiers, not composing — the IME gate matches
 * `ui/src/remote/chat/imeKeys.ts`) pages the message stream when the composer is not the
 * scroller. Holding the key accelerates, like the reference controller.
 */
export type PageScrollKey = "PageUp" | "PageDown";

const PAGE_SCROLL_ANIMATION_MS = 150;
const PAGE_SCROLL_ACCELERATION_MS = 400;
const PAGE_SCROLL_MAX_MULTIPLIER = 2;
const PAGE_SCROLL_ALIGNMENT_OFFSET_PX = 36;
const PAGE_SCROLL_BOUNDARY_EPSILON_PX = 1;

export interface PageScrollKeyEvent {
  readonly key: string;
  readonly defaultPrevented: boolean;
  readonly altKey: boolean;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly shiftKey: boolean;
  /** An IME composition buffer owns the key (the gate matches `imeKeys.ts`). */
  readonly isComposing?: boolean;
  /** `keyCode === 229` is a composition-buffer key; the reference controller refuses it. */
  readonly keyCode?: number;
}

interface PageScrollMetrics {
  readonly clientHeight: number;
  readonly scrollHeight: number;
  readonly scrollTop: number;
}

function canScrollInDirection(metrics: PageScrollMetrics, key: PageScrollKey): boolean {
  const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  const clampedScrollTop = Math.min(maxScrollTop, Math.max(0, metrics.scrollTop));
  return key === "PageUp"
    ? clampedScrollTop > PAGE_SCROLL_BOUNDARY_EPSILON_PX
    : clampedScrollTop < maxScrollTop - PAGE_SCROLL_BOUNDARY_EPSILON_PX;
}

/**
 * Which page key this event is, when it must page the timeline: a plain PageUp/PageDown that
 * the timeline can actually honour in the pressed direction.
 */
export function timelinePageScrollKey(
  event: PageScrollKeyEvent,
  metrics: PageScrollMetrics,
): PageScrollKey | null {
  if (event.key !== "PageUp" && event.key !== "PageDown") return null;
  if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return null;
  }
  if (event.isComposing || event.keyCode === 229) return null;
  return canScrollInDirection(metrics, event.key) ? event.key : null;
}

/** Distance one page covers: the viewport minus the alignment inset the reference keeps. */
export function pageScrollDistancePx(containerHeightPx: number): number {
  return Math.max(0, containerHeightPx - PAGE_SCROLL_ALIGNMENT_OFFSET_PX);
}

function multiplier(holdElapsedMs: number): number {
  const progress = Math.max(0, holdElapsedMs) / PAGE_SCROLL_ACCELERATION_MS;
  return 1 + Math.min(progress, 1) * (PAGE_SCROLL_MAX_MULTIPLIER - 1);
}

/** The scrollTop an animated page move reaches, clamped into the document range. */
export function pageScrollTarget(metrics: PageScrollMetrics, key: PageScrollKey, holdElapsedMs: number): number {
  const maxScrollTop = Math.max(0, metrics.scrollHeight - metrics.clientHeight);
  const distance = pageScrollDistancePx(metrics.clientHeight) * multiplier(holdElapsedMs);
  const target = key === "PageUp" ? metrics.scrollTop - distance : metrics.scrollTop + distance;
  return Math.min(Math.max(0, target), maxScrollTop);
}

/** Animation frame driver the component injects, so tests run without fake timers. */
export interface PageScrollEnv {
  readonly now: () => number;
  readonly requestFrame: (callback: () => void) => unknown;
  readonly cancelFrame: (handle: unknown) => void;
}

/** Animate one page move; a reduced-motion reader jumps instead of gliding. */
export function animatePageScroll(
  container: {
    readonly scrollTop: number;
    readonly scrollHeight: number;
    readonly clientHeight: number;
    readonly scrollTo: (options: { top: number }) => void;
  },
  key: PageScrollKey,
  reducedMotion: boolean,
  env: PageScrollEnv,
): () => void {
  const startTop = container.scrollTop;
  const metrics: PageScrollMetrics = {
    scrollTop: startTop,
    scrollHeight: container.scrollHeight,
    clientHeight: container.clientHeight,
  };
  const target = pageScrollTarget(metrics, key, 0);
  if (reducedMotion || target === startTop) {
    container.scrollTo({ top: target });
    return () => undefined;
  }
  const startedAt = env.now();
  let handle: unknown = null;
  const step = (): void => {
    const progress = Math.min(1, (env.now() - startedAt) / PAGE_SCROLL_ANIMATION_MS);
    const eased = 1 - (1 - progress) * (1 - progress);
    container.scrollTo({ top: startTop + (target - startTop) * eased });
    if (progress < 1) handle = env.requestFrame(step);
  };
  handle = env.requestFrame(step);
  return () => {
    if (handle !== null) env.cancelFrame(handle);
  };
}
