import {
  createBrowserPaneContent,
  type LayoutState,
  type PaneContent,
  type TabGroup,
  type TabGroupLayoutNode,
  type TabPaneLayout,
} from "../lib/types";
import { normalizeLayout } from "./layout";
import {
  collectLeafIds,
  createLeafNode,
  findFirstLeafId,
  splitLeafWithSubtree,
  type PaneDirection,
  type PaneNode,
} from "./paneTree";

export type TabPaneDropTarget = {
  tabId: string;
  leafId: string;
};

/**
 * Moves a terminal or browser tab's existing pane subtree into a specific leaf of another
 * tab. No terminal session or browser is created or destroyed: ownership moves
 * entirely inside the layout graph so xterm buffers, webviews, and native PTYs survive.
 */
export function moveTabIntoPaneSplit(
  inputLayout: LayoutState,
  sourceTabId: string,
  targetTabId: string,
  targetLeafId: string,
  direction: PaneDirection,
  position: "first" | "second" = "second",
  ratio = 0.5,
): LayoutState {
  const state = normalizeLayout(inputLayout);
  if (sourceTabId === targetTabId) return state;

  const sourceTab = state.tabs.find((tab) => tab.id === sourceTabId);
  const targetTab = state.tabs.find((tab) => tab.id === targetTabId);
  if (!sourceTab || !targetTab) return state;

  let sourceLayout = state.layoutsByTabId[sourceTabId];
  if (!sourceLayout && sourceTab.kind === "browser") {
    const leafId = `leaf-browser:${sourceTab.id}`;
    const content = createBrowserPaneContent({
      browserId: sourceTab.browserId,
      url: sourceTab.url,
      title: sourceTab.title,
      loading: sourceTab.loading,
      canGoBack: sourceTab.canGoBack,
      canGoForward: sourceTab.canGoForward,
      profileId: sourceTab.profileId,
      worktreePath: sourceTab.worktreePath,
      worktreeLabel: sourceTab.worktreeLabel,
    });
    sourceLayout = {
      root: createLeafNode(leafId),
      activeLeafId: leafId,
      expandedLeafId: null,
      sessionIdsByLeafId: { [leafId]: "" },
      contentsByLeafId: { [leafId]: content },
    };
  }

  const targetLayout = state.layoutsByTabId[targetTabId];
  if (!sourceLayout || !targetLayout) {
    return state;
  }

  const targetLeafIds = collectLeafIds(targetLayout.root);
  if (!targetLeafIds.includes(targetLeafId)) return state;

  const groups = { ...(state.tabGroups ?? {}) };
  const sourceGroup = findGroupContainingTab(groups, sourceTabId);
  const targetGroup = findGroupContainingTab(groups, targetTabId);
  if (!sourceGroup || !targetGroup) return state;

  // Legacy layouts can give every tab the same leaf id (for example `leaf-init`).
  // Remap only colliding source ids before the subtree is inserted so the combined
  // tree has a one-to-one leaf -> local session/browser mapping.
  const moved = remapPaneLayout(sourceLayout, new Set(targetLeafIds));
  const nextTargetRoot = splitLeafWithSubtree(
    targetLayout.root,
    targetLeafId,
    moved.root,
    direction,
    position,
    ratio,
  );
  if (nextTargetRoot === targetLayout.root) return state;

  let tabGroupLayout = state.tabGroupLayout ?? null;
  const sourceIndex = sourceGroup.tabIds.indexOf(sourceTabId);
  const remainingSourceTabIds = sourceGroup.tabIds.filter((tabId) => tabId !== sourceTabId);
  if (remainingSourceTabIds.length === 0) {
    delete groups[sourceGroup.id];
    tabGroupLayout = removeTabGroupLeaf(tabGroupLayout, sourceGroup.id);
  } else {
    groups[sourceGroup.id] = {
      ...sourceGroup,
      tabIds: remainingSourceTabIds,
      activeTabId:
        sourceGroup.activeTabId === sourceTabId || !remainingSourceTabIds.includes(sourceGroup.activeTabId ?? "")
          ? remainingSourceTabIds[Math.min(sourceIndex, remainingSourceTabIds.length - 1)] ?? remainingSourceTabIds[0]
          : sourceGroup.activeTabId,
    };
  }

  const liveTargetGroup = groups[targetGroup.id];
  if (!liveTargetGroup) return state;
  groups[targetGroup.id] = { ...liveTargetGroup, activeTabId: targetTabId };

  const layoutsByTabId = { ...state.layoutsByTabId };
  delete layoutsByTabId[sourceTabId];
  layoutsByTabId[targetTabId] = {
    ...targetLayout,
    root: nextTargetRoot,
    activeLeafId: moved.activeLeafId ?? findFirstLeafId(moved.root),
    expandedLeafId: null,
    sessionIdsByLeafId: {
      ...targetLayout.sessionIdsByLeafId,
      ...moved.sessionIdsByLeafId,
    },
    contentsByLeafId: {
      ...(targetLayout.contentsByLeafId ?? {}),
      ...(moved.contentsByLeafId ?? {}),
    },
  };

  return normalizeLayout({
    ...state,
    tabs: state.tabs.filter((tab) => tab.id !== sourceTabId),
    activeTabId: targetTabId,
    focusedGroupId: targetGroup.id,
    tabGroups: groups,
    tabGroupLayout,
    layoutsByTabId,
  });
}

function remapPaneLayout(layout: TabPaneLayout, reservedLeafIds: Set<string>): TabPaneLayout {
  const leafIdMap = new Map<string, string>();
  const visit = (node: PaneNode): PaneNode => {
    if (node.type === "leaf") {
      let nextLeafId = node.leafId;
      while (reservedLeafIds.has(nextLeafId)) nextLeafId = createMovedLeafId();
      reservedLeafIds.add(nextLeafId);
      leafIdMap.set(node.leafId, nextLeafId);
      return nextLeafId === node.leafId ? node : { ...node, leafId: nextLeafId };
    }

    const first = visit(node.first);
    const second = visit(node.second);
    return first === node.first && second === node.second ? node : { ...node, first, second };
  };

  const root = visit(layout.root);
  const sessionIdsByLeafId: Record<string, string> = {};
  for (const [leafId, sessionId] of Object.entries(layout.sessionIdsByLeafId)) {
    sessionIdsByLeafId[leafIdMap.get(leafId) ?? leafId] = sessionId;
  }
  const contentsByLeafId: Record<string, PaneContent> = {};
  for (const [leafId, content] of Object.entries(layout.contentsByLeafId ?? {})) {
    contentsByLeafId[leafIdMap.get(leafId) ?? leafId] = content;
  }

  return {
    root,
    activeLeafId: layout.activeLeafId ? (leafIdMap.get(layout.activeLeafId) ?? layout.activeLeafId) : null,
    expandedLeafId: layout.expandedLeafId ? (leafIdMap.get(layout.expandedLeafId) ?? layout.expandedLeafId) : null,
    sessionIdsByLeafId,
    contentsByLeafId,
  };
}

function findGroupContainingTab(groups: Record<string, TabGroup>, tabId: string): TabGroup | null {
  return Object.values(groups).find((group) => group.tabIds.includes(tabId)) ?? null;
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

function createMovedLeafId(): string {
  const randomPart = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `leaf:moved:${randomPart}`;
}
