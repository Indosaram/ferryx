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
  Activity,
  CheckCheck,
  ChevronRight,
  Folder,
  MoreHorizontal,
  PanelLeftClose,
  Plus,
  Settings2,
  Eye,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState, useSyncExternalStore } from "react";
import { toast } from "sonner";

import { resolveActivityIndicator, type ActivitySummary } from "../lib/activity";
import { cn } from "../lib/cn";
import { workspaceName } from "../lib/branchFilter";
import { projectRootWorktree } from "../lib/projectIdentity";
import { groupProjects, isProjectGroupActive } from "../lib/projectGrouping";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import { useSshHosts } from "../lib/sshHosts";
import type { RemoteContext } from "../lib/machineNavigation";
import {
  getPairedHostReachability,
  subscribePairedHostReachability,
  usePairedHostReachability,
} from "../lib/pairedHostReachability";
import { remoteHostStore } from "../state/remoteHostStore";
import { worktreeListingFailureStore } from "../state/worktreeListingFailures";
import { isMacShortcutPlatform } from "../lib/shortcuts";
import {
  clampSidebarWidth,
  loadCollapsedProjects,
  loadSidebarWidth,
  loadWorktreeOrder,
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  persistCollapsedProjects,
  persistSidebarWidth,
  persistWorktreeOrder,
  type WorktreeOrder,
} from "../lib/sidebarPersistence";
import {
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
} from "../lib/storageKeys";
import { revealPath, type RegisteredProject } from "../lib/tauri";
import { AttentionInbox } from "../features/ferryx/attention/AttentionInbox";
import type { AttentionRow } from "../features/ferryx/attention/attentionModel";
import type { ActiveAgent, DirtyState, Worktree } from "../lib/types";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
import { projectSortableId, SortableProjectSection } from "./sidebar-dnd/SortableProjectSection";
import { openNativePopupMenu, type NativeMenuEntry } from "../lib/nativeMenu";
import { IconButton } from "./ui/IconButton";
import { Input } from "./ui/input";
import { SectionHeader } from "./ui/SectionHeader";
import { SegmentedControl } from "./ui/SegmentedControl";
import { SupervisorPanel, useSupervisorTyping } from "./supervisor/SupervisorPanel";
import { useAdvancedFeatures } from "../lib/advancedFeatures";
import { SidebarStateSlot } from "./sidebar/SidebarStateSlot";
import {
  filterSidebarEntries,
  groupHeaderShowsHost,
  isStandaloneRemoteGroup,
  projectDisplayLabel,
  projectFilterLabels,
  projectHostLabel,
  SIDEBAR_EMPTY_TEXT,
  worktreeLabel,
} from "./sidebar/sidebarLabels";
import {
  applyWorktreeOrder,
  emptyProjectGroupIds,
  groupWorktreesByProject,
  initialCollapsedProjects,
  seedCollapsedProjects,
} from "./sidebar/sidebarRows";
import {
  pairedSlotStatus,
  projectAttentionState,
  projectSlot,
  projectStatus,
  summarizeProjectActivity,
  worktreesFooterSummary,
} from "./sidebar/sidebarSlots";
import type { SectionId } from "./settings/types";
import { EmptyWorktreesHint, fileManagerActionLabel, WorktreeList, WorktreeRow, worktreeSortableId } from "./WorktreeList";

export {
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
};
const SIDEBAR_KEYBOARD_STEP = 16;

type SidebarProjectDragData = {
  type: "sidebar-project";
  workspaceId: string;
};

type SidebarWorktreeDragData = {
  type: "sidebar-worktree";
  workspaceId: string;
  worktreePath: string;
  rowWorkspaceId?: string;
};

type SidebarDragData = SidebarProjectDragData | SidebarWorktreeDragData;

