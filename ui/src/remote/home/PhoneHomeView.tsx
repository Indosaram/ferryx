// The phone home container: the destination body, an optional resume bar for the session
// already open, and the bottom tab bar. RemoteApp supplies the Worktrees body because it owns
// the selection callbacks.
import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

import { InboxHome } from "./InboxHome";
import { MobileHomeTabBar } from "./MobileHomeTabBar";
import type { InboxRow } from "./phoneHomeModel";
import { SupervisorHome, type SupervisorHomeProps } from "./SupervisorHome";
import type { PhoneDestination } from "./usePhoneNavigation";

export type PhoneHomeViewProps = {
  readonly destination: PhoneDestination;
  readonly onSelectTab: (destination: PhoneDestination) => void;
  readonly supervisor: SupervisorHomeProps;
  readonly inbox: { readonly rows: readonly InboxRow[]; readonly loading: boolean; readonly onOpen: (row: InboxRow) => void };
  readonly worktrees: ReactNode;
  /** The session already open on this phone; tapping it returns to the Session screen. */
  readonly resume: { readonly title: string; readonly subtitle: string; readonly onOpen: () => void } | null;
};

export function PhoneHomeView({ destination, onSelectTab, supervisor, inbox, worktrees, resume }: PhoneHomeViewProps) {
  const waiting = inbox.rows.filter((row) => row.kind !== "done").length;
  return (
    <div data-testid="mobile-home" data-destination={destination} className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen pt-safe-header">
      <main className="flex min-h-0 min-w-0 flex-1 flex-col">
        {destination === "supervisor" ? (
          <SupervisorHome {...supervisor} />
        ) : destination === "inbox" ? (
          <InboxHome rows={inbox.rows} now={supervisor.now} loading={inbox.loading} onOpen={inbox.onOpen} />
        ) : (
          worktrees
        )}
      </main>
      {resume ? (
        <button
          type="button"
          data-testid="mobile-home-resume"
          aria-label={`Back to ${resume.title}`}
          onClick={resume.onOpen}
          className="mx-3 mb-2 flex min-h-11 shrink-0 items-center gap-3 rounded-xl bg-chat-surface-raised px-3 py-2 text-left transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-[12px] text-chat-foreground-secondary">Open session</span>
            <span className="min-w-0 truncate text-[14px] font-medium text-chat-foreground">{resume.title}</span>
          </span>
          <span className="min-w-0 max-w-[40%] truncate text-[12px] text-chat-foreground-secondary">{resume.subtitle}</span>
          <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
        </button>
      ) : null}
      <MobileHomeTabBar active={destination} inboxCount={waiting} onSelect={onSelectTab} />
    </div>
  );
}
