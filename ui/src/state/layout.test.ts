import { describe, expect, it } from "vitest";

import type { TerminalTab } from "../lib/types";
import { createLayoutState, layoutReducer } from "./layout";

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

describe("layoutReducer with per-tab split trees", () => {
  it("initializes each tab with its own independent single-pane root layout", () => {
    const tab1 = tab("tab-1", "session-1");
    const tab2 = tab("tab-2", "session-2");
    const state = createLayoutState([tab1, tab2], tab1.id);

    expect(state.tabs).toHaveLength(2);
    expect(state.activeTabId).toBe("tab-1");
    expect(state.layoutsByTabId["tab-1"].root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(state.layoutsByTabId["tab-1"].sessionIdsByLeafId["leaf-init"]).toBe("session-1");
    expect(state.layoutsByTabId["tab-2"].root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(state.layoutsByTabId["tab-2"].sessionIdsByLeafId["leaf-init"]).toBe("session-2");
  });

  it("splits any pane in a tab horizontally or vertically without affecting other tabs", () => {
    const tab1 = tab("tab-1", "session-1");
    const tab2 = tab("tab-2", "session-2");
    let state = createLayoutState([tab1, tab2], tab1.id);

    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-right",
      sessionId: "session-1-right",
    });

    expect(state.layoutsByTabId["tab-1"].root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-init" },
      second: { type: "leaf", leafId: "leaf-right" },
      ratio: 0.5,
    });
    expect(state.layoutsByTabId["tab-1"].sessionIdsByLeafId["leaf-right"]).toBe("session-1-right");
    expect(state.layoutsByTabId["tab-1"].activeLeafId).toBe("leaf-right");
    expect(state.layoutsByTabId["tab-2"].root).toEqual({ type: "leaf", leafId: "leaf-init" });

    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-right",
      direction: "vertical",
      newLeafId: "leaf-right-bottom",
      sessionId: "session-1-bottom",
    });

    expect(state.layoutsByTabId["tab-1"].root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-init" },
      second: {
        type: "split",
        direction: "vertical",
        first: { type: "leaf", leafId: "leaf-right" },
        second: { type: "leaf", leafId: "leaf-right-bottom" },
      },
    });
  });

  it("rejects missing split targets and duplicate leaf ids without creating orphan session bindings", () => {
    const initial = createLayoutState([tab("tab-1", "session-1")], "tab-1");

    const missingTarget = layoutReducer(initial, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "missing",
      direction: "horizontal",
      newLeafId: "orphan",
      sessionId: "session-orphan",
    });
    expect(missingTarget).toBe(initial);
    expect(missingTarget.layoutsByTabId["tab-1"].sessionIdsByLeafId).toEqual({ "leaf-init": "session-1" });

    const duplicateLeaf = layoutReducer(initial, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-init",
      sessionId: "session-orphan",
    });
    expect(duplicateLeaf).toBe(initial);
  });

  it("closes an individual pane and collapses the parent split into the sibling", () => {
    const tab1 = tab("tab-1", "session-1");
    let state = createLayoutState([tab1], tab1.id);
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-2",
      sessionId: "session-2",
    });

    state = layoutReducer(state, { type: "CLOSE_PANE", tabId: "tab-1", leafId: "leaf-2" });

    const tab1Layout = state.layoutsByTabId["tab-1"];
    expect(tab1Layout.root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(tab1Layout.activeLeafId).toBe("leaf-init");
    expect(tab1Layout.sessionIdsByLeafId["leaf-2"]).toBeUndefined();
  });

  it("selects the adjacent tab when closing the active tab", () => {
    const tabs = [tab("tab-1", "s1"), tab("tab-2", "s2"), tab("tab-3", "s3"), tab("tab-4", "s4")];
    let state = createLayoutState(tabs, "tab-2");

    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: "tab-2" });
    expect(state.activeTabId).toBe("tab-3");

    state = layoutReducer({ ...state, activeTabId: "tab-4" }, { type: "CLOSE_TAB", tabId: "tab-4" });
    expect(state.activeTabId).toBe("tab-3");
  });

  it("reorders tabs by stable tab id without disturbing active tab or pane layouts", () => {
    const state = createLayoutState([tab("tab-1", "s1"), tab("tab-2", "s2"), tab("tab-3", "s3")], "tab-2");
    const layouts = state.layoutsByTabId;

    const reordered = layoutReducer(state, { type: "REORDER_TAB", tabId: "tab-1", targetIndex: 2 });

    expect(reordered.tabs.map((item) => item.id)).toEqual(["tab-2", "tab-3", "tab-1"]);
    expect(reordered.activeTabId).toBe("tab-2");
    expect(reordered.layoutsByTabId).toBe(layouts);
  });

  it("renames and pins tabs in layout state while rejecting blank titles", () => {
    let state = createLayoutState([tab("tab-1", "s1")], "tab-1");
    state = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "  custom title  " });
    state = layoutReducer(state, { type: "SET_TAB_PINNED", tabId: "tab-1", pinned: true });

    expect(state.tabs[0]).toMatchObject({ label: "custom title", pinned: true });
    const unchanged = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "   " });
    expect(unchanged).toBe(state);
  });

  it("ignores focus requests for leaves that do not exist", () => {
    const state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    const focused = layoutReducer(state, { type: "FOCUS_PANE", tabId: "tab-1", leafId: "missing" });
    expect(focused).toBe(state);
    expect(focused.layoutsByTabId["tab-1"].activeLeafId).toBe("leaf-init");
  });

  it("normalizes stale pane session mappings to the actual leaf set", () => {
    const state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    const malformed = {
      ...state,
      layoutsByTabId: {
        ...state.layoutsByTabId,
        "tab-1": {
          ...state.layoutsByTabId["tab-1"],
          sessionIdsByLeafId: { "leaf-init": "session-1", ghost: "session-ghost" },
        },
      },
    };

    const normalized = layoutReducer(malformed, { type: "ACTIVATE_TAB", tabId: "tab-1" });
    expect(normalized.layoutsByTabId["tab-1"].sessionIdsByLeafId).toEqual({ "leaf-init": "session-1" });
  });

  it("swaps pane positions for drag-and-drop reorder", () => {
    let state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-2",
      sessionId: "session-2",
    });

    state = layoutReducer(state, {
      type: "SWAP_PANES",
      tabId: "tab-1",
      sourceLeafId: "leaf-init",
      targetLeafId: "leaf-2",
    });

    expect(state.layoutsByTabId["tab-1"].root).toMatchObject({
      type: "split",
      first: { type: "leaf", leafId: "leaf-2" },
      second: { type: "leaf", leafId: "leaf-init" },
    });
  });

  it("adjusts split ratio at dot path", () => {
    let state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-2",
      sessionId: "session-2",
    });

    state = layoutReducer(state, { type: "SET_PANE_RATIO", tabId: "tab-1", path: "", ratio: 0.7 });
    expect(state.layoutsByTabId["tab-1"].root).toMatchObject({ type: "split", ratio: 0.7 });
  });
});
