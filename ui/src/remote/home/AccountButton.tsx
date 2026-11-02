// The Account entry each phone home destination's header carries.
import { UserRound } from "lucide-react";

export function AccountButton({ onOpen }: { readonly onOpen: () => void }) {
  return (
    <button
      type="button"
      data-testid="account-open"
      onClick={onOpen}
      className="flex h-11 shrink-0 items-center gap-1.5 rounded-lg px-3 text-sm font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <UserRound aria-hidden="true" className="size-4 text-chat-foreground-secondary" />
      Account
    </button>
  );
}
