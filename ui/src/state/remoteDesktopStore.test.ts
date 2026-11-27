import { describe, expect, it, vi } from "vitest";

import type { RdFrameMessage, RdInputEvent } from "../lib/remoteDesktop";
import { createRemoteDesktopStore, type RemoteDesktopServices, type RemoteDesktopStore } from "./remoteDesktopStore";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function status(state: "connecting" | "connected" | "reconnecting" | "ended", reason: string | null = null): RdFrameMessage {
  return { kind: "status", state, reason };
}

function harness() {
  const channels = new Map<string, (message: RdFrameMessage) => void>();
  const pending = new Map<string, ReturnType<typeof deferred<{ rdSessionId: string }>>>();
  const inputs: Array<{ rdSessionId: string; events: RdInputEvent[] }> = [];
  const services: RemoteDesktopServices = {
    connect: vi.fn((hostId: string, onFrame: (message: RdFrameMessage) => void) => {
      channels.set(hostId, onFrame);
      const result = deferred<{ rdSessionId: string }>();
      pending.set(hostId, result);
      return result.promise;
    }) as unknown as RemoteDesktopServices["connect"],
    disconnect: vi.fn(async () => undefined) as unknown as RemoteDesktopServices["disconnect"],
    input: vi.fn(async (rdSessionId: string, events: RdInputEvent[]) => {
      inputs.push({ rdSessionId, events });
    }) as unknown as RemoteDesktopServices["input"],
  };
  const store: RemoteDesktopStore = createRemoteDesktopStore(services);
  return { store, services, channels, pending, inputs };
}

