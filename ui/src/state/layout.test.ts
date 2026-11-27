import { describe, expect, it } from "vitest";

import type { BrowserTab, FileTab, LayoutState, TabGroupLayoutNode, TerminalTab } from "../lib/types";
import { createTerminalPaneContent } from "../lib/types";
import { createLayoutState, defaultContentForTab, focusedPaneSessionId, layoutReducer, normalizeLayout } from "./layout";
import { computeLeafRects, findLeaf, resolveSeam } from "./paneTree";

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

it("stores file rename intent separately and carries it into a pane", () => {
  const file: FileTab = {
    kind: "file", id: "file", label: "index.ts", path: "/repo/src/index.ts",
    backendSessionId: "backend", line: null, col: null, workspaceId: null, previewId: "preview",
  };
  const initial = createLayoutState([file], file.id);
  const renamed = layoutReducer(initial, { type: "RENAME_TAB", tabId: file.id, label: "src/index.ts" });
  expect(renamed.tabs[0]).toMatchObject({ label: "index.ts", customLabel: "src/index.ts" });
  expect(defaultContentForTab(renamed.tabs[0])).toMatchObject({ customLabel: "src/index.ts" });
  const cleared = layoutReducer(renamed, { type: "RENAME_TAB", tabId: file.id, label: "" });
  expect(cleared.tabs[0]).not.toHaveProperty("customLabel");
  expect(cleared.tabs[0].label).toBe("index.ts");
});

describe("pane view", () => {
  it("creates a chat tab in chat view on every reduction", () => {
    const initial = createLayoutState();
    for (let reduction = 0; reduction < 2; reduction += 1) {
      const next = layoutReducer(initial, {
        type: "ADD_TAB", tab: tab("chat", "session-chat"), sessionId: "session-chat", view: "chat",
      });
      const pane = next.layoutsByTabId.chat;
      expect(pane.root).toMatchObject({ type: "leaf", view: "chat" });
      expect(Object.values(pane.sessionIdsByLeafId)).toEqual(["session-chat"]);
    }
  });

  it("changes the presentation without replacing session bindings", () => {
    const initial = createLayoutState([tab("a", "session-a")], "a");
    const pane = initial.layoutsByTabId.a;
    if (!pane.activeLeafId) throw new Error("expected active pane");
    const next = layoutReducer(initial, {
      type: "SET_PANE_VIEW", tabId: "a", leafId: pane.activeLeafId, view: "chat",
    });
    expect(findLeaf(next.layoutsByTabId.a.root, pane.activeLeafId)?.view).toBe("chat");
    expect(next.layoutsByTabId.a.sessionIdsByLeafId).toBe(pane.sessionIdsByLeafId);
    expect(next.tabs).toBe(initial.tabs);
  });

  it("retains the selected view when a chat split becomes a separate tab", () => {
    const initial = createLayoutState([tab("a", "session-a")], "a");
    const split = layoutReducer(initial, {
      type: "SPLIT_PANE", tabId: "a", direction: "horizontal", newLeafId: "chat-leaf",
      sessionId: "session-chat", view: "chat",
    });
    const detached = layoutReducer(split, {
      type: "DETACH_PANE_TO_TAB", sourceTabId: "a", leafId: "chat-leaf", newTab: tab("b", "session-chat"),
    });
    expect(detached.layoutsByTabId.b.root).toEqual({ type: "leaf", leafId: "chat-leaf", view: "chat" });
    expect(detached.layoutsByTabId.b.sessionIdsByLeafId["chat-leaf"]).toBe("session-chat");
    expect(findLeaf(detached.layoutsByTabId.a.root, "chat-leaf")).toBeNull();
  });
});

