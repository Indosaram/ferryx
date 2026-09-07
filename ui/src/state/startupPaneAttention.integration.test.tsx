import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { serializeWorkspaceState, deserializeWorkspaceState } from "../lib/sessionPersistence";
import { TerminalSplitView } from "../components/TerminalSplitView";
import { resetNotificationSettings } from "../lib/notificationSettings";
import type { LayoutState, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import type { WorkspaceState } from "./workspaceStore";

vi.mock("../components/TerminalPane", () => ({
  TerminalPane: ({
    session,
    needsAttention,
  }: {
    session: TerminalSession;
    needsAttention?: boolean;
  }) => (
    <div
      data-testid="terminal-pane"
      data-session-id={session.id}
      data-needs-attention={String(Boolean(needsAttention))}
    />
  ),
}));

beforeEach(() => {
  resetNotificationSettings();
});

afterEach(() => {
  cleanup();
  resetNotificationSettings();
});

describe("app startup pane attention frame integration", () => {
  it("restores split panes with seen: true and does not render attention frames on startup", () => {
    const tab1: TerminalTab = { id: "tab-1", label: "main", sessionId: "sess-1" };
    const sessions: Record<string, TerminalSession> = {
      "sess-1": {
        id: "sess-1",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-default",
        worktree: { wsId: "ws-default", slug: "main" },
        backendSessionId: "backend-1",
        lifecycle: "running",
        agentType: "omo",
      },
      "sess-2": {
        id: "sess-2",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "ws-default",
        worktree: { wsId: "ws-default", slug: "main" },
        backendSessionId: "backend-2",
        lifecycle: "running",
        agentType: "claude",
      },
    };

    const splitLayout: LayoutState = {
      tabs: [tab1],
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            ratio: 0.5,
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
          },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: {
            "leaf-1": "sess-1",
            "leaf-2": "sess-2",
          },
        },
      },
    };

    const worktrees: Worktree[] = [
      {
        path: "/repo",
        head: "main",
        branch: "main",
        bare: false,
        detached: false,
        locked: null,
        prunable: null,
      },
    ];

    const originalState: WorkspaceState = {
      workspaceId: "ws-default",
      worktrees,
      activeWorktreePath: "/repo",
      sessions,
      layout: splitLayout,
      unreadTabIds: {},
      unreadWorktreePaths: {},
      activityBySessionId: {
        "sess-1": {
          state: "working",
          title: "omo working",
          isAgent: true,
          agentType: "omo",
          source: "screen",
          seen: false,
        },
        "sess-2": {
          state: "waiting",
          title: "claude waiting",
          isAgent: true,
          agentType: "claude",
          source: "screen",
          seen: false,
        },
      },
    };

    const persisted = serializeWorkspaceState("ws-default", "/repo", originalState);
    const liveBackendIds = [{ sessionId: "backend-1" }, { sessionId: "backend-2" }];
    const restored = deserializeWorkspaceState("ws-default", persisted, liveBackendIds);
    expect(restored).not.toBeNull();
    if (!restored) return;

    expect(restored.activityBySessionId?.["sess-1"]?.seen).toBe(true);
    expect(restored.activityBySessionId?.["sess-2"]?.seen).toBe(true);

    render(
      <TerminalSplitView
        layout={restored.layout}
        sessions={restored.sessions}
        activityBySessionId={restored.activityBySessionId}
      />,
    );

    expect(screen.queryByTestId("attention-frame-bottom")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-left")).not.toBeInTheDocument();
    expect(screen.queryByTestId("attention-frame-corner-right")).not.toBeInTheDocument();

    const terminalPanes = screen.getAllByTestId("terminal-pane");
    expect(terminalPanes).toHaveLength(2);
    for (const pane of terminalPanes) {
      expect(pane).toHaveAttribute("data-needs-attention", "false");
    }
  });
});
