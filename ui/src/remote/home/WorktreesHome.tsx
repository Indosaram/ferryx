// The Worktrees destination: the same project -> worktree -> session inventory the drawer
// shows, laid out inline under the shared phone home header.
import type { ReactNode } from "react";

import { AccountButton } from "./AccountButton";
import { PhoneHomeHeader } from "./PhoneHomeHeader";

export type WorktreesHomeProps = {
  /** The inline RemoteWorkspaceMirror (it owns rows, New terminal and New worktree). */
  readonly children: ReactNode;
  /** Trailing header content; wins over `onOpenAccount`. */
  readonly headerAction?: ReactNode;
  /** Opens the Account sheet from this header when no `headerAction` is given. */
  readonly onOpenAccount?: () => void;
};

export function WorktreesHome({ children, headerAction, onOpenAccount }: WorktreesHomeProps) {
  const trailing = headerAction ?? (onOpenAccount ? <AccountButton onOpen={onOpenAccount} /> : undefined);
  return (
    <div data-testid="worktrees-home" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen">
      <PhoneHomeHeader title="Worktrees" trailing={trailing} />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">{children}</div>
    </div>
  );
}
