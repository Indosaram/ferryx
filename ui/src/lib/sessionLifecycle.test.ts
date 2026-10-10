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
  subscribeSleepingSessions,
  getDaemonSuspension,
} from "./sessionLifecycle";
import type { TerminalSession, TerminalLifecyclePayload } from "./types";
import { TerminalEventBus } from "./terminalEvents";

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
  it("EOF listener mutation cannot notify removed or newly registered owners", async () => {
    const terminalEvents = new TerminalEventBus();
    let deliver: ((payload: TerminalLifecyclePayload) => void) | undefined;
    vi.spyOn(tauri, "onTerminalOutput").mockResolvedValue(() => {});
    vi.spyOn(tauri, "onTerminalLifecycle").mockImplementation(async (listener) => {
      deliver = listener;
      return () => {};
    });
    const states: string[] = [];
    let removeSecond = () => {};
    let removeNew = () => {};
    const removeFirst = terminalEvents.subscribeLifecycle(() => {
      states.push("first-exited");
      removeSecond();
      removeNew = terminalEvents.subscribeLifecycle(() => states.push("new-exited"));
    });
    removeSecond = terminalEvents.subscribeLifecycle(() => states.push("removed-exited"));
    await terminalEvents.ensureStarted();
    expect(deliver).toBeTypeOf("function");
    deliver!({ sessionId: "eof-owner", state: "exited", exitCode: 0, reason: null });
    expect(states).toEqual(["first-exited"]);
    removeFirst();
    removeSecond();
    removeNew();
  });
  afterEach(() => {
    clearSleepingSessions();
    resetSessionLifecycleForTests();
    vi.restoreAllMocks();
  });

  it("displays authoritative suspension without resuming on activation", async () => {
    let resolve: (value: tauri.TerminalDescribeResult) => void = () => {};
    const reply = new Promise<tauri.TerminalDescribeResult>((done) => { resolve = done; });
    vi.spyOn(tauri, "describeTerminal").mockImplementation(() => reply);
    const resume = vi.spyOn(tauri, "resumeTerminal").mockResolvedValue(undefined);
    const changed = new Promise<void>((done) => {
      const unsubscribe = subscribeSleepingSessions(() => { unsubscribe(); done(); });
    });
    const current = session("back");
    registerSessionSnapshot(current);
    setSessionActive(current.id, true);
    resolve({ sessionId: "back", cols: 80, rows: 24, running: true, suspended: true, kernelStopped: true, suspensionSource: "externalOrUnknownStop" });
    await changed;
    expect(isSessionSleeping(current.id)).toBe(true);
    expect(getDaemonSuspension(current.id)?.suspensionSource).toBe("externalOrUnknownStop");
    expect(resume).not.toHaveBeenCalled();
  });

  it("failed explicit resume preserves the suspended state", async () => {
    vi.spyOn(tauri, "describeTerminal").mockResolvedValue(null);
    vi.spyOn(tauri, "resumeTerminal").mockRejectedValue({ code: "TIMEOUT", message: "held" });
    const current = session("back", "suspended");
    registerSessionSnapshot(current);
    setSessionSleeping(current.id, true);
    await expect(resumeRegisteredSession(current.id)).rejects.toMatchObject({ code: "TIMEOUT" });
    expect(isSessionSleeping(current.id)).toBe(true);
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

  it("explicit resume invokes resumeTerminal on backend", async () => {
    const resumeSpy = vi.spyOn(tauri, "resumeTerminal").mockResolvedValue(undefined as any);
    const sess = session("backend-live-resume", "suspended");
    registerSessionSnapshot(sess);
    setSessionSleeping(sess.id, true);
    await resumeRegisteredSession(sess.id);
    expect(resumeSpy).toHaveBeenCalledWith("backend-live-resume");
    expect(isSessionSleeping(sess.id)).toBe(false);
    resumeSpy.mockRestore();
  });

  it("preserves durable session across unmount without destroying backing PTY", () => {
    const sess = session("backend-live-pty", "running");
    registerSessionSnapshot(sess);
    setSessionActive(sess.id, true);
    setSessionActive(sess.id, false);
    expect(getSessionProcessState(sess)).toBe("running");
    expect(sess.backendSessionId).toBe("backend-live-pty");
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
    // This text is persisted inside session_state.json, which is saved by rewriting the whole
    // file. At 256 KB per session the file reached 8.4 MB and a burst of session churn dirtied
    // 2.1 GB in 142 seconds, freezing the app; the bound is deliberately small.
    expect(stored!.length).toBe(24_000);
    expect(stored!.endsWith("tail")).toBe(true);
  });

  it("adopts a daemon-reported suspension the GUI did not perform (restart case)", async () => {
    // Regression: after a GUI restart the persisted pane said "running" while the daemon's
    // process was SIGSTOPped, so no overlay or auto-resume ever fired and the pane froze.
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-stopped", cols: 80, rows: 24, running: true, suspended: true,
    });
    try {
      const restored = session("backend-stopped", "running");
      registerSessionSnapshot(restored, "idle");
      await vi.waitFor(() => expect(isSessionSleeping(restored.id)).toBe(true));
      expect(describeSpy).toHaveBeenCalledWith("backend-stopped");

      // Re-registering the same binding must not query the daemon again.
      registerSessionSnapshot(restored, "idle");
      expect(describeSpy).toHaveBeenCalledTimes(1);
    } finally {
      describeSpy.mockRestore();
    }
  });

  it("leaves a running daemon session awake", async () => {
    const describeSpy = vi.spyOn(tauri, "describeTerminal").mockResolvedValue({
      sessionId: "backend-live", cols: 80, rows: 24, running: true, suspended: false,
    });
    try {
      const live = session("backend-live", "running");
      registerSessionSnapshot(live, "idle");
      await vi.waitFor(() => expect(describeSpy).toHaveBeenCalledWith("backend-live"));
      await Promise.resolve();
      expect(isSessionSleeping(live.id)).toBe(false);
    } finally {
      describeSpy.mockRestore();
    }
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
        // Only the one-time suspension reconcile on registration; the disabled sweep never asks.
        expect(spies.describeSpy).toHaveBeenCalledTimes(1);
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

    it("leaves inactivity suspension to the daemon even when classifier and output agree", async () => {
      vi.useFakeTimers();
      const restoreSettings = useIdleSettings(30);
      const spies = armSpies(Promise.resolve({
        sessionId: "backend-live-1", cols: 80, rows: 24, running: true, lastOutputAgeMs: 45 * 60_000,
      }));
      try {
        const live = session("backend-live-1", "running");
        registerSessionSnapshot(live, "idle");
        await vi.advanceTimersByTimeAsync(31 * 60_000);
        expect(spies.suspendSpy).not.toHaveBeenCalled();
        expect(isSessionSleeping(live.id)).toBe(false);
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
