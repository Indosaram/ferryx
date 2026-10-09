import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./switchDebug", () => ({ switchDebug: vi.fn() }));

import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  emitNativeTerminalPresentation,
  presentNativeTerminalLifecycle,
  resetNativeTerminalLifecycleForTest,
  subscribeNativeTerminalPresentation,
  type NativeTerminalPresentationReceipt,
  type NativeTerminalPresentationSubscription,
} from "./nativeTerminalLifecycle";

function deferred() {
  let resolve = () => {};
  let reject = (_error: Error) => {};
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("nativeTerminalLifecycle sibling pane ownership", () => {
  beforeEach(resetNativeTerminalLifecycleForTest);

  it("waits for native readiness when a pending attachment is reused", async () => {
    const native = deferred();
    const attach = attachNativeTerminalLifecycle("pending", () => native.promise);
    const detach = detachNativeTerminalLifecycle("pending", async () => undefined);
    const duplicate = vi.fn(async () => undefined);
    let ready = false;
    const reused = attachNativeTerminalLifecycle("pending", duplicate).then(() => { ready = true; });

    // Deliver the reaction of an incorrectly already-resolved reuse promise.
    await Promise.resolve();
    const premature = ready;
    native.resolve();
    await Promise.all([attach, reused]);

    expect(premature).toBe(false);
    expect(ready).toBe(true);
    expect(duplicate).not.toHaveBeenCalled();
    expect(await detach).toBe(false);
  });

  it("propagates the original attachment failure to its replacement owner", async () => {
    const native = deferred();
    const error = new Error("native attach failed");
    const attach = attachNativeTerminalLifecycle("failed", () => native.promise);
    const reused = attachNativeTerminalLifecycle("failed", async () => undefined);
    const results = Promise.allSettled([attach, reused]);

    native.reject(error);

    expect(await results).toEqual([
      { status: "rejected", reason: error },
      { status: "rejected", reason: error },
    ]);
    const retry = vi.fn(async () => undefined);
    await attachNativeTerminalLifecycle("failed", retry);
    expect(retry).toHaveBeenCalledOnce();
  });

  it("reattaches after a detach that has already started", async () => {
    await attachNativeTerminalLifecycle("detaching", async () => undefined);
    const started = deferred();
    const release = deferred();
    const detach = detachNativeTerminalLifecycle("detaching", async () => {
      started.resolve();
      await release.promise;
    });
    await started.promise;
    const operation = vi.fn(async () => undefined);

    const attach = attachNativeTerminalLifecycle("detaching", operation);
    release.resolve();
    await Promise.all([detach, attach]);

    expect(operation).toHaveBeenCalledOnce();
  });

  it("keeps a sibling split pane attached when both panes remount in one turn", async () => {
    // The recorded blank-right-pane failure: React replays both panes'
    // effects in a single commit, so each queues a detach and re-attaches.
    // The left pane's re-attach must not adopt the right pane's pending
    // detachment as an outgoing surface it replaces, or presenting the left
    // pane tears the right pane's live surface down.
    const detached: string[] = [];
    await attachNativeTerminalLifecycle("pane-left", async () => undefined);
    presentNativeTerminalLifecycle("pane-left");
    await attachNativeTerminalLifecycle("pane-right", async () => undefined);
    presentNativeTerminalLifecycle("pane-right");

    const leftDetach = detachNativeTerminalLifecycle("pane-left", async () => {
      detached.push("pane-left");
    });
    const rightDetach = detachNativeTerminalLifecycle("pane-right", async () => {
      detached.push("pane-right");
    });
    void attachNativeTerminalLifecycle("pane-left", async () => undefined);
    void attachNativeTerminalLifecycle("pane-right", async () => undefined);

    presentNativeTerminalLifecycle("pane-left");
    presentNativeTerminalLifecycle("pane-right");
    await Promise.all([leftDetach, rightDetach]);

    expect(detached).toEqual([]);
  });

  it("cancels a detachment already parked under a replacement when its pane returns", async () => {
    // A genuine replacement parks the outgoing surface's detachment until the
    // incoming one presents. If that outgoing pane comes back before the
    // release, the parked teardown must be cancelled -- it is no longer
    // outgoing, and firing it would blank the pane that just re-attached.
    const detached: string[] = [];
    await attachNativeTerminalLifecycle("pane-outgoing", async () => undefined);
    presentNativeTerminalLifecycle("pane-outgoing");

    const outgoingDetach = detachNativeTerminalLifecycle("pane-outgoing", async () => {
      detached.push("pane-outgoing");
    });
    // Incoming surface takes over the compositor, parking the detachment.
    void attachNativeTerminalLifecycle("pane-incoming", async () => undefined);
    // The outgoing pane returns before the incoming one presents.
    void attachNativeTerminalLifecycle("pane-outgoing", async () => undefined);
    presentNativeTerminalLifecycle("pane-incoming");
    await outgoingDetach;

    expect(detached).toEqual([]);
  });

  it("still detaches a surface that unmounts without returning", async () => {
    // The guards above must not strand real teardowns: a pane that unmounts
    // and does not re-attach has to release its compositor surface.
    const detached: string[] = [];
    await attachNativeTerminalLifecycle("pane-gone", async () => undefined);
    presentNativeTerminalLifecycle("pane-gone");

    await detachNativeTerminalLifecycle("pane-gone", async () => {
      detached.push("pane-gone");
    });
    expect(detached).toEqual(["pane-gone"]);
  });

  it("skips slow detach queued in lifecycle queue when session re-attaches before detach runs", async () => {
    const detached: string[] = [];
    let resolveSlowAttach: (() => void) | undefined;
    const slowAttach = new Promise<void>((resolve) => {
      resolveSlowAttach = resolve;
    });

    // Fresh attach in flight
    const initialAttach = attachNativeTerminalLifecycle("pane-slow", async () => {
      await slowAttach;
    });

    // Pane unmounts: queues detach
    const queuedDetach = detachNativeTerminalLifecycle("pane-slow", async () => {
      detached.push("pane-slow");
    });

    // Let microtask queue execute detachment into lifecycle tail
    await Promise.resolve();

    // Pane remounts: attaches again (reused / bumped generation)
    void attachNativeTerminalLifecycle("pane-slow", async () => undefined);

    // Initial attach now finishes
    resolveSlowAttach?.();
    await Promise.all([initialAttach, queuedDetach]);

    // Detach operation must have been skipped because generation changed
    expect(detached).toEqual([]);
  });
});

describe("nativeTerminalLifecycle positive presentation subscriptions", () => {
  const receipt: NativeTerminalPresentationReceipt = {
    frontendSessionId: "fs-1",
    paneIdentity: "pane-1",
    backendSessionId: "backend-1",
    bindingKey: "backend-1::0:",
    attemptGeneration: 2,
  };

  beforeEach(() => {
    resetNativeTerminalLifecycleForTest();
  });

  afterEach(resetNativeTerminalLifecycleForTest);

  it("delivers only to a full exact subscription, rejecting one-field mismatches", () => {
    const fullSubscription: NativeTerminalPresentationSubscription = {
      frontendSessionId: "fs-1",
      paneIdentity: "pane-1",
      backendSessionId: "backend-1",
      bindingKey: "backend-1::0:",
      attemptGeneration: 2,
    };
    // One table-driven probe per payload field: each differs from `receipt`
    // in exactly that field, so the full subscription must reject all of them.
    const mismatches: NativeTerminalPresentationSubscription[] = [
      { frontendSessionId: "fs-other" },
      { paneIdentity: "pane-other" },
      { backendSessionId: "backend-other" },
      { bindingKey: "backend-1::1:" },
      { attemptGeneration: 3 },
    ];
    const received: NativeTerminalPresentationReceipt[] = [];
    const unsubscribe = subscribeNativeTerminalPresentation(
      fullSubscription,
      (r) => received.push(r),
    );

    expect(emitNativeTerminalPresentation(receipt)).toBe(1);
    expect(received).toEqual([receipt]);

    for (const mismatch of mismatches) {
      const probe: NativeTerminalPresentationReceipt = {
        frontendSessionId: mismatch.frontendSessionId ?? receipt.frontendSessionId,
        paneIdentity: mismatch.paneIdentity ?? receipt.paneIdentity,
        backendSessionId: mismatch.backendSessionId ?? receipt.backendSessionId,
        bindingKey: mismatch.bindingKey !== undefined ? mismatch.bindingKey : receipt.bindingKey,
        attemptGeneration: mismatch.attemptGeneration ?? receipt.attemptGeneration,
      };
      expect(probe).not.toEqual(receipt);
      expect(emitNativeTerminalPresentation(probe)).toBe(0);
    }
    expect(received).toHaveLength(1);
    unsubscribe();
  });

  it("treats omitted fields as unconstrained and explicit null bindingKey as a literal match", () => {
    const anyFilter: NativeTerminalPresentationReceipt[] = [];
    const nullBinding: NativeTerminalPresentationReceipt[] = [];
    subscribeNativeTerminalPresentation({}, (r) => anyFilter.push(r));
    subscribeNativeTerminalPresentation({ bindingKey: null }, (r) => nullBinding.push(r));

    expect(emitNativeTerminalPresentation(receipt)).toBe(1);
    expect(emitNativeTerminalPresentation({ ...receipt, bindingKey: null })).toBe(2);
    expect(anyFilter).toHaveLength(2);
    expect(nullBinding).toHaveLength(1);
    expect(nullBinding[0]?.bindingKey).toBeNull();
  });

  it("stops delivering after unsubscribe", () => {
    const received: NativeTerminalPresentationReceipt[] = [];
    const unsubscribe = subscribeNativeTerminalPresentation({}, (r) => received.push(r));

    expect(emitNativeTerminalPresentation(receipt)).toBe(1);
    unsubscribe();
    expect(emitNativeTerminalPresentation(receipt)).toBe(0);
    expect(received).toHaveLength(1);
  });

  it("isolates a throwing subscriber from the remaining subscribers", () => {
    const received: NativeTerminalPresentationReceipt[] = [];
    subscribeNativeTerminalPresentation({}, () => {
      throw new Error("boom");
    });
    subscribeNativeTerminalPresentation({}, (r) => received.push(r));

    expect(emitNativeTerminalPresentation(receipt)).toBe(2);
    expect(received).toEqual([receipt]);
  });

  it("keeps generic lifecycle present independent of typed subscribers", async () => {
    const received: NativeTerminalPresentationReceipt[] = [];
    const unsubscribe = subscribeNativeTerminalPresentation({}, (r) => received.push(r));

    await attachNativeTerminalLifecycle("session-a", async () => {});
    const detachA = detachNativeTerminalLifecycle("session-a", async () => {});
    // A fresh attach takes over the outgoing surface's pending detachment and
    // parks it behind the replacement's presentation.
    await attachNativeTerminalLifecycle("session-b", async () => {});
    presentNativeTerminalLifecycle("session-b");

    // The generic present releases the held detach exactly as before...
    await expect(detachA).resolves.toBe(true);
    // ...without ever notifying typed presentation subscribers.
    expect(received).toEqual([]);
    unsubscribe();
  });
});