describe("divider run equalization", () => {
  it("gives five nested horizontal tab groups equal widths from any divider", () => {
    const groups: TabGroupLayoutNode = {
      type: "split", direction: "horizontal", ratio: 0.32,
      first: { type: "group", groupId: "a" },
      second: {
        type: "split", direction: "horizontal", ratio: 0.44,
        first: { type: "group", groupId: "b" },
        second: {
          type: "split", direction: "horizontal", ratio: 0.55,
          first: { type: "group", groupId: "c" },
          second: {
            type: "split", direction: "horizontal", ratio: 0.7,
            first: { type: "group", groupId: "d" },
            second: { type: "group", groupId: "e" },
          },
        },
      },
    };
    const initial = createLayoutState(["a", "b", "c", "d", "e"].map((id) => tab(id, id)), "a");
    for (const path of ["", "second", "second.second", "second.second.second"]) {
      const state = layoutReducer({
        ...initial,
        tabGroups: Object.fromEntries(["a", "b", "c", "d", "e"].map((id) => [id, { id, tabIds: [id], activeTabId: id }])),
        tabGroupLayout: groups,
      }, { type: "EQUALIZE_TAB_GROUP_RUN", path });
      const widths: number[] = [];
      const visit = (node: TabGroupLayoutNode, width: number) => {
        if (node.type === "group") { widths.push(width); return; }
        visit(node.first, width * node.ratio);
        visit(node.second, width * (1 - node.ratio));
      };
      if (!state.tabGroupLayout) throw new Error("missing groups");
      visit(state.tabGroupLayout, 1);
      expect(widths).toHaveLength(5);
      for (const width of widths) expect(width).toBeCloseTo(0.2, 6);
    }
    expect(groups.ratio).toBe(0.32);
  });

  it("equalizes only the selected axis run inside perpendicular splits", () => {
    const initial = createLayoutState([tab("a", "a")], "a");
    const root = {
      type: "split" as const, direction: "vertical" as const, ratio: 0.65,
      first: {
        type: "split" as const, direction: "horizontal" as const, ratio: 0.7,
        first: { type: "leaf" as const, leafId: "a" },
        second: { type: "leaf" as const, leafId: "b" },
      },
      second: {
        type: "split" as const, direction: "horizontal" as const, ratio: 0.2,
        first: { type: "leaf" as const, leafId: "c" },
        second: { type: "leaf" as const, leafId: "d" },
      },
    };
    const state = layoutReducer({
      ...initial,
      layoutsByTabId: { a: { ...initial.layoutsByTabId.a, root } },
    }, { type: "EQUALIZE_PANE_RUN", tabId: "a", path: "first" });
    const next = state.layoutsByTabId.a.root;
    const rects = computeLeafRects(next);
    expect(rects.get("a")?.w).toBeCloseTo(0.5);
    expect(rects.get("b")?.w).toBeCloseTo(0.5);
    expect(rects.get("c")?.w).toBeCloseTo(0.2);
    expect(next.type === "split" ? next.ratio : null).toBe(0.65);
  });
});

