import { Activity, Settings2 } from "lucide-react";

import { IconButton } from "../ui/IconButton";

export type SidebarFooterProps = {
  readonly summary: string;
  readonly onOpenResources?: () => void;
  readonly onOpenSettings?: () => void;
  readonly settingsShortcut?: string;
};

/** Footer: the running / needs-input summary, then the resource and settings buttons when offered. */
export function SidebarFooter({ summary, onOpenResources, onOpenSettings, settingsShortcut }: SidebarFooterProps) {
  return (
    <div className="flex shrink-0 items-center justify-end gap-1 border-t border-worktree-sidebar-border px-2 py-1.5">
      <span data-testid="sidebar-status-footer" className="mr-auto min-w-0 truncate pl-0.5 text-[10px] text-muted-foreground">
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
