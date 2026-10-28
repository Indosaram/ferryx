// useMachineEventSockets: every reachable machine the active connection does not cover gets its
// own events socket, and its frames reach the supervisor frame bus.
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AccountSessionError, REMOTE_SUSPENDED, REMOTE_SUSPENDED_CLOSE_REASON, type AccountMachineView } from "../accountSession";
import type { TunnelMessageEvent, TunnelWebSocket } from "../attachTunnel";
import { isSupervisorChangedFrame } from "../remoteSupervisor";
import {
  createSupervisorFrameBus,
  MACHINE_EVENTS_RETRY_MAX_MS,
  useMachineEventSockets,
  type MachineEventSocketsInput,
  type MachineLink,
} from "./machineSupervisorSources";

const machine = (machineId: string, online = true): AccountMachineView => ({
  machineRecordId: `rec-${machineId}`, machineId, displayName: machineId, publicKey: "", attachPublicKey: "", relayOrigin: "",
  platform: "linux", online, enrollmentEpoch: "1", lastSeenAt: 0,
});

type FakeSocket = Pick<TunnelWebSocket, "close" | "onmessage" | "onclose" | "onerror"> & { closed: boolean };

function fakeOpener(refuse?: () => Error | null) {
  const opened: Array<{ machineId: string; deviceToken: string; socket: FakeSocket }> = [];
  const open = vi.fn(async (target: AccountMachineView, deviceToken: string) => {
    const refusal = refuse?.();
    if (refusal) throw refusal;
    const socket: FakeSocket = { closed: false, onmessage: null, onclose: null, onerror: null, close: () => { socket.closed = true; } };
    opened.push({ machineId: target.machineId, deviceToken, socket });
    return socket;
  });
  return { open, opened };
}

const link = (deviceToken: string): MachineLink => ({ transport: { fetchLike: vi.fn() }, deviceToken });

function render(over: Partial<MachineEventSocketsInput> = {}, refuse?: () => Error | null) {
  const bus = createSupervisorFrameBus();
  const { open, opened } = fakeOpener(refuse);
  const onPlanLimit = vi.fn();
  const links: Record<string, MachineLink> = { studio: link("tok-studio"), laptop: link("tok-laptop"), gone: link("tok-gone") };
  const input: MachineEventSocketsInput = {
    enabled: true,
    machines: [machine("studio"), machine("laptop"), machine("gone", false)],
    linkFor: (id) => links[id] ?? null,
    skip: "laptop",
    open,
    bus,
    onPlanLimit,
    ...over,
  };
  const hook = renderHook((props: MachineEventSocketsInput) => useMachineEventSockets(props), { initialProps: input });
  return { hook, bus, open, opened, input, onPlanLimit };
}

const frame = (data: string): TunnelMessageEvent => ({ data });

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("useMachineEventSockets", () => {
  it("opens one socket per reachable machine the active connection does not cover, and relays its frames", async () => {
    const { bus, opened } = render();
    await act(async () => {});

    // laptop is the active connection's machine; gone is offline.
    expect(opened.map(({ machineId, deviceToken }) => [machineId, deviceToken])).toEqual([["studio", "tok-studio"]]);
    const received: unknown[] = [];
    bus.subscribe("studio", (event) => received.push(event));
    const change = JSON.stringify({ type: "supervisorChanged", goalId: "g-1" });
    act(() => opened[0].socket.onmessage?.(frame(change)));
    expect(received).toEqual([change]);
    expect(isSupervisorChangedFrame(received[0])).toBe(true);
  });

  it("reopens a closed socket after a bounded backoff and closes it on unmount", async () => {
    vi.useFakeTimers();
    const { hook, opened } = render();
    await act(async () => {});
    expect(opened).toHaveLength(1);

    act(() => opened[0].socket.onclose?.({ code: 1006, reason: "" } as never));
    await act(async () => { await vi.advanceTimersByTimeAsync(MACHINE_EVENTS_RETRY_MAX_MS); });
    expect(opened).toHaveLength(2);
    expect(opened[1].machineId).toBe("studio");

    hook.unmount();
    expect(opened[1].socket.closed).toBe(true);
  });

  it("reports the exact suspension close and never reopens that socket", async () => {
    vi.useFakeTimers();
    const { opened, onPlanLimit } = render();
    await act(async () => {});

    act(() => opened[0].socket.onclose?.({ code: 1008, reason: REMOTE_SUSPENDED_CLOSE_REASON, wasClean: true } as never));
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });

    expect(onPlanLimit).toHaveBeenCalledWith({ code: REMOTE_SUSPENDED });
    expect(opened).toHaveLength(1);
  });

  it("reports a suspension close that reaches a socket already let go of by a re-key", async () => {
    vi.useFakeTimers();
    const { hook, opened, input, onPlanLimit } = render({ skip: null });
    await act(async () => {});
    const before = opened.map(({ socket }) => socket);

    // A worktree opens on laptop: the hook re-keys and closes the sockets it had.
    hook.rerender({ ...input, skip: "laptop" });
    expect(before.every((socket) => socket.closed)).toBe(true);
    act(() => before[0].onclose?.({ code: 1008, reason: REMOTE_SUSPENDED_CLOSE_REASON, wasClean: true } as never));
    expect(onPlanLimit).toHaveBeenCalledWith({ code: REMOTE_SUSPENDED });

    // A plain close of a socket the hook closed itself is never reopened.
    const opens = opened.length;
    act(() => before[1].onclose?.({ code: 1000, reason: "", wasClean: true } as never));
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(opened).toHaveLength(opens);
  });

  it("reports a plan-limit refusal of the open and does not retry it", async () => {
    vi.useFakeTimers();
    const { open, onPlanLimit } = render({}, () => new AccountSessionError(REMOTE_SUSPENDED, "remote access is suspended"));
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });

    expect(onPlanLimit).toHaveBeenCalledTimes(1);
    expect(onPlanLimit.mock.calls[0][0].code).toBe(REMOTE_SUSPENDED);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("closes every socket and stops reconnecting once disabled (the plan-limit notice disables it)", async () => {
    vi.useFakeTimers();
    const { hook, opened, input } = render({ skip: null });
    await act(async () => {});
    expect(opened.map(({ machineId }) => machineId)).toEqual(["studio", "laptop"]);

    hook.rerender({ ...input, skip: null, enabled: false });
    expect(opened.every(({ socket }) => socket.closed)).toBe(true);
    act(() => opened[0].socket.onclose?.({ code: 1006, reason: "", wasClean: false } as never));
    await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
    expect(opened).toHaveLength(2);
  });

  it("opens nothing while disabled", async () => {
    const { open } = render({ enabled: false });
    await act(async () => {});
    expect(open).not.toHaveBeenCalled();
  });
});
