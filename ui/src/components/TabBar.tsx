import { Plus, TerminalSquare, X } from "lucide-react";

import { cn } from "../lib/cn";
import type { TerminalTab } from "../lib/types";
import { IconButton } from "./ui/IconButton";

type TabBarProps = {
  tabs: TerminalTab[];
  activeTabId: string;
  onActivate: (id: string) => void;
  onClose: (id: string) => void;
  onAdd: () => void;
};

export function TabBar({ tabs, activeTabId, onActivate, onClose, onAdd }: TabBarProps) {
  return (
    <div className="flex h-tabbar shrink-0 items-stretch border-b border-border bg-card pr-1.5">
      <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto scrollbar-none">
        {tabs.map((tab) => {
          const active = tab.id === activeTabId;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => onActivate(tab.id)}
              className={cn(
                "group relative flex min-w-tab max-w-tab items-center gap-1.5 border-r border-border px-2 text-xs transition-colors",
                active ? "bg-background text-foreground" : "bg-card text-muted-foreground hover:bg-accent/50 hover:text-foreground",
              )}
            >
              <TerminalSquare className="size-3.5 shrink-0" />
              <span className="min-w-0 flex-1 truncate text-left">{tab.label}</span>
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
                    event.stopPropagation();
                    onClose(tab.id);
                  }
                }}
                className="rounded p-0.5 text-muted-foreground opacity-0 hover:bg-muted hover:text-foreground group-hover:opacity-100"
              >
                <X className="size-3" />
              </span>
              {active ? <span className="absolute inset-x-0 bottom-0 h-0.5 bg-foreground" /> : null}
            </button>
          );
        })}
      </div>
      <IconButton label="New terminal" size="sm" className="my-auto ml-1" onClick={onAdd}>
        <Plus className="size-3.5" />
      </IconButton>
    </div>
  );
}
