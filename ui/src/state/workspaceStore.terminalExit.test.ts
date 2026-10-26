import { describe, expect, it, vi } from "vitest";

import { workspaceReducer, reconcileSessionFromDescribe, type WorkspaceState } from "./workspaceStore";
import { createLayoutState } from "./layout";
import * as tauri from "../lib/tauri";
import type { TerminalSession, TerminalTab } from "../lib/types";

function createInitialState(session: TerminalSession): WorkspaceState {
  const tab: TerminalTab = {
    id: "tab-1",
    label: "Test Tab",
    sessionId: session.id,
  };
  return {
    workspaceId: session.workspaceId,
    worktrees: [],
    activeWorktreePath: "/repo",
    layout: createLayoutState([tab], "tab-1"),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: {
      [session.id]: session,
    },
    activityBySessionId: {},
  };
}

describe("workspaceStore SESSION_BACKEND_UNAVAILABLE compare-and-set reducer", () => {
  it("preserves backendSessionId and activity under INV-1 when owner is unavailable", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-live-1",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-live-1",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["term-1"].lifecycle).toBe("working");
    expect(next.sessions["term-1"].backendSessionId).toBe("backend-live-1");
    expect(next.sessions["term-1"].reconnectLifecycle ?? "idle").toBe("idle");
  });

  it("drops a stale disconnected transport state when the local backend is affirmatively exited", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-live-1",
      lifecycle: "working",
      processState: "running",
      remoteConnectionState: "disconnected",
      reconnectLifecycle: "failed",
      incarnation: "inc-1",
      daemonEpoch: "epoch-1",
      recoveryKey: "key-1",
      recoveryToken: 1,
    };

    const next = workspaceReducer(createInitialState(session), {
      type: "SESSION_AFFIRMATIVE_EXIT",
      sessionId: "term-1",
      backendSessionId: "backend-live-1",
      incarnation: "inc-1",
      daemonEpoch: "epoch-1",
      key: "key-1",
      token: 1,
      reason: "process-exited",
      exitCode: 37,
    });

    expect(next.sessions["term-1"].remoteConnectionState).toBeUndefined();
    expect(next.sessions["term-1"]).toMatchObject({ lifecycle: "exited", processState: "standby", reconnectLifecycle: "idle", exitCode: 37 });
  });

  it("drops a stale disconnected transport state when an identity-fenced local PTY exit arrives", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-live-1",
      lifecycle: "working",
      processState: "running",
      remoteConnectionState: "disconnected",
      incarnation: "inc-1",
      daemonEpoch: "epoch-1",
    };

    const next = workspaceReducer(createInitialState(session), {
      type: "SESSION_LIFECYCLE",
      backendSessionId: "backend-live-1",
      lifecycle: "exited",
      incarnation: "inc-1",
      daemonEpoch: "epoch-1",
    });

    expect(next.sessions["term-1"].remoteConnectionState).toBeUndefined();
    expect(next.sessions["term-1"]).toMatchObject({ lifecycle: "exited", backendSessionId: null, processState: "standby" });
  });

  it.each([0, 37, null])("retains fenced exit status %s and clears it on rebinding", (exitCode) => {
    const session: TerminalSession = {
      id: "term-exit", cwd: "/repo", workspaceId: "ws-local", worktree: null,
      backendSessionId: "backend-exit", lifecycle: "running",
      incarnation: "inc-exit", daemonEpoch: "epoch-exit",
    };
    const state = createInitialState(session);
    const stale = workspaceReducer(state, {
      type: "SESSION_LIFECYCLE", backendSessionId: "backend-exit", lifecycle: "exited",
      incarnation: "old-incarnation", daemonEpoch: "epoch-exit", exitCode,
    });
    expect(stale.sessions[session.id].exitCode).toBeUndefined();
    expect(stale.sessions[session.id].backendSessionId).toBe("backend-exit");
    const ended = workspaceReducer(state, {
      type: "SESSION_LIFECYCLE", backendSessionId: "backend-exit", lifecycle: "exited",
      incarnation: "inc-exit", daemonEpoch: "epoch-exit", exitCode,
    });
    expect(ended.sessions[session.id].exitCode).toBe(exitCode);
    const rebound = workspaceReducer(ended, {
      type: "REBIND_SESSION_BACKEND", sessionId: session.id,
      backendSessionId: "backend-new", incarnation: "inc-new", daemonEpoch: "epoch-exit",
    });
    expect(rebound.sessions[session.id].exitCode).toBeNull();
    expect(rebound.sessions[session.id].backendSessionId).toBe("backend-new");
  });

  it("is a no-op when backendSessionId has already re-bound to a different ID", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-new-2",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-stale-1",
      reason: "daemon-attach-not-found",
    });

    expect(next).toBe(state);
    expect(next.sessions["term-1"].lifecycle).toBe("working");
    expect(next.sessions["term-1"].backendSessionId).toBe("backend-new-2");
  });

  it("marks remote workspace session disconnected with a failure", () => {
    const session: TerminalSession = {
      id: "term-remote",
      cwd: "/remote/path",
      workspaceId: "ssh:remote-host-1",
      worktree: { wsId: "ssh:remote-host-1", slug: "remote" },
      backendSessionId: "backend-remote-1",
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-remote",
      backendSessionId: "backend-remote-1",
      reason: "daemon-attach-not-found",
    });

    expect(next.sessions["term-remote"].lifecycle).toBe("working");
    expect(next.sessions["term-remote"].backendSessionId).toBe("backend-remote-1");
    expect(next.sessions["term-remote"].remoteConnectionState).toBe("disconnected");
    expect(next.sessions["term-remote"].remoteGeneration).toBeNull();
    expect(next.sessions["term-remote"].remoteFailure).toEqual({
      kind: "network",
      message: "daemon-attach-not-found",
    });
    expect(next.sessions["term-remote"].reconnectLifecycle ?? "idle").toBe("idle");
  });

  it("preserves active activity on unavailable owner (INV-1) and settles once on affirmative exit (INV-2)", () => {
    const session: TerminalSession = {
      id: "term-activity",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-act-1",
      lifecycle: "working",
      incarnation: "inc-1",
      daemonEpoch: "epoch-act-1",
      recoveryKey: "local:term-activity:backend-act-1:inc-1",
      recoveryToken: 10,
    };

    const state: WorkspaceState = {
      ...createInitialState(session),
      activityBySessionId: {
        "term-activity": {
          state: "working",
          title: "Running build",
          isAgent: false,
        },
      },
    };

    // 1. Unavailable owner: retains backendSessionId and activity (INV-1)
    const unavail = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-activity",
      backendSessionId: "backend-act-1",
      attemptKey: "local:term-activity:backend-act-1:inc-1",
      attemptToken: 10,
      reason: "daemon-attach-not-found",
    });

    expect(unavail.sessions["term-activity"].lifecycle).toBe("working");
    expect(unavail.sessions["term-activity"].backendSessionId).toBe("backend-act-1");
    expect(unavail.activityBySessionId?.["term-activity"]?.state).toBe("working");
    expect(unavail.sessions["term-activity"].recoveryState).toEqual({
      phase: "recovering",
      reason: "daemon-attach-not-found",
    });

    // 2. Affirmative exit: matching incarnation transitions to exited, clears ID, settles activity (INV-2)
    const exited = workspaceReducer(unavail, {
      type: "SESSION_AFFIRMATIVE_EXIT",
      sessionId: "term-activity",
      backendSessionId: "backend-act-1",
      incarnation: "inc-1",
      daemonEpoch: "epoch-act-1",
      token: 10,
      key: "local:term-activity:backend-act-1:inc-1",
      exitCode: 0,
    });

    expect(exited.sessions["term-activity"].lifecycle).toBe("exited");
    expect(exited.sessions["term-activity"].backendSessionId).toBeNull();
    expect(exited.sessions["term-activity"].recoveryState).toEqual({ phase: "exited", reason: null });
    expect(exited.activityBySessionId?.["term-activity"]?.state).toBe("done");

    const blocked = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-activity",
      backendSessionId: "backend-act-1",
      attemptKey: "local:term-activity:backend-act-1:inc-1",
      attemptToken: 10,
      outcome: "blocked",
      reason: "Authentication required",
    });
    expect(blocked.sessions["term-activity"].lifecycle).toBe("working");
    expect(blocked.sessions["term-activity"].backendSessionId).toBe("backend-act-1");
    expect(blocked.sessions["term-activity"].recoveryState).toEqual({
      phase: "blocked",
      reason: "Authentication required",
    });
  });

  it("is a no-op when bindingKey does not match the current session binding epoch", () => {
    const session: TerminalSession = {
      id: "term-1",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-1",
      daemonEpoch: "2",
      remoteGeneration: 0,
      lifecycle: "working",
    };

    const state = createInitialState(session);
    const next = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-1",
      backendSessionId: "backend-1",
      bindingKey: "backend-1:1:0:",
      reason: "daemon-attach-not-found",
    });

    expect(next).toBe(state);
    expect(next.sessions["term-1"].lifecycle).toBe("working");
    expect(next.sessions["term-1"].backendSessionId).toBe("backend-1");
  });

  it("fences unavailable dispatch by matching key and token under INV-7", () => {
    const session: TerminalSession = {
      id: "term-fence",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-fence-1",
      recoveryKey: "local:term-fence:backend-fence-1:inc-1",
      recoveryToken: 5,
      lifecycle: "working",
    };

    const state = createInitialState(session);

    // Stale token rejected
    const staleToken = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-fence",
      backendSessionId: "backend-fence-1",
      attemptKey: "local:term-fence:backend-fence-1:inc-1",
      attemptToken: 4, // stale!
      reason: "timeout",
    });
    expect(staleToken).toBe(state);

    // Mismatched key rejected
    const mismatchedKey = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-fence",
      backendSessionId: "backend-fence-1",
      attemptKey: "local:term-fence:other-key",
      attemptToken: 5,
      reason: "timeout",
    });
    expect(mismatchedKey).toBe(state);

    // Matching key and token accepted
    const matching = workspaceReducer(state, {
      type: "SESSION_BACKEND_UNAVAILABLE",
      sessionId: "term-fence",
      backendSessionId: "backend-fence-1",
      attemptKey: "local:term-fence:backend-fence-1:inc-1",
      attemptToken: 5,
      outcome: "blocked",
      reason: "identity_conflict",
    });
    expect(matching.sessions["term-fence"].backendSessionId).toBe("backend-fence-1");
    expect(matching.sessions["term-fence"].reconnectLifecycle).toBe("failed");
    expect(matching.sessions["term-fence"].backendUnavailableReason).toBe("identity_conflict");
    expect(matching.sessions["term-fence"].recoveryState).toEqual({ phase: "blocked", reason: "identity_conflict" });
  });

  it("safely discards stale lifecycle and exit receipts for differing incarnation under INV-5", () => {
    const session: TerminalSession = {
      id: "term-rebound",
      cwd: "/repo",
      workspaceId: "ws-local",
      worktree: { wsId: "ws-local", slug: "main" },
      backendSessionId: "backend-shared",
      incarnation: "incarnation-new-2",
      daemonEpoch: "epoch-rebound-1",
      recoveryToken: 1,
      recoveryKey: "local:term-rebound:backend-shared:incarnation-new-2",
      lifecycle: "working",
    };

    const state = createInitialState(session);

    // Stale lifecycle event from old incarnation-1 must not kill rebound pane
    const staleLifecycle = workspaceReducer(state, {
      type: "SESSION_LIFECYCLE",
      backendSessionId: "backend-shared",
      lifecycle: "exited",
      incarnation: "incarnation-old-1",
    });
    expect(staleLifecycle.sessions["term-rebound"].lifecycle).toBe("working");

    // Stale affirmative exit from old incarnation-1 must not kill rebound pane
    const staleAffirmative = workspaceReducer(state, {
      type: "SESSION_AFFIRMATIVE_EXIT",
      sessionId: "term-rebound",
      backendSessionId: "backend-shared",
      incarnation: "incarnation-old-1",
      daemonEpoch: "epoch-rebound-1",
      token: 1,
      key: "local:term-rebound:backend-shared:incarnation-old-1",
    });
    expect(staleAffirmative).toBe(state);
  });

  describe("reconcileSessionFromDescribe authoritative describe-to-exit consumer", () => {
    it("dispatches SESSION_AFFIRMATIVE_EXIT when describe returns complete matching owner proof (INV-2)", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-1",
          daemonEpoch: "epoch-1",
        },
      });

      const initialSession: TerminalSession = {
        id: "term-1",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-1",
        incarnation: "inc-1",
        daemonEpoch: "epoch-1",
        recoveryToken: 1,
        recoveryKey: "local:ws-local:backend-1:inc-1",
        lifecycle: "working",
      };
      let currentState = createInitialState(initialSession);
      const dispatched: any[] = [];
      const dispatchWrapper = (action: any) => {
        dispatched.push(action);
        currentState = workspaceReducer(currentState, action);
      };
      const outcome = await reconcileSessionFromDescribe(
        "term-1", "backend-1", "inc-1", dispatchWrapper, "epoch-1", 1, "local:ws-local:backend-1:inc-1",
        () => currentState.sessions["term-1"]
      );

      expect(outcome).toBe("exited");
      expect(dispatched).toHaveLength(1);
      expect(dispatched[0]).toEqual({
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-1",
        backendSessionId: "backend-1",
        incarnation: "inc-1",
        daemonEpoch: "epoch-1",
        token: 1,
        key: "local:ws-local:backend-1:inc-1",
        exitCode: 0,
        reason: null,
      });
      expect(currentState.sessions["term-1"].lifecycle).toBe("exited");
      expect(currentState.sessions["term-1"].backendSessionId).toBeNull();
      describeSpy.mockRestore();
    });

    it("returns uncertain for non-running standby session without affirmative exit status", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-1",
      });

      const dispatched: any[] = [];
      const outcome = await reconcileSessionFromDescribe("term-1", "backend-1", "inc-1", (a) => dispatched.push(a), "epoch-1", 1);

      expect(outcome).toBe("uncertain");
      expect(dispatched).toHaveLength(0);
      describeSpy.mockRestore();
    });

    it("returns uncertain when current owner context (epoch or token) is missing or mismatched", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-1",
          daemonEpoch: "epoch-1",
        },
      });

      const dispatched: any[] = [];
      const outcomeNoEpoch = await reconcileSessionFromDescribe("term-1", "backend-1", "inc-1", (a) => dispatched.push(a), null, 1);
      expect(outcomeNoEpoch).toBe("uncertain");

      const outcomeWrongEpoch = await reconcileSessionFromDescribe("term-1", "backend-1", "inc-1", (a) => dispatched.push(a), "epoch-2", 1);
      expect(outcomeWrongEpoch).toBe("uncertain");

      const outcomeNoToken = await reconcileSessionFromDescribe("term-1", "backend-1", "inc-1", (a) => dispatched.push(a), "epoch-1", null);
      expect(outcomeNoToken).toBe("uncertain");

      expect(dispatched).toHaveLength(0);
      describeSpy.mockRestore();
    });

    it("returns uncertain and does not dispatch exit for paired proxy actor running=false (INV-4)", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "paired-backend-1",
        cols: 80,
        rows: 24,
        running: false,
        isPairedProxy: true,
        incarnation: "proxy-inc-1",
        recoveryAuthority: {
          status: "owner_unavailable",
          isPairedProxy: true,
          incarnation: "proxy-inc-1",
        },
      });

      const dispatched: any[] = [];
      const outcome = await reconcileSessionFromDescribe("term-1", "paired-backend-1", "proxy-inc-1", (a) => dispatched.push(a), "epoch-1", 1);

      expect(outcome).toBe("uncertain");
      expect(dispatched).toHaveLength(0);
      describeSpy.mockRestore();
    });

    it("returns conflict and does not dispatch exit on mismatched incarnation (INV-3)", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: true,
        incarnation: "inc-2",
      });

      const dispatched: any[] = [];
      const outcome = await reconcileSessionFromDescribe("term-1", "backend-1", "inc-1", (a) => dispatched.push(a), "epoch-1", 1);

      expect(outcome).toBe("conflict");
      expect(dispatched).toHaveLength(0);
      describeSpy.mockRestore();
    });

    it("returns conflict and rejects describe exit when current session underwent late rebind", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-1",
          daemonEpoch: "epoch-1",
        },
      });

      const dispatched: any[] = [];
      // Current session rebound to a different backend ID during in-flight describe
      let currentSession: TerminalSession = {
        id: "term-1",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-rebound-new",
        incarnation: "inc-new",
        daemonEpoch: "epoch-2",
        lifecycle: "working",
      };

      const outcome = await reconcileSessionFromDescribe(
        "term-1",
        "backend-1",
        "inc-1",
        (a) => dispatched.push(a),
        "epoch-1",
        1,
        null,
        () => currentSession,
      );

      expect(outcome).toBe("conflict");
      expect(dispatched).toHaveLength(0);
      describeSpy.mockRestore();
    });

    it("returns conflict when pre-dispatch check detects owner token mismatch", async () => {
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce({
        sessionId: "backend-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-1",
          daemonEpoch: "epoch-1",
        },
      });

      let state = createInitialState({
        id: "term-1",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-1",
        incarnation: "inc-1",
        daemonEpoch: "epoch-1",
        recoveryToken: 99, // Mismatched recoveryToken on state
        lifecycle: "working",
      });

      const outcome = await reconcileSessionFromDescribe(
        "term-1",
        "backend-1",
        "inc-1",
        (action) => {
          state = workspaceReducer(state, action);
        },
        "epoch-1",
        1, // Passes token 1, which does not match token 99
        null,
        () => state.sessions["term-1"],
      );

      // Pre-dispatch check detects owner token mismatch (1 !== 99) and returns conflict
      expect(state.sessions["term-1"].lifecycle).toBe("working");
      expect(outcome).toBe("conflict");
      describeSpy.mockRestore();
    });
  });
});
