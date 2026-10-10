/**
 * Ported from the pinned upstream `src/lib/messageQueue.test.ts`
 * (`devswha/herdr-web-ui` @ `54e5a1f6…`, MIT). Authored, NOT EXECUTED — execution override.
 */
import { describe, expect, it } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import { REFERENCE_QUEUE_STORAGE_PREFIX, ReferenceQueueStore } from "./referenceQueue";
import { referenceDraftKey, type ReferenceTargetRef } from "./referenceTypes";

const base: TargetRef = {
  hostId: "host-a",
  ownerId: "owner-a",
  epoch: "18446744073709551615",
  backendSessionId: "sess-1",
};

function target(
  overrides: Partial<TargetRef> = {},
  providerSessionId?: string | null,
): ReferenceTargetRef {
  return { target: { ...base, ...overrides }, providerSessionId };
}

const paneA = target();
const paneB = target({ backendSessionId: "sess-2" });
const otherHost = target({ hostId: "host-b" });

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
  return { data, storage, queue: new ReferenceQueueStore(() => storage) };
}

const texts = (queue: ReferenceQueueStore, owner: ReferenceTargetRef) =>
  queue.read(owner).map((message) => message.text);

describe("reference held queue persistence", () => {
  it("keys the queue on the frozen target key under the canonical prefix", () => {
    const { queue } = fixture();
    expect(queue.storageKey(paneA)).toBe(REFERENCE_QUEUE_STORAGE_PREFIX + referenceDraftKey(paneA));
    expect(queue.storageKey(paneA)).not.toBe(queue.storageKey(paneB));
    expect(queue.storageKey(paneA)).not.toBe(queue.storageKey(otherHost));
  });

  it("persists multiple independent items, including identical text, and individual edits", () => {
    const { queue, storage, data } = fixture();
    queue.add(paneA, "same");
    queue.add(paneA, "same");
    const [first, second] = queue.read(paneA);
    expect(first!.id).not.toBe(second!.id);

    queue.edit(paneA, second!.id, "edited");
    expect(texts(new ReferenceQueueStore(() => storage), paneA)).toEqual(["same", "edited"]);

    queue.remove(paneA, first!.id);
    expect(texts(queue, paneA)).toEqual(["edited"]);
    queue.remove(paneA, second!.id);
    expect(data.has(queue.storageKey(paneA))).toBe(false);
  });

  it("migrates the previous plain-text queue without losing JSON-like messages", () => {
    for (const text of ["legacy", '{"message":"legacy"}']) {
      const { queue, data, storage } = fixture();
      data.set(queue.storageKey(paneA), text);
      expect(queue.read(paneA)[0]!.text).toBe(text);
      queue.add(paneA, "next");
      expect(texts(new ReferenceQueueStore(() => storage), paneA)).toEqual([text, "next"]);
    }
  });
});

describe("reference held queue target scoping", () => {
  it("an old acknowledgement removes only its captured item and owner", () => {
    const { queue } = fixture();
    queue.add(paneA, "in flight");
    const sent = queue.read(paneA)[0]!;
    queue.add(paneA, "newly queued");
    queue.add(otherHost, "other PC");
    queue.add(paneB, "other pane");

    queue.remove(paneA, sent.id);
    expect(texts(queue, paneA)).toEqual(["newly queued"]);
    expect(texts(queue, otherHost)).toEqual(["other PC"]);
    expect(texts(queue, paneB)).toEqual(["other pane"]);
  });

  it("removes nothing when an acknowledgement names another target's row", () => {
    const { queue } = fixture();
    const rowA = queue.add(paneA, "on A");
    queue.add(paneB, "on B");
    queue.remove(paneB, rowA.id);
    expect(texts(queue, paneA)).toEqual(["on A"]);
    expect(texts(queue, paneB)).toEqual(["on B"]);
  });

  it("keeps an identical text on two targets as two independent rows", () => {
    const { queue } = fixture();
    const rowA = queue.add(paneA, "same text");
    queue.add(paneB, "same text");
    queue.remove(paneA, rowA.id);
    expect(texts(queue, paneA)).toEqual([]);
    expect(texts(queue, paneB)).toEqual(["same text"]);
  });
});

