import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-backend-session-id={session.backendSessionId ?? session.id} />
  ),
}));

afterEach(cleanup);

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string, backendSessionId: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId,
    lifecycle: "working",
  };
}

function singleTabLayout(tabId = "tab-1", sessionId = "session-1", leafId = "leaf-1"): LayoutState {
  return {
    tabs: [tab(tabId, sessionId)],
    activeTabId: tabId,
    layoutsByTabId: {
      [tabId]: {
        root: { type: "leaf", leafId },
        activeLeafId: leafId,
        expandedLeafId: null,
        sessionIdsByLeafId: { [leafId]: sessionId },
      },
    },
  };
}

describe("TerminalSplitView with per-tab tree splits and drag handles", () => {
  it("renders a single pane with move handle and split controls", () => {
    const layout = singleTabLayout();
    const sessions = { "session-1": session("session-1", "backend-1") };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);

    expect(screen.getByTestId("pane-leaf")).toBeInTheDocument();
    expect(screen.getByTestId("pane-toolbar")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane right" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane down" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close split view" })).not.toBeInTheDocument();
    expect(screen.getByTestId("pane-toolbar")).toHaveClass("z-30");
    expect(screen.getByTestId("pane-toolbar-hotspot")).toHaveClass("z-20");
  });

  it("keeps the tab bar usable when all tabs have been closed", () => {
    const onAddTab = vi.fn();
    const layout: LayoutState = { tabs: [], activeTabId: null, layoutsByTabId: {} };

    render(<TerminalSplitView layout={layout} sessions={{}} onAddTab={onAddTab} />);

    expect(screen.getByTestId("tab-strip")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Terminal/i }));
    expect(onAddTab).toHaveBeenCalledOnce();
  });

  it("renders a nested horizontal and vertical split tree with resize dividers", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-left" },
            second: {
              type: "split",
              direction: "vertical",
              first: { type: "leaf", leafId: "leaf-right-top" },
              second: { type: "leaf", leafId: "leaf-right-bottom" },
              ratio: 0.5,
            },
            ratio: 0.5,
          },
          activeLeafId: "leaf-right-top",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-left": "session-1",
            "leaf-right-top": "session-2",
            "leaf-right-bottom": "session-3",
          },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
      "session-3": session("session-3", "backend-3"),
    };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);

    expect(screen.getAllByTestId("pane-leaf")).toHaveLength(3);
    expect(screen.getAllByTestId("pane-toolbar")).toHaveLength(3);
    expect(screen.getAllByRole("separator", { name: "Resize terminal panes" })).toHaveLength(2);
  });

  it("dispatches split actions from pane toolbar buttons", () => {
    const layout = singleTabLayout();
    const sessions = { "session-1": session("session-1", "backend-1") };
    const onSplitPane = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSplitPane={onSplitPane} />);

    fireEvent.click(screen.getByRole("button", { name: "Split pane right" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "horizontal");

    fireEvent.click(screen.getByRole("button", { name: "Split pane down" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "vertical");
  });

  it("routes a tab context-menu split to that tab's own focused leaf", () => {
    const layout: LayoutState = {
      tabs: [tab("tab-1", "session-1"), tab("tab-2", "session-2")],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-a" },
          activeLeafId: "leaf-a",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-a": "session-1" },
        },
        "tab-2": {
          root: {
            type: "split",
            direction: "vertical",
            first: { type: "leaf", leafId: "leaf-b-top" },
            second: { type: "leaf", leafId: "leaf-b-bottom" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-b-bottom",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-b-top": "session-2", "leaf-b-bottom": "session-2" },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
    };
    const onSplitPane = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSplitPane={onSplitPane} />);

    fireEvent.contextMenu(screen.getByText("tab-2"));
    fireEvent.click(screen.getByRole("menuitem", { name: /Split terminal right/i }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-2", "leaf-b-bottom", "horizontal");
  });

  it("does not start pane dragging from toolbar controls", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1", "leaf-2": "session-2" },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
    };
    const onSwapPanes = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSwapPanes={onSwapPanes} />);

    fireEvent.dragStart(screen.getAllByRole("button", { name: "Split pane right" })[0], {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    const panes = screen.getAllByTestId("pane-leaf");
    fireEvent.dragOver(panes[1], { dataTransfer: { dropEffect: "" } });
    fireEvent.drop(panes[1]);
    expect(onSwapPanes).not.toHaveBeenCalled();
  });

  it("handles drag-and-drop swap between panes", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1", "leaf-2": "session-2" },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
    };
    const onSwapPanes = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSwapPanes={onSwapPanes} />);

    const toolbars = screen.getAllByTestId("pane-toolbar");
    const panes = screen.getAllByTestId("pane-leaf");

    fireEvent.dragStart(toolbars[0], {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });
    fireEvent.dragOver(panes[1], { dataTransfer: { dropEffect: "" } });
    fireEvent.drop(panes[1]);

    expect(onSwapPanes).toHaveBeenCalledWith("tab-1", "leaf-1", "leaf-2");
  });

  it("falls back to an even ratio when a resize container is smaller than two minimum panes", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.8,
          },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1", "leaf-2": "session-2" },
        },
      },
    };
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
    };
    const onSetRatio = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSetRatio={onSetRatio} />);

    const divider = screen.getByRole("separator", { name: "Resize terminal panes" });
    const parent = divider.parentElement as HTMLElement;
    vi.spyOn(parent, "getBoundingClientRect").mockReturnValue({
      left: 0,
      right: 120,
      width: 120,
      top: 0,
      bottom: 100,
      height: 100,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    });

    fireEvent.pointerDown(divider, { clientX: 90, clientY: 20 });
    fireEvent.pointerMove(window, { clientX: 110, clientY: 20 });
    fireEvent.pointerUp(window);
    expect(onSetRatio).toHaveBeenCalledWith("tab-1", "", 0.5);
  });
});
