// usePhoneSupervisor routing: lanes are driven by the account supervisor, so pausing a lane on
// a standby machine goes to the holder's tunnel, and a holder's lane pause goes to itself.
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi, type Mock } from "vitest";

import type { AccountMachineView } from "../accountSession";
import type { TunnelResponse, TunnelTransport } from "../attachTunnel";
import type { LanePauseResult } from "../remoteSupervisor";
import { createSupervisorFrameBus } from "./machineSupervisorSources";
import { usePhoneSupervisor, type PhoneSupervisorInput } from "./usePhoneSupervisor";

const PAUSE_PATH = "/api/v1/supervisor/pause";

const machine = (machineId: string): AccountMachineView => ({
  machineRecordId: `rec-${machineId}`, machineId, displayName: machineId, publicKey: "", attachPublicKey: "", relayOrigin: "",
  platform: "linux", online: true, enrollmentEpoch: "1", lastSeenAt: 0,
});

const json = (status: number, body: unknown): TunnelResponse => ({ status, headers: {}, body: new TextEncoder().encode(JSON.stringify(body)) });

const lane = (key: string) => ({ key, title: key, status: "working", sessionId: `s-${key}`, cwd: "/r", goal: null, paused: false, lastActivityAt: "", lastReason: null });

const holderSnapshot = {
  hub: { available: true, running: true, error: null, role: "holder", holderMachineId: "studio", holderLabel: "Studio" },
  lanes: [lane("studio-lane")],
  watched: [],
  actions: [],
  supervisor: { identity: { supervisorId: "s", ownerPrincipal: "o" }, projects: [], goals: [], health: null },
};

const standbySnapshot = {
  hub: { available: true, running: false, error: null, role: "standby", holderMachineId: "studio", holderLabel: "Studio" },
  lanes: [lane("laptop-lane")],
  watched: [],
  actions: [],
};

type FakeTransport = TunnelTransport & { fetchLike: Mock<TunnelTransport["fetchLike"]> };

/** One machine's tunnel: answers the snapshot GET and the pause POST, recording every call. */
function fakeTransport(snapshot: unknown): FakeTransport {
  const fetchLike = vi.fn<TunnelTransport["fetchLike"]>(async (path, init) => {
    if (path === PAUSE_PATH && init?.method === "POST") {
      const body = JSON.parse(typeof init.body === "string" ? init.body : "{}") as { laneKey: string; paused: boolean };
      return json(200, { laneKey: body.laneKey, paused: body.paused, changed: true });
    }
    return json(200, snapshot);
  });
  return { fetchLike, openWebSocket: vi.fn(), close: vi.fn() };
}

function pauseCalls(transport: FakeTransport) {
  return transport.fetchLike.mock.calls.filter(([path]) => path === PAUSE_PATH);
}

function renderPhoneSupervisor() {
  const studio = fakeTransport(holderSnapshot);
  const laptop = fakeTransport(standbySnapshot);
  const transports: Record<string, TunnelTransport> = { studio, laptop };
  const input: PhoneSupervisorInput = {
    enabled: true,
    discovery: {
      machines: [machine("studio"), machine("laptop")],
      machineStatuses: {},
      peekConnection: (machineId) => {
        const transport = transports[machineId];
        return transport ? { transport, deviceToken: `tok-${machineId}` } : null;
      },
    },
    active: null,
    bus: createSupervisorFrameBus(),
  };
  const hook = renderHook(() => usePhoneSupervisor(input));
  return { hook, studio, laptop };
}

async function settled(hook: ReturnType<typeof renderPhoneSupervisor>["hook"]) {
  await waitFor(() => {
    expect(hook.result.current.machines.map((item) => item.snapshot?.hub.role ?? null)).toEqual(["holder", "standby"]);
    expect(hook.result.current.host?.machineId).toBe("studio");
  });
}

afterEach(cleanup);

describe("usePhoneSupervisor lane pause routing", () => {
  it("sends a standby machine's lane pause through the holder's tunnel", async () => {
    const { hook, studio, laptop } = renderPhoneSupervisor();
    await settled(hook);
    let result: LanePauseResult | undefined;
    await act(async () => {
      result = await hook.result.current.setPaused("laptop", "laptop-lane", true);
    });
    expect(result).toEqual({ kind: "ok", laneKey: "laptop-lane", paused: true, changed: true });
    expect(pauseCalls(laptop)).toEqual([]);
    expect(pauseCalls(studio)).toEqual([
      [PAUSE_PATH, expect.objectContaining({ method: "POST", body: JSON.stringify({ laneKey: "laptop-lane", paused: true }) })],
    ]);
    expect(studio.fetchLike).toHaveBeenCalledWith(PAUSE_PATH, expect.objectContaining({ headers: expect.objectContaining({ Authorization: "Bearer tok-studio" }) }));
  });

  it("sends the holder's own lane pause to the holder itself", async () => {
    const { hook, studio, laptop } = renderPhoneSupervisor();
    await settled(hook);
    await act(async () => {
      await hook.result.current.setPaused("studio", "studio-lane", false);
    });
    expect(pauseCalls(laptop)).toEqual([]);
    expect(pauseCalls(studio)).toEqual([
      [PAUSE_PATH, expect.objectContaining({ method: "POST", body: JSON.stringify({ laneKey: "studio-lane", paused: false }) })],
    ]);
  });
});
