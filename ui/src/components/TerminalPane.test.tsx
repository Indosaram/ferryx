import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import dagRunSampleJson from "../state/__fixtures__/dagRunSample.json";
import { parseDagRunSnapshot } from "../lib/dagTypes";
import type { TerminalSession } from "../lib/types";
import { dagStore } from "../state/dagStore";
import { TerminalPane } from "./TerminalPane";

const dagSnapshot = parseDagRunSnapshot(dagRunSampleJson)!;

vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: vi.fn(({ sessionId, session }) => (
    <div
      data-testid="native-terminal-pane"
      data-session-id={sessionId}
      data-backend-id={session.backendSessionId}
    />
  )),
}));

vi.mock("./TerminalSearchOverlay", () => ({
  TerminalSearchOverlay: vi.fn(({ sessionId, onClose }) => (
    <div data-testid="terminal-search-overlay" data-session-id={sessionId} onClick={onClose} />
  )),
}));

function createSession(id = "session-native"): TerminalSession {
  return {
    id,
    cwd: "/repo/main",
    workspaceId: "ws-main",
    worktree: { wsId: "ws-main", slug: "main" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

describe("TerminalPane native routing contract", () => {
  beforeEach(() => {
    dagStore.reset();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("mounts NativeTerminalPane for active and inactive states and conditionally renders search overlay", () => {
    const session = createSession();
    const onCloseSearch = vi.fn();
    const { rerender } = render(
      <TerminalPane
        session={session}
        active={true}
        searchOpen={false}
        onCloseSearch={onCloseSearch}
      />,
    );

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-search-overlay")).toBeNull();

    rerender(
      <TerminalPane
        session={session}
        active={false}
        searchOpen={true}
        onCloseSearch={onCloseSearch}
      />,
    );

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-overlay")).toBeInTheDocument();
    expect(screen.getByTestId("terminal-search-overlay")).toHaveAttribute("data-session-id", "backend-session-native");
  });

  it("binds the floating DAG badge to the terminal worktree rather than its nested cwd", () => {
    dagStore.applySnapshot("/repo/worktree", {
      ...dagSnapshot,
      runId: "worktree-dag",
      status: "running",
    });
    const session = {
      ...createSession(),
      cwd: "/repo/worktree/packages/ui",
      worktreePath: "/repo/worktree",
    };

    render(<TerminalPane session={session} active={true} />);

    expect(screen.getByTestId("dag-pane-badge")).toBeInTheDocument();
  });
});
