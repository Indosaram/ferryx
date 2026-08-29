import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type CollisionDetection,
  type DragCancelEvent,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
  type UniqueIdentifier,
} from "@dnd-kit/core";
import { sortableKeyboardCoordinates } from "@dnd-kit/sortable";
import { Columns2, Rows2, X } from "lucide-react";
import React, { useEffect, useRef, useState } from "react";

import type { ActivitySummary } from "../lib/activity";
import type {
  BrowserTab,
  LayoutState,
  PaneContent,
  TabGroup,
  TabGroupLayoutNode,
  TabPaneLayout,
  TerminalSession,
  WorkspaceTab,
} from "../lib/types";
import { defaultContentForTab, getTabPaneLayout, normalizeLayout, toPaneContent } from "../state/layout";
import { isRedundantSplit as isRedundantPaneSplit, type PaneDirection, type PaneNode } from "../state/paneTree";
import { BrowserPane } from "./BrowserPane";
import { DagGraphView } from "./dag/DagGraphView";
import { TabBar } from "./TabBar";
import { NativeTerminalVisibilityProvider } from "../lib/nativeTerminalVisibility";
import { PaneEdgeDropZones } from "./tab-dnd/PaneEdgeDropZones";
import { resolveSplitEdgeForPoint } from "./tab-dnd/SplitEdgeDropZone";
import { TabGroupDropSurface } from "./tab-dnd/TabGroupDropSurface";
import {
  dropPriority,
  edgeToSplit,
  isWorkspaceDragData,
  isWorkspaceDropData,
  resolveWorkspaceDropCommand,
  type RedundantSplitCheck,
  type TabDropEdge,
  type WorkspaceDragData,
  type WorkspaceDropData,
} from "./tab-dnd/tabDragTypes";
import { TerminalPane } from "./TerminalPane";
import { IconButton } from "./ui/IconButton";

const MIN_PANE_SIZE_PX = 80;

export const FALLBACK_SESSION: TerminalSession = {
  id: "",
  cwd: "",
  worktreePath: "",
  workspaceId: "",
  worktree: null,
  backendSessionId: null,
  lifecycle: "working",
};

export const workspaceCollisionDetection: CollisionDetection = (args) => createWorkspaceCollisionDetection()(args);

/**
 * `isRedundantSplit` drops targets whose drop would rebuild the current tree, so the
 * edge preview never advertises a split that cannot happen.
 */
export function createWorkspaceCollisionDetection(
  isRedundantSplit?: RedundantSplitCheck,
): CollisionDetection {
  return (args) => {
    const activeData = args.active.data.current;
    if (!isWorkspaceDragData(activeData)) return pointerWithin(args);
    const containerDataById = new Map<UniqueIdentifier, unknown>();
    for (const container of args.droppableContainers) {
      containerDataById.set(container.id, container.data.current);
    }
    const pointer = args.pointerCoordinates;
    return pointerWithin(args)
      .filter((collision) => {
        const data = containerDataById.get(collision.id);
        if (!isWorkspaceDropData(data) || dropPriority(activeData, data) >= 100) return false;
        // Edge zones span their whole surface, so keep only the edge owning the pointer's
        // side of the center lines; without this every edge would collide at once.
        if (data.type === "pane-edge" || data.type === "group-edge") {
          const rect = args.droppableRects.get(collision.id);
          if (!pointer || !rect) return false;
          if (resolveSplitEdgeForPoint(pointer, rect) !== data.edge) return false;
        }
        return resolveWorkspaceDropCommand(activeData, data, isRedundantSplit) !== null;
      })
      .sort((left, right) => {
        const leftData = containerDataById.get(left.id);
        const rightData = containerDataById.get(right.id);
        if (!isWorkspaceDropData(leftData) || !isWorkspaceDropData(rightData)) return 0;
        return dropPriority(activeData, leftData) - dropPriority(activeData, rightData);
      });
  };
}

type SplitPaneOptions = { position?: "first" | "second" };

