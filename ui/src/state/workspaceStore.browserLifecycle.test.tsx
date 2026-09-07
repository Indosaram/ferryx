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

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as browserTauri from "../lib/browserTauri";
import { createBrowserPaneContent, type Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc",
  branch: "refs/heads/orca/workspace-1/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

describe("useWorkspaceStore browser lifecycle", () => {
  beforeEach(() => {
    (services.spawnTerminal as any).mockReset();
    (services.spawnTerminal as any).mockResolvedValue("backend-unused");

    vi.spyOn(browserTauri, "createBrowser").mockResolvedValue({
      browserId: "browser-1",
      webviewLabel: "browser-webview-1",
      workspaceId: "workspace-1",
      worktreePath: null,
      profileId: "default",
      generation: 1,
      url: "http://localhost:3000",
      title: null,
      loading: false,
      canGoBack: false,
      canGoForward: false,
      zoomFactor: 1,
      loadError: null,
      visible: true,
    });
    vi.spyOn(browserTauri, "navigateBrowser").mockResolvedValue(undefined);
    vi.spyOn(browserTauri, "reloadBrowser").mockResolvedValue(undefined);
    vi.spyOn(browserTauri, "closeBrowser").mockResolvedValue(undefined);
  });

  it("closes the native child webview when closing the sole browser tab", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    let tabId = "";
    await act(async () => {
      const created = await result.current.createBrowserTab();
      if (created === null) throw new Error("expected createBrowserTab to return a tab id");
      tabId = created;
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);

    await act(async () => {
      await result.current.closeTab(tabId);
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
    expect(services.spawnTerminal).not.toHaveBeenCalled();
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.layout.activeTabId).toBeNull();
  });

  it("closes the native child webview when closing a browser pane leaf inside a split", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-target": {
          id: "session-target",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "workspace-1",
          backendSessionId: "backend-session-target",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-terminal" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.5,
            },
            activeLeafId: "leaf-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-terminal": "session-target", "leaf-browser": "" },
            contentsByLeafId: {
              "leaf-terminal": { kind: "terminal", sessionId: "session-target" },
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closePane("terminal-target", "leaf-browser");
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
    // The terminal pane survives; only the browser leaf is removed.
    expect(result.current.state.layout.layoutsByTabId["terminal-target"].root).toEqual({
      type: "leaf",
      leafId: "leaf-terminal",
    });
    expect(result.current.state.sessions["session-target"]).toBeDefined();
  });

  it("keeps the native child webview when another tab still references the same browser id", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-target": {
          id: "session-target",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "workspace-1",
          backendSessionId: "backend-session-target",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
          { kind: "browser", id: "browser-tab", label: "Browser", browserId: "browser-1", url: "https://example.com" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-terminal" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.5,
            },
            activeLeafId: "leaf-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-terminal": "session-target", "leaf-browser": "" },
            contentsByLeafId: {
              "leaf-terminal": { kind: "terminal", sessionId: "session-target" },
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closePane("terminal-target", "leaf-browser");
    });

    expect(browserTauri.closeBrowser).not.toHaveBeenCalled();
    // The pane leaf is still removed while the browser tab keeps the webview alive.
    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["terminal-target", "browser-tab"]);
    expect(result.current.state.layout.layoutsByTabId["terminal-target"].root).toEqual({
      type: "leaf",
      leafId: "leaf-terminal",
    });
  });

  it("reuses the same clientRequestId when one logical terminal spawn retries after an ambiguous renderer transport failure", async () => {
    (services.spawnTerminal as any)
      .mockRejectedValueOnce({ code: "UNKNOWN", message: "lost Tauri response", details: {} })
      .mockResolvedValueOnce("backend-retried");

    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    await act(async () => {
      await result.current.openTab(worktree);
    });

    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
    const firstRequest = (services.spawnTerminal as any).mock.calls[0]?.[0] as Record<string, unknown>;
    const secondRequest = (services.spawnTerminal as any).mock.calls[1]?.[0] as Record<string, unknown>;
    expect(firstRequest.clientRequestId).toEqual(expect.any(String));
    expect(secondRequest.clientRequestId).toBe(firstRequest.clientRequestId);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    const tab = result.current.state.layout.tabs[0];
    expect(tab.kind).not.toBe("browser");
    if (tab.kind !== "browser") {
      expect(result.current.state.sessions[tab.sessionId].backendSessionId).toBe("backend-retried");
    }
  });

  it("closes the native child webview when closing a terminal tab whose pane layout holds a browser leaf", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-target": {
          id: "session-target",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "workspace-1",
          backendSessionId: "backend-session-target",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-terminal" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.5,
            },
            activeLeafId: "leaf-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-terminal": "session-target", "leaf-browser": "" },
            contentsByLeafId: {
              "leaf-terminal": { kind: "terminal", sessionId: "session-target" },
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closeTab("terminal-target");
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
    expect(result.current.state.layout.tabs).toHaveLength(0);
  });

  it("keeps the native child webview when closing a terminal tab while another tab still references the same browser id", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-target": {
          id: "session-target",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "workspace-1",
          backendSessionId: "backend-session-target",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
          { kind: "browser", id: "browser-tab", label: "Browser", browserId: "browser-1", url: "https://example.com" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-terminal" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.5,
            },
            activeLeafId: "leaf-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-terminal": "session-target", "leaf-browser": "" },
            contentsByLeafId: {
              "leaf-terminal": { kind: "terminal", sessionId: "session-target" },
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closeTab("terminal-target");
    });

    expect(browserTauri.closeBrowser).not.toHaveBeenCalled();
    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual(["browser-tab"]);
  });

  it("keeps the native child webview when closing a terminal tab while a parked worktree layout still references the same browser id", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {
        "session-target": {
          id: "session-target",
          cwd: worktree.path,
          worktreePath: worktree.path,
          workspaceId: "workspace-1",
          backendSessionId: "backend-session-target",
          lifecycle: "working",
        },
      },
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: {
              type: "split",
              direction: "horizontal",
              first: { type: "leaf", leafId: "leaf-terminal" },
              second: { type: "leaf", leafId: "leaf-browser" },
              ratio: 0.5,
            },
            activeLeafId: "leaf-terminal",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-terminal": "session-target", "leaf-browser": "" },
            contentsByLeafId: {
              "leaf-terminal": { kind: "terminal", sessionId: "session-target" },
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      worktreeLayouts: {
        "/repo/feature": {
          tabs: [
            { kind: "terminal", id: "parked-terminal", label: "Terminal", sessionId: "session-parked" },
          ],
          activeTabId: "parked-terminal",
          layoutsByTabId: {
            "parked-terminal": {
              root: { type: "leaf", leafId: "leaf-parked-browser" },
              activeLeafId: "leaf-parked-browser",
              expandedLeafId: null,
              sessionIdsByLeafId: {},
              contentsByLeafId: {
                "leaf-parked-browser": createBrowserPaneContent({
                  browserId: "browser-1",
                  url: "https://example.com",
                  title: "Example Domain",
                  profileId: "default",
                }),
              },
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closeTab("terminal-target");
    });

    expect(browserTauri.closeBrowser).not.toHaveBeenCalled();
    expect(result.current.state.layout.tabs).toHaveLength(0);
    expect(result.current.state.worktreeLayouts?.["/repo/feature"]).toBeDefined();
  });

  it("closes the native child webview when closePane delegates to closeTab for the last remaining browser leaf", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    const initialState: WorkspaceState = {
      worktrees: [worktree],
      activeWorktreePath: worktree.path,
      sessions: {},
      layout: {
        tabs: [
          { kind: "terminal", id: "terminal-target", label: "Terminal", sessionId: "session-target" },
        ],
        activeTabId: "terminal-target",
        layoutsByTabId: {
          "terminal-target": {
            root: { type: "leaf", leafId: "leaf-browser" },
            activeLeafId: "leaf-browser",
            expandedLeafId: null,
            sessionIdsByLeafId: {},
            contentsByLeafId: {
              "leaf-browser": createBrowserPaneContent({
                browserId: "browser-1",
                url: "https://example.com",
                title: "Example Domain",
                profileId: "default",
              }),
            },
          },
        },
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    act(() => result.current.restoreWorkspace(initialState));

    await act(async () => {
      await result.current.closePane("terminal-target", "leaf-browser");
    });

    expect(browserTauri.closeBrowser).toHaveBeenCalledWith("browser-1");
    expect(result.current.state.layout.tabs).toHaveLength(0);
  });
});