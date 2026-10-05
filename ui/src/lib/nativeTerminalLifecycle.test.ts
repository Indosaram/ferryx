import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./switchDebug", () => ({ switchDebug: vi.fn() }));

import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  presentNativeTerminalLifecycle,
  subscribeNativeTerminalPresentation,
  emitNativeTerminalPresentation,
  type NativeTerminalPresentationReceipt,
  resetNativeTerminalLifecycleForTest,
  registerDurableNativeBinding,
  getDurableNativeBinding,
} from "./nativeTerminalLifecycle";
import type { PaneAttachTuple } from "./types";

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

  it("rejects stale tuple ownership and lets a bumped retry supersede it", () => {
    const tuple: PaneAttachTuple = {
      backendSessionId: "back", incarnation: "life", daemonEpoch: "8",
      frontendSessionId: "front", paneIdentity: "pane", bindingKey: "binding", attemptGeneration: 3,
    };
    expect(registerDurableNativeBinding(tuple)).toBe(true);
    const stale: PaneAttachTuple[] = [
      { ...tuple, incarnation: "other" }, { ...tuple, daemonEpoch: "7" },
      { ...tuple, frontendSessionId: "other" }, { ...tuple, paneIdentity: "other" },
      { ...tuple, bindingKey: "other" }, { ...tuple, attemptGeneration: 2 },
    ];
    for (const candidate of stale) {
      expect(registerDurableNativeBinding(candidate)).toBe(false);
      expect(getDurableNativeBinding("back")).toEqual(tuple);
    }
    const retry = { ...tuple, attemptGeneration: 4 };
    expect(registerDurableNativeBinding(retry)).toBe(true);
    expect(registerDurableNativeBinding(tuple)).toBe(false);
    expect(getDurableNativeBinding("back")).toEqual(retry);
  });

  it("replaces an authoritative binding only while its persisted predecessor is current", () => {
    const previous: PaneAttachTuple = { backendSessionId: "back", incarnation: "life", daemonEpoch: "8",
      frontendSessionId: "front", paneIdentity: "pane", bindingKey: "old", attemptGeneration: 3 };
    expect(registerDurableNativeBinding(previous)).toBe(true);
    const next = { ...previous, daemonEpoch: "9", bindingKey: "new", attemptGeneration: 4 };
    expect(registerDurableNativeBinding(next)).toBe(false);
    expect(registerDurableNativeBinding(next, performance.now(), previous)).toBe(true);
    expect(getDurableNativeBinding("back")).toEqual(next);
    expect(registerDurableNativeBinding({ ...previous, attemptGeneration: 5 }, performance.now(), previous)).toBe(false);
    expect(getDurableNativeBinding("back")).toEqual(next);
  });

  it("requires every supplied attach tuple field before becoming ready", () => {
    const tuple: NativeTerminalPresentationReceipt = {
      backendSessionId: "back", incarnation: "life", daemonEpoch: "8",
      frontendSessionId: "front", paneIdentity: "pane", bindingKey: "binding", attemptGeneration: 3,
    };
    let ready = false;
    subscribeNativeTerminalPresentation(tuple, () => { ready = true; });
    const stale = [
      { ...tuple, backendSessionId: "other" }, { ...tuple, incarnation: "other" },
      { ...tuple, daemonEpoch: "7" }, { ...tuple, frontendSessionId: "other" },
      { ...tuple, paneIdentity: "other" }, { ...tuple, bindingKey: "other" },
      { ...tuple, attemptGeneration: 2 },
    ];
    for (const receipt of stale) emitNativeTerminalPresentation(receipt);
    expect(ready).toBe(false);
    emitNativeTerminalPresentation(tuple);
    expect(ready).toBe(true);
  });

  it("listener mutation does not deliver an EOF receipt to newly added owners", () => {
    const receipt: NativeTerminalPresentationReceipt = {
      frontendSessionId: "front", paneIdentity: "pane", backendSessionId: "back",
      bindingKey: "binding", attemptGeneration: 1,
    };
    const seen: string[] = [];
    let remove = () => {};
    subscribeNativeTerminalPresentation({}, () => {
      seen.push("first"); remove();
      subscribeNativeTerminalPresentation({}, () => seen.push("new"));
    });
    remove = subscribeNativeTerminalPresentation({}, () => seen.push("removed"));
    emitNativeTerminalPresentation(receipt);
    expect(seen).toEqual(["first"]);
  });

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

  it("delivers matched presentation receipts and respects reset", () => {
    const received: NativeTerminalPresentationReceipt[] = [];
    subscribeNativeTerminalPresentation(
      { paneIdentity: "pane-1", attemptGeneration: 2 },
      (receipt) => { received.push(receipt); },
    );

    const nonMatching: NativeTerminalPresentationReceipt = {
      frontendSessionId: "fs-1",
      paneIdentity: "pane-2",
      backendSessionId: "bs-1",
      bindingKey: "bk-1",
      attemptGeneration: 2,
    };
    expect(emitNativeTerminalPresentation(nonMatching)).toBe(0);
    expect(received).toHaveLength(0);

    const matching: NativeTerminalPresentationReceipt = {
      frontendSessionId: "fs-1",
      paneIdentity: "pane-1",
      backendSessionId: "bs-1",
      bindingKey: "bk-1",
      attemptGeneration: 2,
    };
    expect(emitNativeTerminalPresentation(matching)).toBe(1);
    expect(received).toEqual([matching]);

    resetNativeTerminalLifecycleForTest();
    expect(emitNativeTerminalPresentation(matching)).toBe(0);
    expect(received).toHaveLength(1);
  });
});