type SidebarProps = {
  open?: boolean;
  isMac?: boolean;
  projects?: RegisteredProject[];
  activeProjectId?: string;
  worktrees: Worktree[];
  inactiveProjectWorktrees?: Record<string, Worktree[]>;
  emptyWorkspaceIds?: readonly string[];
  agents: ActiveAgent[];
  activePath: string;
  statuses?: Record<string, DirtyState | undefined>;
  unreadWorktreePaths?: Record<string, boolean>;
  activityByWorktreePath?: Record<string, ActivitySummary | undefined>;
  onSelectProject?: (project: RegisteredProject) => void;
  onReorderProjects?: (orderedWorkspaceIds: string[]) => void;
  onAddProject?: () => void;
  onRemoveProject?: (project: RegisteredProject) => void;
  onSelectWorktree: (worktree: Worktree) => void;
  onCreateWorktree: (project?: RegisteredProject) => void;
  onDeleteWorktree?: (worktree: Worktree) => void;
  onResetAgentState?: (worktree: Worktree) => void;
  onManageDisk?: (project: RegisteredProject) => void;
  onOpenHistory?: (project: RegisteredProject) => void;
  onOpenCommandPalette?: () => void;
  /** A remote context deep-links Settings > Remote to one machine (paired status slot). */
  onOpenSettings?: (section?: SectionId, remoteContext?: RemoteContext) => void;
  onOpenResources?: () => void;
  onToggle?: () => void;
  onHide?: () => void;
  /** Present when the sidebar offers the attention inbox as a sub view behind its header icon. */
  attention?: SidebarAttention;
};

export type SidebarAttention = {
  rows: readonly AttentionRow[];
  onOpen: (row: AttentionRow) => void;
  onDismiss: (row: AttentionRow) => void;
  onDismissAll: () => void;
  inboxOpen: boolean;
  onInboxOpenChange: (open: boolean) => void;
};

