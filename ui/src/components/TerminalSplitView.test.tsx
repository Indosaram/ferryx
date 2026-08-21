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

describe("TerminalSplitView with per-tab tree splits and drag handles", () => {
  it("renders a single pane with move handle and split controls", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1" },
        },
      },
    };
    const sessions = { "session-1": session("session-1", "backend-1") };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);

    expect(screen.getByTestId("pane-leaf")).toBeInTheDocument();
    expect(screen.getByTestId("pane-drag-handle")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane right" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Split pane down" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close split view" })).not.toBeInTheDocument();
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
    expect(screen.getAllByTestId("pane-drag-handle")).toHaveLength(3);
    expect(screen.getAllByRole("separator", { name: "Resize terminal panes" })).toHaveLength(2);
  });

  it("dispatches split actions from pane toolbar buttons", () => {
    const primary = tab("tab-1", "session-1");
    const layout: LayoutState = {
      tabs: [primary],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: { type: "leaf", leafId: "leaf-1" },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "session-1" },
        },
      },
    };
    const sessions = { "session-1": session("session-1", "backend-1") };
    const onSplitPane = vi.fn();

    render(<TerminalSplitView layout={layout} sessions={sessions} onSplitPane={onSplitPane} />);

    fireEvent.click(screen.getByRole("button", { name: "Split pane right" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "horizontal");

    fireEvent.click(screen.getByRole("button", { name: "Split pane down" }));
    expect(onSplitPane).toHaveBeenCalledWith("tab-1", "leaf-1", "vertical");
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

    const handles = screen.getAllByTestId("pane-drag-handle");
    const panes = screen.getAllByTestId("pane-leaf");

    fireEvent.dragStart(handles[0], {
      dataTransfer: { setData: vi.fn(), effectAllowed: "" },
    });

    fireEvent.dragOver(panes[1], {
      dataTransfer: { dropEffect: "" },
      preventDefault: vi.fn(),
    });
    fireEvent.drop(panes[1], {
      preventDefault: vi.fn(),
    });

    expect(onSwapPanes).toHaveBeenCalledWith("tab-1", "leaf-1", "leaf-2");
  });
});
