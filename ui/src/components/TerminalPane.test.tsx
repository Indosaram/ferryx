import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { TerminalPane } from "./TerminalPane";

vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: vi.fn(({ sessionId, session, onBell, onTitleChange }) => (
    <div
      data-testid="native-terminal-pane"
      data-session-id={sessionId}
      data-backend-id={session.backendSessionId}
      data-has-bell-handler={typeof onBell === "function"}
      data-has-title-handler={typeof onTitleChange === "function"}
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
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("mounts NativeTerminalPane for active and inactive states and conditionally renders search overlay", () => {
    const session = createSession();
    const onCloseSearch = vi.fn();
    const onBell = vi.fn();
    const onTitleChange = vi.fn();
    const { rerender } = render(
      <TerminalPane
        session={session}
        active={true}
        onBell={onBell}
        onTitleChange={onTitleChange}
        searchOpen={false}
        onCloseSearch={onCloseSearch}
      />,
    );

    expect(screen.getByTestId("native-terminal-pane")).toBeInTheDocument();
    expect(screen.queryByTestId("terminal-search-overlay")).toBeNull();
    expect(screen.getByTestId("native-terminal-pane")).toHaveAttribute("data-has-bell-handler", "true");
    expect(screen.getByTestId("native-terminal-pane")).toHaveAttribute("data-has-title-handler", "true");

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
});
