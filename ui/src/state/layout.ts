import type { LayoutState, SplitMode, TerminalTab } from "../lib/types";

export type LayoutAction =
  | { type: "ADD_TAB"; tab: TerminalTab; activate?: boolean }
  | { type: "CLOSE_TAB"; tabId: string; replacementTab?: TerminalTab }
  | { type: "ACTIVATE_PRIMARY"; tabId: string }
  | { type: "ACTIVATE_SECONDARY"; tabId: string }
  | {
      type: "ENABLE_SPLIT";
      orientation: Exclude<SplitMode, "none">;
      secondaryTabId?: string;
      secondaryTab?: TerminalTab;
    }
  | { type: "ROTATE_SPLIT" }
  | { type: "DISABLE_SPLIT" };

export function createLayoutState(tabs: TerminalTab[] = [], primaryTabId?: string | null): LayoutState {
  return normalizeLayout({
    tabs,
    primaryTabId: primaryTabId ?? tabs[0]?.id ?? null,
    secondaryTabId: null,
    split: "none",
  });
}

export function layoutReducer(state: LayoutState, action: LayoutAction): LayoutState {
  switch (action.type) {
    case "ADD_TAB": {
      const tabs = state.tabs.some((tab) => tab.id === action.tab.id) ? state.tabs : [...state.tabs, action.tab];
      return normalizeLayout({
        ...state,
        tabs,
        primaryTabId: action.activate === false ? state.primaryTabId : action.tab.id,
      });
    }
    case "CLOSE_TAB": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return normalizeLayout(state);

      let tabs = state.tabs.filter((tab) => tab.id !== action.tabId);
      if (tabs.length === 0 && action.replacementTab) tabs = [action.replacementTab];

      let primaryTabId = state.primaryTabId === action.tabId ? tabs[0]?.id ?? null : state.primaryTabId;
      if (primaryTabId && !tabs.some((tab) => tab.id === primaryTabId)) primaryTabId = tabs[0]?.id ?? null;

      let secondaryTabId = state.secondaryTabId === action.tabId ? null : state.secondaryTabId;
      if (secondaryTabId && !tabs.some((tab) => tab.id === secondaryTabId)) secondaryTabId = null;

      let split = state.split;
      if (split !== "none") {
        if (!secondaryTabId || secondaryTabId === primaryTabId) {
          secondaryTabId = tabs.find((tab) => tab.id !== primaryTabId)?.id ?? null;
        }
        if (!secondaryTabId) split = "none";
      }

      return normalizeLayout({ tabs, primaryTabId, secondaryTabId, split });
    }
    case "ACTIVATE_PRIMARY": {
      if (!state.tabs.some((tab) => tab.id === action.tabId)) return state;
      return normalizeLayout({ ...state, primaryTabId: action.tabId });
    }
    case "ACTIVATE_SECONDARY": {
      if (state.split === "none" || !state.tabs.some((tab) => tab.id === action.tabId)) return state;
      return normalizeLayout({ ...state, secondaryTabId: action.tabId });
    }
    case "ENABLE_SPLIT": {
      const tabs = action.secondaryTab && !state.tabs.some((tab) => tab.id === action.secondaryTab?.id)
        ? [...state.tabs, action.secondaryTab]
        : state.tabs;
      const primaryTabId = state.primaryTabId ?? tabs[0]?.id ?? null;
      const requestedSecondaryId = action.secondaryTab?.id ?? action.secondaryTabId ?? null;
      const requestedExists = !!requestedSecondaryId && tabs.some((tab) => tab.id === requestedSecondaryId);
      const mirrorRequested = requestedExists && requestedSecondaryId === primaryTabId && tabs.length === 1;
      const secondaryTabId = mirrorRequested
        ? primaryTabId
        : requestedExists && requestedSecondaryId !== primaryTabId
          ? requestedSecondaryId
          : tabs.find((tab) => tab.id !== primaryTabId)?.id ?? null;

      return normalizeLayout({
        tabs,
        primaryTabId,
        secondaryTabId,
        split: secondaryTabId ? action.orientation : "none",
      });
    }
    case "ROTATE_SPLIT":
      return normalizeLayout({
        ...state,
        split: state.split === "horizontal" ? "vertical" : state.split === "vertical" ? "horizontal" : "none",
      });
    case "DISABLE_SPLIT":
      return normalizeLayout({ ...state, split: "none", secondaryTabId: null });
  }
}

function normalizeLayout(state: LayoutState): LayoutState {
  const tabs = dedupeTabs(state.tabs);
  if (tabs.length === 0) {
    return { tabs: [], primaryTabId: null, secondaryTabId: null, split: "none" };
  }

  const primaryTabId = tabs.some((tab) => tab.id === state.primaryTabId) ? state.primaryTabId : tabs[0].id;
  let split = state.split;
  let secondaryTabId = tabs.some((tab) => tab.id === state.secondaryTabId) ? state.secondaryTabId : null;

  const isMirror = split !== "none" && tabs.length === 1 && secondaryTabId === primaryTabId;
  if (secondaryTabId === primaryTabId && !isMirror) {
    secondaryTabId = tabs.find((tab) => tab.id !== primaryTabId)?.id ?? null;
  }

  if (split !== "none" && !secondaryTabId) {
    secondaryTabId = tabs.find((tab) => tab.id !== primaryTabId)?.id ?? null;
  }

  if (split !== "none" && !secondaryTabId) split = "none";
  if (split === "none") secondaryTabId = null;

  return { tabs, primaryTabId, secondaryTabId, split };
}

function dedupeTabs(tabs: TerminalTab[]) {
  const seen = new Set<string>();
  return tabs.filter((tab) => {
    if (seen.has(tab.id)) return false;
    seen.add(tab.id);
    return true;
  });
}
