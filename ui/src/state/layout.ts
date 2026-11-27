import {
  createBrowserPaneContent,
  createChangesPaneContent,
  createDagPaneContent,
  createFilePaneContent,
  createRemoteDesktopPaneContent,
  createTerminalPaneContent,
  isTerminalTab,
  type BrowserPaneState,
  type ChangesPaneContent,
  type DagPaneState,
  type LayoutState,
  type PaneContent,
  type TabGroup,
  type TabGroupLayoutNode,
  type TabPaneLayout,
  type WorkspaceTab,
} from "../lib/types";
import {
  applySeamRatio,
  clampRatio,
  collectLeafIds,
  createLeafNode,
  equalizeRatios,
  findAlignedSplitRatio,
  findFirstLeafId,
  findLeaf,
  findSiblingLeafId,
  setLeafView,
  type PaneView,
  type PaneDirection,
  removeLeaf,
  setCollinearRatioAtPath,
  setRatioAtPath,
  splitLeaf,
  swapLeaves,
  type PaneNode,
  type ResolvedSeam,
} from "./paneTree";
import { equalizeSplitRun } from "./splitEqualization";

export type LayoutAction =
  | { type: "ADD_TAB"; tab: WorkspaceTab; sessionId?: string; activate?: boolean; view?: PaneView; leafId?: string }
  | { type: "CLOSE_TAB"; tabId: string; replacementTab?: WorkspaceTab }
  | { type: "ACTIVATE_TAB"; tabId: string }
  | { type: "UPDATE_FILE_TAB"; tabId: string; line: number | null; col: number | null; path?: string; remote?: import("../lib/filePreviewTypes").RemotePreviewOrigin }
  | { type: "REORDER_TAB"; tabId: string; targetIndex: number }
  | { type: "RENAME_TAB"; tabId: string; label: string }
  | { type: "SET_TAB_PINNED"; tabId: string; pinned: boolean }
  | {
      type: "SPLIT_PANE";
      tabId: string;
      targetLeafId?: string;
      direction: PaneDirection;
      newLeafId?: string;
      view?: PaneView;
      sessionId?: string;
      content?: PaneContent;
      position?: "first" | "second";
      ratio?: number;
    }
  | {
      type: "MOVE_TAB_TO_GROUP";
      sourceTabId: string;
      targetGroupId: string;
      targetIndex?: number;
    }
  | {
      type: "MOVE_TAB_TO_SPLIT";
      sourceTabId: string;
      targetGroupId: string;
      direction: PaneDirection;
      position?: "first" | "second";
      ratio?: number;
      /** Pre-minted by `dispatch` so both of its reductions name the new group identically. */
      newGroupId?: string;
    }
  | {
      type: "DETACH_PANE_TO_TAB";
      sourceTabId: string;
      leafId: string;
      newTab: WorkspaceTab;
      targetGroupId?: string;
      targetIndex?: number;
    }
  | { type: "SET_TAB_GROUP_RATIO"; path: string; ratio: number }
  | { type: "EQUALIZE_TAB_GROUP_RUN"; path: string }
  | { type: "CLOSE_PANE"; tabId: string; leafId: string; replacementSessionId?: string }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | { type: "SET_PANE_VIEW"; tabId: string; leafId: string; view: PaneView }
  | {
      type: "SET_PANE_RATIO";
      tabId: string;
      path: string;
      ratio: number;
      isolated?: boolean;
      seam?: ResolvedSeam | null;
    }
  | { type: "SWAP_PANES"; tabId: string; sourceLeafId: string; targetLeafId: string }
  | { type: "TOGGLE_PANE_EXPANDED"; tabId: string; leafId: string }
  | { type: "EQUALIZE_PANES"; tabId: string }
  | { type: "EQUALIZE_PANE_RUN"; tabId: string; path: string };

export function createLayoutState(tabs: WorkspaceTab[] = [], activeTabId?: string | null): LayoutState {
  const layoutsByTabId: Record<string, TabPaneLayout> = {};
  for (const tab of tabs) {
    const leafId = "leaf-init";
    const content = defaultContentForTab(tab);
    const sessionId = content.kind === "terminal" ? content.sessionId : "";
    layoutsByTabId[tab.id] = {
      root: createLeafNode(leafId),
      activeLeafId: leafId,
      expandedLeafId: null,
      sessionIdsByLeafId: { [leafId]: sessionId },
      contentsByLeafId: { [leafId]: content },
    };
  }

  return normalizeLayoutInternal(
    {
      tabs,
      activeTabId: activeTabId ?? tabs[0]?.id ?? null,
      layoutsByTabId,
    },
    true,
  );
}

export function getTabGroups(state: LayoutState): Record<string, TabGroup> {
  return normalizeLayout(state).tabGroups ?? {};
}

export function getTabGroupLayout(state: LayoutState): TabGroupLayoutNode | null {
  return normalizeLayout(state).tabGroupLayout ?? null;
}

export function getFocusedGroupId(state: LayoutState): string | null {
  return normalizeLayout(state).focusedGroupId ?? null;
}

export function getGroupForTab(state: LayoutState, tabId: string): TabGroup | null {
  const normalized = normalizeLayout(state);
  return findGroupContainingTab(normalized.tabGroups ?? {}, tabId);
}

export function getTabsForGroup(state: LayoutState, groupId: string): WorkspaceTab[] {
  const normalized = normalizeLayout(state);
  const group = normalized.tabGroups?.[groupId];
  if (!group) return [];
  const byId = new Map(normalized.tabs.map((tab) => [tab.id, tab]));
  return group.tabIds.map((tabId) => byId.get(tabId)).filter((tab): tab is WorkspaceTab => Boolean(tab));
}

