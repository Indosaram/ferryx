// The phone's one Inbox on the home: the same rows the session sheet shows (every waiting or
// finished session on the host), each needs-you row answerable in place. The tab badge, the
// "Needs input" group and the empty line all count the same needs-you rows.
import { LoaderCircle } from "lucide-react";
import { useRef, type ReactNode } from "react";

import { AttentionInbox } from "../../features/ferryx/attention/AttentionInbox";
import type { AttentionRow } from "../../features/ferryx/attention/attentionModel";
import type { AttentionPrompts } from "../../features/ferryx/attention/attentionPrompts";
import { cn } from "../../lib/cn";
import { PhoneHomeHeader } from "./PhoneHomeHeader";
import { PullToRefreshIndicator } from "./PullToRefreshIndicator";
import { pullContentProps, usePullToRefresh, type RefreshHandler } from "./usePullToRefresh";

export type InboxHomeProps = {
  readonly rows: readonly AttentionRow[];
  readonly prompts?: AttentionPrompts;
  readonly now: number;
  readonly loading: boolean;
  readonly onOpen: (row: AttentionRow) => void;
  /** Leaves the Inbox for the Worktrees tab, the next step when nothing waits. */
  readonly onBrowseWorktrees?: () => void;
  readonly headerAction?: ReactNode;
  /** Pull-to-refresh on the list. Absent: pulling does nothing. */
  readonly onRefresh?: RefreshHandler;
};

export function InboxHome({ rows, prompts, now, loading, onOpen, onBrowseWorktrees, headerAction, onRefresh }: InboxHomeProps) {
  const waiting = rows.filter((row) => row.state === "needs-you").length;
  const scrollRef = useRef<HTMLDivElement>(null);
  const pull = usePullToRefresh(scrollRef, onRefresh);
  const pullContent = pullContentProps(pull);
  return (
    <div data-testid="inbox-home" className="flex min-h-0 flex-1 flex-col bg-chat-screen">
      <PhoneHomeHeader title="Inbox" trailing={headerAction} />
      <div ref={scrollRef} className="relative flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-y-contain scrollbar-sleek">
        {onRefresh ? <PullToRefreshIndicator {...pull} /> : null}
        <div className={cn("flex w-full flex-1 flex-col gap-3 px-4 pb-6", pullContent.className)} style={pullContent.style}>
          {loading ? (
            <p data-testid="inbox-home-loading" role="status" className="flex items-center gap-2 px-1 text-sm text-chat-foreground-secondary">
              <LoaderCircle aria-hidden="true" className="size-4 animate-spin motion-reduce:animate-none" />
              {rows.length === 0 ? "Checking your computers…" : "Still checking some computers…"}
            </p>
          ) : null}
          {!loading && waiting === 0 ? (
            <div data-testid="inbox-home-empty" className="flex flex-col items-start gap-2 rounded-xl bg-chat-surface px-4 py-4">
              <p className="text-base font-medium text-chat-foreground">Nothing needs you</p>
              <p className="text-sm leading-snug text-chat-foreground-secondary">
                Sessions that wait for an answer on any computer show up here.
              </p>
              {onBrowseWorktrees ? (
                <button
                  type="button"
                  data-testid="inbox-home-browse"
                  onClick={onBrowseWorktrees}
                  className="mt-1 flex h-11 items-center rounded-lg bg-chat-surface-raised px-4 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  Open worktrees
                </button>
              ) : null}
            </div>
          ) : null}
          {rows.length > 0 ? <AttentionInbox compact touch rows={rows} prompts={prompts} now={now} onOpen={onOpen} /> : null}
        </div>
      </div>
    </div>
  );
}
