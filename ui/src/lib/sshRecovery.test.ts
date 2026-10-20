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
  it.each(["expired", "legacyLost"] as const)("keeps genuine %s loss distinct from a transport outage", stateName => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS", status: { sessionId: "stable", state: stateName, generation: 3, failure: null, replayGap: null } });
    expect(next.sessions.pane).toMatchObject({ backendSessionId: "stable", lifecycle: "exited", remoteConnectionState: stateName, agentSessionId: "agent-owned" });
  });
  it("treats an unknown-to-this-daemon session as recoverable rather than dead", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS", status: { sessionId: "stable", state: "missing", generation: 3, failure: null, replayGap: null } });
    expect(next.sessions.pane).toMatchObject({ backendSessionId: "stable", lifecycle: session.lifecycle, remoteConnectionState: "missing", agentSessionId: "agent-owned" });
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
  it("retries once and dispatches the re-probed status when the daemon does not know the session", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const connected = { type: "remoteSessionDetailsOk" as const, legacyDirectSsh: false,
      details: { state: "connected" as const, generation: 7, failure: null, replayGap: null, attempts: 1, pid: 42,
        descriptor: { backendSessionId: "stable", target: { hostId: "h", ownerId: "o", backendSessionId: "remote", epoch: "1" }, config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24 } } };
    const status = vi.fn()
      .mockResolvedValueOnce({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false })
      .mockResolvedValueOnce(connected);
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {}, status: status as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).toHaveBeenCalledExactlyOnceWith("stable");
    expect(status).toHaveBeenCalledTimes(2);
    // Only the settled re-probe reaches the store; the transient `missing` is never dispatched.
    expect(dispatch.mock.calls.some(([action]) => action.status.state === "missing")).toBe(false);
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ type: "SESSION_REMOTE_STATUS", daemonEpoch: "new", status: { sessionId: "stable", state: "connected", generation: 7 } });
    recovery.stop();
  });
  it("swallows a structured retry failure and still applies the re-probed status", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => { throw { code: "IO_ERROR", message: "daemon busy", details: {} }; });
    const status = vi.fn()
      .mockResolvedValueOnce({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false })
      .mockResolvedValueOnce({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false });
    const onError = vi.fn();
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError,
      subscribe: async () => () => {}, status: status as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).toHaveBeenCalledOnce();
    expect(onError).not.toHaveBeenCalled();
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ type: "SESSION_REMOTE_STATUS", status: { sessionId: "stable", state: "missing" } });
    recovery.stop();
  });
  it("does not retry a legacy direct SSH session", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => undefined);
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {},
      status: async () => ({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: true }),
      retry, list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).not.toHaveBeenCalled();
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ status: { state: "legacyLost" } });
    recovery.stop();
  });

  // A details-bearing status for the parked-transport cases below: `attempts` at the daemon's
  // reconnect budget and a transport failure is exactly the shape a session parks in.
  const parkedDetails = (state: string, generation: number) => ({
    type: "remoteSessionDetailsOk" as const, legacyDirectSsh: false,
    details: { state, generation, failure: state === "disconnected" ? { kind: "transport", message: "SSH setup error" } : null,
      replayGap: null, attempts: state === "disconnected" ? 5 : 0, pid: 10,
      descriptor: { backendSessionId: "stable", target: { hostId: "h", ownerId: "o", backendSessionId: "remote", epoch: "1" },
        config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24 } },
  });
  it("re-arms a transport the daemon parked in disconnected and settles on the recovered status", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const status = vi.fn()
      .mockResolvedValueOnce(parkedDetails("disconnected", 257))
      .mockResolvedValueOnce(parkedDetails("connected", 258));
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {}, status: status as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    // The daemon's own loop already gave up on this session, so one re-arm is what revives it.
    expect(retry).toHaveBeenCalledExactlyOnceWith("stable");
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ type: "SESSION_REMOTE_STATUS", daemonEpoch: "new",
      status: { sessionId: "stable", state: "connected", generation: 258 } });
    recovery.stop();
  });
  function createMockSeams(overrides?: {
    status?: (id: string) => Promise<any>;
    onError?: (err: any) => void;
  }) {
    type Listener<T> = {
      predicate: (val: T) => boolean;
      resolve: (val: T) => void;
      reject: (err: any) => void;
      timer: ReturnType<typeof setTimeout>;
    };
    const dispatchListeners: Listener<any>[] = [];
    const errorListeners: Listener<any>[] = [];

    const notify = <T>(listeners: Listener<T>[], val: T) => {
      for (let i = listeners.length - 1; i >= 0; i--) {
        const l = listeners[i];
        try {
          if (l.predicate(val)) {
            clearTimeout(l.timer);
            listeners.splice(i, 1);
            l.resolve(val);
          }
        } catch (err) {
          clearTimeout(l.timer);
          listeners.splice(i, 1);
          l.reject(err);
        }
      }
    };

    const addWaiter = <T>(listeners: Listener<T>[], predicate: (val: T) => boolean, timeoutMs = 2000, desc = "event"): Promise<T> => {
      return new Promise<T>((resolve, reject) => {
        const timer = setTimeout(() => {
          const idx = listeners.findIndex(l => l.timer === timer);
          if (idx !== -1) listeners.splice(idx, 1);
          reject(new Error(`Timed out after ${timeoutMs}ms waiting for ${desc}`));
        }, timeoutMs);
        listeners.push({ predicate, resolve, reject, timer });
      });
    };

    const dispatch = vi.fn((action: any) => {
      notify(dispatchListeners, action);
    });

    const onError = vi.fn((err: any) => {
      notify(errorListeners, err);
      overrides?.onError?.(err);
    });

    const list = vi.fn(async () => {
      return [{ sessionId: "stable", daemonEpoch: "new" }];
    });

    const status = vi.fn(async (id: string) => {
      const res = overrides?.status ? await overrides.status(id) : parkedDetails("disconnected", 400);
      return res;
    });

    return {
      dispatch,
      onError,
      list,
      status,
      waitForSettled: (generation: number, timeoutMs?: number) =>
        addWaiter(dispatchListeners, a => a?.daemonEpoch === "new" && a?.status?.generation === generation, timeoutMs, `settled status gen ${generation}`),
      waitForError: (predicate: (err: any) => boolean = () => true, timeoutMs?: number) =>
        addWaiter(errorListeners, predicate, timeoutMs, "onError"),
    };
  }
  const outageStream = () => {
    let emit: (status: any) => void = () => {};
    const seams = createMockSeams({
      onError: error => { throw error; },
      status: async () => parkedDetails("disconnected", 400),
    });
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    // Every re-probe still reports a park: the outage outlives each re-armed budget.
    const recovery = startSshRecovery({ sessions: [session], dispatch: seams.dispatch, onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: seams.status as any, retry,
      list: seams.list,
    });
    return { recovery, emit, retry, dispatch: seams.dispatch, seams };
  };
  const park = { sessionId: "stable", state: "disconnected" as const, generation: 5, failure: null, replayGap: null };
  const reconnect = { sessionId: "stable", state: "reconnecting" as const, generation: 6, failure: null, replayGap: null };
  it("re-arms on each new park and ignores repeats of the same one", async () => {
    const { recovery, emit, retry, seams } = outageStream();
    await recovery.subscribed;
    const waitPark = seams.waitForSettled(400);
    emit(park);
    await waitPark;
    expect(retry).toHaveBeenCalledTimes(1);
    // A repeat of the same park is not a new outage.
    const waitRepeat = seams.waitForSettled(5);
    emit(park);
    await waitRepeat;
    expect(retry).toHaveBeenCalledTimes(1);
    const waitReconnect = seams.waitForSettled(6);
    emit(reconnect);
    await waitReconnect;
    const waitPark2 = seams.waitForSettled(400);
    emit({ ...park, generation: 7 });
    await waitPark2;
    expect(retry).toHaveBeenCalledTimes(2);
    recovery.stop();
  });
  it("stops re-arming after the bounded number of automatic attempts", async () => {
    const { recovery, emit, retry, seams } = outageStream();
    await recovery.subscribed;
    for (let i = 0; i < 8; i += 1) {
      const waitReconnect = seams.waitForSettled(10 + i * 2);
      emit({ ...reconnect, generation: 10 + i * 2 });
      await waitReconnect;
      const waitPark = seams.waitForSettled(i < 5 ? 400 : 11 + i * 2);
      emit({ ...park, generation: 11 + i * 2 });
      await waitPark;
    }
    // A long outage must not keep dialing the host forever; the manual button remains.
    expect(retry).toHaveBeenCalledTimes(5);
    recovery.stop();
  });
  it("resets the re-arm budget once the transport recovers", async () => {
    const { recovery, emit, retry, seams } = outageStream();
    await recovery.subscribed;
    for (let i = 0; i < 6; i += 1) {
      const waitReconnect = seams.waitForSettled(20 + i * 2);
      emit({ ...reconnect, generation: 20 + i * 2 });
      await waitReconnect;
      const waitPark = seams.waitForSettled(i < 5 ? 400 : 21 + i * 2);
      emit({ ...park, generation: 21 + i * 2 });
      await waitPark;
    }
    expect(retry).toHaveBeenCalledTimes(5);
    const waitConnected = seams.waitForSettled(60);
    emit({ sessionId: "stable", state: "connected", generation: 60, failure: null, replayGap: null });
    await waitConnected;
    const waitRecoveredPark = seams.waitForSettled(400);
    emit({ ...park, generation: 61 });
    await waitRecoveredPark;
    expect(retry).toHaveBeenCalledTimes(6);
    recovery.stop();
  });
  it("never re-arms a session whose remote process is gone", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {}, status: (async () => parkedDetails("expired", 9)) as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).not.toHaveBeenCalled();
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ status: { sessionId: "stable", state: "expired" } });
    recovery.stop();
  });

  it("retains auto-rearm count across recovery re-instantiations and respects cap", async () => {
    const sharedRearms = new Map<string, number>();
    let emit1: (status: any) => void = () => {};
    const seams1 = createMockSeams({
      onError: error => { throw error; },
      status: async () => parkedDetails("disconnected", 400),
    });
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const recovery1 = startSshRecovery({
      sessions: [session],
      dispatch: seams1.dispatch,
      onError: seams1.onError,
      subscribe: async handler => { emit1 = handler; return () => {}; },
      status: seams1.status as any,
      retry,
      list: seams1.list,
      rearms: sharedRearms,
    });
    await recovery1.subscribed;
    for (let i = 0; i < 3; i++) {
      const waitReconnect = seams1.waitForSettled(10 + i * 2);
      emit1({ ...reconnect, generation: 10 + i * 2 });
      await waitReconnect;
      const waitPark = seams1.waitForSettled(400);
      emit1({ ...park, generation: 11 + i * 2 });
      await waitPark;
    }
    expect(retry).toHaveBeenCalledTimes(3);
    recovery1.stop();

    // Re-instantiate recovery effect with the same shared rearms map (e.g. held by sshRearmsRef)
    let emit2: (status: any) => void = () => {};
    const seams2 = createMockSeams({
      onError: error => { throw error; },
      status: async () => parkedDetails("disconnected", 500),
    });
    const recovery2 = startSshRecovery({
      sessions: [session],
      dispatch: seams2.dispatch,
      onError: seams2.onError,
      subscribe: async handler => { emit2 = handler; return () => {}; },
      status: seams2.status as any,
      retry,
      list: seams2.list,
      rearms: sharedRearms,
    });
    await recovery2.subscribed;
    for (let i = 0; i < 4; i++) {
      const waitReconnect = seams2.waitForSettled(20 + i * 2);
      emit2({ ...reconnect, generation: 20 + i * 2 });
      await waitReconnect;
      const waitPark = seams2.waitForSettled(i < 2 ? 500 : 21 + i * 2);
      emit2({ ...park, generation: 21 + i * 2 });
      await waitPark;
    }
    // Must be capped at 5 total across re-instantiations, NOT 3 + 4 = 7
    expect(retry).toHaveBeenCalledTimes(5);
    recovery2.stop();
  });

  it("catches non-structured retry errors in rearmAndReprobe without unhandled rejection", async () => {
    let emit: (status: any) => void = () => {};
    const nonStructuredError = new Error("IPC transport disconnected unexpectedly");
    const retry = vi.fn(async () => { throw nonStructuredError; });
    const seams = createMockSeams({
      status: async () => parkedDetails("disconnected", 400),
    });
    const { dispatch, onError } = seams;
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch,
      onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: seams.status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;
    const waitError = seams.waitForError(err => err === nonStructuredError);
    emit({ sessionId: "stable", state: "disconnected", generation: 10, failure: null, replayGap: null });
    await waitError;
    expect(retry).toHaveBeenCalledOnce();
    expect(onError).toHaveBeenCalledWith(nonStructuredError);
    recovery.stop();
  });

  it("re-arms exactly once during startup when details are initially missing", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const disconnectedDetails = parkedDetails("disconnected", 1);
    const status = vi.fn()
      .mockResolvedValueOnce({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false })
      .mockResolvedValue(disconnectedDetails);
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch,
      onError: error => { throw error; },
      subscribe: async () => () => {},
      status: status as any,
      retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    // Missing details should trigger startup rearm once, and not double-rearm in rearmAndReprobe
    expect(retry).toHaveBeenCalledTimes(1);
    recovery.stop();
  });
});
