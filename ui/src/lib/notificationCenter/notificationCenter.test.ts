import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createNotificationCenterStore } from "./notificationCenterStore";
import type { ActivityOccurrence, BellOccurrence } from "./types";

const KEY = "ferryx.notifications.history:v1";
const activity = (overrides: Partial<ActivityOccurrence> = {}): ActivityOccurrence => ({
  workspaceId: "workspace", sessionId: "session", subject: "agent",
  labels: { workspaceLabel: "Project", worktreeLabel: "main", agentLabel: "Claude", terminalTitle: "Task" },
  previousState: "working", state: "waiting", occurredAt: 100, observed: false,
  ...overrides,
});
const bell = (overrides: Partial<BellOccurrence> = {}): BellOccurrence => ({
  workspaceId: "workspace", sessionId: "session", labels: {}, subject: "terminal",
  occurredAt: 100, observed: false, ...overrides,
});
const stores: ReturnType<typeof createNotificationCenterStore>[] = [];
function create(options: Parameters<typeof createNotificationCenterStore>[0] = { storage: null }) {
  const store = createNotificationCenterStore(options);
  stores.push(store);
  return store;
}

beforeEach(() => { localStorage.clear(); vi.useFakeTimers(); vi.setSystemTime(1000); });
afterEach(() => { stores.splice(0).forEach((store) => store.dispose()); vi.useRealTimers(); });

describe("notification inbox occurrences", () => {
  it.each([
    ["working", "waiting"], ["working", "done"], ["waiting", "done"],
    ["done", "waiting"], ["idle", "waiting"], ["idle", "done"],
  ] as const)("records %s -> %s", (previousState, state) => {
    const store = create();
    expect(store.recordActivity(activity({ previousState, state }))).toBe(true);
    expect(store.getSnapshot().entries[0]).toMatchObject({
      id: JSON.stringify(["session", "workspace", "session"]), reason: state,
      revision: 1, occurrenceCount: 1, firstOccurredAt: 100, lastOccurredAt: 100,
      updateOrder: 1, read: { unread: true },
    });
  });

  it.each([
    { previousState: "done", state: "done" },
    { previousState: "waiting", state: "waiting" },
    { previousState: undefined, state: "done" },
    { previousState: "working", state: "working" },
    { previousState: "done", state: "idle" },
    { notificationSuppressed: true },
  ] as const)("rejects no-op/baseline observations %j", (overrides) => {
    const store = create();
    const before = store.getSnapshot();
    expect(store.recordActivity(activity(overrides))).toBe(false);
    expect(store.getSnapshot()).toBe(before);
  });

  it("coalesces repeats, refreshes metadata, and recomputes seen rev1 to unread rev2", () => {
    const store = create();
    store.recordActivity(activity({ observed: true }));
    expect(store.getSnapshot().entries[0].read).toEqual({ seen: true, seenAt: 100 });
    store.recordBell(bell({ sessionId: "other", occurredAt: 150 }));
    store.recordActivity(activity({ previousState: "waiting", state: "done", occurredAt: 200,
      labels: { terminalTitle: "Finished" } }));
    expect(store.getSnapshot().entries).toHaveLength(2);
    expect(store.getSnapshot().entries[0]).toMatchObject({
      sessionId: "session", reason: "done", labels: { terminalTitle: "Finished" },
      revision: 2, occurrenceCount: 2, firstOccurredAt: 100, lastOccurredAt: 200,
      updateOrder: 3, read: { unread: true },
    });
    expect(store.getSnapshot().entries[0].labels.agentLabel).toBeUndefined();
    expect(store.getSnapshot().nextUpdateOrder).toBe(4);
  });

  it("rejects duplicate edges without notifying or modifying a snapshot", () => {
    const store = create();
    const listener = vi.fn();
    const unsubscribe = store.subscribe(listener);
    const occurrence = activity({ occurrenceId: "edge-1" });
    store.recordActivity(occurrence);
    const snapshot = store.getSnapshot();
    expect(store.recordActivity({ ...occurrence, occurredAt: 101, labels: { terminalTitle: "refresh" } })).toBe(false);
    expect(store.getSnapshot()).toBe(snapshot);
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
    store.recordActivity(activity({ occurrenceId: "edge-2", occurredAt: 200 }));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getSnapshot().entries[0].revision).toBe(2);
  });

  it("deduplicates identical timestamped edges without an explicit occurrence ID", () => {
    const store = create();
    expect(store.recordActivity(activity())).toBe(true);
    expect(store.recordActivity(activity())).toBe(false);
    expect(store.getSnapshot().entries[0].occurrenceCount).toBe(1);
  });

  it("coalesces accepted bells with activity by session and rejects duplicate bells", () => {
    const store = create();
    store.recordActivity(activity());
    expect(store.recordBell(bell({ occurredAt: 200 }))).toBe(true);
    expect(store.recordBell(bell({ occurredAt: 200 }))).toBe(false);
    expect(store.recordBell(bell({ occurredAt: 300, observed: true }))).toBe(true);
    expect(store.getSnapshot().entries).toHaveLength(1);
    expect(store.getSnapshot().entries[0]).toMatchObject({
      subject: "terminal", reason: "bell", occurrenceCount: 3, revision: 3,
      firstOccurredAt: 100, lastOccurredAt: 300, read: { seen: true, seenAt: 300 },
    });
  });

  it("keeps collision-safe identities across workspaces", () => {
    const store = create();
    store.recordBell(bell({ workspaceId: "a:b", sessionId: "c" }));
    store.recordBell(bell({ workspaceId: "a", sessionId: "b:c" }));
    expect(new Set(store.getSnapshot().entries.map((entry) => entry.id)).size).toBe(2);
  });
});