type TerminalSplitViewProps = {
  layout: LayoutState;
  sessions: Record<string, TerminalSession>;
  onActivateTab?: (tabId: string) => void;
  onCloseTab?: (tabId: string) => void;
  onCloseOtherTabs?: (tabId: string) => void;
  onCloseTabsToRight?: (tabId: string) => void;
  onCloseTabsToLeft?: (tabId: string) => void;
  /** Legacy same-group API; dnd-kit uses onMoveTabToGroup instead. */
  onReorderTab?: (tabId: string, targetIndex: number) => void;
  onMoveTabToGroup?: (tabId: string, targetGroupId: string, targetIndex?: number) => void;
  onMoveTabToSplit?: (
    tabId: string,
    targetGroupId: string,
    direction: PaneDirection,
    position?: "first" | "second",
    targetPane?: { tabId: string; leafId: string },
  ) => void;
  onDetachPaneToTab?: (
    sourceTabId: string,
    leafId: string,
    targetGroupId?: string,
    targetIndex?: number,
  ) => string | null | void;
  onRenameTab?: (tabId: string, label: string) => void;
  onToggleTabPin?: (tabId: string, pinned: boolean) => void;
  onAddTab?: () => void;
  onAddBrowserTab?: (url?: string, profileId?: string) => void;
  onDuplicateBrowserTab?: (tabId: string, profileId?: string) => void;
  onAddMarkdown?: () => void;
  onAddMobileEmulator?: () => void;
  onOpenSettings?: () => void;
  agents?: Array<{ name: string; command: string; args: string }>;
  onLaunchAgent?: (agent: { name: string; command: string; args: string }) => void;
  defaultAgentId?: string | null;
  onNavigateBrowserTab?: (tabId: string, url: string) => void;
  onReloadBrowserTab?: (tabId: string) => void;
  onSplitPane?: (tabId: string, leafId: string, direction: PaneDirection, options?: SplitPaneOptions) => void;
  onClosePane?: (tabId: string, leafId: string) => void;
  onSetRatio?: (tabId: string, path: string, ratio: number) => void;
  onSetGroupRatio?: (path: string, ratio: number) => void;
  onSwapPanes?: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane?: (tabId: string, leafId: string) => void;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
  leadingSpacer?: number;
  searchLeafId?: string | null;
  onCloseSearch?: () => void;
};

type DragFocusSnapshot = {
  activeTabId: string | null;
  focusedGroupId: string | null;
};

