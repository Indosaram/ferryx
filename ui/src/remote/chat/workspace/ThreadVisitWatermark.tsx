import React from "react";

/**
 * The "new messages" divider, ported from t3code `chat/ThreadVisitWatermark.tsx` (MIT): the
 * reference tracker renders nothing itself and the timeline draws one divider at the first
 * message the reader has not visited. The visit model lives in `timelineWatermark.ts`; this
 * is that divider.
 */
export const ThreadVisitWatermark: React.FC = () => (
  <div
    data-testid="thread-visit-watermark"
    role="separator"
    aria-label="New messages"
    className="flex min-w-0 items-center gap-2 py-2 text-xs text-chat-foreground-secondary"
  >
    <span aria-hidden="true" className="h-px flex-1 bg-chat-border" />
    <span className="shrink-0">New messages</span>
    <span aria-hidden="true" className="h-px flex-1 bg-chat-border" />
  </div>
);
