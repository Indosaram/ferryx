// The Worktrees destination: the same project -> worktree -> session inventory the drawer
// shows, laid out inline under the shared phone home header.
import { useContext, useRef, type ReactNode } from "react";

import { cn } from "../../lib/cn";
import { AccountButton } from "./AccountButton";
import { PhoneHomeHeader } from "./PhoneHomeHeader";
import { PullToRefreshIndicator } from "./PullToRefreshIndicator";
import { PullToRefreshContext, pullContentProps, usePullToRefresh, type RefreshHandler } from "./usePullToRefresh";

export type WorktreesHomeProps = {
  /** The inline RemoteWorkspaceMirror (it owns rows, New terminal and New worktree). */
  readonly children: ReactNode;
  /** Trailing header content; wins over `onOpenAccount`. */
  readonly headerAction?: ReactNode;
  /** Opens the Account sheet from this header when no `headerAction` is given. */
  readonly onOpenAccount?: () => void;
  /** Pull-to-refresh on the list; defaults to the refresh the phone home provides. */
  readonly onRefresh?: RefreshHandler;
};

export function WorktreesHome({ children, headerAction, onOpenAccount, onRefresh }: WorktreesHomeProps) {
  const trailing = headerAction ?? (onOpenAccount ? <AccountButton onOpen={onOpenAccount} /> : undefined);
  const contextRefresh = useContext(PullToRefreshContext);
  const refresh = onRefresh ?? contextRefresh;
  // The list scrolls inside the mirror; the hook only starts a pull when every scroller between
  // the touched row and this wrapper is at its top.
  const scrollRef = useRef<HTMLDivElement>(null);
  const pull = usePullToRefresh(scrollRef, refresh);
  const pullContent = pullContentProps(pull);
  return (
    <div data-testid="worktrees-home" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen">
      <PhoneHomeHeader title="Worktrees" trailing={trailing} />
      <div ref={scrollRef} className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
        {refresh ? <PullToRefreshIndicator {...pull} /> : null}
        <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col", pullContent.className)} style={pullContent.style}>
          {children}
        </div>
      </div>
    </div>
  );
}
