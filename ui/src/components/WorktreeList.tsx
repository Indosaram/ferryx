import { ChevronDown, GitBranch, LockKeyhole, MoreHorizontal, Plus } from "lucide-react";

import { cn } from "../lib/cn";
import type { ActiveAgent, Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";

type WorktreeListProps = {
  worktrees: Worktree[];
  activePath: string;
  agents: ActiveAgent[];
  onSelect: (worktree: Worktree) => void;
  onCreate: () => void;
};

function basename(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function branchName(worktree: Worktree) {
  return worktree.branch?.replace(/^refs\/heads\//, "") ?? "detached HEAD";
}

function workspaceName(worktree: Worktree) {
  const parts = branchName(worktree).split("/");
  return parts[0] === "orca" && parts.length > 2 ? parts.slice(2).join("/") : basename(worktree.path);
}

export function WorktreeList({ worktrees, activePath, agents, onSelect, onCreate }: WorktreeListProps) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-1 pb-3 scrollbar-sleek">
      <div className="sticky top-0 z-10 flex h-7 items-center gap-1.5 bg-worktree-sidebar pr-2 text-left">
        <span className="flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground">
          <ChevronDown className="size-3.5" />
        </span>
        <span className="min-w-0 flex-1 truncate text-[12px] font-semibold text-foreground">omo-bridge</span>
        <IconButton label="New worktree" size="sm" onClick={onCreate}>
          <Plus className="size-3.5" />
        </IconButton>
      </div>

      <div className="space-y-1 pl-3 pr-1">
        {worktrees.length === 0 ? (
          <div className="rounded-md border border-dashed border-worktree-sidebar-border px-3 py-4 text-center text-[11px] leading-relaxed text-muted-foreground">
            No Git worktrees found for this repository.
          </div>
        ) : null}

        {worktrees.map((worktree, index) => {
          const active = worktree.path === activePath;
          const agent = agents.find((candidate) => candidate.worktreePath === worktree.path);
          return (
            <button
              key={worktree.path}
              type="button"
              onClick={() => onSelect(worktree)}
              className={cn(
                "group/worktree-card relative w-full animate-enter rounded-md border px-2 py-2 text-left transition-colors",
                active
                  ? "border-worktree-sidebar-ring/35 bg-worktree-sidebar-accent/95 ring-1 ring-worktree-sidebar-ring/25"
                  : "border-transparent hover:border-worktree-sidebar-border hover:bg-worktree-sidebar-accent/55",
              )}
              style={{ animationDelay: `${index * 35}ms` }}
            >
              <div className="flex min-w-0 items-start gap-2">
                <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded bg-worktree-sidebar-foreground/5 text-muted-foreground">
                  <GitBranch className="size-3" />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-1.5">
                    {agent ? <StatusDot state={agent.state} /> : <span className="size-2 rounded-full bg-status-idle" />}
                    <span className="min-w-0 flex-1 truncate text-[13px] font-medium leading-tight text-foreground">
                      {workspaceName(worktree)}
                    </span>
                  </span>
                  <span className="mt-1 block truncate font-mono text-[10px] leading-snug text-muted-foreground">
                    {branchName(worktree)}
                  </span>
                  {agent ? (
                    <span className="mt-1.5 flex items-center gap-1.5 truncate text-[11px] text-muted-foreground">
                      <span className="truncate">{agent.name}</span>
                      <span aria-hidden="true">·</span>
                      <span className="truncate">{agent.task}</span>
                    </span>
                  ) : null}
                </span>
                <span className="flex shrink-0 items-center gap-0.5">
                  {worktree.locked ? <LockKeyhole className="size-3 text-status-warning" /> : null}
                  <MoreHorizontal className="size-3.5 text-muted-foreground opacity-0 transition-opacity group-hover/worktree-card:opacity-100" />
                </span>
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