export function Sidebar({
  open = true,
  isMac = isMacShortcutPlatform(),
  projects = [],
  activeProjectId,
  worktrees,
  inactiveProjectWorktrees,
  emptyWorkspaceIds,
  agents,
  activePath,
  statuses = {},
  unreadWorktreePaths,
  activityByWorktreePath,
  onSelectProject = () => undefined,
  onReorderProjects = () => undefined,
  onAddProject = () => undefined,
  onRemoveProject,
  onSelectWorktree,
  onCreateWorktree,
  onDeleteWorktree = () => undefined,
  onResetAgentState,
  onManageDisk,
  onOpenHistory,
  onOpenSettings,
  onOpenResources,
  onToggle,
  onHide,
  attention,
}: SidebarProps) {
  const selectWorktree = onSelectWorktree;
  const inboxOpen = attention?.inboxOpen ?? false;
  const [supervisorOpenState, setSupervisorOpen] = useState(false);
  const advancedFeatures = useAdvancedFeatures();
  const supervisorOpen = supervisorOpenState && !inboxOpen && advancedFeatures;
  const supervisorTyping = useSupervisorTyping();
  const sidebarView = supervisorOpen ? "supervisor" : inboxOpen ? "inbox" : "worktrees";
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [resizing, setResizing] = useState(false);
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

  const projectGroups = useMemo(() => groupProjects(projects), [projects]);
  const emptyGroupIds = useMemo(
    () => emptyProjectGroupIds(projectGroups, emptyWorkspaceIds),
    [emptyWorkspaceIds, projectGroups],
  );

  // Accordion state is stored as the set of *collapsed* projects, so each project keeps its own
  // open/closed state and toggling one never disturbs another. Projects the user has never
  // toggled default to collapsed unless they are the active project.
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() =>
    initialCollapsedProjects({ stored: loadCollapsedProjects(), projects, activeProjectId, emptyGroupIds }));
  const previousEmptyGroupIds = useRef(emptyGroupIds);
  useEffect(() => {
    const newlyEmpty = [...emptyGroupIds].filter((id) => !previousEmptyGroupIds.current.has(id));
    const restored = [...previousEmptyGroupIds.current].filter((id) => !emptyGroupIds.has(id));
    previousEmptyGroupIds.current = emptyGroupIds;
    if (newlyEmpty.length === 0 && restored.length === 0) return;
    setCollapsedProjects((current) => {
      const next = new Set(current);
      for (const id of newlyEmpty) next.add(id);
      for (const id of restored) next.delete(id);
      return next;
    });
  }, [emptyGroupIds]);
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

  const { hosts } = useSshHosts();

  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  // Paired worktree rows read reachability non-reactively, so regroup whenever it changes.
  const [reachabilityVersion, bumpReachabilityVersion] = useReducer((version: number) => version + 1, 0);
  useEffect(() => {
    const unsubscribe = subscribePairedHostReachability(bumpReachabilityVersion);
    return () => { unsubscribe(); };
  }, []);
  const naturalWorktreesByProject = useMemo(
    () => groupWorktreesByProject({
      worktrees,
      projects,
      activeProjectId,
      inactiveProjectWorktrees,
      hosts,
      remoteState,
      reachability: getPairedHostReachability,
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reachabilityVersion invalidates the store read inside
    [activeProjectId, hosts, inactiveProjectWorktrees, projects, worktrees, remoteState, reachabilityVersion],
  );
  const worktreesByProject = useMemo(
    () => applyWorktreeOrder(naturalWorktreesByProject, worktreeOrder),
    [naturalWorktreesByProject, worktreeOrder],
  );
  const projectSortableItems = useMemo(
    () => projectGroups.map((group) => projectSortableId(group.groupId)),
    [projectGroups],
  );

  // The filter is session-only UI state: never persisted.
  const [filterQuery, setFilterQuery] = useState("");
  const visibleProjectGroups = useMemo(
    () => filterSidebarEntries(
      projectGroups.map((group) => ({ group, rows: worktreesByProject.get(group.primaryProject.workspaceId) ?? [] })),
      filterQuery,
      { project: (group) => projectFilterLabels(group, { hosts, remoteState }), row: worktreeLabel },
    ),
    [filterQuery, hosts, projectGroups, remoteState, worktreesByProject],
  );
  const statusFooterText = useMemo(
    () => worktreesFooterSummary(visibleProjectGroups.flatMap((entry) => entry.rows), activityByWorktreePath),
    [activityByWorktreePath, visibleProjectGroups],
  );

  // The active row is highlighted in whichever project's group actually renders
  // it, so a nested project sharing the path prefix does not light up too.
  // Only the active project can own the active row. Matching by path alone lit
  // up an inactive project whose list still contained that path.
  const activeWorktreeOwnerId = useMemo(() => {
    if (!activePath || !activeProjectId) return undefined;
    const activeGroup = projectGroups.find((g) => isProjectGroupActive(g, activeProjectId));
    if (!activeGroup) return undefined;
    const activeRows = worktreesByProject.get(activeGroup.primaryProject.workspaceId);
    return activeRows?.some(
      (row) =>
        row.path === activePath &&
        (row.workspaceId ? row.workspaceId === activeProjectId : true),
    )
      ? activeGroup.primaryProject.workspaceId
      : undefined;
  }, [activePath, activeProjectId, projectGroups, worktreesByProject]);

  const endResize = useCallback(() => {
    if (!dragRef.current) return;
    dragRef.current = null;
    persistSidebarWidth(widthRef.current);
    document.body.style.cursor = "";
    setResizing(false);
  }, []);

  useEffect(() => {
    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const nextWidth = clampSidebarWidth(drag.startWidth + event.clientX - drag.startX);
      widthRef.current = nextWidth;
      setWidth(nextWidth);
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", endResize);
      window.removeEventListener("pointercancel", endResize);
      document.body.style.cursor = "";
    };
  }, [endResize]);

  const startResize = (event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    dragRef.current = { startX: event.clientX, startWidth: widthRef.current };
    document.body.style.cursor = "col-resize";
    setResizing(true);
    // Capturing the pointer keeps pointerup/lostpointercapture routed here even over a native pane.
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // The pointer may already be released; window listeners still end the drag.
    }
  };

  const resizeWithKeyboard = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const delta = event.key === "ArrowLeft" ? -SIDEBAR_KEYBOARD_STEP : SIDEBAR_KEYBOARD_STEP;
    const nextWidth = clampSidebarWidth(widthRef.current + delta);
    widthRef.current = nextWidth;
    setWidth(nextWidth);
    persistSidebarWidth(nextWidth);
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
    const paths = rows.map((worktree) => worktreeSortableId(active.workspaceId, worktree.path, worktree.workspaceId));
    const fromIndex = paths.indexOf(worktreeSortableId(active.workspaceId, active.worktreePath, active.rowWorkspaceId));
    const toIndex = paths.indexOf(worktreeSortableId(over.workspaceId, over.worktreePath, over.rowWorkspaceId));
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
          ?.find((worktree) => worktreeSortableId(activeDrag.workspaceId, worktree.path, worktree.workspaceId) ===
            worktreeSortableId(activeDrag.workspaceId, activeDrag.worktreePath, activeDrag.rowWorkspaceId))
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
          className="flex h-titlebar shrink-0 items-center px-2 gap-0.5"
        >
          {isMac ? <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" /> : null}
          <IconButton
            label="Hide sidebar"
            data-shortcut={onToggle || onHide ? "sidebar.left.toggle" : undefined}
            className="no-drag"
            size="sm"
            onClick={onToggle ?? onHide}
          >
            <PanelLeftClose className="size-3.5" />
          </IconButton>
          <IconButton
            data-shortcut="project.add"
            label="Add project"
            className="no-drag"
            size="sm"
            onClick={onAddProject}
          >
            <Plus className="size-3.5" />
          </IconButton>
          {attention ? (
            <div className="ml-auto flex items-center gap-0.5">
              {inboxOpen ? (
                <IconButton
                  label="Mark all read"
                  className="no-drag"
                  size="sm"
                  disabled={attention.rows.length === 0}
                  onClick={attention.onDismissAll}
                >
                  <CheckCheck className="size-3.5" />
                </IconButton>
              ) : null}
            </div>
          ) : null}
        </div>

        {attention ? (
          <div className="shrink-0 px-2 pb-1">
            <SegmentedControl
              label="Sidebar view"
              value={sidebarView}
              onChange={(view) => {
                setSupervisorOpen(view === "supervisor");
                if ((view === "inbox") !== inboxOpen) attention.onInboxOpenChange(view === "inbox");
              }}
              options={[
                { value: "worktrees", label: "Worktrees" },
                {
                  value: "inbox",
                  label: "Inbox",
                  ariaLabel: attention.rows.length > 0 ? `Inbox (${attention.rows.length})` : "Inbox",
                  count: attention.rows.length > 0 ? (attention.rows.length > 9 ? "9+" : attention.rows.length) : undefined,
                  countTestId: "sidebar-attention-badge",
                  "data-shortcut": "notifications.toggle",
                },
                ...(advancedFeatures
                  ? [{
                      value: "supervisor" as const,
                      label: "Supervisor",
                      ariaLabel: supervisorTyping ? "Supervisor (typing)" : "Supervisor",
                      count: supervisorTyping ? <Eye aria-hidden="true" className="size-3 text-foreground" /> : undefined,
                      countTestId: "sidebar-supervisor-typing",
                    }]
                  : []),
              ]}
            />
          </div>
        ) : null}

        {attention && supervisorOpen ? <SupervisorPanel /> : null}

        {attention && inboxOpen ? (
          <AttentionInbox
            rows={attention.rows}
            onOpen={attention.onOpen}
            onDismiss={attention.onDismiss}
          />
        ) : null}

        <div
          data-testid="worktree-region"
          hidden={inboxOpen || supervisorOpen || undefined}
          className={cn("min-h-0 flex-1 flex-col", inboxOpen || supervisorOpen ? "hidden" : "flex")}
        >
          <div className="shrink-0 px-2 pt-1">
            <Input
              type="search"
              value={filterQuery}
              placeholder="Filter worktrees"
              aria-label="Filter worktrees"
              className="h-7 text-[12px] md:text-[12px]"
              onChange={(event) => setFilterQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Escape" || filterQuery === "") return;
                event.preventDefault();
                event.stopPropagation();
                setFilterQuery("");
              }}
            />
          </div>
          <SectionHeader title="Projects" count={projectGroups.length} className="shrink-0" />
          <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2 pb-2 scrollbar-sleek">
            {projects.length === 0 ? (
              <p className="px-2 py-3 text-[11px] leading-relaxed text-muted-foreground">
                {SIDEBAR_EMPTY_TEXT.noProjects}
              </p>
            ) : null}

            <SortableContext items={projectSortableItems} strategy={verticalListSortingStrategy}>
              {visibleProjectGroups.map(({ group, rows: projectWorktrees, forceExpanded }) => {
                const project = group.primaryProject;
                const active = isProjectGroupActive(group, activeProjectId);
                const expanded = forceExpanded || !collapsedProjects.has(group.groupId);
                const staleSummary = projectWorktrees.find((w) => w.stale || w.freshness?.stale)?.hostSummary;
                const projectActivity = summarizeProjectActivity(
                  projectWorktrees,
                  activityByWorktreePath,
                  unreadWorktreePaths,
                );
                const attentionState = projectAttentionState(projectActivity);
                const activityIndicator = resolveActivityIndicator(projectActivity);
                const isStandaloneRemote = isStandaloneRemoteGroup(group);
                const standaloneHostLabel = project.target?.kind === "ssh"
                  ? (projectHostLabel(project, { hosts, remoteState }) ?? undefined)
                  : undefined;
                const standaloneRootWorktree = projectRootWorktree(project, standaloneHostLabel);
                const standaloneDisplayName = workspaceName(standaloneRootWorktree);
                const isLocal =
                  !isRemoteWorkspaceId(project.workspaceId) &&
                  !isPairedWorkspaceId(project.workspaceId);
                const header = (
                  <ProjectHeader
                    project={project}
                    active={active}
                    expanded={expanded}
                    activity={projectActivity}
                    attentionState={attentionState}
                    onToggle={() => toggleProject(group.groupId)}
                    onSelect={() => {
                      onSelectProject(project);
                      if (!expanded) toggleProject(group.groupId);
                    }}
                    onCreateWorktree={() => onCreateWorktree(project)}
                    onRemoveProject={onRemoveProject ? () => onRemoveProject(project) : undefined}
                    onManageDisk={onManageDisk ? () => onManageDisk(project) : undefined}
                    onOpenHistory={onOpenHistory && isLocal ? () => onOpenHistory(project) : undefined}
                    isStandaloneRemote={groupHeaderShowsHost(group)}
                    staleSummary={staleSummary}
                    onOpenRemoteSettings={onOpenSettings ? (context) => onOpenSettings("remote", context) : undefined}
                  />
                );

                return (
                  <SortableProjectSection key={group.groupId} workspaceId={group.groupId} header={header}>
                    {expanded ? (
                      <div
                        className="ml-3 border-l border-worktree-sidebar-border pl-2 pr-0.5 pt-0.5"
                        onPointerDown={(event) => event.stopPropagation()}
                      >
                        <SortableContext
                          items={projectWorktrees.map((row) => worktreeSortableId(group.groupId, row.path, row.workspaceId))}
                          strategy={verticalListSortingStrategy}
                        >
                          {isStandaloneRemote ? (
                            <button
                              type="button"
                              className={cn(
                                "group/worktree-row relative my-0.5 flex h-7 w-full items-center gap-1.5 rounded-md px-2 text-left text-xs transition-colors",
                                activeWorktreeOwnerId === project.workspaceId
                                  ? "bg-worktree-sidebar-accent text-foreground"
                                  : "bg-transparent hover:bg-white/[0.04]",
                              )}
                              aria-current={activeWorktreeOwnerId === project.workspaceId ? "true" : undefined}
                              title={`Remote worktree: ${project.repoRoot}${standaloneHostLabel ? ` (${standaloneHostLabel})` : ""}`}
                              data-shortcut-worktree-path={project.repoRoot}
                              data-shortcut-workspace-id={project.workspaceId}
                              onClick={() => selectWorktree(standaloneRootWorktree)}
                            >
                              {activeWorktreeOwnerId === project.workspaceId ? (
                                <span
                                  aria-hidden="true"
                                  data-testid="worktree-rail-active"
                                  className="pointer-events-none absolute -left-[9px] top-1 bottom-1 w-px bg-worktree-sidebar-ring"
                                />
                              ) : null}
                              <span
                                className={cn(
                                  "min-w-0 truncate text-[12px] font-semibold leading-tight",
                                  activeWorktreeOwnerId === project.workspaceId ? "text-foreground" : "text-worktree-sidebar-foreground",
                                )}
                              >
                                {standaloneDisplayName}
                              </span>
                              <SidebarStateSlot
                                state={activityIndicator}
                                host={standaloneHostLabel}
                                title={activityIndicator ? undefined : standaloneHostLabel}
                                className="ml-auto min-w-0 truncate"
                                data-testid={activityIndicator ? "worktree-status-dot" : "remote-machine-badge"}
                              />
                            </button>
                          ) : <WorktreeList
                            worktrees={projectWorktrees}
                            repoRoot={project.repoRoot}
                            agents={agents}
                            activePath={activeWorktreeOwnerId === project.workspaceId ? activePath : ""}
                            activeWorkspaceId={activeProjectId}
                            statuses={statuses}
                            unreadWorktreePaths={unreadWorktreePaths}
                            activityByWorktreePath={activityByWorktreePath}
                            onSelect={selectWorktree}
                            onCreateWorktree={
                              project.gitRoot !== null && project.target?.kind !== "ssh" ? () => onCreateWorktree(project) : undefined
                            }
                            onDelete={onDeleteWorktree}
                            onResetAgentState={onResetAgentState}
                            sortableWorkspaceId={group.groupId}
                            label={`${group.groupId} worktrees`}
                            emptyState={
                              <EmptyWorktreesHint
                                onCreateWorktree={
                                  project.gitRoot !== null && project.target?.kind !== "ssh" ? () => onCreateWorktree(project) : undefined
                                }
                              />
                            }
                          />}
                        </SortableContext>
                      </div>
                    ) : null}
                  </SortableProjectSection>
                );
              })}
            </SortableContext>
          </div>
        </div>

        <div className="flex shrink-0 items-center justify-end gap-1 border-t border-worktree-sidebar-border px-2 py-1.5">
          <span data-testid="sidebar-status-footer" className="mr-auto min-w-0 truncate pl-0.5 text-[10px] text-muted-foreground">
            {statusFooterText}
          </span>
          <IconButton label="System resources" size="sm" onClick={onOpenResources}>
            <Activity className="size-3.5" />
          </IconButton>
          <IconButton data-shortcut={onOpenSettings ? "settings.toggle" : undefined} label="Settings" size="sm" onClick={() => onOpenSettings?.()}>
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
          tabIndex={0}
          onPointerDown={startResize}
          onLostPointerCapture={endResize}
          onKeyDown={resizeWithKeyboard}
          className="no-drag absolute inset-y-0 right-0 z-30 w-1.5 cursor-col-resize touch-none"
        >
          <span className="pointer-events-none absolute inset-y-0 right-0 w-px bg-worktree-sidebar-border transition-colors group-hover:bg-ring/45" />
        </div>
        {resizing ? (
          // A dialog-role surface makes native terminal/browser surfaces yield (see
          // nativeTerminalVisibility) so a native pane cannot swallow the resize pointerup.
          <div role="dialog" aria-hidden="true" data-testid="sidebar-resize-mask" className="fixed inset-0 z-50 cursor-col-resize" />
        ) : null}
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
                repoRoot={projects.find((p) => p.workspaceId === activeDrag.workspaceId)?.repoRoot}
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
  attentionState: "waiting" | "done" | "unread" | null;
  onToggle?: () => void;
  onSelect?: () => void;
  onCreateWorktree?: () => void;
  onRemoveProject?: () => void;
  onManageDisk?: () => void;
  onOpenHistory?: () => void;
  inert?: boolean;
  isStandaloneRemote?: boolean;
  staleSummary?: string;
  onOpenRemoteSettings?: (context: RemoteContext) => void;
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
  onRemoveProject,
  onManageDisk,
  onOpenHistory,
  inert = false,
  isStandaloneRemote = true,
  staleSummary,
  onOpenRemoteSettings,
}: ProjectHeaderProps) {
  const { hosts } = useSshHosts();
  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const reachability = usePairedHostReachability(project.target?.kind === "pairedDaemon" ? project.target.hostId : "");
  const listingFailure = useSyncExternalStore(
    worktreeListingFailureStore.subscribe,
    worktreeListingFailureStore.getState,
  )[project.workspaceId];
  const remote = project.target && project.target.kind !== "local" ? project.target : null;
  const hostLabel = projectHostLabel(project, { hosts, remoteState });
  const projectLabel = projectDisplayLabel(project, hostLabel, isStandaloneRemote);
  const menuUnlistenRef = useRef<(() => void) | null>(null);
  const slot = projectSlot({
    status: projectStatus({
      listingFailure,
      paired: remote?.kind === "pairedDaemon"
        ? { staleSummary, ...pairedSlotStatus(remote.hostId, remoteState, reachability) }
        : null,
    }),
    activity,
    attentionState,
  });
  const pairedHostId = remote?.kind === "pairedDaemon" ? remote.hostId : null;
  const activateSlot = slot.kind === "paired" && !slot.connected && pairedHostId && onOpenRemoteSettings && !inert
    ? () => onOpenRemoteSettings({
        page: "machines",
        filter: "all",
        machine: { kind: "pairedDaemon", hostId: pairedHostId, generation: remoteState.hosts[pairedHostId]?.generation ?? "" },
      })
    : undefined;
  const slotElement = slot.kind === "none" ? null : (
    <SidebarStateSlot
      state={slot.state}
      label={slot.label}
      title={slot.title}
      className={slot.kind === "failure" ? "text-destructive" : undefined}
      data-testid={slot.testId}
      data-attention-state={slot.kind === "attention" ? slot.state ?? undefined : undefined}
      onActivate={activateSlot}
      ariaLabel={activateSlot ? `${slot.label}: open remote settings for ${hostLabel ?? projectLabel}` : undefined}
    />
  );

  useEffect(() => {
    return () => {
      menuUnlistenRef.current?.();
      menuUnlistenRef.current = null;
    };
  }, []);

  const openProjectMenu = (position: { x: number; y: number }) => {
    const items: NativeMenuEntry[] = [
      { kind: "item", id: "add-worktree", label: "Add Worktree", enabled: project.gitRoot !== null, icon: "add" },
      { kind: "item", id: "manage-disk", label: "Manage Worktree Disk…", enabled: !remote && project.gitRoot !== null, icon: "disk" },
      { kind: "item", id: "past-conversations", label: "Past Conversations…", enabled: Boolean(onOpenHistory), icon: "history" },
      { kind: "item", id: "reveal", label: remote ? "Local reveal unavailable over SSH" : fileManagerActionLabel(), enabled: !remote, icon: "reveal" },
      { kind: "item", id: "copy-path", label: "Copy Project Path" },
      { kind: "separator" },
      { kind: "item", id: "remove", label: "Remove Project", enabled: Boolean(onRemoveProject), icon: "trash" },
    ];
    menuUnlistenRef.current?.();
    const controller = new AbortController();
    menuUnlistenRef.current = () => controller.abort();
    void openNativePopupMenu(
      "cmd_native_sidebar_context_menu",
      items,
      position,
      (id) => {
        menuUnlistenRef.current?.();
        menuUnlistenRef.current = null;
        if (id === "add-worktree") onCreateWorktree?.();
        else if (id === "manage-disk") onManageDisk?.();
        else if (id === "past-conversations") onOpenHistory?.();
        else if (id === "reveal" && !remote) {
          revealPath(project.repoRoot).catch((err: unknown) => {
            toast.error(`Failed to reveal path: ${err instanceof Error ? err.message : String(err)}`);
          });
        } else if (id === "copy-path") {
          if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
            navigator.clipboard
              .writeText(project.repoRoot)
              .then(() => toast.success("Path copied to clipboard"))
              .catch(() => toast.error("Failed to copy to clipboard"));
          } else {
            toast.error("Clipboard API unavailable");
          }
        } else if (id === "remove") onRemoveProject?.();
      },
      controller.signal,
    ).catch((error: unknown) => console.warn("Could not open native project menu", error));
  };

  const handleContextMenu = (event: React.MouseEvent) => {
    if (inert) return;
    event.preventDefault();
    event.stopPropagation();
    openProjectMenu({ x: event.clientX, y: event.clientY });
  };

  const handleMoreActions = (event: React.MouseEvent<HTMLButtonElement>) => {
    if (inert) return;
    event.stopPropagation();
    const rect = event.currentTarget.getBoundingClientRect();
    openProjectMenu({ x: rect.left, y: rect.bottom });
  };

  return (
    <>
      <div
        onContextMenu={handleContextMenu}
        className="group/project flex h-7 w-full items-center gap-0.5 rounded-md pr-1 text-worktree-sidebar-foreground/65 transition-colors hover:bg-worktree-sidebar-accent/60 hover:text-worktree-sidebar-foreground"
      >
        <button
          type="button"
          disabled={inert}
          onClick={onToggle}
          onPointerDown={(event) => event.stopPropagation()}
          aria-expanded={expanded}
          aria-label={`${expanded ? "Collapse" : "Expand"} ${projectLabel}`}
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
          aria-label={projectLabel}
          title={remote ? `SSH: ${hostLabel} - ${project.repoRoot}` : project.repoRoot}
          className="flex min-w-0 flex-1 items-center gap-1.5 rounded-sm py-1 text-left text-[12px] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none"
        >
          <Folder className="size-3.5 shrink-0" />
          <span className="min-w-0 flex-1 truncate">{projectLabel}</span>
          {activateSlot ? null : slotElement}
        </button>
        {activateSlot ? slotElement : null}
        {project.gitRoot !== null ? (
          <IconButton
            label={`Add worktree to ${project.workspaceId}`}
            size="sm"
            disabled={inert}
            className="size-5 opacity-0 transition-opacity focus-visible:opacity-100 group-hover/project:opacity-100"
            onClick={onCreateWorktree}
            onPointerDown={(event) => event.stopPropagation()}
          >
            <Plus className="size-3.5" />
          </IconButton>
        ) : null}
        <IconButton
          label={`More actions for ${projectLabel}`}
          size="sm"
          disabled={inert}
          className="size-5 opacity-0 transition-opacity focus-visible:opacity-100 group-hover/project:opacity-100"
          onClick={handleMoreActions}
          onPointerDown={(event) => event.stopPropagation()}
        >
          <MoreHorizontal className="size-3.5" />
        </IconButton>
      </div>

    </>
  );
}

function pruneWorktreeOrder(order: WorktreeOrder, grouped: Map<string, Worktree[]>) {
  const pruned: WorktreeOrder = {};
  for (const [workspaceId, storedPaths] of Object.entries(order)) {
    const rows = grouped.get(workspaceId) ?? [];
    const seen = new Set<string>();
    const next = storedPaths.flatMap((path) => {
      const row = rows.find((row) => worktreeSortableId(workspaceId, row.path, row.workspaceId) === path)
        ?? rows.find((row) => (row.path === path || (row.workspaceId && `${row.workspaceId}:${row.path}` === path)) &&
          !seen.has(worktreeSortableId(workspaceId, row.path, row.workspaceId)));
      if (!row) return [];
      const id = worktreeSortableId(workspaceId, row.path, row.workspaceId);
      if (seen.has(id)) return [];
      seen.add(id);
      return [id];
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
      ? { type: value.type, workspaceId: value.workspaceId, worktreePath: value.worktreePath,
          rowWorkspaceId: "rowWorkspaceId" in value && typeof value.rowWorkspaceId === "string" ? value.rowWorkspaceId : undefined }
      : null;
  }
  return null;
}
