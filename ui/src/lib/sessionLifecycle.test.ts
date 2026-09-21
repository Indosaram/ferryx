import { afterEach, describe, expect, it, vi } from "vitest";
import * as tauri from "./tauri";
import * as generalSettingsModule from "./generalSettings";

import {
  clearSleepingSessions,
  createStandbyBackendSessionId,
  getSessionProcessState,
  getSessionRecentScrollback,
  isSessionAutoResumeHeld,
  isSessionSleeping,
  isStandbyBackendSessionId,
  registerSessionSnapshot,
  requestSessionLifecycleAction,
  resetSessionLifecycleForTests,
  restartRegisteredSession,
  restoreSessionRecentScrollback,
  resumeRegisteredSession,
  setSessionActive,
  setSessionRebindHandler,
  setSessionSleeping,
  suspendRegisteredSession,
} from "./sessionLifecycle";
import type { TerminalSession } from "./types";

function session(backendSessionId: string | null, processState?: TerminalSession["processState"]): TerminalSession {
  return {
    id: "session-a",
    backendSessionId,
    processState,
    lifecycle: backendSessionId ? "working" : "exited",
    cwd: "/repo/main",
    workspaceId: "workspace-a",
    worktree: null,
  } as TerminalSession;
}

describe("sessionLifecycle", () => {
  afterEach(() => {
    clearSleepingSessions();
    resetSessionLifecycleForTests();
  });

  it("represents a restored shell as standby without allocating a real backend", () => {
    const backendSessionId = createStandbyBackendSessionId("session-a");
    setSessionSleeping("session-a", true);
    expect(backendSessionId).toBe("standby:session-a");
    expect(isStandbyBackendSessionId(backendSessionId)).toBe(true);
    expect(getSessionProcessState(session(backendSessionId, "standby"))).toBe("standby");
  });

  it("distinguishes running and hibernated resource state", () => {
    expect(getSessionProcessState(session("daemon-session-a"))).toBe("running");
    setSessionSleeping("session-a", true);
    expect(isSessionSleeping("session-a")).toBe(true);
    expect(getSessionProcessState(session(null))).toBe("hibernated");
  });

  it("preserves an explicitly persisted hibernated standby identity", () => {
    expect(getSessionProcessState(session("standby:session-a", "hibernated"))).toBe("hibernated");
  });

  it("clears hibernated state after a process is resumed", () => {
    setSessionSleeping("session-a", true);
    setSessionSleeping("session-a", false);
    expect(isSessionSleeping("session-a")).toBe(false);
    expect(getSessionProcessState(session("daemon-session-a", "hibernated"))).toBe("running");
  });

  it("manual Hibernate holds an active pane asleep until a real focus transition", () => {
    const current = session(null, "standby");
    registerSessionSnapshot(current, "idle");
    setSessionActive(current.id, true);

    requestSessionLifecycleAction("hibernate", current.id);
    expect(isSessionAutoResumeHeld(current.id)).toBe(true);
    expect(isSessionSleeping(current.id)).toBe(true);

    // Re-rendering the same active pane must not count as a focus event.
    setSessionActive(current.id, true);
    expect(isSessionAutoResumeHeld(current.id)).toBe(true);

    setSessionActive(current.id, false);
    setSessionActive(current.id, true);
    expect(isSessionAutoResumeHeld(current.id)).toBe(false);
  });

  it("retains only bounded recent scrollback for persisted hibernated sessions", () => {
    const history = `${"x".repeat(300_000)}tail`;
    restoreSessionRecentScrollback("session-a", history);
    const stored = getSessionRecentScrollback("session-a");
    expect(stored).toBeDefined();
    expect(stored!.length).toBe(256_000);
    expect(stored!.endsWith("tail")).toBe(true);
  });

  it("suspendRegisteredSession preserves backendSessionId and marks processState suspended", async () => {
    const live = session("backend-live-1", "running");
    registerSessionSnapshot(live, "idle");

    await suspendRegisteredSession(live.id);
    expect(isSessionSleeping(live.id)).toBe(true);
    expect(getSessionProcessState(live)).toBe("suspended");
    // Crucial: backendSessionId is NOT wiped!
    expect(live.backendSessionId).toBe("backend-live-1");
  });

  it("resumeRegisteredSession clears sleeping bit and restores running state", async () => {
    const live = session("backend-live-1", "running");
    registerSessionSnapshot(live, "idle");

    await suspendRegisteredSession(live.id);
    expect(isSessionSleeping(live.id)).toBe(true);

    await resumeRegisteredSession(live.id);
    expect(isSessionSleeping(live.id)).toBe(false);
    expect(getSessionProcessState(live)).toBe("running");
  });

  it("restartRegisteredSession closes old backend and allocates a fresh running shell, invoking rebind handler", async () => {
    const live = session("backend-old", "running");
    registerSessionSnapshot(live, "idle");

    const closeSpy = vi.spyOn(tauri, "closeTerminal").mockResolvedValue(undefined as any);
    const spawnSpy = vi.spyOn(tauri, "spawnTerminalDetailed").mockResolvedValue({
      sessionId: "backend-new",
      session: { sessionId: "backend-new", cwd: "/repo/main" } as any,
      daemonEpoch: "epoch-1",
    });

    const rebindSpy = vi.fn();
    setSessionRebindHandler(rebindSpy);

    await restartRegisteredSession(live.id);

    expect(closeSpy).toHaveBeenCalledWith("backend-old");
    expect(spawnSpy).toHaveBeenCalled();
    expect(rebindSpy).toHaveBeenCalledWith("session-a", "backend-new", "/repo/main", "epoch-1");
    expect(isSessionSleeping(live.id)).toBe(false);
    expect(getSessionProcessState(live)).toBe("running");

    setSessionRebindHandler(null);
    closeSpy.mockRestore();
    spawnSpy.mockRestore();
  });

  describe("idle sweep", () => {
    function useIdleSettings(minutes: number): () => void {
      const spy = vi
        .spyOn(generalSettingsModule, "loadGeneralSettings")
        .mockReturnValue({
          confirmCloseTab: false,
          sessionRestorePolicy: "lazy",
          sessionIdleTimeoutMinutes: minutes,
        });
      return () => spy.mockRestore();
    }

    function armSpies(describeResult: Promise<unknown>) {
      const suspendSpy = vi.spyOn(tauri, "suspendTerminal").mockResolvedValue(undefined as any);
      const describeSpy = vi.spyOn(tauri, "describeTerminal").mockReturnValue(describeResult as any);
      const historySpy = vi.spyOn(tauri, "getTerminalHistorySnapshot").mockResolvedValue(null as any);
      return { suspendSpy, describeSpy, historySpy };
    }

    function disarmSpies(spies: ReturnType<typeof armSpies>): void {
      spies.suspendSpy.mockRestore();
      spies.describeSpy.mockRestore();
      spies.historySpy.mockRestore();
    }

    it("never suspends when the idle timeout is 0 (off)", async () => {
      vi.useFakeTimers();
      const restoreSettings = useIdleSettings(0);
      const spies = armSpies(Promise.resolve({
        sessionId: "backend-live-1", cols: 80, rows: 24, running: true, lastOutputAgeMs: null,
      }));
      try {
        const live = session("backend-live-1", "running");
        registerSessionSnapshot(live, "idle");
        await vi.advanceTimersByTimeAsync(31 * 60_000);
        expect(spies.describeSpy).not.toHaveBeenCalled();
        expect(spies.suspendSpy).not.toHaveBeenCalled();
        expect(isSessionSleeping(live.id)).toBe(false);
      } finally {
        vi.useRealTimers();
        restoreSettings();
        disarmSpies(spies);
      }
    });

    it("vetoes suspension when the session produced recent PTY output despite an idle classifier", async () => {
      // Regression: a working omo subagent keeps redrawing its TUI, but the
      // classifier can report "idle" during long tool turns. The daemon-side
      // output ground truth (read 1 minute ago) must beat the classifier.
      vi.useFakeTimers();
      const restoreSettings = useIdleSettings(30);
      const spies = armSpies(Promise.resolve({
        sessionId: "backend-live-1", cols: 80, rows: 24, running: true, lastOutputAgeMs: 60_000,
      }));
      try {
        const live = session("backend-live-1", "running");
        registerSessionSnapshot(live, "idle");
        await vi.advanceTimersByTimeAsync(31 * 60_000);
        expect(spies.describeSpy).toHaveBeenCalled();
        expect(spies.suspendSpy).not.toHaveBeenCalled();
        expect(isSessionSleeping(live.id)).toBe(false);
      } finally {
        vi.useRealTimers();
        restoreSettings();
        disarmSpies(spies);
      }
    });

    it("suspends when classifier and output ground truth agree on inactivity", async () => {
      vi.useFakeTimers();
      const restoreSettings = useIdleSettings(30);
      const spies = armSpies(Promise.resolve({
        sessionId: "backend-live-1", cols: 80, rows: 24, running: true, lastOutputAgeMs: 45 * 60_000,
      }));
      try {
        const live = session("backend-live-1", "running");
        registerSessionSnapshot(live, "idle");
        await vi.advanceTimersByTimeAsync(31 * 60_000);
        expect(spies.suspendSpy).toHaveBeenCalledWith("backend-live-1");
        expect(isSessionSleeping(live.id)).toBe(true);
      } finally {
        vi.useRealTimers();
        restoreSettings();
        disarmSpies(spies);
      }
    });

    it("vetoes suspension when the output ground truth cannot be verified", async () => {
      vi.useFakeTimers();
      const restoreSettings = useIdleSettings(30);
      const suspendSpy = vi.spyOn(tauri, "suspendTerminal").mockResolvedValue(undefined as any);
      const describeSpy = vi
        .spyOn(tauri, "describeTerminal")
        .mockImplementation(() => Promise.reject(new Error("ipc down")));
      const historySpy = vi.spyOn(tauri, "getTerminalHistorySnapshot").mockResolvedValue(null as any);
      try {
        const live = session("backend-live-1", "running");
        registerSessionSnapshot(live, "idle");
        await vi.advanceTimersByTimeAsync(31 * 60_000);
        expect(describeSpy).toHaveBeenCalled();
        expect(suspendSpy).not.toHaveBeenCalled();
        expect(isSessionSleeping(live.id)).toBe(false);
      } finally {
        vi.useRealTimers();
        restoreSettings();
        suspendSpy.mockRestore();
        describeSpy.mockRestore();
        historySpy.mockRestore();
      }
    });
  });
});
