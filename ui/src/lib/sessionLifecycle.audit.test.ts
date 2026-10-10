import { afterEach, describe, expect, it, vi } from "vitest";
import * as tauri from "./tauri";
import {
  clearShellReplacementInflightForTests,
  replaceExitedShellSession,
} from "./shellReplacement";
import {
  clearSleepingSessions,
  isSessionSleeping,
  registerSessionSnapshot,
  resetSessionLifecycleForTests,
  restartRegisteredSession,
  resumeRegisteredSession,
  setSessionSleeping,
} from "./sessionLifecycle";
import type { TerminalSession } from "./types";

describe("sessionLifecycle and shellReplacement audit regression tests", () => {
  afterEach(() => {
    clearSleepingSessions();
    resetSessionLifecycleForTests();
    clearShellReplacementInflightForTests();
    vi.restoreAllMocks();
  });

  describe("ui-local-lifecycle-3: shell replacement and restart forward incarnation", () => {
    it("replaceExitedShellSession forwards session incarnation in REBIND_SESSION_BACKEND", async () => {
      const session: TerminalSession = {
        id: "pane-incarnation-1",
        workspaceId: "ws-local",
        cwd: "/repo",
        worktree: null,
        backendSessionId: null,
        lifecycle: "exited",
        incarnation: "stale-incarnation-uuid",
      };

      const spawn = vi.fn().mockResolvedValue({
        sessionId: "fresh-backend-pty",
        daemonEpoch: "epoch-10",
        session: {
          cwd: "/repo",
          incarnation: "fresh-incarnation-uuid",
        },
      });

      const dispatch = vi.fn();
      await replaceExitedShellSession("pane-incarnation-1", {
        getSessions: () => ({ "pane-incarnation-1": session }),
        spawn,
        dispatch,
      });

      expect(dispatch).toHaveBeenCalledWith({
        type: "REBIND_SESSION_BACKEND",
        sessionId: "pane-incarnation-1",
        backendSessionId: "fresh-backend-pty",
        cwd: "/repo",
        daemonEpoch: "epoch-10",
        incarnation: "fresh-incarnation-uuid",
        clearAgent: undefined,
      });
    });

    it("restartRegisteredSession forwards incarnation to SessionRebindHandler", async () => {
      const session: TerminalSession = {
        id: "session-restart-inc",
        workspaceId: "ws-local",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "backend-old-1",
        lifecycle: "working",
        processState: "running",
        incarnation: "old-incarnation",
      };

      registerSessionSnapshot(session);
      vi.spyOn(tauri, "closeTerminal").mockResolvedValue(undefined as any);
      vi.spyOn(tauri, "spawnTerminalDetailed").mockResolvedValue({
        sessionId: "backend-restarted-2",
        daemonEpoch: "epoch-5",
        session: {
          cwd: "/repo",
          incarnation: "new-incarnation-uuid",
        } as any,
      });

      const rebindHandler = vi.fn().mockResolvedValue(undefined);
      await restartRegisteredSession("session-restart-inc", rebindHandler);

      expect(rebindHandler).toHaveBeenCalledWith(
        "session-restart-inc",
        "backend-restarted-2",
        "/repo",
        "epoch-5",
        "new-incarnation-uuid",
      );
    });
  });

  describe("ui-local-lifecycle-4: replaceExitedShellSession allows replacing failed session", () => {
    it("allows replacing a session when lifecycle is failed", async () => {
      const session: TerminalSession = {
        id: "pane-failed",
        workspaceId: "ws-local",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "backend-failed-pty",
        lifecycle: "failed",
      };

      const spawn = vi.fn().mockResolvedValue({
        sessionId: "new-pty-after-failure",
        daemonEpoch: "epoch-1",
        session: { cwd: "/repo", incarnation: "inc-1" },
      });
      const dispatch = vi.fn();

      const result = await replaceExitedShellSession("pane-failed", {
        getSessions: () => ({ "pane-failed": session }),
        spawn,
        dispatch,
      });

      expect(result.sessionId).toBe("new-pty-after-failure");
      expect(dispatch).toHaveBeenCalledWith({
        type: "REBIND_SESSION_BACKEND",
        sessionId: "pane-failed",
        backendSessionId: "new-pty-after-failure",
        cwd: "/repo",
        daemonEpoch: "epoch-1",
        incarnation: "inc-1",
        clearAgent: undefined,
      });
    });
  });

  describe("ui-local-lifecycle-6: resumeRegisteredSession clears sleeping on SESSION_NOT_FOUND", () => {
    it("clears sleeping state when backend session is not found", async () => {
      const session: TerminalSession = {
        id: "session-sleeping-not-found",
        workspaceId: "ws-local",
        cwd: "/repo",
        worktree: null,
        backendSessionId: "backend-gone-1",
        lifecycle: "working",
        processState: "suspended",
      };

      registerSessionSnapshot(session);
      setSessionSleeping("session-sleeping-not-found", true);
      expect(isSessionSleeping("session-sleeping-not-found")).toBe(true);

      vi.spyOn(tauri, "resumeTerminal").mockRejectedValue({
        code: "SESSION_NOT_FOUND",
        message: "Session backend-gone-1 was not found on daemon",
      });

      await expect(resumeRegisteredSession("session-sleeping-not-found")).rejects.toMatchObject({
        code: "SESSION_NOT_FOUND",
      });

      expect(isSessionSleeping("session-sleeping-not-found")).toBe(false);
    });
  });
});