export function layoutReducer(inputState: LayoutState, action: LayoutAction): LayoutState {
  const state = normalizeLayout(inputState);
  switch (action.type) {
    case "ADD_TAB": {
      const exists = state.tabs.some((tab) => tab.id === action.tab.id);
      const tabs = exists ? state.tabs : [...state.tabs, action.tab];
      const layoutsByTabId = { ...state.layoutsByTabId };
      if (!layoutsByTabId[action.tab.id]) {
        const leafId = action.leafId ?? createLayoutId("leaf");
        const content = defaultContentForTab(action.tab, action.sessionId);
        const sessionId = content.kind === "terminal" ? content.sessionId : "";
        layoutsByTabId[action.tab.id] = {
          root: { ...createLeafNode(leafId), ...(action.view ? { view: action.view } : {}) },
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: sessionId },
          contentsByLeafId: { [leafId]: content },
        };
      }
      return normalizeLayoutInternal(
        {
          ...state,
          tabs,
          activeTabId: action.activate === false ? state.activeTabId : action.tab.id,
          returnTabIdByTabId:
            !exists && action.activate !== false && (action.tab.kind === "file" || action.tab.kind === "browser" || action.tab.kind === "changes" || action.tab.kind === "remoteDesktop") && state.activeTabId
              ? { ...state.returnTabIdByTabId, [action.tab.id]: state.activeTabId }
              : state.returnTabIdByTabId,
          layoutsByTabId,
        },
        true,
      );
    }
    case "CLOSE_TAB": {
      const closingIndex = state.tabs.findIndex((tab) => tab.id === action.tabId);
      if (closingIndex < 0) return state;

      let tabs = state.tabs.filter((tab) => tab.id !== action.tabId);
      const layoutsByTabId = { ...state.layoutsByTabId };
      delete layoutsByTabId[action.tabId];

      if (tabs.length === 0 && action.replacementTab) {
        tabs = [action.replacementTab];
        const leafId = "leaf-replacement";
        const content = defaultContentForTab(action.replacementTab);
        const defaultSessionId = content.kind === "terminal" ? content.sessionId : "";
        layoutsByTabId[action.replacementTab.id] = {
          root: createLeafNode(leafId),
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: defaultSessionId },
          contentsByLeafId: { [leafId]: content },
        };
      }

      // The closing tab's own group decides the replacement. A multi-group layout must pick a
      // neighbor inside that group; indexing flat `state.tabs` would jump to whatever tab happens
      // to sit next in the flattened order, which may belong to an unrelated group.
      const groups = { ...(state.tabGroups ?? {}) };
      const closingGroup = findGroupContainingTab(groups, action.tabId);
      const groupIndex = closingGroup ? closingGroup.tabIds.indexOf(action.tabId) : -1;
      const remainingGroupTabs = closingGroup
        ? closingGroup.tabIds.filter((tabId) => tabId !== action.tabId)
        : [];
      const groupNeighbor = remainingGroupTabs.length > 0
        ? remainingGroupTabs[Math.min(groupIndex, remainingGroupTabs.length - 1)]
        : null;
      if (closingGroup) {
        if (remainingGroupTabs.length === 0) {
          delete groups[closingGroup.id];
        } else {
          groups[closingGroup.id] = {
            ...closingGroup,
            tabIds: remainingGroupTabs,
            activeTabId: closingGroup.activeTabId === action.tabId && groupNeighbor
              ? groupNeighbor
              : closingGroup.activeTabId,
          };
        }
      }

      let activeTabId = state.activeTabId;
      let focusedGroupId = state.focusedGroupId ?? null;
      if (state.activeTabId === action.tabId) {
        const returnTabId = state.returnTabIdByTabId?.[action.tabId];
        if (returnTabId && tabs.some((tab) => tab.id === returnTabId)) {
          activeTabId = returnTabId;
          focusedGroupId = findGroupContainingTab(groups, returnTabId)?.id ?? focusedGroupId;
        } else if (closingGroup && groupNeighbor) {
          activeTabId = groupNeighbor;
          focusedGroupId = closingGroup.id;
        } else {
          // The closing group is gone (its last tab closed), so focus moves to a surviving group.
          // That group's own `activeTabId` wins: `closingIndex` indexes the flat pre-filter tab
          // list, so using it here would land on an arbitrary neighbor of a different group.
          const survivingGroup = Object.values(groups).find((group) => group.tabIds.length > 0);
          const survivingActiveTabId = survivingGroup?.activeTabId;
          const fallbackTabId = (survivingActiveTabId && tabs.some((tab) => tab.id === survivingActiveTabId)
            ? survivingActiveTabId
            : null) ?? tabs[Math.min(closingIndex, tabs.length - 1)]?.id ?? null;
          activeTabId = fallbackTabId;
          const fallbackGroup = fallbackTabId ? findGroupContainingTab(groups, fallbackTabId) : null;
          if (fallbackGroup) focusedGroupId = fallbackGroup.id;
        }
      }

      // A closed tab is never a valid return target, whether it was the key or the value.
      const returnTabIdByTabId = Object.fromEntries(
        Object.entries(state.returnTabIdByTabId ?? {})
          .filter(([key, value]) => key !== action.tabId && value !== action.tabId),
      );
      return normalizeLayoutInternal(
        { ...state, tabs, activeTabId, focusedGroupId, tabGroups: groups, returnTabIdByTabId, layoutsByTabId },
        true,
      );
    }
    case "ACTIVATE_TAB": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return state;
      const group = findGroupContainingTab(state.tabGroups ?? {}, action.tabId);
      if (!group) return normalizeLayoutInternal({ ...state, activeTabId: action.tabId }, true);
      return normalizeLayoutInternal(
        {
          ...state,
          activeTabId: action.tabId,
          focusedGroupId: group.id,
          tabGroups: {
            ...state.tabGroups,
            [group.id]: { ...group, activeTabId: action.tabId },
          },
        },
        true,
      );
    }
    case "UPDATE_FILE_TAB": {
      const tab = state.tabs.find((item) => item.id === action.tabId);
      if (!tab || tab.kind !== "file") return state;
      const update = { line: action.line, col: action.col, ...(action.path ? { path: action.path } : {}), ...(action.remote ? { remote: action.remote } : {}) };
      const tabs = state.tabs.map((item) =>
        item.id === action.tabId && item.kind === "file"
          ? { ...item, ...update }
          : item,
      );
      const group = findGroupContainingTab(state.tabGroups ?? {}, action.tabId);
      const focused = group
        ? {
            focusedGroupId: group.id,
            tabGroups: {
              ...state.tabGroups,
              [group.id]: { ...group, activeTabId: action.tabId },
            },
          }
        : {};
      const existing = state.layoutsByTabId[action.tabId];
      if (!existing) {
        return normalizeLayoutInternal({ ...state, ...focused, tabs, activeTabId: action.tabId }, true);
      }
      const contentsByLeafId = { ...existing.contentsByLeafId };
      for (const [leafId, content] of Object.entries(contentsByLeafId)) {
        if (content.kind === "file" && content.previewId === tab.previewId) {
          contentsByLeafId[leafId] = { ...content, ...update };
        }
      }
      return normalizeLayoutInternal(
        {
          ...state,
          ...focused,
          tabs,
          activeTabId: action.tabId,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: { ...existing, contentsByLeafId },
          },
        },
        true,
      );
    }
    case "REORDER_TAB": {
      const group = findGroupContainingTab(state.tabGroups ?? {}, action.tabId);
      if (!group || group.tabIds.length < 2 || !Number.isFinite(action.targetIndex)) return state;
      const sourceIndex = group.tabIds.indexOf(action.tabId);
      const targetIndex = Math.max(0, Math.min(group.tabIds.length - 1, Math.trunc(action.targetIndex)));
      if (sourceIndex === targetIndex) return state;
      const tabIds = [...group.tabIds];
      const [moved] = tabIds.splice(sourceIndex, 1);
      tabIds.splice(targetIndex, 0, moved);
      return normalizeLayoutInternal(
        {
          ...state,
          tabGroups: {
            ...state.tabGroups,
            [group.id]: { ...group, tabIds },
          },
        },
        true,
      );
    }
    case "RENAME_TAB": {
      const label = action.label.trim();
      const target = state.tabs.find((tab) => tab.id === action.tabId);
      if (!target) return state;
      if (isTerminalTab(target) || target.kind === "file") {
        // A terminal tab's label is its automatic fallback; a user rename lives beside it so
        // live titles never overwrite it, and clearing the rename returns the tab to automatic.
        if (!label && target.customLabel === undefined) return state;
        if (label && target.customLabel === label) return state;
        return normalizeLayoutInternal(
          {
            ...state,
            tabs: state.tabs.map((tab) => {
              if (tab.id !== action.tabId || (!isTerminalTab(tab) && tab.kind !== "file")) return tab;
              if (label) return { ...tab, customLabel: label };
              const { customLabel: _cleared, ...rest } = tab;
              return rest;
            }),
          },
          true,
        );
      }
      if (!label) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          tabs: state.tabs.map((tab) => (tab.id === action.tabId ? { ...tab, label } : tab)),
        },
        true,
      );
    }
    case "SET_TAB_PINNED": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          tabs: state.tabs.map((tab) => (tab.id === action.tabId ? { ...tab, pinned: action.pinned } : tab)),
        },
        true,
      );
    }
    case "SPLIT_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const targetLeafId = action.targetLeafId ?? tabLayout.activeLeafId ?? findFirstLeafId(tabLayout.root);
      const existingLeafIds = collectLeafIds(tabLayout.root);
      if (!targetLeafId || !existingLeafIds.includes(targetLeafId)) return state;
      const newLeafId = action.newLeafId ?? createLayoutId("leaf");
      if (existingLeafIds.includes(newLeafId)) return state;
      const effectiveRatio =
        action.ratio ?? findAlignedSplitRatio(tabLayout.root, targetLeafId, action.direction);
      const newRoot = splitLeaf(
        tabLayout.root,
        targetLeafId,
        newLeafId,
        action.direction,
        action.position,
        effectiveRatio,
      );
      const newContent = action.content ?? createTerminalPaneContent(action.sessionId ?? "");
      const newSessionId = newContent.kind === "terminal" ? newContent.sessionId : "";
      return normalizeLayoutInternal(
        {
          ...state,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: {
              ...tabLayout,
              root: action.view && newContent.kind === "terminal" ? setLeafView(newRoot, newLeafId, action.view) : newRoot,
              activeLeafId: newLeafId,
              expandedLeafId: null,
              sessionIdsByLeafId: { ...tabLayout.sessionIdsByLeafId, [newLeafId]: newSessionId },
              contentsByLeafId: { ...(tabLayout.contentsByLeafId ?? {}), [newLeafId]: newContent },
            },
          },
        },
        true,
      );
    }
    case "MOVE_TAB_TO_GROUP": {
      const sourceTab = state.tabs.find((tab) => tab.id === action.sourceTabId);
      const groups = { ...(state.tabGroups ?? {}) };
      const sourceGroup = findGroupContainingTab(groups, action.sourceTabId);
      const targetGroup = groups[action.targetGroupId];
      if (!sourceTab || !sourceGroup || !targetGroup || !Number.isFinite(action.targetIndex ?? 0)) return state;

      if (sourceGroup.id === targetGroup.id) {
        if (action.targetIndex === undefined || sourceGroup.tabIds.length < 2) return state;
        const sourceIndex = sourceGroup.tabIds.indexOf(sourceTab.id);
        const targetIndex = Math.max(0, Math.min(sourceGroup.tabIds.length - 1, Math.trunc(action.targetIndex)));
        if (sourceIndex === targetIndex) return state;
        const tabIds = [...sourceGroup.tabIds];
        tabIds.splice(sourceIndex, 1);
        tabIds.splice(targetIndex, 0, sourceTab.id);
        groups[sourceGroup.id] = { ...sourceGroup, tabIds, activeTabId: sourceTab.id };
        return normalizeLayoutInternal(
          { ...state, activeTabId: sourceTab.id, focusedGroupId: sourceGroup.id, tabGroups: groups },
          true,
        );
      }

      const sourceIndex = sourceGroup.tabIds.indexOf(sourceTab.id);
      const remainingSourceTabIds = sourceGroup.tabIds.filter((tabId) => tabId !== sourceTab.id);
      let tabGroupLayout = state.tabGroupLayout ?? null;
      if (remainingSourceTabIds.length === 0) {
        delete groups[sourceGroup.id];
        tabGroupLayout = removeTabGroupLeaf(tabGroupLayout, sourceGroup.id);
      } else {
        groups[sourceGroup.id] = {
          ...sourceGroup,
          tabIds: remainingSourceTabIds,
          activeTabId:
            sourceGroup.activeTabId === sourceTab.id || !remainingSourceTabIds.includes(sourceGroup.activeTabId ?? "")
              ? remainingSourceTabIds[Math.min(sourceIndex, remainingSourceTabIds.length - 1)] ?? remainingSourceTabIds[0]
              : sourceGroup.activeTabId,
        };
      }

      const liveTarget = groups[targetGroup.id];
      if (!liveTarget) return state;
      const insertionIndex = Math.max(
        0,
        Math.min(liveTarget.tabIds.length, Math.trunc(action.targetIndex ?? liveTarget.tabIds.length)),
      );
      const targetTabIds = [...liveTarget.tabIds];
      targetTabIds.splice(insertionIndex, 0, sourceTab.id);
      groups[liveTarget.id] = { ...liveTarget, tabIds: targetTabIds, activeTabId: sourceTab.id };

      return normalizeLayoutInternal(
        {
          ...state,
          activeTabId: sourceTab.id,
          focusedGroupId: liveTarget.id,
          tabGroups: groups,
          tabGroupLayout,
        },
        true,
      );
    }
    case "MOVE_TAB_TO_SPLIT": {
      const sourceTab = state.tabs.find((tab) => tab.id === action.sourceTabId);
      const groups = { ...(state.tabGroups ?? {}) };
      const sourceGroup = findGroupContainingTab(groups, action.sourceTabId);
      const targetGroup = groups[action.targetGroupId];
      if (!sourceTab || !sourceGroup || !targetGroup) return state;
      if (sourceGroup.id === targetGroup.id && sourceGroup.tabIds.length <= 1) return state;

      const sourceIndex = sourceGroup.tabIds.indexOf(sourceTab.id);
      const remainingSourceTabIds = sourceGroup.tabIds.filter((tabId) => tabId !== sourceTab.id);
      const nextSourceActive =
        remainingSourceTabIds[Math.min(sourceIndex, remainingSourceTabIds.length - 1)] ?? remainingSourceTabIds[0] ?? null;

      let tabGroupLayout = state.tabGroupLayout ?? null;
      if (remainingSourceTabIds.length === 0 && sourceGroup.id !== targetGroup.id) {
        delete groups[sourceGroup.id];
        tabGroupLayout = removeTabGroupLeaf(tabGroupLayout, sourceGroup.id);
      } else {
        groups[sourceGroup.id] = {
          ...sourceGroup,
          tabIds: remainingSourceTabIds,
          activeTabId:
            sourceGroup.activeTabId === sourceTab.id || !remainingSourceTabIds.includes(sourceGroup.activeTabId ?? "")
              ? nextSourceActive
              : sourceGroup.activeTabId,
        };
      }

      const newGroupId = action.newGroupId ?? createLayoutId("group");
      groups[newGroupId] = { id: newGroupId, tabIds: [sourceTab.id], activeTabId: sourceTab.id };
      tabGroupLayout = splitTabGroupLeaf(
        tabGroupLayout,
        targetGroup.id,
        newGroupId,
        action.direction,
        action.position,
        action.ratio,
      );

      return normalizeLayoutInternal(
        {
          ...state,
          activeTabId: sourceTab.id,
          focusedGroupId: newGroupId,
          tabGroups: groups,
          tabGroupLayout,
        },
        true,
      );
    }
    case "DETACH_PANE_TO_TAB": {
      const sourceTab = state.tabs.find((tab) => tab.id === action.sourceTabId);
      const sourceLayout = state.layoutsByTabId[action.sourceTabId];
      const sourceGroup = findGroupContainingTab(state.tabGroups ?? {}, action.sourceTabId);
      if (!sourceTab || !sourceLayout || !sourceGroup || state.tabs.some((tab) => tab.id === action.newTab.id)) {
        return state;
      }
      const leafIds = collectLeafIds(sourceLayout.root);
      if (leafIds.length <= 1 || !leafIds.includes(action.leafId)) return state;
      const movedSessionId = sourceLayout.sessionIdsByLeafId[action.leafId] ?? "";
      const movedContent =
        sourceLayout.contentsByLeafId?.[action.leafId] ??
        (movedSessionId ? createTerminalPaneContent(movedSessionId) : undefined);
      // Any pane can become its own tab: the detached leaf keeps its own type (a browser leaf
      // becomes a browser tab, a file leaf a file tab, and so on). A `dag` pane has no tab type
      // of its own, so it is refused rather than silently re-typed as a terminal.
      const newTab = createTabForPaneContent(
        action.newTab.id,
        action.newTab.label,
        movedContent,
        movedSessionId,
        { pinned: action.newTab.pinned },
      );
      if (!newTab) return state;
      const newTabSessionId = isTerminalTab(newTab) ? movedSessionId : "";

      const nextRoot = removeLeaf(sourceLayout.root, action.leafId);
      if (!nextRoot) return state;
      const fallbackLeafId = findSiblingLeafId(sourceLayout.root, action.leafId) ?? findFirstLeafId(nextRoot);
      const nextSessionIdsByLeafId = { ...sourceLayout.sessionIdsByLeafId };
      delete nextSessionIdsByLeafId[action.leafId];
      const nextContentsByLeafId = { ...(sourceLayout.contentsByLeafId ?? {}) };
      delete nextContentsByLeafId[action.leafId];
      const nextSourceLayout: TabPaneLayout = {
        ...sourceLayout,
        root: nextRoot,
        activeLeafId: sourceLayout.activeLeafId === action.leafId ? fallbackLeafId : sourceLayout.activeLeafId,
        expandedLeafId: sourceLayout.expandedLeafId === action.leafId ? null : sourceLayout.expandedLeafId,
        sessionIdsByLeafId: nextSessionIdsByLeafId,
        contentsByLeafId: nextContentsByLeafId,
      };
      // Re-type the remaining source tab instead of patching `sessionId` by hand: the detached
      // leaf may have been its only terminal pane, in which case the tab has to follow the pane
      // that survives rather than keep a primary session the tree no longer contains.
      const nextSourceTab = reconcileTabWithPaneLayout(sourceTab, nextSourceLayout);
      const targetGroupId = action.targetGroupId ?? sourceGroup.id;
      const targetGroup = state.tabGroups?.[targetGroupId];
      if (!targetGroup) return state;

      const groups = { ...(state.tabGroups ?? {}) };
      const liveTarget = groups[targetGroupId];
      const defaultIndex = targetGroupId === sourceGroup.id
        ? sourceGroup.tabIds.indexOf(sourceTab.id) + 1
        : liveTarget.tabIds.length;
      const insertionIndex = Math.max(
        0,
        Math.min(liveTarget.tabIds.length, Math.trunc(action.targetIndex ?? defaultIndex)),
      );
      const targetTabIds = [...liveTarget.tabIds];
      targetTabIds.splice(insertionIndex, 0, newTab.id);
      groups[targetGroupId] = { ...liveTarget, tabIds: targetTabIds, activeTabId: newTab.id };

      return normalizeLayoutInternal(
        {
          ...state,
          tabs: [...state.tabs.map((tab) => (tab.id === sourceTab.id ? nextSourceTab : tab)), newTab],
          activeTabId: newTab.id,
          focusedGroupId: targetGroupId,
          tabGroups: groups,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [sourceTab.id]: nextSourceLayout,
            [newTab.id]: {
              root: findLeaf(sourceLayout.root, action.leafId) ?? createLeafNode(action.leafId),
              activeLeafId: action.leafId,
              expandedLeafId: null,
              sessionIdsByLeafId: { [action.leafId]: newTabSessionId },
              contentsByLeafId: { [action.leafId]: movedContent as PaneContent },
            },
          },
        },
        true,
      );
    }
    case "SET_TAB_GROUP_RATIO": {
      if (!state.tabGroupLayout) return state;
      const nextLayout = setTabGroupRatioAtPath(state.tabGroupLayout, action.path, action.ratio);
      if (nextLayout === state.tabGroupLayout) return state;
      return normalizeLayoutInternal({ ...state, tabGroupLayout: nextLayout }, true);
    }
    case "EQUALIZE_TAB_GROUP_RUN": {
      if (!state.tabGroupLayout) return state;
      const nextLayout = equalizeSplitRun(state.tabGroupLayout, action.path, (node): node is Extract<TabGroupLayoutNode, { type: "split" }> => node.type === "split");
      if (nextLayout === state.tabGroupLayout) return state;
      return normalizeLayoutInternal({ ...state, tabGroupLayout: nextLayout }, true);
    }
    case "CLOSE_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const leafIds = collectLeafIds(tabLayout.root);
      if (!leafIds.includes(action.leafId)) return state;
      const fallbackLeafId = findSiblingLeafId(tabLayout.root, action.leafId);
      const newRoot = removeLeaf(tabLayout.root, action.leafId);
      if (!newRoot) return layoutReducer(state, { type: "CLOSE_TAB", tabId: action.tabId });
      const newSessionIds = { ...tabLayout.sessionIdsByLeafId };
      delete newSessionIds[action.leafId];
      const newContents = { ...(tabLayout.contentsByLeafId ?? {}) };
      delete newContents[action.leafId];
      const nextTabLayout: TabPaneLayout = {
        ...tabLayout,
        root: newRoot,
        activeLeafId:
          tabLayout.activeLeafId === action.leafId
            ? fallbackLeafId ?? findFirstLeafId(newRoot)
            : tabLayout.activeLeafId,
        expandedLeafId: tabLayout.expandedLeafId === action.leafId ? null : tabLayout.expandedLeafId,
        sessionIdsByLeafId: newSessionIds,
        contentsByLeafId: newContents,
      };
      // Closing a pane works on any tab kind, so the tab's own identity has to follow: a terminal
      // tab that just lost its last terminal pane is re-typed to the surviving pane instead of
      // keeping a primary session the tree no longer holds.
      const tabs = state.tabs.map((tab) =>
        tab.id === action.tabId ? reconcileTabWithPaneLayout(tab, nextTabLayout) : tab,
      );
      return normalizeLayoutInternal(
        {
          ...state,
          tabs,
          layoutsByTabId: { ...state.layoutsByTabId, [action.tabId]: nextTabLayout },
        },
        true,
      );
    }
    case "SET_PANE_VIEW": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const content = tabLayout.contentsByLeafId?.[action.leafId];
      if (content && content.kind !== "terminal") return state;
      const root = setLeafView(tabLayout.root, action.leafId, action.view);
      if (root === tabLayout.root) return state;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: { ...tabLayout, root },
        },
      };
    }
    case "FOCUS_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout || !collectLeafIds(tabLayout.root).includes(action.leafId)) return state;
      const group = findGroupContainingTab(state.tabGroups ?? {}, action.tabId);
      return normalizeLayoutInternal(
        {
          ...state,
          activeTabId: action.tabId,
          focusedGroupId: group?.id ?? state.focusedGroupId,
          tabGroups: group
            ? { ...state.tabGroups, [group.id]: { ...group, activeTabId: action.tabId } }
            : state.tabGroups,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: { ...tabLayout, activeLeafId: action.leafId },
          },
        },
        true,
      );
    }
    case "SET_PANE_RATIO": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const root = action.isolated
        ? setRatioAtPath(tabLayout.root, action.path, action.ratio)
        : action.seam
          ? applySeamRatio(tabLayout.root, action.seam, action.ratio)
          : setCollinearRatioAtPath(tabLayout.root, action.path, action.ratio);
      if (root === tabLayout.root) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: { ...tabLayout, root },
          },
        },
        true,
      );
    }
    case "EQUALIZE_PANE_RUN": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const root = equalizeSplitRun(tabLayout.root, action.path, (node): node is Extract<PaneNode, { type: "split" }> => node.type === "split");
      if (root === tabLayout.root) return state;
      return normalizeLayoutInternal({
        ...state,
        layoutsByTabId: { ...state.layoutsByTabId, [action.tabId]: { ...tabLayout, root } },
      }, true);
    }
    case "SWAP_PANES": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const root = swapLeaves(tabLayout.root, action.sourceLeafId, action.targetLeafId);
      if (root === tabLayout.root) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: { ...tabLayout, root },
          },
        },
        true,
      );
    }
    case "TOGGLE_PANE_EXPANDED": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout || !collectLeafIds(tabLayout.root).includes(action.leafId)) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: {
              ...tabLayout,
              expandedLeafId: tabLayout.expandedLeafId === action.leafId ? null : action.leafId,
            },
          },
        },
        true,
      );
    }
    case "EQUALIZE_PANES": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const root = equalizeRatios(tabLayout.root);
      if (root === tabLayout.root) return state;
      return normalizeLayoutInternal(
        {
          ...state,
          layoutsByTabId: {
            ...state.layoutsByTabId,
            [action.tabId]: { ...tabLayout, root },
          },
        },
        true,
      );
    }
  }
}

