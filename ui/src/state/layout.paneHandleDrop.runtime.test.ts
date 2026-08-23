import { describe, expect, it } from "vitest";

import type { LayoutState, TerminalTab } from "../lib/types";
import { getGroupForTab, layoutReducer, normalizeLayout } from "./layout";
import { moveTabIntoPaneSplit } from "./tabPaneDrop";

function sameTabLayout(): LayoutState {
  const tab: TerminalTab = { id: "tab-main", label: "main", sessionId: "session-a" };
  return normalizeLayout({
    tabs: [tab],
    activeTabId: tab.id,
    layoutsByTabId: {
      [tab.id]: {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-a" },
          second: {
            type: "split",
            direction: "vertical",
            first: { type: "leaf", leafId: "leaf-b" },
            second: { type: "leaf", leafId: "leaf-c" },
            ratio: 0.5,
          },
          ratio: 0.4,
        },
        activeLeafId: "leaf-a",
        expandedLeafId: null,
        sessionIdsByLeafId: {
          "leaf-a": "session-a",
          "leaf-b": "session-b",
          "leaf-c": "session-c",
        },
      },
    },
  });
}

describe("pane-handle edge drop composition", () => {
  it("detaches the existing pane session and reinserts that exact leaf at the target edge", () => {
    const state = sameTabLayout();
    const groupId = getGroupForTab(state, "tab-main")!.id;
    const detachedTab: TerminalTab = { id: "tab-detached", label: "main", sessionId: "session-a" };

    const detached = layoutReducer(state, {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: "tab-main",
      leafId: "leaf-a",
      newTab: detachedTab,
      targetGroupId: groupId,
    });
    const next = moveTabIntoPaneSplit(
      detached,
      detachedTab.id,
      "tab-main",
      "leaf-c",
      "vertical",
      "first",
    );

    expect(next.tabs.map((tab) => tab.id)).toEqual(["tab-main"]);
    expect(next.layoutsByTabId["tab-main"].root).toEqual({
      type: "split",
      direction: "vertical",
      first: { type: "leaf", leafId: "leaf-b" },
      second: {
        type: "split",
        direction: "vertical",
        first: { type: "leaf", leafId: "leaf-a" },
        second: { type: "leaf", leafId: "leaf-c" },
        ratio: 0.5,
      },
      ratio: 0.5,
    });
    expect(next.layoutsByTabId["tab-main"].activeLeafId).toBe("leaf-a");
    expect(next.layoutsByTabId["tab-main"].sessionIdsByLeafId).toEqual({
      "leaf-a": "session-a",
      "leaf-b": "session-b",
      "leaf-c": "session-c",
    });
  });

  it("preserves pane ownership when a handle is moved to an edge in another tab", () => {
    const source: TerminalTab = { id: "tab-source", label: "source", sessionId: "session-moved" };
    const target: TerminalTab = { id: "tab-target", label: "target", sessionId: "session-target" };
    const state = normalizeLayout({
      tabs: [source, target],
      activeTabId: source.id,
      layoutsByTabId: {
        [source.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-moved" },
            second: { type: "leaf", leafId: "leaf-remain" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-moved",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-moved": "session-moved",
            "leaf-remain": "session-remain",
          },
        },
        [target.id]: {
          root: { type: "leaf", leafId: "leaf-target" },
          activeLeafId: "leaf-target",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-target": "session-target" },
        },
      },
    });
    const targetGroupId = getGroupForTab(state, target.id)!.id;
    const detachedTab: TerminalTab = { id: "tab-detached", label: "source", sessionId: "session-moved" };

    const detached = layoutReducer(state, {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: source.id,
      leafId: "leaf-moved",
      newTab: detachedTab,
      targetGroupId,
    });
    const next = moveTabIntoPaneSplit(
      detached,
      detachedTab.id,
      target.id,
      "leaf-target",
      "horizontal",
      "second",
    );

    expect(next.tabs.find((tab) => tab.id === source.id)).toMatchObject({ sessionId: "session-remain" });
    expect(next.layoutsByTabId[source.id]).toMatchObject({
      root: { type: "leaf", leafId: "leaf-remain" },
      activeLeafId: "leaf-remain",
      sessionIdsByLeafId: { "leaf-remain": "session-remain" },
    });
    expect(next.layoutsByTabId[target.id].root).toEqual({
      type: "split",
      direction: "horizontal",
      first: { type: "leaf", leafId: "leaf-target" },
      second: { type: "leaf", leafId: "leaf-moved" },
      ratio: 0.5,
    });
    expect(next.layoutsByTabId[target.id].sessionIdsByLeafId).toEqual({
      "leaf-target": "session-target",
      "leaf-moved": "session-moved",
    });
    expect(next.layoutsByTabId[target.id].activeLeafId).toBe("leaf-moved");
  });
});