async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("remoteDesktopStore", () => {
  it("starts empty and lists sessions by host", () => {
    const { store } = harness();
    expect(store.list()).toEqual([]);
    expect(store.getSession("host-1")).toBeNull();
  });

  it("connects once and reuses the connecting session for a second caller", async () => {
    const { store, services, pending } = harness();
    expect(store.getSession("host-1")).toBeNull();

    const first = store.connect("host-1", "Studio");
    expect(store.getSession("host-1")).toMatchObject({ hostId: "host-1", label: "Studio", status: "connecting", rdSessionId: null });
    const second = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await Promise.all([first, second]);

    expect(services.connect).toHaveBeenCalledTimes(1);
    expect(store.getSession("host-1")).toMatchObject({ status: "connecting", rdSessionId: "rd-1" });
    expect(store.list()).toHaveLength(1);
  });

  it("is idempotent for an already connected session and only bumps the label", async () => {
    const { store, services, pending } = harness();
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;

    await store.connect("host-1", "Studio Renamed");

    expect(services.connect).toHaveBeenCalledTimes(1);
    expect(store.getSession("host-1")).toMatchObject({ rdSessionId: "rd-1", label: "Studio Renamed" });
  });

  it("follows the frame channel's wire status and fans video frames to every subscriber", async () => {
    const { store, channels, pending } = harness();
    const seenA: RdFrameMessage[] = [];
    const seenB: RdFrameMessage[] = [];
    store.subscribeFrames("host-1", (message) => seenA.push(message));
    store.subscribeFrames("host-1", (message) => seenB.push(message));
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;

    channels.get("host-1")!(status("connected"));
    expect(store.getSession("host-1")).toMatchObject({ status: "connected", wireState: "connected", reason: null });
    const video: RdFrameMessage = { kind: "video", width: 2, height: 2, frameId: 7, jpeg: new Uint8Array([1, 2]) };
    channels.get("host-1")!(video);

    expect(seenA).toEqual([video]);
    expect(seenB).toEqual([video]);
  });

  it("does not report connected until the session id has resolved", async () => {
    const { store, channels } = harness();
    void store.connect("host-1", "Studio");
    channels.get("host-1")!(status("connected"));
    // Input needs a live session id, so the wire state alone is not enough.
    expect(store.getSession("host-1")).toMatchObject({ status: "connecting", wireState: "connected", rdSessionId: null });
  });

  it("surfaces a structured error by code, not by message text", async () => {
    const { store, pending } = harness();
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.reject({ code: "RD_HOST_OFFLINE", message: "The paired host is not reachable", details: {} });
    await started;

    expect(store.getSession("host-1")).toMatchObject({
      status: "error",
      rdSessionId: null,
      error: { code: "RD_HOST_OFFLINE" },
    });
  });

  it("reconnects from an error state with a fresh connect", async () => {
    const { store, services, pending } = harness();
    const failed = store.connect("host-1", "Studio");
    pending.get("host-1")!.reject({ code: "RD_HOST_DISABLED", message: "off" });
    await failed;

    const retry = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-2" });
    await retry;

    expect(services.connect).toHaveBeenCalledTimes(2);
    expect(store.getSession("host-1")).toMatchObject({ status: "connecting", rdSessionId: "rd-2", error: undefined });
  });

  it("disconnects only through the explicit action, once per session", async () => {
    const { store, services, pending, channels } = harness();
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;
    channels.get("host-1")!(status("connected"));

    await store.disconnect("host-1");

    expect(services.disconnect).toHaveBeenCalledTimes(1);
    expect(services.disconnect).toHaveBeenCalledWith("rd-1");
    expect(store.getSession("host-1")).toMatchObject({ status: "idle", rdSessionId: null, wireState: null });
    // A second disconnect has no live session left to tear down.
    await store.disconnect("host-1");
    expect(services.disconnect).toHaveBeenCalledTimes(1);
  });

  it("tears down a session that resolves after an explicit disconnect", async () => {
    const { store, services, pending } = harness();
    const started = store.connect("host-1", "Studio");
    await store.disconnect("host-1");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-late" });
    await started;

    expect(store.getSession("host-1")).toMatchObject({ status: "idle", rdSessionId: null });
    expect(services.disconnect).toHaveBeenCalledWith("rd-late");
    expect(services.disconnect).toHaveBeenCalledTimes(1);
  });

  it("ignores frames from a session that was torn down", async () => {
    const { store, channels, pending } = harness();
    const seen: RdFrameMessage[] = [];
    store.subscribeFrames("host-1", (message) => seen.push(message));
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;
    const stale = channels.get("host-1")!;
    await store.disconnect("host-1");

    stale(status("connected"));
    stale({ kind: "video", width: 1, height: 1, frameId: 1, jpeg: new Uint8Array([1]) });

    expect(store.getSession("host-1")).toMatchObject({ status: "idle", wireState: null });
    expect(seen).toEqual([]);
  });

  it("stops fanning frames to an unsubscribed canvas without dropping the session", async () => {
    const { store, channels, pending, services } = harness();
    const seen: RdFrameMessage[] = [];
    const unsubscribe = store.subscribeFrames("host-1", (message) => seen.push(message));
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;
    channels.get("host-1")!(status("connected"));
    unsubscribe();

    const video: RdFrameMessage = { kind: "video", width: 1, height: 1, frameId: 1, jpeg: new Uint8Array([1]) };
    channels.get("host-1")!(video);

    expect(seen).toEqual([]);
    expect(services.disconnect).not.toHaveBeenCalled();
    expect(store.getSession("host-1")).toMatchObject({ status: "connected", rdSessionId: "rd-1" });
  });

  it("sends input batches through the session's serializer and drops them once closed", async () => {
    const { store, pending, inputs } = harness();
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;

    store.sendInput("host-1", { kind: "key", code: "KeyA", down: true });
    await settle();
    expect(inputs).toEqual([{ rdSessionId: "rd-1", events: [{ kind: "key", code: "KeyA", down: true }] }]);

    await store.disconnect("host-1");
    store.sendInput("host-1", { kind: "key", code: "KeyA", down: false });
    await settle();
    expect(inputs).toHaveLength(1);
  });

  it("notifies subscribers on every state change", async () => {
    const { store, pending, channels } = harness();
    const listener = vi.fn();
    store.subscribe(listener);
    const started = store.connect("host-1", "Studio");
    expect(listener).toHaveBeenCalledTimes(1);
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;
    channels.get("host-1")!(status("connected"));
    expect(listener.mock.calls.length).toBeGreaterThanOrEqual(3);
  });

  it("reset drops every session and disconnects the live ones", async () => {
    const { store, services, pending } = harness();
    const started = store.connect("host-1", "Studio");
    pending.get("host-1")!.resolve({ rdSessionId: "rd-1" });
    await started;

    store.reset();

    expect(store.list()).toEqual([]);
    expect(services.disconnect).toHaveBeenCalledWith("rd-1");
  });
});