describe("read acknowledgements and retention", () => {
  it("revision guard skips a stale read ack after a newer occurrence", () => {
    const store = create();
    store.recordActivity(activity());
    const clicked = store.getSnapshot().entries[0];
    store.recordActivity(activity({ previousState: "waiting", state: "done", occurredAt: 200 }));
    const before = store.getSnapshot();
    store.markEntriesRead([{ id: clicked.id, expectedRevision: clicked.revision }], 300);
    expect(store.getSnapshot()).toBe(before);
    expect(store.getSnapshot().entries[0].read).toEqual({ unread: true });
    store.markEntriesRead([{ id: clicked.id, expectedRevision: 2 }], 400);
    expect(store.getSnapshot().entries[0].read).toEqual({ seen: true, seenAt: 400 });
    expect(store.getSnapshot().entries[0].revision).toBe(2);
  });

  it("marks all read, dismisses, and clears without changing order or no-op snapshots", () => {
    const store = create();
    store.recordBell(bell());
    store.recordBell(bell({ sessionId: "second" }));
    const ids = store.getSnapshot().entries.map((entry) => entry.id);
    store.markAllRead(500);
    expect(store.getSnapshot().entries.map((entry) => entry.id)).toEqual(ids);
    expect(store.getSnapshot().entries.every((entry) => "seen" in entry.read)).toBe(true);
    const seen = store.getSnapshot();
    store.markAllRead(600);
    store.markEntriesRead([{ id: "missing", expectedRevision: 1 }]);
    store.dismissEntry("missing");
    expect(store.getSnapshot()).toBe(seen);
    store.dismissEntry(ids[0]);
    expect(store.getSnapshot().entries.map((entry) => entry.id)).toEqual([ids[1]]);
    store.clearAll();
    expect(store.getSnapshot().entries).toEqual([]);
    const empty = store.getSnapshot();
    store.clearAll();
    expect(store.getSnapshot()).toBe(empty);
  });

  it.each([199, 200, 201])("caps %i rows, evicting oldest seen before oldest unread", (count) => {
    const store = create();
    for (let index = 0; index < count; index++) {
      store.recordBell(bell({ sessionId: `s${index}`, occurredAt: index, observed: index === 50 || index === 100 }));
    }
    const sessions = store.getSnapshot().entries.map((entry) => entry.sessionId);
    expect(sessions).toHaveLength(Math.min(count, 200));
    expect(sessions[0]).toBe(`s${count - 1}`);
    expect(sessions.at(-1)).toBe("s0");
    expect(sessions.includes("s50")).toBe(count <= 200);
    expect(sessions).toContain("s100");
  });

  it("evicts the oldest unread when none are seen, using update order not wall time", () => {
    const store = create();
    for (let index = 0; index < 201; index++) {
      store.recordBell(bell({ sessionId: `s${index}`, occurredAt: 500 - index }));
    }
    expect(store.getSnapshot().entries.at(-1)?.sessionId).toBe("s1");
    expect(store.getSnapshot().entries.some((entry) => entry.sessionId === "s0")).toBe(false);
  });
});

