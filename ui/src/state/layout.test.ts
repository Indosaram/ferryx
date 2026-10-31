import { describe, expect, it } from "vitest";

import type { BrowserTab, FileTab, TabGroupLayoutNode, TerminalTab } from "../lib/types";
import { createLayoutState, defaultContentForTab, focusedPaneSessionId, layoutReducer } from "./layout";
import { computeLeafRects, resolveSeam } from "./paneTree";

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

