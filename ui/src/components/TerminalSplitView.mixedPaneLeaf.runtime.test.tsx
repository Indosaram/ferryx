import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  BrowserTab,
  LayoutState,
  TabPaneLayout,
  TerminalSession,
  TerminalTab,
} from "../lib/types";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ startDragging: vi.fn() }),
}));

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id} />
  ),
}));

vi.mock("./BrowserPane", () => ({
  BrowserPane: ({
    tab,
    visible,
    onNavigate,
    onReload,
  }: {
    tab: BrowserTab;
    visible?: boolean;
    onNavigate: (url: string) => void;
    onReload: () => void;
  }) => (
    <div
      data-testid="browser-pane"
      data-browser-id={tab.browserId}
      data-url={tab.url}
      data-visible={String(Boolean(visible))}
    >
      <span data-testid="browser-title">{tab.title}</span>
      <button
        type="button"
        data-testid="browser-navigate-button"
        onClick={() => onNavigate("https://example.com/updated")}
      >
        Navigate
      </button>
      <button
        type="button"
        data-testid="browser-reload-button"
        onClick={() => onReload()}
      >
        Reload
      </button>
    </div>
  ),
}));

import { TerminalSplitView } from "./TerminalSplitView";

afterEach(cleanup);

type BrowserPaneLeafContent = {
  kind: "browser";
  browserId: string;
  url: string;
  title?: string | null;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  profileId?: string;
  worktreePath?: string;
  worktreeLabel?: string;
};

type TerminalPaneLeafContent = {
  kind: "terminal";
  sessionId: string;
};

type PaneLeafContent = TerminalPaneLeafContent | BrowserPaneLeafContent;

type MixedTabPaneLayout = TabPaneLayout & {
  contentsByLeafId?: Record<string, PaneLeafContent>;
};

type MixedLayoutState = Omit<LayoutState, "layoutsByTabId"> & {
  layoutsByTabId: Record<string, MixedTabPaneLayout>;
};

function createSession(id: string): TerminalSession {
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

describe("TerminalSplitView mixed terminal and browser pane layout", () => {
  it("renders terminal and browser pane leaves inside a single top-level terminal tab", () => {
    const singleTerminalTab: TerminalTab = {
      kind: "terminal",
      id: "tab-term-1",
      label: "Terminal Workspace",
      sessionId: "session-term-1",
    };

    const mixedLayout: MixedLayoutState = {
      tabs: [singleTerminalTab],
      activeTabId: singleTerminalTab.id,
      tabGroups: {
        "group-default": {
          id: "group-default",
          tabIds: [singleTerminalTab.id],
          activeTabId: singleTerminalTab.id,
        },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {
        [singleTerminalTab.id]: {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-term" },
            second: { type: "leaf", leafId: "leaf-browser" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-term",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-term": "session-term-1",
            "leaf-browser": "",
          },
          contentsByLeafId: {
            "leaf-term": {
              kind: "terminal",
              sessionId: "session-term-1",
            },
            "leaf-browser": {
              kind: "browser",
              browserId: "browser-leaf-1",
              url: "https://example.com/docs",
              title: "Documentation",
              loading: false,
              canGoBack: true,
              canGoForward: false,
              profileId: "default",
            },
          },
        },
      },
    };

    const sessions: Record<string, TerminalSession> = {
      "session-term-1": createSession("session-term-1"),
    };

    const onNavigateBrowserTab = vi.fn();
    const onReloadBrowserTab = vi.fn();

    render(
      <TerminalSplitView
        layout={mixedLayout as LayoutState}
        sessions={sessions}
        onNavigateBrowserTab={onNavigateBrowserTab}
        onReloadBrowserTab={onReloadBrowserTab}
      />,
    );

    // 1. One tab strip and one tab label (no separate browser tab in tab strip)
    const tabStrips = screen.getAllByTestId("tab-strip");
    expect(tabStrips).toHaveLength(1);

    const renderedTabs = screen.getAllByRole("tab");
    expect(renderedTabs).toHaveLength(1);
    expect(renderedTabs[0]).toHaveTextContent("Terminal Workspace");

    // 2. No separate browser tab or secondary tab-group panel exists
    const tabGroupPanels = screen.getAllByTestId("tab-group-panel");
    expect(tabGroupPanels).toHaveLength(1);

    // 3. One TerminalPane and one BrowserPane render in sibling pane leaves
    const paneLeaves = screen.getAllByTestId("pane-leaf");
    expect(paneLeaves).toHaveLength(2);

    const terminalPane = screen.getByTestId("terminal-pane");
    const browserPane = screen.getByTestId("browser-pane");

    expect(paneLeaves[0]).toContainElement(terminalPane);
    expect(paneLeaves[1]).toContainElement(browserPane);
    expect(terminalPane).toHaveAttribute("data-session-id", "session-term-1");

    // 4. Browser receives its own metadata and navigation/reload callbacks
    expect(browserPane).toHaveAttribute("data-browser-id", "browser-leaf-1");
    expect(browserPane).toHaveAttribute("data-url", "https://example.com/docs");
    expect(screen.getByTestId("browser-title")).toHaveTextContent("Documentation");

    fireEvent.click(screen.getByTestId("browser-navigate-button"));
    expect(onNavigateBrowserTab).toHaveBeenCalledWith("tab-term-1", "https://example.com/updated");

    fireEvent.click(screen.getByTestId("browser-reload-button"));
    expect(onReloadBrowserTab).toHaveBeenCalledWith("tab-term-1");

    // 5. Both pane-edge zones mount
    const termEdgeDropZones = within(paneLeaves[0]).getAllByTestId("pane-edge-drop-zone");
    const browserEdgeDropZones = within(paneLeaves[1]).getAllByTestId("pane-edge-drop-zone");

    expect(termEdgeDropZones).toHaveLength(4);
    expect(browserEdgeDropZones).toHaveLength(4);
  });
});
