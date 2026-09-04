import {
  type DragCancelEvent,
  type DragEndEvent,
  DragOverlay,
  type DragStartEvent,
  DndContext,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  ChevronRight,
  FolderGit2,
  FolderPlus,
  PanelLeftClose,
  Plus,
  Settings2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { combineActivitySummaries, type ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import { resolveWorktreeOwnerId } from "../lib/worktreeOwnership";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import {
  getMigratedItem,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "../lib/storageKeys";
import type { RegisteredProject } from "../lib/tauri";
import { type ActiveAgent, type DirtyState, type Worktree } from "../lib/types";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
import { projectSortableId, SortableProjectSection } from "./sidebar-dnd/SortableProjectSection";
import { IconButton } from "./ui/IconButton";
import { StatusDot } from "./ui/StatusDot";
import { WorktreeList, WorktreeRow, worktreeSortableId } from "./WorktreeList";

export {
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
};
const DEFAULT_SIDEBAR_WIDTH = 236;
const MIN_SIDEBAR_WIDTH = 220;
const MAX_SIDEBAR_WIDTH = 420;

type WorktreeOrder = Record<string, string[]>;

type SidebarProjectDragData = {
  type: "sidebar-project";
  workspaceId: string;
};

type SidebarWorktreeDragData = {
  type: "sidebar-worktree";
  workspaceId: string;
  worktreePath: string;
};

type SidebarDragData = SidebarProjectDragData | SidebarWorktreeDragData;

type SidebarProps = {
  open?: boolean;
  isMac?: boolean;
  projects?: RegisteredProject[];
  activeProjectId?: string;
  worktrees: Worktree[];
  inactiveProjectWorktrees?: Record<string, Worktree[]>;
  agents: ActiveAgent[];
  activePath: string;
  statuses?: Record<string, DirtyState | undefined>;
  unreadWorktreePaths?: Record<string, boolean>;
  activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  onSelectProject?: (project: RegisteredProject) => void;
  onReorderProjects?: (orderedWorkspaceIds: string[]) => void;
  onAddProject?: () => void;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: (project?: RegisteredProject) => void;
  onDeleteWorktree?: (worktree: Worktree) => void;
  onOpenCommandPalette?: () => void;
  onOpenSettings?: () => void;
  onToggle?: () => void;
  onHide?: () => void;
};

export function Sidebar({
  open = true,
  isMac = isMacShortcutPlatform(),
  projects = [],
  activeProjectId,
  worktrees,
  inactiveProjectWorktrees,
  agents,
  activePath,
  statuses = {},
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelectProject = () => undefined,
  onReorderProjects = () => undefined,
  onAddProject = () => undefined,
  onSelectWorktree,
  onCreateWorktree,
  onDeleteWorktree = () => undefined,
  onOpenSettings,
  onToggle,
  onHide,
}: SidebarProps) {
  const worktreeRegionRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [width, setWidth] = useState(loadSidebarWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const [worktreeOrder, setWorktreeOrder] = useState<WorktreeOrder>(loadWorktreeOrder);
  const worktreeOrderRef = useRef(worktreeOrder);
  worktreeOrderRef.current = worktreeOrder;
  const [activeDrag, setActiveDrag] = useState<SidebarDragData | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  // Accordion state is stored as the set of *collapsed* projects, so each project keeps its own
  // open/closed state and toggling one never disturbs another. Projects the user has never
  // toggled default to collapsed unless they are the active project.
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() => {
    const stored = loadCollapsedProjects();
    // Once the user has toggled anything the persisted set is authoritative; only a first run
    // (nothing persisted) falls back to "everything but the active project starts collapsed".
    return stored ?? seedCollapsedProjects(new Set<string>(), projects, activeProjectId);
  });
  const knownProjectsRef = useRef<Set<string> | null>(null);
  if (knownProjectsRef.current === null) {
    knownProjectsRef.current = new Set(projects.map((project) => project.workspaceId));
  }

  useEffect(() => {
    if (knownProjectsRef.current === null) {
      knownProjectsRef.current = new Set(projects.map((project) => project.workspaceId));
    }
    // Projects registered after mount inherit the same default as the initial seed.
    const known = knownProjectsRef.current;
    const unseen = projects.filter((project) => !known.has(project.workspaceId));
    if (unseen.length === 0) return;
    for (const project of unseen) known.add(project.workspaceId);
    setCollapsedProjects((current) => seedCollapsedProjects(current, unseen, activeProjectId));
  }, [activeProjectId, projects]);

  const toggleProject = useCallback((workspaceId: string) => {
    setCollapsedProjects((current) => {
      const next = new Set(current);
      if (next.has(workspaceId)) next.delete(workspaceId);
      else next.add(workspaceId);
      persistCollapsedProjects(next);
      return next;
    });
  }, []);

  const naturalWorktreesByProject = useMemo(
    () => groupWorktreesByProject(worktrees, projects, activeProjectId, inactiveProjectWorktrees),
    [activeProjectId, inactiveProjectWorktrees, projects, worktrees],
  );
  const worktreesByProject = useMemo(
    () => applyWorktreeOrder(naturalWorktreesByProject, worktreeOrder),
    [naturalWorktreesByProject, worktreeOrder],
  );
  const projectSortableItems = useMemo(
    () => projects.map((project) => projectSortableId(project.workspaceId)),
    [projects],
  );

  // The active row is highlighted in whichever project's group actually renders
  // it, so a nested project sharing the path prefix does not light up too.
  // Only the active project can own the active row. Matching by path alone lit
  // up an inactive project whose list still contained that path.
  const activeWorktreeOwnerId = useMemo(() => {
    if (!activePath || !activeProjectId) return undefined;
    const activeRows = worktreesByProject.get(activeProjectId);
    return activeRows?.some((row) => row.path === activePath) ? activeProjectId : undefined;
  }, [activePath, activeProjectId, worktreesByProject]);

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const nextWidth = clampSidebarWidth(drag.startWidth + event.clientX - drag.startX);
      widthRef.current = nextWidth;
      setWidth(nextWidth);
    };

    const finishDrag = () => {
      if (!dragRef.current) return;
      dragRef.current = null;
      persistSidebarWidth(widthRef.current);
      document.body.style.cursor = "";
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      document.body.style.cursor = "";
    };
  }, []);

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    document.body.style.cursor = "col-resize";
  };

  const clearActiveDrag = () => setActiveDrag(null);

  const handleDragStart = (event: DragStartEvent) => {
    const data = readSidebarDragData(event.active.data.current);
    if (data) setActiveDrag(data);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const active = readSidebarDragData(event.active.data.current);
    const over = readSidebarDragData(event.over?.data.current);
    clearActiveDrag();
    if (active?.type === "sidebar-worktree") {
      const prunedOrder = pruneWorktreeOrder(worktreeOrderRef.current, naturalWorktreesByProject);
      if (!worktreeOrdersEqual(prunedOrder, worktreeOrderRef.current)) {
        worktreeOrderRef.current = prunedOrder;
        setWorktreeOrder(prunedOrder);
      }
      persistWorktreeOrder(prunedOrder);
    }
    if (!active || !over || active.type !== over.type) return;

    if (active.type === "sidebar-project" && over.type === "sidebar-project") {
      const orderedWorkspaceIds = projects.map((project) => project.workspaceId);
      const fromIndex = orderedWorkspaceIds.indexOf(active.workspaceId);
      const toIndex = orderedWorkspaceIds.indexOf(over.workspaceId);
      if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return;
      onReorderProjects(arrayMove(orderedWorkspaceIds, fromIndex, toIndex));
      return;
    }

    if (
      active.type !== "sidebar-worktree" ||
      over.type !== "sidebar-worktree" ||
      active.workspaceId !== over.workspaceId
    ) {
      return;
    }
    const rows = worktreesByProject.get(active.workspaceId) ?? [];
    const paths = rows.map((worktree) => worktree.path);
    const fromIndex = paths.indexOf(active.worktreePath);
    const toIndex = paths.indexOf(over.worktreePath);
    if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return;

    const nextOrder = pruneWorktreeOrder(
      {
        ...worktreeOrderRef.current,
        [active.workspaceId]: arrayMove(paths, fromIndex, toIndex),
      },
      naturalWorktreesByProject,
    );
    worktreeOrderRef.current = nextOrder;
    setWorktreeOrder(nextOrder);
    persistWorktreeOrder(nextOrder);
  };

  const handleDragCancel = (_event: DragCancelEvent) => clearActiveDrag();

  const activeProjectOverlay =
    activeDrag?.type === "sidebar-project"
      ? projects.find((project) => project.workspaceId === activeDrag.workspaceId)
      : undefined;
  const activeWorktreeOverlay =
    activeDrag?.type === "sidebar-worktree"
      ? worktreesByProject
          .get(activeDrag.workspaceId)
          ?.find((worktree) => worktree.path === activeDrag.worktreePath)
      : undefined;

  if (!open) {
    return null;
  }

  return (
    <DndContext
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <aside
        className="relative flex h-full shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground"
        style={{ width: `${width}px`, minWidth: `${MIN_SIDEBAR_WIDTH}px`, maxWidth: `${MAX_SIDEBAR_WIDTH}px` }}
      >
        <div
          data-testid="sidebar-drag-region"
          data-tauri-drag-region
          className="drag-region flex h-titlebar shrink-0 items-center px-2"
        >
          {isMac ? <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" /> : null}
          <IconButton
            label="Hide sidebar"
            className="no-drag"
            size="sm"
            onClick={onToggle ?? onHide}
          >
            <PanelLeftClose className="size-3.5" />
          </IconButton>
          <IconButton label="Add project" className="no-drag" size="sm" onClick={onAddProject}>
            <Plus className="size-3.5" />
          </IconButton>
        </div>

        <div ref={worktreeRegionRef} tabIndex={-1} data-testid="worktree-region" className="flex min-h-0 flex-1 flex-col outline-none">
          <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2 pt-2 pb-2 scrollbar-sleek">
            {projects.length === 0 ? (
              <p className="px-2 py-3 text-[11px] leading-relaxed text-muted-foreground">
                No projects registered yet.
              </p>
            ) : null}

            <SortableContext items={projectSortableItems} strategy={verticalListSortingStrategy}>
              {projects.map((project) => {
                const active = project.workspaceId === activeProjectId;
                const expanded = !collapsedProjects.has(project.workspaceId);
                const projectWorktrees = worktreesByProject.get(project.workspaceId) ?? [];
                const projectActivity = summarizeProjectActivity(
                  projectWorktrees,
                  activityByWorktreePath,
                  unreadWorktreePaths,
                );
                const attentionState = projectAttentionState(projectActivity);
                const header = (
                  <ProjectHeader
                    project={project}
                    active={active}
                    expanded={expanded}
                    activity={projectActivity}
                    attentionState={attentionState}
                    onToggle={() => toggleProject(project.workspaceId)}
                    onSelect={() => {
                      onSelectProject(project);
                      toggleProject(project.workspaceId);
                    }}
                    onCreateWorktree={() => onCreateWorktree(project)}
                  />
                );

                return (
                  <SortableProjectSection key={project.workspaceId} workspaceId={project.workspaceId} header={header}>
                    {expanded ? (
                      <div
                        className="pl-3 pr-0.5 pt-0.5"
                        onPointerDown={(event) => event.stopPropagation()}
                      >
                        <SortableContext
                          items={projectWorktrees.map((row) => worktreeSortableId(project.workspaceId, row.path))}
                          strategy={verticalListSortingStrategy}
                        >
                          <WorktreeList
                            worktrees={projectWorktrees}
                            agents={agents}
                            activePath={activeWorktreeOwnerId === project.workspaceId ? activePath : ""}
                            statuses={statuses}
                            unreadWorktreePaths={unreadWorktreePaths}
                            activityByWorktreePath={activityByWorktreePath}
                            onSelect={onSelectWorktree}
                            onDelete={onDeleteWorktree}
                            sortableWorkspaceId={project.workspaceId}
                            label={`${project.workspaceId} worktrees`}
                          />
                        </SortableContext>
                      </div>
                    ) : null}
                  </SortableProjectSection>
                );
              })}
            </SortableContext>
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-end border-t border-worktree-sidebar-border px-2 py-1.5">
          <IconButton label="Settings" size="sm" onClick={onOpenSettings}>
            <Settings2 className="size-3.5" />
          </IconButton>
        </div>

        <div
          role="separator"
          aria-label="Resize sidebar"
          aria-orientation="vertical"
          aria-valuemin={MIN_SIDEBAR_WIDTH}
          aria-valuemax={MAX_SIDEBAR_WIDTH}
          aria-valuenow={width}
          onPointerDown={startResize}
          className="no-drag absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none"
        >
          <span className="pointer-events-none absolute inset-y-0 right-0 w-px bg-worktree-sidebar-border transition-colors group-hover:bg-ring/45" />
        </div>
      </aside>

      <DragOverlay dropAnimation={null}>
        {activeProjectOverlay ? (
          <div data-testid="sidebar-drag-overlay" className="w-sidebar max-w-[calc(100vw-1rem)]">
            <SidebarDragRow kind="project" overlay>
              <ProjectHeader
                project={activeProjectOverlay}
                active={activeProjectOverlay.workspaceId === activeProjectId}
                expanded={!collapsedProjects.has(activeProjectOverlay.workspaceId)}
                activity={summarizeProjectActivity(
                  worktreesByProject.get(activeProjectOverlay.workspaceId) ?? [],
                  activityByWorktreePath,
                  unreadWorktreePaths,
                )}
                attentionState={projectAttentionState(
                  summarizeProjectActivity(
                    worktreesByProject.get(activeProjectOverlay.workspaceId) ?? [],
                    activityByWorktreePath,
                    unreadWorktreePaths,
                  ),
                )}
                inert
              />
            </SidebarDragRow>
          </div>
        ) : activeWorktreeOverlay && activeDrag?.type === "sidebar-worktree" ? (
          <div data-testid="sidebar-drag-overlay" className="w-sidebar max-w-[calc(100vw-1rem)]">
            <SidebarDragRow kind="worktree" overlay>
              <WorktreeRow
                worktree={activeWorktreeOverlay}
                active={activeWorktreeOwnerId === activeDrag.workspaceId && activeWorktreeOverlay.path === activePath}
                agent={agents.find((agent) => agent.worktreePath === activeWorktreeOverlay.path)}
                status={statuses[activeWorktreeOverlay.path]}
                unread={Boolean(unreadWorktreePaths?.[activeWorktreeOverlay.path])}
                activitySummary={activityByWorktreePath?.[activeWorktreeOverlay.path]}
                onSelect={() => undefined}
                onDelete={() => undefined}
              />
            </SidebarDragRow>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

type ProjectHeaderProps = {
  project: RegisteredProject;
  active: boolean;
  expanded: boolean;
  activity: ActivitySummary;
  attentionState: "waiting" | "unread" | null;
  onToggle?: () => void;
  onSelect?: () => void;
  onCreateWorktree?: () => void;
  inert?: boolean;
};

function ProjectHeader({
  project,
  active,
  expanded,
  activity,
  attentionState,
  onToggle,
  onSelect,
  onCreateWorktree,
  inert = false,
}: ProjectHeaderProps) {
  return (
    <div className="group/project flex h-7 w-full items-center gap-0.5 rounded-md pr-1 text-worktree-sidebar-foreground/65 transition-colors hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground">
      <button
        type="button"
        disabled={inert}
        onClick={onToggle}
        onPointerDown={(event) => event.stopPropagation()}
        aria-expanded={expanded}
        aria-label={`${expanded ? "Collapse" : "Expand"} ${project.workspaceId}`}
        className="flex size-5 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-worktree-sidebar-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none"
      >
        <ChevronRight
          aria-hidden="true"
          className={cn("size-3 shrink-0 transition-transform", expanded && "rotate-90")}
        />
      </button>
      <button
        type="button"
        disabled={inert}
        onClick={onSelect}
        aria-current={active ? "true" : undefined}
        aria-label={project.workspaceId}
        className="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm py-1 text-left text-[12px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none"
      >
        <FolderGit2 className="size-3.5 shrink-0" />
        <span className="min-w-0 flex-1 truncate">{project.workspaceId}</span>
        {activity.runningCount > 0 ? (
          <span
            data-testid="project-running-badge"
            className="shrink-0 rounded-full bg-status-working/12 px-1.5 py-px text-[9px] font-medium leading-none text-status-working"
          >
            {activity.runningCount} running
          </span>
        ) : null}
        {attentionState ? (
          <span
            data-testid="project-attention-indicator"
            data-attention-state={attentionState}
            title={attentionState === "waiting" ? "Agent needs attention" : "Unread activity"}
            className="inline-flex size-3 shrink-0 items-center justify-center"
          >
            <StatusDot state={attentionState} />
          </span>
        ) : null}
      </button>
      {project.gitRoot !== null ? (
        <IconButton
          label={`Add worktree to ${project.workspaceId}`}
          size="sm"
          disabled={inert}
          className="size-5 opacity-55 transition-opacity focus-visible:opacity-100 group-hover/project:opacity-100"
          onClick={onCreateWorktree}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <FolderPlus className="size-3" />
        </IconButton>
      ) : null}
    </div>
  );
}

function projectAttentionState(activity: ActivitySummary): "waiting" | "unread" | null {
  if (activity.hasWaiting) return "waiting";
  return activity.hasUnread ? "unread" : null;
}

function summarizeProjectActivity(
  worktrees: Worktree[],
  activityByWorktreePath: Record<string, ActivitySummary | undefined> | undefined,
  unreadWorktreePaths: Record<string, boolean> | undefined,
): ActivitySummary {
  const summaries = worktrees
    .map((worktree) => activityByWorktreePath?.[worktree.path])
    .filter((summary): summary is ActivitySummary => Boolean(summary));
  const hasUnread = worktrees.some((worktree) => Boolean(unreadWorktreePaths?.[worktree.path]));
  return combineActivitySummaries(summaries, hasUnread);
}

/**
 * Worktrees carry their owning project inside their `orca/<wsId>/<slug>` branch name. Anything
 * without that identity is attributed by path — a project root, or a path nested under one — so a
 * plain-folder project's root row never lands under whichever project happens to be active.
 */
function groupWorktreesByProject(
  worktrees: Worktree[],
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
  inactiveProjectWorktrees?: Record<string, Worktree[]>,
) {
  const grouped = new Map<string, Worktree[]>();
  for (const project of projects) {
    const listed = project.workspaceId === activeProjectId ? [] : inactiveProjectWorktrees?.[project.workspaceId];
    grouped.set(project.workspaceId, listed ? [...listed] : []);
  }

  for (const worktree of worktrees) {
    const owner = resolveWorktreeOwnerId(worktree, projects, activeProjectId);
    if (!owner) continue;
    const bucket = grouped.get(owner);
    if (!bucket || bucket.some((candidate) => candidate.path === worktree.path)) continue;
    bucket.push(worktree);
  }

  if (activeProjectId) {
    const activeBucket = grouped.get(activeProjectId);
    if (activeBucket && activeBucket.length === 0) {
      const cached = inactiveProjectWorktrees?.[activeProjectId];
      if (cached && cached.length > 0) {
        for (const row of cached) {
          if (!activeBucket.some((candidate) => candidate.path === row.path)) {
            activeBucket.push(row);
          }
        }
      }
    }
  }

  // A non-Git project has no git worktrees at all, so without a synthesized
  // folder row its group would render empty and the folder would be
  // unselectable - including while it is the active project.
  for (const project of projects) {
    if (project.gitRoot !== null) continue;
    const bucket = grouped.get(project.workspaceId);
    if (!bucket || bucket.length > 0) continue;
    bucket.push({
      path: project.repoRoot,
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    });
  }

  return grouped;
}

function applyWorktreeOrder(grouped: Map<string, Worktree[]>, order: WorktreeOrder) {
  const ordered = new Map<string, Worktree[]>();
  for (const [workspaceId, rows] of grouped) {
    const byPath = new Map(rows.map((row) => [row.path, row]));
    const seen = new Set<string>();
    const next: Worktree[] = [];
    for (const path of order[workspaceId] ?? []) {
      const row = byPath.get(path);
      if (!row || seen.has(path)) continue;
      seen.add(path);
      next.push(row);
    }
    for (const row of rows) {
      if (seen.has(row.path)) continue;
      seen.add(row.path);
      next.push(row);
    }
    ordered.set(workspaceId, next);
  }
  return ordered;
}

function pruneWorktreeOrder(order: WorktreeOrder, grouped: Map<string, Worktree[]>) {
  const pruned: WorktreeOrder = {};
  for (const [workspaceId, storedPaths] of Object.entries(order)) {
    const existingPaths = new Set((grouped.get(workspaceId) ?? []).map((row) => row.path));
    const seen = new Set<string>();
    const next = storedPaths.filter((path) => {
      if (!existingPaths.has(path) || seen.has(path)) return false;
      seen.add(path);
      return true;
    });
    if (next.length > 0) pruned[workspaceId] = next;
  }
  return pruned;
}

function worktreeOrdersEqual(left: WorktreeOrder, right: WorktreeOrder) {
  const leftEntries = Object.entries(left);
  const rightEntries = Object.entries(right);
  if (leftEntries.length !== rightEntries.length) return false;
  return leftEntries.every(([workspaceId, paths]) => {
    const other = right[workspaceId];
    return other?.length === paths.length && paths.every((path, index) => path === other[index]);
  });
}

function readSidebarDragData(value: unknown): SidebarDragData | null {
  if (!value || typeof value !== "object" || !("type" in value)) return null;
  if (value.type === "sidebar-project") {
    return "workspaceId" in value && typeof value.workspaceId === "string"
      ? { type: value.type, workspaceId: value.workspaceId }
      : null;
  }
  if (value.type === "sidebar-worktree") {
    return "workspaceId" in value &&
      typeof value.workspaceId === "string" &&
      "worktreePath" in value &&
      typeof value.worktreePath === "string"
      ? { type: value.type, workspaceId: value.workspaceId, worktreePath: value.worktreePath }
      : null;
  }
  return null;
}

function loadWorktreeOrder(): WorktreeOrder {
  try {
    const raw = getMigratedItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).flatMap(([workspaceId, paths]) => {
        if (!Array.isArray(paths)) return [];
        const uniquePaths = [...new Set(paths.filter((path): path is string => typeof path === "string"))];
        return uniquePaths.length > 0 ? [[workspaceId, uniquePaths]] : [];
      }),
    );
  } catch {
    return {};
  }
}

function persistWorktreeOrder(order: WorktreeOrder) {
  try {
    window.localStorage.setItem(SIDEBAR_WORKTREE_ORDER_STORAGE_KEY, JSON.stringify(order));
  } catch {
    // A storage failure should not break drag reordering for this session.
  }
}

function seedCollapsedProjects(
  collapsed: Set<string>,
  projects: RegisteredProject[],
  activeProjectId: string | undefined,
) {
  const seeded = projects.filter(
    (project) => project.workspaceId !== activeProjectId && !collapsed.has(project.workspaceId),
  );
  if (seeded.length === 0) return collapsed;
  const next = new Set(collapsed);
  for (const project of seeded) next.add(project.workspaceId);
  return next;
}

/** Returns `null` when the accordion has never been persisted, so callers can apply defaults. */
function loadCollapsedProjects(): Set<string> | null {
  try {
    const raw = getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return null;
    return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    return null;
  }
}

function persistCollapsedProjects(collapsed: Set<string>) {
  try {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([...collapsed]));
  } catch {
    // A storage failure should not break accordion toggling for this session.
  }
}

function loadSidebarWidth() {
  try {
    const raw = getMigratedItem(SIDEBAR_WIDTH_STORAGE_KEY);
    const stored = Number(raw);
    return Number.isFinite(stored) && stored > 0 ? clampSidebarWidth(stored) : DEFAULT_SIDEBAR_WIDTH;
  } catch {
    return DEFAULT_SIDEBAR_WIDTH;
  }
}

function persistSidebarWidth(width: number) {
  try {
    window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(width)));
  } catch {
    // A storage failure should not break pointer resizing for this session.
  }
}

function clampSidebarWidth(width: number) {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, Math.round(width)));
}
