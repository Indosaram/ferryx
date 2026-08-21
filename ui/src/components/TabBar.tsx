import { Globe, Plus, TerminalSquare, X } from "lucide-react";
import type { ReactNode } from "react";

import { cn } from "../lib/cn";
import type { WorkspaceTab } from "../lib/types";
import { IconButton } from "./ui/IconButton";

type TabBarProps = {
  tabs: WorkspaceTab[];
  activeTabId: string;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onAdd: () => void;
  onAddBrowser?: () => void;
  actions?: ReactNode;
  showAdd?: boolean;
  leadingSpacer?: number;
  unreadTabIds?: Record<string, boolean>;
};

export function TabBar({
  tabs,
  activeTabId,
  onActivate,
  onClose,
  onAdd,
  actions,
  showAdd = true,
  leadingSpacer = 0,
  unreadTabIds,
}: TabBarProps) {
  return (
    <div data-testid="tab-strip" className="flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1">
      {leadingSpacer > 0 ? (
        <div
          data-testid="tab-strip-leading-spacer"
          style={{ width: `${leadingSpacer}px` }}
          className="shrink-0 border-r border-border"
        />
      ) : null}
      <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto scrollbar-none">
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
          const isUnread = Boolean(unreadTabIds?.[tab.id] && !active);
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => onActivate(tab.id)}
              className={cn(
                "group relative flex min-w-tab max-w-tab items-center gap-1.5 border-r border-border px-2 text-[12px] transition-colors",
                active ? "bg-terminal text-foreground" : "bg-card text-muted-foreground hover:bg-accent/45 hover:text-foreground",
              )}
            >
              {tab.kind === "browser" ? (
                <Globe className="size-3 shrink-0 text-blue-400" />
              ) : (
                <TerminalSquare className="size-3 shrink-0" />
              )}
              <span className="min-w-0 flex-1 truncate text-left">{tab.label}</span>
              {isUnread ? <span className="size-1.5 shrink-0 rounded-full bg-blue-500" data-testid="tab-unread-dot" /> : null}
              <span
                role="button"
                tabIndex={0}
                aria-label={`Close ${tab.label}`}
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(tab.id);
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    event.stopPropagation();
                    onClose(tab.id);
                  }
                }}
                className="no-drag rounded p-0.5 text-muted-foreground opacity-0 hover:bg-muted hover:text-foreground group-hover:opacity-100 focus:opacity-100"
              >
                <X className="size-3" />
              </span>
              {active ? (
                <span data-testid="tab-active-indicator" className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-foreground" />
              ) : null}
            </button>
          );
        })}
      </div>
      {showAdd ? (
        <IconButton label="New terminal" size="sm" className="no-drag my-auto ml-1" onClick={onAdd}>
          <Plus className="size-3.5" />
        </IconButton>
      ) : null}
      {actions ? <div className="no-drag ml-0.5 flex shrink-0 items-center gap-0.5">{actions}</div> : null}
    </div>
  );
}