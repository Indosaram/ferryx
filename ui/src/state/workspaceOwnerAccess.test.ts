import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAgentReconnectInflightForTests, reconnectAgentSession } from "../lib/agentReconnect";
import type { SpawnTerminalResult } from "../lib/tauri";
import type { TerminalSession } from "../lib/types";
import { createOwnedWorkspaceAccess } from "./workspaceOwnerAccess";
import { clearWorkspaceSnapshot, getWorkspaceSnapshot, setWorkspaceSnapshot } from "./workspaceSnapshotCache";
import { workspaceReducer, type WorkspaceAction, type WorkspaceState } from "./workspaceStore";

const PANE = "session:lectures-pane";

function standbyPane(): TerminalSession {
  return {
    id: PANE, workspaceId: "lectures", worktree: null, cwd: "/repos/lectures",
    backendSessionId: `standby:${PANE}`, lifecycle: "exited", processState: "standby", agentType: "claude",
    providerSession: { key: "session_id", id: "provider-1" }, reconnectLifecycle: "idle",
  };
}

function lecturesState(): WorkspaceState {
  return { workspaceId: "lectures", sessions: { [PANE]: standbyPane() } } as unknown as WorkspaceState;
}

function otherState(): WorkspaceState {
  return { workspaceId: "ferryx", sessions: {} } as unknown as WorkspaceState;
}

function spawnResult(): SpawnTerminalResult {
  return {
    sessionId: "backend-new", daemonEpoch: "42",
    session: { sessionId: "backend-new", workspaceId: "lectures", worktree: null, cwd: "/repos/lectures", cols: 80, rows: 24, running: true },
  } as SpawnTerminalResult;
}

describe("agent reconnect across a project switch", () => {
  beforeEach(() => {
    clearAgentReconnectInflightForTests();
    clearWorkspaceSnapshot();
  });
  afterEach(() => clearWorkspaceSnapshot());

  it("settles the pane in its own parked workspace when the user switches away mid-spawn", async () => {
    let mountedState = lecturesState();
    const mountedDispatch = vi.fn((action: WorkspaceAction) => { mountedState = workspaceReducer(mountedState, action); });
    const owner = createOwnedWorkspaceAccess("lectures", {
      isMounted: () => mountedState.workspaceId === "lectures",
      getState: () => mountedState,
      dispatch: mountedDispatch,
    });

    let finishSpawn!: (result: SpawnTerminalResult) => void;
    const spawn = vi.fn(() => new Promise<SpawnTerminalResult>((resolve) => { finishSpawn = resolve; }));
    const attempt = reconnectAgentSession(PANE, {
      getSessions: () => owner.getState()?.sessions ?? {},
      dispatch: owner.dispatch,
      spawn,
      attach: vi.fn(async () => undefined),
    });
    await vi.waitFor(() => expect(spawn).toHaveBeenCalledTimes(1));
    expect(mountedState.sessions[PANE]?.reconnectLifecycle).toBe("spawning");

    setWorkspaceSnapshot("lectures", mountedState);
    mountedState = otherState();

    finishSpawn(spawnResult());
    await attempt;

    const parked = getWorkspaceSnapshot("lectures")?.sessions[PANE];
    expect(parked?.reconnectLifecycle).not.toBe("spawning");
    expect(parked?.backendSessionId).toBe("backend-new");
    expect(mountedState.sessions[PANE]).toBeUndefined();
  });

  it("routes to the mounted store while the owner is on screen", () => {
    const mountedDispatch = vi.fn();
    const owner = createOwnedWorkspaceAccess("lectures", {
      isMounted: () => true,
      getState: lecturesState,
      dispatch: mountedDispatch,
    });
    const action: WorkspaceAction = { type: "SET_RECONNECT_LIFECYCLE", sessionId: PANE, lifecycle: "validating" };
    owner.dispatch(action);
    expect(mountedDispatch).toHaveBeenCalledWith(action);
    expect(getWorkspaceSnapshot("lectures")).toBeNull();
  });
});
