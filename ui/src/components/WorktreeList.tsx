import { useSortable } from "@dnd-kit/sortable";
import { LockKeyhole, Plus, Trash2 } from "lucide-react";
import { memo, useEffect, useMemo, useRef, type ReactNode } from "react";
import { toast } from "sonner";

import { resolveActivityIndicator, type ActivitySummary } from "../lib/activity";
import { workspaceName } from "../lib/branchFilter";
import { cn } from "../lib/cn";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { getHostPlatform } from "../lib/platform";
import { revealPath } from "../lib/tauri";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
import { IconButton } from "./ui/IconButton";
import { StatusDot, type StatusDotState } from "./ui/StatusDot";

type WorktreeListProps = {
  readonly worktrees: readonly Worktree[];
  readonly repoRoot?: string | null;
  readonly activePath: string;
  readonly activeWorkspaceId?: string;
  readonly agents: readonly ActiveAgent[];
  readonly statuses: Record<string, DirtyState | undefined>;
  readonly unreadWorktreePaths?: Record<string, boolean>;
  readonly activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  readonly onSelect: (worktree: Worktree) => void;
  readonly onCreateWorktree?: (worktree: Worktree) => void;
  readonly onDelete: (worktree: Worktree) => void;
  readonly onResetAgentState?: (worktree: Worktree) => void;
  readonly sortableWorkspaceId?: string;
  readonly label?: string;
  /** Rendered when the list is empty. The sidebar passes a hint; other callers keep an empty list silent. */
  readonly emptyState?: ReactNode;
};

export type WorktreeRowProps = {
  readonly worktree: Worktree;
  readonly repoRoot?: string | null;
  readonly active: boolean;
  readonly agent: ActiveAgent | undefined;
  readonly status: DirtyState | undefined;
  readonly unread: boolean;
  readonly activitySummary: ActivitySummary | undefined;
  readonly onSelect: (worktree: Worktree) => void;
  readonly onCreateWorktree?: (worktree: Worktree) => void;
  readonly onDelete: (worktree: Worktree) => void;
  readonly onResetAgentState?: (worktree: Worktree) => void;
};

export function fileManagerActionLabel() {
  const platform = getHostPlatform();
  if (platform === "macos") return "Reveal in Finder";
  if (platform === "windows") return "Show in File Explorer";
  return "Open in File Manager";
}

function isCaseInsensitivePlatform(): boolean {
  const platform = getHostPlatform();
  return platform === "macos" || platform === "windows";
}

export function normalizeWorktreePath(p: string, caseInsensitive = isCaseInsensitivePlatform()): string {
  let normalized = p.trim().replace(/\\/g, "/");
  normalized = normalized.replace(/\/+/g, "/");
  if (normalized.length > 1 && normalized.endsWith("/")) {
    normalized = normalized.slice(0, -1);
  }
  if (caseInsensitive) {
    normalized = normalized.toLowerCase();
  }
  return normalized;
}

export function isSameWorktreePath(pathA: string, pathB: string, caseInsensitive?: boolean): boolean {
  return normalizeWorktreePath(pathA, caseInsensitive) === normalizeWorktreePath(pathB, caseInsensitive);
}

/**
 * The repository root worktree is identified by comparing its path against the project repo root.
 * If no repo root is available, falls back to checking if the branch is not an `orca/<ws>/<slug>` worktree branch.
 */
export function isPrimaryWorktree(worktree: Worktree, repoRoot?: string | null): boolean {
  if (repoRoot && repoRoot.trim().length > 0) {
    return isSameWorktreePath(worktree.path, repoRoot);
  }
  return worktreeIdentity(worktree) === null;
}

