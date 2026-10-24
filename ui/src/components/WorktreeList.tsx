import { useSortable } from "@dnd-kit/sortable";
import { LockKeyhole, Plus, Trash2 } from "lucide-react";
import { memo, useEffect, useMemo, useRef, type ReactNode } from "react";
import { toast } from "sonner";

import type { ActivitySummary } from "../lib/activity";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { getHostPlatform } from "../lib/platform";
import { revealPath } from "../lib/tauri";
import { useSupervisor } from "../lib/supervisor";
import { worktreeIdentity, type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
import { worktreeRowLive, type SidebarRowLive } from "./sidebar/sidebarModel";
import { SidebarEmptyStatus, SidebarWorktreeRow, SidebarWorktreesEmpty } from "./sidebar/SidebarWorktreeRow";
import { SIDEBAR_EMPTY_TEXT, worktreeLabel, worktreeRowHost } from "./sidebar/sidebarLabels";
import { worktreeSortableId } from "./sidebar/sidebarRows";
import { isSameWorktreePath, normalizeWorktreePath, supervisorTypingIn } from "./sidebar/sidebarSupervisor";
import { IconButton } from "./ui/IconButton";

export { isSameWorktreePath, normalizeWorktreePath, supervisorTypingIn, worktreeSortableId };

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
  /** The raw unread mark; worktreeRowLive decides whether the row shows it. */
  readonly unread: boolean;
  readonly activitySummary: ActivitySummary | undefined;
  readonly onSelect: (worktree: Worktree) => void;
  readonly onCreateWorktree?: (worktree: Worktree) => void;
  readonly onDelete: (worktree: Worktree) => void;
  readonly onResetAgentState?: (worktree: Worktree) => void;
  /** Precomputed row state (sidebarLiveState); without it the row derives its own. */
  readonly live?: SidebarRowLive;
};

/** What one list row needs besides the worktree: the list's selection and live inputs. */
export type WorktreeListRowContext = Pick<
  WorktreeListProps,
  "repoRoot" | "activePath" | "activeWorkspaceId" | "statuses" | "unreadWorktreePaths" | "activityByWorktreePath" |
  "onSelect" | "onCreateWorktree" | "onDelete" | "onResetAgentState" | "sortableWorkspaceId"
> & { readonly agentsByPath: ReadonlyMap<string, ActiveAgent> };

/** Row props for one worktree of a list; the sidebar container and WorktreeList share it. */
export function worktreeListRowProps(worktree: Worktree, context: WorktreeListRowContext): WorktreeRowProps {
  const effectiveWorkspaceId = worktree.workspaceId ?? context.sortableWorkspaceId;
  const active =
    worktree.path === context.activePath &&
    (!context.activeWorkspaceId || !effectiveWorkspaceId || effectiveWorkspaceId === context.activeWorkspaceId);
  // Paired terminal proxy is unavailable: path-only local terminal data
  // must never light up a machine row with the same absolute path.
  const paired = worktree.workspaceId?.startsWith("daemon:");
  return {
    worktree,
    repoRoot: context.repoRoot,
    active,
    agent: paired ? undefined : context.agentsByPath.get(worktree.path),
    status: paired ? undefined : context.statuses[worktree.path],
    unread: Boolean(context.unreadWorktreePaths?.[worktree.path]),
    activitySummary: context.activityByWorktreePath?.[worktree.path],
    onSelect: context.onSelect,
    onCreateWorktree: context.onCreateWorktree,
    onDelete: context.onDelete,
    onResetAgentState: context.onResetAgentState,
  };
}

export function agentsByWorktreePath(agents: readonly ActiveAgent[]): Map<string, ActiveAgent> {
  return new Map(agents.map((agent) => [agent.worktreePath, agent]));
}

export function fileManagerActionLabel() {
  const platform = getHostPlatform();
  if (platform === "macos") return "Reveal in Finder";
  if (platform === "windows") return "Show in File Explorer";
  return "Open in File Manager";
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
  unread,
  activitySummary,
  onSelect,
  onCreateWorktree,
  onDelete,
  onResetAgentState,
  live,
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
  const displayName = worktreeLabel(worktree);
  const { snapshot: supervisor, now: supervisorNow } = useSupervisor();
  const hostName = worktreeRowHost(worktree);
  const rowLive: SidebarRowLive = live ?? worktreeRowLive(worktree, active, {
    activityByWorktreePath: { [worktree.path]: activitySummary },
    unreadWorktreePaths: { [worktree.path]: unread },
    agentStateByPath: { [worktree.path]: agent?.state },
    isSupervisorTyping: (path) => supervisorTypingIn(supervisor, supervisorNow, path),
  });

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
    <SidebarWorktreeRow
      node={{ path: worktree.path, label: displayName, hostLabel: hostName ?? null, stale: isStale }}
      live={rowLive}
      disabled={isDisabled}
      onContextMenu={handleContextMenu}
      onSelect={() => {
        if (unavailableReason) {
          toast.info(unavailableReason);
          return;
        }
        onSelect(worktree);
      }}
      shortcutWorkspaceId={worktree.workspaceId ?? ""}
      title={unavailableReason ?? (isRemote ? `Remote SSH root: ${worktree.path}${worktree.hostLabel ? ` (${worktree.hostLabel})` : ""}` : undefined)}
      slotTitle={isRemote && rowLive.slot === null ? (worktree.hostSummary ? `${worktree.hostLabel ?? "Remote"} (${worktree.hostSummary})` : hostName) : undefined}
      trailing={
        <>
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
        </>
      }
    />
  );
});

type SortableWorktreeRowProps = WorktreeRowProps & {
  readonly workspaceId: string;
};

export const SortableWorktreeRow = memo(function SortableWorktreeRow({
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
  const agentsByPath = useMemo(() => agentsByWorktreePath(agents), [agents]);

  if (worktrees.length === 0) {
    return emptyState ? <SidebarEmptyStatus>{emptyState}</SidebarEmptyStatus> : null;
  }

  return (
    <div role="list" aria-label={label} className="m-0 p-0">
      {worktrees.map((worktree) => {
        const rowProps = worktreeListRowProps(worktree, {
          repoRoot, activePath, activeWorkspaceId, statuses, unreadWorktreePaths, activityByWorktreePath,
          onSelect, onCreateWorktree, onDelete, onResetAgentState, sortableWorkspaceId, agentsByPath,
        });
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
  return <SidebarWorktreesEmpty text={SIDEBAR_EMPTY_TEXT.noWorktrees} onCreateWorktree={onCreateWorktree} />;
}
