import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ConnectionStatusPayload, SshHost, Worktree } from "../lib/types";

const mocks = vi.hoisted(() => ({
  connectionListeners: new Set<(payload: ConnectionStatusPayload) => void>(),
  invoke: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  isTauri: vi.fn(() => true),
  Channel: class MockChannel {},
}));

vi.mock("../lib/terminalEvents", () => ({
  ensureTerminalEvents: vi.fn(async () => undefined),
  terminalEventBus: {
    subscribeLifecycle: vi.fn(() => () => undefined),
    subscribeConnectionStatus: vi.fn((listener: (payload: ConnectionStatusPayload) => void) => {
      mocks.connectionListeners.add(listener);
      return () => mocks.connectionListeners.delete(listener);
    }),
    clearSession: vi.fn(),
  },
}));

vi.mock("../lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  spawnTerminal: vi.fn(async () => "backend-ssh-1"),
  closeTerminal: vi.fn(async () => undefined),
  getTerminalCwd: vi.fn(async () => null),
  waitForTerminalExit: vi.fn(async () => undefined),
  onNativeTerminalTitle: vi.fn(async () => () => undefined),
  onNativeTerminalBell: vi.fn(async () => () => undefined),
  onNativeTerminalAgentState: vi.fn(async () => () => undefined),
}));

const { useWorkspaceStore } = await import("./workspaceStore");
const { SshConnectionStatusBadge } = await import("../components/NativeTerminalPane");
type WorkspaceServices = import("./workspaceStore").WorkspaceServices;

const worktree: Worktree = {
  path: "ssh://host-1/repo",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const host: SshHost = {
  id: "host-1",
  label: "Remote",
  hostname: "example.test",
  source: "manual",
  authMethod: "agent",
};

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "backend-ssh-1"),
    getTerminalCwd: vi.fn(async () => null),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

function emitConnectionStatus(payload: ConnectionStatusPayload): void {
  for (const listener of mocks.connectionListeners) listener(payload);
}

describe("ssh connection status", () => {
  afterEach(cleanup);

  beforeEach(() => {
    mocks.connectionListeners.clear();
    mocks.invoke.mockClear();
  });

  it("maps backend connected -> reconnecting -> failed events onto the local ssh session", async () => {
    const { result, unmount } = renderHook(() =>
      useWorkspaceStore({ initialWorktrees: [worktree], services: services() }),
    );

    await act(async () => {
      await result.current.openSshHostTerminal(host, { path: "/repo" });
    });
    await waitFor(() => expect(mocks.connectionListeners.size).toBe(1));

    const localSessionId = Object.keys(result.current.state.sessions)[0];
    expect(localSessionId).toBeTruthy();

    act(() => emitConnectionStatus({ sessionId: "backend-ssh-1", state: "connected", attempt: 0 }));
    expect(result.current.state.sessions[localSessionId].connectionStatus?.state).toBe("connected");

    act(() => emitConnectionStatus({ sessionId: "backend-ssh-1", state: "reconnecting", kind: "network", attempt: 1 }));
    expect(result.current.state.sessions[localSessionId].connectionStatus).toMatchObject({
      state: "reconnecting",
      kind: "network",
      attempt: 1,
    });

    act(() => emitConnectionStatus({ sessionId: "backend-ssh-1", state: "reconnection-failed", kind: "network", attempt: 5 }));
    expect(result.current.state.sessions[localSessionId].connectionStatus?.state).toBe("reconnection-failed");

    await act(async () => {
      await result.current.retrySshConnection(localSessionId);
    });
    expect(mocks.invoke).toHaveBeenCalledWith("cmd_terminal_retry", { sessionId: "backend-ssh-1" });
    unmount();
  });

  it("hides healthy status, shows neutral abnormal labels, and Retry invokes the daemon IPC", async () => {
    const { rerender } = render(
      <SshConnectionStatusBadge
        sessionId="backend-ssh-1"
        status={{ sessionId: "backend-ssh-1", state: "connected", attempt: 0 }}
      />,
    );
    expect(screen.queryByTestId("ssh-connection-status")).toBeNull();

    rerender(
      <SshConnectionStatusBadge
        sessionId="backend-ssh-1"
        status={{ sessionId: "backend-ssh-1", state: "reconnecting", kind: "network", attempt: 1 }}
      />,
    );
    expect(screen.getByText("Reconnecting...")).toBeInTheDocument();
    expect(screen.getByTestId("ssh-connection-status").className).not.toMatch(/blue/i);

    rerender(
      <SshConnectionStatusBadge
        sessionId="backend-ssh-1"
        status={{ sessionId: "backend-ssh-1", state: "reconnection-failed", kind: "network", attempt: 5 }}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("cmd_terminal_retry", { sessionId: "backend-ssh-1" }));
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("renders a neutral redeploy affordance for daemon-gone continuity sessions", async () => {
    render(
      <SshConnectionStatusBadge
        sessionId="backend-ssh-1"
        status={{ sessionId: "backend-ssh-1", state: "daemon-gone", kind: "daemon-gone", attempt: 2 }}
      />,
    );

    expect(screen.getByText("Remote daemon unavailable")).toBeInTheDocument();
    const prompt = screen.getByTestId("ssh-connection-status");
    expect(prompt.className).toContain("bg-muted");
    fireEvent.click(screen.getByRole("button", { name: "Enable / redeploy" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("cmd_terminal_retry", { sessionId: "backend-ssh-1" }),
    );
  });
});
