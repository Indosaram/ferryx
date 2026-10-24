import {
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
import { useCallback, useEffect, useMemo, useReducer, useRef, useState, useSyncExternalStore, type MouseEvent } from "react";

import type { ActivitySummary } from "../lib/activity";
import { projectRootWorktree } from "../lib/projectIdentity";
import { groupProjects } from "../lib/projectGrouping";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import { useSshHosts } from "../lib/sshHosts";
import type { RemoteContext } from "../lib/machineNavigation";
import { getPairedHostReachability, subscribePairedHostReachability } from "../lib/pairedHostReachability";
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
import type { RegisteredProject } from "../lib/tauri";
import type { AttentionRow } from "../features/ferryx/attention/attentionModel";
import type { ActiveAgent, DirtyState, Worktree } from "../lib/types";
import { useAdvancedFeatures } from "../lib/advancedFeatures";
import { SidebarDragRow } from "./sidebar-dnd/SidebarDragRow";
import { pruneWorktreeOrder, readSidebarDragData, worktreeOrdersEqual, type SidebarDragData } from "./sidebar-dnd/sidebarDragData";
import { projectSortableId, SortableProjectSection } from "./sidebar-dnd/SortableProjectSection";
import { SupervisorPanel, useSupervisorTyping } from "./supervisor/SupervisorPanel";
import { isStandaloneRemoteGroup, projectDisplayLabel, projectHostLabel } from "./sidebar/sidebarLabels";
import { buildSidebarModel, sidebarLiveState, type SidebarProjectNode, type SidebarRowLive, type SidebarWorktreeNode } from "./sidebar/sidebarModel";
import { applyWorktreeOrder, emptyProjectGroupIds, groupWorktreesByProject, initialCollapsedProjects, seedCollapsedProjects } from "./sidebar/sidebarRows";
import { SidebarProjectHeader } from "./sidebar/SidebarProjectHeader";
import { supervisorTypingIn } from "./sidebar/sidebarSupervisor";
import { SidebarView, type SidebarReorder } from "./sidebar/SidebarView";
import type { SidebarViewId } from "./sidebar/SidebarViewTabs";
import { SidebarWorktreeRow } from "./sidebar/SidebarWorktreeRow";
import type { SectionId } from "./settings/types";
import { openProjectMenu } from "./sidebarProjectMenu";
import { agentsByWorktreePath, SortableWorktreeRow, WorktreeRow, worktreeListRowProps, worktreeSortableId } from "./WorktreeList";
import { useSupervisor } from "../lib/supervisor";

export {
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  SIDEBAR_WORKTREE_ORDER_STORAGE_KEY,
};
const SIDEBAR_KEYBOARD_STEP = 16;
const noop = () => undefined;

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

/**
 * Desktop container around the shared SidebarView: it owns persistence (width, order, collapsed
 * set), resize, drag-and-drop and the native menus, and feeds the view a SidebarModel.
 */
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
  onSelectProject = noop,
  onReorderProjects = noop,
  onAddProject = noop,
  onRemoveProject,
  onSelectWorktree,
  onCreateWorktree,
  onDeleteWorktree = noop,
  onResetAgentState,
  onManageDisk,
  onOpenHistory,
  onOpenSettings,
  onOpenResources,
  onToggle,
  onHide,
  attention,
}: SidebarProps) {
  const inboxOpen = attention?.inboxOpen ?? false;
  const [supervisorOpenState, setSupervisorOpen] = useState(false);
  const advancedFeatures = useAdvancedFeatures();
  const supervisorOpen = supervisorOpenState && !inboxOpen && advancedFeatures;
  const supervisorTyping = useSupervisorTyping();
  const sidebarView: SidebarViewId = supervisorOpen ? "supervisor" : inboxOpen ? "inbox" : "worktrees";
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [resizing, setResizing] = useState(false);
  const [width, setWidth] = useState(loadSidebarWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const [worktreeOrder, setWorktreeOrder] = useState<WorktreeOrder>(loadWorktreeOrder);
  const worktreeOrderRef = useRef(worktreeOrder);
  worktreeOrderRef.current = worktreeOrder;
  const [activeDrag, setActiveDrag] = useState<SidebarDragData | null>(null);
  const menuAbortRef = useRef<AbortController | null>(null);
  useEffect(() => () => menuAbortRef.current?.abort(), []);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const projectGroups = useMemo(() => groupProjects(projects), [projects]);
  const emptyGroupIds = useMemo(
    () => emptyProjectGroupIds(projectGroups, emptyWorkspaceIds),
    [emptyWorkspaceIds, projectGroups],
  );

  // Accordion state is the set of *collapsed* groups. The container owns it and persists every
  // change (toggles, seeding of new projects, empty-group transitions) so the published model
  // (desktopSidebarSync) always matches what this sidebar shows.
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(() =>
    initialCollapsedProjects({ stored: loadCollapsedProjects(), projects, activeProjectId, emptyGroupIds }));
  const updateCollapsed = useCallback((update: (current: Set<string>) => Set<string>) => {
    setCollapsedProjects((current) => {
      const next = update(current);
      if (next !== current) persistCollapsedProjects(next);
      return next;
    });
  }, []);
  const previousEmptyGroupIds = useRef(emptyGroupIds);
  useEffect(() => {
    const newlyEmpty = [...emptyGroupIds].filter((id) => !previousEmptyGroupIds.current.has(id));
    const restored = [...previousEmptyGroupIds.current].filter((id) => !emptyGroupIds.has(id));
    previousEmptyGroupIds.current = emptyGroupIds;
    if (newlyEmpty.length === 0 && restored.length === 0) return;
    updateCollapsed((current) => {
      const next = new Set(current);
      for (const id of newlyEmpty) next.add(id);
      for (const id of restored) next.delete(id);
      return next;
    });
  }, [emptyGroupIds, updateCollapsed]);
  const knownProjectsRef = useRef<Set<string> | null>(null);
  if (knownProjectsRef.current === null) {
    knownProjectsRef.current = new Set(projects.map((project) => project.workspaceId));
  }
  useEffect(() => {
    const known = knownProjectsRef.current ?? new Set<string>();
    knownProjectsRef.current = known;
    // Projects registered after mount inherit the same default as the initial seed.
    const unseen = projects.filter((project) => !known.has(project.workspaceId));
    if (unseen.length === 0) return;
    for (const project of unseen) known.add(project.workspaceId);
    updateCollapsed((current) => seedCollapsedProjects(current, unseen, activeProjectId));
  }, [activeProjectId, projects, updateCollapsed]);

  const toggleProject = useCallback((groupId: string) => {
    updateCollapsed((current) => {
      const next = new Set(current);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  }, [updateCollapsed]);

  const { hosts } = useSshHosts();
  const remoteState = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState);
  const listingFailures = useSyncExternalStore(worktreeListingFailureStore.subscribe, worktreeListingFailureStore.getState);
  // Paired rows read reachability non-reactively, so regroup whenever it changes.
  const [reachabilityVersion, bumpReachabilityVersion] = useReducer((version: number) => version + 1, 0);
  useEffect(() => {
    const unsubscribe = subscribePairedHostReachability(bumpReachabilityVersion);
    return () => { unsubscribe(); };
  }, []);
  const rowsInput = useMemo(
    () => ({ worktrees, projects, activeProjectId, inactiveProjectWorktrees, hosts, remoteState, reachability: getPairedHostReachability }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reachabilityVersion invalidates the store read inside
    [activeProjectId, hosts, inactiveProjectWorktrees, projects, worktrees, remoteState, reachabilityVersion],
  );
  const naturalWorktreesByProject = useMemo(() => groupWorktreesByProject(rowsInput), [rowsInput]);
  const worktreesByProject = useMemo(
    () => applyWorktreeOrder(naturalWorktreesByProject, worktreeOrder),
    [naturalWorktreesByProject, worktreeOrder],
  );
  const model = useMemo(() => buildSidebarModel({
    ...rowsInput,
    source: "desktop",
    generatedAt: "",
    worktreeOrder,
    collapsedGroupIds: collapsedProjects,
    advancedFeatures,
    listingFailures,
  }), [advancedFeatures, collapsedProjects, listingFailures, rowsInput, worktreeOrder]);
  const projectById = useMemo(() => new Map(projects.map((project) => [project.workspaceId, project])), [projects]);
  const groupById = useMemo(() => new Map(projectGroups.map((group) => [group.groupId, group])), [projectGroups]);
  const worktreeByKey = useMemo(() => {
    const byKey = new Map<string, Worktree>();
    for (const group of projectGroups) {
      for (const row of worktreesByProject.get(group.primaryProject.workspaceId) ?? []) {
        byKey.set(worktreeSortableId(group.groupId, row.path, row.workspaceId), row);
      }
    }
    return byKey;
  }, [projectGroups, worktreesByProject]);
  const agentsByPath = useMemo(() => agentsByWorktreePath(agents), [agents]);

  // The filter is session-only UI state: never persisted.
  const [filterQuery, setFilterQuery] = useState("");
  const { snapshot: supervisorSnapshot, now: supervisorNow } = useSupervisor();
  const live = useMemo(() => sidebarLiveState(model, {
    activityByWorktreePath,
    unreadWorktreePaths,
    agentStateByPath: Object.fromEntries([...agentsByPath].map(([path, agent]) => [path, agent.state])),
    activeWorkspaceId: activeProjectId,
    activePath,
    isSupervisorTyping: (path) => supervisorTypingIn(supervisorSnapshot, supervisorNow, path),
  }, filterQuery), [activePath, activeProjectId, activityByWorktreePath, agentsByPath, filterQuery, model, supervisorNow, supervisorSnapshot, unreadWorktreePaths]);
  const canCreateWorktree = (project: RegisteredProject) => project.gitRoot !== null && project.target?.kind !== "ssh";

  const projectChrome = (node: SidebarProjectNode) => {
    const project = projectById.get(node.primaryWorkspaceId);
    if (!project) return {};
    const remote = project.target && project.target.kind !== "local" ? project.target : null;
    const slot = live.projects[node.groupId] ?? { kind: "none" as const };
    const pairedHostId = remote?.kind === "pairedDaemon" ? remote.hostId : null;
    const onActivateSlot = slot.kind === "paired" && !slot.connected && pairedHostId && onOpenSettings
      ? () => onOpenSettings("remote", {
          page: "machines",
          filter: "all",
          machine: { kind: "pairedDaemon", hostId: pairedHostId, generation: remoteState.hosts[pairedHostId]?.generation ?? "" },
        })
      : undefined;
    const isLocal = !isRemoteWorkspaceId(project.workspaceId) && !isPairedWorkspaceId(project.workspaceId);
    const showMenu = (position: { x: number; y: number }) => {
      menuAbortRef.current?.abort();
      const controller = new AbortController();
      menuAbortRef.current = controller;
      openProjectMenu(project, position, {
        onCreateWorktree: () => onCreateWorktree(project),
        onManageDisk: onManageDisk ? () => onManageDisk(project) : undefined,
        onOpenHistory: onOpenHistory && isLocal ? () => onOpenHistory(project) : undefined,
        onRemoveProject: onRemoveProject ? () => onRemoveProject(project) : undefined,
      }, controller.signal);
    };
    return {
      title: remote ? `SSH: ${node.hostLabel} - ${project.repoRoot}` : project.repoRoot,
      addWorktreeLabel: `Add worktree to ${project.workspaceId}`,
      onAddWorktree: project.gitRoot !== null ? () => onCreateWorktree(project) : undefined,
      onMoreActions: (event: MouseEvent<HTMLButtonElement>) => {
        event.stopPropagation();
        const rect = event.currentTarget.getBoundingClientRect();
        showMenu({ x: rect.left, y: rect.bottom });
      },
      onContextMenu: (event: MouseEvent) => {
        event.preventDefault();
        event.stopPropagation();
        showMenu({ x: event.clientX, y: event.clientY });
      },
      onActivateSlot,
      slotAriaLabel: onActivateSlot ? `${slot.kind === "none" ? "" : slot.label}: open remote settings for ${node.hostLabel ?? node.label}` : undefined,
      onCreateWorktree: canCreateWorktree(project) ? () => onCreateWorktree(project) : undefined,
    };
  };

  const renderRow = (node: SidebarProjectNode, row: SidebarWorktreeNode, rowLive: SidebarRowLive) => {
    const project = projectById.get(node.primaryWorkspaceId);
    const group = groupById.get(node.groupId);
    if (!project || !group) return null;
    if (isStandaloneRemoteGroup(group)) {
      const root = projectRootWorktree(project, node.hostLabel ?? undefined);
      return (
        <div role="listitem">
          <SidebarWorktreeRow
            node={row}
            live={rowLive}
            title={`Remote worktree: ${project.repoRoot}${node.hostLabel ? ` (${node.hostLabel})` : ""}`}
            slotTitle={rowLive.slot ? undefined : node.hostLabel ?? undefined}
            shortcutWorkspaceId={project.workspaceId}
            onSelect={() => onSelectWorktree(root)}
          />
        </div>
      );
    }
    const worktree = worktreeByKey.get(row.key);
    if (!worktree) return null;
    return (
      <SortableWorktreeRow
        workspaceId={node.groupId}
        {...worktreeListRowProps(worktree, {
          repoRoot: project.repoRoot,
          activePath: rowLive.active ? activePath : "",
          activeWorkspaceId: activeProjectId,
          statuses,
          unreadWorktreePaths,
          activityByWorktreePath,
          onSelect: onSelectWorktree,
          onCreateWorktree: canCreateWorktree(project) ? () => onCreateWorktree(project) : undefined,
          onDelete: onDeleteWorktree,
          onResetAgentState,
          sortableWorkspaceId: node.groupId,
          agentsByPath,
        })}
        live={rowLive}
      />
    );
  };

  const reorder: SidebarReorder = {
    projects: (children) => (
      <SortableContext items={model.projects.map((node) => projectSortableId(node.groupId))} strategy={verticalListSortingStrategy}>
        {children}
      </SortableContext>
    ),
    section: (node, header, body) => (
      <SortableProjectSection workspaceId={node.groupId} header={header}>{body}</SortableProjectSection>
    ),
    rows: (_node, rows, children) => (
      <SortableContext items={rows.map((row) => row.key)} strategy={verticalListSortingStrategy}>{children}</SortableContext>
    ),
  };

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

  const handleDragStart = (event: DragStartEvent) => {
    const data = readSidebarDragData(event.active.data.current);
    if (data) setActiveDrag(data);
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const active = readSidebarDragData(event.active.data.current);
    const over = readSidebarDragData(event.over?.data.current);
    setActiveDrag(null);
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

    if (active.type !== "sidebar-worktree" || over.type !== "sidebar-worktree" || active.workspaceId !== over.workspaceId) {
      return;
    }
    const rows = worktreesByProject.get(active.workspaceId) ?? [];
    const paths = rows.map((worktree) => worktreeSortableId(active.workspaceId, worktree.path, worktree.workspaceId));
    const fromIndex = paths.indexOf(worktreeSortableId(active.workspaceId, active.worktreePath, active.rowWorkspaceId));
    const toIndex = paths.indexOf(worktreeSortableId(over.workspaceId, over.worktreePath, over.rowWorkspaceId));
    if (fromIndex < 0 || toIndex < 0 || fromIndex === toIndex) return;

    const nextOrder = pruneWorktreeOrder(
      { ...worktreeOrderRef.current, [active.workspaceId]: arrayMove(paths, fromIndex, toIndex) },
      naturalWorktreesByProject,
    );
    worktreeOrderRef.current = nextOrder;
    setWorktreeOrder(nextOrder);
    persistWorktreeOrder(nextOrder);
  };

  const activeProjectOverlay =
    activeDrag?.type === "sidebar-project" ? projectById.get(activeDrag.workspaceId) : undefined;
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

  const hide = onToggle ?? onHide;
  return (
    <DndContext sensors={sensors} onDragStart={handleDragStart} onDragEnd={handleDragEnd} onDragCancel={() => setActiveDrag(null)}>
      <aside
        className="relative flex h-full shrink-0 flex-col overflow-hidden bg-worktree-sidebar text-worktree-sidebar-foreground"
        style={{ width: `${width}px`, minWidth: `${MIN_SIDEBAR_WIDTH}px`, maxWidth: `${MAX_SIDEBAR_WIDTH}px` }}
      >
        <SidebarView
          model={model}
          live={live}
          activeWorkspaceId={activeProjectId}
          actions={{
            selectWorktree: (_node, row) => {
              const worktree = worktreeByKey.get(row.key);
              if (worktree) onSelectWorktree(worktree);
            },
            toggleGroup: toggleProject,
            selectProject: (node) => {
              const project = projectById.get(node.primaryWorkspaceId);
              if (project) onSelectProject(project);
            },
            addProject: onAddProject,
            hideSidebar: hide ?? noop,
            openSettings: () => onOpenSettings?.(),
            openResources: onOpenResources ?? noop,
            onViewChange: attention
              ? (view) => {
                  setSupervisorOpen(view === "supervisor");
                  if ((view === "inbox") !== inboxOpen) attention.onInboxOpenChange(view === "inbox");
                }
              : undefined,
          }}
          filter={{ query: filterQuery, onChange: setFilterQuery }}
          view={sidebarView}
          inbox={attention}
          supervisor={attention ? { typing: supervisorTyping, content: <SupervisorPanel /> } : undefined}
          titlebarLeading={isMac ? <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" /> : null}
          titlebarProps={{ "data-testid": "sidebar-drag-region", "data-tauri-drag-region": true }}
          shortcuts={{
            hideSidebar: hide ? "sidebar.left.toggle" : undefined,
            addProject: "project.add",
            settings: onOpenSettings ? "settings.toggle" : undefined,
            inbox: "notifications.toggle",
          }}
          projectChrome={projectChrome}
          renderRow={renderRow}
          reorder={reorder}
        />

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
              <SidebarProjectHeader
                label={projectDisplayLabel(activeProjectOverlay, projectHostLabel(activeProjectOverlay, { hosts, remoteState }), true)}
                active={activeProjectOverlay.workspaceId === activeProjectId}
                expanded={!collapsedProjects.has(activeProjectOverlay.workspaceId)}
                slot={live.projects[activeProjectOverlay.workspaceId] ?? { kind: "none" }}
                onAddWorktree={activeProjectOverlay.gitRoot !== null ? noop : undefined}
                addWorktreeLabel={`Add worktree to ${activeProjectOverlay.workspaceId}`}
                onMoreActions={noop}
                inert
              />
            </SidebarDragRow>
          </div>
        ) : activeWorktreeOverlay && activeDrag?.type === "sidebar-worktree" ? (
          <div data-testid="sidebar-drag-overlay" className="w-sidebar max-w-[calc(100vw-1rem)]">
            <SidebarDragRow kind="worktree" overlay>
              <WorktreeRow
                worktree={activeWorktreeOverlay}
                repoRoot={projectById.get(activeDrag.workspaceId)?.repoRoot}
                active={live.rows[worktreeSortableId(activeDrag.workspaceId, activeWorktreeOverlay.path, activeWorktreeOverlay.workspaceId)]?.active ?? false}
                agent={agents.find((agent) => agent.worktreePath === activeWorktreeOverlay.path)}
                status={statuses[activeWorktreeOverlay.path]}
                unread={Boolean(unreadWorktreePaths?.[activeWorktreeOverlay.path])}
                activitySummary={activityByWorktreePath?.[activeWorktreeOverlay.path]}
                onSelect={noop}
                onDelete={noop}
              />
            </SidebarDragRow>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
