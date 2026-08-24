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
    vi.mocked(services.spawnTerminal).mockReset();
    vi.mocked(services.spawnTerminal).mockResolvedValue("backend-unused");

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

  it("closes the native child webview before replacing the sole browser tab with a terminal", async () => {
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
    expect(services.spawnTerminal).toHaveBeenCalledTimes(1);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    expect(result.current.state.layout.tabs[0].kind).not.toBe("browser");
    expect(result.current.state.layout.activeTabId).toBe(result.current.state.layout.tabs[0].id);
  });

  it("reuses the same clientRequestId when one logical terminal spawn retries after an ambiguous renderer transport failure", async () => {
    vi.mocked(services.spawnTerminal)
      .mockRejectedValueOnce({ code: "UNKNOWN", message: "lost Tauri response", details: {} })
      .mockResolvedValueOnce("backend-retried");

    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", initialWorktrees: [worktree], services }),
    );

    await act(async () => {
      await result.current.openTab(worktree);
    });

    expect(services.spawnTerminal).toHaveBeenCalledTimes(2);
    const firstRequest = vi.mocked(services.spawnTerminal).mock.calls[0]?.[0] as Record<string, unknown>;
    const secondRequest = vi.mocked(services.spawnTerminal).mock.calls[1]?.[0] as Record<string, unknown>;
    expect(firstRequest.clientRequestId).toEqual(expect.any(String));
    expect(secondRequest.clientRequestId).toBe(firstRequest.clientRequestId);
    expect(result.current.state.layout.tabs).toHaveLength(1);
    const tab = result.current.state.layout.tabs[0];
    expect(tab.kind).not.toBe("browser");
    if (tab.kind !== "browser") {
      expect(result.current.state.sessions[tab.sessionId].backendSessionId).toBe("backend-retried");
    }
  });
});