import { useSortable } from "@dnd-kit/sortable";
import { LockKeyhole, Trash2 } from "lucide-react";
import { memo, useMemo } from "react";

import { resolveActivityIndicator, type ActivitySummary } from "../lib/activity";
import { workspaceName } from "../lib/branchFilter";
import { cn } from "../lib/cn";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
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
  readonly onDelete: (worktree: Worktree) => void;
  readonly sortableWorkspaceId?: string;
  readonly label?: string;
};

export type WorktreeRowProps = {
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
  const displayName = workspaceName(worktree);
  const displaySummary = activitySummary
    ? activitySummary.hasUnread === unread
      ? activitySummary
      : { ...activitySummary, hasUnread: unread }
    : undefined;
  const aggregateIndicator = resolveActivityIndicator(displaySummary);
  const indicator: StatusDotState | null =
    aggregateIndicator ?? (activitySummary === undefined && agent ? agent.state : null);

  return (
    <div
      className={cn(
        "group/worktree-row relative my-0.5 w-full rounded-md border transition-colors",
        active
          ? "border-[#6c6c6c] bg-[#3f3f3f]"
          : "border-transparent bg-transparent hover:bg-white/[0.04]",
      )}
    >
      <button
        type="button"
        onClick={() => onSelect(worktree)}
        aria-current={active ? "true" : undefined}
        className="flex min-h-[28px] w-full flex-col justify-center rounded-md px-2 py-1 pr-8 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
      >
        <span className="flex min-w-0 flex-col">
          <span className="flex min-w-0 items-center gap-1.5">
            <span
              data-testid="worktree-status-dot"
              data-activity-state={indicator ?? "idle"}
              className="inline-flex size-2 shrink-0 items-center justify-center"
            >
              {indicator ? <StatusDot state={indicator} /> : <span className="size-2 shrink-0 rounded-full bg-status-idle" />}
            </span>
            <span
              className={cn(
                "truncate text-[12px] font-semibold leading-tight",
                active ? "text-[#fafafa]" : "text-worktree-sidebar-foreground",
              )}
            >
              {displayName}
            </span>
            {primary ? (
              <span className="shrink-0 rounded bg-[#4a4a4a] px-1.5 py-px text-[10px] font-medium leading-none text-[#d8d8d8]">
                primary
              </span>
            ) : null}
            {status?.isDirty ? (
              <span className="shrink-0 text-[10px] text-status-warning">
                Dirty · {status.files.length} {status.files.length === 1 ? "file" : "files"}
              </span>
            ) : null}
          </span>
        </span>
      </button>

      <div className="absolute right-1.5 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-55 transition-opacity focus-within:opacity-100 group-hover/worktree-row:opacity-100">
        {worktree.locked ? <LockKeyhole className="mr-0.5 size-3 text-status-warning" /> : null}
        <IconButton label="Delete worktree" size="sm" disabled={!canDelete} onClick={() => onDelete(worktree)}>
          <Trash2 className="size-3" />
        </IconButton>
      </div>
    </div>
  );
});

type SortableWorktreeRowProps = WorktreeRowProps & {
  readonly workspaceId: string;
};

const SortableWorktreeRow = memo(function SortableWorktreeRow({
  workspaceId,
  ...rowProps
}: SortableWorktreeRowProps) {
  const worktreePath = rowProps.worktree.path;
  const sortable = useSortable({
    id: worktreeSortableId(workspaceId, worktreePath),
    data: { type: "sidebar-worktree", workspaceId, worktreePath },
  });

  return (
    <SidebarDragRow
      kind="worktree"
      setNodeRef={sortable.setNodeRef}
      setActivatorNodeRef={sortable.setActivatorNodeRef}
      attributes={sortable.attributes}
      listeners={sortable.listeners}
      transform={sortable.transform}
      transition={sortable.transition}
      dragging={sortable.isDragging}
    >
      <WorktreeRow {...rowProps} />
    </SidebarDragRow>
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
  onDelete,
  sortableWorkspaceId,
  label = "Worktrees",
}: WorktreeListProps) {
  const agentsByPath = useMemo(() => {
    const map = new Map<string, ActiveAgent>();
    for (const agent of agents) {
      map.set(agent.worktreePath, agent);
    }
    return map;
  }, [agents]);

  if (worktrees.length === 0) return null;

  return (
    <div role="list" aria-label={label} className="m-0 p-0">
      {worktrees.map((worktree) => {
        const active = worktree.path === activePath;
        const agent = agentsByPath.get(worktree.path);
        const status = statuses[worktree.path];
        const summary = activityByWorktreePath?.[worktree.path];
        const hasUnread = !active && Boolean(summary?.hasUnread || unreadWorktreePaths?.[worktree.path]);

        const rowProps: WorktreeRowProps = {
          worktree,
          active,
          agent,
          status,
          unread: hasUnread,
          activitySummary: summary,
          onSelect,
          onDelete,
        };

        return sortableWorkspaceId ? (
          <SortableWorktreeRow key={worktree.path} workspaceId={sortableWorkspaceId} {...rowProps} />
        ) : (
          <div key={worktree.path} role="listitem">
            <WorktreeRow {...rowProps} />
          </div>
        );
      })}
    </div>
  );
}

export function worktreeSortableId(workspaceId: string, worktreePath: string) {
  return `sidebar-worktree:${workspaceId}:${worktreePath}`;
}
