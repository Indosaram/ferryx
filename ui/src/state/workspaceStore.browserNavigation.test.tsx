import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import * as browserTauri from "../lib/browserTauri";
import { createBrowserPaneContent, type WorkspaceTab } from "../lib/types";
import { createLayoutState } from "./layout";
import { clearHmrWorkspaceState, useWorkspaceStore, type WorkspaceServices, type WorkspaceState } from "./workspaceStore";

const workspaceId = "browser-navigation-regression";
const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

function initialState(parent: "terminal" | "browser"): WorkspaceState {
  const tab: WorkspaceTab = parent === "terminal"
    ? { kind: "terminal", id: "owner", label: "Terminal", sessionId: "terminal" }
    : { kind: "browser", id: "owner", label: "Browser", browserId: "browser-owner", url: "https://example.com/" };
  const layout = createLayoutState([tab]);
  layout.layoutsByTabId.owner = {
    root: {
      type: "split", direction: "horizontal", ratio: 0.5,
      first: { type: "leaf", leafId: "parent" },
      second: { type: "leaf", leafId: "browser" },
    },
    activeLeafId: "parent",
    expandedLeafId: null,
    sessionIdsByLeafId: { parent: parent === "terminal" ? "terminal" : "", browser: "" },
    contentsByLeafId: {
      parent: parent === "terminal"
        ? { kind: "terminal", sessionId: "terminal" }
        : createBrowserPaneContent({ browserId: "browser-owner", url: "https://example.com/" }),
      browser: createBrowserPaneContent({ browserId: "browser-child", url: "about:blank", profileId: "default" }),
    },
  };
  return {
    workspaceId, worktrees: [], activeWorktreePath: null, sessions: {}, layout,
    unreadTabIds: {}, unreadWorktreePaths: {},
  };
}

beforeEach(() => {
  clearHmrWorkspaceState(workspaceId);
  vi.spyOn(browserTauri, "navigateBrowser").mockResolvedValue(undefined);
  vi.spyOn(browserTauri, "reloadBrowser").mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  clearHmrWorkspaceState(workspaceId);
});

describe("browser navigation inside mixed panes", () => {
  it.each(["terminal", "browser"] as const)("navigates the child browser under a %s tab", async (parent) => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId, services }));
    const state = initialState(parent);
    act(() => result.current.restoreWorkspace(state));

    await act(async () => {
      await result.current.navigateBrowserTab("owner", "https://google.com/", "browser-child");
    });

    expect(browserTauri.navigateBrowser).toHaveBeenCalledExactlyOnceWith("browser-child", "https://google.com/");
    expect(result.current.state.layout.layoutsByTabId.owner.contentsByLeafId?.browser).toMatchObject({
      browser: { browserId: "browser-child", url: "https://google.com/", loading: false },
    });
    expect(result.current.state.layout.tabs).toEqual(state.layout.tabs);
    expect(result.current.state.layout.layoutsByTabId.owner.contentsByLeafId?.parent)
      .toEqual(state.layout.layoutsByTabId.owner.contentsByLeafId?.parent);
  });

  it("reloads the child browser rather than its owning tab", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId, services }));
    act(() => result.current.restoreWorkspace(initialState("terminal")));

    await act(async () => {
      await result.current.reloadBrowserTab("owner", "browser-child");
    });

    expect(browserTauri.reloadBrowser).toHaveBeenCalledExactlyOnceWith("browser-child");
  });

  it("keeps standalone tab navigation working without an explicit browser id", async () => {
    const { result } = renderHook(() => useWorkspaceStore({ workspaceId, services }));
    act(() => result.current.restoreWorkspace(initialState("browser")));

    await act(async () => {
      await result.current.navigateBrowserTab("owner", "https://google.com/");
    });

    expect(browserTauri.navigateBrowser).toHaveBeenCalledExactlyOnceWith("browser-owner", "https://google.com/");
    expect(result.current.state.layout.tabs[0]).toMatchObject({ url: "https://google.com/" });
  });
});
