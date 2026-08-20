import { Bot, FolderPlus, LayoutDashboard, PanelLeftClose, Search, Settings2, SlidersHorizontal } from "lucide-react";

import type { ActiveAgent, Worktree } from "../lib/types";
import { AgentCards } from "./AgentCards";
import { IconButton } from "./ui/IconButton";
import { SectionHeader } from "./ui/SectionHeader";
import { WorktreeList } from "./WorktreeList";

type SidebarProps = {
  worktrees: Worktree[];
  agents: ActiveAgent[];
  activePath: string;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: () => void;
};

export function Sidebar({ worktrees, agents, activePath, onSelectWorktree, onCreateWorktree }: SidebarProps) {
  return (
    <aside className="flex h-full w-sidebar shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground">
      <div className="drag-region flex h-titlebar shrink-0 items-center justify-end px-2 pt-1">
        <IconButton label="Hide sidebar" className="no-drag" size="sm">
          <PanelLeftClose className="size-3.5" />
        </IconButton>
      </div>

      <div className="space-y-1 px-2 pb-1">
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-md bg-worktree-sidebar-accent px-2 py-1.5 text-left text-[13px] font-medium tracking-tight text-worktree-sidebar-accent-foreground"
        >
          <LayoutDashboard className="size-4" />
          <span className="flex-1">Workspace</span>
        </button>
        <button
          type="button"
          className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] font-medium tracking-tight text-worktree-sidebar-foreground/60 transition-colors hover:bg-worktree-sidebar-foreground/8"
        >
          <Bot className="size-4 text-worktree-sidebar-foreground/35" />
          <span className="flex-1">Agents</span>
          <span className="rounded-full bg-primary px-1.5 py-px text-[10px] font-semibold text-primary-foreground">
            {agents.length}
          </span>
        </button>
        <button
          type="button"
          className="group relative flex h-7 w-full items-center rounded-md border border-worktree-sidebar-border/70 bg-worktree-sidebar-foreground/5 pl-7 pr-1.5 text-left text-[12px] font-medium tracking-tight text-worktree-sidebar-foreground/45 transition-colors hover:border-worktree-sidebar-border hover:bg-worktree-sidebar-foreground/8"
        >
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3 -translate-y-1/2 text-worktree-sidebar-foreground/30" />
          Search workspaces
          <kbd className="ml-auto rounded border border-worktree-sidebar-border/80 bg-worktree-sidebar-foreground/8 px-1 py-px text-[9px] text-worktree-sidebar-foreground/55">
            ⌘K
          </kbd>
        </button>
      </div>

      <SectionHeader
        title="Active agents"
        count={agents.length}
        actions={
          <IconButton label="Agent options" size="sm">
            <SlidersHorizontal className="size-3.5" />
          </IconButton>
        }
      />
      <AgentCards agents={agents} activeWorktreePath={activePath} onSelect={(path) => {
        const worktree = worktrees.find((candidate) => candidate.path === path);
        if (worktree) onSelectWorktree(worktree);
      }} />

      <SectionHeader
        title="Worktrees"
        count={worktrees.length}
        actions={
          <IconButton label="Add worktree" size="sm" onClick={onCreateWorktree}>
            <FolderPlus className="size-3.5" />
          </IconButton>
        }
      />
      <WorktreeList
        worktrees={worktrees}
        agents={agents}
        activePath={activePath}
        onSelect={onSelectWorktree}
        onCreate={onCreateWorktree}
      />

      <div className="flex shrink-0 items-center justify-between border-t border-worktree-sidebar-border px-2 py-1.5">
        <div className="flex items-center gap-2 px-1 text-[11px] text-muted-foreground">
          <span className="size-1.5 rounded-full bg-status-success" />
          Local runtime
        </div>
        <IconButton label="Settings" size="sm">
          <Settings2 className="size-3.5" />
        </IconButton>
      </div>
    </aside>
  );
}
