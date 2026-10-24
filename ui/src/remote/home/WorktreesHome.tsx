// The Worktrees destination: the same project -> worktree -> session inventory the drawer
// shows, laid out inline, with the Account entry above it.
import { UserRound } from "lucide-react";
import type { ReactNode } from "react";

export type WorktreesHomeProps = {
  /** The inline RemoteWorkspaceMirror (it owns rows, New terminal, New worktree, Sign out). */
  readonly children: ReactNode;
  readonly onOpenAccount: () => void;
};

export function WorktreesHome({ children, onOpenAccount }: WorktreesHomeProps) {
  return (
    <div data-testid="worktrees-home" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen">
      <div className="flex h-14 shrink-0 items-center gap-2 px-4">
        <h1 className="min-w-0 flex-1 truncate text-[17px] font-semibold text-chat-foreground">Worktrees</h1>
        <button
          type="button"
          data-testid="account-open"
          onClick={onOpenAccount}
          className="flex h-11 shrink-0 items-center gap-1.5 rounded-full bg-chat-surface-raised px-3.5 text-[14px] font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <UserRound aria-hidden="true" className="size-4 text-chat-foreground-secondary" />
          Account
        </button>
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  );
}
