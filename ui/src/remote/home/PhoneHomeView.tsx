// The phone home container: the destination body, an optional resume row for the session
// already open, and the bottom tab bar. RemoteApp supplies the Worktrees body because it owns
// the selection callbacks.
import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

import { InboxHome } from "./InboxHome";
import { MobileHomeTabBar, PHONE_HOME_PANEL_ID, phoneHomeTabId } from "./MobileHomeTabBar";
import type { AttentionRow } from "../../features/ferryx/attention/attentionModel";
import type { AttentionPrompts } from "../../features/ferryx/attention/attentionPrompts";
import { SupervisorHome, type SupervisorHomeProps } from "./SupervisorHome";
import type { PhoneDestination } from "./usePhoneNavigation";
import { PullToRefreshContext } from "./usePullToRefresh";

export type PhoneHomeViewProps = {
  readonly destination: PhoneDestination;
  readonly onSelectTab: (destination: PhoneDestination) => void;
  readonly supervisor: SupervisorHomeProps;
  readonly inbox: {
    readonly rows: readonly AttentionRow[];
    readonly prompts?: AttentionPrompts;
    readonly loading: boolean;
    readonly onOpen: (row: AttentionRow) => void;
  };
  readonly worktrees: ReactNode;
  /** The session already open on this phone; tapping it returns to the Session screen. */
  readonly resume: { readonly title: string; readonly subtitle: string; readonly onOpen: () => void } | null;
  /** The Account entry every destination's header carries. */
  readonly accountAction?: ReactNode;
  /**
   * Pull-to-refresh for the destination on screen; the spinner holds until the promise settles.
   * Absent: the lists do not pull. The Worktrees body reads it through `PullToRefreshContext`.
   */
  readonly onRefresh?: (destination: PhoneDestination) => Promise<unknown> | void;
};

/** The badge, the Inbox's "Needs input" group and its empty line count the same rows. */
export function inboxBadgeCount(rows: readonly AttentionRow[]): number {
  return rows.filter((row) => row.state === "needs-you").length;
}

export function PhoneHomeView({ destination, onSelectTab, supervisor, inbox, worktrees, resume, accountAction, onRefresh }: PhoneHomeViewProps) {
  const refresh = onRefresh ? () => onRefresh(destination) : undefined;
  return (
    <div data-testid="mobile-home" data-destination={destination} className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen pt-[env(safe-area-inset-top)] px-safe">
      <main id={PHONE_HOME_PANEL_ID} role="tabpanel" aria-labelledby={phoneHomeTabId(destination)} className="flex min-h-0 min-w-0 flex-1 flex-col">
        {destination === "supervisor" ? (
          <SupervisorHome {...supervisor} phoneHeader headerAction={accountAction} onRefresh={refresh} />
        ) : destination === "inbox" ? (
          <InboxHome
            rows={inbox.rows}
            prompts={inbox.prompts}
            now={supervisor.now}
            loading={inbox.loading}
            onOpen={inbox.onOpen}
            onBrowseWorktrees={() => onSelectTab("worktrees")}
            headerAction={accountAction}
            onRefresh={refresh}
          />
        ) : (
          <PullToRefreshContext.Provider value={refresh}>{worktrees}</PullToRefreshContext.Provider>
        )}
      </main>
      {resume ? (
        <aside aria-label="Open session" className="shrink-0 border-t border-chat-border bg-chat-header">
          <button
            type="button"
            data-testid="mobile-home-resume"
            onClick={resume.onOpen}
            className="flex min-h-11 w-full items-center gap-2 px-4 py-2 text-left transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring"
          >
            <span className="min-w-0 flex-1 truncate text-sm">
              <span className="text-chat-foreground-secondary">Back to </span>
              <span className="font-medium text-chat-foreground">{resume.title}</span>
            </span>
            <span className="min-w-0 max-w-[40%] truncate text-xs text-chat-foreground-secondary">{resume.subtitle}</span>
            <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
          </button>
        </aside>
      ) : null}
      <MobileHomeTabBar active={destination} inboxCount={inboxBadgeCount(inbox.rows)} onSelect={onSelectTab} />
    </div>
  );
}
