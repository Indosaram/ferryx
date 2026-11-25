import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";

const browserMocks = vi.hoisted(() => ({
  BROWSER_SHORTCUT_EVENT: "ferryx:browser-shortcut",
  setBrowserBounds: vi.fn(async () => undefined),
  setBrowserVisible: vi.fn(async () => undefined),
  closeBrowser: vi.fn(async () => undefined),
  onBrowserShortcutRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadRequested: vi.fn(async () => () => undefined),
  onBrowserDownloadUpdated: vi.fn(async () => () => undefined),
  extractBrowserErrorCode: vi.fn((err: unknown) => String((err as { code?: unknown })?.code ?? "UNKNOWN_ERROR")),
}));

vi.mock("../lib/browserTauri", () => browserMocks);

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

const BROWSER_ID = "browser-split-detach";

beforeEach(() => {
  browserMocks.closeBrowser.mockClear();
});

afterEach(() => {
  cleanup();
});

function session(id: string): TerminalSession {
  return {
    id,
    cwd: `/repo/${id}`,
    worktreePath: `/repo/${id}`,
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: id },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

/** A terminal tab whose right leaf is a browser pane: the worktree split shape. */
function terminalWithBrowserLeaf(): LayoutState {
  const tab: TerminalTab = { id: "tab-main", label: "main", sessionId: "session-a" };
  return {
    tabs: [tab],
    activeTabId: tab.id,
    tabGroups: { "group-main": { id: "group-main", tabIds: [tab.id], activeTabId: tab.id } },
    tabGroupLayout: { type: "group", groupId: "group-main" },
    focusedGroupId: "group-main",
    layoutsByTabId: {
      [tab.id]: {
        root: {
          type: "split",
          direction: "horizontal",
          first: { type: "leaf", leafId: "leaf-terminal" },
          second: { type: "leaf", leafId: "leaf-browser" },
          ratio: 0.5,
        },
        activeLeafId: "leaf-browser",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-terminal": "session-a" },
        contentsByLeafId: {
          "leaf-browser": { kind: "browser", browser: { browserId: BROWSER_ID, url: "https://example.com" } },
        },
      },
    },
  } as unknown as LayoutState;
}

function renderSplit(overrides: Partial<Parameters<typeof TerminalSplitView>[0]> = {}) {
  const onDetachBrowserPane = vi.fn();
  const onClosePane = vi.fn();
  render(
    <TerminalSplitView
      layout={terminalWithBrowserLeaf()}
      sessions={{ "session-a": session("session-a") }}
      onDetachBrowserPane={onDetachBrowserPane}
      onClosePane={onClosePane}
      {...overrides}
    />,
  );
  return { onDetachBrowserPane, onClosePane };
}

describe("browser split detach-by-default", () => {
  it("labels the browser leaf's close control as a detach, not a split close", async () => {
    renderSplit();

    const detach = await screen.findByTestId("pane-browser-detach");
    expect(detach).toHaveAttribute("aria-label", "Detach browser pane");
  });

  it("detaching asks the owner to drop the leaf AND leaves the backend session alone", async () => {
    const { onDetachBrowserPane, onClosePane } = renderSplit();

    fireEvent.click(await screen.findByTestId("pane-browser-detach"));

    // The callback receives everything the owner needs to remove the leaf from the layout.
    expect(onDetachBrowserPane).toHaveBeenCalledOnce();
    expect(onDetachBrowserPane).toHaveBeenCalledWith(BROWSER_ID, "tab-main", "leaf-browser");
    // Detach is not a close: the pane close path (which tears down the webview) is not taken.
    expect(onClosePane).not.toHaveBeenCalled();
    expect(browserMocks.closeBrowser).not.toHaveBeenCalled();
  });

  it("falls back to closing the pane when no detach callback is wired", async () => {
    const onClosePane = vi.fn();
    render(
      <TerminalSplitView
        layout={terminalWithBrowserLeaf()}
        sessions={{ "session-a": session("session-a") }}
        onClosePane={onClosePane}
      />,
    );

    fireEvent.click(await screen.findByTestId("pane-browser-detach"));

    expect(onClosePane).toHaveBeenCalledWith("tab-main", "leaf-browser");
  });

  it("offers the fullscreen and destructive close actions from the browser context menu", async () => {
    const onOpenBrowserFullscreen = vi.fn();
    const onCloseBrowserSession = vi.fn();
    renderSplit({ onOpenBrowserFullscreen, onCloseBrowserSession });

    fireEvent.contextMenu(await screen.findByTestId("pane-browser-detach"));

    const menu = await screen.findByTestId("pane-browser-menu");
    expect(menu).toBeInTheDocument();

    fireEvent.click(screen.getByRole("menuitem", { name: "Open in Browser fullscreen" }));
    expect(onOpenBrowserFullscreen).toHaveBeenCalledWith(BROWSER_ID);

    fireEvent.contextMenu(screen.getByTestId("pane-browser-detach"));
    fireEvent.click(screen.getByRole("menuitem", { name: "Close browser session" }));
    expect(onCloseBrowserSession).toHaveBeenCalledWith(BROWSER_ID);
    // The destructive path never routes through the detach callback by accident.
    expect(browserMocks.closeBrowser).not.toHaveBeenCalled();
  });

  it("keeps the terminal leaf's close control a real split close", async () => {
    const { onDetachBrowserPane, onClosePane } = renderSplit();

    await waitFor(() => expect(screen.getAllByTestId("pane-leaf")).toHaveLength(2));
    const terminalLeaf = screen
      .getAllByTestId("pane-leaf")
      .find((leaf) => leaf.getAttribute("data-leaf-id") === "leaf-terminal");
    expect(terminalLeaf).toBeDefined();
    const closeButton = terminalLeaf!.querySelector<HTMLButtonElement>('button[aria-label="Close split view"]');
    expect(closeButton).not.toBeNull();

    fireEvent.click(closeButton!);

    expect(onClosePane).toHaveBeenCalledWith("tab-main", "leaf-terminal");
    expect(onDetachBrowserPane).not.toHaveBeenCalled();
  });
});
