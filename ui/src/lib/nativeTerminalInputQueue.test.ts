import { describe, expect, it, vi, beforeEach } from "vitest";
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
});