export function normalizeLayout(state: LayoutState): LayoutState {
  return normalizeLayoutInternal(state, false);
}

export function toPaneContent(raw: unknown, fallbackSessionId = ""): PaneContent {
  if (raw && typeof raw === "object" && "kind" in raw) {
    const item = raw as {
      kind: string;
      sessionId?: string;
      browser?: BrowserPaneState;
      browserId?: string;
      url?: string;
      title?: string | null;
      loading?: boolean;
      canGoBack?: boolean;
      canGoForward?: boolean;
      profileId?: string;
      worktreePath?: string;
      worktreeLabel?: string;
    };
    if (item.kind === "terminal") {
      return createTerminalPaneContent(item.sessionId ?? fallbackSessionId);
    }
    if (item.kind === "browser") {
      const browserState: BrowserPaneState = item.browser
        ? {
            browserId: item.browser.browserId ?? item.browserId ?? "",
            url: item.browser.url ?? item.url ?? "about:blank",
            title: item.browser.title ?? item.title ?? null,
            loading: item.browser.loading ?? item.loading ?? false,
            canGoBack: item.browser.canGoBack ?? item.canGoBack ?? false,
            canGoForward: item.browser.canGoForward ?? item.canGoForward ?? false,
            profileId: item.browser.profileId ?? item.profileId,
            worktreePath: item.browser.worktreePath ?? item.worktreePath,
            worktreeLabel: item.browser.worktreeLabel ?? item.worktreeLabel,
          }
        : {
            browserId: item.browserId ?? "",
            url: item.url ?? "about:blank",
            title: item.title ?? null,
            loading: item.loading ?? false,
            canGoBack: item.canGoBack ?? false,
            canGoForward: item.canGoForward ?? false,
            profileId: item.profileId,
            worktreePath: item.worktreePath,
            worktreeLabel: item.worktreeLabel,
          };
      return createBrowserPaneContent(browserState);
    }
    if (item.kind === "file") {
      const file = item as {
        path?: string;
        backendSessionId?: string;
        line?: number | null;
        col?: number | null;
        workspaceId?: string | null;
        previewId?: string;
        customLabel?: string;
        remote?: import("../lib/filePreviewTypes").RemotePreviewOrigin;
      };
      return createFilePaneContent({
        path: file.path ?? "",
        backendSessionId: file.backendSessionId ?? "",
        line: file.line ?? null,
        col: file.col ?? null,
        workspaceId: file.workspaceId ?? null,
        previewId: file.previewId ?? "",
        ...(file.remote ? { remote: file.remote } : {}),
        ...(typeof file.customLabel === "string" ? { customLabel: file.customLabel } : {}),
      });
    }
    if (item.kind === "changes") {
      const changes = item as Partial<ChangesPaneContent>;
      if (changes.workspaceId && changes.worktreePath && changes.worktree !== undefined) {
        return createChangesPaneContent({
          workspaceId: changes.workspaceId,
          worktreePath: changes.worktreePath,
          worktree: changes.worktree,
        });
      }
    }
    if (item.kind === "remoteDesktop") {
      const hostId = (item as { hostId?: unknown }).hostId;
      if (typeof hostId === "string" && hostId) return createRemoteDesktopPaneContent(hostId);
    }
    if (item.kind === "dag") {
      const dagState: DagPaneState = (item as { dag?: DagPaneState }).dag
        ? { runId: (item as { dag?: DagPaneState }).dag?.runId ?? (item as { runId?: string | null }).runId ?? null }
        : { runId: (item as { runId?: string | null }).runId ?? null };
      return createDagPaneContent(dagState);
    }
  }
  return createTerminalPaneContent(fallbackSessionId);
}

