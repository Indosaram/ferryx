import { describe, expect, it, vi } from "vitest";
import { coalesceListCalls, startSshRecovery } from "./sshRecovery";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./sessionPersistence";
import { workspaceReducer, type WorkspaceState } from "../state/workspaceStore";
import { createLayoutState } from "../state/layout";
import type { RemoteConnectionState, RemoteSessionStatusResponse, TerminalRemoteStatus } from "./types";
import type { RecoveryOutcome } from "./sessionRecovery";
import { REMOTE_SESSION_GONE_REASON } from "./sessionLifecycle";

describe("SSH recovery", () => {
  const session = { id: "pane", workspaceId: "ssh:host:project", cwd: "/srv", worktree: null,
    backendSessionId: "stable", lifecycle: "running" as const, daemonEpoch: "old",
    agentType: "claude", agentSessionId: "agent-owned", providerSession: { key: "session_id" as const, id: "agent-owned" } };
  const sshAuthority: Pick<TerminalRemoteStatus, "daemonEpoch" | "incarnation" | "target"> = {
    daemonEpoch: "new",
    incarnation: "stable-incarnation",
    target: { kind: "ssh", hostId: "host" },
  };

  it("retains stable identity on lifecycle loss and accepts remote status without replacing the pane", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const lost = workspaceReducer(state, { type: "SESSION_LIFECYCLE", backendSessionId: "stable", lifecycle: "exited" });
    expect(lost.sessions.pane.backendSessionId).toBe("stable");
    const recovered = workspaceReducer(lost, { type: "SESSION_REMOTE_STATUS", status: { ...sshAuthority, sessionId: "stable", state: "connected", generation: 2, failure: null, replayGap: null }, daemonEpoch: "new" });
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
    emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 2, failure: null, replayGap: null });
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

  it("keeps genuine expired loss distinct from a transport outage", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv",
      layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {},
      sessions: { pane: session } };
    const status = { ...sshAuthority, sessionId: "stable", state: "expired" as const, generation: 3,
      replayGap: null };
    const failure = { kind: "expired" as const, message: "Remote target exited" };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS",
      status: { ...status, failure } });
    // A confirmed target exit keeps the pane and its agent on a standby backend for a resume on
    // the same host; it never reuses the dead remote backend id.
    expect(next.sessions.pane).toMatchObject({
      backendSessionId: "standby:pane", lifecycle: "exited", processState: "standby",
      backendUnavailableReason: REMOTE_SESSION_GONE_REASON, remoteFailure: failure,
      agentSessionId: "agent-owned", providerSession: session.providerSession,
    });
    expect(next.layout).toBe(state.layout);

    // Retry exhaustion also reports `expired`, but its transport failure proves nothing about
    // the remote process, so the pane keeps its remote backend.
    const exhausted = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS",
      status: { ...status, failure: { kind: "transport", message: "unreachable" } } });
    expect(exhausted.sessions.pane).toMatchObject({ backendSessionId: "stable",
      lifecycle: session.lifecycle, remoteConnectionState: "expired",
      agentSessionId: "agent-owned" });
  });

  it.each([
    ["legacyLost", null],
    ["expired", null],
  ] as const)("does not read %s with failure %j as process loss", (stateName, failure) => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv",
      layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {},
      sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS", status: {
      ...sshAuthority, sessionId: "stable", state: stateName, generation: 3, failure,
      replayGap: null,
    } });
    // Only an `expired` target failure proves the remote process exited. A legacy direct SSH
    // session merely cannot be reattached; it keeps its backend so the pane shows that state.
    expect(next.sessions.pane).toMatchObject({ backendSessionId: "stable",
      lifecycle: session.lifecycle, remoteConnectionState: stateName,
      agentSessionId: "agent-owned" });
  });

  it("treats an unknown-to-this-daemon session as recoverable rather than dead", () => {
    const state: WorkspaceState = { worktrees: [], activeWorktreePath: "/srv", layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {}, sessions: { pane: session } };
    const next = workspaceReducer(state, { type: "SESSION_REMOTE_STATUS", status: { ...sshAuthority, sessionId: "stable", state: "missing", generation: 3, failure: null, replayGap: null } });
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
        details: { ...sshAuthority, state: "connected", generation: 4, failure: null, replayGap: null, attempts: 0, pid: 10,
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
      details: { ...sshAuthority, state: "connected" as const, generation: 7, failure: null, replayGap: null, attempts: 1, pid: 42,
        descriptor: { backendSessionId: "stable", target: { hostId: "h", ownerId: "o", backendSessionId: "remote", epoch: "1" }, config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24 } } };
    const status = vi.fn()
      .mockResolvedValueOnce({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false })
      .mockResolvedValueOnce(connected);
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError: error => { throw error; },
      subscribe: async () => () => {}, status: status as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).toHaveBeenCalledWith("stable");
    expect(dispatch.mock.calls.some(([action]) => action.status.state === "missing")).toBe(false);
    expect(dispatch.mock.calls.at(-1)?.[0]).toMatchObject({ type: "SESSION_REMOTE_STATUS", daemonEpoch: "new", status: { sessionId: "stable", state: "connected", generation: 7 } });
    recovery.stop();
  });

  it("swallows a structured retry failure and still applies the re-probed status", async () => {
    const dispatch = vi.fn();
    const retry = vi.fn(async () => { throw { code: "IO_ERROR", message: "daemon busy", details: {} }; });
    const status = vi.fn()
      .mockResolvedValue({ type: "remoteSessionDetailsOk", details: null, legacyDirectSsh: false });
    const onError = vi.fn();
    const recovery = startSshRecovery({ sessions: [session], dispatch, onError,
      subscribe: async () => () => {}, status: status as any, retry,
      list: async () => [{ sessionId: "stable", daemonEpoch: "new" }],
    });
    await recovery.ready;
    expect(retry).toHaveBeenCalled();
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

  const parkedDetails = (state: RemoteConnectionState, generation: number, failure: any = null): RemoteSessionStatusResponse => ({
    type: "remoteSessionDetailsOk" as const, legacyDirectSsh: false,
    details: { ...sshAuthority, state, generation, failure: failure ?? (state === "disconnected" ? { kind: "transport", message: "SSH setup error" } : null),
      replayGap: null, attempts: state === "disconnected" ? 5 : 0, pid: 10,
      descriptor: { backendSessionId: "stable", target: { hostId: "h", ownerId: "o", backendSessionId: "remote", epoch: "1" },
        config: {}, clientRequestId: "request", remoteCursor: "0", cols: 80, rows: 24 } },
  } satisfies RemoteSessionStatusResponse);

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
    expect(retry).toHaveBeenCalledWith("stable");
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

  const park: TerminalRemoteStatus = {
    ...sshAuthority,
    sessionId: "stable",
    state: "disconnected",
    generation: 5,
    failure: null,
    replayGap: null,
  };

  it("re-arms on each new park and ignores repeats of the same one", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    let currentProbeGen = 5;
    const seams = createMockSeams({
      onError: error => { throw error; },
      status: async () => parkedDetails("disconnected", currentProbeGen),
    });
    const retryListeners: Array<() => void> = [];
    const retry = vi.fn(async () => {
      const res = { type: "retryRemoteSessionOk" as const };
      for (const l of [...retryListeners]) l();
      return res;
    });
    const waitForRetryCount = (target: number, timeoutMs = 2000) => {
      if (retry.mock.calls.length >= target) return Promise.resolve();
      return new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`Timed out waiting for retry count ${target}`)), timeoutMs);
        const check = () => {
          if (retry.mock.calls.length >= target) {
            clearTimeout(timer);
            const idx = retryListeners.indexOf(check);
            if (idx !== -1) retryListeners.splice(idx, 1);
            resolve();
          }
        };
        retryListeners.push(check);
      });
    };

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status: seams.status,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;

    // 1. First park at generation 5 triggers retry 1:
    currentProbeGen = 5;
    const waitRetry1 = waitForRetryCount(1);
    const waitPark = seams.waitForSettled(5);
    await emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 5, failure: null, replayGap: null });
    await waitPark;
    await waitRetry1;
    expect(retry).toHaveBeenCalledTimes(1);

    // 2. Repeat of same park at generation 5 is ignored (zero extra retry):
    const waitRepeat = seams.waitForSettled(5);
    await emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 5, failure: null, replayGap: null });
    await waitRepeat;
    expect(retry).toHaveBeenCalledTimes(1);

    // 3. Reconnect at generation 6:
    currentProbeGen = 6;
    const waitReconnect = seams.waitForSettled(6);
    await emit({ ...sshAuthority, sessionId: "stable", state: "reconnecting", generation: 6, failure: null, replayGap: null });
    await waitReconnect;

    // 4. New park at generation 7 triggers retry 2:
    currentProbeGen = 7;
    const waitRetry2 = waitForRetryCount(2);
    const waitPark2 = seams.waitForSettled(7);
    await emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 7, failure: null, replayGap: null });
    await waitPark2;
    await waitRetry2;
    expect(retry).toHaveBeenCalledTimes(2);

    recovery.stop();
  });

  it("supports indefinite recovery attempts over 20 failures with bounded backoff", async () => {
    let emit: (status: TerminalRemoteStatus) => void = () => {};
    const identityKey = JSON.stringify(["ssh", "pane", "stable", null]);
    type SettledAttempt = Parameters<NonNullable<Parameters<typeof startSshRecovery>[0]["onAttemptSettled"]>>[0];
    type Outcome = RecoveryOutcome & { readonly backendSessionId?: string };
    const waiterTimers = new Set<ReturnType<typeof setTimeout>>();
    const observations = <T,>() => {
      const listeners = new Set<(value: T) => void>();
      return {
        publish: (value: T) => {
          for (const listener of [...listeners]) listener(value);
        },
        next: (predicate: (value: T) => boolean) => new Promise<T>((resolve, reject) => {
          const timer = setTimeout(() => {
            listeners.delete(listener);
            waiterTimers.delete(timer);
            reject(new Error("Timed out waiting for SSH recovery observation"));
          }, 2000);
          const listener = (value: T) => {
            if (!predicate(value)) return;
            clearTimeout(timer);
            waiterTimers.delete(timer);
            listeners.delete(listener);
            resolve(value);
          };
          waiterTimers.add(timer);
          listeners.add(listener);
        }),
      };
    };
    const outcomes = observations<Outcome>();
    const settlements = observations<SettledAttempt>();
    let virtualTime = 1000;
    type TimerRecord = { id: number; cb: () => void; due: number; delay: number };
    const timers = new Map<number, TimerRecord>();
    const scheduled = observations<TimerRecord>();
    let timerIdSeq = 0;
    const virtualClock = {
      now: () => virtualTime,
      setTimeout: (cb: () => void, ms: number) => {
        const id = ++timerIdSeq;
        const timer = { id, cb, due: virtualTime + ms, delay: ms };
        timers.set(id, timer);
        scheduled.publish(timer);
        return id;
      },
      clearTimeout: (id: unknown) => {
        if (typeof id === "number") timers.delete(id);
      },
    };
    // Given: every completed retry fails; the eventual retry remains held across ready.
    let releaseRetry: (value: unknown) => void = () => {};
    const heldRetry = new Promise<unknown>(resolve => { releaseRetry = resolve; });
    const retryEntries = observations<number>();
    let retryCount = 0;
    const retry = vi.fn(async (_id: string) => {
      retryEntries.publish(++retryCount);
      if (retryCount === 23) return heldRetry;
      throw { code: "IO_ERROR", message: "SSH transport unavailable", details: { kind: "transport" } };
    });
    const seams = createMockSeams({
      onError: error => { throw error; },
      status: async () => parkedDetails("disconnected", 100),
    });
    const nextFailure = (attempt: number) => ({
      settled: settlements.next(value => value.backendSessionId === "stable" &&
        value.identityKey === identityKey && value.attemptNumber === attempt),
      outcome: outcomes.next(value => value.backendSessionId === "stable" &&
        value.identityKey === identityKey && value.phase === "retryable" && value.attempt === attempt),
      backoff: scheduled.next(value => value.delay === Math.min(250 * 2 ** (attempt - 1), 4000)),
    });
    let failure = nextFailure(1);
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler;
        return () => {};
      },
      status: seams.status,
      retry,
      list: seams.list,
      clock: virtualClock,
      initialDelayMs: 250,
      maxDelayMs: 4000,
      backoffFactor: 2,
      onOutcome: outcomes.publish,
      onAttemptSettled: settlements.publish,
    });
    try {
      await recovery.subscribed;
      let backoff: TimerRecord | undefined;
      for (let attempt = 1; attempt <= 22; attempt++) {
        const settled = await failure.settled;
        const outcome = await failure.outcome;
        expect(outcome).toMatchObject({ phase: "retryable", reason: "transport", token: settled.token });
        backoff = await failure.backoff;
        expect(retry).toHaveBeenCalledTimes(attempt);
        expect(seams.status).toHaveBeenCalledTimes(attempt * 2);
        expect(timers.size).toBe(1);
        if (attempt < 22) {
          failure = nextFailure(attempt + 1);
          timers.delete(backoff.id);
          virtualTime = backoff.due;
          backoff.cb();
        }
      }
      expect(backoff).toBeDefined();
      if (!backoff) throw new Error("Missing recovery backoff after 22 failed attempts");

      // When: hold the next flight, then deliver authoritative connected for the same owner.
      const entered = retryEntries.next(count => count === 23);
      const settled = settlements.next(value => value.backendSessionId === "stable" &&
        value.identityKey === identityKey && value.attemptNumber === 23);
      timers.delete(backoff.id);
      virtualTime = backoff.due;
      backoff.cb();
      await entered;
      expect(retry).toHaveBeenCalledTimes(23);
      expect(seams.status).toHaveBeenCalledTimes(45);
      const ready = outcomes.next(value => value.backendSessionId === "stable" &&
        value.identityKey === identityKey && value.phase === "ready");
      const connected = seams.waitForSettled(101);
      emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 101, failure: null, replayGap: null });
      const readyOutcome = await ready;
      await connected;
      expect(seams.dispatch.mock.calls.at(-1)?.[0]).toMatchObject({
        daemonEpoch: "new", status: { sessionId: "stable", state: "connected", generation: 101 },
      });

      // Then: late completion cannot create another flight, probe, or backoff after ready.
      releaseRetry({ type: "retryRemoteSessionOk" });
      const lateAttempt = await settled;
      expect(lateAttempt.token).toBe(23);
      expect(readyOutcome.token).toBe(24);
      expect(lateAttempt.token).toBeLessThan(readyOutcome.token);
      expect(retry).toHaveBeenCalledTimes(23);
      expect(retry.mock.calls.every(([id]) => id === "stable")).toBe(true);
      expect(seams.status).toHaveBeenCalledTimes(45);
      expect(timers.size).toBe(0);
      expect(seams.onError).not.toHaveBeenCalled();
    } finally {
      recovery.stop();
      releaseRetry({ type: "retryRemoteSessionOk" });
      for (const timer of waiterTimers) clearTimeout(timer);
    }
  });

  it("coalesces burst disconnected events into a single recovery flight", async () => {
    let emit: (status: any) => void = () => {};
    let resolveProbe!: (value: any) => void;
    const probePromise = new Promise(resolve => { resolveProbe = resolve; });
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const status = vi.fn().mockReturnValue(probePromise);
    const seams = createMockSeams({
      status: status as any,
      onError: error => { throw error; },
    });
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: seams.status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;
    emit({ ...park, generation: 10 });
    emit({ ...park, generation: 11 });
    emit({ ...park, generation: 12 });
    emit({ ...park, generation: 13 });
    resolveProbe(parkedDetails("disconnected", 400));
    const waitSettled = seams.waitForSettled(400);
    await waitSettled;
    expect(retry).toHaveBeenCalledTimes(1);
    recovery.stop();
  });

  it("releases single-flight owner when attempt deadline is exceeded", async () => {
    let emit!: (status: TerminalRemoteStatus) => void;
    let resolveRetry!: (value: any) => void;
    const retryPromise = new Promise(resolve => { resolveRetry = resolve; });
    let retryCalledResolve!: () => void;
    let retryTimeout: ReturnType<typeof setTimeout> | undefined;
    const retryCalledPromise = new Promise<void>((resolve, reject) => {
      retryTimeout = setTimeout(() => reject(new Error("Timed out waiting for retry entry")), 2000);
      retryCalledResolve = () => {
        clearTimeout(retryTimeout);
        resolve();
      };
    });
    retryCalledPromise.catch(() => {});
    const retry = vi.fn(() => {
      retryCalledResolve();
      return retryPromise;
    });

    let probeCount = 0;
    let probe2Resolve!: () => void;
    let probe2Timeout: ReturnType<typeof setTimeout> | undefined;
    const probe2CalledPromise = new Promise<void>((resolve, reject) => {
      probe2Timeout = setTimeout(() => reject(new Error("Timed out waiting for probe 2 entry")), 2000);
      probe2Resolve = () => {
        clearTimeout(probe2Timeout);
        resolve();
      };
    });
    probe2CalledPromise.catch(() => {});

    let resolveProbe2!: (value: any) => void;
    const probe2Promise = new Promise(resolve => { resolveProbe2 = resolve; });

    const status = vi.fn(async () => {
      probeCount++;
      if (probeCount === 1) {
        return parkedDetails("disconnected", 400);
      }
      probe2Resolve();
      return probe2Promise;
    });

    let timerIdSeq = 0;
    type TimerRecord = { id: number; cb: () => void; due: number; delayMs: number };
    const timerMap = new Map<number, TimerRecord>();
    const timerWaiters: Array<(rec: TimerRecord) => void> = [];

    const mockClock = {
      now: () => 1000,
      setTimeout: (cb: () => void, delayMs: number) => {
        const id = ++timerIdSeq;
        const rec: TimerRecord = { id, cb, due: 1000 + delayMs, delayMs };
        timerMap.set(id, rec);
        for (const waiter of [...timerWaiters]) {
          waiter(rec);
        }
        return id;
      },
      clearTimeout: (id: number) => {
        timerMap.delete(id);
      },
      waitForTimer: (predicate: (t: TimerRecord) => boolean, timeoutMs = 2000, desc = "timer") => {
        for (const t of timerMap.values()) {
          if (predicate(t)) return Promise.resolve(t);
        }
        return new Promise<TimerRecord>((resolve, reject) => {
          const timeoutHandle = setTimeout(() => {
            const idx = timerWaiters.indexOf(waiter);
            if (idx !== -1) timerWaiters.splice(idx, 1);
            reject(new Error(`Timed out after ${timeoutMs}ms waiting for ${desc}`));
          }, timeoutMs);
          const waiter = (t: TimerRecord) => {
            if (predicate(t)) {
              clearTimeout(timeoutHandle);
              const idx = timerWaiters.indexOf(waiter);
              if (idx !== -1) timerWaiters.splice(idx, 1);
              resolve(t);
            }
          };
          timerWaiters.push(waiter);
        });
      },
    };

    const seams = createMockSeams();

    type SettledAttempt = Parameters<NonNullable<Parameters<typeof startSshRecovery>[0]["onAttemptSettled"]>>[0];
    let resolveSettled!: (attempt: SettledAttempt) => void;
    let settledTimeout: ReturnType<typeof setTimeout> | undefined;
    const oldAttemptSettled = new Promise<SettledAttempt>((resolve, reject) => {
      settledTimeout = setTimeout(() => reject(new Error("Timed out waiting for attempt 1 settlement")), 2000);
      resolveSettled = attempt => {
        clearTimeout(settledTimeout);
        resolve(attempt);
      };
    });
    oldAttemptSettled.catch(() => {});

    let resolveSettled2!: (attempt: SettledAttempt) => void;
    let settled2Timeout: ReturnType<typeof setTimeout> | undefined;
    const attempt2Settled = new Promise<SettledAttempt>((resolve, reject) => {
      settled2Timeout = setTimeout(() => reject(new Error("Timed out waiting for attempt 2 settlement")), 2000);
      resolveSettled2 = attempt => {
        clearTimeout(settled2Timeout);
        resolve(attempt);
      };
    });
    attempt2Settled.catch(() => {});

    const publishedOutcomes: any[] = [];
    let resolveTimeoutOutcome!: (outcome: any) => void;
    let outcomeTimeout: ReturnType<typeof setTimeout> | undefined;
    const timeoutOutcomePromise = new Promise<any>((resolve, reject) => {
      outcomeTimeout = setTimeout(() => reject(new Error("Timed out waiting for timeout outcome")), 2000);
      resolveTimeoutOutcome = outcome => {
        clearTimeout(outcomeTimeout);
        resolve(outcome);
      };
    });
    timeoutOutcomePromise.catch(() => {});

    let resolveAttempt2Outcome!: (outcome: RecoveryOutcome) => void;
    let attempt2OutcomeTimeout: ReturnType<typeof setTimeout> | undefined;
    const attempt2OutcomePromise = new Promise<RecoveryOutcome>((resolve, reject) => {
      attempt2OutcomeTimeout = setTimeout(() => reject(new Error("Timed out waiting for attempt 2 outcome")), 2000);
      resolveAttempt2Outcome = outcome => {
        clearTimeout(attempt2OutcomeTimeout);
        resolve(outcome);
      };
    });
    attempt2OutcomePromise.catch(() => {});

    const expectedIdentityKey = JSON.stringify(["ssh", "pane", "stable", null]);

    // Pre-arm readyPromise
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: status as any,
      retry,
      list: seams.list,
      clock: mockClock as any,
      attemptDeadlineMs: 100,
      onOutcome: outcome => {
        publishedOutcomes.push(outcome);
        if (outcome.phase === "retryable" && outcome.reason === "timeout") {
          resolveTimeoutOutcome(outcome);
        } else if (outcome.phase === "retryable" && outcome.attempt === 2) {
          resolveAttempt2Outcome(outcome);
        }
      },
      onAttemptSettled: attempt => {
        if (attempt.attemptNumber === 1) {
          resolveSettled(attempt);
        } else if (attempt.attemptNumber === 2) {
          resolveSettled2(attempt);
        }
      },
    });
    try {
      await recovery.subscribed;
      expect(emit).toBeDefined();

      // 1. Await entry into retry for attempt 1
      await retryCalledPromise;
      expect(retry).toHaveBeenCalledTimes(1);

      // 2. Owning attempt deadline timer (100ms) selected after retry entry
      const deadlineTimer = await mockClock.waitForTimer(t => t.delayMs === 100, 2000, "deadline timer");
      timerMap.delete(deadlineTimer.id);
      deadlineTimer.cb();

      // 3. Observable timeout publication published before old retry resolved
      const timeoutOutcome = await timeoutOutcomePromise;
      expect(timeoutOutcome).toMatchObject({
        phase: "retryable",
        reason: "timeout",
        attempt: 1,
      });

      // 4. Backoff timer for attempt 2 (delayMs: 250) scheduled in clock queue (single-flight owner slot freed)
      const nextAttemptTimer = await mockClock.waitForTimer(t => t.delayMs === 250, 2000, "attempt 2 backoff timer");
      expect(nextAttemptTimer).toBeDefined();

      // 5. Fire attempt 2 backoff timer while old attempt 1 retry is STILL held
      timerMap.delete(nextAttemptTimer.id);
      nextAttemptTimer.cb();

      // 6. PROVE SECOND PROBE ENTRY while old retry is held:
      await probe2CalledPromise;
      expect(status).toHaveBeenCalledTimes(2);

      // 7. EXERCISE SAME-GENERATION GUARD while old retry is STILL HELD:
      // Probe 2 returns disconnected 400; because old retry is still pending, generation 400
      // is recorded in pendingMutationGenerations. The scheduler suppresses redundant retry
      // and returns retryable boundedly without awaiting the uncertain mutation.
      resolveProbe2(parkedDetails("disconnected", 400));

      // Await attempt 2 settlement and verify retry count stays strictly at 1
      const settled2 = await attempt2Settled;
      expect(settled2.attemptNumber).toBe(2);
      await attempt2OutcomePromise;
      clearTimeout(attempt2OutcomeTimeout);
      expect(retry).toHaveBeenCalledTimes(1);

      // Snapshot outcomes and dispatch before old retry resolves
      const outcomeCountBeforeOldResolve = publishedOutcomes.length;
      const outcomesSnapshot = [...publishedOutcomes];
      const dispatchCallsBeforeOldResolve = seams.dispatch.mock.calls.length;

      // 8. STALE COMPLETION SUPPRESSION: Now resolve attempt 1 retry
      resolveRetry({ type: "retryRemoteSessionOk" });

      // Old attempt settles via onAttemptSettled with exact token and identity match
      const settled1 = await oldAttemptSettled;
      expect(settled1.token).toBe(1);
      expect(settled1.identityKey).toBe(expectedIdentityKey);
      expect(settled1.attemptNumber).toBe(1);

      // Assert dispatch and published outcomes remain strictly invariant across late attempt 1 resolution
      expect(seams.dispatch.mock.calls.length).toBe(dispatchCallsBeforeOldResolve);
      expect(publishedOutcomes.length).toBe(outcomeCountBeforeOldResolve);
      expect(publishedOutcomes).toEqual(outcomesSnapshot);

      await recovery.ready;
    } finally {
      clearTimeout(retryTimeout);
      clearTimeout(probe2Timeout);
      clearTimeout(settledTimeout);
      clearTimeout(settled2Timeout);
      clearTimeout(outcomeTimeout);
      clearTimeout(attempt2OutcomeTimeout);
      try { resolveRetry({ type: "retryRemoteSessionOk" }); } catch {}
      try { resolveProbe2(parkedDetails("connected", 400)); } catch {}
      timerWaiters.length = 0;
      recovery.stop();
    }
  });

  it("does not allow cancelled old recovery flight to apply state after stop", async () => {
    // Given: the first session attempt is held inside its retry producer.
    let resolveRetry: (value: unknown) => void = () => {};
    const retryPromise = new Promise<unknown>(resolve => { resolveRetry = resolve; });
    let retryCalledResolve!: () => void;
    const retryCalledPromise = new Promise<void>(r => { retryCalledResolve = r; });
    const retry = vi.fn(() => {
      retryCalledResolve();
      return retryPromise;
    });

    const status = vi.fn(async () => parkedDetails("disconnected", 400));
    const seams = createMockSeams({
      status,
    });

    // Pre-arm only the exact old attempt; producer resolution is not consumer settlement.
    const oldIdentityKey = JSON.stringify(["ssh", "pane", "stable", null]);
    type SettledAttempt = Parameters<NonNullable<Parameters<typeof startSshRecovery>[0]["onAttemptSettled"]>>[0];
    let observeSettled: (attempt: SettledAttempt) => void = () => {};
    let settledTimeout: ReturnType<typeof setTimeout> | undefined;
    const oldAttemptSettled = new Promise<SettledAttempt>((resolve, reject) => {
      settledTimeout = setTimeout(() => reject(new Error("Timed out waiting for cancelled SSH attempt settlement")), 2000);
      observeSettled = attempt => {
        if (attempt.backendSessionId === "stable" && attempt.token === 1 &&
            attempt.identityKey === oldIdentityKey && attempt.attemptNumber === 1) {
          clearTimeout(settledTimeout);
          resolve(attempt);
        }
      };
    });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async () => () => {},
      status: seams.status,
      retry,
      list: seams.list,
      onAttemptSettled: observeSettled,
    });
    try {
      await recovery.subscribed;
      await retryCalledPromise;
      expect(retry).toHaveBeenCalledTimes(1);

      // When: stop the owner before releasing its pending producer.
      recovery.stop();
      const callsBeforeResolve = seams.dispatch.mock.calls.length;
      resolveRetry({ type: "retryRemoteSessionOk" });
      await oldAttemptSettled;

      // Then: the real old continuation finished without a second probe or stale dispatch.
      expect(status).toHaveBeenCalledTimes(1);
      expect(seams.dispatch.mock.calls.length).toBe(callsBeforeResolve);
      expect(seams.dispatch).not.toHaveBeenCalled();
    } finally {
      clearTimeout(settledTimeout);
      recovery.stop();
    }
  });

  it("treats expired target as terminal and does not retry", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    let currentProbedState: RemoteConnectionState = "connected";
    let currentProbedGen = 89;
    const seams = createMockSeams({
      status: async () => parkedDetails(currentProbedState, currentProbedGen),
    });
    let exitOutcome: any;
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status: seams.status as any,
      retry,
      list: seams.list,
      onOutcome: outcome => {
        if (outcome.phase === "exited") {
          exitOutcome = outcome;
        }
      },
    });
    await recovery.subscribed;
    await recovery.ready;

    // Genuine expired event arrives at generation 90:
    currentProbedState = "expired";
    currentProbedGen = 90;
    const waitExpired = seams.waitForSettled(90);
    await emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 90, failure: null, replayGap: null });
    await waitExpired;
    expect(exitOutcome).toMatchObject({ phase: "exited" });
    expect(retry).not.toHaveBeenCalled();

    await emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 91, failure: null, replayGap: null });
    expect(retry).not.toHaveBeenCalled();
    recovery.stop();
  });

  it("rejects stale expired event after newer connected generation and retains genuine exit", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    let currentProbedState: RemoteConnectionState = "connected";
    let currentProbedGen = 100;
    const seams = createMockSeams({
      status: async () => parkedDetails(currentProbedState, currentProbedGen),
    });

    let exitOutcome: any;
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status: seams.status as any,
      retry,
      list: seams.list,
      onOutcome: outcome => {
        if (outcome.phase === "exited") {
          exitOutcome = outcome;
        }
      },
    });
    await recovery.subscribed;

    // 1. Establish initial connected state at generation 100
    const waitConnected = seams.waitForSettled(100);
    await emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 100, failure: null, replayGap: null });
    await waitConnected;

    // 2. Stale stream expired event arrives with older generation 99:
    // Stale generation must be rejected BEFORE publication (no exit, no fence)
    await emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 99, failure: null, replayGap: null });
    expect(exitOutcome).toBeUndefined();

    // 3. Stale stream connected event arrives with older generation 98:
    // Stale generation must be rejected BEFORE publication
    await emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 98, failure: null, replayGap: null });
    expect(exitOutcome).toBeUndefined();

    // 4. Ambiguous expired event arrives at generation 101, but probe reveals target is NOT exited:
    currentProbedState = "connected";
    currentProbedGen = 101;
    await emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 101, failure: null, replayGap: null });
    // Describe owner authority is checked; target is connected so terminal fence is NOT applied
    expect(exitOutcome).toBeUndefined();

    // 5. Genuine expired event arrives at generation 102; probe confirms affirmative target exit:
    currentProbedState = "expired";
    currentProbedGen = 102;
    const waitExpired = seams.waitForSettled(102);
    await emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 102, failure: null, replayGap: null });
    await waitExpired;
    expect(exitOutcome).toMatchObject({ phase: "exited" });

    recovery.stop();
  });

  it("revalidates held ambiguous expired probe as connected, then allows subsequent genuine exit to fence and report exit", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    let resolveProbe101!: (res: RemoteSessionStatusResponse) => void;
    let probe101Timeout: ReturnType<typeof setTimeout> | undefined;
    const probe101Promise = new Promise<RemoteSessionStatusResponse>((resolve, reject) => {
      probe101Timeout = setTimeout(() => reject(new Error("Timed out waiting for probe 101")), 2000);
      resolveProbe101 = res => {
        clearTimeout(probe101Timeout);
        resolve(res);
      };
    });
    probe101Promise.catch(() => {});

    let currentProbeGen = 100;
    const seams = createMockSeams({
      status: async () => {
        if (currentProbeGen === 101) {
          return probe101Promise;
        }
        return parkedDetails(currentProbeGen === 102 ? "expired" : "connected", currentProbeGen);
      },
    });

    let exitOutcome: any;
    let readyOutcome: any;
    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status: seams.status,
      retry,
      list: seams.list,
      onOutcome: outcome => {
        if (outcome.phase === "exited") {
          exitOutcome = outcome;
        } else if (outcome.phase === "ready") {
          readyOutcome = outcome;
        }
      },
    });
    await recovery.subscribed;

    // 1. Initial connected state at generation 100:
    const waitConnected = seams.waitForSettled(100);
    await emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 100, failure: null, replayGap: null });
    await waitConnected;

    // 2. Ambiguous expired event arrives at generation 101; probe is held:
    currentProbeGen = 101;
    const emit101Promise = emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 101, failure: null, replayGap: null });

    // 3. Probe 101 resolves revealing target is connected (revalidated, not exited):
    resolveProbe101(parkedDetails("connected", 101));
    await emit101Promise;
    expect(exitOutcome).toBeUndefined();
    expect(readyOutcome).toMatchObject({ phase: "ready" });

    // 4. Real genuine expired arrives at generation 102; probe confirms affirmative exit:
    currentProbeGen = 102;
    const waitExpired = seams.waitForSettled(102);
    await emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 102, failure: null, replayGap: null });
    await waitExpired;

    expect(exitOutcome).toMatchObject({ phase: "exited" });
    expect(retry).not.toHaveBeenCalled();

    recovery.stop();
  });

  it("drops stale stream disconnected event when probe has already observed higher generation", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const seams = createMockSeams({
      status: async () => parkedDetails("connected", 400),
    });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status: seams.status,
      retry,
      list: seams.list,
    });

    await recovery.subscribed;
    await recovery.ready;

    // Probe has authoritatively observed connected at generation 400
    const dispatchCallsBeforeStaleStream = seams.dispatch.mock.calls.length;
    expect(retry).not.toHaveBeenCalled();

    // Stale stream disconnected event arrives carrying older generation 5:
    await emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 5, failure: null, replayGap: null });

    // Assert: zero retry, zero extra dispatch, zero regression from connected gen 400
    expect(retry).not.toHaveBeenCalled();
    expect(seams.dispatch.mock.calls.length).toBe(dispatchCallsBeforeStaleStream);

    recovery.stop();
  });

  it("discards held expired probe when intervening connected event arrives before probe resolves", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    let startupResolved = false;
    let resolveExpiredProbe!: (res: RemoteSessionStatusResponse) => void;
    let expiredProbeTimeout: ReturnType<typeof setTimeout> | undefined;
    const expiredProbePromise = new Promise<RemoteSessionStatusResponse>((resolve, reject) => {
      expiredProbeTimeout = setTimeout(() => reject(new Error("Timed out waiting for probe to resolve")), 2000);
      resolveExpiredProbe = res => {
        clearTimeout(expiredProbeTimeout);
        resolve(res);
      };
    });
    expiredProbePromise.catch(() => {});

    let probeCalls = 0;
    const status = vi.fn(async (_id: string): Promise<RemoteSessionStatusResponse> => {
      probeCalls++;
      if (!startupResolved) {
        return parkedDetails("connected", 90);
      }
      return expiredProbePromise;
    });

    const seams = createMockSeams({ status });
    let exitOutcome: RecoveryOutcome | undefined;
    let readyOutcome: RecoveryOutcome | undefined;

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status,
      retry: seams.status,
      list: seams.list,
      onOutcome: outcome => {
        if (outcome.phase === "exited") exitOutcome = outcome;
        if (outcome.phase === "ready") readyOutcome = outcome;
      },
    });
    await recovery.subscribed;
    await recovery.ready;
    expect(status).toHaveBeenCalledTimes(1);
    startupResolved = true;

    // 1. Initial stream expired event arrives at generation 100; retain in-flight completion:
    const expiredCompletion = emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 100, failure: null, replayGap: null });
    expect(status).toHaveBeenCalledTimes(2);

    // 2. While expired probe is STILL HELD, a newer connected event arrives at generation 100; await its completion:
    await emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 100, failure: null, replayGap: null });
    expect(readyOutcome).toMatchObject({ phase: "ready" });
    expect(exitOutcome).toBeUndefined();

    // 3. Now the held expired probe finally resolves with expired for gen 100; await its completion:
    resolveExpiredProbe(parkedDetails("expired", 100));
    await expiredCompletion;

    // 4. Assert: the late expired probe was DISCARDED because intervening connected arrived;
    // session must NOT be fenced or transitioned to exited!
    expect(exitOutcome).toBeUndefined();
    expect(readyOutcome).toMatchObject({ phase: "ready" });
    clearTimeout(expiredProbeTimeout);
    recovery.stop();
  });

  it("invalidates held expired probe when session is closed or rebound before probe resolves", async () => {
    let emit!: (status: TerminalRemoteStatus) => Promise<void>;
    let startupResolved = false;
    let resolveExpiredProbe!: (res: RemoteSessionStatusResponse) => void;
    let expiredProbeTimeout: ReturnType<typeof setTimeout> | undefined;
    const expiredProbePromise = new Promise<RemoteSessionStatusResponse>((resolve, reject) => {
      expiredProbeTimeout = setTimeout(() => reject(new Error("Timed out waiting for probe to resolve")), 2000);
      resolveExpiredProbe = res => {
        clearTimeout(expiredProbeTimeout);
        resolve(res);
      };
    });
    expiredProbePromise.catch(() => {});

    let probeCalls = 0;
    const status = vi.fn(async (_id: string): Promise<RemoteSessionStatusResponse> => {
      probeCalls++;
      if (!startupResolved) {
        return parkedDetails("connected", 90);
      }
      return expiredProbePromise;
    });

    const seams = createMockSeams({ status });
    let exitOutcome: RecoveryOutcome | undefined;

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => {
        emit = handler as (status: TerminalRemoteStatus) => Promise<void>;
        return () => {};
      },
      status,
      retry: seams.status,
      list: seams.list,
      onOutcome: outcome => {
        if (outcome.phase === "exited") exitOutcome = outcome;
      },
    });
    await recovery.subscribed;
    await recovery.ready;
    expect(status).toHaveBeenCalledTimes(1);
    startupResolved = true;

    // 1. Stream expired event arrives at generation 100; probe is held:
    const expiredCompletion = emit({ ...sshAuthority, sessionId: "stable", state: "expired", generation: 100, failure: null, replayGap: null });
    expect(status).toHaveBeenCalledTimes(2);

    // 2. Session is closed via closeSession:
    recovery.closeSession("stable");

    // 3. Now held probe resolves; await completion:
    resolveExpiredProbe(parkedDetails("expired", 100));
    await expiredCompletion;

    // 4. Assert: late expired probe discarded; no exit published
    expect(exitOutcome).toBeUndefined();
    clearTimeout(expiredProbeTimeout);
    recovery.stop();
  });

  it("catches non-structured retry errors in rearm without unhandled rejection", async () => {
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
    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 10, failure: null, replayGap: null });
    await waitError;
    expect(retry).toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(nonStructuredError);
    recovery.stop();
  });

  it("retries subscription indefinitely on initial rejection and resumes startup on recovery", async () => {
    let subscribeAttempts = 0;
    let secondSubscribeEnteredResolve!: () => void;
    const secondSubscribeEnteredPromise = new Promise<void>(r => { secondSubscribeEnteredResolve = r; });

    let timerIdSeq = 0;
    type TimerRecord = { id: number; cb: () => void; delayMs: number };
    const timerMap = new Map<number, TimerRecord>();
    const timerWaiters: Array<(rec: TimerRecord) => void> = [];

    const subscribeMock = vi.fn(async () => {
      subscribeAttempts++;
      if (subscribeAttempts === 1) {
        throw new Error("subscription IPC socket unavailable");
      }
      secondSubscribeEnteredResolve();
      return () => {};
    });

    const mockClock = {
      now: () => Date.now(),
      setTimeout: (cb: () => void, delayMs: number) => {
        const id = ++timerIdSeq;
        const rec: TimerRecord = { id, cb, delayMs };
        timerMap.set(id, rec);
        for (const waiter of [...timerWaiters]) {
          waiter(rec);
        }
        return id;
      },
      clearTimeout: (id: number) => {
        timerMap.delete(id);
      },
      waitForTimer: (predicate: (t: TimerRecord) => boolean) => {
        for (const t of timerMap.values()) {
          if (predicate(t)) return Promise.resolve(t);
        }
        return new Promise<TimerRecord>(resolve => {
          const waiter = (t: TimerRecord) => {
            if (predicate(t)) {
              const idx = timerWaiters.indexOf(waiter);
              if (idx !== -1) timerWaiters.splice(idx, 1);
              resolve(t);
            }
          };
          timerWaiters.push(waiter);
        });
      },
    };

    const dispatch = vi.fn();
    const onError = vi.fn();
    const status = vi.fn(async () => parkedDetails("connected", 10));

    let subscriptionRetryableResolve!: () => void;
    const subscriptionRetryablePromise = new Promise<void>(r => { subscriptionRetryableResolve = r; });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch,
      onError,
      subscribe: subscribeMock as any,
      status: status as any,
      clock: mockClock as any,
      onOutcome: outcome => {
        if (outcome.backendSessionId === "stream_subscription" && outcome.phase === "retryable") {
          subscriptionRetryableResolve();
        }
      },
    });

    // Await affirmative rejection outcome signal from subscription scheduler
    await subscriptionRetryablePromise;

    // Wait for the subscription backoff timer (delayMs: 250) to be registered
    const backoffTimer = await mockClock.waitForTimer(t => t.delayMs === 250);
    expect(subscribeMock).toHaveBeenCalledTimes(1);

    // Fire the registered backoff timer callback
    timerMap.delete(backoffTimer.id);
    backoffTimer.cb();

    // Await affirmative entry of second subscription attempt
    await secondSubscribeEnteredPromise;
    expect(subscribeMock).toHaveBeenCalledTimes(2);

    await recovery.subscribed;
    await recovery.ready;
    expect(dispatch).toHaveBeenCalledWith(expect.objectContaining({
      status: expect.objectContaining({ state: "connected", generation: 10 }),
    }));
    recovery.stop();
  });

  it("drops late connected probe when stream disconnected arrived for same generation", async () => {
    let emit: (status: any) => void = () => {};
    let resolveProbe!: (value: any) => void;
    const probePromise = new Promise(resolve => { resolveProbe = resolve; });
    const status = vi.fn().mockReturnValue(probePromise);
    const seams = createMockSeams({ status: status as any });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: status as any,
      list: seams.list,
    });
    await recovery.subscribed;

    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 20, failure: null, replayGap: null });
    const waitSettled = seams.waitForSettled(20);
    await waitSettled;

    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 21, failure: null, replayGap: null });
    const waitSettled21 = seams.waitForSettled(21);
    await waitSettled21;

    resolveProbe(parkedDetails("connected", 20));

    const waitSettled22 = seams.waitForSettled(22);
    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 22, failure: null, replayGap: null });
    await waitSettled22;

    const lastDispatch = seams.dispatch.mock.calls.at(-1)?.[0];
    expect(lastDispatch.status.state).toBe("disconnected");
    expect(lastDispatch.status.generation).toBe(22);
    recovery.stop();
  });

  it("discards late list reconcile dispatch when deadline expires while list is pending", async () => {
    let resolveList!: (value: any) => void;
    const pendingListPromise = new Promise(resolve => { resolveList = resolve; });
    const listMock = vi.fn().mockReturnValue(pendingListPromise);
    const seams = createMockSeams();

    let timerIdSeq = 0;
    const timerMap = new Map<number, () => void>();
    const mockClock = {
      now: () => Date.now(),
      setTimeout: (cb: () => void) => {
        const id = ++timerIdSeq;
        timerMap.set(id, cb);
        return id;
      },
      clearTimeout: (id: number) => {
        timerMap.delete(id);
      },
    };

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async () => () => {},
      status: async () => parkedDetails("connected", 30),
      list: listMock as any,
      clock: mockClock as any,
      attemptDeadlineMs: 50,
    });
    await recovery.subscribed;

    for (const [id, cb] of [...timerMap.entries()]) {
      timerMap.delete(id);
      cb();
    }

    resolveList([{ sessionId: "stable", daemonEpoch: "stale_epoch" }]);

    const waitReconcileFence = seams.waitForSettled(31);
    seams.dispatch({ type: "SESSION_REMOTE_STATUS", status: { ...sshAuthority, sessionId: "stable", state: "connected", generation: 31, failure: null, replayGap: null }, daemonEpoch: "new" });
    await waitReconcileFence;

    const staleEpochDispatches = seams.dispatch.mock.calls.filter(([a]) => a?.daemonEpoch === "stale_epoch");
    expect(staleEpochDispatches.length).toBe(0);
    recovery.stop();
  });

  it("does not rearm on initial startup when session is already connected", async () => {
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const status = vi.fn(async () => parkedDetails("connected", 100));
    const seams = createMockSeams();

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async () => () => {},
      status: status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;
    await recovery.ready;

    expect(retry).not.toHaveBeenCalled();
    expect(seams.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      status: expect.objectContaining({ state: "connected", generation: 100 }),
    }));
    recovery.stop();
  });

  it("maintains bounded status queries without UI rearm when transitioning ready to reconnecting", async () => {
    let emit: (status: any) => void = () => {};
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    let probeCount = 0;
    const status = vi.fn(async () => {
      probeCount++;
      return parkedDetails("reconnecting", 50 + probeCount);
    });
    const seams = createMockSeams();

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;

    emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 40, failure: null, replayGap: null });
    const waitConnected = seams.waitForSettled(40);
    await waitConnected;

    emit({ ...sshAuthority, sessionId: "stable", state: "reconnecting", generation: 50, failure: null, replayGap: null });
    const waitReconnecting = seams.waitForSettled(50);
    await waitReconnecting;

    expect(retry).not.toHaveBeenCalled();
    recovery.stop();
  });

  it("does not allow stale probe expired to kill newer stream connected state", async () => {
    let emit: (status: any) => void = () => {};
    let resolveProbe!: (value: any) => void;
    const probePromise = new Promise(resolve => { resolveProbe = resolve; });
    const status = vi.fn().mockReturnValue(probePromise);
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const seams = createMockSeams({ status: status as any });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;

    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 10, failure: null, replayGap: null });
    const waitDisconnected = seams.waitForSettled(10);
    await waitDisconnected;

    emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 11, failure: null, replayGap: null });
    const waitConnected = seams.waitForSettled(11);
    await waitConnected;

    resolveProbe(parkedDetails("expired", 10));

    const waitSettled12 = seams.waitForSettled(12);
    emit({ ...sshAuthority, sessionId: "stable", state: "connected", generation: 12, failure: null, replayGap: null });
    await waitSettled12;

    const lastDispatch = seams.dispatch.mock.calls.at(-1)?.[0];
    expect(lastDispatch.status.state).toBe("connected");
    expect(lastDispatch.status.generation).toBe(12);
    recovery.stop();
  });

  it("blocks immediately on disconnected authentication failure without attempting rearm", async () => {
    let emit: (status: any) => void = () => {};
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const seams = createMockSeams();

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      retry,
      list: seams.list,
    });
    await recovery.subscribed;

    emit({
      ...sshAuthority,
      sessionId: "stable",
      state: "disconnected",
      generation: 15,
      failure: { kind: "authentication", message: "SSH key rejected" },
      replayGap: null,
    });
    const waitBlocked = seams.waitForSettled(15);
    await waitBlocked;

    expect(retry).not.toHaveBeenCalled();
    recovery.stop();
  });

  it("does not rearm when stale probe returns disconnected after stream arrived with auth blocked", async () => {
    let emit: (status: any) => void = () => {};
    let resolveProbe!: (value: any) => void;
    const probePromise = new Promise(resolve => { resolveProbe = resolve; });
    const status = vi.fn().mockReturnValue(probePromise);
    const retry = vi.fn(async () => ({ type: "retryRemoteSessionOk" as const }));
    const seams = createMockSeams({ status: status as any });

    const recovery = startSshRecovery({
      sessions: [session],
      dispatch: seams.dispatch,
      onError: seams.onError,
      subscribe: async handler => { emit = handler; return () => {}; },
      status: status as any,
      retry,
      list: seams.list,
    });
    await recovery.subscribed;

    emit({ ...sshAuthority, sessionId: "stable", state: "disconnected", generation: 10, failure: null, replayGap: null });
    const waitDisconnected = seams.waitForSettled(10);
    await waitDisconnected;

    emit({
      ...sshAuthority,
      sessionId: "stable",
      state: "disconnected",
      generation: 11,
      failure: { kind: "authentication", message: "Key revoked" },
      replayGap: null,
    });
    const waitBlocked = seams.waitForSettled(11);
    await waitBlocked;

    resolveProbe(parkedDetails("disconnected", 10));

    const waitSettle = seams.waitForSettled(12);
    emit({
      ...sshAuthority,
      sessionId: "stable",
      state: "disconnected",
      generation: 12,
      failure: { kind: "authentication", message: "Key revoked" },
      replayGap: null,
    });
    await waitSettle;

    expect(retry).not.toHaveBeenCalled();
    recovery.stop();
  });
});

describe("coalesceListCalls", () => {
  it("runs one call at a time and serves every caller that arrived mid-flight from one follow-up", async () => {
    const resolvers: Array<(value: number) => void> = [];
    const list = vi.fn(() => new Promise<number>(resolve => { resolvers.push(resolve); }));
    const coalesced = coalesceListCalls(list);

    const first = coalesced();
    const burst = Array.from({ length: 50 }, () => coalesced());
    expect(list).toHaveBeenCalledTimes(1);

    resolvers[0](1);
    await expect(first).resolves.toBe(1);
    await vi.waitFor(() => expect(list).toHaveBeenCalledTimes(2));
    resolvers[1](2);
    await expect(Promise.all(burst)).resolves.toEqual(Array(50).fill(2));
    expect(list).toHaveBeenCalledTimes(2);
  });

  it("still issues the follow-up when the in-flight call fails", async () => {
    let calls = 0;
    const coalesced = coalesceListCalls(async () => {
      calls += 1;
      if (calls === 1) throw new Error("daemon busy");
      return calls;
    });
    const first = coalesced();
    const second = coalesced();
    await expect(first).rejects.toThrow("daemon busy");
    await expect(second).resolves.toBe(2);
    await expect(coalesced()).resolves.toBe(3);
  });
});
