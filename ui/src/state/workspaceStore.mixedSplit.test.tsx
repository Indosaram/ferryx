import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
}

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as browserTauri from "../lib/browserTauri";
import { saveBrowserSettings } from "../lib/browserSettings";
import {
  deserializeWorkspaceState,
  serializeWorkspaceState,
} from "../lib/sessionPersistence";
import {
  createBrowserPaneContent,
  createFilePaneContent,
  type BrowserTab,
  type TerminalSession,
  type TerminalTab,
  type Worktree,
} from "../lib/types";
import {
  focusedPaneSessionId,
  layoutReducer,
  normalizeLayout,
} from "./layout";
import {
  selectTabActivitySummaries,
  selectWorktreeActivitySummaries,
  useWorkspaceStore,
  workspaceReducer,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";

afterEach(cleanup);

const worktree: Worktree = {
  path: "/repo/main",
  head: "head-main",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function terminalSession(id: string): TerminalSession {
  return {
    id,
    cwd: worktree.path,
    worktreePath: worktree.path,
    workspaceId: "workspace-1",
    worktree: null,
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "backend-unused"),
    getTerminalCwd: vi.fn(async () => null),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

/** A browser tab that hosts a terminal pane in a horizontal split. */
function mixedBrowserTabState(overrides?: Partial<WorkspaceState>): WorkspaceState {
  const browserTab: BrowserTab = {
    kind: "browser",
    id: "tab-browser",
    label: "Docs",
    browserId: "browser-1",
    url: "https://example.com",
    title: "Example",
    profileId: "default",
  };
  return {
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: { "session-terminal": terminalSession("session-terminal") },
    layout: {
      tabs: [browserTab],
      activeTabId: browserTab.id,
      layoutsByTabId: {
        [browserTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-browser" },
            second: { type: "leaf", leafId: "leaf-terminal" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-terminal",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-browser": "", "leaf-terminal": "session-terminal" },
          contentsByLeafId: {
            "leaf-browser": createBrowserPaneContent({
              browserId: "browser-1",
              url: "https://example.com",
              title: "Example",
              profileId: "default",
            }),
            "leaf-terminal": { kind: "terminal", sessionId: "session-terminal" },
          },
        },
      },
    },
    // `unreadTabIds`/`unreadWorktreePaths`/`activityBySessionId` are workspace-level state, not
    // layout state: the activity selectors read them off the store state.
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
    ...overrides,
  } as WorkspaceState;
}

beforeEach(() => {
  const values = new Map<string, string>();
  const storage: Storage = {
    get length() { return values.size; },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => { values.delete(key); },
    setItem: (key, value) => { values.set(key, String(value)); },
  };
  vi.spyOn(window, "localStorage", "get").mockReturnValue(storage);
  saveBrowserSettings({ restoreTabsOnLaunch: true });
  vi.spyOn(browserTauri, "closeBrowser").mockResolvedValue(undefined);
});

afterEach(() => { vi.restoreAllMocks(); });

describe("mixed split tabs: closing a browser tab that hosts a terminal pane", () => {
  it("closes both the browser webview and the terminal PTY", async () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(mixedBrowserTabState()));

    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["tab-browser"]);

    await act(async () => {
      await result.current.closeTab("tab-browser");
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
    expect(workspaceServices.closeTerminal).toHaveBeenCalledWith("backend-session-terminal");
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.sessions["session-terminal"]).toBeUndefined();
  });

  it("still closes the webview when the tab-level browser id and a pane leaf share one browser", async () => {
    const workspaceServices = services();
    const state = mixedBrowserTabState();
    // The tab's own browser is also the pane leaf browser: it must be closed exactly once.
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(state));

    await act(async () => {
      await result.current.closeTab("tab-browser");
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledTimes(1);
    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
  });
});

describe("mixed split tabs: closing a pane inside a browser tab", () => {
  it("closes the pane and clears the dangling session when the last terminal pane goes", async () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(mixedBrowserTabState()));

    await act(async () => {
      await result.current.closePane("tab-browser", "leaf-terminal");
    });

    const tabLayout = result.current.state.layout.layoutsByTabId["tab-browser"];
    expect(tabLayout.root).toEqual({ type: "leaf", leafId: "leaf-browser" });
    expect(tabLayout.sessionIdsByLeafId).toEqual({ "leaf-browser": "" });
    // The PTY is disposed and no pane or tab keeps pointing at the removed session.
    expect(workspaceServices.closeTerminal).toHaveBeenCalledWith("backend-session-terminal");
    expect(result.current.state.sessions["session-terminal"]).toBeUndefined();
    const tab = result.current.state.layout.tabs[0];
    expect(tab.kind).toBe("browser");
    expect((tab as { sessionId?: string }).sessionId ?? "").toBe("");
    // The surviving browser leaf keeps its webview: it is still referenced.
    expect(browserTauri.closeBrowser).not.toHaveBeenCalled();
  });

  it("re-points the tab's primary session to a surviving terminal leaf", () => {
    const terminalTab: TerminalTab = {
      kind: "terminal",
      id: "tab-terminal",
      label: "Main",
      sessionId: "session-primary",
    };
    const base = normalizeLayout({
      tabs: [terminalTab],
      activeTabId: terminalTab.id,
      layoutsByTabId: {
        [terminalTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-primary" },
            second: { type: "leaf", leafId: "leaf-second" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-primary",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-primary": "session-primary", "leaf-second": "session-second" },
          contentsByLeafId: {
            "leaf-primary": { kind: "terminal", sessionId: "session-primary" },
            "leaf-second": { kind: "terminal", sessionId: "session-second" },
          },
        },
      },
    });

    const next = layoutReducer(base, { type: "CLOSE_PANE", tabId: terminalTab.id, leafId: "leaf-primary" });
    const tab = next.tabs[0] as TerminalTab;
    expect(tab.sessionId).toBe("session-second");
    expect(next.layoutsByTabId[terminalTab.id].sessionIdsByLeafId).toEqual({ "leaf-second": "session-second" });
  });

  it("re-types a terminal tab to browser when its last terminal pane closes", () => {
    const terminalTab: TerminalTab = {
      kind: "terminal",
      id: "tab-terminal",
      label: "Main",
      sessionId: "session-primary",
    };
    const base = normalizeLayout({
      tabs: [terminalTab],
      activeTabId: terminalTab.id,
      layoutsByTabId: {
        [terminalTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-terminal" },
            second: { type: "leaf", leafId: "leaf-browser" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-terminal",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-terminal": "session-primary", "leaf-browser": "" },
          contentsByLeafId: {
            "leaf-terminal": { kind: "terminal", sessionId: "session-primary" },
            "leaf-browser": createBrowserPaneContent({
              browserId: "browser-9",
              url: "https://example.com/9",
              title: "Nine",
              profileId: "default",
            }),
          },
        },
      },
    });

    const next = layoutReducer(base, { type: "CLOSE_PANE", tabId: terminalTab.id, leafId: "leaf-terminal" });
    const tab = next.tabs[0];
    expect(tab.kind).toBe("browser");
    if (tab.kind !== "browser") throw new Error("expected a browser tab");
    expect(tab.browserId).toBe("browser-9");
    expect(tab.url).toBe("https://example.com/9");
    expect((tab as { sessionId?: string }).sessionId ?? "").toBe("");
  });
});

