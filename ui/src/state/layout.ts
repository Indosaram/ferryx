import type { LayoutState, TabPaneLayout, WorkspaceTab } from "../lib/types";
import {
  collectLeafIds,
  createLeafNode,
  equalizeRatios,
  findFirstLeafId,
  findSiblingLeafId,
  PaneDirection,
  removeLeaf,
  setRatioAtPath,
  splitLeaf,
  swapLeaves,
} from "./paneTree";

export type LayoutAction =
  | { type: "ADD_TAB"; tab: WorkspaceTab; sessionId?: string; activate?: boolean }
  | { type: "CLOSE_TAB"; tabId: string; replacementTab?: WorkspaceTab }
  | { type: "ACTIVATE_TAB"; tabId: string }
  | {
      type: "SPLIT_PANE";
      tabId: string;
      targetLeafId?: string;
      direction: PaneDirection;
      newLeafId?: string;
      sessionId: string;
      position?: "first" | "second";
      ratio?: number;
    }
  | { type: "CLOSE_PANE"; tabId: string; leafId: string; replacementSessionId?: string }
  | { type: "FOCUS_PANE"; tabId: string; leafId: string }
  | { type: "SET_PANE_RATIO"; tabId: string; path: string; ratio: number }
  | { type: "SWAP_PANES"; tabId: string; sourceLeafId: string; targetLeafId: string }
  | { type: "TOGGLE_PANE_EXPANDED"; tabId: string; leafId: string }
  | { type: "EQUALIZE_PANES"; tabId: string };

export function createLayoutState(tabs: WorkspaceTab[] = [], activeTabId?: string | null): LayoutState {
  const layoutsByTabId: Record<string, TabPaneLayout> = {};
  for (const tab of tabs) {
    const leafId = "leaf-init";
    const sessionId = tab.kind === "browser" ? "" : tab.sessionId;
    layoutsByTabId[tab.id] = {
      root: createLeafNode(leafId),
      activeLeafId: leafId,
      expandedLeafId: null,
      sessionIdsByLeafId: { [leafId]: sessionId },
    };
  }

  return normalizeLayout({
    tabs,
    activeTabId: activeTabId ?? tabs[0]?.id ?? null,
    layoutsByTabId,
  });
}

