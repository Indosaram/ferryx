import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TerminalSession, TerminalTab } from "../lib/types";
import { createLayoutState, layoutReducer } from "../state/layout";
import { TerminalSplitView } from "./TerminalSplitView";

vi.mock("./TerminalPane", () => ({
  TerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="terminal-pane" data-backend-session-id={session.backendSessionId ?? ""} />
  ),
}));

function tab(id: string, sessionId: string): TerminalTab {
  return { id, label: id, sessionId };
}

function session(id: string, backendSessionId: string): TerminalSession {
  return {
    id,
    cwd: "/repo",
    workspaceId: "ws-main",
    worktreeId: "wt-main",
    backendSessionId,
    lifecycle: "working",
  };
}

describe("TerminalSplitView", () => {
  it("renders a secondary pane immediately for an atomic single-tab split state", () => {
    const primary = tab("tab-1", "session-1");
    const secondary = tab("tab-2", "session-2");
    const layout = layoutReducer(createLayoutState([primary], primary.id), {
      type: "ENABLE_SPLIT",
      orientation: "horizontal",
      secondaryTab: secondary,
    });
    const sessions = {
      "session-1": session("session-1", "backend-1"),
      "session-2": session("session-2", "backend-2"),
    };

    render(<TerminalSplitView layout={layout} sessions={sessions} />);

    expect(screen.getByTestId("primary-pane")).toBeInTheDocument();
    expect(screen.getByTestId("secondary-pane")).toBeInTheDocument();
    expect(screen.getAllByTestId("terminal-pane")).toHaveLength(2);
  });
});