describe("reference held queue explicit dispatch", () => {
  it("never dispatches a held row without an explicit send", () => {
    const { queue, storage } = fixture();
    queue.add(paneA, "held one");
    queue.add(paneA, "held two");
    const [first, second] = queue.read(paneA);

    expect(queue.isSending(first!.id)).toBe(false);
    expect(queue.isSending(second!.id)).toBe(false);

    queue.refresh(paneA);
    queue.refreshStorageKey(queue.storageKey(paneA));
    const remounted = new ReferenceQueueStore(() => storage);
    expect(texts(remounted, paneA)).toEqual(["held one", "held two"]);

    queue.endSend(paneA, first!.id);
    expect(texts(queue, paneA)).toEqual(["held one", "held two"]);
    expect(queue.isSending(first!.id)).toBe(false);
  });

  it("keeps a held row across a remount until it is explicitly removed", () => {
    const { queue, storage } = fixture();
    const row = queue.add(paneA, "keep me");
    expect(queue.beginSend(paneA, row.id)).toBe(true);
    expect(queue.beginSend(paneA, row.id)).toBe(false);
    queue.endSend(paneA, row.id);

    const remounted = new ReferenceQueueStore(() => storage);
    expect(texts(remounted, paneA)).toEqual(["keep me"]);
    expect(remounted.isSending(row.id)).toBe(false);
    remounted.remove(paneA, remounted.read(paneA)[0]!.id);
    expect(texts(remounted, paneA)).toEqual([]);
  });

  it("shares pending sends and notifies a remounted view on acknowledgement", () => {
    const { queue } = fixture();
    queue.add(paneA, "first");
    const sent = queue.read(paneA)[0]!;
    expect(queue.beginSend(paneA, sent.id)).toBe(true);
    expect(queue.beginSend(paneA, sent.id)).toBe(false);

    let notices = 0;
    const unsubscribe = queue.subscribe(() => {
      notices += 1;
    });
    queue.add(paneA, "second");
    queue.remove(paneA, sent.id);
    queue.endSend(paneA, sent.id);
    expect(queue.isSending(sent.id)).toBe(false);
    expect(texts(queue, paneA)).toEqual(["second"]);
    expect(notices).toBe(3);
    unsubscribe();
    queue.add(paneA, "third");
    expect(notices).toBe(3);
  });

  it("reads another tab's latest additions before edits or late acknowledgements", () => {
    const { queue, storage } = fixture();
    const other = new ReferenceQueueStore(() => storage);
    queue.add(paneA, "first");
    const first = other.read(paneA)[0]!;
    queue.add(paneA, "second");
    other.add(paneA, "third");
    queue.remove(paneA, first.id);
    other.refresh(paneA);
    expect(texts(other, paneA)).toEqual(["second", "third"]);
  });
});

describe("reference held queue storage failure", () => {
  it("keeps messages in memory when storage is unavailable", () => {
    const queue = new ReferenceQueueStore(() => {
      throw new Error("denied");
    });
    queue.add(paneA, "first");
    queue.add(paneA, "second");
    expect(texts(queue, paneA)).toEqual(["first", "second"]);
  });

  it("makes a failed persistence visible without throwing away the in-memory queue", () => {
    const queue = new ReferenceQueueStore(() => {
      throw new Error("quota");
    });
    queue.add(paneA, "keep me");
    expect(queue.isUnsaved(paneA)).toBe(true);
    expect(queue.isUnsaved(paneB)).toBe(false);
    queue.refresh(paneA);
    expect(texts(queue, paneA)).toEqual(["keep me"]);
  });

  it("clears the visible failure once a later write succeeds", () => {
    const data = new Map<string, string>();
    let blocked = true;
    const queue = new ReferenceQueueStore(() => {
      if (blocked) throw new Error("quota");
      return {
        getItem: (key: string) => data.get(key) ?? null,
        setItem: (key: string, value: string) => {
          data.set(key, value);
        },
        removeItem: (key: string) => {
          data.delete(key);
        },
      };
    });

    queue.add(paneA, "held");
    expect(queue.isUnsaved(paneA)).toBe(true);

    blocked = false;
    queue.add(paneA, "held again");
    expect(queue.isUnsaved(paneA)).toBe(false);

    const reopened = new ReferenceQueueStore(() => ({
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => {
        data.set(key, value);
      },
      removeItem: (key: string) => {
        data.delete(key);
      },
    }));
    expect(texts(reopened, paneA)).toEqual(["held", "held again"]);
  });
});
