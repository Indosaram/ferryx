import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { newBrowserTabUrl, saveBrowserSettings } from "../lib/browserSettings";
import type { WorkspaceServices } from "../state/workspaceStore";
import { useWorkspaceStore } from "../state/workspaceStore";
import { TerminalSplitView } from "./TerminalSplitView";

const browserMocks = vi.hoisted(() => ({
  createBrowser: vi.fn(),
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  closeBrowser: vi.fn(async () => undefined),
  navigateBrowser: vi.fn(async () => undefined),
  reloadBrowser: vi.fn(async () => undefined),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: vi.fn() }),
}));

const services: WorkspaceServices = {
  ensureTerminalEvents: vi.fn(async () => undefined),
  spawnTerminal: vi.fn(async () => "backend-unused"),
  getTerminalCwd: vi.fn(async () => null),
  closeTerminal: vi.fn(async () => undefined),
  waitForTerminalExit: vi.fn(async () => undefined),
};

function Harness() {
  const store = useWorkspaceStore({ workspaceId: "workspace-1", services });
  return (
    <TerminalSplitView
      layout={store.state.layout}
      sessions={store.state.sessions}
      onActivateTab={store.activateTab}
      onCloseTab={(tabId) => {
        void store.closeTab(tabId);
      }}
      onAddTab={() => undefined}
      onAddBrowserTab={(url) => {
        void store.createBrowserTab(url ?? newBrowserTabUrl());
      }}
    />
  );
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("New Browser Tab click creates visible browser content", () => {
  beforeEach(() => {
    localStorage.clear();
    browserMocks.createBrowser.mockReset();
    browserMocks.createBrowser.mockImplementation(async (request: { url: string }) => ({
      browserId: "browser-1",
      webviewLabel: "browser-webview-1",
      workspaceId: "workspace-1",
      worktreePath: null,
      profileId: "default",
      generation: 1,
      url: request.url,
      title: null,
      loading: false,
      canGoBack: false,
      canGoForward: false,
      zoomFactor: 1,
      loadError: null,
      visible: true,
    }));
  });

  it("shows a browser tab and viewport for the configured homepage", async () => {
    saveBrowserSettings({ homePage: "https://example.com/start" });
    render(<Harness />);

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Browser Tab/i }));

    await waitFor(() => {
      expect(browserMocks.createBrowser).toHaveBeenCalledWith(
        expect.objectContaining({ url: "https://example.com/start" }),
      );
    });
    expect(screen.getByRole("tab", { name: /Browser/i })).toBeInTheDocument();
    expect(screen.getByTestId("browser-viewport")).toBeInTheDocument();
    expect(screen.getByDisplayValue("https://example.com/start")).toBeInTheDocument();
  });

  it("shows a browser tab and viewport for a blank homepage", async () => {
    render(<Harness />);

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Browser Tab/i }));

    await waitFor(() => {
      expect(browserMocks.createBrowser).toHaveBeenCalledWith(expect.objectContaining({ url: "about:blank" }));
    });
    expect(screen.getByRole("tab", { name: /Browser/i })).toBeInTheDocument();
    expect(screen.getByTestId("browser-viewport")).toBeInTheDocument();
    expect(screen.getByDisplayValue("about:blank")).toBeInTheDocument();
  });
});
