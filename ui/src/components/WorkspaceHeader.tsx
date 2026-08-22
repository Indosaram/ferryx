import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, GitBranch, PanelLeft, Radio } from "lucide-react";

import { branchName, displayWorkspaceTitle } from "../lib/branchFilter";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import type { ActiveAgent, Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";

type WorkspaceHeaderProps = {
  worktree: Worktree;
  agent?: ActiveAgent;
  sidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  isMac?: boolean;
};

export function WorkspaceHeader({
  worktree,
  agent,
  sidebarOpen = true,
  onToggleSidebar,
  isMac = isMacShortcutPlatform(),
}: WorkspaceHeaderProps) {
  const startWindowDrag = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest(".no-drag")) return;
    void getCurrentWindow().startDragging();
  };

  return (
    <header
      data-tauri-drag-region
      onPointerDown={startWindowDrag}
      className="drag-region flex h-titlebar shrink-0 items-center border-b border-border bg-card pl-3 pr-2"
    >
      <div className="flex min-w-0 flex-1 items-center gap-2">
        {!sidebarOpen ? (
          <>
            {isMac ? <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" /> : null}
            {onToggleSidebar ? (
              <IconButton label="Show sidebar" className="no-drag mr-1" size="sm" onClick={onToggleSidebar}>
                <PanelLeft className="size-3.5" />
              </IconButton>
            ) : null}
          </>
        ) : null}
        <div className="flex size-5 shrink-0 items-center justify-center text-muted-foreground/75">
          <GitBranch className="size-3.5" />
        </div>
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-[12px] font-medium text-foreground">{displayWorkspaceTitle(worktree)}</span>
          <span className="truncate font-mono text-[10px] text-muted-foreground/65">{branchName(worktree)}</span>
          {agent ? <StatusDot state={agent.state} /> : null}
        </div>
      </div>

      {agent ? (
        <div
          data-testid="workspace-agent-chip"
          className="no-drag hidden h-6 items-center gap-1.5 border-l border-border pl-2 text-[10px] text-muted-foreground md:flex"
        >
          <Bot className="size-3" />
          <span className="max-w-agent truncate">{agent.name}</span>
          <Radio className="size-2.5 text-status-working" />
        </div>
      ) : null}
    </header>
  );
}