export function defaultContentForTab(tab: WorkspaceTab, sessionIdOverride?: string): PaneContent {
  if (tab.kind === "browser") {
    return createBrowserPaneContent({
      browserId: tab.browserId,
      url: tab.url,
      title: tab.title,
      loading: tab.loading,
      canGoBack: tab.canGoBack,
      canGoForward: tab.canGoForward,
      profileId: tab.profileId,
      worktreePath: tab.worktreePath,
      worktreeLabel: tab.worktreeLabel,
    });
  }
  if (tab.kind === "file") {
    return createFilePaneContent({
      path: tab.path,
      backendSessionId: tab.backendSessionId,
      line: tab.line,
      col: tab.col,
      workspaceId: tab.workspaceId,
      previewId: tab.previewId,
      ...(tab.remote ? { remote: tab.remote } : {}),
      ...(tab.customLabel !== undefined ? { customLabel: tab.customLabel } : {}),
    });
  }
  if (tab.kind === "changes") {
    return createChangesPaneContent({ workspaceId: tab.workspaceId, worktreePath: tab.worktreePath, worktree: tab.worktree });
  }
  if (tab.kind === "remoteDesktop") {
    return createRemoteDesktopPaneContent(tab.hostId);
  }
  return createTerminalPaneContent(sessionIdOverride ?? tab.sessionId);
}

