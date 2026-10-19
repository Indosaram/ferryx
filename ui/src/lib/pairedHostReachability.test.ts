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

async function load() {
  vi.resetModules();
  return import("./pairedHostReachability");
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
afterEach(() => vi.restoreAllMocks());

describe("pairedHostReachability store", () => {
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
