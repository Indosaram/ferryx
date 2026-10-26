import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { createLayoutState } from "./layout";
import {
  clearWorkspaceSnapshot,
  getWorkspaceSnapshot,
  setWorkspaceSnapshot,
  subscribeWorkspaceSnapshots,
} from "./workspaceSnapshotCache";
import {
  requestLocalRecoveryAdmission,
  resetLocalRecoveryForTests,
  setLocalRecoveryClockForTests,
  setLocalRecoveryPersistence,
  workspaceReducer,
  type LocalRecoveryAdmissionResult,
  type WorkspaceState,
  useWorkspaceStore,
} from "./workspaceStore";
import type { RecoveryClock } from "../lib/sessionRecovery";

const describeTerminalMock = vi.fn();
vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  describeTerminal: (id: string) => describeTerminalMock(id),
  isTauriRuntime: () => true,
}));

const startSshRecoveryMock = vi.fn();
vi.mock("../lib/sshRecovery", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/sshRecovery")>()),
  startSshRecovery: (options: unknown) => startSshRecoveryMock(options),
}));

const PAIRED_ID = `daemon:${"b".repeat(64)}`;

function session(id: string, workspaceId: string, backendSessionId: string | null): TerminalSession {
  return {
    id,
    workspaceId,
    worktree: null,
    backendSessionId,
    worktreePath: "/repo/main",
    cwd: "/repo/main",
    lifecycle: "working",
    daemonEpoch: "epoch-old",
    incarnation: "incarnation-1",
    lastOutputSequence: "450",
    remoteConnectionState: "reconnecting",
  };
}

function stateWith(sessions: TerminalSession[], workspaceId = "local"): WorkspaceState {
  return {
    workspaceId,
    worktrees: [],
    activeWorktreePath: "/repo/main",
    sessions: Object.fromEntries(sessions.map((candidate) => [candidate.id, candidate])),
    layout: createLayoutState(
      sessions.map((candidate) => ({
        id: `tab-${candidate.id}`,
        sessionId: candidate.id,
        label: candidate.id,
        kind: "terminal" as const,
      })),
    ),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
}

function virtualRecoveryClock(): RecoveryClock & {
  advanceBy(ms: number): void;
  waitForTimer(ms: number): Promise<void>;
  waitForSignal(signal: (resolve: () => void) => void): Promise<void>;
} {
  let now = 0;
  let nextId = 0;
  const timers = new Map<number, { at: number; callback: () => void }>();
  const delayWaiters = new Map<number, Array<() => void>>();
  return {
    now: () => now,
    setTimeout(callback, ms) {
      const id = ++nextId;
      const delay = Math.max(0, ms);
      timers.set(id, { at: now + delay, callback });
      for (const resolve of delayWaiters.get(delay) ?? []) resolve();
      delayWaiters.delete(delay);
      return id;
    },
    clearTimeout(handle) {
      timers.delete(handle as number);
    },
    advanceBy(ms) {
      const target = now + ms;
      while (true) {
        const due = [...timers.entries()]
          .filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due;
        timers.delete(id);
        now = timer.at;
        timer.callback();
      }
      now = target;
    },
    waitForTimer(ms) {
      return new Promise((resolve) => {
        const waiters = delayWaiters.get(ms) ?? [];
        waiters.push(resolve);
        delayWaiters.set(ms, waiters);
      });
    },
    waitForSignal(signal) {
      return new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error("timed out waiting for virtual recovery signal")), 5_000);
        signal(() => {
          clearTimeout(timer);
          resolve();
        });
      });
    },
  };
}

