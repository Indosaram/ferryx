import { describe, expect, it, vi } from "vitest";
import {
  classifyPaneLiveness,
  observePaneLiveness,
  PaneLivenessSnapshot,
} from "./paneLiveness";
import { terminalInputQueue } from "./nativeTerminalInputQueue";

describe("classifyPaneLiveness", () => {
  it("returns UNKNOWN when telemetry is not available", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: false,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("UNKNOWN");
  });

  it("never returns IDLE when session identity is missing", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: null,
      queuedHeadAgeMs: null,
      executingRunningAgeMs: null,
      writePendingMs: null,
      readerPaused: false,
      kernelStopped: false,
      registrySuspended: false,
      suspended: false,
      hasUnpresentedFrames: false,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("UNKNOWN");
  });

  it("returns IDLE only when session identity and all negative states are positively confirmed", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      queuedHeadAgeMs: null,
      queuedCount: 0,
      executingRunningAgeMs: null,
      writePendingMs: null,
      readerPaused: false,
      kernelStopped: false,
      registrySuspended: false,
      suspended: false,
      hasUnpresentedFrames: false,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("IDLE");
  });

  it("does not classify normal in-flight dispatch under threshold as blocked", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      stage: "dispatch",
      executingRunningAgeMs: 50,
      queuedHeadAgeMs: null,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("UNKNOWN");
  });

  it("classifies in-flight execution over 250ms threshold as BLOCKED_IN_IPC_WRITE", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      stage: "backend_write_start",
      executingRunningAgeMs: 300,
      queuedHeadAgeMs: 400,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("BLOCKED_IN_IPC_WRITE");
  });

  it("fences sequence comparison by session identity in addition to epoch", () => {
    const mismatchedSession: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-A",
      vtSessionId: "session-B",
      daemonEpoch: "epoch-1",
      vtEpoch: "epoch-1",
      hubEndSequence: 100,
      vtConsumedSequence: 50,
    };
    expect(classifyPaneLiveness(mismatchedSession)).toBe("UNKNOWN");

    const matchedSession: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-A",
      vtSessionId: "session-A",
      daemonEpoch: "epoch-1",
      vtEpoch: "epoch-1",
      hubEndSequence: 100,
      vtConsumedSequence: 50,
    };
    expect(classifyPaneLiveness(matchedSession)).toBe("BLOCKED_IN_PRESENTATION");
  });

  it("prioritizes BLOCKED_IN_READER_PAUSED when reader is explicitly paused", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      readerPaused: true,
      queuedHeadAgeMs: 500,
      executingRunningAgeMs: 500,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("BLOCKED_IN_READER_PAUSED");
  });

  it("treats reader_paused=false as not paused, continuing to check backlog", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      readerPaused: false,
      queuedHeadAgeMs: 300,
      executingRunningAgeMs: null,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("BLOCKED_IN_QUEUE");
  });

  it("returns UNKNOWN for unverified registry suspension without verified actuation receipt", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      suspended: true,
      registrySuspended: true,
      suspensionSource: "unknown",
      verifiedActuationReceipt: null,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("UNKNOWN");
  });

  it("returns BLOCKED_IN_ATTRIBUTED_SUSPENSION only when actuation receipt is verified", () => {
    const snapshot: PaneLivenessSnapshot = {
      telemetryAvailable: true,
      sessionId: "session-1",
      suspended: true,
      suspensionSource: "ferryx-lifecycle",
      verifiedActuationReceipt: true,
    };
    expect(classifyPaneLiveness(snapshot)).toBe("BLOCKED_IN_ATTRIBUTED_SUSPENSION");
  });

  it("observePaneLiveness queries real input queue state", () => {
    const verdict = observePaneLiveness("session-observed", {
      readerPaused: false,
      kernelStopped: false,
      suspended: false,
      hasUnpresentedFrames: false,
    });
    expect(verdict).toBe("IDLE");

    const unknownVerdict = observePaneLiveness("");
    expect(unknownVerdict).toBe("UNKNOWN");
  });

  it("observePaneLiveness tracks real in-flight execution barrier", async () => {
    let releaseBarrier: () => void = () => {};
    const barrierPromise = new Promise<void>((resolve) => {
      releaseBarrier = resolve;
    });

    const pending = terminalInputQueue.enqueue("session-real-barrier", null, 10, async () => {
      await barrierPromise;
      return "done";
    });

    const activeVerdict = observePaneLiveness("session-real-barrier", {
      readerPaused: false,
      kernelStopped: false,
      suspended: false,
      hasUnpresentedFrames: false,
    });
    expect(activeVerdict).toBe("UNKNOWN");

    releaseBarrier();
    await pending;

    const settledVerdict = observePaneLiveness("session-real-barrier", {
      readerPaused: false,
      kernelStopped: false,
      suspended: false,
      hasUnpresentedFrames: false,
    });
    expect(settledVerdict).toBe("IDLE");
  });

  it("prioritizes held execution over queue backlog and asserts explicit ages with controlled clock", async () => {
    vi.useFakeTimers();
    try {
      let resolveFirst: (v: string) => void = () => {};
      const firstDeferred = new Promise<string>((res) => {
        resolveFirst = res;
      });

      const p1 = terminalInputQueue.enqueue("sess-precedence", null, 10, async () => {
        return await firstDeferred;
      });

      vi.advanceTimersByTime(10);

      const p2 = terminalInputQueue.enqueue("sess-precedence", null, 10, async () => {
        return "second";
      });

      vi.advanceTimersByTime(300);

      const runningAge = terminalInputQueue.getRunningAgeMs("sess-precedence");
      const queuedHeadAge = terminalInputQueue.getQueuedHeadAgeMs("sess-precedence");

      expect(runningAge).toBe(310);
      expect(queuedHeadAge).toBe(300);
      expect(queuedHeadAge).toBeGreaterThan(250);

      const inFlightVerdict = observePaneLiveness("sess-precedence");
      expect(inFlightVerdict).toBe("BLOCKED_IN_IPC_WRITE");

      resolveFirst("first");
      await p1;

      expect(terminalInputQueue.getQueuedHeadAgeMs("sess-precedence")).toBeNull();
      await p2;
    } finally {
      vi.useRealTimers();
    }
  });
});