export function TerminalSplitView({
  layout,
  sessions,
  onActivateTab = () => undefined,
  onCloseTab = () => undefined,
  onCloseOtherTabs,
  onCloseTabsToRight,
  onCloseTabsToLeft,
  onReorderTab,
  onMoveTabToGroup,
  onMoveTabToSplit,
  onDetachPaneToTab,
  onRenameTab,
  onToggleTabPin,
  onAddTab = () => undefined,
  onAddBrowserTab = () => undefined,
  onDuplicateBrowserTab = () => undefined,
  onAddMarkdown,
  onAddMobileEmulator,
  onOpenSettings,
  agents,
  onLaunchAgent,
  defaultAgentId,
  onNavigateBrowserTab = () => undefined,
  onReloadBrowserTab = () => undefined,
  onSplitPane = () => undefined,
  onClosePane = () => undefined,
  onSetRatio = () => undefined,
  onSetGroupRatio = () => undefined,
  onSwapPanes = () => undefined,
  onFocusPane = () => undefined,
  unreadTabIds,
  activityByTabId,
  leadingSpacer = 0,
  searchLeafId,
  onCloseSearch,
}: TerminalSplitViewProps) {
  const normalizedLayout = normalizeLayout(layout);
  const groups = normalizedLayout.tabGroups ?? {};
  const groupLayout = normalizedLayout.tabGroupLayout ?? null;
  const firstGroupId = firstTabGroupId(groupLayout);
  const dragSnapshotRef = useRef<DragFocusSnapshot | null>(null);
  const activeDragRef = useRef<WorkspaceDragData | null>(null);
  const [activeDrag, setActiveDrag] = useState<WorkspaceDragData | null>(null);
  const [dropFeedbackLeafId, setDropFeedbackLeafId] = useState<string | null>(null);
  const previewedTabIdRef = useRef<string | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 12 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const isRedundantSplit: RedundantSplitCheck = ({ tabId, sourceLeafId, targetLeafId, direction, position }) => {
    const tab = normalizedLayout.tabs.find((candidate) => candidate.id === tabId);
    if (!tab) return false;
    const root = getTabPaneLayout(normalizedLayout, tab).root;
    return isRedundantPaneSplit(root, sourceLeafId, targetLeafId, direction, position);
  };
  const collisionDetection = React.useMemo(
    () => createWorkspaceCollisionDetection(isRedundantSplit),
    [normalizedLayout],
  );

  const splitTerminalTab = (tabId: string, direction: PaneDirection) => {
    const tab = normalizedLayout.tabs.find((candidate) => candidate.id === tabId);
    if (!tab || tab.kind === "browser") return;
    const tabLayout = getTabPaneLayout(normalizedLayout, tab);
    const targetLeafId = tabLayout.activeLeafId ?? firstLeafId(tabLayout.root);
    onSplitPane(tab.id, targetLeafId, direction);
  };

  const restoreDragFocus = () => {
    const snapshot = dragSnapshotRef.current;
    if (snapshot?.activeTabId) onActivateTab(snapshot.activeTabId);
  };

  const clearDragState = () => {
    activeDragRef.current = null;
    setActiveDrag(null);
    setDropFeedbackLeafId(null);
    dragSnapshotRef.current = null;
    previewedTabIdRef.current = null;
  };

  const handleDragStart = (event: DragStartEvent) => {
    const data = event.active.data.current;
    if (!isWorkspaceDragData(data)) return;
    activeDragRef.current = data;
    setActiveDrag(data);
    dragSnapshotRef.current = {
      activeTabId: normalizedLayout.activeTabId,
      focusedGroupId: normalizedLayout.focusedGroupId ?? null,
    };
    previewedTabIdRef.current = null;
  };

  const handleDragOver = (event: DragOverEvent) => {
    const active = activeDragRef.current;
    const overData = event.over?.data.current;

    // Drop feedback is painted by the hovered pane only, and a native terminal surface
    // would cover it, so exactly that one pane yields its surface while it is targeted.
    setDropFeedbackLeafId(
      isWorkspaceDropData(overData) && (overData.type === "pane-edge" || overData.type === "pane-leaf")
        ? overData.leafId
        : null,
    );

    if (!active || active.type !== "tab" || !isWorkspaceDropData(overData)) return;

    let previewTabId: string | null = null;
    if (overData.type === "tab" || overData.type === "pane-edge" || overData.type === "pane-leaf") {
      previewTabId = overData.tabId;
    }
    if (overData.type === "group-body" || overData.type === "group-edge") {
      previewTabId = groups[overData.groupId]?.activeTabId ?? groups[overData.groupId]?.tabIds[0] ?? null;
    }
    if (
      !previewTabId ||
      previewTabId === active.tabId ||
      previewTabId === normalizedLayout.activeTabId ||
      previewedTabIdRef.current === previewTabId
    )
      return;
    previewedTabIdRef.current = previewTabId;
    onActivateTab(previewTabId);
  };

  const executeDrop = (active: WorkspaceDragData, over: WorkspaceDropData | null) => {
    const command = resolveWorkspaceDropCommand(active, over, isRedundantSplit);
    if (!command) return false;

    switch (command.type) {
      case "move-tab-to-group": {
        if (onMoveTabToGroup) {
          onMoveTabToGroup(command.tabId, command.targetGroupId, command.targetIndex);
          return true;
        }
        if (active.type === "tab" && command.targetGroupId === active.groupId && command.targetIndex !== undefined) {
          onReorderTab?.(command.tabId, command.targetIndex);
          return Boolean(onReorderTab);
        }
        return false;
      }
      case "move-tab-to-split":
        onMoveTabToSplit?.(command.tabId, command.targetGroupId, command.direction, command.position);
        return Boolean(onMoveTabToSplit);
      case "move-tab-to-pane-split": {
        const targetGroupId =
          Object.values(groups).find((g) => g.tabIds.includes(command.targetTabId))?.id ??
          firstGroupId ??
          "";
        if (command.sourceTabId === command.targetTabId) {
          onSplitPane(command.targetTabId, command.targetLeafId, command.direction, {
            position: command.position,
          });
          return true;
        }
        onMoveTabToSplit?.(
          command.sourceTabId,
          targetGroupId,
          command.direction,
          command.position,
          { tabId: command.targetTabId, leafId: command.targetLeafId },
        );
        return Boolean(onMoveTabToSplit);
      }
      case "move-pane-to-pane-split": {
        const targetGroupId =
          Object.values(groups).find((g) => g.tabIds.includes(command.targetTabId))?.id ??
          firstGroupId ??
          "";
        const sourceLayout = normalizedLayout.layoutsByTabId?.[command.sourceTabId];
        const isSinglePane = !sourceLayout || sourceLayout.root.type === "leaf";
        if (isSinglePane) {
          onMoveTabToSplit?.(
            command.sourceTabId,
            targetGroupId,
            command.direction,
            command.position,
            { tabId: command.targetTabId, leafId: command.targetLeafId },
          );
          return Boolean(onMoveTabToSplit);
        }
        const sourceGroupId =
          Object.values(groups).find((g) => g.tabIds.includes(command.sourceTabId))?.id ??
          firstGroupId ??
          "";
        const detachedTabId = onDetachPaneToTab?.(command.sourceTabId, command.sourceLeafId, sourceGroupId);
        if (typeof detachedTabId !== "string" || !detachedTabId) return false;
        onMoveTabToSplit?.(
          detachedTabId,
          targetGroupId,
          command.direction,
          command.position,
          { tabId: command.targetTabId, leafId: command.targetLeafId },
        );
        return true;
      }
      case "detach-pane-to-tab":
        onDetachPaneToTab?.(command.sourceTabId, command.leafId, command.targetGroupId, command.targetIndex);
        return Boolean(onDetachPaneToTab);
      case "swap-panes":
        onSwapPanes(command.tabId, command.sourceLeafId, command.targetLeafId);
        return true;
    }
  };

  const handleDragEnd = (event: DragEndEvent) => {
    const active = activeDragRef.current;
    const overData = event.over?.data.current;
    const handled = active && isWorkspaceDropData(overData) ? executeDrop(active, overData) : false;
    if (!handled) restoreDragFocus();
    clearDragState();
  };

  const handleDragCancel = (_event: DragCancelEvent) => {
    restoreDragFocus();
    clearDragState();
  };

  const sharedGroupProps: Omit<TabGroupViewProps, "groupId" | "leadingSpacer"> = {
    layout: normalizedLayout,
    groups,
    sessions,
    onActivateTab,
    onCloseTab,
    onCloseOtherTabs,
    onCloseTabsToRight,
    onCloseTabsToLeft,
    onRenameTab,
    onToggleTabPin,
    onAddTab,
    onAddBrowserTab,
    onDuplicateBrowserTab,
    onAddMarkdown,
    onAddMobileEmulator,
    onOpenSettings,
    agents,
    onLaunchAgent,
    defaultAgentId,
    onNavigateBrowserTab,
    onReloadBrowserTab,
    onSplitPane,
    onMoveTabToSplit,
    onClosePane,
    onSetRatio,
    onSwapPanes,
    onFocusPane,
    unreadTabIds,
    activityByTabId,
    searchLeafId,
    onCloseSearch,
    splitTerminalTab,
    browserPanesVisible: activeDrag === null,
    dropFeedbackLeafId,
  };

  const overlayLabel =
    activeDrag?.type === "tab"
      ? normalizedLayout.tabs.find((tab) => tab.id === activeDrag.tabId)?.label ?? "Tab"
      : activeDrag?.type === "pane"
        ? "Terminal pane"
        : null;

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={collisionDetection}
      onDragStart={handleDragStart}
      onDragOver={handleDragOver}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <div className="relative flex flex-1 flex-col overflow-hidden bg-terminal" data-testid="terminal-layout">
        {groupLayout ? (
          <TabGroupLayoutRenderer
            node={groupLayout}
            path=""
            firstGroupId={firstGroupId}
            leadingSpacer={leadingSpacer}
            onSetGroupRatio={onSetGroupRatio}
            groupProps={sharedGroupProps}
          />
        ) : (
          <TabBar
            tabs={[]}
            activeTabId=""
            onActivate={onActivateTab}
            onClose={onCloseTab}
            onAdd={onAddTab}
            onAddBrowser={onAddBrowserTab}
            onDuplicateBrowser={onDuplicateBrowserTab}
            onAddMarkdown={onAddMarkdown}
            onAddMobileEmulator={onAddMobileEmulator}
            onOpenSettings={onOpenSettings}
            agents={agents}
            onLaunchAgent={onLaunchAgent}
            defaultAgentId={defaultAgentId}
            leadingSpacer={leadingSpacer}
          />
        )}
      </div>
      <DragOverlay dropAnimation={null}>
        {overlayLabel ? (
          <div data-testid="workspace-drag-overlay" className="rounded border border-border bg-card px-3 py-1.5 text-xs text-foreground shadow-lg">
            {overlayLabel}
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

type TabGroupLayoutRendererProps = {
  node: TabGroupLayoutNode;
  path: string;
  firstGroupId: string | null;
  leadingSpacer: number;
  onSetGroupRatio: (path: string, ratio: number) => void;
  groupProps: Omit<TabGroupViewProps, "groupId" | "leadingSpacer">;
};

function TabGroupLayoutRenderer({ node, path, firstGroupId, leadingSpacer, onSetGroupRatio, groupProps }: TabGroupLayoutRendererProps) {
  if (node.type === "group") {
    return (
      <TabGroupView
        {...groupProps}
        groupId={node.groupId}
        leadingSpacer={node.groupId === firstGroupId ? leadingSpacer : 0}
      />
    );
  }

  const isHorizontal = node.direction === "horizontal";
  const ratio = node.ratio ?? 0.5;
  return (
    <div
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden ${isHorizontal ? "flex-row" : "flex-col"}`}
      data-testid="tab-group-split"
      data-direction={node.direction}
    >
      <div className="relative min-h-0 min-w-0 overflow-hidden" style={{ flexBasis: `${ratio * 100}%`, flexGrow: 0, flexShrink: 0 }}>
        <TabGroupLayoutRenderer
          node={node.first}
          path={path ? `${path}.first` : "first"}
          firstGroupId={firstGroupId}
          leadingSpacer={leadingSpacer}
          onSetGroupRatio={onSetGroupRatio}
          groupProps={groupProps}
        />
      </div>
      <PaneResizeDivider
        direction={node.direction}
        ratio={ratio}
        ariaLabel="Resize tab groups"
        onRatioChange={(newRatio) => onSetGroupRatio(path, newRatio)}
      />
      <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden" style={{ flexBasis: `${(1 - ratio) * 100}%`, flexGrow: 1, flexShrink: 1 }}>
        <TabGroupLayoutRenderer
          node={node.second}
          path={path ? `${path}.second` : "second"}
          firstGroupId={firstGroupId}
          leadingSpacer={leadingSpacer}
          onSetGroupRatio={onSetGroupRatio}
          groupProps={groupProps}
        />
      </div>
    </div>
  );
}

type TabGroupViewProps = {
  groupId: string;
  layout: LayoutState;
  groups: Record<string, TabGroup>;
  sessions: Record<string, TerminalSession>;
  leadingSpacer: number;
  onActivateTab: (tabId: string) => void;
  onCloseTab: (tabId: string) => void;
  onCloseOtherTabs?: (tabId: string) => void;
  onCloseTabsToRight?: (tabId: string) => void;
  onCloseTabsToLeft?: (tabId: string) => void;
  onRenameTab?: (tabId: string, label: string) => void;
  onToggleTabPin?: (tabId: string, pinned: boolean) => void;
  onAddTab: () => void;
  onAddBrowserTab: (url?: string, profileId?: string) => void;
  onDuplicateBrowserTab: (tabId: string, profileId?: string) => void;
  onAddMarkdown?: () => void;
  onAddMobileEmulator?: () => void;
  onOpenSettings?: () => void;
  agents?: Array<{ name: string; command: string; args: string }>;
  onLaunchAgent?: (agent: { name: string; command: string; args: string }) => void;
  defaultAgentId?: string | null;
  onNavigateBrowserTab: (tabId: string, url: string) => void;
  onReloadBrowserTab: (tabId: string) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection, options?: SplitPaneOptions) => void;
  onMoveTabToSplit?: (
    tabId: string,
    targetGroupId: string,
    direction: PaneDirection,
    position?: "first" | "second",
    targetPane?: { tabId: string; leafId: string },
  ) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSetRatio: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
  unreadTabIds?: Record<string, boolean>;
  activityByTabId?: Record<string, ActivitySummary | undefined>;
  searchLeafId?: string | null;
  onCloseSearch?: () => void;
  splitTerminalTab: (tabId: string, direction: PaneDirection) => void;
  browserPanesVisible: boolean;
  dropFeedbackLeafId: string | null;
};

function TabGroupView({
  groupId,
  layout,
  groups,
  sessions,
  leadingSpacer,
  onActivateTab,
  onCloseTab,
  onCloseOtherTabs,
  onCloseTabsToRight,
  onCloseTabsToLeft,
  onRenameTab,
  onToggleTabPin,
  onAddTab,
  onAddBrowserTab,
  onAddMarkdown,
  onDuplicateBrowserTab,
  onAddMobileEmulator,
  onOpenSettings,
  agents,
  onLaunchAgent,
  defaultAgentId,
  onNavigateBrowserTab,
  onReloadBrowserTab,
  onSplitPane,
  onMoveTabToSplit,
  onClosePane,
  onSetRatio,
  onSwapPanes,
  onFocusPane,
  unreadTabIds,
  activityByTabId,
  searchLeafId,
  onCloseSearch,
  splitTerminalTab,
  browserPanesVisible,
  dropFeedbackLeafId,
}: TabGroupViewProps) {
  const group = groups[groupId];
  if (!group) return null;
  const tabById = new Map(layout.tabs.map((tab) => [tab.id, tab]));
  const tabs = group.tabIds.map((tabId) => tabById.get(tabId)).filter((tab): tab is WorkspaceTab => Boolean(tab));
  const activeTab = tabs.find((tab) => tab.id === group.activeTabId) ?? tabs[0] ?? null;
  const activeTabLayout = activeTab ? getTabPaneLayout(layout, activeTab) : null;
  const isFocused = layout.focusedGroupId === groupId;

  const focusGroup = () => {
    if (activeTab) onActivateTab(activeTab.id);
  };

  const moveTabToSplitEdge = (tabId: string, edge: TabDropEdge) => {
    const { direction, position } = edgeToSplit(edge);
    onMoveTabToSplit?.(tabId, groupId, direction, position);
  };

  return (
    <div
      className={`relative flex h-full w-full min-h-0 min-w-0 flex-col overflow-hidden bg-terminal ${isFocused ? "" : "opacity-95"}`}
      data-testid="tab-group-panel"
      data-tab-group-panel-id={groupId}
      onPointerDown={focusGroup}
      onFocusCapture={focusGroup}
    >
      <TabBar
        groupId={groupId}
        tabs={tabs}
        activeTabId={activeTab?.id ?? ""}
        unreadTabIds={unreadTabIds}
        activityByTabId={activityByTabId}
        onActivate={onActivateTab}
        onClose={onCloseTab}
        onCloseOthers={onCloseOtherTabs}
        onCloseToRight={onCloseTabsToRight}
        onCloseToLeft={onCloseTabsToLeft}
        onRenameTab={onRenameTab}
        onTogglePin={onToggleTabPin}
        onSplitRight={(tabId) => splitTerminalTab(tabId, "horizontal")}
        onSplitDown={(tabId) => splitTerminalTab(tabId, "vertical")}
        onMoveTabToSplit={moveTabToSplitEdge}
        onAdd={() => {
          focusGroup();
          onAddTab();
        }}
        onAddBrowser={(url, profileId) => {
          focusGroup();
          onAddBrowserTab(url, profileId);
        }}
        onDuplicateBrowser={onDuplicateBrowserTab}
        onAddMarkdown={
          onAddMarkdown
            ? () => {
                focusGroup();
                onAddMarkdown();
              }
            : undefined
        }
        onAddMobileEmulator={
          onAddMobileEmulator
            ? () => {
                focusGroup();
                onAddMobileEmulator();
              }
            : undefined
        }
        onOpenSettings={onOpenSettings}
        agents={agents}
        defaultAgentId={defaultAgentId}
        onLaunchAgent={
          onLaunchAgent
            ? (agent) => {
                focusGroup();
                onLaunchAgent(agent);
              }
            : undefined
        }
        leadingSpacer={leadingSpacer}
      />

      <TabGroupDropSurface groupId={groupId}>
        {!activeTab ? null : activeTabLayout ? (
          <PaneRenderer
            node={activeTabLayout.root}
            tab={activeTab}
            tabLayout={activeTabLayout}
            sessions={sessions}
            groupFocused={isFocused}
            browserPanesVisible={browserPanesVisible}
            dropFeedbackLeafId={dropFeedbackLeafId}
            onNavigateBrowserTab={onNavigateBrowserTab}
            onReloadBrowserTab={onReloadBrowserTab}
            path=""
            searchLeafId={searchLeafId}
            onCloseSearch={onCloseSearch}
            onSplitPane={onSplitPane}
            onClosePane={onClosePane}
            onSetRatio={onSetRatio}
            onSwapPanes={onSwapPanes}
            onFocusPane={onFocusPane}
          />
        ) : null}
      </TabGroupDropSurface>
    </div>
  );
}

function firstLeafId(node: PaneNode): string {
  let current = node;
  while (current.type === "split") current = current.first;
  return current.leafId;
}

function firstTabGroupId(node: TabGroupLayoutNode | null): string | null {
  if (!node) return null;
  let current = node;
  while (current.type === "split") current = current.first;
  return current.groupId;
}

type PaneRendererProps = {
  node: PaneNode;
  tab: WorkspaceTab;
  tabLayout: TabPaneLayout;
  sessions: Record<string, TerminalSession>;
  groupFocused: boolean;
  browserPanesVisible: boolean;
  dropFeedbackLeafId: string | null;
  path: string;
  searchLeafId?: string | null;
  onCloseSearch?: () => void;
  onNavigateBrowserTab: (tabId: string, url: string) => void;
  onReloadBrowserTab: (tabId: string) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection, options?: SplitPaneOptions) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onSetRatio: (tabId: string, path: string, ratio: number) => void;
  onSwapPanes: (tabId: string, sourceLeafId: string, targetLeafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
};

const PaneRenderer = React.memo(function PaneRenderer(props: PaneRendererProps) {
  const { node, tab, tabLayout, sessions, path, browserPanesVisible, dropFeedbackLeafId, onNavigateBrowserTab, onReloadBrowserTab } = props;
  if (node.type === "leaf") {
    const rawContent = tabLayout.contentsByLeafId?.[node.leafId];
    const defaultSessionId = tab.kind === "browser" ? "" : tab.sessionId;
    const sessionId = tabLayout.sessionIdsByLeafId[node.leafId] ?? defaultSessionId;
    const content = rawContent
      ? toPaneContent(rawContent, sessionId)
      : defaultContentForTab(tab, sessionId);
    const session = sessions[sessionId] ?? FALLBACK_SESSION;
    return (
      <PaneLeafView
        leafId={node.leafId}
        tab={tab}
        content={content}
        session={session}
        isOnlyLeaf={tabLayout.root.type === "leaf"}
        isActive={props.groupFocused && tabLayout.activeLeafId === node.leafId}
        browserPanesVisible={browserPanesVisible}
        dropFeedbackLeafId={dropFeedbackLeafId}
        searchOpen={props.searchLeafId === node.leafId}
        onCloseSearch={props.onCloseSearch}
        onNavigateBrowserTab={onNavigateBrowserTab}
        onReloadBrowserTab={onReloadBrowserTab}
        onSplitPane={props.onSplitPane}
        onClosePane={props.onClosePane}
        onFocusPane={props.onFocusPane}
      />
    );
  }

  const isHorizontal = node.direction === "horizontal";
  const ratio = node.ratio ?? 0.5;
  return (
    <div
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden ${isHorizontal ? "flex-row" : "flex-col"}`}
      data-testid="pane-split"
      data-direction={node.direction}
    >
      <div className="relative min-h-0 min-w-0 overflow-hidden" style={{ flexBasis: `${ratio * 100}%`, flexGrow: 0, flexShrink: 0 }}>
        <PaneRenderer {...props} node={node.first} path={path ? `${path}.first` : "first"} />
      </div>
      <PaneResizeDivider
        direction={node.direction}
        ratio={ratio}
        ariaLabel="Resize terminal panes"
        onRatioChange={(newRatio) => props.onSetRatio(tab.id, path, newRatio)}
      />
      <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden" style={{ flexBasis: `${(1 - ratio) * 100}%`, flexGrow: 1, flexShrink: 1 }}>
        <PaneRenderer {...props} node={node.second} path={path ? `${path}.second` : "second"} />
      </div>
    </div>
  );
});

type PaneLeafViewProps = {
  leafId: string;
  tab: WorkspaceTab;
  content: PaneContent;
  session: TerminalSession;
  isOnlyLeaf: boolean;
  isActive: boolean;
  browserPanesVisible: boolean;
  dropFeedbackLeafId: string | null;
  searchOpen?: boolean;
  onCloseSearch?: () => void;
  onNavigateBrowserTab: (tabId: string, url: string) => void;
  onReloadBrowserTab: (tabId: string) => void;
  onSplitPane: (tabId: string, leafId: string, direction: PaneDirection, options?: SplitPaneOptions) => void;
  onClosePane: (tabId: string, leafId: string) => void;
  onFocusPane: (tabId: string, leafId: string) => void;
};

const PaneLeafView = React.memo(function PaneLeafView({
  leafId,
  tab,
  content,
  session,
  isOnlyLeaf,
  isActive,
  browserPanesVisible,
  dropFeedbackLeafId,
  searchOpen,
  onCloseSearch,
  onNavigateBrowserTab,
  onReloadBrowserTab,
  onSplitPane,
  onClosePane,
  onFocusPane,
}: PaneLeafViewProps) {
  const [isHoveredTop, setIsHoveredTop] = React.useState(false);
  const droppable = useDroppable({
    id: `pane-leaf:${tab.id}:${leafId}`,
    data: { type: "pane-leaf", tabId: tab.id, leafId },
  });
  const draggable = useDraggable({
    id: `pane:${tab.id}:${leafId}`,
    disabled: false,
    data: {
      type: "pane",
      tabId: tab.id,
      sourcePaneId: leafId,
      sourceSessionId: content.kind === "terminal" ? session.id : "",
    },
  });

  // Native compositor child views paint above WKWebView on macOS, so DOM drop feedback
  // cannot cover a live terminal surface. Only the targeted pane paints feedback, so only
  // it yields its surface -- every other terminal keeps rendering during the drag.
  const showsDropFeedback = dropFeedbackLeafId === leafId;

  return (
    <NativeTerminalVisibilityProvider visible={!showsDropFeedback}>
    <div
      ref={droppable.setNodeRef}
      className={`relative flex h-full w-full min-h-0 min-w-0 overflow-hidden bg-terminal ${
        droppable.isOver ? "ring-2 ring-primary/80 ring-inset" : ""
      }`}
      data-testid="pane-leaf"
      data-leaf-id={leafId}
      data-tab-id={tab.id}
      data-dnd-type="pane-leaf"
      onClick={() => onFocusPane(tab.id, leafId)}
      onMouseMove={(event) => {
        const rect = event.currentTarget.getBoundingClientRect();
        const relativeY = event.clientY - rect.top;
        if (relativeY <= 16) {
          if (!isHoveredTop) setIsHoveredTop(true);
        } else {
          if (isHoveredTop) setIsHoveredTop(false);
        }
      }}
      onMouseLeave={() => {
        setIsHoveredTop(false);
      }}
    >
      <PaneEdgeDropZones tabId={tab.id} leafId={leafId} />
      <div
        className="absolute inset-x-0 top-0 z-20 h-4 pointer-events-none"
        data-testid="pane-toolbar-hotspot"
        onMouseEnter={() => setIsHoveredTop(true)}
        onMouseLeave={(event) => {
          const related = event.relatedTarget;
          if (!(related instanceof Element) || !related.closest('[data-testid="pane-toolbar"]')) setIsHoveredTop(false);
        }}
      />

      <div
        ref={draggable.setNodeRef}
        {...draggable.attributes}
        {...draggable.listeners}
        className={`absolute inset-x-0 top-0 z-30 flex h-3 items-center justify-end overflow-visible border-b border-border/30 bg-background/85 backdrop-blur-md px-2 text-[11px] text-muted-foreground transition-opacity duration-150 select-none cursor-grab touch-none active:cursor-grabbing ${
          isHoveredTop ? "pointer-events-auto opacity-100" : "pointer-events-none opacity-0"
        } ${draggable.isDragging ? "opacity-30" : ""}`}
        data-testid="pane-toolbar"
        data-dnd-type="pane"
        onMouseEnter={() => setIsHoveredTop(true)}
        onMouseLeave={() => setIsHoveredTop(false)}
      >
        <div className="flex items-center gap-0.5">
          <IconButton
            label="Split pane right"
            size="sm"
            className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => {
              event.stopPropagation();
              onSplitPane(tab.id, leafId, "horizontal");
            }}
          >
            <Columns2 className="size-3" />
          </IconButton>
          <IconButton
            label="Split pane down"
            size="sm"
            className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => {
              event.stopPropagation();
              onSplitPane(tab.id, leafId, "vertical");
            }}
          >
            <Rows2 className="size-3" />
          </IconButton>
          {!isOnlyLeaf ? (
            <IconButton
              label="Close split view"
              size="sm"
              className="size-5 rounded p-0 text-muted-foreground/70 hover:bg-accent/60 hover:text-foreground"
              onPointerDown={(event) => event.stopPropagation()}
              onClick={(event) => {
                event.stopPropagation();
                onClosePane(tab.id, leafId);
              }}
            >
              <X className="size-3" />
            </IconButton>
          ) : null}
        </div>
      </div>

      <div className="h-full w-full min-h-0 flex-1 overflow-hidden">
        {(() => {
          switch (content.kind) {
            case "terminal":
              return (
                <TerminalPane
                  session={session}
                  active={isActive}
                  searchOpen={searchOpen}
                  onCloseSearch={onCloseSearch}
                />
              );
            case "browser": {
              const browserState = content.browser ?? {
                browserId: content.browserId ?? "",
                url: content.url ?? "about:blank",
                title: content.title ?? null,
                loading: content.loading ?? false,
                canGoBack: content.canGoBack ?? false,
                canGoForward: content.canGoForward ?? false,
                profileId: content.profileId,
                zoomFactor: content.zoomFactor,
                loadError: content.loadError ?? null,
                worktreePath: content.worktreePath,
                worktreeLabel: content.worktreeLabel,
              };
              const browserTab: BrowserTab = {
                id: tab.id,
                kind: "browser",
                label: (tab.kind === "browser" ? tab.label : browserState.title) || tab.label,
                browserId: browserState.browserId ?? "",
                url: browserState.url ?? "",
                title: browserState.title ?? null,
                loading: browserState.loading ?? false,
                canGoBack: browserState.canGoBack ?? false,
                canGoForward: browserState.canGoForward ?? false,
                profileId: browserState.profileId,
                zoomFactor: browserState.zoomFactor,
                loadError: browserState.loadError ?? null,
                worktreePath: browserState.worktreePath,
                worktreeLabel: browserState.worktreeLabel,
                pinned: tab.pinned,
              };
              return (
                <BrowserPane
                  tab={browserTab}
                  visible={browserPanesVisible}
                  onNavigate={(url) => onNavigateBrowserTab(tab.id, url)}
                  onReload={() => onReloadBrowserTab(tab.id)}
                />
              );
            }
            case "dag": {
              return <DagGraphView runId={content.runId ?? undefined} />;
            }
          }
        })()}
      </div>
    </div>
    </NativeTerminalVisibilityProvider>
  );
});

