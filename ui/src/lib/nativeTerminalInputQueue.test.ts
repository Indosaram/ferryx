import { describe, expect, it, vi, beforeEach } from "vitest";
import * as switchDebugModule from "./switchDebug";
import {
  NativeTerminalInputQueueManager,
  NativeTerminalQueueOverflowError,
  NativeTerminalStaleGenerationError,
  recordTerminalInputDrop,
  getTerminalInputDropCount,
  getTerminalInputDropTotals,
  subscribeTerminalInputDrop,
  resetTerminalInputDropCountsForTest,
  type TerminalInputDropReason,
} from "./nativeTerminalInputQueue";

function createDeferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("NativeTerminalInputQueueManager", () => {
  let queue: NativeTerminalInputQueueManager;

  beforeEach(() => {
    queue = new NativeTerminalInputQueueManager({
      maxQueueBytes: 1024,
      maxQueueEntries: 4,
    });
  });

  it("reports the age of the in-flight input when the queue overflows", async () => {
    vi.useFakeTimers();
    try {
      const pending = createDeferred();
      const first = queue.enqueue("blocked", null, 1, () => pending.promise);
      expect(queue.getRunningAgeMs("blocked")).toBe(0);
      vi.advanceTimersByTime(125);
      expect(queue.getRunningAgeMs("blocked")).toBe(125);

      const waiting = Array.from({ length: 3 }, () =>
        queue.enqueue("blocked", null, 1, async () => undefined),
      );
      await expect(queue.enqueue("blocked", null, 1, async () => undefined))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      expect(queue.getQueuedCount("blocked")).toBe(4);

      pending.resolve();
      await first;
      await Promise.all(waiting);
      expect(queue.getRunningAgeMs("blocked")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("executes tasks strictly in FIFO order per session without concurrent overlap", async () => {
    const executionOrder: string[] = [];
    let inFlightCount = 0;
    let maxConcurrent = 0;

    const d1 = createDeferred<void>();
    const d2 = createDeferred<void>();
    const d3 = createDeferred<void>();

    const task1 = async () => {
      inFlightCount += 1;
      maxConcurrent = Math.max(maxConcurrent, inFlightCount);
      executionOrder.push("start:op1");
      await d1.promise;
      executionOrder.push("end:op1");
      inFlightCount -= 1;
      return "op1";
    };

    const task2 = async () => {
      inFlightCount += 1;
      maxConcurrent = Math.max(maxConcurrent, inFlightCount);
      executionOrder.push("start:op2");
      await d2.promise;
      executionOrder.push("end:op2");
      inFlightCount -= 1;
      return "op2";
    };

    const task3 = async () => {
      inFlightCount += 1;
      maxConcurrent = Math.max(maxConcurrent, inFlightCount);
      executionOrder.push("start:op3");
      await d3.promise;
      executionOrder.push("end:op3");
      inFlightCount -= 1;
      return "op3";
    };

    const p1 = queue.enqueue("session-1", 1, 10, task1);
    const p2 = queue.enqueue("session-1", 1, 10, task2);
    const p3 = queue.enqueue("session-1", 1, 10, task3);

    d1.resolve();
    await p1;

    d2.resolve();
    await p2;

    d3.resolve();
    await p3;

    expect(maxConcurrent).toBe(1);
    expect(executionOrder).toEqual([
      "start:op1",
      "end:op1",
      "start:op2",
      "end:op2",
      "start:op3",
      "end:op3",
    ]);
  });

  it("isolates queues across different sessions without head-of-line blocking", async () => {
    const started: string[] = [];
    const d1Started = createDeferred<void>();
    const d2Started = createDeferred<void>();
    const d1Finish = createDeferred<void>();
    const d2Finish = createDeferred<void>();

    const task1 = async () => {
      started.push("s1");
      d1Started.resolve();
      await d1Finish.promise;
      return "s1-done";
    };

    const task2 = async () => {
      started.push("s2");
      d2Started.resolve();
      await d2Finish.promise;
      return "s2-done";
    };

    const p1 = queue.enqueue("session-1", 1, 10, task1);
    const p2 = queue.enqueue("session-2", 1, 10, task2);

    await Promise.all([d1Started.promise, d2Started.promise]);
    expect(started).toEqual(["s1", "s2"]);

    d2Finish.resolve();
    const res2 = await p2;
    expect(res2).toBe("s2-done");

    d1Finish.resolve();
    const res1 = await p1;
    expect(res1).toBe("s1-done");
  });

  it("enforces byte-bounded queue limit and explicitly rejects with overflow error", async () => {
    const d1 = createDeferred<void>();
    const d2 = createDeferred<void>();

    const p1 = queue.enqueue("session-1", 1, 600, () => d1.promise);
    expect(queue.getQueuedBytes("session-1")).toBe(600);

    const p2 = queue.enqueue("session-1", 1, 400, () => d2.promise);
    expect(queue.getQueuedBytes("session-1")).toBe(1000);

    await expect(
      queue.enqueue("session-1", 1, 50, async () => "overflow"),
    ).rejects.toThrow(NativeTerminalQueueOverflowError);

    d1.resolve();
    await p1;
    expect(queue.getQueuedBytes("session-1")).toBe(400);

    d2.resolve();
    await p2;
    expect(queue.getQueuedBytes("session-1")).toBe(0);

    const p3 = await queue.enqueue("session-1", 1, 200, async () => "p3-done");
    expect(p3).toBe("p3-done");
  });

  it("enforces entry-bounded queue limit and rejects when entries exceed maxQueueEntries", async () => {
    const d1 = createDeferred<void>();
    const d2 = createDeferred<void>();
    const d3 = createDeferred<void>();
    const d4 = createDeferred<void>();

    const p1 = queue.enqueue("session-1", 1, 10, () => d1.promise);
    const p2 = queue.enqueue("session-1", 1, 10, () => d2.promise);
    const p3 = queue.enqueue("session-1", 1, 10, () => d3.promise);
    const p4 = queue.enqueue("session-1", 1, 10, () => d4.promise);

    await expect(
      queue.enqueue("session-1", 1, 10, async () => "overflow-5"),
    ).rejects.toThrow(NativeTerminalQueueOverflowError);

    d1.resolve();
    d2.resolve();
    d3.resolve();
    d4.resolve();
    await Promise.all([p1, p2, p3, p4]);
  });

  it("preserves original structured error object unchanged without stringifying", async () => {
    const structuredError = {
      code: "INTERNAL_ERROR",
      message: "Remote control is busy",
      details: { kind: "busy", inputWritten: false },
    };

    let caughtError: unknown;
    try {
      await queue.enqueue("session-1", 1, 10, async () => {
        throw structuredError;
      });
    } catch (err) {
      caughtError = err;
    }

    expect(caughtError).toBe(structuredError);
  });

  it("explicitly rejects queued old-generation operations when generation advances", async () => {
    const d1 = createDeferred<void>();
    const op2Spy = vi.fn(async () => "op2-dispatched");

    const p1 = queue.enqueue("session-1", 1, 10, () => d1.promise);
    const p2 = queue.enqueue("session-1", 1, 10, op2Spy);

    queue.invalidateOldGenerations("session-1", 2);

    d1.resolve();
    await p1;

    await expect(p2).rejects.toThrow(NativeTerminalStaleGenerationError);
    expect(op2Spy).not.toHaveBeenCalled();
    expect(queue.getQueuedBytes("session-1")).toBe(0);
  });

  it("clearing a session cancels pending admissions without corrupting in-flight completion accounting", async () => {
    const d1 = createDeferred<string>();
    const pendingTask = vi.fn(async () => "never");

    const p1 = queue.enqueue("session-1", 1, 10, () => d1.promise);
    const p2 = queue.enqueue("session-1", 1, 20, pendingTask);
    expect(queue.getQueuedBytes("session-1")).toBe(30);

    queue.clear("session-1");
    expect(queue.getQueuedBytes("session-1")).toBe(10);

    d1.resolve("done");
    const res1 = await p1;
    expect(res1).toBe("done");
    expect(queue.getQueuedBytes("session-1")).toBe(0);

    await expect(p2).rejects.toThrow("Terminal input queue cleared");
    expect(pendingTask).not.toHaveBeenCalled();
  });

  it("retries a recovered item before later items so keystrokes keep their order", async () => {
    const writes: string[] = [];
    const recovery = createDeferred<boolean>();
    let failA = true;
    const send = (text: string) => async () => {
      if (text === "A" && failA) {
        failA = false;
        throw new Error("not written");
      }
      writes.push(text);
      return text;
    };
    const a = queue.enqueue("session-1", null, 1, send("A"), undefined, "input", undefined, {
      recover: () => recovery.promise,
    });
    const b = queue.enqueue("session-1", null, 1, send("B"));
    const c = queue.enqueue("session-1", null, 1, send("C"));
    await Promise.resolve();
    expect(writes).toEqual([]);
    recovery.resolve(true);
    await Promise.all([a, b, c]);
    expect(writes).toEqual(["A", "B", "C"]);
  });

  it("bounds wheel/mouse items separately so they never evict keystrokes", async () => {
    const blocked = createDeferred<void>();
    const pointers = [queue.enqueue("session-1", null, 32, () => blocked.promise, undefined, "wheel", undefined, { pointer: true })];
    for (let index = 1; index < 4; index += 1) {
      pointers.push(queue.enqueue("session-1", null, 32, async () => undefined, undefined, "wheel", undefined, { pointer: true }));
    }
    await expect(queue.enqueue("session-1", null, 32, async () => undefined, undefined, "wheel", undefined, { pointer: true }))
      .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
    const keys = Array.from({ length: 4 }, (_, index) => queue.enqueue("session-1", null, 1, async () => index));
    expect(queue.getQueuedCount("session-1")).toBe(4);
    blocked.resolve();
    await Promise.all([...pointers, ...keys]);
  });

  it("drops an idle session's state when it is cleared", async () => {
    const pending = createDeferred<void>();
    const running = queue.enqueue("session-1", 3, 1, () => pending.promise);
    queue.invalidateOldGenerations("session-1", 3);
    queue.clear("session-1");
    pending.resolve();
    await running;
    // A fresh entry has no remembered generation: old-generation input is admitted again.
    await expect(queue.enqueue("session-1", 1, 1, async () => "fresh")).resolves.toBe("fresh");
  });

  it("delivers a preedit update enqueued while input send is in flight strictly after that input", async () => {
    const deliveryOrder: string[] = [];
    const inputDeferred = createDeferred<string>();
    const preeditDeferred = createDeferred<string>();

    const inputTask = async () => {
      deliveryOrder.push("start:input");
      await inputDeferred.promise;
      deliveryOrder.push("end:input");
      return "input-done";
    };

    const preeditTask = async () => {
      deliveryOrder.push("start:preedit");
      await preeditDeferred.promise;
      deliveryOrder.push("end:preedit");
      return "preedit-done";
    };

    const inputPromise = queue.enqueue("session-1", 1, 10, inputTask);
    const preeditPromise = queue.enqueuePreedit("session-1", 1, 10, preeditTask);

    expect(deliveryOrder).toEqual(["start:input"]);

    inputDeferred.resolve("done");
    await inputPromise;

    expect(deliveryOrder).toEqual(["start:input", "end:input", "start:preedit"]);

    preeditDeferred.resolve("done");
    await preeditPromise;

    expect(deliveryOrder).toEqual([
      "start:input",
      "end:input",
      "start:preedit",
      "end:preedit",
    ]);
  });

  it("coalesces consecutive preedit updates into one delivered call", async () => {
    const inFlightDeferred = createDeferred<void>();
    const preedit1Spy = vi.fn(async () => "preedit-1");
    const preedit2Spy = vi.fn(async () => "preedit-2");
    const preedit3Spy = vi.fn(async () => "preedit-3");

    const inputPromise = queue.enqueue("session-1", 1, 10, () => inFlightDeferred.promise);

    const p1 = queue.enqueuePreedit("session-1", 1, 10, preedit1Spy);
    const p2 = queue.enqueuePreedit("session-1", 1, 10, preedit2Spy);
    const p3 = queue.enqueuePreedit("session-1", 1, 10, preedit3Spy);

    expect(preedit1Spy).not.toHaveBeenCalled();
    expect(preedit2Spy).not.toHaveBeenCalled();
    expect(preedit3Spy).not.toHaveBeenCalled();

    inFlightDeferred.resolve();
    await inputPromise;

    const [r1, r2, r3] = await Promise.all([p1, p2, p3]);

    expect(preedit1Spy).not.toHaveBeenCalled();
    expect(preedit2Spy).not.toHaveBeenCalled();
    expect(preedit3Spy).toHaveBeenCalledTimes(1);
    expect(r3).toBe("preedit-3");
    expect(r1).toBe("preedit-3");
    expect(r2).toBe("preedit-3");
  });

  it("increments each drop reason counter exactly once for its scenario", () => {
    resetTerminalInputDropCountsForTest();
    expect(getTerminalInputDropCount()).toBe(0);

    const reasons: TerminalInputDropReason[] = [
      "dropped",
      "outage",
      "quarantined",
      "overflow",
      "stale-generation",
    ];

    const observed: string[] = [];
    const unsubscribe = subscribeTerminalInputDrop((reason) => {
      observed.push(reason);
    });

    for (let i = 0; i < reasons.length; i++) {
      const reason = reasons[i];
      recordTerminalInputDrop(reason);
      expect(getTerminalInputDropCount(reason)).toBe(1);
      expect(getTerminalInputDropCount()).toBe(i + 1);
    }

    expect(observed).toEqual(reasons);
    expect(getTerminalInputDropTotals()).toEqual({
      dropped: 1,
      outage: 1,
      quarantined: 1,
      overflow: 1,
      "stale-generation": 1,
    });

    unsubscribe();
    recordTerminalInputDrop("dropped");
    expect(getTerminalInputDropCount("dropped")).toBe(2);
    expect(observed).toEqual(reasons);
  });

  it("does not dispatch a committed input until the unresolved preedit settles", async () => {
    const deliveryOrder: string[] = [];
    const preeditDeferred = createDeferred<string>();
    const inputDeferred = createDeferred<string>();

    const preeditPromise = queue.enqueuePreedit("session-1", 1, 10, async () => {
      deliveryOrder.push("start:preedit");
      await preeditDeferred.promise;
      deliveryOrder.push("end:preedit");
      return "preedit-done";
    });
    const inputPromise = queue.enqueue("session-1", 1, 10, async () => {
      deliveryOrder.push("start:input");
      await inputDeferred.promise;
      return "input-done";
    });

    expect(deliveryOrder).toEqual(["start:preedit"]);

    preeditDeferred.resolve("done");
    await preeditPromise;
    await Promise.resolve();

    expect(deliveryOrder).toEqual(["start:preedit", "end:preedit", "start:input"]);

    inputDeferred.resolve("done");
    await inputPromise;
  });

  it("explicitly settles displaced preedit and records drop when replacement preedit exceeds queue capacity", async () => {
    resetTerminalInputDropCountsForTest();
    expect(getTerminalInputDropCount("overflow")).toBe(0);

    const oldPreeditDeferred = createDeferred<string>();
    const oldPreeditTask = async () => {
      await oldPreeditDeferred.promise;
      return "old-preedit";
    };

    // Block the executing slot so the preedit is queued and waiting.
    const inFlightDeferred = createDeferred<void>();
    const inFlightPromise = queue.enqueue("session-1", 1, 10, () => inFlightDeferred.promise);

    // Enqueue the initial preedit into the waiting queue.
    const oldPreeditPromise = queue.enqueuePreedit("session-1", 1, 10, oldPreeditTask);
    expect(queue.getQueuedBytes("session-1")).toBe(20);

    // Enqueue an oversized replacement preedit exceeding maxQueueBytes (1024 bytes).
    const oversizedPromise = queue.enqueuePreedit("session-1", 1, 2000, async () => "oversized");

    // The oversized replacement must reject with NativeTerminalQueueOverflowError.
    await expect(oversizedPromise).rejects.toThrow(NativeTerminalQueueOverflowError);

    // The displaced old preedit promise must be explicitly settled (rejected with NativeTerminalQueueOverflowError).
    await expect(oldPreeditPromise).rejects.toThrow(NativeTerminalQueueOverflowError);

    // A drop must be recorded in the terminal input drop accounting.
    expect(getTerminalInputDropCount("overflow")).toBeGreaterThanOrEqual(1);

    // Cleanly finish in-flight task and confirm queue drops to 0 allocated bytes.
    inFlightDeferred.resolve();
    await inFlightPromise;
    expect(queue.getQueuedBytes("session-1")).toBe(0);
  });
  it("maintains isolated per-lane in-flight request ID and running age when input and preedit overlap", async () => {
    vi.useFakeTimers();
    try {
      const inputDeferred = createDeferred<string>();
      const preeditDeferred = createDeferred<string>();

      let capturedInputReqId = "";
      let capturedPreeditReqId = "";

      const inputPromise = queue.enqueue("session-concurrent", 1, 10, async (reqId) => {
        capturedInputReqId = reqId;
        await inputDeferred.promise;
        return "input-done";
      });

      expect(capturedInputReqId).toMatch(/^req-[a-zA-Z0-9_-]+-session-concurrent-\d+$/);
      expect(queue.getInFlightRequestId("session-concurrent")).toBe(capturedInputReqId);
      expect(queue.getRunningAgeMs("session-concurrent")).toBe(0);

      vi.advanceTimersByTime(50);
      expect(queue.getRunningAgeMs("session-concurrent")).toBe(50);

      const preeditPromise = queue.enqueuePreedit("session-concurrent", 1, 10, async (reqId) => {
        capturedPreeditReqId = reqId;
        await preeditDeferred.promise;
        return "preedit-done";
      });

      // While input is in flight, serialized pump keeps preedit queued
      expect(capturedPreeditReqId).toBe("");
      expect(queue.getInFlightRequestId("session-concurrent")).toBe(capturedInputReqId);

      vi.advanceTimersByTime(25);
      expect(queue.getRunningAgeMs("session-concurrent")).toBe(75);

      // Settle input: pump now dispatches the queued preedit operation
      inputDeferred.resolve("done");
      await inputPromise;

      expect(capturedPreeditReqId).toMatch(/^req-preedit-[a-zA-Z0-9_-]+-session-concurrent-\d+$/);
      expect(capturedInputReqId).not.toBe(capturedPreeditReqId);

      // Now preedit is in flight: request ID points to preedit and running age tracks preedit
      expect(queue.getInFlightRequestId("session-concurrent")).toBe(capturedPreeditReqId);
      expect(queue.getRunningAgeMs("session-concurrent")).toBe(0);

      vi.advanceTimersByTime(40);
      expect(queue.getRunningAgeMs("session-concurrent")).toBe(40);

      // Completing preedit lane clears remaining in-flight state cleanly
      preeditDeferred.resolve("done");
      await preeditPromise;

      expect(queue.getInFlightRequestId("session-concurrent")).toBeNull();
      expect(queue.getRunningAgeMs("session-concurrent")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });


  it("emits bounded slow in-flight diagnostics and correlated request id when operation stalls", async () => {
    vi.useFakeTimers();
    try {
      const debugSpy = vi.spyOn(switchDebugModule, "switchDebug");
      debugSpy.mockClear();

      const slowDeferred = createDeferred<string>();
      let receivedRequestId = "";
      const slowPromise = queue.enqueue("session-slow", 1, 10, async (reqId) => {
        receivedRequestId = reqId;
        return slowDeferred.promise;
      }, undefined, "mouse.Press");

      expect(receivedRequestId).toMatch(/^req-[a-zA-Z0-9_-]+-session-slow-\d+$/);
      expect(queue.getInFlightRequestId("session-slow")).toBe(receivedRequestId);

      // Advance time before threshold (200ms < 250ms)
      vi.advanceTimersByTime(200);
      expect(debugSpy).not.toHaveBeenCalledWith(
        "terminal.surface.input.in_flight_slow",
        expect.anything(),
      );

      // Advance past threshold (250ms)
      vi.advanceTimersByTime(50);
      expect(debugSpy).toHaveBeenCalledWith(
        "terminal.surface.input.in_flight_slow",
        expect.objectContaining({
          backendSessionId: "session-slow",
          inFlightRequestId: receivedRequestId,
          operation: "mouse.Press",
          phase: "pending_ipc",
        }),
      );

      // Enqueue a second item while head is blocked
      const secondDeferred = createDeferred<string>();
      const secondPromise = queue.enqueue("session-slow", 1, 10, async () => secondDeferred.promise);

      expect(debugSpy).toHaveBeenCalledWith(
        "terminal.surface.input.in_flight_slow",
        expect.objectContaining({
          backendSessionId: "session-slow",
          inFlightRequestId: receivedRequestId,
          trigger: "enqueue_head_blocked",
        }),
      );

      // Enqueue a third item immediately: rate-limited, must NOT re-emit enqueue_head_blocked
      debugSpy.mockClear();
      const thirdDeferred = createDeferred<string>();
      const thirdPromise = queue.enqueue("session-slow", 1, 10, async () => thirdDeferred.promise);
      expect(debugSpy).not.toHaveBeenCalledWith(
        "terminal.surface.input.in_flight_slow",
        expect.objectContaining({ trigger: "enqueue_head_blocked" }),
      );

      // Settle the slow first operation
      slowDeferred.resolve("first-done");
      await slowPromise;

      expect(debugSpy).toHaveBeenCalledWith(
        "terminal.surface.input.slow",
        expect.objectContaining({
          backendSessionId: "session-slow",
          requestId: receivedRequestId,
        }),
      );

      // Verify FIFO: second operation was in flight before third
      secondDeferred.resolve("second-done");
      await secondPromise;
      thirdDeferred.resolve("third-done");
      await thirdPromise;

      expect(queue.getInFlightRequestId("session-slow")).toBeNull();
      debugSpy.mockRestore();
    } finally {
      vi.useRealTimers();
    }
  });

  it("propagates custom requestId and operationName through enqueue and reports custom ID during in-flight execution", async () => {
    const d1 = createDeferred<string>();
    let receivedReqId = "";

    const promise = queue.enqueue(
      "session-custom",
      1,
      64,
      async (reqId) => {
        receivedReqId = reqId;
        await d1.promise;
        return "custom-done";
      },
      "custom-req-mouse-down-1",
      "mouse.down",
    );

    expect(receivedReqId).toBe("custom-req-mouse-down-1");
    expect(queue.getInFlightRequestId("session-custom")).toBe("custom-req-mouse-down-1");
    expect(queue.getRunningAgeMs("session-custom")).toBeGreaterThanOrEqual(0);

    d1.resolve("done");
    const result = await promise;
    expect(result).toBe("custom-done");
    expect(queue.getInFlightRequestId("session-custom")).toBeNull();
    expect(queue.getRunningAgeMs("session-custom")).toBeNull();
  });

  it("resets in-flight request ID and running age to null when in-flight operation rejects and proceeds to next queued operation", async () => {
    const op1Deferred = createDeferred<void>();
    const op2Deferred = createDeferred<string>();
    let op2ReqId = "";

    const expectedError = new Error("PTY write rejected");

    const p1 = queue.enqueue("session-fail", 1, 10, async () => {
      await op1Deferred.promise;
      throw expectedError;
    });
    const p2 = queue.enqueue("session-fail", 1, 10, async (reqId) => {
      op2ReqId = reqId;
      await op2Deferred.promise;
      return "op2-recovered";
    });

    expect(queue.getInFlightRequestId("session-fail")).toBeTruthy();
    expect(queue.getRunningAgeMs("session-fail")).toBeGreaterThanOrEqual(0);

    // Reject op1
    op1Deferred.reject(expectedError);
    await expect(p1).rejects.toThrow("PTY write rejected");

    // Queue immediately advances to op2, with op2's request ID in flight
    expect(op2ReqId).toBeTruthy();
    expect(queue.getInFlightRequestId("session-fail")).toBe(op2ReqId);

    // Resolve op2 and verify complete settlement to null
    op2Deferred.resolve("ok");
    const res2 = await p2;
    expect(res2).toBe("op2-recovered");
    expect(queue.getInFlightRequestId("session-fail")).toBeNull();
    expect(queue.getRunningAgeMs("session-fail")).toBeNull();
  });

  it("preserves independent in-flight tracking across distinct sessions during concurrent progress and adversarial stale generation rejection", async () => {
    vi.useFakeTimers();
    try {
      const dA1 = createDeferred<string>();
      const dA2Spy = vi.fn(async () => "A2-dispatched");
      const dB1 = createDeferred<string>();

      let reqA1 = "";
      let reqB1 = "";

      const pA1 = queue.enqueue("session-A", 1, 10, async (reqId) => {
        reqA1 = reqId;
        await dA1.promise;
        return "A1-done";
      });
      const pA2 = queue.enqueue("session-A", 1, 10, dA2Spy);
      const pB1 = queue.enqueue("session-B", 1, 10, async (reqId) => {
        reqB1 = reqId;
        await dB1.promise;
        return "B1-done";
      });

      vi.advanceTimersByTime(30);
      expect(queue.getInFlightRequestId("session-A")).toBe(reqA1);
      expect(queue.getRunningAgeMs("session-A")).toBe(30);
      expect(queue.getInFlightRequestId("session-B")).toBe(reqB1);
      expect(queue.getRunningAgeMs("session-B")).toBe(30);

      // Adversarial generation advancement on session-A invalidates queued pA2
      queue.invalidateOldGenerations("session-A", 2);
      await expect(pA2).rejects.toThrow(NativeTerminalStaleGenerationError);
      expect(dA2Spy).not.toHaveBeenCalled();

      // In-flight op A1 and Session B remain unaffected
      expect(queue.getInFlightRequestId("session-A")).toBe(reqA1);
      expect(queue.getInFlightRequestId("session-B")).toBe(reqB1);

      // Synchronous stale enqueue rejection on session-A
      await expect(
        queue.enqueue("session-A", 0, 10, async () => "old-sync"),
      ).rejects.toThrow(NativeTerminalStaleGenerationError);

      // Settle session A
      dA1.resolve("A1");
      await pA1;
      expect(queue.getInFlightRequestId("session-A")).toBeNull();
      expect(queue.getRunningAgeMs("session-A")).toBeNull();

      // Session B remains in flight with continuous running age
      expect(queue.getInFlightRequestId("session-B")).toBe(reqB1);
      expect(queue.getRunningAgeMs("session-B")).toBe(30);

      // Settle session B
      dB1.resolve("B1");
      await pB1;
      expect(queue.getInFlightRequestId("session-B")).toBeNull();
      expect(queue.getRunningAgeMs("session-B")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("distinguishes in-flight execution age from waiting queued head age", async () => {
    vi.useFakeTimers();
    try {
      const d1 = createDeferred<string>();
      const d2 = createDeferred<string>();

      let req1 = "";
      let req2 = "";

      const p1 = queue.enqueue("sess-queue-age", 1, 10, async (id) => {
        req1 = id;
        await d1.promise;
        return "first";
      });

      vi.advanceTimersByTime(20);

      const p2 = queue.enqueue("sess-queue-age", 1, 10, async (id) => {
        req2 = id;
        await d2.promise;
        return "second";
      }, undefined, "input", {
        paneIdentity: "pane-1",
        bindingKey: "bk-1",
      });

      vi.advanceTimersByTime(50);

      const queuedId = queue.getHeadQueuedRequestId("sess-queue-age");
      expect(queuedId).toBeTruthy();
      expect(queue.getInFlightRequestId("sess-queue-age")).toBe(req1);
      expect(queue.getRunningAgeMs("sess-queue-age")).toBe(70);
      expect(queue.getQueuedHeadAgeMs("sess-queue-age")).toBe(50);

      d1.resolve("first");
      await p1;

      expect(queue.getInFlightRequestId("sess-queue-age")).toBe(queuedId);
      expect(queue.getInFlightRequestId("sess-queue-age")).toBe(req2);
      expect(queue.getHeadQueuedRequestId("sess-queue-age")).toBeNull();
      expect(queue.getQueuedHeadAgeMs("sess-queue-age")).toBeNull();

      d2.resolve("second");
      await p2;

      expect(queue.getInFlightRequestId("sess-queue-age")).toBeNull();
      expect(queue.getRunningAgeMs("sess-queue-age")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("tracks in-flight request age and repeated drop accounting accurately during stalls", async () => {
    vi.useFakeTimers();
    try {
      const queue = new NativeTerminalInputQueueManager({
        maxQueueEntries: 2,
        maxQueueBytes: 64,
      });
      const sessionId = "session-stall-test";
      const debugSpy = vi.spyOn(switchDebugModule, "switchDebug").mockImplementation(() => null);

      const inFlightDeferred = createDeferred<string>();
      let inFlightRequestId = "";
      const inFlightPromise = queue.enqueue(sessionId, null, 10, async (reqId) => {
        inFlightRequestId = reqId;
        return inFlightDeferred.promise;
      });

      await vi.advanceTimersByTimeAsync(0);
      expect(queue.getInFlightRequestId(sessionId)).toBe(inFlightRequestId);
      expect(queue.getRunningAgeMs(sessionId)).toBe(0);

      await vi.advanceTimersByTimeAsync(300);
      expect(queue.getRunningAgeMs(sessionId)).toBe(300);

      const secondDeferred = createDeferred<string>();
      const secondPromise = queue.enqueue(sessionId, null, 10, async () => {
        return secondDeferred.promise;
      });

      let overflowError: unknown = null;
      try {
        await queue.enqueue(sessionId, null, 10, async () => "overflow");
      } catch (err) {
        overflowError = err;
      }
      expect(overflowError).toBeInstanceOf(NativeTerminalQueueOverflowError);
      expect(queue.getRunningAgeMs(sessionId)).toBe(300);
      expect(queue.getInFlightRequestId(sessionId)).toBe(inFlightRequestId);

      inFlightDeferred.resolve("done-1");
      await inFlightPromise;

      secondDeferred.resolve("done-2");
      await secondPromise;

      expect(queue.getInFlightRequestId(sessionId)).toBeNull();
      expect(queue.getRunningAgeMs(sessionId)).toBeNull();
      debugSpy.mockRestore();
    } finally {
      vi.useRealTimers();
    }
  });

  describe("printable text coalescing at the entry cap", () => {
    function textSender(sent: string[], head?: Promise<void>) {
      return async (text: string) => {
        sent.push(text);
        if (head && sent.length === 1) await head;
        return text;
      };
    }
    function enqueueText(
      sessionId: string,
      generation: number | null,
      text: string,
      send: (text: string, requestId: string) => Promise<unknown>,
    ) {
      return queue.enqueue(sessionId, generation, new TextEncoder().encode(text).length,
        (requestId) => send(text, requestId), undefined, "input", undefined,
        { coalesceText: { text, send } });
    }

    it("appends text to the queued tail at the cap and never exceeds the entry bound", async () => {
      const head = createDeferred<void>();
      const sent: string[] = [];
      const send = textSender(sent, head.promise);
      const burst = "abcdefghij";
      const results = Array.from(burst, (key) => enqueueText("s", null, key, send));
      // Cap is 4: one in flight, three queued; the queued tail absorbs keys 5..10.
      expect(queue.getQueuedCount("s")).toBe(4);
      expect(queue.getQueuedBytes("s")).toBe(burst.length);
      head.resolve();
      await Promise.all(results);
      expect(sent).toEqual(["a", "b", "c", "defghij"]);
      expect(sent.join("")).toBe(burst);
      expect(queue.getQueuedCount("s")).toBe(0);
      expect(queue.getQueuedBytes("s")).toBe(0);
    });

    it("does not merge across a non-text tail and keeps rejecting at the cap", async () => {
      const head = createDeferred<void>();
      const order: string[] = [];
      const send = async (text: string) => {
        order.push(text);
        if (order.length === 1) await head.promise;
        return text;
      };
      const items = [
        enqueueText("s", null, "a", send),
        enqueueText("s", null, "b", send),
        enqueueText("s", null, "c", send),
        queue.enqueue("s", null, 32, async () => {
          order.push("<Enter>");
        }),
      ];
      await expect(enqueueText("s", null, "d", send)).rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      head.resolve();
      await Promise.all(items);
      expect(order).toEqual(["a", "b", "c", "<Enter>"]);
    });

    it("does not merge non-coalescable input into a text tail", async () => {
      const head = createDeferred<void>();
      const sent: string[] = [];
      const send = textSender(sent, head.promise);
      const items = ["a", "b", "c", "d"].map((key) => enqueueText("s", null, key, send));
      const control = vi.fn(async () => undefined);
      await expect(queue.enqueue("s", null, 1, control)).rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      head.resolve();
      await Promise.all(items);
      expect(control).not.toHaveBeenCalled();
      expect(sent).toEqual(["a", "b", "c", "d"]);
    });

    it("does not merge across a generation change and rejects the merged keys with the stale tail", async () => {
      const head = createDeferred<void>();
      const sent: string[] = [];
      const send = textSender(sent, head.promise);
      const items = ["a", "b", "c", "d"].map((key) => enqueueText("s", 1, key, send));
      const mergedIntoTail = enqueueText("s", 1, "e", send);
      // A newer generation never merges into an older-generation tail.
      await expect(enqueueText("s", 2, "x", send)).rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      queue.invalidateOldGenerations("s", 2);
      await expect(items[1]).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      await expect(items[2]).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      await expect(items[3]).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      await expect(mergedIntoTail).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      head.resolve();
      await items[0];
      expect(sent).toEqual(["a"]);
      expect(queue.getQueuedBytes("s")).toBe(0);
    });

    it("still overflows when the merge would exceed the byte cap", async () => {
      const small = new NativeTerminalInputQueueManager({ maxQueueBytes: 8, maxQueueEntries: 2 });
      const head = createDeferred<void>();
      const sent: string[] = [];
      const send = textSender(sent, head.promise);
      const enqueue = (text: string) =>
        small.enqueue("s", null, text.length, (requestId) => send(text, requestId), undefined, "input",
          undefined, { coalesceText: { text, send } });
      const items = [enqueue("a"), enqueue("bc"), enqueue("defg")];
      expect(small.getQueuedBytes("s")).toBe(7);
      await expect(enqueue("hi")).rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      expect(small.getQueuedBytes("s")).toBe(7);
      head.resolve();
      await Promise.all(items);
      expect(sent).toEqual(["a", "bcdefg"]);
    });

    it("retries a recovered coalesced item with its whole merged text before later items", async () => {
      const head = createDeferred<void>();
      const sent: string[] = [];
      let failOnce = true;
      const send = async (text: string) => {
        if (text === "de" && failOnce) {
          failOnce = false;
          throw new Error("not written");
        }
        sent.push(text);
        if (sent.length === 1) await head.promise;
        return text;
      };
      const enqueue = (text: string) =>
        queue.enqueue("s", null, 1, (requestId) => send(text, requestId), undefined, "input", undefined, {
          coalesceText: { text, send },
          recover: async () => true,
        });
      const items = ["a", "b", "c", "d", "e"].map(enqueue);
      head.resolve();
      await Promise.all(items);
      // Cap 4: "e" merges into the queued tail "d"; the recovered retry resends "de" whole.
      expect(sent).toEqual(["a", "b", "c", "de"]);
    });
  });

  describe("paste and control lanes", () => {
    const PASTE = { lane: "paste" } as const;
    const CONTROL = { lane: "control" } as const;

    function enqueuePaste<T>(target: NativeTerminalInputQueueManager, generation: number | null, bytes: number, op: () => Promise<T>) {
      return target.enqueue("s", generation, bytes, op, undefined, "paste", undefined, PASTE);
    }
    function enqueueControl<T>(target: NativeTerminalInputQueueManager, generation: number | null, op: () => Promise<T>) {
      return target.enqueue("s", generation, 1, op, undefined, "key", undefined, CONTROL);
    }

    it("resolves a control key before an unresolved paste while later text stays queued", async () => {
      const order: string[] = [];
      const paste = createDeferred<string>();
      let pasteSettled = false;
      const pastePromise = enqueuePaste(queue, 1, 900, async () => {
        order.push("paste");
        return paste.promise;
      }).finally(() => {
        pasteSettled = true;
      });
      const controlPromise = enqueueControl(queue, 1, async () => {
        order.push("ctrl-c");
        return "ctrl-c";
      });
      const text = vi.fn(async () => {
        order.push("text");
        return "text";
      });
      const textPromise = queue.enqueue("s", 1, 1, text);

      await expect(controlPromise).resolves.toBe("ctrl-c");
      expect(pasteSettled).toBe(false);
      expect(text).not.toHaveBeenCalled();
      expect(queue.getQueuedCount("s")).toBe(2);
      expect(queue.getQueuedBytes("s")).toBe(901);

      paste.resolve("pasted");
      await expect(pastePromise).resolves.toBe("pasted");
      await expect(textPromise).resolves.toBe("text");
      expect(order).toEqual(["paste", "ctrl-c", "text"]);
      expect(queue.getQueuedCount("s")).toBe(0);
      expect(queue.getQueuedBytes("s")).toBe(0);
    });

    it("serializes control keys among themselves during an in-flight paste", async () => {
      const order: string[] = [];
      const paste = createDeferred<void>();
      const first = createDeferred<void>();
      const pastePromise = enqueuePaste(queue, null, 10, () => paste.promise);
      const c1 = enqueueControl(queue, null, async () => {
        order.push("start:c1");
        await first.promise;
        order.push("end:c1");
      });
      const c2 = enqueueControl(queue, null, async () => {
        order.push("c2");
      });
      expect(order).toEqual(["start:c1"]);
      first.resolve();
      await c1;
      await c2;
      expect(order).toEqual(["start:c1", "end:c1", "c2"]);
      expect(queue.isRunning("s")).toBe(true);
      paste.resolve();
      await pastePromise;
      expect(queue.isRunning("s")).toBe(false);
    });

    it("does not let a control key overtake earlier queued text or in-flight ordinary input", async () => {
      const order: string[] = [];
      const paste = createDeferred<void>();
      const pastePromise = enqueuePaste(queue, null, 10, async () => {
        order.push("paste");
        await paste.promise;
      });
      const textPromise = queue.enqueue("s", null, 1, async () => {
        order.push("text");
      });
      const control = vi.fn(async () => {
        order.push("ctrl-c");
      });
      const controlPromise = enqueueControl(queue, null, control);
      expect(control).not.toHaveBeenCalled();
      paste.resolve();
      await Promise.all([pastePromise, textPromise, controlPromise]);
      expect(order).toEqual(["paste", "text", "ctrl-c"]);

      const ordinary = createDeferred<void>();
      const ordinaryPromise = queue.enqueue("s", null, 1, () => ordinary.promise);
      const control2 = vi.fn(async () => undefined);
      const control2Promise = enqueueControl(queue, null, control2);
      expect(control2).not.toHaveBeenCalled();
      ordinary.resolve();
      await Promise.all([ordinaryPromise, control2Promise]);
      expect(control2).toHaveBeenCalledTimes(1);
    });

    it("admits a 512 KiB paste by default and still admits control keys and typing beside it", async () => {
      const defaults = new NativeTerminalInputQueueManager();
      const paste = createDeferred<string>();
      const pastePromise = enqueuePaste(defaults, null, 512 * 1024, () => paste.promise);
      await expect(enqueuePaste(defaults, null, 1, async () => "more"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      await expect(enqueueControl(defaults, null, async () => "ctrl")).resolves.toBe("ctrl");
      const textPromise = defaults.enqueue("s", null, 1, async () => "t");
      expect(defaults.getQueuedBytes("s")).toBe(512 * 1024 + 1);
      paste.resolve("pasted");
      await expect(pastePromise).resolves.toBe("pasted");
      await expect(textPromise).resolves.toBe("t");
      expect(defaults.getQueuedBytes("s")).toBe(0);

      await expect(enqueuePaste(defaults, null, 512 * 1024 + 1, async () => "too big"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      // Ordinary input keeps its own 256 KiB byte bound.
      await expect(defaults.enqueue("s", null, 256 * 1024 + 1, async () => "too big"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      expect(defaults.getQueuedBytes("s")).toBe(0);
    });

    it("bounds control entries separately from a full ordinary queue", async () => {
      const paste = createDeferred<void>();
      const held = createDeferred<void>();
      const items: Promise<unknown>[] = [enqueuePaste(queue, null, 10, () => paste.promise)];
      for (let index = 0; index < 3; index += 1) {
        items.push(queue.enqueue("s", null, 1, async () => index));
      }
      await expect(queue.enqueue("s", null, 1, async () => "overflow"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      await expect(enqueuePaste(queue, null, 1, async () => "overflow"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);

      items.push(enqueueControl(queue, null, () => held.promise));
      for (let index = 0; index < 3; index += 1) {
        items.push(enqueueControl(queue, null, async () => index));
      }
      expect(queue.getQueuedCount("s")).toBe(8);
      await expect(enqueueControl(queue, null, async () => "overflow"))
        .rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);

      held.resolve();
      paste.resolve();
      await Promise.all(items);
      expect(queue.getQueuedCount("s")).toBe(0);
      expect(queue.getQueuedBytes("s")).toBe(0);
    });

    it("fences queued control keys and text by generation while paste and control are in flight", async () => {
      const paste = createDeferred<string>();
      const c1 = createDeferred<string>();
      const pastePromise = enqueuePaste(queue, 1, 10, () => paste.promise);
      const c1Promise = enqueueControl(queue, 1, () => c1.promise);
      const staleControl = vi.fn(async () => "stale");
      const staleControlPromise = enqueueControl(queue, 1, staleControl);
      const staleText = vi.fn(async () => "stale");
      const staleTextPromise = queue.enqueue("s", 1, 1, staleText);

      queue.invalidateOldGenerations("s", 2);
      await expect(staleControlPromise).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      await expect(staleTextPromise).rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      await expect(enqueueControl(queue, 1, async () => "old"))
        .rejects.toBeInstanceOf(NativeTerminalStaleGenerationError);
      expect(queue.getQueuedCount("s")).toBe(2);
      expect(queue.getQueuedBytes("s")).toBe(11);

      const fresh = enqueueControl(queue, 2, async () => "fresh");
      c1.resolve("c1");
      await expect(c1Promise).resolves.toBe("c1");
      await expect(fresh).resolves.toBe("fresh");
      paste.resolve("pasted");
      await expect(pastePromise).resolves.toBe("pasted");
      expect(staleControl).not.toHaveBeenCalled();
      expect(staleText).not.toHaveBeenCalled();
      expect(queue.getQueuedBytes("s")).toBe(0);
    });

    it("clear rejects queued control keys and text while in-flight paste and control finish their accounting", async () => {
      const paste = createDeferred<string>();
      const c1 = createDeferred<string>();
      const pastePromise = enqueuePaste(queue, 3, 10, () => paste.promise);
      const c1Promise = enqueueControl(queue, 3, () => c1.promise);
      const queuedControl = vi.fn(async () => "never");
      const queuedControlPromise = enqueueControl(queue, 3, queuedControl);
      const queuedTextPromise = queue.enqueue("s", 3, 1, async () => "never");
      queue.invalidateOldGenerations("s", 3);

      queue.clear("s");
      await expect(queuedControlPromise).rejects.toThrow("Terminal input queue cleared");
      await expect(queuedTextPromise).rejects.toThrow("Terminal input queue cleared");
      expect(queue.getQueuedBytes("s")).toBe(11);

      c1.resolve("c1");
      await expect(c1Promise).resolves.toBe("c1");
      paste.resolve("pasted");
      await expect(pastePromise).resolves.toBe("pasted");
      expect(queuedControl).not.toHaveBeenCalled();
      expect(queue.isRunning("s")).toBe(false);
      expect(queue.getQueuedBytes("s")).toBe(0);
      // The cleared session was dropped once idle: its remembered generation is gone.
      await expect(enqueueControl(queue, 1, async () => "fresh")).resolves.toBe("fresh");
    });

    it("never coalesces text admitted after a queued control key into an earlier text tail", async () => {
      const order: string[] = [];
      const paste = createDeferred<void>();
      const send = async (text: string) => {
        order.push(text);
        return text;
      };
      const enqueueText = (text: string) =>
        queue.enqueue("s", null, 1, () => send(text), undefined, "input", undefined, {
          coalesceText: { text, send },
        });
      const items: Promise<unknown>[] = [
        enqueuePaste(queue, null, 10, async () => {
          order.push("paste");
          await paste.promise;
        }),
        enqueueText("a"),
        enqueueText("b"),
        enqueueText("c"),
        enqueueControl(queue, null, async () => {
          order.push("ctrl-c");
        }),
      ];
      await expect(enqueueText("d")).rejects.toBeInstanceOf(NativeTerminalQueueOverflowError);
      paste.resolve();
      await Promise.all(items);
      expect(order).toEqual(["paste", "a", "b", "c", "ctrl-c"]);
    });
  });
});