describe("file preview tabs", () => {
  it("updates only matching preview identities in a mixed file split", () => {
    const file: FileTab = {
      kind: "file", id: "file", label: "a.txt", path: "/cache/a.txt",
      backendSessionId: "backend", line: 1, col: null, workspaceId: null, previewId: "preview-a",
    };
    const other = defaultContentForTab({ ...file, id: "other", path: "/cache/b.txt", previewId: "preview-b" });
    const initial = createLayoutState([file], file.id);
    initial.layoutsByTabId.file = {
      ...initial.layoutsByTabId.file,
      root: {
        type: "split", direction: "horizontal", ratio: 0.5,
        first: { type: "leaf", leafId: "leaf-init" }, second: { type: "leaf", leafId: "other" },
      },
      contentsByLeafId: { "leaf-init": defaultContentForTab(file), other },
      sessionIdsByLeafId: { "leaf-init": "", other: "" },
    };
    const updated = layoutReducer(initial, {
      type: "UPDATE_FILE_TAB", tabId: "file", path: "/cache/new-a.txt", line: 9, col: 2,
      remote: { workspaceId: "ssh:host", path: "/srv/a.txt", sessionId: null },
    });
    expect(updated.layoutsByTabId.file.contentsByLeafId?.["leaf-init"]).toMatchObject({
      previewId: "preview-a", path: "/cache/new-a.txt", line: 9, col: 2,
    });
    expect(updated.layoutsByTabId.file.contentsByLeafId?.other).toEqual(other);
  });

  it("returns to the previously active tab after closing a newly opened file tab", () => {
    const fileTab: FileTab = {
      kind: "file", id: "file", label: "readme.md", path: "/repo/readme.md",
      backendSessionId: "backend-1", line: null, col: null,
      workspaceId: "ws-1", previewId: "preview-1",
    };
    let state = createLayoutState([tab("left", "s1"), tab("middle", "s2"), tab("right", "s3")], "middle");
    state = layoutReducer(state, { type: "ADD_TAB", tab: fileTab });

    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: fileTab.id });

    expect(state.activeTabId).toBe("middle");
    expect(state.tabGroups?.[state.focusedGroupId ?? ""]?.activeTabId).toBe("middle");
  });

  it("returns to the previously active tab after closing a newly opened browser tab", () => {
    const browserTab: BrowserTab = {
      kind: "browser", id: "browser", label: "Browser", browserId: "browser-1",
      url: "about:blank", title: "Browser", loading: false, canGoBack: false,
      canGoForward: false, zoomFactor: 1, loadError: null,
    };
    let state = createLayoutState([tab("left", "s1"), tab("middle", "s2"), tab("right", "s3")], "middle");
    state = layoutReducer(state, { type: "ADD_TAB", tab: browserTab });

    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: browserTab.id });

    expect(state.activeTabId).toBe("middle");
  });

  it("falls back to an adjacent tab if the previous tab was closed first", () => {
    const fileTab: FileTab = {
      kind: "file", id: "file", label: "readme.md", path: "/repo/readme.md",
      backendSessionId: "backend-1", line: null, col: null,
      workspaceId: "ws-1", previewId: "preview-1",
    };
    let state = createLayoutState([tab("left", "s1"), tab("middle", "s2"), tab("right", "s3")], "middle");
    state = layoutReducer(state, { type: "ADD_TAB", tab: fileTab });
    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: "middle" });

    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: fileTab.id });

    expect(state.activeTabId).toBe("right");
  });

  it("returns through nested browser and file tabs in opening order", () => {
    const browserTab: BrowserTab = {
      kind: "browser", id: "browser", label: "Browser", browserId: "browser-1",
      url: "about:blank", title: "Browser", loading: false, canGoBack: false,
      canGoForward: false, zoomFactor: 1, loadError: null,
    };
    const fileTab: FileTab = {
      kind: "file", id: "file", label: "readme.md", path: "/repo/readme.md",
      backendSessionId: "backend-1", line: null, col: null,
      workspaceId: "ws-1", previewId: "preview-1",
    };
    let state = createLayoutState([tab("left", "s1"), tab("middle", "s2"), tab("right", "s3")], "middle");
    state = layoutReducer(state, { type: "ADD_TAB", tab: browserTab });
    state = layoutReducer(state, { type: "ADD_TAB", tab: fileTab });

    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: fileTab.id });
    expect(state.activeTabId).toBe(browserTab.id);
    state = layoutReducer(state, { type: "CLOSE_TAB", tabId: browserTab.id });
    expect(state.activeTabId).toBe("middle");
  });

  it("uses a file pane, not a terminal session, as the default content", () => {
    const fileTab: FileTab = {
      kind: "file",
      id: "tab-file",
      label: "readme.md",
      path: "/repo/readme.md",
      backendSessionId: "backend-1",
      line: 4,
      col: 2,
      workspaceId: "ws-1",
      previewId: "preview-readme",
    };
    const content = defaultContentForTab(fileTab);
    expect(content.kind).toBe("file");
    if (content.kind !== "file") return;
    expect(content.path).toBe("/repo/readme.md");
    expect("sessionId" in content).toBe(false);

    const state = layoutReducer(createLayoutState(), { type: "ADD_TAB", tab: fileTab });
    const leafId = Object.keys(state.layoutsByTabId["tab-file"].contentsByLeafId ?? {})[0];
    expect(state.layoutsByTabId["tab-file"].contentsByLeafId?.[leafId!]?.kind).toBe("file");
    expect(state.layoutsByTabId["tab-file"].sessionIdsByLeafId[leafId!]).toBe("");
  });

  it("focuses the group that owns a reused file tab", () => {
    const fileTab: FileTab = {
      kind: "file",
      id: "tab-file",
      label: "readme.md",
      path: "/repo/readme.md",
      backendSessionId: "backend-1",
      line: 4,
      col: 2,
      workspaceId: "ws-1",
      previewId: "preview-readme",
    };
    const terminal: TerminalTab = { id: "tab-term", label: "shell", sessionId: "session-1" };
    let state = layoutReducer(createLayoutState(), { type: "ADD_TAB", tab: terminal });
    state = layoutReducer(state, { type: "ADD_TAB", tab: fileTab });
    state = layoutReducer(state, { type: "ACTIVATE_TAB", tabId: "tab-term" });
    state = layoutReducer(state, {
      type: "UPDATE_FILE_TAB",
      tabId: "tab-file",
      line: 9,
      col: 1,
    });
    expect(state.activeTabId).toBe("tab-file");
    expect(state.focusedGroupId).toBeTruthy();
    const group = state.tabGroups?.[state.focusedGroupId!];
    expect(group?.activeTabId).toBe("tab-file");
  });
});

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

    // A terminal rename is stored beside the automatic label so live titles never overwrite it.
    expect(state.tabs[0]).toMatchObject({ customLabel: "custom title", pinned: true });
    expect(state.tabs[0].label).toBe("tab-1");
    const unchanged = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "custom title" });
    expect(unchanged).toBe(state);
  });

  it("clears a terminal tab's custom label on a blank rename so the tab returns to automatic", () => {
    let state = createLayoutState([tab("tab-1", "s1")], "tab-1");
    const untouched = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "   " });
    expect(untouched).toBe(state);

    state = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "build watcher" });
    state = layoutReducer(state, { type: "RENAME_TAB", tabId: "tab-1", label: "   " });

    expect(state.tabs[0]).not.toHaveProperty("customLabel");
    expect(state.tabs[0].label).toBe("tab-1");
  });

  it("renames browser tabs through their label and still rejects blank browser titles", () => {
    const browser: BrowserTab = { id: "browser-1", kind: "browser", label: "Browser", browserId: "b1", url: "https://example.com" };
    let state = createLayoutState([tab("tab-1", "s1"), browser], "browser-1");
    state = layoutReducer(state, { type: "RENAME_TAB", tabId: "browser-1", label: " Docs " });

    expect(state.tabs[1]).toMatchObject({ label: "Docs" });
    expect(state.tabs[1]).not.toHaveProperty("customLabel");
    expect(layoutReducer(state, { type: "RENAME_TAB", tabId: "browser-1", label: " " })).toBe(state);
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

  it("synchronizes collinear split ratios in a 2x2 pane grid", () => {
    let state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-right",
      sessionId: "session-right",
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "vertical",
      newLeafId: "leaf-left-bottom",
      sessionId: "session-left-bottom",
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-right",
      direction: "vertical",
      newLeafId: "leaf-right-bottom",
      sessionId: "session-right-bottom",
    });

    state = layoutReducer(state, {
      type: "SET_PANE_RATIO",
      tabId: "tab-1",
      path: "first",
      ratio: 0.65,
    });

    const root = state.layoutsByTabId["tab-1"].root;
    if (root.type !== "split") throw new Error("expected split root");
    if (root.first.type !== "split" || root.second.type !== "split") {
      throw new Error("expected child splits");
    }
    expect(root.first.ratio).toBe(0.65);
    expect(root.second.ratio).toBe(0.65);
  });

  it("inherits aligned split ratio when splitting sibling pane", () => {
    let state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-right",
      sessionId: "session-right",
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "vertical",
      newLeafId: "leaf-left-bottom",
      sessionId: "session-left-bottom",
      ratio: 0.7,
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-right",
      direction: "vertical",
      newLeafId: "leaf-right-bottom",
      sessionId: "session-right-bottom",
    });

    const root = state.layoutsByTabId["tab-1"].root;
    if (root.type !== "split") throw new Error("expected split root");
    if (root.second.type !== "split") throw new Error("expected second split");
    expect(root.second.ratio).toBe(0.7);
  });

  it("applies updates via pre-resolved frozen seam without mid-drag capture", () => {
    let state = createLayoutState([tab("tab-1", "session-1")], "tab-1");
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-right",
      sessionId: "session-right",
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-init",
      direction: "vertical",
      newLeafId: "leaf-left-bottom",
      sessionId: "session-left-bottom",
      ratio: 0.3,
    });
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: "tab-1",
      targetLeafId: "leaf-right",
      direction: "vertical",
      newLeafId: "leaf-right-bottom",
      sessionId: "session-right-bottom",
      ratio: 0.6,
    });

    const frozenSeam = resolveSeam(state.layoutsByTabId["tab-1"].root, "first");
    expect(frozenSeam?.members.map((m) => m.path)).toEqual(["first"]);

    state = layoutReducer(state, {
      type: "SET_PANE_RATIO",
      tabId: "tab-1",
      path: "first",
      ratio: 0.61,
      seam: frozenSeam,
    });

    const root = state.layoutsByTabId["tab-1"].root;
    if (root.type !== "split" || root.first.type !== "split" || root.second.type !== "split") {
      throw new Error("expected split tree");
    }
    expect(root.first.ratio).toBe(0.61);
    expect(root.second.ratio).toBe(0.6);
  });
});