describe("mixed split tabs: detaching a browser leaf", () => {
  it("creates a browser tab preserving the browser id and leaves the terminal pane intact", () => {
    const workspaceServices = services();
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services: workspaceServices }),
    );
    act(() => result.current.restoreWorkspace(mixedBrowserTabState()));

    let detachedId: string | null = null;
    act(() => {
      detachedId = result.current.detachPaneToTab("tab-browser", "leaf-browser");
    });

    expect(detachedId).toBeTruthy();
    const tabs = result.current.state.layout.tabs;
    expect(tabs).toHaveLength(2);

    const detached = tabs.find((tab) => tab.id === detachedId);
    expect(detached?.kind).toBe("browser");
    if (!detached || detached.kind !== "browser") throw new Error("expected a detached browser tab");
    expect(detached.browserId).toBe("browser-1");
    expect(detached.url).toBe("https://example.com");

    // The source tab keeps its terminal pane and its session untouched.
    const sourceLayout = result.current.state.layout.layoutsByTabId["tab-browser"];
    expect(sourceLayout.root).toEqual({ type: "leaf", leafId: "leaf-terminal" });
    expect(sourceLayout.sessionIdsByLeafId).toEqual({ "leaf-terminal": "session-terminal" });
    expect(result.current.state.sessions["session-terminal"]).toBeDefined();
    expect(workspaceServices.closeTerminal).not.toHaveBeenCalled();
    expect(browserTauri.closeBrowser).not.toHaveBeenCalled();
  });

  it("detaches a file leaf from a mixed tab as a file tab", () => {
    const terminalTab: TerminalTab = {
      kind: "terminal",
      id: "tab-terminal",
      label: "Main",
      sessionId: "session-primary",
    };
    const base = normalizeLayout({
      tabs: [terminalTab],
      activeTabId: terminalTab.id,
      layoutsByTabId: {
        [terminalTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-terminal" },
            second: { type: "leaf", leafId: "leaf-file" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-terminal",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-terminal": "session-primary", "leaf-file": "" },
          contentsByLeafId: {
            "leaf-terminal": { kind: "terminal", sessionId: "session-primary" },
            "leaf-file": createFilePaneContent({
              path: "/repo/main/README.md",
              backendSessionId: "",
              line: null,
              col: null,
              workspaceId: null,
              previewId: "preview-1",
            }),
          },
        },
      },
    });

    const next = layoutReducer(base, {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: terminalTab.id,
      leafId: "leaf-file",
      newTab: { kind: "terminal", id: "tab-new", label: "README", sessionId: "" },
    });

    const detached = next.tabs.find((tab) => tab.id === "tab-new");
    expect(detached?.kind).toBe("file");
    if (!detached || detached.kind !== "file") throw new Error("expected a detached file tab");
    expect(detached.path).toBe("/repo/main/README.md");
    expect(next.layoutsByTabId["tab-new"].sessionIdsByLeafId).toEqual({ "leaf-file": "" });
    // The remaining terminal tab keeps its primary session.
    expect((next.tabs.find((tab) => tab.id === terminalTab.id) as TerminalTab).sessionId).toBe("session-primary");
  });

  it("refuses to detach a dag pane, which has no tab type of its own", () => {
    const terminalTab: TerminalTab = {
      kind: "terminal",
      id: "tab-terminal",
      label: "Main",
      sessionId: "session-primary",
    };
    const base = normalizeLayout({
      tabs: [terminalTab],
      activeTabId: terminalTab.id,
      layoutsByTabId: {
        [terminalTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-terminal" },
            second: { type: "leaf", leafId: "leaf-dag" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-terminal",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-terminal": "session-primary", "leaf-dag": "" },
          contentsByLeafId: {
            "leaf-terminal": { kind: "terminal", sessionId: "session-primary" },
            "leaf-dag": { kind: "dag", dag: { runId: "run-1" } },
          },
        },
      },
    });

    const next = layoutReducer(base, {
      type: "DETACH_PANE_TO_TAB",
      sourceTabId: terminalTab.id,
      leafId: "leaf-dag",
      newTab: { kind: "terminal", id: "tab-new", label: "Run", sessionId: "" },
    });

    expect(next).toBe(base);
  });
});

