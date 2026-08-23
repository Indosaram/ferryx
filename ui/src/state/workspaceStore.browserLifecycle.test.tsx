import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const browserMocks = vi.hoisted(() => ({
  createBrowser: vi.fn(),
  navigateBrowser: vi.fn(),
  reloadBrowser: vi.fn(),
  closeBrowser: vi.fn(),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

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
    browserMocks.createBrowser.mockReset();
    browserMocks.navigateBrowser.mockReset();
    browserMocks.reloadBrowser.mockReset();
    browserMocks.closeBrowser.mockReset();

    browserMocks.createBrowser.mockResolvedValue({
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
    browserMocks.navigateBrowser.mockResolvedValue(undefined);
    browserMocks.reloadBrowser.mockResolvedValue(undefined);
    browserMocks.closeBrowser.mockResolvedValue(undefined);
  });

  it("closes the native child webview before removing its React tab", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    let tabId = "";
    await act(async () => {
      tabId = await result.current.createBrowserTab();
    });

    expect(result.current.state.layout.tabs).toHaveLength(1);

    await act(async () => {
      await result.current.closeTab(tabId);
    });

    expect(browserMocks.closeBrowser).toHaveBeenCalledWith("browser-1");
    expect(result.current.state.layout.tabs).toHaveLength(1);
    const replacementTab = result.current.state.layout.tabs[0];
    expect(replacementTab.kind).not.toBe("browser");
    if (replacementTab.kind !== "browser") {
      expect(replacementTab.sessionId).toBeDefined();
      const session = result.current.state.sessions[replacementTab.sessionId];
      expect(session).toBeDefined();
      expect(session.backendSessionId).toBeDefined();
    }
  });
});