export function layoutReducer(state: LayoutState, action: LayoutAction): LayoutState {
  switch (action.type) {
    case "ADD_TAB": {
      const exists = state.tabs.some((tab) => tab.id === action.tab.id);
      const tabs = exists ? state.tabs : [...state.tabs, action.tab];
      const layoutsByTabId = { ...state.layoutsByTabId };
      if (!layoutsByTabId[action.tab.id]) {
        const leafId = `leaf-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
        const defaultSessionId = action.tab.kind === "browser" ? "" : action.tab.sessionId;
        const sessionId = action.sessionId ?? defaultSessionId;
        layoutsByTabId[action.tab.id] = {
          root: createLeafNode(leafId),
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: sessionId },
        };
      }
      return normalizeLayout({
        ...state,
        tabs,
        activeTabId: action.activate === false ? state.activeTabId : action.tab.id,
        layoutsByTabId,
      });
    }
    case "CLOSE_TAB": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return normalizeLayout(state);

      let tabs = state.tabs.filter((tab) => tab.id !== action.tabId);
      const layoutsByTabId = { ...state.layoutsByTabId };
      delete layoutsByTabId[action.tabId];

      if (tabs.length === 0 && action.replacementTab) {
        tabs = [action.replacementTab];
        const leafId = "leaf-replacement";
        const defaultSessionId = action.replacementTab.kind === "browser" ? "" : action.replacementTab.sessionId;
        layoutsByTabId[action.replacementTab.id] = {
          root: createLeafNode(leafId),
          activeLeafId: leafId,
          expandedLeafId: null,
          sessionIdsByLeafId: { [leafId]: defaultSessionId },
        };
      }

      let activeTabId = state.activeTabId === action.tabId ? tabs[0]?.id ?? null : state.activeTabId;
      if (activeTabId && !tabs.some((tab) => tab.id === activeTabId)) activeTabId = tabs[0]?.id ?? null;

      return normalizeLayout({ tabs, activeTabId, layoutsByTabId });
    }
    case "ACTIVATE_TAB": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return state;
      return normalizeLayout({ ...state, activeTabId: action.tabId });
    }
    case "SPLIT_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const targetLeafId = action.targetLeafId ?? tabLayout.activeLeafId ?? findFirstLeafId(tabLayout.root);
      const newLeafId = action.newLeafId ?? `leaf-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
      const newRoot = splitLeaf(tabLayout.root, targetLeafId, newLeafId, action.direction, action.position, action.ratio);
      const newSessionIds = { ...tabLayout.sessionIdsByLeafId, [newLeafId]: action.sessionId };
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            root: newRoot,
            activeLeafId: newLeafId,
            sessionIdsByLeafId: newSessionIds,
          },
        },
      };
    }
    case "CLOSE_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const fallbackLeafId = findSiblingLeafId(tabLayout.root, action.leafId);
      const newRoot = removeLeaf(tabLayout.root, action.leafId);
      if (!newRoot) {
        // Last pane in tab was closed -> close tab
        return layoutReducer(state, { type: "CLOSE_TAB", tabId: action.tabId });
      }
      const newSessionIds = { ...tabLayout.sessionIdsByLeafId };
      delete newSessionIds[action.leafId];
      const nextActiveLeafId =
        tabLayout.activeLeafId === action.leafId
          ? fallbackLeafId ?? findFirstLeafId(newRoot)
          : tabLayout.activeLeafId;
      const nextExpanded = tabLayout.expandedLeafId === action.leafId ? null : tabLayout.expandedLeafId;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            root: newRoot,
            activeLeafId: nextActiveLeafId,
            expandedLeafId: nextExpanded,
            sessionIdsByLeafId: newSessionIds,
          },
        },
      };
    }
    case "FOCUS_PANE": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            activeLeafId: action.leafId,
          },
        },
      };
    }
    case "SET_PANE_RATIO": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            root: setRatioAtPath(tabLayout.root, action.path, action.ratio),
          },
        },
      };
    }
    case "SWAP_PANES": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            root: swapLeaves(tabLayout.root, action.sourceLeafId, action.targetLeafId),
          },
        },
      };
    }
    case "TOGGLE_PANE_EXPANDED": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      const isExpanded = tabLayout.expandedLeafId === action.leafId;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            expandedLeafId: isExpanded ? null : action.leafId,
          },
        },
      };
    }
    case "EQUALIZE_PANES": {
      const tabLayout = state.layoutsByTabId[action.tabId];
      if (!tabLayout) return state;
      return {
        ...state,
        layoutsByTabId: {
          ...state.layoutsByTabId,
          [action.tabId]: {
            ...tabLayout,
            root: equalizeRatios(tabLayout.root),
          },
        },
      };
    }
  }
}

function normalizeLayout(state: LayoutState): LayoutState {
  const tabs = dedupeTabs(state.tabs);
  if (tabs.length === 0) {
    return { tabs: [], activeTabId: null, layoutsByTabId: {} };
  }

  const activeTabId = tabs.some((tab) => tab.id === state.activeTabId) ? state.activeTabId : tabs[0].id;
  const layoutsByTabId: Record<string, TabPaneLayout> = {};
  for (const tab of tabs) {
    const existing = state.layoutsByTabId[tab.id];
    if (existing) {
      const leaves = collectLeafIds(existing.root);
      const activeLeafId = leaves.includes(existing.activeLeafId ?? "") ? existing.activeLeafId : leaves[0] ?? null;
      const expandedLeafId = leaves.includes(existing.expandedLeafId ?? "") ? existing.expandedLeafId : null;
      layoutsByTabId[tab.id] = {
        ...existing,
        activeLeafId,
        expandedLeafId,
      };
    } else {
      const leafId = "leaf-default";
      const defaultSessionId = tab.kind === "browser" ? "" : tab.sessionId;
      layoutsByTabId[tab.id] = {
        root: createLeafNode(leafId),
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: defaultSessionId },
      };
    }
  }

  return { tabs, activeTabId, layoutsByTabId };
}

function dedupeTabs(tabs: WorkspaceTab[]) {
  const seen = new Set<string>();
  return tabs.filter((tab) => {
    if (seen.has(tab.id)) return false;
    seen.add(tab.id);
    return true;
  });
}
