import { describe, expect, it, vi } from "vitest";

import { subscribeNotificationActivations } from "./notificationActivation";
import type { NotificationTarget } from "./types";

function deferred<T = void>() {
  let resolve!: (value?: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res as (value?: T) => void;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/**
 * A fully controllable harness. Every observable step - listener registration, each drain call,
 * and each navigation - is a deferred promise the test awaits, so ordering is asserted from real
 * signals rather than timers or polling.
 */
function harness() {
  const log: string[] = [];
  let listener: (() => void) | null = null;
  const registered = deferred();
  const unlisten = vi.fn(() => log.push("unlisten"));

  const takeStarted = Array.from({ length: 8 }, () => deferred());
  const takeRelease = Array.from({ length: 8 }, () => deferred<NotificationTarget[]>());
  let takeIndex = 0;

  const navigations: NotificationTarget[] = [];
  const navSignals = new Map<string, ReturnType<typeof deferred>>();
  const navigatedFor = (id: string) => {
    if (!navSignals.has(id)) navSignals.set(id, deferred());
    return navSignals.get(id)!.promise;
  };

  const deps = {
    onNotificationActivated: vi.fn(async (cb: () => void) => {
      log.push("listen");
      listener = cb;
      registered.resolve();
      return unlisten;
    }),
    // Not async-wrapped: returns the exact deferred promise so the module's await attaches
    // directly to it, keeping microtask ordering single-hop and deterministic.
    takeNotificationActivations: vi.fn(() => {
      const i = takeIndex++;
      log.push(`take:${i}`);
      takeStarted[i].resolve();
      return takeRelease[i].promise;
    }),
  };

  const navigate = (target: NotificationTarget) => {
    log.push(`nav:${target.sessionId}`);
    navigations.push(target);
    if (!navSignals.has(target.sessionId)) navSignals.set(target.sessionId, deferred());
    navSignals.get(target.sessionId)!.resolve();
  };

  return {
    deps,
    navigate,
    log,
    unlisten,
    registered,
    navigations,
    takeStarted,
    takeRelease,
    navigatedFor,
    fire: () => listener?.(),
  };
}

describe("subscribeNotificationActivations", () => {
  it("registers the listener before the recovery drain and navigates the queued click", async () => {
    const h = harness();
    const navigated = h.navigatedFor("sess-1");

    subscribeNotificationActivations(h.navigate, h.deps);
    await h.registered.promise;
    await h.takeStarted[0].promise;

    // Registration strictly precedes the recovery drain, so a pre-registration click is caught.
    expect(h.log).toEqual(["listen", "take:0"]);

    h.takeRelease[0].resolve([{ workspaceId: "ws-1", sessionId: "sess-1" }]);
    await navigated;

    expect(h.navigations).toEqual([{ workspaceId: "ws-1", sessionId: "sess-1" }]);
  });

  it("drains the same queue again when a live wake fires", async () => {
    const h = harness();

    subscribeNotificationActivations(h.navigate, h.deps);
    await h.takeStarted[0].promise;
    h.takeRelease[0].resolve([]);

    const navigated = h.navigatedFor("sess-2");
    h.fire();
    await h.takeStarted[1].promise;
    h.takeRelease[1].resolve([{ workspaceId: "ws-2", sessionId: "sess-2" }]);
    await navigated;

    expect(h.navigations.map((t) => t.sessionId)).toEqual(["sess-2"]);
  });

  it("serializes overlapping wakes and replays activations in arrival order", async () => {
    const h = harness();

    subscribeNotificationActivations(h.navigate, h.deps);
    await h.takeStarted[0].promise;

    // Two wakes arrive while the startup drain is still in flight.
    h.fire();
    h.fire();

    const a = h.navigatedFor("A");
    h.takeRelease[0].resolve([{ workspaceId: "ws", sessionId: "A" }]);
    await a;

    await h.takeStarted[1].promise;
    const b = h.navigatedFor("B");
    h.takeRelease[1].resolve([{ workspaceId: "ws", sessionId: "B" }]);
    await b;

    await h.takeStarted[2].promise;
    const c = h.navigatedFor("C");
    h.takeRelease[2].resolve([{ workspaceId: "ws", sessionId: "C" }]);
    await c;

    // Strict interleave proves serialization: a drain never starts before the prior one finished.
    expect(h.log).toEqual([
      "listen",
      "take:0",
      "nav:A",
      "take:1",
      "nav:B",
      "take:2",
      "nav:C",
    ]);
    expect(h.navigations.map((t) => t.sessionId)).toEqual(["A", "B", "C"]);
  });

  it("does not navigate for an in-flight drain once disposed", async () => {
    const h = harness();

    const dispose = subscribeNotificationActivations(h.navigate, h.deps);
    await h.takeStarted[0].promise;
    dispose();
    h.takeRelease[0].resolve([{ workspaceId: "ws-1", sessionId: "sess-1" }]);
    // The module's navigation decision runs before this await resumes (its await on the same
    // promise was attached first), so no polling is needed to observe the outcome.
    await h.takeRelease[0].promise;

    expect(h.navigations).toEqual([]);
    expect(h.unlisten).toHaveBeenCalledTimes(1);
  });

  it("reports a drain IPC rejection instead of swallowing it, and still serves the next wake", async () => {
    const h = harness();
    const onError = vi.fn();
    const ipcError = new Error("ipc down");

    subscribeNotificationActivations(h.navigate, h.deps, onError);
    await h.takeStarted[0].promise;
    h.takeRelease[0].reject(ipcError);
    // The module's catch runs before this await resumes; a leaked rejection would fail the test.
    await h.takeRelease[0].promise.catch(() => undefined);

    // The failure is surfaced with its original error, never swallowed.
    expect(onError).toHaveBeenCalledWith(ipcError, "drain");

    const navigated = h.navigatedFor("sess-3");
    h.fire();
    await h.takeStarted[1].promise;
    h.takeRelease[1].resolve([{ workspaceId: "ws-3", sessionId: "sess-3" }]);
    await navigated;

    expect(h.navigations.map((t) => t.sessionId)).toEqual(["sess-3"]);
  });

  it("reports a listener registration failure with its original error", async () => {
    const listenError = new Error("listen refused");
    const reported = deferred<{ error: unknown; source: string }>();
    const onError = vi.fn((error: unknown, source: string) => reported.resolve({ error, source }));
    const deps = {
      onNotificationActivated: vi.fn(() => Promise.reject(listenError)),
      takeNotificationActivations: vi.fn(async () => []),
    };

    subscribeNotificationActivations(vi.fn(), deps, onError);

    // Await the exact diagnostic signal rather than a microtask guess.
    await expect(reported.promise).resolves.toEqual({ error: listenError, source: "listen" });
  });

  it("disposes the listener that resolves after an early unsubscribe", async () => {
    const disposed = deferred();
    const unlisten = vi.fn(() => disposed.resolve());
    const deps = {
      onNotificationActivated: vi.fn(async () => unlisten),
      takeNotificationActivations: vi.fn(async () => []),
    };

    // Unsubscribe BEFORE onNotificationActivated resolves.
    const dispose = subscribeNotificationActivations(vi.fn(), deps);
    dispose();

    // The late-resolving listener is torn down; await that exact teardown signal.
    await disposed.promise;
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