describe("notification persistence", () => {
  it("debounces and round-trips unread history before any subscription", () => {
    const store = create({ storage: localStorage, debounceMs: 50 });
    store.recordActivity(activity());
    vi.advanceTimersByTime(49);
    expect(localStorage.getItem(KEY)).toBeNull();
    store.recordBell(bell({ sessionId: "second" }));
    vi.advanceTimersByTime(49);
    expect(localStorage.getItem(KEY)).toBeNull();
    vi.advanceTimersByTime(1);
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(store.getSnapshot());
    const restored = create({ storage: localStorage });
    expect(restored.getSnapshot()).toEqual(store.getSnapshot());
    restored.recordBell(bell({ sessionId: "third" }));
    expect(restored.getSnapshot().entries[0].updateOrder).toBe(3);
  });

  it.each(["pagehide", "visibilitychange"])("flushes pending writes on %s", (event) => {
    const store = create({ storage: localStorage, debounceMs: 500 });
    const schedule = vi.spyOn(globalThis, "setTimeout");
    const cancel = vi.spyOn(globalThis, "clearTimeout");
    store.recordBell(bell());
    const debounceTimer = schedule.mock.results.at(-1)!.value;
    expect(localStorage.getItem(KEY)).toBeNull();
    if (event === "visibilitychange") {
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
      document.dispatchEvent(new Event(event));
      expect(localStorage.getItem(KEY)).toBeNull();
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      document.dispatchEvent(new Event(event));
    } else window.dispatchEvent(new Event(event));
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(store.getSnapshot());
    expect(cancel).toHaveBeenCalledWith(debounceTimer);
    const write = vi.spyOn(Storage.prototype, "setItem");
    vi.advanceTimersByTime(500);
    expect(write).not.toHaveBeenCalled();
  });

  it("explicit flush and dispose persist the latest state and detach listeners", () => {
    const store = create({ storage: localStorage });
    store.recordBell(bell());
    store.flush();
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(store.getSnapshot());
    store.clearAll();
    store.dispose();
    expect(JSON.parse(localStorage.getItem(KEY)!).entries).toEqual([]);
    const write = vi.spyOn(Storage.prototype, "setItem");
    window.dispatchEvent(new Event("pagehide"));
    expect(write).not.toHaveBeenCalled();
  });

  it.each(["{broken", "null", "[]", '{"version":2,"entries":[]}', '{"version":1,"entries":{}}'])
  ("restores empty for corrupt payload %s", (raw) => {
    localStorage.setItem(KEY, raw);
    const store = create({ storage: localStorage });
    expect(store.getSnapshot()).toEqual({ version: 1, nextUpdateOrder: 1, entries: [] });
    expect(() => store.recordBell(bell())).not.toThrow();
  });

  it("validates, deduplicates, sorts, normalizes counters, and caps restored entries", () => {
    const seed = create();
    seed.recordBell(bell());
    const entry = seed.getSnapshot().entries[0];
    const entries = Array.from({ length: 201 }, (_, index) => ({
      ...entry, id: JSON.stringify(["session", "workspace", `s${index}`]), sessionId: `s${index}`,
      updateOrder: index + 1, read: index === 50 ? { seen: true, seenAt: 200 } : { unread: true },
    }));
    localStorage.setItem(KEY, JSON.stringify({ version: 1, nextUpdateOrder: -1, entries: [
      ...entries, entries[0], { ...entry, id: "wrong" }, { ...entry, revision: -1 },
      { ...entry, read: { seen: true } }, { ...entry, labels: { terminalTitle: 123 } },
    ] }));
    const restored = create({ storage: localStorage }).getSnapshot();
    expect(restored.entries).toHaveLength(200);
    expect(restored.entries[0].sessionId).toBe("s200");
    expect(restored.entries.some((value) => value.sessionId === "s50")).toBe(false);
    expect(restored.entries.at(-1)?.sessionId).toBe("s0");
    expect(restored.nextUpdateOrder).toBe(202);
  });

  it.each(["orca", "rorca"])("migrates %s history through the storage-key convention", (prefix) => {
    const seed = create();
    seed.recordBell(bell());
    localStorage.setItem(`${prefix}.notifications.history:v1`, JSON.stringify(seed.getSnapshot()));
    const restored = create({ storage: localStorage });
    expect(restored.getSnapshot()).toEqual(seed.getSnapshot());
    expect(localStorage.getItem(KEY)).toBe(JSON.stringify(seed.getSnapshot()));
  });

  it("retains in-memory state on quota failure, reports once, and can retry flush", () => {
    const onError = vi.fn();
    let blocked = true;
    const quota = new DOMException("Quota exceeded", "QuotaExceededError");
    const storage = { getItem: vi.fn(() => null), setItem: vi.fn((key: string, value: string) => {
      if (blocked) throw quota;
      localStorage.setItem(key, value);
    }) };
    const store = create({ storage, onError });
    store.recordBell(bell());
    const before = store.getSnapshot();
    store.flush();
    store.flush();
    expect(store.getSnapshot()).toBe(before);
    expect(onError).toHaveBeenCalledExactlyOnceWith(quota);
    store.recordBell(bell({ occurredAt: 200 }));
    store.flush();
    expect(store.getSnapshot().entries[0].revision).toBe(2);
    expect(onError).toHaveBeenCalledTimes(1);
    blocked = false;
    store.flush();
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(store.getSnapshot());
  });

  it("reports storage access failure once and keeps recording", () => {
    const onError = vi.fn();
    const failure = new Error("Storage disabled");
    const store = create({ storage: { getItem: () => { throw failure; }, setItem: () => { throw failure; } }, onError });
    expect(store.getSnapshot().entries).toEqual([]);
    store.recordActivity(activity());
    store.flush();
    expect(store.getSnapshot().entries).toHaveLength(1);
    expect(onError).toHaveBeenCalledExactlyOnceWith(failure);
  });
});