export function getTabPaneLayout(layout: LayoutState, tab: WorkspaceTab): TabPaneLayout {
  const isNonTerminalTab = !isTerminalTab(tab);
  const fallbackLeafId = `leaf-default-${tab.id}`;
  const defaultContent = defaultContentForTab(tab);
  return layout.layoutsByTabId?.[tab.id] ?? {
    root: { type: "leaf", leafId: fallbackLeafId },
    activeLeafId: fallbackLeafId,
    expandedLeafId: null,
    sessionIdsByLeafId: { [fallbackLeafId]: isNonTerminalTab ? "" : tab.sessionId },
    contentsByLeafId: { [fallbackLeafId]: defaultContent },
  };
}

/**
 * The frontend session of the pane that has focus inside a tab. A split tab's own `sessionId`
 * names one pane only, so tab-level session actions (Suspend/Restart) must target this one.
 */
export function focusedPaneSessionId(layout: LayoutState, tab: WorkspaceTab): string | null {
  const paneLayout = getTabPaneLayout(layout, tab);
  const leafId = paneLayout.activeLeafId;
  const sessionId = leafId ? paneLayout.sessionIdsByLeafId[leafId] : undefined;
  if (sessionId) return sessionId;
  // A terminal pane can live inside a browser/file/changes/remoteDesktop tab after a mixed split;
  // those tabs have no primary session of their own, so only the focused leaf can name one.
  return isTerminalTab(tab) ? tab.sessionId || null : null;
}