describe("focusedPaneSessionId", () => {
  it("(a) returns the second leaf session when activeLeafId is the second leaf in a two-leaf layout", () => {
    const terminal = tab("tab-term", "session-init");
    let state = createLayoutState([terminal], terminal.id);
    state = layoutReducer(state, {
      type: "SPLIT_PANE",
      tabId: terminal.id,
      targetLeafId: "leaf-init",
      direction: "horizontal",
      newLeafId: "leaf-second",
      sessionId: "session-second",
    });
    state = layoutReducer(state, {
      type: "FOCUS_PANE",
      tabId: terminal.id,
      leafId: "leaf-second",
    });

    expect(state.layoutsByTabId[terminal.id].activeLeafId).toBe("leaf-second");
    expect(focusedPaneSessionId(state, terminal)).toBe("session-second");
  });

  it("(b) returns tab.sessionId when a terminal tab has no entry in layoutsByTabId", () => {
    const terminal = tab("tab-term", "session-fallback");
    const emptyState = createLayoutState();

    expect(emptyState.layoutsByTabId[terminal.id]).toBeUndefined();
    expect(focusedPaneSessionId(emptyState, terminal)).toBe("session-fallback");
  });

  it("(c) returns null for a browser tab", () => {
    const browser: BrowserTab = {
      kind: "browser",
      id: "tab-browser",
      label: "Docs",
      url: "https://example.com",
      browserId: "b-1",
    };
    const state = createLayoutState([browser], browser.id);

    expect(focusedPaneSessionId(state, browser)).toBeNull();
  });
});

