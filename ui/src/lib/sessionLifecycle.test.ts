import { afterEach, describe, expect, it } from "vitest";

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
  restoreSessionRecentScrollback,
  setSessionActive,
  setSessionSleeping,
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
});
