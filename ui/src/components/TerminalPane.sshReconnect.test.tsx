import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useReducer } from "react";
import { replaceExitedShellSession, clearShellReplacementInflightForTests } from "../lib/shellReplacement";
import type { TerminalSession } from "../lib/types";
import { workspaceReducer, type WorkspaceState } from "../state/workspaceStore";
import { TerminalPane } from "./TerminalPane";

vi.mock("./NativeTerminalPane", () => ({
  NativeTerminalPane: ({ session }: { session: TerminalSession }) => (
    <div data-testid="native-terminal" data-backend-id={session.backendSessionId} />
  ),
}));
vi.mock("./dag/DagPaneBadge", () => ({ DagPaneBadge: () => null }));

function connectedWorkspace(): WorkspaceState {
  return {
    workspaceId: "ssh:host-one:project",
    worktrees: [],
    activeWorktreePath: "/srv/project",
    sessions: {
      pane: {
        id: "pane",
        workspaceId: "ssh:host-one:project",
        cwd: "/srv/project",
        worktreePath: "/srv/project",
        worktree: null,
        backendSessionId: "stable-backend",
        lifecycle: "running",
        remoteConnectionState: "connected",
        remoteGeneration: 1,
        agentType: "claude",
        agentSessionId: "remote-agent-session",
        providerSession: { key: "session_id", id: "remote-agent-session" },
        daemonEpoch: "epoch-1",
        lastOutputSequence: "44",
      },
    },
    layout: {
      tabs: [{ id: "tab", sessionId: "pane", label: "SSH" }],
      activeTabId: "tab",
      primaryTabId: "tab",
      secondaryTabId: null,
      split: "none",
      layoutsByTabId: {
        tab: {
          root: { type: "leaf", leafId: "leaf" },
          activeLeafId: "leaf",
          expandedLeafId: null,
          sessionIdsByLeafId: { leaf: "pane" },
        },
      },
    },
    activityBySessionId: {
      pane: { state: "working", title: "Claude", isAgent: true, agentType: "claude" },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

afterEach(() => {
  cleanup();
  clearShellReplacementInflightForTests();
});

describe("SSH reconnect after daemon exit", () => {
  it("does not replace an SSH process through the exited-shell path", async () => {
    const connected = connectedWorkspace();
    const state = workspaceReducer(connected, {
      type: "SESSION_LIFECYCLE",
      backendSessionId: "stable-backend",
      lifecycle: "exited",
    });
    const spawn = vi.fn();
    const dispatch = vi.fn();

    const result = await replaceExitedShellSession("pane", {
      getSessions: () => state.sessions,
      dispatch,
      spawn,
      createRequestId: () => "ssh-retry",
    }).then(() => ({ rejected: false }), () => ({ rejected: true }));

    expect(spawn).not.toHaveBeenCalled();
    expect(dispatch).not.toHaveBeenCalled();
    expect(result.rejected).toBe(true);
    expect(state.layout).toBe(connected.layout);
  });

  it.each([null, { key: "session_id", id: "remote-provider" } as const])(
    "offers SSH reconnect independently of provider reference %j",
    async (providerSession) => {
      // Given: a disconnected SSH pane, whether or not the remote agent supplied a reference.
      const session: TerminalSession = {
        ...connectedWorkspace().sessions.pane,
        remoteConnectionState: "disconnected",
        providerSession,
      };
      const completion = new EventTarget();
      const pending = new Promise<void>((resolve) => {
        completion.addEventListener("complete", () => resolve(), { once: true });
      });
      const onReconnect = vi.fn(() => pending);
      const onOpenNewShell = vi.fn();
      render(<TerminalPane session={session} active onReconnect={onReconnect} onOpenNewShell={onOpenNewShell} />);

      // When: user clicks Reconnect SSH.
      const button = screen.getByRole("button", { name: "Reconnect SSH" });
      fireEvent.click(button);
      fireEvent.click(button);

      // Then: only the reconnect reattachment path runs, once, with an accessible busy state.
      expect(onReconnect).toHaveBeenCalledExactlyOnceWith("pane");
      expect(onOpenNewShell).not.toHaveBeenCalled();
      expect(button).toBeDisabled();
      expect(button).toHaveAttribute("aria-busy", "true");
      await act(async () => {
        completion.dispatchEvent(new Event("complete"));
        await pending;
      });
    },
  );

  it("shows reconnecting state and auto-recovers without button activation", () => {
    // Given: an SSH session in automatic reconnecting state.
    const reconnectingSession: TerminalSession = {
      ...connectedWorkspace().sessions.pane,
      remoteConnectionState: "reconnecting",
    };
    const { rerender } = render(<TerminalPane session={reconnectingSession} active />);

    // Then: overlay is visible with spinner and title, without requiring button activation.
    expect(screen.getByRole("region")).toHaveTextContent("Reconnecting SSH...");
    expect(screen.queryByRole("button", { name: "Reconnect SSH" })).toBeNull();

    // When: reconnection succeeds and remote state transitions back to connected.
    const recoveredSession: TerminalSession = {
      ...reconnectingSession,
      remoteConnectionState: "connected",
    };
    rerender(<TerminalPane session={recoveredSession} active />);

    // Then: overlay disappears automatically.
    expect(screen.queryByTestId("terminal-pane-overlay")).toBeNull();
    expect(screen.getByTestId("native-terminal")).toHaveAttribute("data-backend-id", "stable-backend");
  });

  it("shows a failed SSH attempt and allows retry through the rendered pane", async () => {
    // Given: reconnecting fails with a structured error, then succeeds.
    const failure = { code: "IO_ERROR", message: "SSH host is unreachable", details: {} };
    let shouldFail = true;

    function Harness() {
      const [state, dispatch] = useReducer(workspaceReducer, {
        ...connectedWorkspace(),
        sessions: {
          pane: {
            ...connectedWorkspace().sessions.pane,
            remoteConnectionState: "disconnected",
          },
        },
      });

      const handleReconnect = async (_sessionId: string) => {
        if (shouldFail) {
          shouldFail = false;
          throw failure;
        }
        dispatch({
          type: "SESSION_REMOTE_STATUS",
          status: {
            sessionId: "stable-backend",
            state: "connected",
            generation: 2,
            failure: null,
            replayGap: null,
          },
        });
      };

      return <TerminalPane session={state.sessions.pane} active onReconnect={handleReconnect} />;
    }

    render(<Harness />);

    // When: reconnect fails.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Reconnect SSH" }));
    });

    // Then: alert shows the error message and preserves the stable pane.
    expect(screen.getByRole("alert")).toHaveTextContent(failure.message);

    // When: user retries.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Reconnect SSH" }));
    });

    // Then: reattachment succeeds and overlay disappears without minting a replacement backend.
    expect(screen.queryByTestId("terminal-pane-overlay")).toBeNull();
    expect(screen.getByTestId("native-terminal")).toHaveAttribute("data-backend-id", "stable-backend");
  });

  it("shows remote session expired when the remote process has exited", () => {
    const expiredSession: TerminalSession = {
      ...connectedWorkspace().sessions.pane,
      remoteConnectionState: "expired",
      remoteFailure: { kind: "missing", message: "Target not found on remote helper" },
    };
    render(<TerminalPane session={expiredSession} active />);

    expect(screen.getByText("Remote session expired")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reconnect SSH" })).toBeNull();
  });

  it("still rejects replacing a local agent with a fresh shell", async () => {
    const session: TerminalSession = {
      ...connectedWorkspace().sessions.pane,
      workspaceId: "local",
      backendSessionId: null,
      lifecycle: "exited" as const,
    };
    const spawn = vi.fn();
    await expect(replaceExitedShellSession("pane", {
      getSessions: () => ({ pane: session }),
      dispatch: vi.fn(),
      spawn,
    })).rejects.toMatchObject({ code: "AGENT_RESUME_INVALID" });
    expect(spawn).not.toHaveBeenCalled();
  });
});