describe("LOCAL_SESSIONS_RECONCILED", () => {
  it("clears reconnecting on a live local session and adopts the live epoch", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: "epoch-new", running: true, incarnation: "incarnation-1" }]]),
    });
    expect(next.sessions.s1.remoteConnectionState).toBeUndefined();
    expect(next.sessions.s1).toMatchObject({
      backendSessionId: "b1",
      lifecycle: "running",
      daemonEpoch: "epoch-new",
      lastOutputSequence: null,
    });
  });

  it("keeps the replay cursor when the live epoch is unchanged", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: "epoch-old", running: true, incarnation: "incarnation-1" }]]),
    });
    expect(next.sessions.s1).toMatchObject({ lifecycle: "running", lastOutputSequence: "450" });
  });

  it("reconciles a loaded retrying session only when authoritative identity proof arrives", () => {
    const initial = { ...session("loaded", "local", "backend-loaded"), incarnation: "inc-1", remoteConnectionState: "reconnecting" as const };
    const next = workspaceReducer(stateWith([initial]), {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["backend-loaded", { daemonEpoch: "epoch-2", running: true, incarnation: "inc-1" }]]),
    });
    expect(next.sessions.loaded).toMatchObject({
      backendSessionId: "backend-loaded",
      incarnation: "inc-1",
      daemonEpoch: "epoch-2",
      lifecycle: "running",
    });
    expect(next.sessions.loaded.remoteConnectionState).toBeUndefined();
  });

  it("preserves a missing local session identity as retryable uncertainty", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() });
    expect(next.sessions.s1.remoteConnectionState).toBe("reconnecting");
    expect(next.sessions.s1).toMatchObject({ backendSessionId: "b1", lifecycle: "working", reconnectLifecycle: "idle" });
  });

  it("preserves nonrunning inventory without exit authority as retryable uncertainty", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([["b1", { daemonEpoch: null, running: false }]]),
    });
    expect(next.sessions.s1).toMatchObject({ backendSessionId: "b1", lifecycle: "working", remoteConnectionState: "reconnecting", reconnectLifecycle: "idle" });
  });

  it("retains retryable uncertainty on query failure", () => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: null });
    expect(next.sessions.s1).toMatchObject({ backendSessionId: "b1", remoteConnectionState: "reconnecting" });
  });

  it.each([0, 37, null])("retains exit code %s when authoritative inventory proves affirmative exit", (exitCode) => {
    const state = stateWith([session("s1", "local", "b1")]);
    const next = workspaceReducer(state, {
      type: "LOCAL_SESSIONS_RECONCILED",
      live: new Map([
        [
          "b1",
          {
            daemonEpoch: "epoch-old",
            running: false,
            incarnation: "incarnation-1",
            recoveryAuthority: {
              status: "affirmative_exit",
              exitCode,
              incarnation: "incarnation-1",
              daemonEpoch: "epoch-old",
            },
          },
        ],
      ]),
    });
    expect(next.sessions.s1).toMatchObject({ lifecycle: "exited", backendSessionId: null, exitCode });
  });

  it("leaves ssh and paired sessions untouched", () => {
    const state = stateWith([session("ssh-s", "ssh:host-a", "b-ssh"), session("paired-s", PAIRED_ID, "b-paired")]);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() })).toBe(state);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: null })).toBe(state);
  });

  it("ignores local sessions that are not reconnecting", () => {
    const connected = { ...session("s1", "local", "b1"), remoteConnectionState: undefined };
    const state = stateWith([connected]);
    expect(workspaceReducer(state, { type: "LOCAL_SESSIONS_RECONCILED", live: new Map() })).toBe(state);
  });
});

afterEach(() => {
  resetLocalRecoveryForTests();
  clearWorkspaceSnapshot();
});

function waitForSnapshotUpdate(
  predicate: () => boolean,
  timeoutMs = 5_000,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe();
      reject(new Error("timed out waiting for workspace snapshot update"));
    }, timeoutMs);
    const unsubscribe = subscribeWorkspaceSnapshots(() => {
      if (predicate()) {
        clearTimeout(timer);
        unsubscribe();
        resolve();
      }
    });
    if (predicate()) {
      clearTimeout(timer);
      unsubscribe();
      resolve();
    }
  });
}