describe("normalizeLayout repairs a corrupted LayoutState", () => {
  function singleTab(id: string): TerminalTab {
    return tab(id, `session-${id}`);
  }

  function paneLayoutFor(id: string) {
    const leafId = `leaf-${id}`;
    return {
      root: { type: "leaf" as const, leafId },
      activeLeafId: leafId,
      expandedLeafId: null,
      sessionIdsByLeafId: { [leafId]: `session-${id}` },
      contentsByLeafId: { [leafId]: createTerminalPaneContent(`session-${id}`) },
    };
  }

  it("drops surplus layouts, dangling leaf ids, and duplicate tab ids", () => {
    const base = createLayoutState([singleTab("t1")], "t1");
    const corrupted: LayoutState = {
      ...base,
      // A duplicated tab row survives a stale merge; the reducer keeps one.
      tabs: [singleTab("t1"), singleTab("t1")],
      layoutsByTabId: {
        ...base.layoutsByTabId,
        // A layout for a tab that no longer exists.
        ghost: paneLayoutFor("ghost"),
        // A focus and a zoom that name leaves the tree does not have.
        t1: { ...base.layoutsByTabId.t1, activeLeafId: "missing", expandedLeafId: "missing" },
      },
    };

    const repaired = normalizeLayout(corrupted);

    expect(repaired.tabs.map((candidate) => candidate.id)).toEqual(["t1"]);
    expect(Object.keys(repaired.layoutsByTabId)).toEqual(["t1"]);
    expect(repaired.layoutsByTabId.t1.activeLeafId).toBe("leaf-init");
    expect(repaired.layoutsByTabId.t1.expandedLeafId).toBeNull();
    // A repaired state must satisfy the invariant, or every later reduction pays for it again.
    expect(normalizeLayout(repaired)).toBe(repaired);
  });

  it("realigns activeTabId with the focused group's own active tab", () => {
    const state: LayoutState = {
      tabs: [singleTab("t1"), singleTab("t2")],
      // t2 is not in the focused group, so the pair disagrees.
      activeTabId: "t2",
      layoutsByTabId: { t1: paneLayoutFor("t1"), t2: paneLayoutFor("t2") },
      tabGroups: {
        G1: { id: "G1", tabIds: ["t1"], activeTabId: "t1" },
        G2: { id: "G2", tabIds: ["t2"], activeTabId: "t2" },
      },
      tabGroupLayout: {
        type: "split", direction: "horizontal", ratio: 0.5,
        first: { type: "group", groupId: "G1" },
        second: { type: "group", groupId: "G2" },
      },
      focusedGroupId: "G1",
    };

    const repaired = normalizeLayout(state);

    expect(repaired.activeTabId).toBe("t1");
    expect(repaired.tabGroups?.G1.activeTabId).toBe("t1");
    expect(normalizeLayout(repaired)).toBe(repaired);
  });

  it("repairs a group split tree that names a group no longer present", () => {
    const state: LayoutState = {
      tabs: [singleTab("t1")],
      activeTabId: "t1",
      layoutsByTabId: { t1: paneLayoutFor("t1") },
      tabGroups: { G1: { id: "G1", tabIds: ["t1"], activeTabId: "t1" } },
      // `G-gone` has no entry in tabGroups.
      tabGroupLayout: {
        type: "split", direction: "vertical", ratio: 0.5,
        first: { type: "group", groupId: "G1" },
        second: { type: "group", groupId: "G-gone" },
      },
      focusedGroupId: "G1",
    };

    const repaired = normalizeLayout(state);

    expect(repaired.tabGroupLayout).toEqual({ type: "group", groupId: "G1" });
    expect(normalizeLayout(repaired)).toBe(repaired);
  });
});

