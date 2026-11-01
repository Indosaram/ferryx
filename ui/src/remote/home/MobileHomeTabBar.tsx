// The phone home's bottom tab bar: Supervisor | Inbox | Worktrees. Labels are text; the Inbox
// count is a warning-token badge that also lands in the accessible name.
import { Bot, FolderTree, Inbox, type LucideIcon } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";

import { cn } from "../../lib/cn";
import type { PhoneDestination } from "./usePhoneNavigation";

const TABS: ReadonlyArray<{ id: PhoneDestination; label: string; icon: LucideIcon }> = [
  { id: "supervisor", label: "Supervisor", icon: Bot },
  { id: "inbox", label: "Inbox", icon: Inbox },
  { id: "worktrees", label: "Worktrees", icon: FolderTree },
];

export const PHONE_HOME_PANEL_ID = "phone-home-panel";

export function phoneHomeTabId(destination: PhoneDestination): string {
  return `phone-home-tab-${destination}`;
}

export type MobileHomeTabBarProps = {
  readonly active: PhoneDestination;
  readonly inboxCount: number;
  readonly onSelect: (destination: PhoneDestination) => void;
};

export function MobileHomeTabBar({ active, inboxCount, onSelect }: MobileHomeTabBarProps) {
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = TABS.findIndex((tab) => tab.id === active);
    const target =
      event.key === "ArrowRight"
        ? (index + 1) % TABS.length
        : event.key === "ArrowLeft"
          ? (index - 1 + TABS.length) % TABS.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? TABS.length - 1
              : null;
    if (target === null) return;
    event.preventDefault();
    onSelect(TABS[target].id);
    refs.current[target]?.focus();
  };
  return (
    <nav aria-label="Home" className="shrink-0 border-t border-chat-border bg-chat-header pb-safe px-safe">
      <div data-testid="mobile-home-tabs" role="tablist" onKeyDown={onKeyDown} className="flex h-14">
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
              id={phoneHomeTabId(tab.id)}
              data-testid={`mobile-home-tab-${tab.id}`}
              aria-selected={selected}
              aria-controls={selected ? PHONE_HOME_PANEL_ID : undefined}
              aria-label={count > 0 ? `${tab.label}, ${count} need you` : tab.label}
              tabIndex={selected ? 0 : -1}
              onClick={() => onSelect(tab.id)}
              className={cn(
                "relative flex min-w-0 flex-1 flex-col items-center justify-center gap-0.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-ring",
                selected ? "text-chat-foreground" : "text-chat-foreground-secondary hover:text-chat-foreground",
              )}
            >
              <span className="relative flex">
                <span
                  className={cn(
                    "flex h-7 w-14 items-center justify-center rounded-full border",
                    selected ? "border-chat-border bg-chat-surface-raised" : "border-transparent",
                  )}
                >
                  <Icon aria-hidden="true" className="size-5" strokeWidth={selected ? 2.25 : 1.75} />
                </span>
                {count > 0 ? (
                  <span
                    data-testid="mobile-home-inbox-count"
                    aria-hidden="true"
                    className="absolute -right-1 -top-1 flex h-5 min-w-5 items-center justify-center rounded-full bg-status-warning px-1 text-xs font-semibold leading-none tabular-nums text-chat-screen"
                  >
                    {count > 99 ? "99+" : count}
                  </span>
                ) : null}
              </span>
              <span className={cn("truncate", selected && "font-semibold")}>{tab.label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
