import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkspaceStore, type WorkspaceServices } from "./workspaceStore";

const browserMocks = vi.hoisted(() => ({
  createBrowser: vi.fn(),
  navigateBrowser: vi.fn(async () => undefined),
  reloadBrowser: vi.fn(async () => undefined),
  closeBrowser: vi.fn(async () => undefined),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-unused"),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

describe("useWorkspaceStore browser lifecycle", () => {
  beforeEach(() => {
    browserMocks.createBrowser.mockReset();
    browserMocks.navigateBrowser.mockClear();
    browserMocks.reloadBrowser.mockClear();
    browserMocks.closeBrowser.mockClear();
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
  });

  it("closes the native child webview before removing its React tab", async () => {
    const { result } = renderHook(() =>
      useWorkspaceStore({ workspaceId: "workspace-1", services }),
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
    expect(result.current.state.layout.tabs).toHaveLength(0);
  });
});