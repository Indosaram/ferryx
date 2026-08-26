import { describe, expect, it, vi } from "vitest";

vi.mock("./switchDebug", () => ({ switchDebug: vi.fn() }));

import {
  attachNativeTerminalLifecycle,
  detachNativeTerminalLifecycle,
  presentNativeTerminalLifecycle,
} from "./nativeTerminalLifecycle";

/** Settles the module's microtask-scheduled detachment work. */
async function settle(): Promise<void> {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
}

describe("nativeTerminalLifecycle sibling pane ownership", () => {
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

    void detachNativeTerminalLifecycle("pane-left", async () => {
      detached.push("pane-left");
    });
    void detachNativeTerminalLifecycle("pane-right", async () => {
      detached.push("pane-right");
    });
    void attachNativeTerminalLifecycle("pane-left", async () => undefined);
    void attachNativeTerminalLifecycle("pane-right", async () => undefined);

    presentNativeTerminalLifecycle("pane-left");
    presentNativeTerminalLifecycle("pane-right");
    await settle();

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

    void detachNativeTerminalLifecycle("pane-outgoing", async () => {
      detached.push("pane-outgoing");
    });
    // Incoming surface takes over the compositor, parking the detachment.
    void attachNativeTerminalLifecycle("pane-incoming", async () => undefined);
    // The outgoing pane returns before the incoming one presents.
    void attachNativeTerminalLifecycle("pane-outgoing", async () => undefined);
    presentNativeTerminalLifecycle("pane-incoming");
    await settle();

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
    await settle();

    expect(detached).toEqual(["pane-gone"]);
  });
});