export const WorktreeRow = memo(function WorktreeRow({
  worktree,
  repoRoot,
  active,
  agent,
  status,
  unread,
  activitySummary,
  onSelect,
  onCreateWorktree,
  onDelete,
  onResetAgentState,
}: WorktreeRowProps) {
  const menuUnlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
    };
  }, []);

  const isPaired = Boolean(worktree.workspaceId?.startsWith("daemon:"));
  const isRemote = Boolean(worktree.workspaceId?.startsWith("ssh:")) || isPaired;
  const isPrimary = isPrimaryWorktree(worktree, repoRoot);
  const primary = !isRemote && isPrimary;
  const isRemotePrimary = isRemote && isPrimary;
  const canDelete = !primary && !isPaired;
  const deleteActionLabel = isRemotePrimary ? "Remove Project" : "Delete Worktree";
  const deleteButtonLabel = isRemotePrimary ? "Remove project" : "Delete worktree";
  const managedSlug = worktreeIdentity(worktree)?.slug;
  const displayName = managedSlug ?? workspaceName(worktree);
  const displaySummary = activitySummary
    ? activitySummary.hasUnread === unread
      ? activitySummary
      : { ...activitySummary, hasUnread: unread }
    : undefined;
  const aggregateIndicator = resolveActivityIndicator(displaySummary);
  const indicator: StatusDotState | null =
    aggregateIndicator ?? (activitySummary === undefined && agent ? agent.state : null);

  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    const items: NativeMenuEntry[] = [
      { kind: "item", id: "reveal", label: isRemote ? "Local reveal unavailable over SSH" : fileManagerActionLabel(), enabled: !isRemote && !isDisabled, icon: "reveal" },
      { kind: "item", id: "copy-path", label: isRemote ? "Copy Remote Path" : "Copy Worktree Path" },
    ];
    if (worktree.branch) {
      items.push({ kind: "item", id: "copy-branch", label: "Copy Branch Name" });
    }
    if (onResetAgentState) {
      items.push({ kind: "item", id: "reset-agent-state", label: "Reset Agent State", enabled: !isDisabled, icon: "refresh" });
    }
    items.push({ kind: "separator" });
    items.push({ kind: "item", id: "delete", label: deleteActionLabel, enabled: canDelete && !isDisabled, icon: "trash" });
    menuUnlistenRef.current?.();
    const controller = new AbortController();
    menuUnlistenRef.current = () => controller.abort();
    void openNativePopupMenu(
      "cmd_native_sidebar_context_menu",
      items,
      { x: event.clientX, y: event.clientY },
      (id) => {
        menuUnlistenRef.current?.();
        menuUnlistenRef.current = null;
        if (id === "reveal") handleReveal();
        else if (id === "copy-path") copyPath();
        else if (id === "copy-branch") {
          const branchName = (worktree.branch ?? "").replace(/^refs\/heads\//, "");
          if (branchName) {
            void navigator.clipboard.writeText(branchName).then(() => {
              toast.success("Copied branch name to clipboard");
            });
          }
        } else if (id === "reset-agent-state") {
          onResetAgentState?.(worktree);
        } else if (id === "delete") onDelete(worktree);
      },
      controller.signal,
    ).catch((error: unknown) => console.warn("Could not open native worktree menu", error));
  };

  const copyPath = () => {
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      navigator.clipboard
        .writeText(worktree.path)
        .then(() => toast.success("Path copied to clipboard"))
        .catch(() => toast.error("Failed to copy to clipboard"));
    } else {
      toast.error("Clipboard API unavailable");
    }
  };

  const handleReveal = () => {
    revealPath(worktree.path).catch((err: unknown) => {
      toast.error(`Failed to reveal path: ${err instanceof Error ? err.message : String(err)}`);
    });
  };

  const isStale = Boolean(worktree.stale || worktree.freshness?.stale);
  const isDisabled = Boolean(worktree.disabled || isStale);
  // A stale row shows the last list a paired machine sent before it dropped off. It must not open
  // (that machine cannot serve it now), but it must say why instead of swallowing the click.
  const unavailableReason = isDisabled
    ? isStale
      ? `${worktree.hostSummary ?? "Stale"}: this is the last list the machine sent. The row turns back on when the machine reconnects.`
      : "This worktree is unavailable right now."
    : null;

  return (
    <>
      <div
        data-stale={isStale ? "true" : undefined}
        data-disabled={isDisabled ? "true" : undefined}
        onContextMenu={handleContextMenu}
        className={cn(
          "group/worktree-row relative my-0.5 w-full rounded-md border transition-colors",
          isDisabled && "opacity-60",
          active
            ? "border-[#6c6c6c] bg-[#3f3f3f]"
            : "border-transparent bg-transparent hover:bg-white/[0.04]",
        )}
      >
        <button
          type="button"
          onClick={() => {
            if (unavailableReason) {
              toast.info(unavailableReason);
              return;
            }
            onSelect(worktree);
          }}
          data-shortcut-worktree-path={worktree.path}
          data-shortcut-workspace-id={worktree.workspaceId ?? ""}
          aria-current={active ? "true" : undefined}
          aria-disabled={isDisabled ? "true" : undefined}
          title={unavailableReason ?? (isRemote ? `Remote SSH root: ${worktree.path}${worktree.hostLabel ? ` (${worktree.hostLabel})` : ""}` : undefined)}
          className="flex min-h-[28px] w-full flex-col justify-center rounded-md px-2 py-1 pr-8 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring aria-disabled:cursor-not-allowed"
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
              {isRemote ? (
                <span
                  data-testid="remote-machine-badge"
                  title={worktree.hostSummary ? `${worktree.hostLabel ?? "Remote"} (${worktree.hostSummary})` : (worktree.hostLabel ?? "Remote")}
                  className="ml-auto max-w-[88px] shrink-0 truncate rounded bg-[#4a4a4a] px-1.5 py-px text-[10px] font-medium leading-none text-[#d8d8d8]"
                >
                  {worktree.hostLabel ?? worktree.hostSummary ?? "Remote"}
                </span>
              ) : null}
            </span>
            {isRemote ? (
              <span className="truncate text-[10px] text-muted-foreground pl-3.5">
                {worktree.path}{worktree.hostSummary ? ` - ${worktree.hostSummary}` : ""}
              </span>
            ) : null}
          </span>
        </button>

        <div className="absolute right-1.5 top-1/2 flex -translate-y-1/2 items-center gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover/worktree-row:opacity-100">
          {worktree.locked ? <LockKeyhole className="mr-0.5 size-3 text-status-warning" /> : null}
          {onCreateWorktree ? (
            <IconButton
              label="Add worktree"
              size="sm"
              disabled={isDisabled}
              className="size-5"
              onClick={isDisabled ? undefined : () => onCreateWorktree(worktree)}
              onPointerDown={(event) => event.stopPropagation()}
            >
              <Plus className="size-3" />
            </IconButton>
          ) : null}
          {canDelete ? (
            <IconButton
              label={deleteButtonLabel}
              size="sm"
              disabled={isDisabled}
              className="size-5 hover:text-destructive"
              onClick={isDisabled ? undefined : () => onDelete(worktree)}
              onPointerDown={(event) => event.stopPropagation()}
            >
              <Trash2 className="size-3" />
            </IconButton>
          ) : null}
        </div>

      </div>

    </>
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
  const rowWorkspaceId = rowProps.worktree.workspaceId;
  const sortable = useSortable({
    id: worktreeSortableId(workspaceId, worktreePath, rowWorkspaceId),
    data: { type: "sidebar-worktree", workspaceId, worktreePath, rowWorkspaceId },
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
  repoRoot,
  activePath,
  activeWorkspaceId,
  agents,
  statuses,
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelect,
  onCreateWorktree,
  onDelete,
  onResetAgentState,
  sortableWorkspaceId,
  label = "Worktrees",
  emptyState,
}: WorktreeListProps) {
  const agentsByPath = useMemo(() => {
    const map = new Map<string, ActiveAgent>();
    for (const agent of agents) {
      map.set(agent.worktreePath, agent);
    }
    return map;
  }, [agents]);

  if (worktrees.length === 0) {
    return emptyState ? (
      <div role="status" className="px-2 py-1.5 text-[11px] leading-relaxed text-muted-foreground">
        {emptyState}
      </div>
    ) : null;
  }

  return (
    <div role="list" aria-label={label} className="m-0 p-0">
      {worktrees.map((worktree) => {
        const effectiveWorkspaceId = worktree.workspaceId ?? sortableWorkspaceId;
        const active =
          worktree.path === activePath &&
          (!activeWorkspaceId || !effectiveWorkspaceId || effectiveWorkspaceId === activeWorkspaceId);
        // Paired terminal proxy is unavailable: path-only local terminal data
        // must never light up a machine row with the same absolute path.
        const paired = worktree.workspaceId?.startsWith("daemon:");
        const agent = paired ? undefined : agentsByPath.get(worktree.path);
        const status = paired ? undefined : statuses[worktree.path];
        const summary = paired ? undefined : activityByWorktreePath?.[worktree.path];
        const hasUnread = !paired && !active && Boolean(summary?.hasUnread || unreadWorktreePaths?.[worktree.path]);

        const rowProps: WorktreeRowProps = {
          worktree,
          repoRoot,
          active,
          agent,
          status,
          unread: hasUnread,
          activitySummary: summary,
          onSelect,
          onCreateWorktree,
          onDelete,
          onResetAgentState,
        };

        const rowKey = worktree.workspaceId ? `${worktree.workspaceId}:${worktree.path}` : worktree.path;

        return sortableWorkspaceId ? (
          <SortableWorktreeRow key={rowKey} workspaceId={sortableWorkspaceId} {...rowProps} />
        ) : (
          <div key={rowKey} role="listitem">
            <WorktreeRow {...rowProps} />
          </div>
        );
      })}
    </div>
  );
}

/** Default empty-state hint for a project section: says what is going on and offers the one action. */
export function EmptyWorktreesHint({ onCreateWorktree }: { readonly onCreateWorktree?: () => void }) {
  return (
    <span className="flex items-center justify-between gap-2">
      <span>No worktrees listed yet.</span>
      {onCreateWorktree ? (
        <button
          type="button"
          onClick={onCreateWorktree}
          className="shrink-0 rounded px-1.5 py-0.5 text-[11px] text-foreground hover:bg-white/[0.06] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          Add worktree
        </button>
      ) : null}
    </span>
  );
}

export function worktreeSortableId(workspaceId: string, worktreePath: string, rowWorkspaceId?: string) {
  return rowWorkspaceId
    ? `sidebar-worktree:${workspaceId}:${rowWorkspaceId}:${worktreePath}`
    : `sidebar-worktree:${workspaceId}:${worktreePath}`;
}
