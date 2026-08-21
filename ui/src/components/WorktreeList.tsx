import { ChevronDown, GitBranch, LockKeyhole, Plus, RefreshCcw, Trash2 } from "lucide-react";

import { cn } from "../lib/cn";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";

type WorktreeListProps = {
  worktrees: Worktree[];
  activePath: string;
  agents: ActiveAgent[];
  statuses: Record<string, DirtyState | undefined>;
  onSelect: (worktree: Worktree) => void;
  onCreate: () => void;
  onRefreshStatus: (worktree: Worktree) => void;
  onDelete: (worktree: Worktree) => void;
};

function basename(path: string) {
  if (path === ".") return "main";
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

function branchName(worktree: Worktree) {
  return worktree.branch?.replace(/^refs\/heads\//, "") ?? "detached HEAD";
}

function workspaceName(worktree: Worktree) {
  const parts = branchName(worktree).split("/");
  return parts[0] === "orca" && parts.length > 2 ? parts.slice(2).join("/") : basename(worktree.path);
}

export function WorktreeList({
  worktrees,
  activePath,
  agents,
  statuses,
  onSelect,
  onCreate,
  onRefreshStatus,
  onDelete,
}: WorktreeListProps) {
  const rootName = worktrees[0]?.path && worktrees[0].path !== "."
    ? (worktrees[0].path.split("/").pop() || "Workspace")
    : "Workspace";

  return (
    <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-1 pb-3 scrollbar-sleek">
      <div className="sticky top-0 z-10 flex h-7 items-center gap-1.5 bg-worktree-sidebar pr-2 text-left">
        <span className="flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground">
          <ChevronDown className="size-3.5" />
        </span>
        <span className="min-w-0 flex-1 truncate text-[12px] font-semibold text-foreground">{rootName}</span>
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
          const status = statuses[worktree.path];
          const canDelete = worktreeIdentity(worktree) !== null;
          return (
            <div
              key={worktree.path}
              className={cn(
                "group/worktree-card relative w-full animate-enter rounded-md border transition-colors",
                active
                  ? "border-worktree-sidebar-ring/35 bg-worktree-sidebar-accent/95 ring-1 ring-worktree-sidebar-ring/25"
                  : "border-transparent hover:border-worktree-sidebar-border hover:bg-worktree-sidebar-accent/55",
              )}
              style={{ animationDelay: `${index * 35}ms` }}
            >
              <button type="button" onClick={() => onSelect(worktree)} className="w-full px-2 py-2 pr-14 text-left">
                <span className="flex min-w-0 items-start gap-2">
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
                    <span className={cn("mt-1 block text-[10px]", status?.isDirty ? "text-status-warning" : "text-muted-foreground")}>
                      {status ? (status.isDirty ? `Dirty · ${status.files.length} ${status.files.length === 1 ? "file" : "files"}` : "Clean") : "Status not checked"}
                    </span>
                    {agent ? (
                      <span className="mt-1.5 flex items-center gap-1.5 truncate text-[11px] text-muted-foreground">
                        <span className="truncate">{agent.name}</span>
                        <span aria-hidden="true">·</span>
                        <span className="truncate">{agent.task}</span>
                      </span>
                    ) : null}
                  </span>
                </span>
              </button>

              <div className="absolute right-1 top-1 flex items-center gap-0.5 opacity-70 transition-opacity group-hover/worktree-card:opacity-100">
                {worktree.locked ? <LockKeyhole className="mr-0.5 size-3 text-status-warning" /> : null}
                <IconButton label="Refresh worktree status" size="sm" onClick={() => onRefreshStatus(worktree)}>
                  <RefreshCcw className="size-3" />
                </IconButton>
                <IconButton label="Delete worktree" size="sm" disabled={!canDelete} onClick={() => onDelete(worktree)}>
                  <Trash2 className="size-3" />
                </IconButton>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