/**
 * A tab's primary session only means something while a terminal pane backs it. Closing or
 * detaching the last terminal leaf must never leave a terminal tab pointing at a session the
 * pane tree no longer contains, so the tab is re-typed to whatever pane survives.
 */
export function reconcileTabWithPaneLayout(tab: WorkspaceTab, tabLayout: TabPaneLayout | undefined): WorkspaceTab {
  if (!tabLayout) return tab;
  const leafIds = collectLeafIds(tabLayout.root);
  const terminalSessionIds = leafIds
    .map((leafId) => tabLayout.sessionIdsByLeafId[leafId])
    .filter((sessionId): sessionId is string => Boolean(sessionId));
  if (!isTerminalTab(tab)) return tab;
  if (terminalSessionIds.length > 0) {
    if (terminalSessionIds.includes(tab.sessionId)) return tab;
    const activeLeafId = tabLayout.activeLeafId ?? leafIds[0];
    const activeSessionId = activeLeafId ? tabLayout.sessionIdsByLeafId[activeLeafId] : "";
    const primarySessionId = activeSessionId || terminalSessionIds[0];
    return primarySessionId === tab.sessionId ? tab : { ...tab, sessionId: primarySessionId };
  }
  const survivingLeafId = tabLayout.activeLeafId ?? leafIds[0];
  return retypeTabForContent(tab, survivingLeafId ? tabLayout.contentsByLeafId?.[survivingLeafId] : undefined);
}

/**
 * The tab a detached or surviving pane leaf becomes. `null` when the pane content has no tab
 * type of its own (a `dag` run is pane-only), so callers refuse instead of inventing one.
 */
export function createTabForPaneContent(
  id: string,
  label: string,
  content: PaneContent | undefined,
  sessionId: string,
  options?: { pinned?: boolean },
): WorkspaceTab | null {
  const base = { id, label, ...(options?.pinned ? { pinned: true } : {}) };
  switch (content?.kind) {
    case "terminal":
      return { kind: "terminal", ...base, sessionId: content.sessionId || sessionId };
    case "browser": {
      const browser = content.browser ?? content;
      return {
        kind: "browser",
        ...base,
        browserId: browser.browserId ?? "",
        url: browser.url ?? "about:blank",
        ...(browser.title !== undefined ? { title: browser.title } : {}),
        ...(browser.loading !== undefined ? { loading: browser.loading } : {}),
        ...(browser.canGoBack !== undefined ? { canGoBack: browser.canGoBack } : {}),
        ...(browser.canGoForward !== undefined ? { canGoForward: browser.canGoForward } : {}),
        ...(browser.zoomFactor !== undefined ? { zoomFactor: browser.zoomFactor } : {}),
        ...(browser.profileId !== undefined ? { profileId: browser.profileId } : {}),
        ...(browser.worktreePath !== undefined ? { worktreePath: browser.worktreePath } : {}),
        ...(browser.worktreeLabel !== undefined ? { worktreeLabel: browser.worktreeLabel } : {}),
      };
    }
    case "file":
      return {
        kind: "file",
        ...base,
        path: content.path,
        backendSessionId: content.backendSessionId,
        line: content.line,
        col: content.col,
        workspaceId: content.workspaceId,
        previewId: content.previewId,
        ...(content.remote ? { remote: content.remote } : {}),
        ...(content.customLabel !== undefined ? { customLabel: content.customLabel } : {}),
      };
    case "changes":
      return {
        kind: "changes",
        ...base,
        workspaceId: content.workspaceId,
        worktreePath: content.worktreePath,
        worktree: content.worktree,
      };
    case "remoteDesktop":
      return { kind: "remoteDesktop", ...base, hostId: content.hostId, sessionId: "" };
    default:
      return null;
  }
}

function retypeTabForContent(tab: WorkspaceTab, content: PaneContent | undefined): WorkspaceTab {
  const retyped = createTabForPaneContent(tab.id, tab.label, content, "", { pinned: tab.pinned });
  // A `dag` pane has no typed tab of its own: keep the tab shell rather than inventing one that
  // no pane renders. Only a terminal shell has a primary session to drop; a browser/file/changes
  // tab has no `sessionId` field at all.
  return retyped ?? (isTerminalTab(tab) ? { ...tab, sessionId: "" } : tab);
}

