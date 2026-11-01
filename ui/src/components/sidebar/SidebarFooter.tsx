import { Activity, Settings2 } from "lucide-react";

import { cn } from "../../lib/cn";
import { IconButton } from "../ui/IconButton";
import type { SidebarDensity } from "./SidebarWorktreeRow";

export type SidebarFooterProps = {
  readonly summary: string;
  readonly onOpenResources?: () => void;
  readonly onOpenSettings?: () => void;
  readonly settingsShortcut?: string;
  readonly density?: SidebarDensity;
};

/** Footer: the running / needs-input summary, then the resource and settings buttons when offered. */
export function SidebarFooter({ summary, onOpenResources, onOpenSettings, settingsShortcut, density = "compact" }: SidebarFooterProps) {
  return (
    <div className="flex shrink-0 items-center justify-end gap-1 border-t border-worktree-sidebar-border px-2 py-1.5">
      <span data-testid="sidebar-status-footer" className={cn("mr-auto min-w-0 truncate pl-0.5 text-muted-foreground", density === "touch" ? "text-xs" : "text-[10px]")}>
        {summary}
      </span>
      {onOpenResources ? (
        <IconButton label="System resources" size="sm" onClick={onOpenResources}>
          <Activity className="size-3.5" />
        </IconButton>
      ) : null}
      {onOpenSettings ? (
        <IconButton data-shortcut={settingsShortcut} label="Settings" size="sm" onClick={onOpenSettings}>
          <Settings2 className="size-3.5" />
        </IconButton>
      ) : null}
    </div>
  );
}
