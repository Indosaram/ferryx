import { GitBranch, LockKeyhole, Trash2 } from "lucide-react";
import { memo, useMemo } from "react";

import { resolveActivityIndicator, type ActivitySummary } from "../lib/activity";
import { branchName, workspaceName } from "../lib/branchFilter";
import { cn } from "../lib/cn";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { IconButton } from "./ui/IconButton";
import { StatusDot, type StatusDotState } from "./ui/StatusDot";

type WorktreeListProps = {
  readonly worktrees: readonly Worktree[];
  readonly activePath: string;
  readonly agents: readonly ActiveAgent[];
  readonly statuses: Record<string, DirtyState | undefined>;
  readonly unreadWorktreePaths?: Record<string, boolean>;
  readonly activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  readonly onSelect: (worktree: Worktree) => void;
  readonly onCreate: () => void;
  readonly onDelete: (worktree: Worktree) => void;
  readonly label?: string;
};

type WorktreeRowProps = {
  readonly worktree: Worktree;
  readonly active: boolean;
  readonly agent: ActiveAgent | undefined;
  readonly status: DirtyState | undefined;
  readonly unread: boolean;
  readonly activitySummary: ActivitySummary | undefined;
  readonly onSelect: (worktree: Worktree) => void;
  readonly onDelete: (worktree: Worktree) => void;
};

/** The repository root worktree is the one that is not an `orca/<ws>/<slug>` worktree branch. */
function isPrimaryWorktree(worktree: Worktree) {
  return worktreeIdentity(worktree) === null;
}

export const WorktreeRow = memo(function WorktreeRow({
  worktree,
  active,
  agent,
  status,
  unread,
  activitySummary,
  onSelect,
  onDelete,
}: WorktreeRowProps) {
  const primary = isPrimaryWorktree(worktree);
  const canDelete = !primary;
  const displaySummary = activitySummary
    ? activitySummary.hasUnread === unread
      ? activitySummary
      : { ...activitySummary, hasUnread: unread }
    : undefined;
  const aggregateIndicator = resolveActivityIndicator(displaySummary);
  const indicator: StatusDotState | null =
    aggregateIndicator ?? (activitySummary === undefined && agent ? agent.state : null);

  return (
    <li
      className={cn(
        "group/worktree-row relative border-l transition-colors",
        active
          ? "border-worktree-sidebar-ring/60 bg-worktree-sidebar-accent"
          : "border-worktree-sidebar-border hover:bg-worktree-sidebar-accent/45",
      )}
    >
      <button
        type="button"
        onClick={() => onSelect(worktree)}
        aria-current={active ? "true" : undefined}
        className="w-full rounded-sm py-1.5 pl-2 pr-9 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <span className="flex min-w-0 items-start gap-1.5">
          <span className="mt-0.5 flex size-4 shrink-0 items-center justify-center text-muted-foreground">
            <GitBranch className="size-3" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="flex items-center gap-1.5">
              <span
                data-testid="worktree-status-dot"
                data-activity-state={indicator ?? "idle"}
                className="inline-flex size-3 shrink-0 items-center justify-center"
              >
                {indicator ? <StatusDot state={indicator} /> : <span className="size-1.5 shrink-0 rounded-full bg-status-idle" />}
              </span>
              <span className="min-w-0 flex-1 truncate text-[12px] font-medium leading-tight text-foreground">
                {workspaceName(worktree)}
              </span>
              {primary ? (
                <span className="shrink-0 rounded-full bg-worktree-sidebar-foreground/10 px-1.5 py-px text-[10px] font-medium leading-none text-muted-foreground">
                  primary
                </span>
              ) : null}
            </span>
            <span className="mt-0.5 block truncate font-mono text-[9px] leading-snug text-muted-foreground/75">
              {branchName(worktree)}
            </span>
            <span className={cn("mt-0.5 block text-[9px]", status?.isDirty ? "text-status-warning" : "text-muted-foreground/75")}>
              {status ? (status.isDirty ? `Dirty · ${status.files.length} ${status.files.length === 1 ? "file" : "files"}` : "Clean") : "Status not checked"}
            </span>
            {agent ? (
              <span className="mt-0.5 flex items-center gap-1.5 truncate text-[9px] text-muted-foreground/75">
                <span className="truncate">{agent.name}</span>
                <span aria-hidden="true">·</span>
                <span className="truncate">{agent.task}</span>
              </span>
            ) : null}
          </span>
        </span>
      </button>

      <div className="absolute right-1 top-1 flex items-center gap-0.5 opacity-55 transition-opacity focus-within:opacity-100 group-hover/worktree-row:opacity-100">
        {worktree.locked ? <LockKeyhole className="mr-0.5 size-3 text-status-warning" /> : null}
        <IconButton label="Delete worktree" size="sm" disabled={!canDelete} onClick={() => onDelete(worktree)}>
          <Trash2 className="size-3" />
        </IconButton>
      </div>
    </li>
  );
});

export function WorktreeList({
  worktrees,
  activePath,
  agents,
  statuses,
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelect,
  onCreate,
  onDelete,
  label = "Worktrees",
}: WorktreeListProps) {
  const agentsByPath = useMemo(() => {
    const map = new Map<string, ActiveAgent>();
    for (const agent of agents) {
      map.set(agent.worktreePath, agent);
    }
    return map;
  }, [agents]);

  if (worktrees.length === 0) {
    return (
      <div className="border-l border-worktree-sidebar-border py-2 pl-3 pr-2 text-[11px] leading-relaxed text-muted-foreground">
        <p>No Git worktrees found for this repository.</p>
        <button
          type="button"
          onClick={onCreate}
          className="mt-1 rounded-sm text-[11px] font-medium text-worktree-sidebar-foreground/70 underline-offset-2 transition-colors hover:text-worktree-sidebar-foreground hover:underline focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Create the first worktree
        </button>
      </div>
    );
  }

  return (
    <ul aria-label={label} className="m-0 list-none p-0">
      {worktrees.map((worktree) => {
        const active = worktree.path === activePath;
        const agent = agentsByPath.get(worktree.path);
        const status = statuses[worktree.path];
        const summary = activityByWorktreePath?.[worktree.path];
        const hasUnread = !active && Boolean(summary?.hasUnread || unreadWorktreePaths?.[worktree.path]);

        return (
          <WorktreeRow
            key={worktree.path}
            worktree={worktree}
            active={active}
            agent={agent}
            status={status}
            unread={hasUnread}
            activitySummary={summary}
            onSelect={onSelect}
            onDelete={onDelete}
          />
        );
      })}
    </ul>
  );
}
