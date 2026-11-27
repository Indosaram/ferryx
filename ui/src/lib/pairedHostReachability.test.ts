import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Handler = (event: { payload: unknown }) => void;
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
  listen: vi.fn(),
  handlers: new Map<string, Handler>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mocks.invoke(...args), isTauri: () => mocks.isTauri() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => mocks.listen(...args) }));
vi.mock("./tauri", () => ({ bootTrace: vi.fn().mockResolvedValue(undefined) }));

const entry = (hostId: string, generation: string, state: "checking" | "online" | "offline", checkedAtMs = 1) =>
  ({ hostId, generation, state, checkedAtMs, lastError: state === "offline" ? "unreachable" : null });

let resetStore: (() => void) | undefined;
async function load() {
  resetStore?.();
  vi.resetModules();
  const store = await import("./pairedHostReachability");
  resetStore = store.resetPairedHostReachabilityForTests;
  return store;
}
function fire(payload: unknown) {
  const handler = mocks.handlers.get("paired_host_reachability_changed");
  if (!handler) throw new Error("listener not registered");
  handler({ payload });
}

beforeEach(() => {
  mocks.handlers.clear();
  mocks.invoke.mockReset();
  mocks.isTauri.mockReturnValue(true);
  mocks.listen.mockReset().mockImplementation(async (name: string, handler: Handler) => {
    mocks.handlers.set(name, handler);
    return () => mocks.handlers.delete(name);
  });
});
afterEach(() => { resetStore?.(); vi.restoreAllMocks(); });

