// useMachineEventSockets: every reachable machine the active connection does not cover gets its
// own events socket, and its frames reach the supervisor frame bus.
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AccountMachineView } from "../accountSession";
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

function fakeOpener() {
  const opened: Array<{ machineId: string; deviceToken: string; socket: FakeSocket }> = [];
  const open = vi.fn(async (target: AccountMachineView, deviceToken: string) => {
    const socket: FakeSocket = { closed: false, onmessage: null, onclose: null, onerror: null, close: () => { socket.closed = true; } };
    opened.push({ machineId: target.machineId, deviceToken, socket });
    return socket;
  });
  return { open, opened };
}

const link = (deviceToken: string): MachineLink => ({ transport: { fetchLike: vi.fn() }, deviceToken });

function render(over: Partial<MachineEventSocketsInput> = {}) {
  const bus = createSupervisorFrameBus();
  const { open, opened } = fakeOpener();
  const links: Record<string, MachineLink> = { studio: link("tok-studio"), laptop: link("tok-laptop"), gone: link("tok-gone") };
  const input: MachineEventSocketsInput = {
    enabled: true,
    machines: [machine("studio"), machine("laptop"), machine("gone", false)],
    linkFor: (id) => links[id] ?? null,
    skip: "laptop",
    open,
    bus,
    ...over,
  };
  const hook = renderHook((props: MachineEventSocketsInput) => useMachineEventSockets(props), { initialProps: input });
  return { hook, bus, open, opened, input };
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

  it("opens nothing while disabled", async () => {
    const { open } = render({ enabled: false });
    await act(async () => {});
    expect(open).not.toHaveBeenCalled();
  });
});