describe("mixed split tabs: activity, focus and session lookup", () => {
  it("reports activity for a terminal pane living inside a browser tab", () => {
    const state = mixedBrowserTabState();
    state.activityBySessionId = {
      "session-terminal": { state: "done", title: "Build finished", isAgent: true, agentType: "claude", source: "screen" },
    };

    const tabSummaries = selectTabActivitySummaries(state);
    expect(tabSummaries["tab-browser"]).toBeDefined();
    expect(tabSummaries["tab-browser"]?.agentType).toBe("claude");

    const worktreeSummaries = selectWorktreeActivitySummaries(state);
    expect(worktreeSummaries[worktree.path]).toBeDefined();
  });

  it("finds the focused pane session of a browser tab and resolves the tab for that session", () => {
    const state = mixedBrowserTabState();
    const tab = state.layout.tabs[0];
    expect(focusedPaneSessionId(state.layout, tab)).toBe("session-terminal");

    // A notification/activity write for that session must land on the browser tab. RESET_AGENT_STATE
    // looks the owning tab up by session id, so clearing the tab's unread flag proves the mixed tab
    // was found rather than skipped for not being a terminal tab.
    const withUnread: WorkspaceState = { ...state, unreadTabIds: { "tab-browser": true } };
    const cleared = workspaceReducer(withUnread, { type: "RESET_AGENT_STATE", sessionId: "session-terminal" });
    expect(cleared.unreadTabIds["tab-browser"]).toBeUndefined();
  });

  it("keeps focusedPaneSessionId null for a browser tab with no terminal pane", () => {
    const browser: BrowserTab = {
      kind: "browser",
      id: "tab-plain-browser",
      label: "Docs",
      browserId: "browser-plain",
      url: "https://example.com",
    };
    const state = normalizeLayout({
      tabs: [browser],
      activeTabId: browser.id,
      layoutsByTabId: {
        [browser.id]: {
          root: { type: "leaf", leafId: "leaf-only-browser" },
          activeLeafId: "leaf-only-browser",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-only-browser": "" },
          contentsByLeafId: {
            "leaf-only-browser": createBrowserPaneContent({
              browserId: "browser-plain",
              url: "https://example.com",
              profileId: "default",
            }),
          },
        },
      },
    });

    expect(focusedPaneSessionId(state, browser)).toBeNull();
  });
});

