// The phone home's bottom tab bar: Supervisor | Inbox | Worktrees. Labels are text; the Inbox
// count is a warning-token badge that also lands in the accessible name.
import { Eye, FolderTree, Inbox, type LucideIcon } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";

import { cn } from "../../lib/cn";
import type { PhoneDestination } from "./usePhoneNavigation";

const TABS: ReadonlyArray<{ id: PhoneDestination; label: string; icon: LucideIcon }> = [
  { id: "supervisor", label: "Supervisor", icon: Eye },
  { id: "inbox", label: "Inbox", icon: Inbox },
  { id: "worktrees", label: "Worktrees", icon: FolderTree },
];

export type MobileHomeTabBarProps = {
  readonly active: PhoneDestination;
  readonly inboxCount: number;
  readonly onSelect: (destination: PhoneDestination) => void;
};

export function MobileHomeTabBar({ active, inboxCount, onSelect }: MobileHomeTabBarProps) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = TABS.findIndex((tab) => tab.id === active);
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const next = TABS[(index + step + TABS.length) % TABS.length];
    onSelect(next.id);
    refs.current[TABS.indexOf(next)]?.focus();
  };
  return (
    <nav aria-label="Home" className="shrink-0 border-t border-chat-border bg-chat-header pb-safe">
      <div data-testid="mobile-home-tabs" role="tablist" aria-label="Home" onKeyDown={onKeyDown} className="flex h-14">
        {TABS.map((tab, index) => {
          const selected = tab.id === active;
          const Icon = tab.icon;
          const count = tab.id === "inbox" ? inboxCount : 0;
          return (
            <button
              key={tab.id}
              ref={(el) => {
                refs.current[index] = el;
              }}
              type="button"
              role="tab"
              data-testid={`mobile-home-tab-${tab.id}`}
              aria-selected={selected}
              aria-label={count > 0 ? `${tab.label}, ${count} need you` : tab.label}
              tabIndex={selected ? 0 : -1}
              onClick={() => onSelect(tab.id)}
              className={cn(
                "relative flex min-w-0 flex-1 flex-col items-center justify-center gap-0.5 text-[11px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
                selected ? "text-chat-foreground" : "text-chat-foreground-secondary hover:text-chat-foreground",
              )}
            >
              <span className="relative flex">
                <span className={cn("flex h-7 w-12 items-center justify-center rounded-full", selected && "bg-chat-surface-raised")}>
                  <Icon aria-hidden="true" className="size-[18px]" />
                </span>
                {count > 0 ? (
                  <span
                    data-testid="mobile-home-inbox-count"
                    aria-hidden="true"
                    className="absolute -right-1 -top-1 flex h-4 min-w-4 items-center justify-center rounded-full bg-status-warning px-1 text-[10px] font-semibold leading-none tabular-nums text-chat-screen"
                  >
                    {count > 99 ? "99+" : count}
                  </span>
                ) : null}
              </span>
              <span className="truncate">{tab.label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
