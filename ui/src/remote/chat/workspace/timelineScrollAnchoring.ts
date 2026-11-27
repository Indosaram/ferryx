/**
 * Timeline scroll anchoring, extracted from the workspace's page-prepend behaviour and
 * modelled on t3code `chat/timelineScrollAnchoring.ts` (MIT): an older page lands ABOVE what
 * the reader is looking at, so their place is kept by shifting scrollTop by the growth;
 * anything landing below keeps the bottom-follow rule.
 */
/** One distance from the bottom decides both following the stream and offering the pill. */
export const FOLLOW_THRESHOLD_PX = 80;

/** The viewport as it was before the message list changed. */
export interface TimelineViewportSnapshot {
  readonly top: number;
  readonly height: number;
  readonly count: number;
  readonly atBottom: boolean;
  readonly firstId: string | null;
}

export const EMPTY_VIEWPORT: TimelineViewportSnapshot = {
  top: 0,
  height: 0,
  count: 0,
  atBottom: true,
  firstId: null,
};

export function isViewportAtBottom(scrollTop: number, scrollHeight: number, clientHeight: number): boolean {
  return scrollHeight - scrollTop - clientHeight < FOLLOW_THRESHOLD_PX;
}

export function snapshotViewport(
  scrollTop: number,
  scrollHeight: number,
  clientHeight: number,
  messages: readonly { readonly id: string }[],
): TimelineViewportSnapshot {
  return {
    top: scrollTop,
    height: scrollHeight,
    count: messages.length,
    atBottom: isViewportAtBottom(scrollTop, scrollHeight, clientHeight),
    firstId: messages[0]?.id ?? null,
  };
}

/**
 * The scrollTop that keeps the reader's place: nonzero only when an older page was prepended
 * above them (first turn changed, the old first turn is still present, they were not at the
 * bottom, and the content grew). An animated anchor would show their place jump, so the
 * caller applies this with `scroll-behavior: auto`.
 */
export function anchoredScrollTop(
  previous: TimelineViewportSnapshot,
  messages: readonly { readonly id: string }[],
  scrollHeight: number,
  scrollTop: number,
): number | null {
  const firstId = messages[0]?.id ?? null;
  const prepended =
    previous.firstId !== null &&
    firstId !== previous.firstId &&
    messages.some((message) => message.id === previous.firstId);
  if (prepended && messages.length > previous.count && !previous.atBottom && scrollHeight > previous.height) {
    return scrollTop + (scrollHeight - previous.height);
  }
  return null;
}