function normalizeLayoutInternal(state: LayoutState, force: boolean): LayoutState {
  if (!force && isNormalizedLayoutState(state)) return state;

  const dedupedTabs = dedupeTabs(state.tabs);
  if (dedupedTabs.length === 0) {
    if (
      state.tabs.length === 0 &&
      state.activeTabId === null &&
      Object.keys(state.layoutsByTabId).length === 0 &&
      state.tabGroupLayout === null &&
      state.focusedGroupId === null &&
      Object.keys(state.tabGroups ?? {}).length === 0
    ) {
      return state;
    }
    return {
      ...state,
      tabs: [],
      activeTabId: null,
      layoutsByTabId: {},
      tabGroups: {},
      tabGroupLayout: null,
      focusedGroupId: null,
    };
  }

  const validTabIds = new Set(dedupedTabs.map((tab) => tab.id));
  const nextLayoutsByTabId: Record<string, TabPaneLayout> = {};
  let paneLayoutsChanged = Object.keys(state.layoutsByTabId).length !== dedupedTabs.length;
  for (const tab of dedupedTabs) {
    const existing = state.layoutsByTabId[tab.id];
    if (existing) {
      const leaves = collectLeafIds(existing.root);
      const activeLeafId = leaves.includes(existing.activeLeafId ?? "") ? existing.activeLeafId : leaves[0] ?? null;
      const expandedLeafId = leaves.includes(existing.expandedLeafId ?? "") ? existing.expandedLeafId : null;
      const defaultContent = defaultContentForTab(tab);

      const contentsByLeafId: Record<string, PaneContent> = {};
      const sessionIdsByLeafId: Record<string, string> = {};

      for (const leafId of leaves) {
        if (existing.contentsByLeafId && Object.prototype.hasOwnProperty.call(existing.contentsByLeafId, leafId)) {
          const content = toPaneContent(
            existing.contentsByLeafId[leafId],
            existing.sessionIdsByLeafId?.[leafId] ?? (isTerminalTab(tab) ? tab.sessionId : ""),
          );
          contentsByLeafId[leafId] = content;
          sessionIdsByLeafId[leafId] = content.kind === "terminal" ? content.sessionId : "";
        } else if (existing.sessionIdsByLeafId && Object.prototype.hasOwnProperty.call(existing.sessionIdsByLeafId, leafId)) {
          if (!isTerminalTab(tab)) {
            contentsByLeafId[leafId] = defaultContent;
            sessionIdsByLeafId[leafId] = "";
          } else {
            const sessionId = existing.sessionIdsByLeafId[leafId] ?? tab.sessionId;
            const content = createTerminalPaneContent(sessionId);
            contentsByLeafId[leafId] = content;
            sessionIdsByLeafId[leafId] = sessionId;
          }
        } else {
          contentsByLeafId[leafId] = defaultContent;
          sessionIdsByLeafId[leafId] = defaultContent.kind === "terminal" ? defaultContent.sessionId : "";
        }
      }

      const existingSessionKeys = Object.keys(existing.sessionIdsByLeafId ?? {});
      const sessionMappingIsExact =
        existingSessionKeys.length === leaves.length &&
        leaves.every((leafId) => Object.prototype.hasOwnProperty.call(existing.sessionIdsByLeafId, leafId));

      const existingContentKeys = Object.keys(existing.contentsByLeafId ?? {});
      const contentMappingIsExact =
        existingContentKeys.length === leaves.length &&
        leaves.every((leafId) => Object.prototype.hasOwnProperty.call(existing.contentsByLeafId ?? {}, leafId));

      if (
        activeLeafId === existing.activeLeafId &&
        expandedLeafId === existing.expandedLeafId &&
        sessionMappingIsExact &&
        contentMappingIsExact
      ) {
        nextLayoutsByTabId[tab.id] = existing;
      } else {
        paneLayoutsChanged = true;
        nextLayoutsByTabId[tab.id] = {
          ...existing,
          activeLeafId,
          expandedLeafId,
          sessionIdsByLeafId,
          contentsByLeafId,
        };
      }
    } else {
      paneLayoutsChanged = true;
      // Derived, never random: `dispatch` reduces the same action twice (its own pre-reduce and
      // React's commit reduction), and both passes must name this tab's leaf identically. A tab
      // owns its leaf ids alone, so the tab id is unique enough and `getTabPaneLayout`'s fallback
      // already uses this exact shape.
      const leafId = `leaf-default-${tab.id}`;
      const content = defaultContentForTab(tab);
      const defaultSessionId = content.kind === "terminal" ? content.sessionId : "";
      nextLayoutsByTabId[tab.id] = {
        root: createLeafNode(leafId),
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: defaultSessionId },
        contentsByLeafId: { [leafId]: content },
      };
    }
  }
  const layoutsByTabId = paneLayoutsChanged ? nextLayoutsByTabId : state.layoutsByTabId;

  const incomingGroups = state.tabGroups ?? {};
  const groups: Record<string, TabGroup> = {};
  const assigned = new Set<string>();
  for (const [groupId, group] of Object.entries(incomingGroups)) {
    const tabIds = group.tabIds.filter((tabId) => validTabIds.has(tabId) && !assigned.has(tabId));
    if (tabIds.length === 0) continue;
    tabIds.forEach((tabId) => assigned.add(tabId));
    groups[groupId] = {
      id: groupId,
      tabIds,
      activeTabId: tabIds.includes(group.activeTabId ?? "") ? group.activeTabId : tabIds[0],
    };
  }

  let tabGroupLayout = pruneTabGroupLayout(state.tabGroupLayout ?? null, new Set(Object.keys(groups)));
  if (Object.keys(groups).length === 0) {
    const groupId = "group-default";
    groups[groupId] = {
      id: groupId,
      tabIds: dedupedTabs.map((tab) => tab.id),
      activeTabId: validTabIds.has(state.activeTabId ?? "") ? state.activeTabId : dedupedTabs[0].id,
    };
    dedupedTabs.forEach((tab) => assigned.add(tab.id));
    tabGroupLayout = { type: "group", groupId };
  }

  let focusedGroupId =
    state.focusedGroupId && groups[state.focusedGroupId]
      ? state.focusedGroupId
      : findGroupContainingTab(groups, state.activeTabId ?? "")?.id ?? findFirstTabGroupId(tabGroupLayout);
  if (!focusedGroupId || !groups[focusedGroupId]) focusedGroupId = Object.keys(groups)[0] ?? null;

  const unassigned = dedupedTabs.map((tab) => tab.id).filter((tabId) => !assigned.has(tabId));
  if (unassigned.length > 0 && focusedGroupId) {
    groups[focusedGroupId] = {
      ...groups[focusedGroupId],
      tabIds: [...groups[focusedGroupId].tabIds, ...unassigned],
    };
    unassigned.forEach((tabId) => assigned.add(tabId));
  }

  for (const groupId of Object.keys(groups)) {
    if (!containsTabGroup(tabGroupLayout, groupId)) {
      tabGroupLayout = appendTabGroupLayout(tabGroupLayout, { type: "group", groupId });
    }
  }
  tabGroupLayout = pruneTabGroupLayout(tabGroupLayout, new Set(Object.keys(groups)));

  if (focusedGroupId) {
    const focused = groups[focusedGroupId];
    if (focused && focused.tabIds.includes(state.activeTabId ?? "")) {
      groups[focusedGroupId] = { ...focused, activeTabId: state.activeTabId };
    }
  }

  const focusedGroup = focusedGroupId ? groups[focusedGroupId] : null;
  const activeTabId = focusedGroup?.activeTabId ?? dedupedTabs[0].id;
  const tabById = new Map(dedupedTabs.map((tab) => [tab.id, tab]));
  const flattenedIds = flattenTabIds(tabGroupLayout, groups);
  const tabs = flattenedIds.map((tabId) => tabById.get(tabId)).filter((tab): tab is WorkspaceTab => Boolean(tab));
  for (const tab of dedupedTabs) if (!flattenedIds.includes(tab.id)) tabs.push(tab);

  return {
    ...state,
    tabs,
    activeTabId,
    layoutsByTabId,
    tabGroups: groups,
    tabGroupLayout,
    focusedGroupId,
  };
}

