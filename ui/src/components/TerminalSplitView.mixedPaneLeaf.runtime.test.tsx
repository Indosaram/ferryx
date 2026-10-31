import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type {
  BrowserTab,
  FileTab,
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

vi.mock("./FilePreviewPane", () => ({
  FilePreviewPane: ({ path, visible }: { path: string; visible?: boolean }) => (
    <div data-testid="file-preview-pane" data-path={path} data-visible={String(visible)} />
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

type FilePaneLeafContent = {
  kind: "file";
  path: string;
  previewId?: string;
};

type PaneLeafContent = TerminalPaneLeafContent | BrowserPaneLeafContent | FilePaneLeafContent;

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
    expect(onNavigateBrowserTab).toHaveBeenCalledWith("tab-term-1", "https://example.com/updated", "browser-leaf-1");

    fireEvent.click(screen.getByTestId("browser-reload-button"));
    expect(onReloadBrowserTab).toHaveBeenCalledWith("tab-term-1", "browser-leaf-1", undefined);

    // 5. Both pane-edge zones mount
    const termEdgeDropZones = within(paneLeaves[0]).getAllByTestId("pane-edge-drop-zone");
    const browserEdgeDropZones = within(paneLeaves[1]).getAllByTestId("pane-edge-drop-zone");

    expect(termEdgeDropZones).toHaveLength(4);
    expect(browserEdgeDropZones).toHaveLength(4);

    // 6. Copy Debug Info button exists on terminal pane toolbar, omitted on browser pane toolbar
    const termToolbar = within(paneLeaves[0]).getByTestId("pane-toolbar");
    const browserToolbar = within(paneLeaves[1]).getByTestId("pane-toolbar");
    expect(within(termToolbar).getByRole("button", { name: "Copy Debug Info" })).toBeInTheDocument();
    expect(within(browserToolbar).queryByRole("button", { name: "Copy Debug Info" })).not.toBeInTheDocument();
  });

  it("shows Copy Debug Info button only for terminal content, omitting it on browser and file panes", () => {
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
            second: {
              type: "split",
              direction: "vertical",
              first: { type: "leaf", leafId: "leaf-browser" },
              second: { type: "leaf", leafId: "leaf-file" },
              ratio: 0.5,
            },
            ratio: 0.5,
          },
          activeLeafId: "leaf-term",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-term": "session-term-1",
            "leaf-browser": "",
            "leaf-file": "",
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
            "leaf-file": {
              kind: "file",
              path: "/repo/readme.md",
              previewId: "preview-readme",
              backendSessionId: "",
              line: null,
              col: null,
              workspaceId: null,
            },
          },
        },
      },
    };

    const sessions: Record<string, TerminalSession> = {
      "session-term-1": createSession("session-term-1"),
    };

    render(
      <TerminalSplitView
        layout={mixedLayout as LayoutState}
        sessions={sessions}
        onNavigateBrowserTab={vi.fn()}
        onReloadBrowserTab={vi.fn()}
      />,
    );

    const paneLeaves = screen.getAllByTestId("pane-leaf");
    expect(paneLeaves).toHaveLength(3);

    // Terminal leaf: has Copy Debug Info
    const termToolbar = within(paneLeaves[0]).getByTestId("pane-toolbar");
    expect(within(termToolbar).getByRole("button", { name: "Copy Debug Info" })).toBeInTheDocument();

    // Browser leaf: no Copy Debug Info
    const browserToolbar = within(paneLeaves[1]).getByTestId("pane-toolbar");
    expect(within(browserToolbar).queryByRole("button", { name: "Copy Debug Info" })).not.toBeInTheDocument();

    // File leaf: no Copy Debug Info
    const fileToolbar = within(paneLeaves[2]).getByTestId("pane-toolbar");
    expect(within(fileToolbar).queryByRole("button", { name: "Copy Debug Info" })).not.toBeInTheDocument();
  });

  it("marks only the active file tab's preview pane visible", () => {
    const fileTab = (id: string, path: string): FileTab => ({
      kind: "file",
      id,
      label: path.split("/").pop() ?? path,
      path,
      backendSessionId: "backend-1",
      line: null,
      col: null,
      workspaceId: null,
      previewId: id,
    });
    const first = fileTab("tab-file-a", "/repo/a.md");
    const second = fileTab("tab-file-b", "/repo/b.md");
    const layout = (activeTabId: string): LayoutState => ({
      tabs: [first, second],
      activeTabId,
      tabGroups: {
        "group-default": { id: "group-default", tabIds: [first.id, second.id], activeTabId },
      },
      tabGroupLayout: { type: "group", groupId: "group-default" },
      focusedGroupId: "group-default",
      layoutsByTabId: {},
    });
    const visibilityByPath = () =>
      Object.fromEntries(
        screen
          .getAllByTestId("file-preview-pane")
          .map((element): [string, string] => [
            element.getAttribute("data-path") ?? "",
            element.getAttribute("data-visible") ?? "",
          ]),
      );

    const { rerender } = render(
      <TerminalSplitView layout={layout(first.id)} sessions={{}} onNavigateBrowserTab={vi.fn()} onReloadBrowserTab={vi.fn()} />,
    );
    expect(visibilityByPath()).toEqual({ "/repo/a.md": "true", "/repo/b.md": "false" });

    rerender(
      <TerminalSplitView layout={layout(second.id)} sessions={{}} onNavigateBrowserTab={vi.fn()} onReloadBrowserTab={vi.fn()} />,
    );
    expect(visibilityByPath()).toEqual({ "/repo/a.md": "false", "/repo/b.md": "true" });
  });
});
