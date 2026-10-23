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

type State = "relay" | "attempting" | "direct" | "disconnected";
const route = (hostId: string, state: State) => ({ hostId, state });

async function load() {
  vi.resetModules();
  return import("./pairedHostRoute");
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
afterEach(() => vi.restoreAllMocks());

describe("pairedHostRoute store", () => {
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
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "attempting") })).toBe("Relay");
    expect(m.pairedPathLabel("relay", { supported: true, entry: route("a", "disconnected") })).toBe("Relay");
    expect(m.pairedPathLabel("relay", { supported: true, entry: null })).toBe("Relay");
    expect(m.pairedPathLabel("relay", { supported: false, entry: null })).toBe("Relay");
    expect(m.pairedPathLabel("tailscale", { supported: false, entry: null })).toBe("tailscale");
  });
});
