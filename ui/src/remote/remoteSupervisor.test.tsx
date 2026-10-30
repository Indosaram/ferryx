import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SUPERVISOR_ACTING_MS } from "../lib/supervisorModel";
import {
  fetchSupervisorConversation,
  fetchSupervisorMemory,
  fetchSupervisorSnapshot,
  postSupervisorMemory,
  postSupervisorRequest,
  hasSupervisorCapability,
  isSupervisorChangedFrame,
  setRemoteLanePaused,
  useRemoteSupervisor,
  type RemoteSupervisorMachine,
  type SupervisorFetch,
  type SupervisorHttpResponse,
} from "./remoteSupervisor";

const lane = { key: "ledger", title: "Ledger", status: "working", sessionId: "s1", cwd: "/r", goal: null, paused: false, lastActivityAt: "", lastReason: null };
const snapshotBody = (over: Record<string, unknown> = {}) => ({
  hub: { available: true, running: true, error: null }, lanes: [lane], watched: [], actions: [], ...over,
});
const json = (status: number, body: unknown): SupervisorHttpResponse => ({ status, body: new TextEncoder().encode(JSON.stringify(body)) });

/** A fake events socket plus a recorded transport; `log` keeps subscribe/fetch order. */
function fakeMachine(responses: Array<() => SupervisorHttpResponse>, over: Partial<RemoteSupervisorMachine> = {}) {
  const log: string[] = [];
  const handlers = new Set<(frame: unknown) => void>();
  let call = 0;
  const fetchLike = vi.fn<SupervisorFetch>((path) => {
    log.push(`fetch ${path}`);
    const next = responses[Math.min(call++, responses.length - 1)];
    return Promise.resolve(next ? next() : json(500, {}));
  });
  const machine: RemoteSupervisorMachine = {
    machineId: "m1",
    label: "Studio",
    fetchLike,
    capabilities: { capabilities: ["supervisorV1"] },
    subscribe: (handler) => {
      log.push("subscribe");
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    ...over,
  };
  const emit = (frame: unknown) => act(() => handlers.forEach((h) => h(frame)));
  return { machine, fetchLike, log, emit, handlers };
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("remote supervisor client", () => {
  it("reads supervisorV1 from the capabilities document or list", () => {
    expect(hasSupervisorCapability({ capabilities: ["terminalCreateV1", "supervisorV1"] })).toBe(true);
    expect(hasSupervisorCapability(["supervisorV1"])).toBe(true);
    expect(hasSupervisorCapability({ capabilities: ["terminalCreateV1"] })).toBe(false);
    expect(hasSupervisorCapability(null)).toBe(false);
  });

  it("recognizes supervisorChanged frames as text or objects", () => {
    expect(isSupervisorChangedFrame('{"type":"supervisorChanged","payload":{"laneKey":"k"}}')).toBe(true);
    expect(isSupervisorChangedFrame({ type: "supervisorChanged", payload: { sessionId: "s" } })).toBe(true);
    expect(isSupervisorChangedFrame({ type: "inventoryInvalidated" })).toBe(false);
    expect(isSupervisorChangedFrame("not json")).toBe(false);
  });

  it("reports an older daemon's 404 route as unsupported", async () => {
    await expect(fetchSupervisorSnapshot(() => Promise.resolve(json(404, { code: "NOT_FOUND", message: "x" })))).resolves.toEqual({ kind: "unsupported" });
  });

  it("parses a snapshot including lastReason", async () => {
    const result = await fetchSupervisorSnapshot(() => Promise.resolve(json(200, snapshotBody({ lanes: [{ ...lane, lastReason: "stuck" }] }))));
    expect(result.kind === "ok" ? result.snapshot.lanes[0]?.lastReason : "wrong").toBe("stuck");
  });

  it.each([
    [403, "FORBIDDEN"],
    [503, "SUPERVISOR_UNAVAILABLE"],
    [404, "NOT_FOUND"],
    [409, "LANE_ENDED"],
    [400, "INVALID_REQUEST"],
  ])("maps a %i pause failure to the server code", async (status, code) => {
    const result = await setRemoteLanePaused(() => Promise.resolve(json(status, { code, message: "m" })), "ledger", true);
    expect(result).toMatchObject({ kind: "error", code, status });
  });

  it("falls back to a status code when the error has no envelope", async () => {
    const result = await setRemoteLanePaused(() => Promise.resolve({ status: 503, body: "gateway" }), "ledger", true);
    expect(result).toMatchObject({ kind: "error", code: "SUPERVISOR_UNAVAILABLE", status: 503 });
  });

  it("reports a thrown transport as OFFLINE with no status", async () => {
    const result = await setRemoteLanePaused(() => Promise.reject(new Error("tunnel closed")), "ledger", true);
    expect(result).toMatchObject({ kind: "error", code: "OFFLINE", status: null });
  });

  it("posts the pause body and returns the server's answer", async () => {
    const fetchLike = vi.fn<SupervisorFetch>(() => Promise.resolve(json(200, { laneKey: "ledger", paused: true, changed: true })));
    await expect(setRemoteLanePaused(fetchLike, "ledger", true)).resolves.toEqual({ kind: "ok", laneKey: "ledger", paused: true, changed: true });
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/supervisor/pause", expect.objectContaining({ method: "POST", body: JSON.stringify({ laneKey: "ledger", paused: true }) }));
  });
});

describe("useRemoteSupervisor", () => {
  it("subscribes to the events socket before the first fetch", async () => {
    const { machine, log } = fakeMachine([() => json(200, snapshotBody())]);
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("ok"));
    expect(log).toEqual(["subscribe", "fetch /api/v1/supervisor"]);
    expect(result.current.lanes).toEqual([{ ...lane, machineId: "m1" }]);
  });

  it("refetches exactly once per supervisorChanged event", async () => {
    const { machine, fetchLike, emit } = fakeMachine([
      () => json(200, snapshotBody()),
      () => json(200, snapshotBody({ lanes: [{ ...lane, paused: true }] })),
    ]);
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("ok"));
    emit({ type: "supervisorChanged", payload: { laneKey: "ledger" } });
    await waitFor(() => expect(result.current.lanes[0]?.paused).toBe(true));
    expect(fetchLike).toHaveBeenCalledTimes(2);
    emit({ type: "sessionOutput" });
    expect(fetchLike).toHaveBeenCalledTimes(2);
  });

  it("refetches when the events socket resubscribes", async () => {
    const { machine, fetchLike, emit } = fakeMachine([() => json(200, snapshotBody())]);
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("ok"));
    emit({ type: "inventoryInvalidated", reason: "subscribe", payload: {} });
    await waitFor(() => expect(fetchLike).toHaveBeenCalledTimes(2));
  });

  it("marks a machine without the capability unsupported and never fetches", async () => {
    const { machine, fetchLike, handlers } = fakeMachine([() => json(200, snapshotBody())], { capabilities: { capabilities: [] } });
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("unsupported"));
    expect(fetchLike).not.toHaveBeenCalled();
    expect(handlers.size).toBe(0);
  });

  it("marks a 404 route unsupported when capabilities are unknown", async () => {
    const { machine } = fakeMachine([() => json(404, { code: "NOT_FOUND", message: "no route" })], { capabilities: null });
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("unsupported"));
  });

  it("marks a throwing transport offline", async () => {
    const { machine } = fakeMachine([() => { throw new Error("tunnel gone"); }]);
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("offline"));
  });

  it("unsubscribes on unmount", async () => {
    const { machine, handlers } = fakeMachine([() => json(200, snapshotBody())]);
    const { result, unmount } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await waitFor(() => expect(result.current.machines[0]?.state).toBe("ok"));
    unmount();
    expect(handlers.size).toBe(0);
  });

  it("re-renders once when the acting glow expires, without refetching", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const start = Date.now();
    const { machine, fetchLike } = fakeMachine([
      () => json(200, snapshotBody({ actions: [{ deviceId: "d", op: "send", sessionId: "s1", requestId: null, code: "OK", atMs: start, text: "go" }] })),
    ]);
    const { result } = renderHook(() => useRemoteSupervisor({ machines: [machine] }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.machines[0]?.state).toBe("ok");
    const before = result.current.now;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(SUPERVISOR_ACTING_MS + 20);
    });
    expect(result.current.now).toBeGreaterThan(before + SUPERVISOR_ACTING_MS - 1);
    expect(fetchLike).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe("unified supervisor remote calls", () => {
  it("posts an envelope to the request route and returns the receipt", async () => {
    const fetchLike = vi.fn<SupervisorFetch>(() => Promise.resolve(json(200, { goalId: "g", revision: 4, runState: "paused" })));
    const envelope = { requestId: "phone-pause-1", goalId: "g", expectedRevision: 3, action: "pause", payload: {} };
    await expect(postSupervisorRequest(fetchLike, envelope)).resolves.toEqual({ kind: "ok", value: { goalId: "g", revision: 4, runState: "paused" } });
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/supervisor/request", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(envelope) });
  });

  it("reads the machine error envelope's code for a stale revision", async () => {
    const result = await postSupervisorRequest(
      () => Promise.resolve(json(409, { error: { code: "REVISION_CONFLICT", message: "goal g revision changed", retryable: false, requestId: "", details: {} } })),
      { requestId: "r", goalId: "g", expectedRevision: 1, action: "resume", payload: {} },
    );
    expect(result).toEqual({ kind: "error", code: "REVISION_CONFLICT", message: "goal g revision changed", status: 409 });
  });

  it("reads conversation and memory through their routes", async () => {
    const fetchLike = vi.fn<SupervisorFetch>((path) =>
      Promise.resolve(path.startsWith("/api/v1/supervisor/memory")
        ? json(503, { error: { code: "MEMORY_RECEIPTS_UNAVAILABLE", message: "not yet" } })
        : json(200, { projectId: "p 1", messages: [], next: 0 })),
    );
    await expect(fetchSupervisorConversation(fetchLike, "p 1", 5)).resolves.toMatchObject({ kind: "ok" });
    expect(fetchLike).toHaveBeenCalledWith("/api/v1/supervisor/conversation?projectId=p+1&after=5");
    await expect(fetchSupervisorMemory(fetchLike)).resolves.toMatchObject({ kind: "error", code: "MEMORY_RECEIPTS_UNAVAILABLE", status: 503 });
    const teach = { requestId: "t", action: "teach", payload: { text: "x" } };
    await postSupervisorMemory(fetchLike, teach);
    expect(fetchLike).toHaveBeenLastCalledWith("/api/v1/supervisor/memory", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(teach) });
  });

  it("reports a thrown transport as OFFLINE", async () => {
    await expect(postSupervisorRequest(() => Promise.reject(new Error("tunnel closed")), { requestId: "r", action: "message", payload: { text: "hi" } }))
      .resolves.toMatchObject({ kind: "error", code: "OFFLINE", status: null });
  });

  it("carries the unified supervisor part of the snapshot", async () => {
    const result = await fetchSupervisorSnapshot(() => Promise.resolve(json(200, snapshotBody({
      supervisor: { identity: { supervisorId: "s", ownerPrincipal: "o" }, projects: [], goals: [], health: null },
    }))));
    expect(result.kind === "ok" ? result.snapshot.supervisor?.identity?.ownerPrincipal : "wrong").toBe("o");
  });
});
