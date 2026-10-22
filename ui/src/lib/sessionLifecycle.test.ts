import { afterEach, describe, expect, it, vi } from "vitest";
import * as tauri from "./tauri";

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

  function stoppedDetails(overrides: Partial<tauri.TerminalDescribeResult>): tauri.TerminalDescribeResult {
    return { sessionId: "back", cols: 80, rows: 24, running: true, suspended: true, kernelStopped: true, ...overrides };
  }

  it("does not mark a running session asleep when only its reader is paused", async () => {
    let answered: () => void = () => {};
    const describeAnswered = new Promise<void>((done) => { answered = done; });
    vi.spyOn(tauri, "describeTerminal").mockImplementation(async () => {
      queueMicrotask(answered);
      return { sessionId: "back", cols: 80, rows: 24, running: true, suspended: false, kernelStopped: false, readerPaused: true };
    });
    const current = session("back");
    registerSessionSnapshot(current);
    await describeAnswered;
    await Promise.resolve();
    await Promise.resolve();
    expect(getDaemonSuspension(current.id)?.readerPaused).toBe(true);
    expect(isSessionSleeping(current.id)).toBe(false);
  });

  it("releases a Ferryx-owned stop left by an earlier build", async () => {
    vi.spyOn(tauri, "describeTerminal").mockResolvedValue(
      stoppedDetails({ suspensionSource: "ferryx-owned", registrySuspended: true }),
    );
    let resumed: () => void = () => {};
    const resumeCalled = new Promise<void>((done) => { resumed = done; });
    const resume = vi.spyOn(tauri, "resumeTerminal").mockImplementation(async () => { resumed(); });
    const current = session("back");
    const awake = new Promise<void>((done) => {
      const unsubscribe = subscribeSleepingSessions(() => {
        if (!isSessionSleeping(current.id) && resume.mock.calls.length > 0) { unsubscribe(); done(); }
      });
    });
    registerSessionSnapshot(current);
    await resumeCalled;
    await awake;
    expect(resume).toHaveBeenCalledTimes(1);
    expect(resume).toHaveBeenCalledWith("back");
    expect(isSessionSleeping(current.id)).toBe(false);
  });

  it("releases a stop the daemon registry records as its own", async () => {
    vi.spyOn(tauri, "describeTerminal").mockResolvedValue(
      stoppedDetails({ suspensionSource: "unknown", registrySuspended: true }),
    );
    let resumed: () => void = () => {};
    const resumeCalled = new Promise<void>((done) => { resumed = done; });
    const resume = vi.spyOn(tauri, "resumeTerminal").mockImplementation(async () => { resumed(); });
    registerSessionSnapshot(session("back"));
    await resumeCalled;
    expect(resume).toHaveBeenCalledTimes(1);
  });

  it("keeps the Resume overlay when releasing a Ferryx-owned stop fails", async () => {
    vi.spyOn(tauri, "describeTerminal").mockResolvedValue(
      stoppedDetails({ suspensionSource: "ferryx-owned", registrySuspended: true }),
    );
    let rejected: () => void = () => {};
    const resumeRejected = new Promise<void>((done) => { rejected = done; });
    vi.spyOn(tauri, "resumeTerminal").mockImplementation(() => {
      queueMicrotask(rejected);
      return Promise.reject({ code: "TIMEOUT", message: "held" });
    });
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const current = session("back");
    registerSessionSnapshot(current);
    await resumeRejected;
    await vi.waitFor(() => expect(warn).toHaveBeenCalledWith("Failed to release a Ferryx-suspended session:", expect.anything()));
    expect(isSessionSleeping(current.id)).toBe(true);
    expect(getDaemonSuspension(current.id)).toBeDefined();
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

  it("resumeRegisteredSession clears sleeping bit and restores running state", async () => {
    vi.spyOn(tauri, "resumeTerminal").mockResolvedValue(undefined);
    const live = session("backend-live-1", "running");
    registerSessionSnapshot(live, "idle");
    setSessionSleeping(live.id, true);
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
});