function deferred<T>() {
  let resolve: (value: T) => void = () => { throw new Error("uninitialized deferred"); };
  let reject: (error: unknown) => void = () => { throw new Error("uninitialized deferred"); };
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

describe("pairedHostReachability store", () => {
  it("retries transient lists with one listener and releases it exactly once", async () => {
    // Given a daemon that becomes available after startup.
    mocks.invoke.mockRejectedValueOnce({ code: "IO", message: "daemon unavailable" })
      .mockResolvedValue({ supported: true, entries: [entry("a", "2", "online")] });
    const dispose = vi.fn();
    mocks.listen.mockResolvedValue(dispose);
    const m = await load();
    // When bootstrap is retried after failure.
    await m.bootstrapPairedHostReachability();
    await Promise.all([m.bootstrapPairedHostReachability(), m.bootstrapPairedHostReachability()]);
    // Then list recovery does not replace subscription ownership.
    expect(m.getPairedHostReachability("a").supported).toBe(true);
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
    expect(mocks.listen).toHaveBeenCalledTimes(1);
    m.resetPairedHostReachabilityForTests();
    m.resetPairedHostReachabilityForTests();
    expect(dispose).toHaveBeenCalledTimes(1);
  });

  it.each([false, "UNKNOWN_COMMAND", "UNSUPPORTED", "Command paired_host_reachability_list not found"] as const)(
    "keeps explicit unsupported result %s terminal until a live event", async (failure) => {
      // Given an older daemon.
      if (failure === false) mocks.invoke.mockResolvedValue({ supported: false, entries: [] });
      else mocks.invoke.mockRejectedValue(failure.includes(" ") ? failure : { code: failure });
      const m = await load();
      await m.bootstrapPairedHostReachability();
      // When bootstrap repeats and a valid live event follows.
      await m.bootstrapPairedHostReachability();
      expect(m.getPairedHostReachability("a").supported).toBe(false);
      fire(entry("a", "1", "online"));
      // Then the event proves support without another list request.
      expect(m.getPairedHostReachability("a").supported).toBe(true);
      expect(mocks.invoke).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["unsupported", "failed"])("preserves event support against a stale %s list", async (outcome) => {
    // Given an in-flight list.
    const response = deferred<unknown>();
    const called = deferred<void>();
    mocks.invoke.mockImplementation(() => { called.resolve(); return response.promise; });
    const m = await load();
    const boot = m.bootstrapPairedHostReachability();
    await called.promise;
    // When a live event wins the race.
    fire(entry("a", "4", "online"));
    if (outcome === "unsupported") response.resolve({ supported: false, entries: [] });
    else response.reject({ code: "IO" });
    await boot;
    // Then list fallback cannot downgrade live support.
    expect(m.getPairedHostReachability("a")).toEqual({ supported: true, entry: entry("a", "4", "online") });
  });

  it("disposes a listener that resolves after reset and fences its old list", async () => {
    // Given pending subscription registration.
    const registration = deferred<() => void>();
    const dispose = vi.fn();
    mocks.listen.mockReturnValueOnce(registration.promise);
    const m = await load();
    const boot = m.bootstrapPairedHostReachability();
    // When reset takes ownership before registration completes.
    m.resetPairedHostReachabilityForTests();
    registration.resolve(dispose);
    await boot;
    // Then the late subscription is released without listing.
    expect(dispose).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("retries subscription registration after a listener failure", async () => {
    // Given a failed listener registration with a successful list.
    mocks.listen.mockRejectedValueOnce({ code: "IO" });
    mocks.invoke.mockResolvedValue({ supported: true, entries: [] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    // When bootstrap retries registration.
    await m.bootstrapPairedHostReachability();
    fire(entry("a", "1", "online"));
    // Then events reach the store through one successfully registered listener.
    expect(mocks.listen).toHaveBeenCalledTimes(2);
    expect(m.getPairedHostReachability("a").entry?.state).toBe("online");
  });

  it("stores the initial list from one invoke", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "3", "online"), entry("b", "1", "offline")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledWith("paired_host_reachability_list");
    expect(m.getPairedHostReachability("a")).toEqual({ supported: true, entry: entry("a", "3", "online") });
    expect(m.getPairedHostReachability("b").entry?.state).toBe("offline");
    expect(m.getPairedHostReachability("missing")).toEqual({ supported: true, entry: null });
  });

  it("is idempotent: a second bootstrap neither re-lists nor re-subscribes", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    await m.bootstrapPairedHostReachability();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.listen).toHaveBeenCalledTimes(1);
  });

  it("upserts by hostId on the change event and notifies the hook", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "3", "checking")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    const { result } = renderHook(() => m.usePairedHostReachability("a"));
    expect(result.current.entry?.state).toBe("checking");
    act(() => fire(entry("a", "3", "online", 2)));
    expect(result.current).toEqual({ supported: true, entry: entry("a", "3", "online", 2) });
    act(() => fire(entry("c", "1", "offline")));
    expect(m.getPairedHostReachability("c").entry?.state).toBe("offline");
  });

  it("drops an event whose generation is older than the stored one", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "5", "online")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    const listener = vi.fn();
    m.subscribePairedHostReachability(listener);
    fire(entry("a", "4", "offline", 99));
    expect(m.getPairedHostReachability("a").entry).toEqual(entry("a", "5", "online"));
    expect(listener).not.toHaveBeenCalled();
    fire(entry("a", "6", "offline", 0));
    expect(m.getPairedHostReachability("a").entry).toEqual(entry("a", "6", "offline", 0));
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("compares generations numerically, not lexically", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "10", "online")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    fire(entry("a", "9", "offline", 50));
    expect(m.getPairedHostReachability("a").entry?.generation).toBe("10");
  });

  it("a list result older than an event that raced it does not overwrite the event", async () => {
    let resolveList!: (value: unknown) => void;
    let listed!: () => void;
    const listCalled = new Promise<void>(resolve => { listed = resolve; });
    mocks.invoke.mockImplementation(() => { listed(); return new Promise(resolve => { resolveList = resolve; }); });
    const m = await load();
    const boot = m.bootstrapPairedHostReachability();
    await listCalled; // the listener is registered before the list is requested
    fire(entry("a", "7", "offline"));
    resolveList({ supported: true, entries: [entry("a", "6", "online")] });
    await boot;
    expect(m.getPairedHostReachability("a").entry).toEqual(entry("a", "7", "offline"));
  });

  it("drops malformed payloads at the boundary", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [{ hostId: "x", generation: "01", state: "online", checkedAtMs: 1, lastError: null }] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    expect(m.getPairedHostReachability("x").entry).toBeNull();
    fire({ hostId: "y", generation: "1", state: "bogus", checkedAtMs: 1, lastError: null });
    fire(null);
    expect(m.getPairedHostReachability("y").entry).toBeNull();
  });

  it("reports supported:false when the daemon lacks the capability", async () => {
    mocks.invoke.mockResolvedValue({ supported: false, entries: [] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    expect(m.getPairedHostReachability("a")).toEqual({ supported: false, entry: null });
  });

  it("treats an unknown command (older backend) as supported:false without throwing", async () => {
    mocks.invoke.mockRejectedValue({ code: "UNKNOWN_COMMAND", message: "Command paired_host_reachability_list not found" });
    const m = await load();
    await expect(m.bootstrapPairedHostReachability()).resolves.toBeUndefined();
    expect(m.getPairedHostReachability("a")).toEqual({ supported: false, entry: null });
  });

  it("treats a plain-string invoke rejection the same way", async () => {
    mocks.invoke.mockRejectedValue("Command paired_host_reachability_list not found");
    const m = await load();
    await expect(m.bootstrapPairedHostReachability()).resolves.toBeUndefined();
    expect(m.getPairedHostReachability("a").supported).toBe(false);
  });

  it("outside Tauri returns supported:false and makes no native calls", async () => {
    mocks.isTauri.mockReturnValue(false);
    const m = await load();
    await m.bootstrapPairedHostReachability();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
    const { result } = renderHook(() => m.usePairedHostReachability("a"));
    expect(result.current).toEqual({ supported: false, entry: null });
  });

  it("never polls: no setInterval or setTimeout during bootstrap and events", async () => {
    const interval = vi.spyOn(globalThis, "setInterval");
    const timeout = vi.spyOn(globalThis, "setTimeout");
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "1", "checking")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    fire(entry("a", "1", "online", 2));
    expect(m.getPairedHostReachability("a").entry?.state).toBe("online");
    expect(interval).not.toHaveBeenCalled();
    expect(timeout).not.toHaveBeenCalled();
  });

  it("the hook returns a stable object while nothing changes", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [entry("a", "1", "online")] });
    const m = await load();
    await m.bootstrapPairedHostReachability();
    const { result, rerender } = renderHook(() => m.usePairedHostReachability("a"));
    const first = result.current;
    rerender();
    expect(result.current).toBe(first);
  });
});
