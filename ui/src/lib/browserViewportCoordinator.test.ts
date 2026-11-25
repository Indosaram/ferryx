import { describe, expect, it, vi } from "vitest";

import { createBrowserViewportCoordinator, type BrowserViewportIpc } from "./browserViewportCoordinator";
import type { LogicalRect } from "./types";

type Pending<T> = { resolve: (value: T) => void; reject: (error: unknown) => void };

/** Every IPC call stays pending until the test resolves it, so acknowledgements can arrive in
 * any order the host could produce. */
function createControlledIpc() {
  const bounds: Array<Pending<unknown> & { browserId: string; bounds: LogicalRect }> = [];
  const visible: Array<Pending<unknown> & { browserId: string; visible: boolean }> = [];
  const ipc: BrowserViewportIpc = {
    setBounds: (browserId, next) =>
      new Promise((resolve, reject) => {
        bounds.push({ browserId, bounds: next, resolve, reject });
      }),
    setVisible: (browserId, next) =>
      new Promise((resolve, reject) => {
        visible.push({ browserId, visible: next, resolve, reject });
      }),
  };
  return { ipc, bounds, visible };
}

function createImmediateIpc() {
  const setBounds = vi.fn(async (_browserId: string, _bounds: LogicalRect) => undefined);
  const setVisible = vi.fn(async (_browserId: string, _visible: boolean) => undefined);
  return { ipc: { setBounds, setVisible }, setBounds, setVisible };
}

const rect = (x: number): LogicalRect => ({ x, y: 0, width: 100, height: 50 });

/** Drains the microtask queue so a resolved fake IPC acknowledgement reaches its handler without
 * introducing a timer the assertions would have to race. */
async function flush(): Promise<void> {
  for (let tick = 0; tick < 10; tick += 1) await Promise.resolve();
}

/** Hides are `setVisible(id, false)`; the handoff contract is about exactly these calls. */
function hideCount(calls: Array<[string, boolean]>): number {
  return calls.filter(([, visible]) => visible === false).length;
}

function visibilityValues(calls: Array<[string, boolean]>): boolean[] {
  return calls.map(([, visible]) => visible);
}

/** The reveal/hide flags of the controlled IPC's recorded calls, in issue order. */
function pendingVisibilityValues(calls: Array<{ visible: boolean }>): boolean[] {
  return calls.map((call) => call.visible);
}

describe("browser viewport coordinator ownership", () => {
  it("hands a browser from one surface to the next without hiding it", async () => {
    const { ipc, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);

    const split = coordinator.claim("browser-1", "split", 0);
    await expect(split.syncBoundsAndShow(rect(0))).resolves.toBe(true);
    expect(visibilityValues(setVisible.mock.calls)).toEqual([true]);

    setVisible.mockClear();

    const fullscreen = coordinator.claim("browser-1", "fullscreen", 10);
    await expect(fullscreen.syncBoundsAndShow(rect(10))).resolves.toBe(true);
    split.release();

    // The old owner's release is a strict no-op: the successor's webview was never hidden.
    expect(visibilityValues(setVisible.mock.calls)).toEqual([true]);
    expect(coordinator.getOwner("browser-1")).toEqual({ surfaceId: "fullscreen", priority: 10 });
  });

  it("stays race-free over 50 split -> fullscreen handoffs", async () => {
    const { ipc, setBounds, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);
    const browserId = "browser-churn";

    for (let round = 0; round < 50; round += 1) {
      const split = coordinator.claim(browserId, `split-${round}`, 0);
      await expect(split.syncBoundsAndShow(rect(round))).resolves.toBe(true);

      const fullscreen = coordinator.claim(browserId, `fullscreen-${round}`, 10);
      await expect(fullscreen.syncBoundsAndShow(rect(round + 1))).resolves.toBe(true);

      // The handoff's old owner releases after the successor already claimed AND revealed.
      split.release();
      expect(hideCount(setVisible.mock.calls)).toBe(round);
      fullscreen.release();
      expect(hideCount(setVisible.mock.calls)).toBe(round + 1);
    }

    // Two shows per round (split, then fullscreen) and exactly one hide per round: the last
    // release. No hide ever lands between a claim and its own reveal.
    expect(visibilityValues(setVisible.mock.calls).filter(Boolean)).toHaveLength(100);
    expect(hideCount(setVisible.mock.calls)).toBe(50);
    expect(setBounds).toHaveBeenCalledTimes(100);
    expect(coordinator.getOwner(browserId)).toBeNull();
  });

  it("ignores a superseded handle instead of hiding the new owner's surface", async () => {
    const { ipc, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);

    const split = coordinator.claim("browser-1", "split");
    await split.syncBoundsAndShow(rect(0));
    const mini = coordinator.claim("browser-1", "mini");
    setVisible.mockClear();

    await split.hide();
    await expect(split.syncBoundsAndShow(rect(1))).resolves.toBe(false);
    split.release();

    expect(setVisible).not.toHaveBeenCalled();
    expect(split.isOwner()).toBe(false);
    expect(mini.isOwner()).toBe(true);
  });
});

