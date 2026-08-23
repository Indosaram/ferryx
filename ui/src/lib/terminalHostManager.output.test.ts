import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createDeferred,
  createSession,
  flushAnimationFrames,
  mocks,
  setupTerminalHostTestEnv,
} from "./terminalHostManagerTestHelper";
import * as tauri from "./tauri";
import { terminalEventBus } from "./terminalEvents";
import { MAX_PENDING_OUTPUT_CHARS, terminalHostManager } from "./terminalHostManager";

describe("terminalHostManager output scheduling & batching", () => {
  beforeEach(() => {
    setupTerminalHostTestEnv();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("unsubscribes previous backendSessionId output subscription when backendSessionId changes in updateSession", async () => {
    const session = createSession("sess-update", "backend-old");
    const unreg = terminalHostManager.registerVisible("sess-update");
    await terminalHostManager.getOrCreate(session, true);

    mocks.terminalWrites.length = 0;
    mocks.emitSessionOutput("backend-old", "hello old");
    flushAnimationFrames();
    expect(mocks.terminalWrites).toContain("hello old");

    mocks.terminalWrites.length = 0;
    const updatedSession = createSession("sess-update", "backend-new");
    terminalHostManager.updateSession("sess-update", updatedSession);

    // Old backend session output must NOT be written anymore
    mocks.emitSessionOutput("backend-old", "old after update");
    flushAnimationFrames();
    expect(mocks.terminalWrites).not.toContain("old after update");

    // New backend session output MUST be written
    mocks.emitSessionOutput("backend-new", "hello new");
    flushAnimationFrames();
    expect(mocks.terminalWrites).toContain("hello new");

    unreg();
    terminalHostManager.destroy("sess-update");
  });

  it("batches same-frame output writes at animation-frame cadence into a single ordered terminal write", async () => {
    const session = createSession("sess-batched", "backend-batch");
    const unreg = terminalHostManager.registerVisible("sess-batched");
    await terminalHostManager.getOrCreate(session, true);

    mocks.terminalWrites.length = 0;

    mocks.emitSessionOutput("backend-batch", "chunk1-");
    mocks.emitSessionOutput("backend-batch", "chunk2-");
    mocks.emitSessionOutput("backend-batch", "chunk3");

    // Zero immediate writes before frame flush
    expect(mocks.terminalWrites).toHaveLength(0);

    // Flush animation frame
    flushAnimationFrames();

    // Exactly one write call with all 3 chunks in original order
    expect(mocks.terminalWrites).toEqual(["chunk1-chunk2-chunk3"]);

    unreg();
    terminalHostManager.destroy("sess-batched");
  });

  it("cancels pending batched output and prevents writes after destroy or updateSession rebinding", async () => {
    // 1. Teardown via destroy
    const session1 = createSession("sess-teardown-1", "backend-td-1");
    const unreg1 = terminalHostManager.registerVisible("sess-teardown-1");
    await terminalHostManager.getOrCreate(session1, true);
    mocks.terminalWrites.length = 0;

    mocks.emitSessionOutput("backend-td-1", "pending before destroy");
    unreg1();
    terminalHostManager.destroy("sess-teardown-1");

    flushAnimationFrames();
    expect(mocks.terminalWrites).toHaveLength(0);

    // 2. Teardown via updateSession rebinding
    const session2 = createSession("sess-teardown-2", "backend-td-old");
    const unreg2 = terminalHostManager.registerVisible("sess-teardown-2");
    await terminalHostManager.getOrCreate(session2, true);
    mocks.terminalWrites.length = 0;

    mocks.emitSessionOutput("backend-td-old", "pending before rebind");
    const session2Updated = createSession("sess-teardown-2", "backend-td-new");
    terminalHostManager.updateSession("sess-teardown-2", session2Updated);

    flushAnimationFrames();
    expect(mocks.terminalWrites).toHaveLength(0);

    // New backend emits and flushes normally
    mocks.emitSessionOutput("backend-td-new", "live new");
    expect(mocks.terminalWrites).toHaveLength(0);
    flushAnimationFrames();
    expect(mocks.terminalWrites).toEqual(["live new"]);

    unreg2();
    terminalHostManager.destroy("sess-teardown-2");
  });

  it("batches outputs per session independently without cross-session mixing", async () => {
    const session1 = createSession("sess-mix-1", "backend-mix-1");
    const session2 = createSession("sess-mix-2", "backend-mix-2");

    const unreg1 = terminalHostManager.registerVisible("sess-mix-1");
    const unreg2 = terminalHostManager.registerVisible("sess-mix-2");

    await terminalHostManager.getOrCreate(session1, true);
    await terminalHostManager.getOrCreate(session2, true);

    mocks.terminalWrites.length = 0;

    mocks.emitSessionOutput("backend-mix-1", "s1-a");
    mocks.emitSessionOutput("backend-mix-2", "s2-a");
    mocks.emitSessionOutput("backend-mix-1", "s1-b");
    mocks.emitSessionOutput("backend-mix-2", "s2-b");

    expect(mocks.terminalWrites).toHaveLength(0);

    flushAnimationFrames();

    expect(mocks.terminalWrites).toEqual(["s1-as1-b", "s2-as2-b"]);

    unreg1();
    unreg2();
    terminalHostManager.destroy("sess-mix-1");
    terminalHostManager.destroy("sess-mix-2");
  });

  it("maintains replay-before-live ordering on initial subscription", async () => {
    const session = createSession("sess-replay", "backend-replay");
    const unreg = terminalHostManager.registerVisible("sess-replay");

    // Simulate backlog replay synchronously during subscribeOutput
    vi.spyOn(terminalEventBus, "subscribeOutput").mockImplementationOnce((sessionId: string, cb: (text: string) => void) => {
      let set = mocks.outputListeners.get(sessionId);
      if (!set) {
        set = new Set();
        mocks.outputListeners.set(sessionId, set);
      }
      set.add(cb);
      cb("replay-chunk-");
      return vi.fn(() => {
        set?.delete(cb);
        if (set?.size === 0) mocks.outputListeners.delete(sessionId);
      });
    });

    await terminalHostManager.getOrCreate(session, true);
    mocks.emitSessionOutput("backend-replay", "live-chunk");

    // Clear preview banner writes
    mocks.terminalWrites.length = 0;

    // Zero immediate writes before frame flush
    expect(mocks.terminalWrites).toHaveLength(0);

    flushAnimationFrames();
    expect(mocks.terminalWrites).toEqual(["replay-chunk-live-chunk"]);

    unreg();
    terminalHostManager.destroy("sess-replay");
  });

  it("synchronously flushes pending output when accumulated characters reach or exceed MAX_PENDING_OUTPUT_CHARS before rAF", async () => {
    const session = createSession("sess-cap", "backend-cap");
    const unreg = terminalHostManager.registerVisible("sess-cap");
    await terminalHostManager.getOrCreate(session, true);

    mocks.terminalWrites.length = 0;

    const halfCap = "A".repeat(MAX_PENDING_OUTPUT_CHARS / 2);
    const exceedCap = "B".repeat(MAX_PENDING_OUTPUT_CHARS / 2 + 10);
    const trailingChunk = "C".repeat(50);

    // 1. Below cap: queued without immediate write
    mocks.emitSessionOutput("backend-cap", halfCap);
    expect(mocks.terminalWrites).toHaveLength(0);

    // 2. Reaching/exceeding cap: synchronously flushes accumulated batch immediately before rAF
    mocks.emitSessionOutput("backend-cap", exceedCap);
    expect(mocks.terminalWrites).toEqual([halfCap + exceedCap]);

    // 3. New chunk arriving after flush is held for next rAF
    mocks.emitSessionOutput("backend-cap", trailingChunk);
    expect(mocks.terminalWrites).toEqual([halfCap + exceedCap]);

    // 4. On rAF flush, trailing chunk is written in ordered sequence
    flushAnimationFrames();
    expect(mocks.terminalWrites).toEqual([halfCap + exceedCap, trailingChunk]);

    unreg();
    terminalHostManager.destroy("sess-cap");
  });

  it("renders decoded base64 history before live output and suppresses duplicate live chunks with sequence <= historyEndSequence", async () => {
    const session = createSession("sess-dedupe", "backend-dedupe");
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-dedupe",
      daemonEpoch: "epoch-1",
      historyStartSequence: "1",
      historyEndSequence: "10",
      history: btoa("HISTORY_1_TO_10;"),
      gap: null,
    });

    const unreg = terminalHostManager.registerVisible("sess-dedupe");
    await terminalHostManager.getOrCreate(session, true);

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-dedupe", afterSequence: null });

    // Emit duplicate live outputs (<= 10) and new live outputs (> 10)
    mocks.emitSessionOutput("backend-dedupe", "LIVE_DUP_9;", "9", "epoch-1");
    mocks.emitSessionOutput("backend-dedupe", "LIVE_DUP_10;", "10", "epoch-1");
    mocks.emitSessionOutput("backend-dedupe", "LIVE_NEW_11;", "11", "epoch-1");
    mocks.emitSessionOutput("backend-dedupe", "LIVE_NEW_12;", "12", "epoch-1");

    // Clear preview banner writes
    mocks.terminalWrites.length = 0;

    flushAnimationFrames();

    const allWrites = mocks.terminalWrites.join("");
    expect(allWrites).toContain("HISTORY_1_TO_10;");
    expect(allWrites).toContain("LIVE_NEW_11;");
    expect(allWrites).toContain("LIVE_NEW_12;");
    expect(allWrites).not.toContain("LIVE_DUP_9;");
    expect(allWrites).not.toContain("LIVE_DUP_10;");
    expect(session.lastOutputSequence).toBe("12");

    unreg();
    terminalHostManager.destroy("sess-dedupe");
  });

  it("suppresses live chunks queued while attach is in-flight when their sequence <= historyEndSequence", async () => {
    const session = createSession("sess-inflight", "backend-inflight");
    const deferredAttach = createDeferred<tauri.AttachTerminalResponse>();
    vi.spyOn(tauri, "attachTerminal").mockImplementationOnce(() => deferredAttach.promise);

    const unreg = terminalHostManager.registerVisible("sess-inflight");
    await terminalHostManager.getOrCreate(session, true);

    // Live chunks arrive while attach is in flight (subscription active, attach pending)
    mocks.emitSessionOutput("backend-inflight", "INFLIGHT_DUP_5;", "5", "epoch-1");
    mocks.emitSessionOutput("backend-inflight", "INFLIGHT_DUP_10;", "10", "epoch-1");
    mocks.emitSessionOutput("backend-inflight", "INFLIGHT_NEW_15;", "15", "epoch-1");

    // Attach completes with history up to sequence 10
    deferredAttach.resolve({
      sessionId: "backend-inflight",
      daemonEpoch: "epoch-1",
      historyStartSequence: "1",
      historyEndSequence: "10",
      history: btoa("ATTACH_HISTORY_10;"),
      gap: null,
    });

    await Promise.resolve();
    mocks.terminalWrites.length = 0;

    flushAnimationFrames();

    const allWrites = mocks.terminalWrites.join("");
    expect(allWrites).toContain("ATTACH_HISTORY_10;");
    expect(allWrites).toContain("INFLIGHT_NEW_15;");
    expect(allWrites).not.toContain("INFLIGHT_DUP_5;");
    expect(allWrites).not.toContain("INFLIGHT_DUP_10;");
    expect(session.lastOutputSequence).toBe("15");

    unreg();
    terminalHostManager.destroy("sess-inflight");
  });

  it("resumes attachment with lastOutputSequence as afterSequence without resetting terminal on clean resumption", async () => {
    const session = createSession("sess-resume", "backend-resume", "ws-1", "epoch-1", "100");
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-resume",
      daemonEpoch: "epoch-1",
      historyStartSequence: "101",
      historyEndSequence: "105",
      history: btoa("INCR_101_105;"),
      gap: null,
    });

    mocks.terminalReset.mockClear();

    const unreg = terminalHostManager.registerVisible("sess-resume");
    await terminalHostManager.getOrCreate(session, true);

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-resume", afterSequence: "100" });
    expect(mocks.terminalReset).not.toHaveBeenCalled();

    mocks.terminalWrites.length = 0;
    flushAnimationFrames();

    expect(mocks.terminalWrites.join("")).toContain("INCR_101_105;");
    expect(session.lastOutputSequence).toBe("105");

    unreg();
    terminalHostManager.destroy("sess-resume");
  });

  it("resets terminal and replays full history when attachment reports an explicit sequence gap", async () => {
    const session = createSession("sess-gap", "backend-gap", "ws-1", "epoch-1", "50");
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-gap",
      daemonEpoch: "epoch-1",
      historyStartSequence: "100",
      historyEndSequence: "150",
      history: btoa("REPLAY_FULL_100_150;"),
      gap: {
        requestedAfterSequence: "50",
        availableFromSequence: "100",
      },
    });

    mocks.terminalReset.mockClear();

    const unreg = terminalHostManager.registerVisible("sess-gap");
    await terminalHostManager.getOrCreate(session, true);

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-gap", afterSequence: "50" });
    expect(mocks.terminalReset).toHaveBeenCalled();

    mocks.terminalWrites.length = 0;
    flushAnimationFrames();

    expect(mocks.terminalWrites.join("")).toContain("REPLAY_FULL_100_150;");
    expect(session.lastOutputSequence).toBe("150");

    unreg();
    terminalHostManager.destroy("sess-gap");
  });

  it("resets terminal and updates daemonEpoch on daemon epoch mismatch", async () => {
    const session = createSession("sess-epoch", "backend-epoch", "ws-1", "epoch-OLD", "80");
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-epoch",
      daemonEpoch: "epoch-NEW",
      historyStartSequence: "1",
      historyEndSequence: "20",
      history: btoa("FRESH_EPOCH_DATA;"),
      gap: null,
    });

    mocks.terminalReset.mockClear();

    const unreg = terminalHostManager.registerVisible("sess-epoch");
    await terminalHostManager.getOrCreate(session, true);

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-epoch", afterSequence: "80" });
    expect(mocks.terminalReset).toHaveBeenCalled();

    mocks.terminalWrites.length = 0;
    flushAnimationFrames();

    expect(mocks.terminalWrites.join("")).toContain("FRESH_EPOCH_DATA;");
    expect(session.daemonEpoch).toBe("epoch-NEW");
    expect(session.lastOutputSequence).toBe("20");

    unreg();
    terminalHostManager.destroy("sess-epoch");
  });

  it("handles 64-bit decimal string sequences (u64) beyond Number.MAX_SAFE_INTEGER without precision loss", async () => {
    const session = createSession("sess-u64", "backend-u64");
    const attachSpy = vi.spyOn(tauri, "attachTerminal").mockResolvedValueOnce({
      sessionId: "backend-u64",
      daemonEpoch: "epoch-1",
      historyStartSequence: "9007199254740990",
      historyEndSequence: "9007199254740995",
      history: btoa("BIG_HISTORY;"),
      gap: null,
    });

    const unreg = terminalHostManager.registerVisible("sess-u64");
    await terminalHostManager.getOrCreate(session, true);

    expect(attachSpy).toHaveBeenCalledWith({ sessionId: "backend-u64", afterSequence: null });

    // Live chunks with 64-bit sequences: duplicate (9007199254740995) vs new (9007199254740996)
    mocks.emitSessionOutput("backend-u64", "BIG_DUP;", "9007199254740995", "epoch-1");
    mocks.emitSessionOutput("backend-u64", "BIG_NEW_96;", "9007199254740996", "epoch-1");
    mocks.emitSessionOutput("backend-u64", "BIG_NEW_97;", "9007199254740997", "epoch-1");

    mocks.terminalWrites.length = 0;
    flushAnimationFrames();

    const allWrites = mocks.terminalWrites.join("");
    expect(allWrites).toContain("BIG_HISTORY;");
    expect(allWrites).toContain("BIG_NEW_96;");
    expect(allWrites).toContain("BIG_NEW_97;");
    expect(allWrites).not.toContain("BIG_DUP;");
    expect(session.lastOutputSequence).toBe("9007199254740997");

    unreg();
    terminalHostManager.destroy("sess-u64");
  });
});
