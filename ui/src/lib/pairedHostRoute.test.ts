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

type State = "relay" | "attempting" | "direct" | "ssh" | "disconnected";
const route = (hostId: string, state: State) => ({ hostId, state });

let resetStore: (() => void) | undefined;
async function load() {
  resetStore?.();
  vi.resetModules();
  const store = await import("./pairedHostRoute");
  resetStore = store.resetPairedHostRouteForTests;
  return store;
}
function fire(payload: unknown) {
  const handler = mocks.handlers.get("paired_host_route_changed");
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

describe("pairedHostRoute store", () => {
  it("renews the list precedence fence on retry without registering another listener", async () => {
    // Given a failed first list and an event received between attempts.
    mocks.invoke.mockRejectedValueOnce({ code: "IO" });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    fire(route("a", "relay"));
    const response = deferred<unknown>();
    const called = deferred<void>();
    mocks.invoke.mockImplementationOnce(() => { called.resolve(); return response.promise; });
    // When a fresh event races the retried list.
    const retry = m.bootstrapPairedHostRoute();
    await called.promise;
    fire(route("b", "direct"));
    response.resolve({ supported: true, entries: [route("a", "direct"), route("b", "attempting")] });
    await retry;
    // Then only this attempt's event overrides the list.
    expect(m.getPairedHostRoute("a").entry).toEqual(route("a", "direct"));
    expect(m.getPairedHostRoute("b").entry).toEqual(route("b", "direct"));
    expect(mocks.listen).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });

  it.each([false, "UNKNOWN_COMMAND", "UNSUPPORTED", "Command paired_host_route_list not found"] as const)(
    "keeps unsupported result %s terminal until a live event", async (failure) => {
      // Given an older daemon.
      if (failure === false) mocks.invoke.mockResolvedValue({ supported: false, entries: [] });
      else mocks.invoke.mockRejectedValue(failure.includes(" ") ? failure : { code: failure });
      const m = await load();
      await m.bootstrapPairedHostRoute();
      // When bootstrap repeats and an event follows.
      await m.bootstrapPairedHostRoute();
      expect(m.getPairedHostRoute("a").supported).toBe(false);
      fire(route("a", "direct"));
      // Then live support is visible without another list.
      expect(m.getPairedHostRoute("a").supported).toBe(true);
      expect(mocks.invoke).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["unsupported", "failed"])("preserves event support against a stale %s list", async (outcome) => {
    // Given an in-flight list.
    const response = deferred<unknown>();
    const called = deferred<void>();
    mocks.invoke.mockImplementation(() => { called.resolve(); return response.promise; });
    const m = await load();
    const boot = m.bootstrapPairedHostRoute();
    await called.promise;
    // When a live event wins the race.
    fire(route("a", "direct"));
    if (outcome === "unsupported") response.resolve({ supported: false, entries: [] });
    else response.reject({ code: "IO" });
    await boot;
    // Then list fallback cannot downgrade live support.
    expect(m.getPairedHostRoute("a")).toEqual({ supported: true, entry: route("a", "direct") });
  });

  it("releases both current and late subscription handles exactly once on reset", async () => {
    // Given a pending registration.
    const registration = deferred<() => void>();
    const lateDispose = vi.fn();
    mocks.listen.mockReturnValueOnce(registration.promise);
    const m = await load();
    const boot = m.bootstrapPairedHostRoute();
    // When reset precedes registration completion.
    m.resetPairedHostRouteForTests();
    registration.resolve(lateDispose);
    await boot;
    // Then the late handle is released and a new lifecycle owns its own handle.
    expect(lateDispose).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).not.toHaveBeenCalled();
    const currentDispose = vi.fn();
    mocks.listen.mockResolvedValueOnce(currentDispose);
    mocks.invoke.mockResolvedValue({ supported: true, entries: [] });
    await m.bootstrapPairedHostRoute();
    m.resetPairedHostRouteForTests();
    m.resetPairedHostRouteForTests();
    expect(currentDispose).toHaveBeenCalledTimes(1);
    expect(lateDispose).toHaveBeenCalledTimes(1);
  });

  it("retries subscription registration after a listener failure", async () => {
    // Given a failed listener registration with a successful list.
    mocks.listen.mockRejectedValueOnce({ code: "IO" });
    mocks.invoke.mockResolvedValue({ supported: true, entries: [] });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    // When bootstrap retries registration.
    await m.bootstrapPairedHostRoute();
    fire(route("a", "direct"));
    // Then events reach the store through the recovered listener.
    expect(mocks.listen).toHaveBeenCalledTimes(2);
    expect(m.getPairedHostRoute("a").entry?.state).toBe("direct");
  });

  it("stores the initial list from one invoke", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [route("a", "direct"), route("b", "relay")] });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
    expect(mocks.invoke).toHaveBeenCalledWith("paired_host_route_list");
    expect(m.getPairedHostRoute("a")).toEqual({ supported: true, entry: route("a", "direct") });
    expect(m.getPairedHostRoute("b").entry?.state).toBe("relay");
    expect(m.getPairedHostRoute("missing")).toEqual({ supported: true, entry: null });
  });

  it("subscribes before listing and is idempotent", async () => {
    const order: string[] = [];
    mocks.listen.mockImplementation(async (name: string, handler: Handler) => {
      order.push("listen");
      mocks.handlers.set(name, handler);
      return () => undefined;
    });
    mocks.invoke.mockImplementation(async () => { order.push("list"); return { supported: true, entries: [] }; });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    await m.bootstrapPairedHostRoute();
    expect(order).toEqual(["listen", "list"]);
  });

  it("upserts by hostId on the change event and notifies the hook", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [route("a", "relay")] });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    const { result } = renderHook(() => m.usePairedHostRoute("a"));
    expect(result.current.entry?.state).toBe("relay");
    act(() => fire(route("a", "direct")));
    expect(result.current).toEqual({ supported: true, entry: route("a", "direct") });
    act(() => fire(route("a", "ssh")));
    expect(result.current).toEqual({ supported: true, entry: route("a", "ssh") });
    expect(m.pairedPathLabel("relay", result.current)).toBe("SSH");
    act(() => fire(route("a", "disconnected")));
    expect(result.current.entry?.state).toBe("disconnected");
  });

  it("an event that raced the list wins over the list result", async () => {
    let resolveList!: (value: unknown) => void;
    let listed!: () => void;
    const listCalled = new Promise<void>(resolve => { listed = resolve; });
    mocks.invoke.mockImplementation(() => { listed(); return new Promise(resolve => { resolveList = resolve; }); });
    const m = await load();
    const boot = m.bootstrapPairedHostRoute();
    await listCalled;
    fire(route("a", "direct"));
    resolveList({ supported: true, entries: [route("a", "attempting"), route("b", "relay")] });
    await boot;
    expect(m.getPairedHostRoute("a").entry).toEqual(route("a", "direct"));
    expect(m.getPairedHostRoute("b").entry).toEqual(route("b", "relay"));
  });

  it("drops malformed payloads and strips unknown fields at the boundary", async () => {
    mocks.invoke.mockResolvedValue({ supported: true, entries: [{ hostId: "x", state: "bogus" }] });
    const m = await load();
    await m.bootstrapPairedHostRoute();
    expect(m.getPairedHostRoute("x").entry).toBeNull();
    fire({ hostId: "", state: "direct" });
    fire(null);
    fire({ hostId: "y", state: "direct", origin: "http://127.0.0.1:1" });
    expect(m.getPairedHostRoute("y").entry).toEqual(route("y", "direct"));
  });

  it("reports supported:false when the daemon lacks the capability or the command is missing", async () => {
    mocks.invoke.mockResolvedValue({ supported: false, entries: [] });
    let m = await load();
    await m.bootstrapPairedHostRoute();
    expect(m.getPairedHostRoute("a")).toEqual({ supported: false, entry: null });
    mocks.invoke.mockRejectedValue({ code: "UNKNOWN_COMMAND", message: "Command paired_host_route_list not found" });
    m = await load();
    await expect(m.bootstrapPairedHostRoute()).resolves.toBeUndefined();
    expect(m.getPairedHostRoute("a")).toEqual({ supported: false, entry: null });
  });

  it("outside Tauri makes no native calls", async () => {
    mocks.isTauri.mockReturnValue(false);
    const m = await load();
    await m.bootstrapPairedHostRoute();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(mocks.listen).not.toHaveBeenCalled();
  });

  it("labels the path from the route when supported, else from the transport", async () => {
    const m = await load();
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "direct") })).toBe("Direct");
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "attempting") })).toBe("Connecting");
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "disconnected") })).toBe("Offline");
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "relay") })).toBe("Relay");
    expect(m.pairedPathLabel("relay", { supported: true, entry: null })).toBe("Relay");
    expect(m.pairedPathLabel("tailscale", { supported: true, entry: null })).toBe("tailscale");
    expect(m.pairedPathLabel("relay", { supported: false, entry: null })).toBe("Relay");
    expect(m.pairedPathLabel("tailscale", { supported: false, entry: null })).toBe("tailscale");
  });
});