describe("CLOSE_TAB in a multi-group layout", () => {
  function multiGroupState(): LayoutState {
    const ids = ["t1", "t2", "t3", "t4", "t5"];
    const layoutsByTabId = Object.fromEntries(ids.map((id) => {
      const leafId = `leaf-${id}`;
      return [id, {
        root: { type: "leaf" as const, leafId },
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: `session-${id}` },
        contentsByLeafId: { [leafId]: createTerminalPaneContent(`session-${id}`) },
      }];
    }));
    return {
      tabs: ids.map((id) => tab(id, `session-${id}`)),
      activeTabId: "t5",
      layoutsByTabId,
      tabGroups: {
        G1: { id: "G1", tabIds: ["t1", "t2"], activeTabId: "t2" },
        G2: { id: "G2", tabIds: ["t3", "t4", "t5"], activeTabId: "t5" },
      },
      tabGroupLayout: {
        type: "split", direction: "horizontal", ratio: 0.5,
        first: { type: "group", groupId: "G1" },
        second: { type: "group", groupId: "G2" },
      },
      focusedGroupId: "G2",
    };
  }

  it("selects the group-local neighbor instead of the next tab in flat order", () => {
    const next = layoutReducer(multiGroupState(), { type: "CLOSE_TAB", tabId: "t5" });

    expect(next.activeTabId).toBe("t4");
    expect(next.focusedGroupId).toBe("G2");
    expect(next.tabGroups?.G2).toMatchObject({ tabIds: ["t3", "t4"], activeTabId: "t4" });
    expect(next.tabGroups?.G1).toMatchObject({ tabIds: ["t1", "t2"], activeTabId: "t2" });
  });

  it("honors a return tab that lives in another group", () => {
    const base = multiGroupState();
    const state: LayoutState = {
      ...base,
      activeTabId: "t3",
      tabGroups: { ...base.tabGroups, G2: { ...base.tabGroups!.G2, activeTabId: "t3" } },
      returnTabIdByTabId: { t3: "t1" },
    };

    const next = layoutReducer(state, { type: "CLOSE_TAB", tabId: "t3" });

    expect(next.activeTabId).toBe("t1");
    expect(next.focusedGroupId).toBe("G1");
    expect(next.tabGroups?.G2.tabIds).toEqual(["t4", "t5"]);
  });

  it("drops return entries that named the closed tab as their target", () => {
    const base = multiGroupState();
    const state: LayoutState = {
      ...base,
      // `t4` was opened from `t5`; closing `t5` must not leave `t4` pointing at a dead tab.
      returnTabIdByTabId: { t4: "t5", t5: "t1" },
    };

    const next = layoutReducer(state, { type: "CLOSE_TAB", tabId: "t5" });

    expect(next.returnTabIdByTabId ?? {}).toEqual({});
  });

  it("falls back to another group when the closing group empties", () => {
    const state: LayoutState = {
      tabs: [tab("t1", "session-t1"), tab("t2", "session-t2"), tab("t3", "session-t3")],
      activeTabId: "t3",
      layoutsByTabId: {
        t1: {
          root: { type: "leaf", leafId: "leaf-t1" }, activeLeafId: "leaf-t1", expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-t1": "session-t1" },
        },
        t2: {
          root: { type: "leaf", leafId: "leaf-t2" }, activeLeafId: "leaf-t2", expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-t2": "session-t2" },
        },
        t3: {
          root: { type: "leaf", leafId: "leaf-t3" }, activeLeafId: "leaf-t3", expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-t3": "session-t3" },
        },
      },
      tabGroups: {
        G1: { id: "G1", tabIds: ["t1", "t2"], activeTabId: "t2" },
        G2: { id: "G2", tabIds: ["t3"], activeTabId: "t3" },
      },
      tabGroupLayout: {
        type: "split", direction: "horizontal", ratio: 0.5,
        first: { type: "group", groupId: "G2" },
        second: { type: "group", groupId: "G1" },
      },
      focusedGroupId: "G2",
    };

    const next = layoutReducer(state, { type: "CLOSE_TAB", tabId: "t3" });

    // The whole group is gone, so the surviving group takes focus at its own active tab.
    expect(next.tabGroups?.G2).toBeUndefined();
    expect(next.activeTabId).toBe("t2");
    expect(next.focusedGroupId).toBe("G1");
  });
});