describe("mixed split tabs: persistence round trip", () => {
  it("round trips a split browser+terminal tab", () => {
    const state = mixedBrowserTabState();
    const saved = serializeWorkspaceState("workspace-1", "/repo/main", state);

    const persistedTab = saved.workspaces["workspace-1"].layout.tabs[0];
    expect(persistedTab.kind).toBe("browser");
    expect(persistedTab.paneTree).toBeDefined();
    expect(persistedTab.sessionIdsByLeafId).toEqual({
      "leaf-browser": "",
      "leaf-terminal": "session-terminal",
    });

    const restored = deserializeWorkspaceState("workspace-1", saved, ["backend-session-terminal"]);
    expect(restored).not.toBeNull();
    const restoredTab = restored!.layout.tabs[0];
    expect(restoredTab.kind).toBe("browser");
    if (restoredTab.kind !== "browser") throw new Error("expected a restored browser tab");
    expect(restoredTab.browserId).toBe("browser-1");

    const restoredLayout = restored!.layout.layoutsByTabId[restoredTab.id];
    expect(restoredLayout.root).toMatchObject({ type: "split", direction: "horizontal" });
    expect(restoredLayout.sessionIdsByLeafId["leaf-terminal"]).toBe("session-terminal");
    expect(restored!.sessions["session-terminal"]).toBeDefined();
  });

  it("excludes private browser leaves from a mixed split while keeping the terminal pane", () => {
    const state = mixedBrowserTabState();
    const tabLayout = state.layout.layoutsByTabId["tab-browser"];
    tabLayout.contentsByLeafId = {
      ...tabLayout.contentsByLeafId,
      "leaf-private": createBrowserPaneContent({
        browserId: "browser-private",
        url: "https://example.com/private",
        profileId: "private",
      }),
    };
    tabLayout.root = {
      type: "split",
      direction: "vertical",
      first: tabLayout.root,
      second: { type: "leaf", leafId: "leaf-private" },
      ratio: 0.5,
    };
    tabLayout.sessionIdsByLeafId = { ...tabLayout.sessionIdsByLeafId, "leaf-private": "" };

    const saved = serializeWorkspaceState("workspace-1", "/repo/main", state);
    const persistedTab = saved.workspaces["workspace-1"].layout.tabs[0];
    expect(persistedTab.contentsByLeafId?.["leaf-private"]).toBeUndefined();
    expect(persistedTab.contentsByLeafId?.["leaf-terminal"]).toEqual({
      kind: "terminal",
      sessionId: "session-terminal",
    });

    const restored = deserializeWorkspaceState("workspace-1", saved, ["backend-session-terminal"])!;
    const restoredLayout = restored.layout.layoutsByTabId[restored.layout.tabs[0].id];
    expect(restoredLayout.sessionIdsByLeafId["leaf-private"]).toBeUndefined();
    expect(restoredLayout.sessionIdsByLeafId["leaf-terminal"]).toBe("session-terminal");
    const restoredBrowserIds = Object.values(restoredLayout.contentsByLeafId ?? {})
      .filter((content) => content.kind === "browser")
      .map((content) => (content.kind === "browser" ? content.browser?.browserId ?? content.browserId : ""));
    expect(restoredBrowserIds).not.toContain("browser-private");
  });
});
