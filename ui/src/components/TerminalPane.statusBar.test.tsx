import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { TerminalSession } from "../lib/types";
import { OUTAGE_NOTICE_GRACE_MS, TerminalPane } from "./TerminalPane";

vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="native-terminal" data-backend-id={session.backendSessionId} tabIndex={-1} />
  ),
}));
vi.mock("./dag/DagPaneBadge", () => ({ DagPaneBadge: () => null }));

function sshSession(overrides: Partial<TerminalSession> = {}): TerminalSession {
  return {
    id: "pane",
    workspaceId: "ssh:host-one:project",
    cwd: "/srv/project",
    worktreePath: "/srv/project",
    worktree: null,
    backendSessionId: "stable-backend",
    lifecycle: "running",
    remoteConnectionState: "connected",
    remoteGeneration: 1,
    ...overrides,
  } as TerminalSession;
}

describe("TerminalPane docked status bar", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 600, 36));
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("keeps a reconnect blip off screen and docks it at the top after the grace window", () => {
    render(<TerminalPane session={sshSession({ remoteConnectionState: "reconnecting" })} active />);
    const bar = screen.getByTestId("terminal-pane-overlay");
    const viewport = screen.getByTestId("terminal-native-viewport");

    expect(bar).toHaveAttribute("data-pane-status-bar", "pending");
    expect(bar).toHaveClass("sr-only");
    expect(viewport).toHaveStyle({ top: "0px", bottom: "0px" });

    act(() => { vi.advanceTimersByTime(OUTAGE_NOTICE_GRACE_MS); });

    expect(bar).toHaveAttribute("data-pane-status-bar", "docked");
    expect(bar).toHaveAttribute("data-pane-status-edge", "top");
    expect(bar).not.toHaveClass("sr-only");
    expect(viewport).toHaveStyle({ top: "36px", bottom: "0px" });
  });

  it("never shows a reconnect that recovers inside the grace window", () => {
    const view = render(<TerminalPane session={sshSession({ remoteConnectionState: "reconnecting" })} active />);
    act(() => { vi.advanceTimersByTime(OUTAGE_NOTICE_GRACE_MS - 1); });
    view.rerender(<TerminalPane session={sshSession({ remoteConnectionState: "connected", remoteGeneration: 2 })} active />);
    act(() => { vi.advanceTimersByTime(OUTAGE_NOTICE_GRACE_MS); });

    expect(screen.queryByTestId("terminal-pane-overlay")).toBeNull();
    expect(screen.getByTestId("terminal-native-viewport")).toHaveStyle({ top: "0px", bottom: "0px" });
  });

  it("docks a recoverable disconnect at the top immediately and runs its primary action on Enter", () => {
    const onReconnect = vi.fn();
    render(<TerminalPane session={sshSession({ remoteConnectionState: "disconnected" })} active onReconnect={onReconnect} />);
    const bar = screen.getByTestId("terminal-pane-overlay");

    expect(bar).toHaveAttribute("data-pane-status-edge", "top");
    expect(bar).toHaveAttribute("aria-keyshortcuts", "Enter");
    expect(bar.querySelector("time")).toBeNull();

    fireEvent.keyDown(screen.getByTestId("native-terminal"), { key: "Enter" });
    expect(onReconnect).toHaveBeenCalledWith("pane");
  });

  it("docks a final exit at the bottom with its exit time, and Enter opens a new shell", () => {
    vi.setSystemTime(new Date(2026, 10, 11, 14, 2));
    const onOpenNewShell = vi.fn();
    render(
      <TerminalPane
        session={{ ...sshSession({ workspaceId: "ws-local" }), lifecycle: "exited", exitCode: 1 }}
        active
        onOpenNewShell={onOpenNewShell}
      />,
    );
    const bar = screen.getByTestId("terminal-pane-overlay");
    expect(bar).toHaveAttribute("data-pane-status-edge", "bottom");
    expect(screen.getByTestId("terminal-native-viewport")).toHaveStyle({ top: "0px", bottom: "36px" });
    expect(bar.querySelector("time")).toHaveAttribute("datetime", new Date(2026, 10, 11, 14, 2).toISOString());

    fireEvent.keyDown(screen.getByTestId("native-terminal"), { key: "Enter", shiftKey: true });
    expect(onOpenNewShell).not.toHaveBeenCalled();
    fireEvent.keyDown(screen.getByTestId("native-terminal"), { key: "Enter" });
    expect(onOpenNewShell).toHaveBeenCalledWith("pane");
  });

  it("leaves Enter to an inactive pane's owner", () => {
    const onOpenNewShell = vi.fn();
    render(
      <TerminalPane
        session={{ ...sshSession({ workspaceId: "ws-local" }), lifecycle: "exited" }}
        active={false}
        onOpenNewShell={onOpenNewShell}
      />,
    );
    fireEvent.keyDown(screen.getByTestId("native-terminal"), { key: "Enter" });
    expect(onOpenNewShell).not.toHaveBeenCalled();
  });
});
