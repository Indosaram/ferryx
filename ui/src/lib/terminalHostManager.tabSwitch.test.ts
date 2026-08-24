import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createSession,
  flushAnimationFrames,
  mocks,
  setupTerminalHostTestEnv,
} from "./terminalHostManagerTestHelper";
import * as tauri from "./tauri";
import { terminalHostManager } from "./terminalHostManager";

describe("terminalHostManager tab visibility resume", () => {
  beforeEach(() => {
    setupTerminalHostTestEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("preserves the warm-cached terminal buffer when a tab resumes without a replay gap", async () => {
    const session = createSession("sess-tab-switch", "backend-tab-switch", "ws-1", "epoch-1", "100");
    const attachSpy = vi.spyOn(tauri, "attachTerminal")
      .mockResolvedValueOnce({
        sessionId: "backend-tab-switch",
        daemonEpoch: "epoch-1",
        historyStartSequence: null,
        historyEndSequence: "100",
        history: "",
        gap: null,
      })
      .mockResolvedValueOnce({
        sessionId: "backend-tab-switch",
        daemonEpoch: "epoch-1",
        historyStartSequence: "101",
        historyEndSequence: "101",
        history: btoa("MISSED_WHILE_HIDDEN;"),
        gap: null,
      });

    const unregisterFirstMount = terminalHostManager.registerVisible(session.id);
    const instance = await terminalHostManager.getOrCreate(session, true);
    await Promise.resolve();

    mocks.terminalReset.mockClear();
    unregisterFirstMount();

    const unregisterSecondMount = terminalHostManager.registerVisible(session.id);
    await Promise.resolve();

    expect(terminalHostManager.getInstance(session.id)).toBe(instance);
    expect(attachSpy).toHaveBeenNthCalledWith(2, {
      sessionId: "backend-tab-switch",
      afterSequence: "100",
    });
    expect(mocks.terminalReset).not.toHaveBeenCalled();

    mocks.terminalWrites.length = 0;
    flushAnimationFrames();
    expect(mocks.terminalWrites.join("")).toContain("MISSED_WHILE_HIDDEN;");
    expect(session.lastOutputSequence).toBe("101");

    unregisterSecondMount();
    terminalHostManager.destroy(session.id);
  });
});
