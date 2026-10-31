import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";

import { classifyNativeTerminalAttachError } from "./nativeTerminalAttachPolicy";
import {
  deserializeWorkspaceState,
  serializeWorkspaceState,
  type LiveSessionInventoryItem,
} from "./sessionPersistence";
import { formatRecoveryKey } from "./sessionRecovery";
import { saveBrowserSettings } from "./browserSettings";
import {
  workspaceReducer,
  reconcileSessionFromDescribe,
  setLocalRecoveryPersistence,
  useWorkspaceStore,
  type WorkspaceAction,
  type WorkspaceServices,
  type WorkspaceState,
} from "../state/workspaceStore";
import { clearWorkspaceSnapshot } from "../state/workspaceSnapshotCache";
import { createLayoutState } from "../state/layout";
import { terminalEventBus } from "./terminalEvents";
import type { TerminalActivity } from "./activity";
import type { TerminalLifecyclePayload, TerminalSession, TerminalTab, Worktree } from "./types";
import * as tauri from "./tauri";

const lifecycleListeners = vi.hoisted(() => new Set<(payload: TerminalLifecyclePayload) => void>());

vi.mock("./tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("./tauri")>(),
  onTerminalOutput: async () => () => undefined,
  onTerminalLifecycle: async (handler: (payload: TerminalLifecyclePayload) => void) => {
    lifecycleListeners.add(handler);
    return () => lifecycleListeners.delete(handler);
  },
  onNativeTerminalTitle: async () => () => undefined,
  onNativeTerminalBell: async () => () => undefined,
}));

beforeEach(async () => {
  cleanup();
  clearWorkspaceSnapshot();
  setLocalRecoveryPersistence(async () => {});
  await terminalEventBus.ensureStarted();
  const values = new Map<string, string>();
  const storage: Storage = {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => {
      values.delete(key);
    },
    setItem: (key, value) => {
      values.set(key, String(value));
    },
  };
  vi.spyOn(window, "localStorage", "get").mockReturnValue(storage);
  saveBrowserSettings({ restoreTabsOnLaunch: true });
});

afterEach(() => {
  cleanup();
  clearWorkspaceSnapshot();
  setLocalRecoveryPersistence(undefined);
  vi.restoreAllMocks();
});

function createTestState(session: TerminalSession, activity?: TerminalActivity): WorkspaceState {
  const tab: TerminalTab = {
    id: "tab-1",
    label: "Test Tab",
    sessionId: session.id,
  };
  const sessionActivity: TerminalActivity = activity ?? {
    state: "working",
    title: "Terminal",
    isAgent: false,
  };
  return {
    workspaceId: session.workspaceId,
    worktrees: [
      {
        path: "/repo",
        head: "head-sha",
        branch: "main",
        bare: false,
        detached: false,
        locked: null,
        prunable: null,
      },
    ],
    activeWorktreePath: "/repo",
    layout: createLayoutState([tab], "tab-1"),
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    sessions: {
      [session.id]: session,
    },
    activityBySessionId: {
      [session.id]: sessionActivity,
    },
  };
}