describe("local recovery retry identity_conflict revalidation", () => {
  it("settles a reconnecting agent pane as exited once the daemon confirms its backend absent", async () => {
    resetLocalRecoveryForTests();
    const clock = virtualRecoveryClock();
    const retained: TerminalSession = { ...session("absent-s1", "local", "backend-absent-1"), agentType: "omo",
      providerSession: { key: "session_id", id: "conversation-1" } };
    setWorkspaceSnapshot("local", stateWith([retained]));
    setLocalRecoveryClockForTests(clock);
    const absent = { code: "SESSION_NOT_FOUND", message: "Session 'backend-absent-1' not found",
      details: { source: "daemon_describe", kind: "session_not_found", reason: "scoped_missing" } };
    describeTerminalMock.mockReset();
    describeTerminalMock.mockRejectedValue(absent);
    const exited = clock.waitForSignal((resolve) => {
      const unsubscribe = subscribeWorkspaceSnapshots(() => {
        if (getWorkspaceSnapshot("local")?.sessions["absent-s1"]?.lifecycle === "exited") {
          unsubscribe();
          resolve();
        }
      });
    });
    const retry = clock.waitForTimer(250);
    setLocalRecoveryPersistence(async () => {});
    await retry;
    clock.advanceBy(250);
    await exited;
    const settled = getWorkspaceSnapshot("local")?.sessions["absent-s1"];
    expect(settled).toMatchObject({ lifecycle: "exited", backendSessionId: null, agentType: "omo",
      providerSession: { key: "session_id", id: "conversation-1" }, recoveryState: { phase: "exited", reason: "absent" } });
    expect(settled?.remoteConnectionState).toBeUndefined();
    expect(describeTerminalMock).toHaveBeenCalledTimes(2);
    resetLocalRecoveryForTests();
    clearWorkspaceSnapshot("local");
    describeTerminalMock.mockReset();
  });

  it("keeps retrying when the predecessor that owns the session is unreachable", async () => {
    resetLocalRecoveryForTests();
    const clock = virtualRecoveryClock();
    setWorkspaceSnapshot("local", stateWith([session("unreachable-s1", "local", "backend-unreachable-1")]));
    setLocalRecoveryClockForTests(clock);
    describeTerminalMock.mockReset();
    describeTerminalMock.mockRejectedValue({ code: "SESSION_NOT_FOUND", message: "peer unreachable",
      details: { source: "daemon_describe", kind: "owner_unavailable", reason: "legacy_peer_unreachable" } });
    let timer = clock.waitForTimer(250);
    setLocalRecoveryPersistence(async () => {});
    for (const delay of [250, 500, 1_000, 2_000]) {
      await timer;
      timer = clock.waitForTimer(delay * 2);
      clock.advanceBy(delay);
    }
    await timer;
    const retained = getWorkspaceSnapshot("local")?.sessions["unreachable-s1"];
    expect(retained).toMatchObject({ backendSessionId: "backend-unreachable-1", lifecycle: "working",
      remoteConnectionState: "reconnecting" });
    expect(describeTerminalMock).toHaveBeenCalledTimes(5);
    resetLocalRecoveryForTests();
    clearWorkspaceSnapshot("local");
    describeTerminalMock.mockReset();
  });

  it("retries a background-loaded session beyond thirty virtual seconds and recovers the retained identity", async () => {
    resetLocalRecoveryForTests();
    const clock = virtualRecoveryClock();
    const retained = session("loaded-s1", "local", "backend-loaded-1");
    retained.incarnation = "incarnation-1";
    retained.remoteConnectionState = "reconnecting";
    setWorkspaceSnapshot("local", stateWith([retained]));
    setLocalRecoveryClockForTests(clock);
    let probeCount = 0;
    let resolveProbe: ((value: unknown) => void) | undefined;
    let notifyProbeStarted: (() => void) | undefined;
    describeTerminalMock.mockReset();
    describeTerminalMock.mockImplementation(() => {
      probeCount += 1;
      const started = notifyProbeStarted;
      const result = new Promise((resolve) => { resolveProbe = resolve; });
      if (started) notifyProbeStarted = undefined;
      started?.();
      return result;
    });
    const delays = [250, 500, 1_000, 2_000, 4_000, 4_000, 4_000, 4_000, 4_000, 4_000, 4_000];
    let virtualElapsed = 0;
    const firstProbe = clock.waitForSignal((resolve) => { notifyProbeStarted = resolve; });
    const firstTimer = clock.waitForTimer(250);
    setLocalRecoveryPersistence(async () => {});
    await firstProbe;
    resolveProbe?.(null);
    await firstTimer;
    let probeStarted = new Promise<void>((resolve) => { notifyProbeStarted = resolve; });
    clock.advanceBy(250);
    await probeStarted;
    virtualElapsed += 250;
    for (let index = 1; index < delays.length; index += 1) {
      const delay = delays[index];
      const timerScheduled = clock.waitForTimer(delay);
      resolveProbe?.(null);
      await timerScheduled;
      const nextProbeStarted = clock.waitForSignal((resolve) => { notifyProbeStarted = resolve; });
      clock.advanceBy(delay);
      probeStarted = nextProbeStarted;
      await probeStarted;
      virtualElapsed += delay;
    }
    expect(virtualElapsed).toBeGreaterThan(30_000);
    const ready = clock.waitForSignal((resolve) => {
      const unsubscribe = subscribeWorkspaceSnapshots(() => {
        const current = getWorkspaceSnapshot("local")?.sessions["loaded-s1"];
        if (current?.backendSessionId === "backend-loaded-1" && current.daemonEpoch === "epoch-new" &&
            current.incarnation === "incarnation-1") {
          unsubscribe();
          resolve();
        }
      });
      if (getWorkspaceSnapshot("local")?.sessions["loaded-s1"]?.daemonEpoch === "epoch-new") {
        unsubscribe();
        resolve();
      }
    });
    let onAdmissionResult: (result: LocalRecoveryAdmissionResult) => void = () => {};
    const admissionResult = new Promise<LocalRecoveryAdmissionResult>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("timed out waiting for loaded-session admission result")), 5_000);
      onAdmissionResult = (result) => {
        if (result.kind !== "pending") {
          clearTimeout(timer);
          resolve(result);
        }
      };
    });
    requestLocalRecoveryAdmission("local", "loaded-s1", false, (result) => {
      onAdmissionResult(result);
    });
    resolveProbe?.({
      sessionId: "backend-loaded-1", incarnation: "incarnation-1", daemonEpoch: "epoch-new", running: true,
      recoveryAuthority: { status: "running", incarnation: "incarnation-1", daemonEpoch: "epoch-new" },
    });
    await ready;
    expect(await admissionResult).toMatchObject({ kind: "ready", session: {
      backendSessionId: "backend-loaded-1", incarnation: "incarnation-1", daemonEpoch: "epoch-new",
    } });
    const recovered = getWorkspaceSnapshot("local")?.sessions["loaded-s1"];
    expect(recovered).toMatchObject({ backendSessionId: "backend-loaded-1", incarnation: "incarnation-1", daemonEpoch: "epoch-new" });
    expect(recovered?.remoteConnectionState).toBeUndefined();
    expect(recovered?.lifecycle).toBe("running");
    expect(probeCount).toBe(delays.length + 1);
    resetLocalRecoveryForTests();
    clearWorkspaceSnapshot("local");
    describeTerminalMock.mockReset();
  });

  it("passive admission without revalidate returns blocked on identity conflict without probing", () => {
    describeTerminalMock.mockReset();
    const s = session("s1", "local", "b1");
    s.backendUnavailableReason = "identity_conflict";
    setWorkspaceSnapshot("local", stateWith([s]));
    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "s1", false, (r) => results.push(r));
    expect(results).toEqual([{ kind: "blocked", reason: "identity_conflict" }]);
    expect(describeTerminalMock).not.toHaveBeenCalled();
    clearWorkspaceSnapshot("local");
  });

  it("manual retry with revalidate unblocks only after affirmative original identity proof", async () => {
    describeTerminalMock.mockReset();
    const s = session("s1", "local", "b1");
    s.backendUnavailableReason = "identity_conflict";
    s.incarnation = "incarnation-1";
    setWorkspaceSnapshot("local", stateWith([s]));
    setLocalRecoveryPersistence(async () => {});
    describeTerminalMock.mockResolvedValueOnce({
      sessionId: "b1",
      incarnation: "incarnation-1",
      daemonEpoch: "epoch-new",
      running: true,
      recoveryAuthority: {
        status: "running",
        incarnation: "incarnation-1",
        daemonEpoch: "epoch-new",
      },
    });
    const updated = waitForSnapshotUpdate(() => {
      const current = getWorkspaceSnapshot("local")?.sessions.s1;
      return Boolean(current && current.backendUnavailableReason !== "identity_conflict");
    });
    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);
    await updated;
    expect(describeTerminalMock).toHaveBeenCalledWith("b1");
    const current = getWorkspaceSnapshot("local")?.sessions.s1;
    expect(current?.backendSessionId).toBe("b1");
    expect(current?.incarnation).toBe("incarnation-1");
    clearWorkspaceSnapshot("local");
    setLocalRecoveryPersistence(undefined);
  });

  it("manual retry does not unblock on changed incarnation and retains original identity", async () => {
    describeTerminalMock.mockReset();
    const s = session("s1", "local", "b1");
    s.backendUnavailableReason = "identity_conflict";
    s.incarnation = "incarnation-1";
    setWorkspaceSnapshot("local", stateWith([s]));
    setLocalRecoveryPersistence(async () => {});
    describeTerminalMock.mockResolvedValueOnce({
      sessionId: "b1",
      incarnation: "incarnation-different",
      daemonEpoch: "epoch-new",
      running: true,
      recoveryAuthority: {
        status: "running",
        incarnation: "incarnation-different",
        daemonEpoch: "epoch-new",
      },
    });
    const results: LocalRecoveryAdmissionResult[] = [];
    let onSettled: () => void = () => {};
    const settled = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error("timed out waiting for admission decision"));
      }, 5_000);
      onSettled = () => {
        clearTimeout(timer);
        resolve();
      };
    });
    requestLocalRecoveryAdmission("local", "s1", true, (r) => {
      results.push(r);
      if (r.kind !== "pending") onSettled();
    });
    expect(results).toEqual([{ kind: "pending" }]);
    await settled;
    expect(results).toEqual([{ kind: "pending" }, { kind: "blocked", reason: "identity_conflict" }]);
    const current = getWorkspaceSnapshot("local")?.sessions.s1;
    expect(current?.backendSessionId).toBe("b1");
    expect(current?.incarnation).toBe("incarnation-1");
    expect(current?.backendUnavailableReason).toBe("identity_conflict");
    clearWorkspaceSnapshot("local");
    setLocalRecoveryPersistence(undefined);
  });

  it("manual retry stays blocked when authority reports conflict or non-running", async () => {
    describeTerminalMock.mockReset();
    const s = session("s1", "local", "b1");
    s.backendUnavailableReason = "identity_conflict";
    s.incarnation = "incarnation-1";
    setWorkspaceSnapshot("local", stateWith([s]));
    setLocalRecoveryPersistence(async () => {});
    describeTerminalMock.mockResolvedValueOnce({
      sessionId: "b1",
      incarnation: "incarnation-1",
      daemonEpoch: "epoch-new",
      running: false,
      recoveryAuthority: {
        status: "identity_conflict",
        incarnation: "incarnation-1",
        daemonEpoch: "epoch-new",
      },
    });
    const results: LocalRecoveryAdmissionResult[] = [];
    let onSettled: () => void = () => {};
    const settled = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error("timed out waiting for admission decision"));
      }, 5_000);
      onSettled = () => {
        clearTimeout(timer);
        resolve();
      };
    });
    requestLocalRecoveryAdmission("local", "s1", true, (r) => {
      results.push(r);
      if (r.kind !== "pending") onSettled();
    });
    expect(results).toEqual([{ kind: "pending" }]);
    await settled;
    expect(results).toEqual([{ kind: "pending" }, { kind: "blocked", reason: "identity_conflict" }]);
    const current = getWorkspaceSnapshot("local")?.sessions.s1;
    expect(current?.backendSessionId).toBe("b1");
    expect(current?.incarnation).toBe("incarnation-1");
    expect(current?.backendUnavailableReason).toBe("identity_conflict");
    clearWorkspaceSnapshot("local");
    setLocalRecoveryPersistence(undefined);
  });

  it("preserves concurrent other pane updates while describeTerminal probe is held", async () => {
    describeTerminalMock.mockReset();
    const s1 = session("s1", "local", "b1");
    s1.backendUnavailableReason = "identity_conflict";
    s1.incarnation = "incarnation-1";
    const s2 = session("s2", "local", "b2");
    s2.lastOutputSequence = "100";
    setWorkspaceSnapshot("local", stateWith([s1, s2]));
    setLocalRecoveryPersistence(async () => {});

    let resolveProbe: (value: unknown) => void = () => {};
    const heldProbe = new Promise((resolve) => { resolveProbe = resolve; });
    describeTerminalMock.mockReturnValueOnce(heldProbe);

    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);

    // While probe is held, an unrelated session in the same workspace is updated:
    const midSnapshot = getWorkspaceSnapshot("local")!;
    setWorkspaceSnapshot("local", {
      ...midSnapshot,
      sessions: {
        ...midSnapshot.sessions,
        s2: { ...midSnapshot.sessions.s2, lastOutputSequence: "999", cwd: "/repo/updated" },
      },
    });

    const updated = waitForSnapshotUpdate(() => {
      const snap = getWorkspaceSnapshot("local");
      return Boolean(snap?.sessions.s1 && snap.sessions.s1.backendUnavailableReason !== "identity_conflict");
    });

    // Now resolve probe affirmatively:
    resolveProbe({
      sessionId: "b1",
      incarnation: "incarnation-1",
      daemonEpoch: "epoch-new",
      running: true,
      recoveryAuthority: {
        status: "running",
        incarnation: "incarnation-1",
        daemonEpoch: "epoch-new",
      },
    });

    await updated;

    // Verify s1 was admitted without clobbering s2:
    const finalSnapshot = getWorkspaceSnapshot("local")!;
    expect(finalSnapshot.sessions.s2.lastOutputSequence).toBe("999");
    expect(finalSnapshot.sessions.s2.cwd).toBe("/repo/updated");
    expect(finalSnapshot.sessions.s1.backendSessionId).toBe("b1");
    expect(finalSnapshot.sessions.s1.incarnation).toBe("incarnation-1");

    clearWorkspaceSnapshot("local");
    setLocalRecoveryPersistence(undefined);
  });

  it("rejects late probe resolution if owner token changed while probe was held", async () => {
    describeTerminalMock.mockReset();
    const s = session("s1", "local", "b1");
    s.backendUnavailableReason = "identity_conflict";
    s.incarnation = "incarnation-1";
    setWorkspaceSnapshot("local", stateWith([s]));

    let resolveProbe: (value: unknown) => void = () => {};
    let resolveProbeStarted!: () => void;
    const probeStarted = new Promise<void>((resolve) => { resolveProbeStarted = resolve; });
    const heldProbe = new Promise((resolve) => { resolveProbe = resolve; });
    describeTerminalMock.mockImplementationOnce(() => {
      resolveProbeStarted();
      return heldProbe;
    });

    const results: LocalRecoveryAdmissionResult[] = [];
    let notifyResult: ((result: LocalRecoveryAdmissionResult) => void) | undefined;
    const cancelResult = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("timed out waiting for token-change cancellation")), 5_000);
      notifyResult = (r) => {
        if (r.kind === "cancelled") {
          clearTimeout(timer);
          resolve();
        }
      };
    });
    setLocalRecoveryPersistence(async () => {});
    requestLocalRecoveryAdmission("local", "s1", true, (r) => {
      results.push(r);
      notifyResult?.(r);
    });
    expect(results).toEqual([{ kind: "pending" }]);
    await probeStarted;

    const tokenChanged = waitForSnapshotUpdate(() =>
      getWorkspaceSnapshot("local")?.sessions.s1?.incarnation === "incarnation-replaced",
    );

    // While probe is held, external event replaces the token on s1:
    const current = getWorkspaceSnapshot("local")!;
    setWorkspaceSnapshot("local", {
      ...current,
      sessions: {
        ...current.sessions,
        s1: { ...current.sessions.s1, incarnation: "incarnation-replaced" },
      },
    });

    await tokenChanged;

    // Resolve probe:
    resolveProbe({
      sessionId: "b1",
      incarnation: "incarnation-1",
      daemonEpoch: "epoch-new",
      running: true,
      recoveryAuthority: {
        status: "running",
        incarnation: "incarnation-1",
        daemonEpoch: "epoch-new",
      },
    });

    await cancelResult;

    // Must be cancelled / rejected because owner token changed:
    expect(results).toContainEqual({ kind: "cancelled" });

    clearWorkspaceSnapshot("local");
    setLocalRecoveryPersistence(undefined);
  });

  describe("useWorkspaceStore ssh recovery effect stability", () => {
    it("retains recovery owner across status updates without resubscribing and disposes on close/rebind", async () => {
      const initialStopMock = vi.fn();
      const reboundStopMock = vi.fn();
      startSshRecoveryMock.mockReset();
      startSshRecoveryMock
        .mockReturnValueOnce({
          ready: Promise.resolve(),
          stop: initialStopMock,
        })
        .mockReturnValueOnce({
          ready: Promise.resolve(),
          stop: reboundStopMock,
        });

      const initialSession: TerminalSession = {
        id: "front-ssh-1",
        workspaceId: "ssh:server-1",
        worktree: null,
        backendSessionId: "backend-ssh-1",
        worktreePath: "/remote/repo",
        cwd: "/remote/repo",
        lifecycle: "working",
        incarnation: "inc-1",
        daemonEpoch: "epoch-1",
        remoteConnectionState: "disconnected",
      };

      setWorkspaceSnapshot("ssh:server-1", stateWith([initialSession], "ssh:server-1"));

      const { result, unmount } = renderHook(() =>
        useWorkspaceStore({ workspaceId: "ssh:server-1" }),
      );

      expect(startSshRecoveryMock).toHaveBeenCalledTimes(1);
      expect(initialStopMock).not.toHaveBeenCalled();

      // 1. Transient connection state change (outcome dispatch) must NOT reset owner or resubscribe:
      act(() => {
        result.current.dispatchWorkspaceAction({
          type: "SESSION_REMOTE_STATUS",
          status: {
            sessionId: "backend-ssh-1",
            daemonEpoch: "epoch-1",
            incarnation: "inc-1",
            target: { kind: "ssh", hostId: "server-1" },
            state: "reconnecting",
            generation: 1,
            failure: null,
            replayGap: null,
          },
        });
      });

      expect(result.current.state.sessions["front-ssh-1"]?.remoteConnectionState).toBe("reconnecting");
      expect(startSshRecoveryMock).toHaveBeenCalledTimes(1);
      expect(initialStopMock).not.toHaveBeenCalled();

      // 2. Further status update to connected must also retain owner without resetting:
      act(() => {
        result.current.dispatchWorkspaceAction({
          type: "SESSION_REMOTE_STATUS",
          status: {
            sessionId: "backend-ssh-1",
            daemonEpoch: "epoch-1",
            incarnation: "inc-1",
            target: { kind: "ssh", hostId: "server-1" },
            state: "connected",
            generation: 2,
            failure: null,
            replayGap: null,
          },
        });
      });

      expect(result.current.state.sessions["front-ssh-1"]?.remoteConnectionState).toBe("connected");
      expect(startSshRecoveryMock).toHaveBeenCalledTimes(1);
      expect(initialStopMock).not.toHaveBeenCalled();

      // 3. Supported identity update seam (REBIND_SESSION_BACKEND) triggers oldstop then newstart:
      act(() => {
        result.current.dispatchWorkspaceAction({
          type: "REBIND_SESSION_BACKEND",
          sessionId: "front-ssh-1",
          backendSessionId: "backend-ssh-2",
          incarnation: "inc-2",
        });
      });

      expect(result.current.state.sessions["front-ssh-1"]?.backendSessionId).toBe("backend-ssh-2");
      expect(result.current.state.sessions["front-ssh-1"]?.incarnation).toBe("inc-2");
      expect(initialStopMock).toHaveBeenCalledTimes(1); // oldstop
      expect(startSshRecoveryMock).toHaveBeenCalledTimes(2); // newstart
      expect(reboundStopMock).not.toHaveBeenCalled();

      // Status update on the new owner does not reset:
      act(() => {
        result.current.dispatchWorkspaceAction({
          type: "SESSION_REMOTE_STATUS",
          status: {
            sessionId: "backend-ssh-2",
            daemonEpoch: "epoch-1",
            incarnation: "inc-2",
            target: { kind: "ssh", hostId: "server-1" },
            state: "reconnecting",
            generation: 1,
            failure: null,
            replayGap: null,
          },
        });
      });

      expect(startSshRecoveryMock).toHaveBeenCalledTimes(2);
      expect(reboundStopMock).not.toHaveBeenCalled();

      // 4. Session-close dispatch triggers closedownerstop:
      act(() => {
        result.current.dispatchWorkspaceAction({
          type: "CLOSE_TAB",
          tabId: "tab-front-ssh-1",
        });
      });

      expect(result.current.state.sessions["front-ssh-1"]).toBeUndefined();
      expect(reboundStopMock).toHaveBeenCalledTimes(1); // closedownerstop
      expect(startSshRecoveryMock).toHaveBeenCalledTimes(2); // no new start because all sessions are closed

      unmount();

      clearWorkspaceSnapshot("ssh:server-1");
    });
  });
});
