/**
 * Ported from the pinned upstream `src/lib/composerDraft.test.ts`
 * (`devswha/herdr-web-ui` @ `54e5a1f6…`, MIT). Authored, NOT EXECUTED — execution override.
 */
import { describe, expect, it } from "vitest";
import type { TargetRef } from "../../lib/scopedContracts";
import { REFERENCE_DRAFT_STORAGE_PREFIX, ReferenceDraftStore } from "./referenceDraft";
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
  return { data, store: new ReferenceDraftStore(() => storage) };
}

describe("reference draft target scoping", () => {
  it("keys a draft on the frozen target key, ignoring the provider session", () => {
    const { store } = fixture();
    expect(store.storageKey(paneA)).toBe(REFERENCE_DRAFT_STORAGE_PREFIX + referenceDraftKey(paneA));
    expect(store.targetKey(paneA)).toBe(referenceDraftKey(paneA));
    expect(store.targetKey(target({}, "provider-7"))).toBe(store.targetKey(paneA));
  });

  it("keeps a draft per target so a cross-target edit never leaks", () => {
    const { store, data } = fixture();
    store.set(paneA, "draft on A");
    store.set(paneB, "draft on B");
    store.set(otherHost, "draft on another host");

    expect(store.read(paneA).text).toBe("draft on A");
    expect(store.read(paneB).text).toBe("draft on B");
    expect(store.read(otherHost).text).toBe("draft on another host");

    store.set(paneB, "edited B");
    expect(store.read(paneA).text).toBe("draft on A");
    expect(data.get(store.storageKey(paneA))).toBe("draft on A");
    expect(data.get(store.storageKey(otherHost))).toBe("draft on another host");
  });

  it("treats another epoch or owner as a different pane with its own storage slot", () => {
    const { store } = fixture();
    const otherEpoch = target({ epoch: "7" });
    const otherOwner = target({ ownerId: "owner-b" });
    store.set(paneA, "A");
    expect(store.read(otherEpoch).text).toBe("");
    expect(store.read(otherOwner).text).toBe("");
    expect(store.storageKey(otherEpoch)).not.toBe(store.storageKey(paneA));
    expect(store.storageKey(otherOwner)).not.toBe(store.storageKey(paneA));
  });
});

describe("reference draft acknowledgement", () => {
  it("clears the sent prefix only and leaves later edits on another target intact", () => {
    const { store, data } = fixture();
    store.set(paneA, "hello");
    store.set(paneB, "held for B");
    store.begin(paneA, "hello");
    store.set(paneA, "hello world");

    expect(store.settle(paneA, "hello")).toEqual({ text: " world", edited: false });
    expect(store.read(paneA).text).toBe(" world");
    expect(store.read(paneB).text).toBe("held for B");
    expect(data.get(store.storageKey(paneA))).toBe(" world");
    expect(data.get(store.storageKey(paneB))).toBe("held for B");
  });

  it("clears a fully acknowledged draft and drops its storage slot without touching a neighbour", () => {
    const { store, data } = fixture();
    store.set(paneA, "sent");
    store.begin(paneA);
    store.set(paneB, "other pane");
    expect(store.begin(paneA)).toBe(false);
    store.settle(paneA, "sent");
    store.end(paneA);

    expect(store.read(paneA)).toEqual({ text: "", sending: false });
    expect(data.has(store.storageKey(paneA))).toBe(false);
    expect(store.read(paneB).text).toBe("other pane");
  });

  it("preserves appended text, internal edits and newer edits from another tab", () => {
    const { store, data } = fixture();
    store.set(paneA, "sent plus next");
    expect(store.settle(paneA, "sent")).toEqual({ text: " plus next", edited: false });

    store.set(paneA, "edited sent");
    expect(store.settle(paneA, "sent")).toEqual({ text: "edited sent", edited: true });

    data.set(store.storageKey(paneA), "another tab's draft");
    expect(store.settle(paneA, "sent").text).toBe("another tab's draft");
  });

  it("keeps a draft cleared and retyped while its send was on its way", () => {
    const { store } = fixture();
    store.set(paneA, "a");
    store.begin(paneA, "a");
    store.set(paneA, "");
    store.set(paneA, "ab");
    expect(store.settle(paneA, "a")).toEqual({ text: "ab", edited: true });
    store.end(paneA);

    store.set(paneA, "x");
    store.begin(paneA, "x");
    store.set(paneA, "xy");
    expect(store.settle(paneA, "x")).toEqual({ text: "y", edited: false });
    store.end(paneA);
  });

  it("keeps a draft another tab cleared and retyped while this tab's send was on its way", () => {
    const { store, data } = fixture();
    store.set(paneA, "sent");
    store.begin(paneA, "sent");
    data.delete(store.storageKey(paneA));
    store.refresh(paneA);
    data.set(store.storageKey(paneA), "sent again");
    store.refresh(paneA);
    expect(store.settle(paneA, "sent")).toEqual({ text: "sent again", edited: true });
    store.end(paneA);
  });

  it("reconciles a raw storage event for the same target before the acknowledgement lands", () => {
    const { store, data } = fixture();
    store.set(paneA, "sent");
    store.begin(paneA, "sent");
    data.delete(store.storageKey(paneA));
    store.refreshStorageKey(store.storageKey(paneA));
    data.set(store.storageKey(paneA), "sent again");
    store.refreshStorageKey(store.storageKey(paneA));
    expect(store.settle(paneA, "sent")).toEqual({ text: "sent again", edited: true });
    store.end(paneA);
  });

  it("treats a cross-tab append that still starts with the sent text as unedited", () => {
    const { store, data } = fixture();
    store.set(paneA, "sent");
    store.begin(paneA, "sent");
    data.set(store.storageKey(paneA), "sent plus next");
    store.refreshStorageKey(store.storageKey(paneA));
    expect(store.settle(paneA, "sent")).toEqual({ text: " plus next", edited: false });
    store.end(paneA);
  });
});

describe("reference draft notification and failure", () => {
  it("notifies a remounted composer and keeps a failed send's draft", () => {
    const { store } = fixture();
    store.set(paneA, "sent");
    store.begin(paneA);
    let updates = 0;
    const off = store.subscribe(() => {
      updates += 1;
    });
    store.end(paneA);
    expect(store.read(paneA).text).toBe("sent");
    store.settle(paneA, "sent");
    expect(updates).toBe(2);
    off();
  });

  it("retains unsaved edits when browser storage is unavailable", () => {
    const store = new ReferenceDraftStore(() => {
      throw new Error("blocked");
    });
    store.set(paneA, "sent then next");
    expect(store.settle(paneA, "sent").text).toBe(" then next");
  });

  it("makes a storage failure visible and clears it once a write succeeds again", () => {
    const data = new Map<string, string>();
    let blocked = true;
    const store = new ReferenceDraftStore(() => {
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

    store.set(paneA, "typed while blocked");
    expect(store.isUnsaved(paneA)).toBe(true);
    expect(store.isUnsaved(paneB)).toBe(false);
    expect(store.read(paneA).text).toBe("typed while blocked");

    blocked = false;
    store.set(paneA, "typed while blocked");
    expect(store.isUnsaved(paneA)).toBe(false);
    expect(data.get(store.storageKey(paneA))).toBe("typed while blocked");
  });

  it("does not discard an unsaved draft when a refresh cannot read storage", () => {
    const store = new ReferenceDraftStore(() => {
      throw new Error("denied");
    });
    store.set(paneA, "keep me");
    store.refresh(paneA);
    expect(store.read(paneA).text).toBe("keep me");
    expect(store.isUnsaved(paneA)).toBe(true);
  });
});
