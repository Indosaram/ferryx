import { beforeEach, describe, expect, it, vi } from "vitest";

import { saveBrowserSettings } from "../lib/browserSettings";
import type { BrowserTab } from "../lib/types";
import { createLayoutState } from "./layout";
import { hydrateRestoredBrowserSessions } from "./browserSessionHydration";
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
  beforeEach(() => {
    localStorage.clear();
    browserMocks.ensureBrowser.mockClear();
  });

  it("recreates persisted native webviews with the stored browser id, URL, profile, and zoom", async () => {
    saveBrowserSettings({ restoreTabsOnLaunch: true });
    const state = workspaceWithBrowser({
      kind: "browser",
      id: "tab-browser",
      label: "Browser",
      browserId: "persisted-browser-id",
      url: "https://example.com/restored",
      profileId: "private",
      zoomFactor: 1.25,
    });

    await hydrateRestoredBrowserSessions(state, "workspace-1");

    expect(browserMocks.ensureBrowser).toHaveBeenCalledWith(expect.objectContaining({
      browserId: "persisted-browser-id",
      workspaceId: "workspace-1",
      url: "https://example.com/restored",
      profile: "private",
      zoomFactor: 1.25,
      visible: false,
    }));
  });

  it("does not materialize persisted browser tabs when launch restore is disabled", async () => {
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
});
