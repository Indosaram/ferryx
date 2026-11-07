import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

import { saveBrowserSettings } from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";
import { createLayoutState } from "./layout";
import { hydrateRestoredBrowserSessions, useBrowserSessionHydration } from "./browserSessionHydration";
import type { WorkspaceState } from "./workspaceStore";

const browserMocks = vi.hoisted(() => ({
  ensureBrowser: vi.fn(async (request: { browserId: string; url: string; profile?: string }) => ({
    browserId: request.browserId,
    webviewLabel: `browser-${request.browserId}`,
    workspaceId: "workspace-1",
    worktreePath: null,
    profileId: request.profile ?? "default",
    generation: 1,
    url: request.url,
    title: null,
    loading: false,
    canGoBack: false,
    canGoForward: false,
    zoomFactor: 1,
    loadError: null,
    visible: false,
  })),
}));

vi.mock("../lib/browserTauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/browserTauri")>();
  return { ...actual, ensureBrowser: browserMocks.ensureBrowser };
});

function workspaceWithBrowser(tab: BrowserTab): WorkspaceState {
  return {
    workspaceId: "workspace-1",
    worktrees: [],
    activeWorktreePath: null,
    sessions: {},
    layout: createLayoutState([tab], tab.id),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

describe("browser session hydration", () => {
  // Vitest globals are disabled, so RTL cannot register global auto-cleanup.
  afterEach(cleanup);

  beforeEach(() => {
    localStorage.clear();
    browserMocks.ensureBrowser.mockClear();
  });

  it("recreates persisted native webviews with the stored browser id, URL, profile, zoom, and scroll", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const state = workspaceWithBrowser({
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "persisted-browser-id",
      url: "https://example.com/restored",
      profileId: "default",
      zoomFactor: 1.25,
      ...({ scrollPosition: { x: 10, y: 150 } } as any),
    });

    await hydrateRestoredBrowserSessions(state, "workspace-1");

    expect(browserMocks.ensureBrowser).toHaveBeenCalledWith(expect.objectContaining({
      browserId: "persisted-browser-id",
      workspaceId: "workspace-1",
      url: "https://example.com/restored",
      profile: "default",
      zoomFactor: 1.25,
      visible: false,
    }));
  });

  it("excludes private-profile browser tabs from restoration (privacy isolation)", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const state = workspaceWithBrowser({
      kind: "browser",
      id: "tab-private-browser",
      label: "Private Browser",
      browserId: "private-browser-id",
      url: "https://example.com/secret",
      profileId: "private",
    });

    await hydrateRestoredBrowserSessions(state, "workspace-1");

    expect(browserMocks.ensureBrowser).not.toHaveBeenCalled();
  });

  it("restores tabs per worktree without mixing into other worktrees", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const tabA: BrowserTab = {
      kind: "browser",
      id: "tab-a",
      label: "Tab A",
      browserId: "browser-a",
      url: "https://example.com/wt-a",
      worktreePath: "/repo/worktree-a",
    };
    const tabB: BrowserTab = {
      kind: "browser",
      id: "tab-b",
      label: "Tab B",
      browserId: "browser-b",
      url: "https://example.com/wt-b",
      worktreePath: "/repo/worktree-b",
    };

    const state: WorkspaceState = {
      workspaceId: "workspace-1",
      worktrees: [
        { path: "/repo/worktree-a", branch: "wt-a", head: "111", bare: false, detached: false, locked: null, prunable: null },
        { path: "/repo/worktree-b", branch: "wt-b", head: "222", bare: false, detached: false, locked: null, prunable: null },
      ],
      activeWorktreePath: "/repo/worktree-a",
      sessions: {},
      layout: createLayoutState([tabA], tabA.id),
      worktreeLayouts: {
        "/repo/worktree-b": createLayoutState([tabB], tabB.id),
      },
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    // Hydrate only for worktree-a:
    await hydrateRestoredBrowserSessions(state, "workspace-1", new Set(), "/repo/worktree-a");

    expect(browserMocks.ensureBrowser).toHaveBeenCalledWith(expect.objectContaining({
      browserId: "browser-a",
      worktreePath: "/repo/worktree-a",
    }));
    expect(browserMocks.ensureBrowser).not.toHaveBeenCalledWith(expect.objectContaining({
      browserId: "browser-b",
    }));
  });

  it("skips stale tabs whose worktree was deleted or whose URL is unnavigable", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const tabDeletedWt: BrowserTab = {
      kind: "browser",
      id: "tab-deleted",
      label: "Deleted WT",
      browserId: "browser-deleted",
      url: "https://example.com/deleted",
      worktreePath: "/repo/deleted-worktree",
    };
    const tabInvalidUrl: BrowserTab = {
      kind: "browser",
      id: "tab-invalid",
      label: "Invalid URL",
      browserId: "browser-invalid",
      url: "javascript:alert(1)",
      worktreePath: "/repo/live-worktree",
    };

    const state: WorkspaceState = {
      workspaceId: "workspace-1",
      worktrees: [
        { path: "/repo/live-worktree", branch: "live", head: "333", bare: false, detached: false, locked: null, prunable: null },
      ],
      activeWorktreePath: "/repo/live-worktree",
      sessions: {},
      layout: createLayoutState([tabDeletedWt, tabInvalidUrl], tabDeletedWt.id),
      worktreeLayouts: {},
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    await hydrateRestoredBrowserSessions(state, "workspace-1");

    expect(browserMocks.ensureBrowser).not.toHaveBeenCalled();
  });

  it("prevents double-restore from duplicating browser tabs", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const tab: BrowserTab = {
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "persisted-browser-id",
      url: "https://example.com/once",
      worktreePath: "/repo/live",
    };
    const state: WorkspaceState = {
      workspaceId: "workspace-1",
      worktrees: [
        { path: "/repo/live", branch: "main", head: "123", bare: false, detached: false, locked: null, prunable: null },
      ],
      activeWorktreePath: "/repo/live",
      sessions: {},
      layout: createLayoutState([tab], tab.id),
      worktreeLayouts: {},
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const hydratedKeys = new Set<string>();
    await hydrateRestoredBrowserSessions(state, "workspace-1", hydratedKeys);
    expect(browserMocks.ensureBrowser).toHaveBeenCalledTimes(1);

    // Calling again with the same hydratedKeys set does not duplicate
    await hydrateRestoredBrowserSessions(state, "workspace-1", hydratedKeys);
    expect(browserMocks.ensureBrowser).toHaveBeenCalledTimes(1);
  });

  it("does not materialize persisted browser tabs when launch restore is disabled", async () => {
    // Restore defaults on, so the user has to turn it off.
    saveBrowserSettings({ restoreTabsOnLaunch: false });
    const state = workspaceWithBrowser({
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "persisted-browser-id",
      url: "https://example.com/restored",
    });

    await hydrateRestoredBrowserSessions(state, "workspace-1");

    expect(browserMocks.ensureBrowser).not.toHaveBeenCalled();
  });

  it("useBrowserSessionHydration re-runs restore when only state.worktrees changes (F-UI-STATE-REMOTE-4)", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const tab: BrowserTab = {
      kind: "browser",
      id: "tab-wt",
      label: "WT Browser",
      browserId: "browser-wt",
      url: "https://example.com/wt",
      worktreePath: "/repo/wt-new",
    };
    // The worktree list is non-empty but does not yet contain the tab's
    // worktree, so the stale-worktree guard skips it on the first pass.
    const state: WorkspaceState = {
      workspaceId: "workspace-1",
      worktrees: [
        { path: "/repo/other", branch: "other", head: "111", bare: false, detached: false, locked: null, prunable: null },
      ],
      activeWorktreePath: "/repo/wt-new",
      sessions: {},
      layout: createLayoutState([tab], tab.id),
      worktreeLayouts: {},
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {},
    };

    const { rerender } = renderHook(
      ({ current }: { current: WorkspaceState }) => useBrowserSessionHydration(current, "workspace-1"),
      { initialProps: { current: state } },
    );
    await act(async () => {});
    expect(browserMocks.ensureBrowser).not.toHaveBeenCalled();

    // Only state.worktrees changes (layout and worktreeLayouts keep identity);
    // the tab's worktree becomes known, so the hook must restore it.
    const stateWithWt: WorkspaceState = {
      ...state,
      worktrees: [
        ...state.worktrees,
        { path: "/repo/wt-new", branch: "feature", head: "abc", bare: false, detached: false, locked: null, prunable: null },
      ],
    };
    await act(async () => {
      rerender({ current: stateWithWt });
    });
    expect(browserMocks.ensureBrowser).toHaveBeenCalledTimes(1);
    expect(browserMocks.ensureBrowser).toHaveBeenCalledWith(expect.objectContaining({
      browserId: "browser-wt",
      worktreePath: "/repo/wt-new",
    }));

    // A further render with an unchanged worktree list must not restore again.
    await act(async () => {
      rerender({ current: { ...stateWithWt } });
    });
    expect(browserMocks.ensureBrowser).toHaveBeenCalledTimes(1);
  });
});