function isNormalizedLayoutState(state: LayoutState): boolean {
  if (state.tabs.length === 0) {
    return (
      state.activeTabId === null &&
      Object.keys(state.layoutsByTabId).length === 0 &&
      state.tabGroupLayout === null &&
      state.focusedGroupId === null &&
      Object.keys(state.tabGroups ?? {}).length === 0
    );
  }
  if (!state.tabGroups || !state.tabGroupLayout || !state.focusedGroupId || !state.tabGroups[state.focusedGroupId]) return false;
  const valid = new Set(state.tabs.map((tab) => tab.id));
  // `normalizeLayout` collapses a duplicate id, so a state carrying one is not normalized.
  if (valid.size !== state.tabs.length) return false;
  // The active tab is always the focused group's own active tab; a mismatch would render a tab
  // that is not in the focused group as selected.
  if (state.activeTabId !== state.tabGroups[state.focusedGroupId].activeTabId) return false;
  // Surplus keys would hand a stale pane layout to a tab id that no longer exists.
  if (Object.keys(state.layoutsByTabId).length !== state.tabs.length) return false;
  const assigned = new Set<string>();
  for (const group of Object.values(state.tabGroups)) {
    if (group.tabIds.length === 0 || !containsTabGroup(state.tabGroupLayout, group.id)) return false;
    if (!group.activeTabId || !group.tabIds.includes(group.activeTabId)) return false;
    for (const tabId of group.tabIds) {
      if (!valid.has(tabId) || assigned.has(tabId)) return false;
      assigned.add(tabId);
    }
  }
  if (assigned.size !== valid.size) return false;
  // Every leaf of the group split tree must name a live group, or the split view renders a hole.
  if (!everyGroupLeafKnown(state.tabGroupLayout, state.tabGroups)) return false;
  return state.tabs.every((tab) => {
    const layout = state.layoutsByTabId[tab.id];
    if (!layout) return false;
    if (!layout.contentsByLeafId) return false;
    const leaves = collectLeafIds(layout.root);
    const activeLeafId = layout.activeLeafId;
    const expandedLeafId = layout.expandedLeafId;
    // A focus or zoom that names a removed leaf leaves the tab rendering an invisible pane.
    if (activeLeafId === null || !leaves.includes(activeLeafId)) return false;
    if (expandedLeafId !== null && !leaves.includes(expandedLeafId)) return false;
    return leaves.every((leafId) => Object.prototype.hasOwnProperty.call(layout.contentsByLeafId, leafId));
  });
}

function findGroupContainingTab(groups: Record<string, TabGroup>, tabId: string): TabGroup | null {
  if (!tabId) return null;
  return Object.values(groups).find((group) => group.tabIds.includes(tabId)) ?? null;
}

function splitTabGroupLeaf(
  root: TabGroupLayoutNode | null,
  targetGroupId: string,
  newGroupId: string,
  direction: PaneDirection,
  position: "first" | "second" = "second",
  ratio = 0.5,
): TabGroupLayoutNode | null {
  if (!root) return null;
  if (root.type === "group") {
    if (root.groupId !== targetGroupId) return root;
    const added: TabGroupLayoutNode = { type: "group", groupId: newGroupId };
    return {
      type: "split",
      direction,
      first: position === "first" ? added : root,
      second: position === "first" ? root : added,
      ratio: clampRatio(ratio),
    };
  }
  const first = splitTabGroupLeaf(root.first, targetGroupId, newGroupId, direction, position, ratio);
  if (first !== root.first) return { ...root, first: first ?? root.first };
  const second = splitTabGroupLeaf(root.second, targetGroupId, newGroupId, direction, position, ratio);
  if (second !== root.second) return { ...root, second: second ?? root.second };
  return root;
}

function removeTabGroupLeaf(root: TabGroupLayoutNode | null, groupId: string): TabGroupLayoutNode | null {
  if (!root) return null;
  if (root.type === "group") return root.groupId === groupId ? null : root;
  const first = removeTabGroupLeaf(root.first, groupId);
  if (!first) return root.second;
  if (first !== root.first) return { ...root, first };
  const second = removeTabGroupLeaf(root.second, groupId);
  if (!second) return root.first;
  if (second !== root.second) return { ...root, second };
  return root;
}

function pruneTabGroupLayout(root: TabGroupLayoutNode | null, validGroupIds: Set<string>): TabGroupLayoutNode | null {
  if (!root) return null;
  if (root.type === "group") return validGroupIds.has(root.groupId) ? root : null;
  const first = pruneTabGroupLayout(root.first, validGroupIds);
  const second = pruneTabGroupLayout(root.second, validGroupIds);
  if (!first) return second;
  if (!second) return first;
  if (first === root.first && second === root.second) return root;
  return { ...root, first, second };
}

function appendTabGroupLayout(first: TabGroupLayoutNode | null, second: TabGroupLayoutNode): TabGroupLayoutNode {
  if (!first) return second;
  return { type: "split", direction: "horizontal", first, second, ratio: 0.5 };
}

function containsTabGroup(root: TabGroupLayoutNode | null, groupId: string): boolean {
  if (!root) return false;
  if (root.type === "group") return root.groupId === groupId;
  return containsTabGroup(root.first, groupId) || containsTabGroup(root.second, groupId);
}

function everyGroupLeafKnown(
  root: TabGroupLayoutNode | null,
  groups: Record<string, TabGroup>,
): boolean {
  if (!root) return false;
  if (root.type === "group") return Boolean(groups[root.groupId]);
  return everyGroupLeafKnown(root.first, groups) && everyGroupLeafKnown(root.second, groups);
}

function findFirstTabGroupId(root: TabGroupLayoutNode | null): string | null {
  if (!root) return null;
  let node = root;
  while (node.type === "split") node = node.first;
  return node.groupId;
}

function flattenTabIds(root: TabGroupLayoutNode | null, groups: Record<string, TabGroup>): string[] {
  if (!root) return [];
  if (root.type === "group") return groups[root.groupId]?.tabIds ?? [];
  return [...flattenTabIds(root.first, groups), ...flattenTabIds(root.second, groups)];
}

function setTabGroupRatioAtPath(root: TabGroupLayoutNode, path: string, ratio: number): TabGroupLayoutNode {
  const segments = path.split(".").filter(Boolean);
  const visit = (node: TabGroupLayoutNode, remaining: string[]): TabGroupLayoutNode => {
    if (node.type === "group") return node;
    if (remaining.length === 0) {
      const nextRatio = clampRatio(ratio);
      return node.ratio === nextRatio ? node : { ...node, ratio: nextRatio };
    }
    const [head, ...rest] = remaining;
    if (head !== "first" && head !== "second") return node;
    const child = visit(node[head], rest);
    return child === node[head] ? node : { ...node, [head]: child };
  };
  return visit(root, segments);
}

function dedupeTabs(tabs: WorkspaceTab[]) {
  const seen = new Set<string>();
  return tabs.filter((tab) => {
    if (seen.has(tab.id)) return false;
    seen.add(tab.id);
    return true;
  });
}

function createLayoutId(prefix: string) {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}:${randomPart}`;
}