describe("sessionAuthority - independent adversarial verification (Task 2)", () => {
  // =========================================================================
  // Area 1: missing/structured scoped-not-found remains unverified
  // =========================================================================
  describe("Area 1: missing/structured scoped-not-found remains unverified", () => {
    it("classifies structured daemon_attach session_not_found as unverified-missing when affirmative exit proof is absent", () => {
      const error = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "sess-req-1",
        },
      };

      const result = classifyNativeTerminalAttachError(error, "sess-req-1");
      expect(result.status).toBe("unverified-missing");
    });

    it("classifies structured SESSION_NOT_FOUND with unclear origin as safe operational refusal", () => {
      const error = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "unrecognized_caller",
          sessionId: "sess-req-1",
        },
      };

      const result = classifyNativeTerminalAttachError(error, "sess-req-1");
      // Contradictory/unrecognized origin is safely refused as operational-error rather than treated as a missing daemon process
      expect(result.status).toBe("operational-error");
    });

    it("classifies contradictory metadata as operational-error rather than process death", () => {
      const error = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "different-session-id",
        },
      };

      const result = classifyNativeTerminalAttachError(error, "sess-req-1");
      expect(result.status).toBe("operational-error");
    });
  });

  // =========================================================================
  // Area 2: persistence uncertainty preserves working/awaiting activity and retained ID
  // =========================================================================
  describe("Area 2: persistence uncertainty preserves working/awaiting activity and retained ID", () => {
    it.each([
      { state: "working" as const, title: "Compiling binary", isAgent: false },
      { state: "waiting" as const, title: "Awaiting user input", isAgent: true },
    ])("preserves backendSessionId, processState, and $state activity on live inventory absence", (activity) => {
      const session: TerminalSession = {
        id: "sess-1",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-retained-1",
        daemonEpoch: "epoch-live-1",
        lastOutputSequence: "100",
        lifecycle: "working",
        processState: "standby",
      };

      const state = createTestState(session, activity);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const restored = deserializeWorkspaceState("default", saved, []);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-1"];

      // Identity and process state preserved
      expect(restoredSession.backendSessionId).toBe("backend-retained-1");
      expect(restoredSession.daemonEpoch).toBe("epoch-live-1");
      expect(restoredSession.lifecycle).toBe("working");
      expect(restoredSession.processState).toBe("standby");
      expect(restoredSession.remoteConnectionState).toBe("reconnecting");

      // Activity state preserved in activityBySessionId map
      expect(restored!.activityBySessionId?.[session.id]?.state).toBe(activity.state);
      expect(restored!.activityBySessionId?.[session.id]?.title).toBe(activity.title);
      expect(restored!.activityBySessionId?.[session.id]?.isAgent).toBe(activity.isAgent);
    });

    it("retains persisted working lifecycle when live session map excludes the backend ID", () => {
      const session: TerminalSession = {
        id: "sess-2",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-target-2",
        daemonEpoch: "epoch-1",
        lifecycle: "working",
        processState: "standby",
      };

      const activity: TerminalActivity = {
        state: "working",
        title: "Testing suite",
        isAgent: false,
      };

      const state = createTestState(session, activity);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const liveInventory = ["other-backend-session-99"];
      const restored = deserializeWorkspaceState("default", saved, liveInventory);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-2"];

      expect(restoredSession.backendSessionId).toBe("backend-target-2");
      expect(restoredSession.lifecycle).toBe("working");
      expect(restoredSession.processState).toBe("standby");
      expect(restoredSession.remoteConnectionState).toBe("reconnecting");
      expect(restored!.activityBySessionId?.[session.id]?.state).toBe("working");
    });

    it("rejects affirmative exit during persistence deserialization when epoch context is completely absent (fail-closed)", () => {
      const session: TerminalSession = {
        id: "sess-epochless-1",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-epochless-1",
        daemonEpoch: null,
        incarnation: "inc-epochless-1",
        lifecycle: "working",
        processState: "running",
      };

      const activity: TerminalActivity = {
        state: "working",
        title: "Active task",
        isAgent: true,
      };

      const state = createTestState(session, activity);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const liveSessions = [
        {
          sessionId: "backend-epochless-1",
          running: false,
          incarnation: "inc-epochless-1",
          affirmativeExit: true,
          recoveryAuthority: {
            status: "affirmative_exit",
            incarnation: "inc-epochless-1",
          },
        },
      ];

      const restored = deserializeWorkspaceState("default", saved, liveSessions as LiveSessionInventoryItem[]);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-epochless-1"];

      // Complete authority requires verified epoch context: missing epoch from both sides must NOT allow affirmative exit
      expect(restoredSession.lifecycle).not.toBe("exited");
      expect(restoredSession.backendSessionId).toBe("backend-epochless-1");
    });

    it("rejects affirmative exit during persistence deserialization when authority epoch differs from live/persisted epoch (persisted=live1 auth2)", () => {
      const session: TerminalSession = {
        id: "sess-mismatch-auth-1",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-mismatch-auth-1",
        daemonEpoch: "epoch-1",
        incarnation: "inc-mismatch-auth-1",
        lifecycle: "working",
        processState: "running",
      };

      const activity: TerminalActivity = {
        state: "working",
        title: "Active task",
        isAgent: true,
      };

      const state = createTestState(session, activity);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const liveSessions = [
        {
          sessionId: "backend-mismatch-auth-1",
          running: false,
          incarnation: "inc-mismatch-auth-1",
          daemonEpoch: "epoch-1",
          affirmativeExit: true,
          recoveryAuthority: {
            status: "affirmative_exit",
            incarnation: "inc-mismatch-auth-1",
            daemonEpoch: "epoch-2",
          },
        },
      ];

      const restored = deserializeWorkspaceState("default", saved, liveSessions as LiveSessionInventoryItem[]);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-mismatch-auth-1"];

      // Authority epoch mismatch must fail closed: do NOT settle to exited
      expect(restoredSession.lifecycle).not.toBe("exited");
      expect(restoredSession.backendSessionId).toBe("backend-mismatch-auth-1");
    });
  });

  // =========================================================================
  // Area 3: differing incarnation blocked/no adoption
  // =========================================================================
  describe("Area 3: differing incarnation blocked/no adoption", () => {
    it("flags identity conflict and blocks recovery without adopting different incarnation", () => {
      const session: TerminalSession = {
        id: "sess-3",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-shared-3",
        daemonEpoch: "epoch-1",
        incarnation: "incarnation-canonical-A",
        lifecycle: "working",
        processState: "standby",
      };

      const state = createTestState(session);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const liveInventory = [
        {
          sessionId: "backend-shared-3",
          daemonEpoch: "epoch-1",
          running: true,
          incarnation: "incarnation-conflicting-B",
        },
      ];

      const restored = deserializeWorkspaceState("default", saved, liveInventory);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-3"];

      expect(restoredSession.backendSessionId).toBe("backend-shared-3");
      expect(restoredSession.incarnation).toBe("incarnation-canonical-A");
      expect(restoredSession.reconnectLifecycle).toBe("failed");
      expect(restoredSession.backendUnavailableReason).toBe("identity_conflict");
      expect(restoredSession.reconnectError?.code).toBe("IDENTITY_CONFLICT");
      expect(restoredSession.remoteConnectionState).toBe("disconnected");
    });
  });

  // =========================================================================
  // Area 4: legacy missing metadata unconfirmed
  // =========================================================================
  describe("Area 4: legacy missing metadata unconfirmed", () => {
    it("classifies legacy INTERNAL_ERROR string messages as unverified-missing rather than death", () => {
      const err1 = {
        code: "INTERNAL_ERROR",
        message: "Session 'sess-target-4' not found",
      };
      const res1 = classifyNativeTerminalAttachError(err1, "sess-target-4");
      expect(res1.status).toBe("unverified-missing");

      const err2 = {
        code: "INTERNAL_ERROR",
        message: "PTY session 'sess-target-4' not found",
      };
      const res2 = classifyNativeTerminalAttachError(err2, "sess-target-4");
      expect(res2.status).toBe("unverified-missing");
    });

    it("does not adopt live session under changed daemon epoch when incarnation metadata is missing", () => {
      const session: TerminalSession = {
        id: "sess-4",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-epoch-change",
        daemonEpoch: "epoch-old-1",
        lifecycle: "working",
        processState: "standby",
      };

      const state = createTestState(session);
      const saved = serializeWorkspaceState("default", "/repo", state);

      const liveInventory = [
        {
          sessionId: "backend-epoch-change",
          daemonEpoch: "epoch-new-2",
          running: true,
        },
      ];

      const restored = deserializeWorkspaceState("default", saved, liveInventory);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-4"];

      expect(restoredSession.backendSessionId).toBe("backend-epoch-change");
      expect(restoredSession.remoteConnectionState).toBe("reconnecting");
    });

    it("retains working lifecycle and backendSessionId when live inventory reports running=false with matching incarnation but no affirmative exit authority", () => {
      const session: TerminalSession = {
        id: "sess-standby",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-standby-1",
        incarnation: "inc-standby-1",
        daemonEpoch: "epoch-1",
        lifecycle: "working",
        processState: "standby",
      };

      const state = createTestState(session);
      const saved = serializeWorkspaceState("default", "/repo", state);

      // Live inventory reports running=false with matching incarnation, but NO affirmative exit authority:
      // must NOT falsely settle to exited/dead (INV-1, INV-2).
      const liveInventory = [
        {
          sessionId: "backend-standby-1",
          daemonEpoch: "epoch-1",
          running: false,
          incarnation: "inc-standby-1",
          // recoveryAuthority absent or unconfirmed
        },
      ];

      const restored = deserializeWorkspaceState("default", saved, liveInventory);
      expect(restored).not.toBeNull();
      const restoredSession = restored!.sessions["sess-standby"];

      expect(restoredSession.backendSessionId).toBe("backend-standby-1");
      expect(restoredSession.lifecycle).toBe("working");
      expect(restoredSession.remoteConnectionState).toBe("reconnecting");
      expect(restored!.activityBySessionId?.[session.id]?.state).toBe("working");
    });
  });

  // =========================================================================
  // Area 5: reducer missing/stale key/token cannot settle/rebind
  // =========================================================================
  describe("Area 5: reducer missing/stale key/token cannot settle/rebind", () => {
    it("refuses to settle pane on SESSION_LIFECYCLE exited event when incarnation context is absent or mismatched", () => {
      const session: TerminalSession = {
        id: "term-1",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-live-5",
        incarnation: "incarnation-active-5",
        lifecycle: "working",
      };

      const state = createTestState(session);

      const actionMissingIncarnation: WorkspaceAction = {
        type: "SESSION_LIFECYCLE",
        backendSessionId: "backend-live-5",
        lifecycle: "exited",
      };
      const stateWithoutIncarnation = workspaceReducer(state, actionMissingIncarnation);

      expect(stateWithoutIncarnation.sessions["term-1"].lifecycle).toBe("working");
      expect(stateWithoutIncarnation.sessions["term-1"].backendSessionId).toBe("backend-live-5");
      expect(stateWithoutIncarnation.sessions["term-1"].remoteConnectionState).toBe("reconnecting");

      const actionMismatchedIncarnation: WorkspaceAction = {
        type: "SESSION_LIFECYCLE",
        backendSessionId: "backend-live-5",
        lifecycle: "exited",
        incarnation: "incarnation-foreign-9",
      };
      const stateWithMismatchedIncarnation = workspaceReducer(state, actionMismatchedIncarnation);

      expect(stateWithMismatchedIncarnation.sessions["term-1"].lifecycle).toBe("working");
      expect(stateWithMismatchedIncarnation.sessions["term-1"].backendSessionId).toBe("backend-live-5");
      expect(stateWithMismatchedIncarnation.sessions["term-1"].remoteConnectionState).toBe("reconnecting");

      // Matching incarnation but action or current session lacks proven daemonEpoch: must NOT kill pane
      const actionMissingEpoch: WorkspaceAction = {
        type: "SESSION_LIFECYCLE",
        backendSessionId: "backend-live-5",
        lifecycle: "exited",
        incarnation: "incarnation-active-5",
        // daemonEpoch omitted
      };
      const stateMissingEpoch = workspaceReducer(state, actionMissingEpoch);
      expect(stateMissingEpoch.sessions["term-1"].lifecycle).toBe("working");
      expect(stateMissingEpoch.sessions["term-1"].backendSessionId).toBe("backend-live-5");
      expect(stateMissingEpoch.sessions["term-1"].remoteConnectionState).toBe("reconnecting");

      // Missing-CURRENT epoch case: CURRENT session has daemonEpoch = null, while incoming event epoch
      // is populated and matching incarnation: must NOT kill pane, must preserve backendSessionId and transition to reconnecting
      const sessionNullCurrentEpoch: TerminalSession = {
        id: "term-1-null-epoch",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-live-5-null-epoch",
        incarnation: "incarnation-active-5",
        daemonEpoch: null,
        lifecycle: "working",
      };
      const stateNullCurrentEpoch = createTestState(sessionNullCurrentEpoch);

      const actionIncomingEpochPopulated: WorkspaceAction = {
        type: "SESSION_LIFECYCLE",
        backendSessionId: "backend-live-5-null-epoch",
        lifecycle: "exited",
        incarnation: "incarnation-active-5",
        daemonEpoch: "epoch-live-5-incoming",
      };
      const stateAfterNullCurrentEpoch = workspaceReducer(stateNullCurrentEpoch, actionIncomingEpochPopulated);
      expect(stateAfterNullCurrentEpoch.sessions["term-1-null-epoch"].lifecycle).toBe("working");
      expect(stateAfterNullCurrentEpoch.sessions["term-1-null-epoch"].backendSessionId).toBe("backend-live-5-null-epoch");
      expect(stateAfterNullCurrentEpoch.sessions["term-1-null-epoch"].remoteConnectionState).toBe("reconnecting");
    });

    it("rejects SESSION_AFFIRMATIVE_EXIT when recoveryToken or daemonEpoch is stale, mismatched, or absent", () => {
      const session: TerminalSession = {
        id: "term-2",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        daemonEpoch: "epoch-5b",
        recoveryToken: 10,
        lifecycle: "working",
      };

      const state = createTestState(session);

      const actionStaleToken: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2",
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        daemonEpoch: "epoch-5b",
        token: 9,
      };
      expect(workspaceReducer(state, actionStaleToken)).toBe(state);

      const actionAbsentToken: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2",
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        daemonEpoch: "epoch-5b",
      };
      expect(workspaceReducer(state, actionAbsentToken)).toBe(state);

      const actionMismatchedEpoch: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2",
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        daemonEpoch: "epoch-other",
        token: 10,
      };
      expect(workspaceReducer(state, actionMismatchedEpoch)).toBe(state);

      const actionAbsentIncarnation: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2",
        backendSessionId: "backend-target-5b",
        daemonEpoch: "epoch-5b",
        token: 10,
      };
      expect(workspaceReducer(state, actionAbsentIncarnation)).toBe(state);

      const actionAbsentActionEpoch: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2",
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        token: 10,
      };
      expect(workspaceReducer(state, actionAbsentActionEpoch)).toBe(state);

      // Missing-CURRENT epoch case: CURRENT session has daemonEpoch = null, while incoming affirmative exit carries populated daemonEpoch
      const sessionNullCurrentEpochExit: TerminalSession = {
        ...session,
        id: "term-2-null-epoch",
        daemonEpoch: null,
      };
      const stateNullCurrentEpochExit = createTestState(sessionNullCurrentEpochExit);

      const actionIncomingEpochForNullCurrentExit: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-2-null-epoch",
        backendSessionId: "backend-target-5b",
        incarnation: "inc-5b",
        daemonEpoch: "epoch-5b",
        token: 10,
      };
      expect(workspaceReducer(stateNullCurrentEpochExit, actionIncomingEpochForNullCurrentExit)).toBe(stateNullCurrentEpochExit);
    });

    it("rejects SESSION_BACKEND_UNAVAILABLE attempt outcome when attemptKey or attemptToken is stale, mismatched, or absent", () => {
      const session: TerminalSession = {
        id: "term-3",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-5c",
        recoveryKey: '["local","term-3","backend-target-5c",null]',
        recoveryToken: 20,
        lifecycle: "working",
      };

      const state = createTestState(session);

      const actionMismatchedKey: WorkspaceAction = {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId: "term-3",
        backendSessionId: "backend-target-5c",
        attemptKey: '["local","term-3","other-backend",null]',
        attemptToken: 20,
        reason: "daemon-attach-not-found",
      };
      expect(workspaceReducer(state, actionMismatchedKey)).toBe(state);

      const actionStaleToken: WorkspaceAction = {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId: "term-3",
        backendSessionId: "backend-target-5c",
        attemptKey: '["local","term-3","backend-target-5c",null]',
        attemptToken: 19,
        reason: "daemon-attach-not-found",
      };
      expect(workspaceReducer(state, actionStaleToken)).toBe(state);

      const actionAbsentToken: WorkspaceAction = {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId: "term-3",
        backendSessionId: "backend-target-5c",
        attemptKey: '["local","term-3","backend-target-5c",null]',
        reason: "daemon-attach-not-found",
      };
      expect(workspaceReducer(state, actionAbsentToken)).toBe(state);

      const actionAbsentKey: WorkspaceAction = {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId: "term-3",
        backendSessionId: "backend-target-5c",
        attemptToken: 20,
        reason: "daemon-attach-not-found",
      };
      expect(workspaceReducer(state, actionAbsentKey)).toBe(state);

      // Absent current recovery owner on session: attempt outcome cannot be validated against owner context
      const uninstrumentedSession: TerminalSession = {
        id: "term-uninstrumented",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-uninstrumented",
        lifecycle: "working",
      };
      const uninstrumentedState = createTestState(uninstrumentedSession);

      const actionAgainstUninstrumented: WorkspaceAction = {
        type: "SESSION_BACKEND_UNAVAILABLE",
        sessionId: "term-uninstrumented",
        backendSessionId: "backend-target-uninstrumented",
        attemptKey: '["local","term-uninstrumented","backend-target-uninstrumented",null]',
        attemptToken: 1,
        reason: "daemon-attach-not-found",
      };
      expect(workspaceReducer(uninstrumentedState, actionAgainstUninstrumented)).toBe(uninstrumentedState);
    });
  });

  // =========================================================================
  // Area 6: matching affirmative exit settles once
  // =========================================================================
  describe("Area 6: matching affirmative exit settles once", () => {
    it("treats bare affirmativeExit as unverified when expected incarnation context is absent, omitted, or mismatched", () => {
      const bareErrorWithoutContext = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "sess-6",
          affirmativeExit: true,
        },
      };

      // Negative case A: Two-argument call without third argument (omitted expectedIncarnation context).
      // Bare boolean affirmativeExit without consumer-validated expected identity context must NOT confirm missing.
      const resBareTwoArg = classifyNativeTerminalAttachError(bareErrorWithoutContext, "sess-6");
      expect(resBareTwoArg.status).toBe("unverified-missing");

      const errorWithObservedIncarnationNoExpected = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "sess-6",
          incarnation: "inc-observed-without-expected",
          affirmativeExit: true,
        },
      };

      // Negative case B: Error details carries an observed incarnation, but caller provides no expected context.
      const resObservedNoExpected = classifyNativeTerminalAttachError(errorWithObservedIncarnationNoExpected, "sess-6");
      expect(resObservedNoExpected.status).toBe("unverified-missing");

      // Negative case C: Caller provides expectedIncarnation, but error details omits incarnation entirely.
      const resA = classifyNativeTerminalAttachError(bareErrorWithoutContext, "sess-6", "inc-expected-6");
      expect(resA.status).toBe("unverified-missing");

      // Negative case D: Caller provides expectedIncarnation, but error details carries conflicting incarnation.
      const errorWithConflictingIncarnation = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "sess-6",
          incarnation: "inc-conflicting-other",
          affirmativeExit: true,
        },
      };

      const resB = classifyNativeTerminalAttachError(errorWithConflictingIncarnation, "sess-6", "inc-expected-6");
      expect(resB.status).toBe("unverified-missing");
    });

    it("confirms missing on structured error bearing affirmativeExit proof with exact matching expected incarnation", () => {
      const error = {
        code: "SESSION_NOT_FOUND",
        message: "Session not found",
        details: {
          source: "daemon_attach",
          kind: "session_not_found",
          sessionId: "sess-6",
          incarnation: "inc-expected-6",
          affirmativeExit: true,
        },
      };

      const res = classifyNativeTerminalAttachError(error, "sess-6", "inc-expected-6");
      expect(res.status).toBe("confirmed-missing");
      if (res.status === "confirmed-missing") {
        expect(res.sessionId).toBe("sess-6");
      }
    });

    it("rejects SESSION_AFFIRMATIVE_EXIT when token matches but owner target, daemonEpoch, or incarnation differs", () => {
      const session: TerminalSession = {
        id: "term-6b",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-6b",
        incarnation: "inc-6b",
        daemonEpoch: "epoch-6b",
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-6b",
          backendSessionId: "backend-target-6b",
          expectedIncarnation: "inc-6b",
        }),
        recoveryToken: 40,
        lifecycle: "working",
      };

      const state = createTestState(session);

      // Same token (40) and matching incarnation, but mismatched daemonEpoch: rejected
      const actionMismatchedEpoch: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-6b",
        backendSessionId: "backend-target-6b",
        incarnation: "inc-6b",
        daemonEpoch: "epoch-foreign-99",
        token: 40,
        reason: "process-exited-normally",
      };
      expect(workspaceReducer(state, actionMismatchedEpoch)).toBe(state);

      // Same token (40) and matching incarnation, but mismatched backendSessionId (distinct owner target): rejected
      const actionMismatchedTarget: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-6b",
        backendSessionId: "backend-other-target",
        incarnation: "inc-6b",
        daemonEpoch: "epoch-6b",
        token: 40,
        reason: "process-exited-normally",
      };
      expect(workspaceReducer(state, actionMismatchedTarget)).toBe(state);

      // Same token (40) and matching daemonEpoch, but distinct incarnation context (inc-7 instead of inc-6b): rejected
      const actionDistinctIncarnation: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-6b",
        backendSessionId: "backend-target-6b",
        incarnation: "inc-7",
        daemonEpoch: "epoch-6b",
        token: 40,
        reason: "process-exited-normally",
      };
      expect(workspaceReducer(state, actionDistinctIncarnation)).toBe(state);

      // Session lacks current recoveryKey: affirmative exit cannot be admitted without proven complete owner context
      const sessionLackingKey: TerminalSession = {
        ...session,
        recoveryKey: undefined,
      };
      const stateLackingKey = createTestState(sessionLackingKey);
      const actionValidKey: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-6b",
        backendSessionId: "backend-target-6b",
        incarnation: "inc-6b",
        daemonEpoch: "epoch-6b",
        token: 40,
        key: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-6b",
          backendSessionId: "backend-target-6b",
          expectedIncarnation: "inc-6b",
        }),
        reason: "process-exited-normally",
      };
      expect(workspaceReducer(stateLackingKey, actionValidKey)).toBe(stateLackingKey);
    });

    it("settles pane once on matching affirmative exit with complete current owner context and remains idempotent", () => {
      // Complete current owner context includes valid canonical recoveryKey, recoveryToken, daemonEpoch, and incarnation
      const session: TerminalSession = {
        id: "term-6",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-6",
        incarnation: "inc-6",
        daemonEpoch: "epoch-6",
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-6",
          backendSessionId: "backend-target-6",
          expectedIncarnation: "inc-6",
        }),
        recoveryToken: 30,
        lifecycle: "working",
      };

      const state = createTestState(session);
      expect(state.activityBySessionId?.["term-6"]?.state).toBe("working");

      const exitAction: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-6",
        backendSessionId: "backend-target-6",
        incarnation: "inc-6",
        daemonEpoch: "epoch-6",
        token: 30,
        key: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-6",
          backendSessionId: "backend-target-6",
          expectedIncarnation: "inc-6",
        }),
        reason: "process-exited-normally",
      };

      const settledState = workspaceReducer(state, exitAction);

      expect(settledState.sessions["term-6"].lifecycle).toBe("exited");
      expect(settledState.sessions["term-6"].backendSessionId).toBeNull();
      expect(settledState.sessions["term-6"].reconnectLifecycle).toBe("idle");
      expect(settledState.activityBySessionId?.["term-6"]?.state).toBe("done");

      const duplicateState = workspaceReducer(settledState, exitAction);
      expect(duplicateState).toBe(settledState);
    });

    it("returns uncertain and does not dispatch exit when describe returns running=false with matching incarnation but status='unconfirmed'", async () => {
      const descriptor: tauri.TerminalDescribeResult = {
        sessionId: "backend-consumer-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-consumer-1",
        daemonEpoch: "epoch-1",
        recoveryAuthority: {
          status: "unconfirmed",
          incarnation: "inc-consumer-1",
          daemonEpoch: "epoch-1",
        },
      };

      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce(descriptor);

      const dispatched: WorkspaceAction[] = [];
      const outcome = await reconcileSessionFromDescribe(
        "term-consumer-1",
        "backend-consumer-1",
        "inc-consumer-1",
        (action: WorkspaceAction) => {
          dispatched.push(action);
        },
        "epoch-1",
        10,
      );

      expect(outcome).toBe("uncertain");
      expect(dispatched).toHaveLength(0);

      describeSpy.mockRestore();
    });

    it("does not report exited when describe returns affirmative_exit with matching epoch and incarnation but no captured owner token", async () => {
      const descriptor: tauri.TerminalDescribeResult = {
        sessionId: "backend-consumer-2",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-consumer-2",
        daemonEpoch: "epoch-2",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-consumer-2",
          daemonEpoch: "epoch-2",
        },
      };

      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce(descriptor);

      const session: TerminalSession = {
        id: "term-consumer-2",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-consumer-2",
        incarnation: "inc-consumer-2",
        daemonEpoch: "epoch-2",
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-consumer-2",
          backendSessionId: "backend-consumer-2",
          expectedIncarnation: "inc-consumer-2",
        }),
        recoveryToken: 20,
        lifecycle: "working",
      };
      let state = createTestState(session);

      const dispatched: WorkspaceAction[] = [];
      const outcome = await reconcileSessionFromDescribe(
        "term-consumer-2",
        "backend-consumer-2",
        "inc-consumer-2",
        (action: WorkspaceAction) => {
          dispatched.push(action);
          state = workspaceReducer(state, action);
        },
        "epoch-2",
        null, // No captured owner token provided
      );

      expect(outcome).not.toBe("exited");
      expect(outcome).toBe("uncertain");
      expect(dispatched).toHaveLength(0);

      expect(state.sessions["term-consumer-2"].lifecycle).toBe("working");
      expect(state.sessions["term-consumer-2"].backendSessionId).toBe("backend-consumer-2");

      describeSpy.mockRestore();
    });

    it("rejects SESSION_AFFIRMATIVE_EXIT when CURRENT session lacks recoveryKey even if action key is absent (both absent)", () => {
      const sessionNoKey: TerminalSession = {
        id: "term-no-key",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-target-nk",
        incarnation: "inc-nk",
        daemonEpoch: "epoch-nk",
        recoveryToken: 50,
        recoveryKey: undefined,
        lifecycle: "working",
      };
      const stateNoKey = createTestState(sessionNoKey);

      const actionNoKey: WorkspaceAction = {
        type: "SESSION_AFFIRMATIVE_EXIT",
        sessionId: "term-no-key",
        backendSessionId: "backend-target-nk",
        incarnation: "inc-nk",
        daemonEpoch: "epoch-nk",
        token: 50,
        reason: "process-exited-normally",
      };

      // Normative invariant: an uninstrumented session lacking a proven recoveryKey must NOT retire on affirmative exit
      expect(workspaceReducer(stateNoKey, actionNoKey)).toBe(stateNoKey);
    });

    it("returns conflict and aborts affirmative exit when session undergoes late rebind before describe resolves", async () => {
      let currentSession: TerminalSession | undefined = {
        id: "term-rebind-1",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-old-1",
        incarnation: "inc-old-1",
        daemonEpoch: "epoch-1",
        recoveryToken: 15,
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-rebind-1",
          backendSessionId: "backend-old-1",
          expectedIncarnation: "inc-old-1",
        }),
        lifecycle: "working",
      };

      const descriptor: tauri.TerminalDescribeResult = {
        sessionId: "backend-old-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-old-1",
        daemonEpoch: "epoch-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-old-1",
          daemonEpoch: "epoch-1",
        },
      };

      let resolveDescribe!: (res: tauri.TerminalDescribeResult) => void;
      const describePromise = new Promise<tauri.TerminalDescribeResult>((resolve) => {
        resolveDescribe = resolve;
      });
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockImplementationOnce(() => describePromise);

      const dispatched: WorkspaceAction[] = [];
      const reconcilePromise = reconcileSessionFromDescribe(
        "term-rebind-1",
        "backend-old-1",
        "inc-old-1",
        (action: WorkspaceAction) => {
          dispatched.push(action);
        },
        "epoch-1",
        15,
        currentSession.recoveryKey,
        () => currentSession,
      );

      // Concurrent late rebind during in-flight describe: backendSessionId changes to backend-new-2
      currentSession = {
        ...currentSession,
        backendSessionId: "backend-new-2",
        incarnation: "inc-new-2",
        recoveryToken: 16,
      };

      resolveDescribe(descriptor);
      const outcome = await reconcilePromise;

      expect(outcome).toBe("conflict");
      expect(dispatched).toHaveLength(0);
      expect(currentSession.lifecycle).toBe("working");

      describeSpy.mockRestore();
    });

    it("returns uncertain when post-await getCurrentSession returns undefined (concurrent close during describe)", async () => {
      let currentSession: TerminalSession | undefined = {
        id: "term-concurrent-close",
        cwd: "/repo",
        workspaceId: "ws-local",
        worktree: { wsId: "ws-local", slug: "main" },
        backendSessionId: "backend-close-1",
        incarnation: "inc-close-1",
        daemonEpoch: "epoch-1",
        recoveryToken: 18,
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-concurrent-close",
          backendSessionId: "backend-close-1",
          expectedIncarnation: "inc-close-1",
        }),
        lifecycle: "working",
      };

      const descriptor: tauri.TerminalDescribeResult = {
        sessionId: "backend-close-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-close-1",
        daemonEpoch: "epoch-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-close-1",
          daemonEpoch: "epoch-1",
        },
      };

      let resolveDescribe!: (res: tauri.TerminalDescribeResult) => void;
      const describePromise = new Promise<tauri.TerminalDescribeResult>((resolve) => {
        resolveDescribe = resolve;
      });
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockImplementationOnce(() => describePromise);

      const dispatched: WorkspaceAction[] = [];
      const reconcilePromise = reconcileSessionFromDescribe(
        "term-concurrent-close",
        "backend-close-1",
        "inc-close-1",
        (action: WorkspaceAction) => {
          dispatched.push(action);
        },
        "epoch-1",
        18,
        currentSession.recoveryKey,
        () => currentSession,
      );

      // Concurrent session closure/removal during in-flight describe:
      currentSession = undefined;

      resolveDescribe(descriptor);
      const outcome = await reconcilePromise;

      expect(outcome).toBe("uncertain");
      expect(dispatched).toHaveLength(0);

      describeSpy.mockRestore();
    });

    it("reconcileSessionFromDescribe with synchronous dispatch wrapper settles exit once and verifies post-dispatch state", async () => {
      let state = createTestState({
        id: "term-sync-dispatch",
        cwd: "/repo",
        worktreePath: "/repo",
        workspaceId: "default",
        worktree: null,
        backendSessionId: "backend-sync-1",
        daemonEpoch: "epoch-sync-1",
        incarnation: "inc-sync-1",
        recoveryToken: 25,
        recoveryKey: formatRecoveryKey({
          kind: "local",
          frontendSessionId: "term-sync-dispatch",
          backendSessionId: "backend-sync-1",
          expectedIncarnation: "inc-sync-1",
        }),
        lifecycle: "working",
      });

      // Synchronous dispatch wrapper modeling workspaceStore.ts:431-443:
      // updates state synchronously via workspaceReducer before any caller reads current
      const dispatch = (action: WorkspaceAction) => {
        state = workspaceReducer(state, action);
      };

      const descriptor: tauri.TerminalDescribeResult = {
        sessionId: "backend-sync-1",
        cols: 80,
        rows: 24,
        running: false,
        incarnation: "inc-sync-1",
        daemonEpoch: "epoch-sync-1",
        recoveryAuthority: {
          status: "affirmative_exit",
          exitCode: 0,
          incarnation: "inc-sync-1",
          daemonEpoch: "epoch-sync-1",
        },
      };

      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValueOnce(descriptor);

      const outcome = await reconcileSessionFromDescribe(
        "term-sync-dispatch",
        "backend-sync-1",
        "inc-sync-1",
        dispatch,
        "epoch-sync-1",
        25,
        state.sessions["term-sync-dispatch"].recoveryKey,
        () => state.sessions["term-sync-dispatch"],
      );

      expect(outcome).toBe("exited");
      expect(state.sessions["term-sync-dispatch"].lifecycle).toBe("exited");
      expect(state.sessions["term-sync-dispatch"].backendSessionId).toBeNull();
      expect(state.activityBySessionId?.["term-sync-dispatch"]?.state).toBe("done");

      describeSpy.mockRestore();
    });
  });

  // =========================================================================
  // Area 7: mounted hook actual exit admission and late-close/rebind observation
  // =========================================================================
  describe("Area 7: mounted hook actual exit admission and late-close/rebind observation", () => {
    const testWorktree: Worktree = {
      path: "/repo/test",
      head: "head-1",
      branch: "main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    function testServices(backendSessionId: string): WorkspaceServices {
      return {
        ensureTerminalEvents: async () => { await terminalEventBus.ensureStarted(); },
        spawnTerminal: async () => backendSessionId,
        getTerminalCwd: async () => testWorktree.path,
        closeTerminal: async () => undefined,
        waitForTerminalExit: async () => undefined,
      };
    }

    it("observes actual exit transition in mounted hook state when describe returns matching affirmative exit", async () => {
      let signalDescribeStarted!: () => void;
      const describeStarted = new Promise<void>((resolve) => {
        signalDescribeStarted = resolve;
      });

      let resolveDescribe!: (res: tauri.TerminalDescribeResult) => void;
      const describePromise = new Promise<tauri.TerminalDescribeResult>((resolve) => {
        resolveDescribe = resolve;
      });
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockImplementation(async () => {
        signalDescribeStarted();
        return describePromise;
      });

      const services = testServices("backend-mounted-1");
      const hook = renderHook(
        () => useWorkspaceStore({
          workspaceId: "ws-mounted-active",
          initialWorktrees: [testWorktree],
          services,
        }),
      );

      try {
        await act(async () => { await terminalEventBus.ensureStarted(); });

        await act(async () => { void (await hook.result.current.openTab(testWorktree)); });
        const openedTab = hook.result.current.state.layout.tabs[0];
        if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
        const sessionId = openedTab.sessionId;

        act(() => hook.result.current.dispatchWorkspaceAction({
          type: "REBIND_SESSION_BACKEND",
          sessionId,
          backendSessionId: "backend-mounted-1",
          daemonEpoch: "epoch-mounted-1",
          incarnation: "inc-mounted-1",
          recoveryToken: 1,
          recoveryKey: formatRecoveryKey({
            kind: "local",
            frontendSessionId: sessionId,
            backendSessionId: "backend-mounted-1",
            expectedIncarnation: "inc-mounted-1",
          }),
        }));

        act(() => hook.result.current.dispatchWorkspaceAction({
          type: "SESSION_SCREEN_ACTIVITY",
          tabId: openedTab.id,
          sessionId,
          state: "working",
          ruleId: "extension",
          manifestId: "omo",
        }));

        expect(hook.result.current.state.sessions[sessionId]?.lifecycle).toBe("running");
        expect(hook.result.current.state.activityBySessionId?.[sessionId]?.state).toBe("working");

        // Fire unconfirmed lifecycle event (missing daemonEpoch/incarnation) through mock IPC listener
        act(() => {
          for (const listener of lifecycleListeners) {
            listener({
              sessionId: "backend-mounted-1",
              state: "exited",
              exitCode: 0,
              reason: null,
            });
          }
        });

        // Await exact deferred signal that describeTerminal was entered and is actively in-flight
        await act(async () => {
          await describeStarted;
        });

        // Resolve describe with matching affirmative exit and flush React updates
        await act(async () => {
          resolveDescribe({
            sessionId: "backend-mounted-1",
            cols: 80,
            rows: 24,
            running: false,
            incarnation: "inc-mounted-1",
            daemonEpoch: "epoch-mounted-1",
            recoveryAuthority: {
              status: "affirmative_exit",
              exitCode: 0,
              incarnation: "inc-mounted-1",
              daemonEpoch: "epoch-mounted-1",
            },
          });
        });

        // Assert actual mounted state transition directly after act async flush without polling
        expect(hook.result.current.state.sessions[sessionId]?.lifecycle).toBe("exited");
        expect(hook.result.current.state.sessions[sessionId]?.backendSessionId).toBeNull();
        expect(hook.result.current.state.activityBySessionId?.[sessionId]?.state).toBe("done");
      } finally {
        describeSpy.mockRestore();
        hook.unmount();
      }
    });

    it("observes that late rebind while describe is in flight aborts exit without corrupting rebound session state", async () => {
      let signalDescribeStarted!: () => void;
      const describeStarted = new Promise<void>((resolve) => {
        signalDescribeStarted = resolve;
      });

      let resolveDescribe!: (res: tauri.TerminalDescribeResult) => void;
      const describePromise = new Promise<tauri.TerminalDescribeResult>((resolve) => {
        resolveDescribe = resolve;
      });
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockImplementation(async () => {
        signalDescribeStarted();
        return describePromise;
      });

      const services = testServices("backend-old-1");
      const hook = renderHook(
        () => useWorkspaceStore({
          workspaceId: "ws-mounted-rebind",
          initialWorktrees: [testWorktree],
          services,
        }),
      );

      try {
        await act(async () => { await terminalEventBus.ensureStarted(); });

        await act(async () => { void (await hook.result.current.openTab(testWorktree)); });
        const openedTab = hook.result.current.state.layout.tabs[0];
        if (openedTab.kind === "browser" || openedTab.kind === "file" || openedTab.kind === "changes") throw new Error("expected a terminal tab");
        const sessionId = openedTab.sessionId;

        const oldKey = formatRecoveryKey({
          kind: "local",
          frontendSessionId: sessionId,
          backendSessionId: "backend-old-1",
          expectedIncarnation: "inc-old-1",
        });

        act(() => hook.result.current.dispatchWorkspaceAction({
          type: "REBIND_SESSION_BACKEND",
          sessionId,
          backendSessionId: "backend-old-1",
          daemonEpoch: "epoch-1",
          incarnation: "inc-old-1",
          recoveryToken: 1,
          recoveryKey: oldKey,
        }));

        // Trigger describe on owning returned promise seam with mounted store state accessor
        const reconcilePromise = reconcileSessionFromDescribe(
          sessionId,
          "backend-old-1",
          "inc-old-1",
          hook.result.current.dispatchWorkspaceAction,
          "epoch-1",
          1,
          oldKey,
          () => hook.result.current.state.sessions[sessionId],
        );

        // AWAIT exact deferred signal proving describeTerminal was entered and is in-flight before REBIND
        await act(async () => {
          await describeStarted;
        });

        // While describeTerminal is in flight, the session undergoes a late rebind (new backend, token, key)
        act(() => hook.result.current.dispatchWorkspaceAction({
          type: "REBIND_SESSION_BACKEND",
          sessionId,
          backendSessionId: "backend-new-2",
          daemonEpoch: "epoch-2",
          incarnation: "inc-new-2",
          recoveryToken: 2,
          recoveryKey: formatRecoveryKey({
            kind: "local",
            frontendSessionId: sessionId,
            backendSessionId: "backend-new-2",
            expectedIncarnation: "inc-new-2",
          }),
        }));

        // Now resolve the old describeTerminal promise with affirmative exit for the OLD backend
        await act(async () => {
          resolveDescribe({
            sessionId: "backend-old-1",
            cols: 80,
            rows: 24,
            running: false,
            incarnation: "inc-old-1",
            daemonEpoch: "epoch-1",
            recoveryAuthority: {
              status: "affirmative_exit",
              exitCode: 0,
              incarnation: "inc-old-1",
              daemonEpoch: "epoch-1",
            },
          });
        });

        // Await owning returned promise seam directly: proves consumer completed and returned conflict
        const outcome = await reconcilePromise;
        expect(outcome).toBe("conflict");

        // Rebound session MUST remain active and un-corrupted by the old backend exit
        const currentSession = hook.result.current.state.sessions[sessionId];
        expect(currentSession?.backendSessionId).toBe("backend-new-2");
        expect(currentSession?.daemonEpoch).toBe("epoch-2");
        expect(currentSession?.incarnation).toBe("inc-new-2");
        expect(currentSession?.recoveryToken).toBe(2);
        expect(currentSession?.lifecycle).not.toBe("exited");
      } finally {
        describeSpy.mockRestore();
        hook.unmount();
      }
    });
  });
});
