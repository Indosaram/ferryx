import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LayoutState, TerminalSession } from "../lib/types";
import type { PaneView } from "../state/paneTree";
import { TerminalSplitView, type RenderChatPane } from "./TerminalSplitView";

const terminalMounts = vi.hoisted(() => ({ count: 0 }));

vi.mock("./TerminalPane", async () => {
  const React = await import("react");
  return {
    TerminalPane: ({ session, active, inputActive }: { session: TerminalSession; active: boolean; inputActive: boolean }) => {
      React.useEffect(() => {
        terminalMounts.count += 1;
      }, []);
      return <div data-testid="terminal-pane" data-session-id={session.id} data-active={String(active)} data-input-active={String(inputActive)} />;
    },
  };
});

afterEach(() => {
  cleanup();
  terminalMounts.count = 0;
});

function agentSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "session-1",
    cwd: "/repo",
    worktreePath: "/repo",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: "backend-1",
    lifecycle: "working",
    agentType: "claude",
    ...overrides,
  };
}

function layout(view?: PaneView): LayoutState {
  return {
    tabs: [{ id: "tab-1", label: "Claude", sessionId: "session-1" }],
    activeTabId: "tab-1",
    layoutsByTabId: {
      "tab-1": {
        root: { type: "leaf", leafId: "leaf-1", ...(view ? { view } : {}) },
        activeLeafId: "leaf-1",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-1": "session-1" },
      },
    },
  };
}

const renderChatPane: RenderChatPane = ({ session, onShowTerminal }) => (
  <div data-testid="chat-pane" data-session-id={session.id}>
    <button type="button" onClick={onShowTerminal}>back</button>
  </div>
);

describe("TerminalSplitView pane chat view", () => {
  it("toggles view through onSetPaneView without remounting the terminal", () => {
    const onSetPaneView = vi.fn();
    const sessions = { "session-1": agentSession() };
    const props = { sessions, chatViewAvailable: true, onSetPaneView, renderChatPane };
    const { rerender } = render(<TerminalSplitView layout={layout()} {...props} />);

    expect(screen.queryByTestId("chat-pane")).toBeNull();
    fireEvent.click(screen.getByTestId("pane-view-toggle"));
    expect(onSetPaneView).toHaveBeenCalledWith("tab-1", "leaf-1", "chat");

    rerender(<TerminalSplitView layout={layout("chat")} {...props} />);
    expect(screen.getByTestId("chat-pane")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-pane")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("terminal-pane")).toHaveAttribute("data-input-active", "false");
    expect(screen.getByTestId("pane-terminal-layer")).toHaveClass("invisible");

    fireEvent.click(screen.getByTestId("pane-view-toggle"));
    expect(onSetPaneView).toHaveBeenLastCalledWith("tab-1", "leaf-1", "terminal");
    fireEvent.click(screen.getByRole("button", { name: "back" }));
    expect(onSetPaneView).toHaveBeenLastCalledWith("tab-1", "leaf-1", "terminal");

    rerender(<TerminalSplitView layout={layout()} {...props} />);
    expect(screen.queryByTestId("chat-pane")).toBeNull();
    expect(terminalMounts.count).toBe(1);
  });

  it("keeps an exited agent pane in chat view and lets it return to the terminal", () => {
    const onSetPaneView = vi.fn();
    render(
      <TerminalSplitView
        layout={layout("chat")}
        sessions={{ "session-1": agentSession({ lifecycle: "exited", backendSessionId: null, lastBackendSessionId: "backend-1" }) }}
        chatViewAvailable
        onSetPaneView={onSetPaneView}
        renderChatPane={renderChatPane}
      />,
    );
    expect(screen.getByTestId("chat-pane")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("pane-view-toggle"));
    expect(onSetPaneView).toHaveBeenCalledWith("tab-1", "leaf-1", "terminal");
  });

  it("does not offer chat for non-agent panes, gated hosts, or without a chat renderer", () => {
    const onSetPaneView = vi.fn();
    const { rerender } = render(
      <TerminalSplitView
        layout={layout()}
        sessions={{ "session-1": agentSession({ agentType: null }) }}
        chatViewAvailable
        onSetPaneView={onSetPaneView}
        renderChatPane={renderChatPane}
      />,
    );
    expect(screen.queryByTestId("pane-view-toggle")).toBeNull();

    rerender(
      <TerminalSplitView
        layout={layout()}
        sessions={{ "session-1": agentSession() }}
        chatViewAvailable={false}
        onSetPaneView={onSetPaneView}
        renderChatPane={renderChatPane}
      />,
    );
    expect(screen.queryByTestId("pane-view-toggle")).toBeNull();

    rerender(
      <TerminalSplitView
        layout={layout("chat")}
        sessions={{ "session-1": agentSession() }}
        chatViewAvailable
        onSetPaneView={onSetPaneView}
      />,
    );
    expect(screen.queryByTestId("chat-pane")).toBeNull();
    expect(screen.queryByTestId("pane-view-toggle")).toBeNull();
    expect(screen.getByTestId("pane-terminal-layer")).not.toHaveClass("invisible");
  });
});