describe("browser viewport coordinator sequencing", () => {
  it("never reveals a surface for a bounds frame that a newer claim superseded", async () => {
    const { ipc, bounds, visible } = createControlledIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);

    const older = coordinator.claim("browser-1", "split");
    const olderReveal = older.syncBoundsAndShow(rect(0));
    const newer = coordinator.claim("browser-1", "fullscreen");
    const newerReveal = newer.syncBoundsAndShow(rect(10));
    expect(bounds).toHaveLength(2);

    // The newest owner acknowledges first and reveals.
    bounds[1].resolve(undefined);
    await flush();
    expect(visible).toHaveLength(1);
    visible[0].resolve(undefined);
    await expect(newerReveal).resolves.toBe(true);

    // The superseded owner acknowledges its obsolete frame last.
    bounds[0].resolve(undefined);
    await expect(olderReveal).resolves.toBe(false);

    expect(pendingVisibilityValues(visible)).toEqual([true]);
    expect(bounds[0].browserId).toBe("browser-1");
    expect(coordinator.getOwner("browser-1")).toEqual({ surfaceId: "fullscreen", priority: 0 });
  });

  it("drops an out-of-order acknowledgement from the same owner", async () => {
    const { ipc, bounds, visible } = createControlledIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);
    const handle = coordinator.claim("browser-1", "split");

    const staleFrame = handle.syncBoundsAndShow(rect(0));
    const latestFrame = handle.syncBoundsAndShow(rect(40));
    expect(bounds).toHaveLength(2);

    bounds[1].resolve(undefined);
    await flush();
    expect(visible).toHaveLength(1);
    visible[0].resolve(undefined);
    await expect(latestFrame).resolves.toBe(true);

    // The older frame now lands: its sequence is superseded, so it cannot reveal again.
    bounds[0].resolve(undefined);
    await expect(staleFrame).resolves.toBe(false);

    expect(pendingVisibilityValues(visible)).toEqual([true]);
    expect(handle.isOwner()).toBe(true);
  });

  it("keeps the newest bounds when an obsolete frame resolves late and then hides", async () => {
    const { ipc, bounds, visible } = createControlledIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);
    const handle = coordinator.claim("browser-1", "split");

    const staleFrame = handle.syncBoundsAndShow(rect(0));
    const latestFrame = handle.syncBoundsAndShow(rect(40));
    bounds[1].resolve(undefined);
    await flush();
    expect(visible).toHaveLength(1);
    visible[0].resolve(undefined);
    await expect(latestFrame).resolves.toBe(true);

    // An occlusion arrives before the stale frame is acknowledged; the late frame must not
    // undo the hide by issuing its own reveal afterwards.
    const hidden = handle.hide();
    expect(pendingVisibilityValues(visible)).toEqual([true, false]);
    visible[1].resolve(undefined);
    await hidden;

    bounds[0].resolve(undefined);
    await expect(staleFrame).resolves.toBe(false);

    expect(pendingVisibilityValues(visible)).toEqual([true, false]);
  });
});

describe("browser viewport coordinator lifetime and occlusion", () => {
  it("hides exactly once when the owner goes away with no successor", async () => {
    const { ipc, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);

    const handle = coordinator.claim("browser-1", "split");
    await expect(handle.syncBoundsAndShow(rect(0))).resolves.toBe(true);
    setVisible.mockClear();

    handle.release();

    expect(visibilityValues(setVisible.mock.calls)).toEqual([false]);
    expect(coordinator.getOwner("browser-1")).toBeNull();
  });

  it("hides and restores the surface around an occlusion", async () => {
    const { ipc, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);
    const handle = coordinator.claim("browser-1", "split");

    await expect(handle.syncBoundsAndShow(rect(0))).resolves.toBe(true);
    await handle.hide();
    expect(handle.isOwner()).toBe(true);
    await expect(handle.syncBoundsAndShow(rect(0))).resolves.toBe(true);

    expect(visibilityValues(setVisible.mock.calls)).toEqual([true, false, true]);
  });

  it("reports which operation failed and never reveals after a bounds failure", async () => {
    const { ipc, bounds, visible } = createControlledIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);
    const handle = coordinator.claim("browser-1", "split");
    const onError = vi.fn();

    const reveal = handle.syncBoundsAndShow(rect(0), onError);
    bounds[0].reject({ code: "WEBVIEW_NOT_FOUND" });
    await expect(reveal).resolves.toBe(false);
    expect(onError).toHaveBeenCalledWith({ code: "WEBVIEW_NOT_FOUND" }, "bounds");
    expect(visible).toHaveLength(0);

    const retry = handle.syncBoundsAndShow(rect(0), onError);
    bounds[1].resolve(undefined);
    await flush();
    expect(visible).toHaveLength(1);
    visible[0].reject({ code: "SURFACE_LOST" });
    await expect(retry).resolves.toBe(false);
    expect(onError).toHaveBeenLastCalledWith({ code: "SURFACE_LOST" }, "visibility");
  });

  it("keeps browsers independent and forgets every owner on reset", async () => {
    const { ipc, setVisible } = createImmediateIpc();
    const coordinator = createBrowserViewportCoordinator(ipc);

    const first = coordinator.claim("browser-1", "split");
    const second = coordinator.claim("browser-2", "split");
    await first.syncBoundsAndShow(rect(0));
    await second.syncBoundsAndShow(rect(0));

    expect(setVisible.mock.calls).toEqual([
      ["browser-1", true],
      ["browser-2", true],
    ]);

    coordinator.reset();

    expect(coordinator.getOwner("browser-1")).toBeNull();
    expect(coordinator.getOwner("browser-2")).toBeNull();
    // A handle from before the reset no longer owns anything.
    await expect(first.syncBoundsAndShow(rect(1))).resolves.toBe(false);
  });
});
