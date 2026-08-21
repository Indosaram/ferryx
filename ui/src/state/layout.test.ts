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
    expect(state.layoutsByTabId["tab-1"]).toBeDefined();
    expect(state.layoutsByTabId["tab-1"].root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(state.layoutsByTabId["tab-1"].sessionIdsByLeafId["leaf-init"]).toBe("session-1");

    expect(state.layoutsByTabId["tab-2"]).toBeDefined();
    expect(state.layoutsByTabId["tab-2"].root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(state.layoutsByTabId["tab-2"].sessionIdsByLeafId["leaf-init"]).toBe("session-2");
  });

  it("splits any pane in a tab horizontally or vertically without affecting other tabs", () => {
    const tab1 = tab("tab-1", "session-1");
    const tab2 = tab("tab-2", "session-2");
    let state = createLayoutState([tab1, tab2], tab1.id);

    // Split tab-1 horizontally
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-right",
      sessionId: "session-1-right",
    });

    // tab-1 should now have a horizontal split
    const tab1Layout = state.layoutsByTabId["tab-1"];
    expect(tab1Layout.root).toMatchObject({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-init" },
      second: { type: "leaf", leafId: "leaf-right" },
      ratio: 0.5,
    });
    expect(tab1Layout.sessionIdsByLeafId["leaf-right"]).toBe("session-1-right");
    expect(tab1Layout.activeLeafId).toBe("leaf-right");

    // tab-2 must remain completely untouched with its own single leaf
    const tab2Layout = state.layoutsByTabId["tab-2"];
    expect(tab2Layout.root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(Object.keys(tab2Layout.sessionIdsByLeafId)).toHaveLength(1);

    // Split the right pane of tab-1 vertically (nested split on the right pane!)
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-right",
      direction: "vertical",
      newLeafId: "leaf-right-bottom",
      sessionId: "session-1-bottom",
    });

    const updatedTab1 = state.layoutsByTabId["tab-1"];
    expect(updatedTab1.root).toMatchObject({
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

    state = layoutReducer(state, {
      type: "CLOSE_PANE",
      tabId: "tab-1",
      leafId: "leaf-2",
    });

    const tab1Layout = state.layoutsByTabId["tab-1"];
    expect(tab1Layout.root).toEqual({ type: "leaf", leafId: "leaf-init" });
    expect(tab1Layout.activeLeafId).toBe("leaf-init");
    expect(tab1Layout.sessionIdsByLeafId["leaf-2"]).toBeUndefined();
  });

  it("swaps pane positions for drag-and-drop reorder", () => {
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

    state = layoutReducer(state, {
      type: "SET_PANE_RATIO",
      tabId: "tab-1",
      path: "",
      ratio: 0.7,
    });

    expect(state.layoutsByTabId["tab-1"].root).toMatchObject({
      type: "split",
      ratio: 0.7,
    });
  });
});
