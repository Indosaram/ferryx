import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LayoutState, TerminalSession, TerminalTab } from "../lib/types";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-session-id={session.id}>
      <textarea data-testid="native-terminal-focus-sink" />
    </div>
  ),
}));

afterEach(() => {
  cleanup();
});

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    worktreePath: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

function splitLayout(): LayoutState {
  return {
    tabs: [tab("tab-1", "session-1")],
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
        sessionIdsByLeafId: {
          "leaf-1": "session-1",
          "leaf-2": "session-2",
        },
      },
    },
  };
}

describe("TerminalSplitView pane click focus restoration", () => {
  it("focuses the terminal input sink and calls onFocusPane on pointerdown on a pane leaf", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const leaves = screen.getAllByTestId("pane-leaf");
    expect(leaves).toHaveLength(2);
    const secondLeaf = leaves[1];
    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const secondSink = sinks[1];
    const focusSpy = vi.spyOn(secondSink, "focus");

    fireEvent.pointerDown(secondLeaf);

    expect(onFocusPane).toHaveBeenCalledWith("tab-1", "leaf-2");
    expect(focusSpy).toHaveBeenCalled();
  });

  it("focuses the terminal input sink and calls onFocusPane on click on a pane leaf", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const leaves = screen.getAllByTestId("pane-leaf");
    const secondLeaf = leaves[1];
    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const secondSink = sinks[1];
    const focusSpy = vi.spyOn(secondSink, "focus");

    fireEvent.click(secondLeaf);

    expect(onFocusPane).toHaveBeenCalledWith("tab-1", "leaf-2");
    expect(focusSpy).toHaveBeenCalled();
  });

  it("focuses the input sink when clicking the pane toolbar", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const toolbars = screen.getAllByTestId("pane-toolbar");
    expect(toolbars).toHaveLength(2);
    const secondToolbar = toolbars[1];
    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const secondSink = sinks[1];
    const focusSpy = vi.spyOn(secondSink, "focus");

    fireEvent.pointerDown(secondToolbar);

    expect(onFocusPane).toHaveBeenCalledWith("tab-1", "leaf-2");
    expect(focusSpy).toHaveBeenCalled();
  });

  it("does not focus the input sink when clicking action buttons inside the toolbar", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const splitButtons = screen.getAllByRole("button", { name: /split pane right/i });
    expect(splitButtons.length).toBeGreaterThan(0);
    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const focusSpy = vi.spyOn(sinks[0], "focus");

    fireEvent.pointerDown(splitButtons[0]);
    expect(focusSpy).not.toHaveBeenCalled();
  });

  it("does not steal focus when clicking an interactive input inside a pane leaf", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const leaves = screen.getAllByTestId("pane-leaf");
    const firstLeaf = leaves[0];
    const searchInput = document.createElement("input");
    searchInput.setAttribute("data-testid", "terminal-search-input");
    firstLeaf.appendChild(searchInput);

    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const firstSink = sinks[0];
    const focusSpy = vi.spyOn(firstSink, "focus");

    fireEvent.pointerDown(searchInput);
    fireEvent.click(searchInput);

    expect(focusSpy).not.toHaveBeenCalled();
    firstLeaf.removeChild(searchInput);
  });

  it("restores sink focus when clicking an already active pane", () => {
    const onFocusPane = vi.fn();
    const layout = splitLayout();
    const sessions = {
      "session-1": session("session-1"),
      "session-2": session("session-2"),
    };

    render(
      <TerminalSplitView
        layout={layout}
        sessions={sessions}
        onFocusPane={onFocusPane}
      />,
    );

    const leaves = screen.getAllByTestId("pane-leaf");
    const firstLeaf = leaves[0];
    const sinks = screen.getAllByTestId("native-terminal-focus-sink");
    const firstSink = sinks[0];
    const focusSpy = vi.spyOn(firstSink, "focus");

    fireEvent.click(firstLeaf);

    expect(onFocusPane).toHaveBeenCalledWith("tab-1", "leaf-1");
    expect(focusSpy).toHaveBeenCalled();
  });
});
