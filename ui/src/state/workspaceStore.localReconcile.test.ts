import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TerminalSession } from "../lib/types";
import { createLayoutState } from "./layout";
import {
  clearWorkspaceSnapshot,
  getWorkspaceSnapshot,
  setWorkspaceSnapshot,
} from "./workspaceSnapshotCache";
import {
  requestLocalRecoveryAdmission,
  setLocalRecoveryPersistence,
  workspaceReducer,
  type LocalRecoveryAdmissionResult,
  type WorkspaceState,
  useWorkspaceStore,
} from "./workspaceStore";

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

  it("settles a local session as exited when authoritative inventory proves affirmative exit", () => {
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
              exitCode: 0,
              incarnation: "incarnation-1",
              daemonEpoch: "epoch-old",
            },
          },
        ],
      ]),
    });
    expect(next.sessions.s1).toMatchObject({ lifecycle: "exited", backendSessionId: null });
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

describe("local recovery retry identity_conflict revalidation", () => {
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
    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);
    await Promise.resolve();
    await Promise.resolve();
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
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);
    await Promise.resolve();
    await Promise.resolve();
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
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);
    await Promise.resolve();
    await Promise.resolve();
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

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

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
    s.recoveryToken = 1;
    s.recoveryKey = "test-key";
    setWorkspaceSnapshot("local", stateWith([s]));
    setLocalRecoveryPersistence(async () => {});

    let resolveProbe: (value: unknown) => void = () => {};
    const heldProbe = new Promise((resolve) => { resolveProbe = resolve; });
    describeTerminalMock.mockReturnValueOnce(heldProbe);

    const results: LocalRecoveryAdmissionResult[] = [];
    requestLocalRecoveryAdmission("local", "s1", true, (r) => results.push(r));
    expect(results).toEqual([{ kind: "pending" }]);

    // While probe is held, external event replaces the token on s1:
    const current = getWorkspaceSnapshot("local")!;
    setWorkspaceSnapshot("local", {
      ...current,
      sessions: {
        ...current.sessions,
        s1: { ...current.sessions.s1, recoveryToken: 42 },
      },
    });

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

    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();

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
