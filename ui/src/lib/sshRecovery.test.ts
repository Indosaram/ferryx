import { describe, expect, it, vi } from "vitest";
import { startSshRecovery } from "./sshRecovery";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import { workspaceReducer, type WorkspaceState } from "../state/workspaceStore";
import { createLayoutState } from "../state/layout";

describe("SSH recovery", () => {
  const session = { id: "pane", workspaceId: "ssh:host:project", cwd: "/srv", worktree: null,
    backendSessionId: "stable", lifecycle: "running" as const, daemonEpoch: "old",
    agentType: "claude", agentSessionId: "agent-owned", providerSession: { key: "session_id" as const, id: "agent-owned" } };
  it("retains stable identity on lifecycle loss and accepts remote status without replacing the pane", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const lost = workspaceReducer(state, { type: "SESSION_LIFECYCLE", backendSessionId: "stable", lifecycle: "exited" });
    expect(lost.sessions.pane.backendSessionId).toBe("stable");
    const recovered = workspaceReducer(lost, { type: "SESSION_REMOTE_STATUS", status: { sessionId: "stable", state: "connected", generation: 2, failure: null, replayGap: null }, daemonEpoch: "new" });
    expect(recovered.sessions.pane).toMatchObject({ ...session, daemonEpoch: "new", remoteGeneration: 2, remoteConnectionState: "connected" });
    expect(recovered.layout).toBe(state.layout);
  });
  it("subscribes before querying, ignores a stale query after an event, and reconciles epoch", async () => {
    let emit: (status: any) => void = () => {};
    let resolve!: (value: any) => void;
    const query = new Promise<any>(r => { resolve = r; });
    const dispatch = vi.fn();
    const dispose = vi.fn();
    const recovery = startSshRecovery({ sessions: [session], dispatch,
      subscribe: async handler => { emit = handler; return dispose; },
      status: vi.fn(() => query), list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
      onError: error => { throw error; },
    });
    await recovery.subscribed;
    emit({ sessionId: "stable", state: "connected", generation: 2, failure: null, replayGap: null });
    resolve({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: true });
    await recovery.ready;
    expect(dispatch.mock.calls.some(([action]) => action.status.state === "legacyLost")).toBe(false);
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ daemonEpoch: "new", status: { state: "connected" } });
    recovery.stop();
    expect(dispose).toHaveBeenCalledOnce();
  });
  it.each([null, [], [{ sessionId: "stable", daemonEpoch: "new", running: false }]])("restores SSH identity without treating list availability or epoch as process loss: %j", live => {
    const layout = createLayoutState();
    layout.tabs = [{ id: "tab", sessionId: "pane", label: "SSH" }];
    const state: WorkspaceState = { workspaceId: session.workspaceId, worktrees: [], activeWorktreePath: "/srv", layout, unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const saved = serializeWorkspaceState(session.workspaceId, "/srv", state);
    const restored = deserializeWorkspaceState(session.workspaceId, saved, live)!;
    expect(restored.sessions.pane).toMatchObject({ backendSessionId: "stable", agentSessionId: "agent-owned", providerSession: session.providerSession });
    expect(restored.sessions.pane.remoteConnectionState).toBe("reconnecting");
    expect(restored.layout.tabs[0].id).toBe("tab");
  });
  it.each(["missing", "expired", "legacyLost"] as const)("keeps genuine %s loss distinct from a transport outage", stateName => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS", status: { sessionId: "stable", state: stateName, generation: 3, failure: null, replayGap: null } });
    expect(next.sessions.pane).toMatchObject({ backendSessionId: "stable", lifecycle: "exited", remoteConnectionState: stateName, agentSessionId: "agent-owned" });
  });
  it("does not preserve agent references when an arbitrary different SSH backend is assigned", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "REBIND_SESSION_BACKEND", sessionId: "pane", backendSessionId: "different" });
    expect(next.sessions.pane).toMatchObject({ agentType: null, agentSessionId: null, providerSession: null });
  });
  it("reports temporary status failure without declaring the target missing", async () => {
    const error = new Error("daemon unavailable");
    const dispatch = vi.fn();
    const onError = vi.fn();
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError,
      subscribe: async () => () => {}, status: async () => { throw error; },
    });
    await recovery.ready;
    expect(dispatch).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(error);
    recovery.stop();
  });
  it("queries restored sessions automatically and reconciles the daemon epoch", async () => {
    const dispatch = vi.fn();
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {},
      status: async () => ({ type: "remoteSessionDetailsOk", legacyDirectSsh: false,
        details: { state: "connected", generation: 4, failure: null, replayGap: null, attempts: 0, pid: 10,
          descriptor: { backendSessionId: "stable", target: { hostId: "h", ownerId: "o", backendSessionId: "remote", epoch: "1" }, config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24 } } }),
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ type: "SESSION_REMOTE_STATUS", daemonEpoch: "new", status: { sessionId: "stable", state: "connected", generation: 4 } });
    recovery.stop();
  });
});
