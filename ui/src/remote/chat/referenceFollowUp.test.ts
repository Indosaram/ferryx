import { describe, expect, it } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import { ReferenceFollowUpStore } from "./referenceFollowUp";
import { referenceDraftKey, type ReferenceTargetRef } from "./referenceTypes";

const base: TargetRef = {
  hostId: "host-a",
  ownerId: "owner-a",
  epoch: "18446744073709551615",
  backendSessionId: "sess-1",
};

function target(overrides: Partial<TargetRef> = {}): ReferenceTargetRef {
  return { target: { ...base, ...overrides } };
}

function fixture() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
  };
  return { data, storage, queue: new ReferenceFollowUpStore(() => storage) };
}

describe("reference follow-up queue", () => {
  it("persists admitted rows on the durable draft key and delivers them once, in order", () => {
    const { queue, storage, data } = fixture();
    const pane = target();
    const seen: string[] = [];
    const stop = queue.subscribe(() => {
      seen.push(queue.read(pane).map((row) => row.text).join("|"));
    });
    queue.admit(pane, "first");
    queue.admit(pane, "second");
    expect(seen).toEqual(["first", "first|second"]);
    expect(data.has(queue.storageKey(pane))).toBe(true);
    expect(queue.storageKey(pane)).toBe("ferryx.referenceChat.followUp:" + referenceDraftKey(pane));

    const ids = ["r1", "r2"];
    const first = queue.beginDelivery(pane, () => ids.shift() ?? "extra");
    expect(first?.text).toBe("first");
    expect(first?.requestId).toBe("r1");
    expect(queue.beginDelivery(pane, () => "should-not")).toBeNull();
    queue.complete(pane, first!.id);

    const second = queue.beginDelivery(pane, () => ids.shift() ?? "extra");
    expect(second?.text).toBe("second");
    expect(second?.requestId).toBe("r2");
    queue.complete(pane, second!.id);
    expect(queue.read(pane)).toEqual([]);
    expect(new ReferenceFollowUpStore(() => storage).read(pane)).toEqual([]);
    stop();
  });

  it("cancels a row only before delivery", () => {
    const { queue } = fixture();
    const pane = target();
    const kept = queue.admit(pane, "keep");
    const gone = queue.admit(pane, "gone");
    expect(queue.cancel(pane, gone.id)).toBe(true);
    expect(queue.read(pane).map((row) => row.id)).toEqual([kept.id]);
    const started = queue.beginDelivery(pane, () => "r1");
    expect(queue.cancel(pane, started!.id)).toBe(false);
    expect(queue.read(pane)).toHaveLength(1);
  });

  it("holds the queue after an unknown outcome and does not allocate another request id", () => {
    const { queue, storage } = fixture();
    const pane = target();
    queue.admit(pane, "maybe");
    queue.admit(pane, "later");
    let allocations = 0;
    const started = queue.beginDelivery(pane, () => {
      allocations += 1;
      return "req-once";
    });
    const held = new Promise<void>((resolve) => {
      const stop = queue.subscribe(() => {
        if (!queue.heldUnknown(pane)) return;
        stop();
        resolve();
      });
    });
    queue.holdUnknown(pane, started!.id);
    return held.then(() => {
      expect(queue.beginDelivery(pane, () => {
        allocations += 1;
        return "req-again";
      })).toBeNull();
      expect(allocations).toBe(1);
      const reloaded = new ReferenceFollowUpStore(() => storage);
      expect(reloaded.read(pane).map((row) => row.phase)).toEqual(["unknown", "queued"]);
      expect(reloaded.beginDelivery(pane, () => "req-reload")).toBeNull();
      expect(reloaded.read(pane)[0]?.requestId).toBe("req-once");
    });
  });

  it("turns a delivering row into unknown on reload so it cannot be posted again", () => {
    const { queue, storage } = fixture();
    const pane = target();
    queue.admit(pane, "in flight");
    queue.beginDelivery(pane, () => "req-flight");
    const reloaded = new ReferenceFollowUpStore(() => storage);
    expect(reloaded.read(pane)[0]?.phase).toBe("unknown");
    expect(reloaded.beginDelivery(pane, () => "req-new")).toBeNull();
  });

  it("drops rows when the live owner or epoch differs and leaves a same-fence row", () => {
    const { queue } = fixture();
    const pane = target();
    queue.admit(pane, "old owner");
    queue.admit(target({ epoch: "9" }), "other epoch key is the same draft key");
    const droppedOwner = queue.dropStale(target({ ownerId: "owner-b" }));
    expect(droppedOwner.map((row) => row.text)).toEqual(["old owner", "other epoch key is the same draft key"]);
    expect(queue.read(target({ ownerId: "owner-b" }))).toEqual([]);

    queue.admit(target({ ownerId: "owner-b" }), "kept");
    expect(queue.dropStale(target({ ownerId: "owner-b" }))).toEqual([]);
    expect(queue.read(target({ ownerId: "owner-b" })).map((row) => row.text)).toEqual(["kept"]);
    expect(queue.dropStale(target({ ownerId: "owner-b", epoch: "2" })).map((row) => row.text)).toEqual(["kept"]);
  });
});
