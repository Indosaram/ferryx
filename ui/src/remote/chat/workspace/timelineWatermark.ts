/**
 * Timeline watermark, ported from t3code's thread-visit model
 * (`chat/ThreadVisitWatermark.tsx`, `client-runtime/state/threadVisits.ts`, MIT): while a
 * thread is being viewed the visit watermark advances with it, so the only unread messages
 * are the ones that arrived while the reader was scrolled away. The divider t3code draws at
 * the first unread message is `workspace/ThreadVisitWatermark.tsx`; the model is here, pure.
 */

/** The visit state one thread lane keeps: the id of the newest message already seen. */
export interface ThreadVisitState {
  /** Id of the newest message counted as visited; null before the first visit. */
  readonly lastSeenId: string | null;
  /** Id of the first message not yet visited, when the reader is away from the bottom. */
  readonly watermarkId: string | null;
}

export const EMPTY_VISIT: ThreadVisitState = { lastSeenId: null, watermarkId: null };

/**
 * Advance the visit on a message-list change. Viewing (at the bottom) marks everything read;
 * away from the bottom, messages appended below the watermark become the new unread range and
 * the watermark moves to the first message the reader has not seen. Existing ids never move.
 */
export function advanceVisit(
  previous: ThreadVisitState,
  messages: readonly { readonly id: string }[],
  atBottom: boolean,
): ThreadVisitState {
  if (messages.length === 0) return atBottom ? EMPTY_VISIT : previous;
  if (atBottom) return { lastSeenId: messages[messages.length - 1]!.id, watermarkId: null };
  const previousIndex = previous.lastSeenId === null
    ? -1
    : messages.findIndex((message) => message.id === previous.lastSeenId);
  if (previousIndex >= 0) {
    const firstUnseen = messages[previousIndex + 1];
    if (!firstUnseen) return { lastSeenId: previous.lastSeenId, watermarkId: null };
    return { lastSeenId: previous.lastSeenId, watermarkId: firstUnseen.id };
  }
  // The seen marker fell out of the window (older pages prepend, or the lane switched).
  // Everything is older context again unless the reader is away and this is a fresh page.
  if (previous.watermarkId !== null) {
    const watermarkIndex = messages.findIndex((message) => message.id === previous.watermarkId);
    if (watermarkIndex >= 0) return previous;
  }
  return { lastSeenId: messages[messages.length - 1]!.id, watermarkId: null };
}