type PaneResizeDividerProps = {
  direction: PaneDirection;
  ratio: number;
  onRatioChange: (ratio: number) => void;
  ariaLabel?: string;
};

function PaneResizeDivider({ direction, ratio, onRatioChange, ariaLabel = "Resize terminal panes" }: PaneResizeDividerProps) {
  const dividerRef = useRef<HTMLDivElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);
  const isHorizontal = direction === "horizontal";

  useEffect(() => {
    return () => {
      if (cleanupRef.current) {
        cleanupRef.current();
        cleanupRef.current = null;
      }
    };
  }, []);

  const handlePointerDown = (event: React.PointerEvent) => {
    event.preventDefault();
    const parent = dividerRef.current?.parentElement;
    if (!parent) return;
    if (cleanupRef.current) {
      cleanupRef.current();
      cleanupRef.current = null;
    }
    document.body.style.cursor = isHorizontal ? "col-resize" : "row-resize";
    const parentRect = parent.getBoundingClientRect();

    const handlePointerMove = (pointerEvent: PointerEvent) => {
      const totalSize = isHorizontal ? parentRect.width : parentRect.height;
      if (totalSize <= 0) return;
      if (totalSize <= MIN_PANE_SIZE_PX * 2) {
        onRatioChange(0.5);
        return;
      }
      const position = isHorizontal ? pointerEvent.clientX - parentRect.left : pointerEvent.clientY - parentRect.top;
      const minimumRatio = MIN_PANE_SIZE_PX / totalSize;
      const clampedRatio = Math.max(minimumRatio, Math.min(1 - minimumRatio, position / totalSize));
      onRatioChange(Number(clampedRatio.toFixed(4)));
    };

    const cleanup = () => {
      document.body.style.cursor = "";
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", cleanup);
      window.removeEventListener("pointercancel", cleanup);
      cleanupRef.current = null;
    };

    cleanupRef.current = cleanup;
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", cleanup);
    window.addEventListener("pointercancel", cleanup);
  };

  return (
    <div
      ref={dividerRef}
      role="separator"
      aria-label={ariaLabel}
      aria-orientation={isHorizontal ? "vertical" : "horizontal"}
      aria-valuenow={Math.round(ratio * 100)}
      data-divider-hit-target="true"
      onPointerDown={handlePointerDown}
      className={`no-drag relative z-20 shrink-0 touch-none ${
        isHorizontal ? "w-px cursor-col-resize" : "h-px cursor-row-resize"
      }`}
      style={{ backgroundColor: "var(--terminal-divider)" }}
    >
      <span
        data-divider-hit-area="true"
        className={`absolute z-10 ${
          isHorizontal ? "-inset-x-1 inset-y-0 cursor-col-resize" : "-inset-y-1 inset-x-0 cursor-row-resize"
        }`}
      />
    </div>
  );
}
