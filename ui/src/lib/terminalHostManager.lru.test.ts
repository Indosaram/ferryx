import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createSession,
  flushAnimationFrames,
  mocks,
  setupTerminalHostTestEnv,
} from "./terminalHostManagerTestHelper";
import * as tauri from "./tauri";
import { terminalHostManager } from "./terminalHostManager";

describe("terminalHostManager LRU eviction & inactive output suspension", () => {
  beforeEach(() => {
    setupTerminalHostTestEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("evicts oldest inactive instance when exceeding warm budget while preserving all visible split instances and backend sessions", async () => {
    // Warm capacity = 2 inactive instances
    terminalHostManager.setWarmCacheLimit(2);

    // 1. Two visible split instances
    const visSession1 = createSession("sess-vis-1", "backend-vis-1");
    const visSession2 = createSession("sess-vis-2", "backend-vis-2");

    const unregVis1 = terminalHostManager.registerVisible("sess-vis-1");
    const unregVis2 = terminalHostManager.registerVisible("sess-vis-2");

    await terminalHostManager.getOrCreate(visSession1, true);
    await terminalHostManager.getOrCreate(visSession2, false);

    expect(terminalHostManager.getInstance("sess-vis-1")).toBeDefined();
    expect(terminalHostManager.getInstance("sess-vis-2")).toBeDefined();

    // 2. Add 3 inactive sessions in sequence (exceeding warm budget of 2)
    const inact1 = createSession("sess-inact-1", "backend-inact-1");
    const inact2 = createSession("sess-inact-2", "backend-inact-2");
    const inact3 = createSession("sess-inact-3", "backend-inact-3");

    await terminalHostManager.getOrCreate(inact1, false);
    await terminalHostManager.getOrCreate(inact2, false);
    await terminalHostManager.getOrCreate(inact3, false);

    // 3. Oldest inactive frontend renderer must be disposed and evicted
    expect(terminalHostManager.getInstance("sess-inact-1")).toBeUndefined();
    expect(mocks.terminalDisposed).toHaveBeenCalled();

    // 4. Visible split instances MUST remain pinned and not evicted
    expect(terminalHostManager.getInstance("sess-vis-1")).toBeDefined();
    expect(terminalHostManager.getInstance("sess-vis-2")).toBeDefined();

    // 5. Inactive instances within warm budget (2 and 3) remain warm
    expect(terminalHostManager.getInstance("sess-inact-2")).toBeDefined();
    expect(terminalHostManager.getInstance("sess-inact-3")).toBeDefined();

    // 6. Backend session record for inact1 is NOT closed or destroyed
    expect(inact1.backendSessionId).toBe("backend-inact-1");

    unregVis1();
    unregVis2();
    terminalHostManager.destroy("sess-vis-1");
    terminalHostManager.destroy("sess-vis-2");
    terminalHostManager.destroy("sess-inact-2");
    terminalHostManager.destroy("sess-inact-3");
  });

  it("suspends inactive output and resumes from the last sequence without resetting a warm renderer", async () => {
    const session = createSession("sess-suspend", "backend-suspend");
    const unreg = terminalHostManager.registerVisible("sess-suspend");
    await terminalHostManager.getOrCreate(session, true);
    await Promise.resolve();

    // Initial sequenced output while visible.
    mocks.terminalWrites.length = 0;
    mocks.emitSessionOutput("backend-suspend", "vis-1", "1", "epoch-1");
    flushAnimationFrames();
    expect(mocks.terminalWrites).toEqual(["vis-1"]);
    expect(session.lastOutputSequence).toBe("1");

    // Unmount / suspend session. Output emitted through the test bus while inactive has no xterm subscriber.
    unreg();
    mocks.terminalWrites.length = 0;
    mocks.emitSessionOutput("backend-suspend", "inact-2", "2", "epoch-1");
    flushAnimationFrames();
    expect(mocks.terminalWrites).toHaveLength(0);

    // Reattachment asks the daemon only for output after the last rendered sequence.
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-suspend",
      daemonEpoch: "epoch-1",
      historyStartSequence: "2",
      historyEndSequence: "2",
      history: btoa("inact-2"),
      gap: null,
    });

    const unreg2 = terminalHostManager.registerVisible("sess-suspend");
    await terminalHostManager.getOrCreate(session, true);
    await Promise.resolve();

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-suspend", afterSequence: "1" });
    expect(mocks.terminalReset).not.toHaveBeenCalled();

    flushAnimationFrames();
    expect(mocks.terminalWrites).toContain("inact-2");
    expect(session.lastOutputSequence).toBe("2");

    unreg2();
    terminalHostManager.destroy("sess-suspend");
  });
});
