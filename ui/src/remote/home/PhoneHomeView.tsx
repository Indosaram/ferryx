// The phone home container. The Supervisor panel is the home screen: no tab bar, no Inbox, no
// lanes list. "+" on the panel opens the Worktrees list (where a session is started) as a screen
// of its own with a way back; RemoteApp supplies that body because it owns the selection callbacks.
import { ChevronLeft } from "lucide-react";
import type { ReactNode } from "react";

import { PhoneSupervisorHome, type PhoneSupervisorHomeProps } from "../supervisorPanel/PhoneSupervisorHome";
import type { PhoneDestination } from "./usePhoneNavigation";
import { PullToRefreshContext } from "./usePullToRefresh";

export type PhoneHomeViewProps = {
  /** The autonomy panel, always: with no hub it renders the setup card, never a fallback home. */
  readonly panel: PhoneSupervisorHomeProps;
  readonly destination: PhoneDestination;
  readonly onSelectTab: (destination: PhoneDestination) => void;
  readonly worktrees: ReactNode;
  /**
   * Pull-to-refresh for the Worktrees screen; the spinner holds until the promise settles.
   * Absent: the list does not pull. The Worktrees body reads it through `PullToRefreshContext`.
   */
  readonly onRefresh?: (destination: PhoneDestination) => Promise<unknown> | void;
};

export function PhoneHomeView({ panel, destination, onSelectTab, worktrees, onRefresh }: PhoneHomeViewProps) {
  if (destination !== "worktrees") return <PhoneSupervisorHome {...panel} />;
  const refresh = onRefresh ? () => onRefresh(destination) : undefined;
  return (
    <div data-testid="mobile-home" data-destination="worktrees" data-home="panel-start" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen pt-[env(safe-area-inset-top)] px-safe">
      <button
        type="button"
        data-testid="phone-panel-start-back"
        onClick={() => onSelectTab("supervisor")}
        className="flex min-h-11 shrink-0 items-center gap-1 px-3 text-sm text-chat-foreground transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <ChevronLeft aria-hidden="true" className="size-4" />
        Back to the supervisor
      </button>
      <PullToRefreshContext.Provider value={refresh}>{worktrees}</PullToRefreshContext.Provider>
    </div>
  );
}
