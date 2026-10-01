import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useDagWatchLifecycle } from "./useDagWatchLifecycle";
import * as bridge from "./tauri";
import { dagStore } from "../state/dagStore";
import { parseDagRunSnapshot } from "./dagTypes";
import fixture from "../state/__fixtures__/dagRunSample.json";

vi.mock("./tauri", () => ({
  listenDagRunUpdated: vi.fn(),
  // The hook subscribes the status channel fire-and-forget; the default no-op
  // unlisten keeps that path exercised, and registration assertions override it.
  listenDagWatchStatus: vi.fn(() => Promise.resolve(() => undefined)),
  watchDagProject: vi.fn(),
  watchDagPairedProject: vi.fn(),
  watchDagSshProject: vi.fn(),
  unwatchDagProject: vi.fn().mockResolvedValue(undefined),
}));

function deferred<T>() {
  let resolve: (value: T) => void = () => { throw new Error("not initialized"); };
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

const target = { kind: "ssh", workspaceId: "workspace", remotePath: "/remote:repo" } as const;
const key = "ssh:workspace:/remote:repo";
beforeEach(() => { vi.clearAllMocks(); dagStore.reset(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

it("waits for listener readiness before subscribing a remote-only project", async () => {
  const ready = deferred<() => void>();
  vi.mocked(bridge.listenDagRunUpdated).mockReturnValue(ready.promise);
  vi.mocked(bridge.watchDagSshProject).mockResolvedValue({ projectPath: key, runs: [] });
  const view = renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  expect(bridge.watchDagSshProject).not.toHaveBeenCalled();
  await act(async () => { ready.resolve(vi.fn()); await ready.promise; });
  expect(bridge.watchDagSshProject).toHaveBeenCalledWith("workspace", "/remote:repo");
  view.unmount();
});

it("does not let delayed initial inventory replace a live snapshot", async () => {
  const response = deferred<bridge.DagWatchProjectResult>();
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  vi.mocked(bridge.watchDagSshProject).mockReturnValue(response.promise);
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  act(() => emit({ projectPath: key, snapshot: { ...snapshot, status: "completed" } }));
  await act(async () => { response.resolve({ projectPath: key, runs: [{ ...snapshot, status: "running" }] }); await response.promise; });
  expect(dagStore.getState().runsByProject[key]?.[snapshot.runId]?.status).toBe("completed");
});

it("discards removed-project hydration and releases its late subscription", async () => {
  const response = deferred<bridge.DagWatchProjectResult>();
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.watchDagSshProject).mockReturnValue(response.promise);
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  const view = renderHook(({ active }) => useDagWatchLifecycle({
    localRoots: [], remoteTargets: active ? [target] : [], watchKey: active ? key : "",
  }), { initialProps: { active: true } });
  await act(async () => {});
  view.rerender({ active: false });
  await act(async () => { response.resolve({ projectPath: key, runs: [snapshot] }); await response.promise; });
  expect(dagStore.getState().runsByProject[key]).toBeUndefined();
  expect(bridge.unwatchDagProject).toHaveBeenCalledWith(key);
});

it("waits for old unsubscribe before readding the same paired project", async () => {
  const released = deferred<void>();
  const paired = { ...target, kind: "pairedDaemon" } as const;
  const pairedKey = "paired:workspace:/remote:repo";
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.watchDagPairedProject).mockResolvedValue({ projectPath: pairedKey, runs: [] });
  vi.mocked(bridge.unwatchDagProject).mockReturnValueOnce(released.promise);
  const view = renderHook(({ active }) => useDagWatchLifecycle({
    localRoots: [], remoteTargets: active ? [paired] : [], watchKey: active ? pairedKey : "",
  }), { initialProps: { active: true } });
  await act(async () => {});
  expect(bridge.watchDagPairedProject).toHaveBeenCalledTimes(1);
  view.rerender({ active: false });
  await act(async () => {});
  view.rerender({ active: true });
  await act(async () => {});
  expect(bridge.watchDagPairedProject).toHaveBeenCalledTimes(1);
  await act(async () => { released.resolve(); await released.promise; });
  expect(bridge.watchDagPairedProject).toHaveBeenCalledTimes(2);
});

it("releases a listener that finishes registering after unmount without starting watches", async () => {
  const ready = deferred<() => void>();
  const unlisten = vi.fn();
  const registering = deferred<void>();
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(() => {
    registering.resolve();
    return ready.promise;
  });
  const view = renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => { await registering.promise; });
  view.unmount();
  await act(async () => { ready.resolve(unlisten); await ready.promise; });
  expect(unlisten).toHaveBeenCalledTimes(1);
  expect(bridge.watchDagSshProject).not.toHaveBeenCalled();
});

it("retries a rejected subscription without requiring a project change", async () => {
  vi.useFakeTimers();
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.watchDagSshProject)
    .mockRejectedValueOnce(new Error("daemon restarting"))
    .mockResolvedValue({ projectPath: key, runs: [] });
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(2);
});

it("retries listener registration before opening subscriptions", async () => {
  vi.useFakeTimers();
  vi.mocked(bridge.listenDagRunUpdated)
    .mockRejectedValueOnce(new Error("event service restarting"))
    .mockResolvedValue(vi.fn());
  vi.mocked(bridge.watchDagSshProject).mockResolvedValue({ projectPath: key, runs: [] });
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  expect(bridge.watchDagSshProject).not.toHaveBeenCalled();
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(bridge.listenDagRunUpdated).toHaveBeenCalledTimes(2);
  expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(1);
});

it("cancels a scheduled retry when the hook unmounts", async () => {
  vi.useFakeTimers();
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.watchDagSshProject).mockRejectedValue(new Error("offline"));
  const view = renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  expect(vi.getTimerCount()).toBe(1);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  await act(async () => { await vi.advanceTimersByTimeAsync(1500); });
  expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(1);
});

it("rejects events from the previous backend generation after rewatch", async () => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  const first = { projectPath: key, generation: 10, runs: [] };
  const second = { projectPath: key, generation: 11, runs: [] };
  vi.mocked(bridge.watchDagSshProject).mockResolvedValueOnce(first).mockResolvedValue(second);
  const view = renderHook(({ revision }) => useDagWatchLifecycle({
    localRoots: [], remoteTargets: [target], watchKey: `${key}:${revision}`,
  }), { initialProps: { revision: 1 } });
  await act(async () => {});
  view.rerender({ revision: 2 });
  await act(async () => {});
  const current = { projectPath: key, generation: 11, snapshot: { ...snapshot, status: "completed" as const } };
  const stale = { projectPath: key, generation: 10, snapshot: { ...snapshot, status: "running" as const } };
  act(() => { emit(current); emit(stale); });
  expect(dagStore.getState().runsByProject[key]?.[snapshot.runId]?.status).toBe("completed");
});

it("buffers generation-tagged events until the watch response establishes ownership", async () => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  const response = deferred<bridge.DagWatchProjectResult>();
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  vi.mocked(bridge.watchDagSshProject).mockReturnValue(response.promise);
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  act(() => {
    emit({ projectPath: key, generation: 11, snapshot: { ...snapshot, status: "completed" } });
    emit({ projectPath: key, generation: 10, snapshot: { ...snapshot, status: "running" } });
  });
  expect(dagStore.getState().runsByProject[key]).toBeUndefined();
  await act(async () => {
    response.resolve({ projectPath: key, generation: 11, runs: [{ ...snapshot, status: "running" }] });
    await response.promise;
  });
  expect(dagStore.getState().runsByProject[key]?.[snapshot.runId]?.status).toBe("completed");
});

it("continues accepting discovered local roots outside the explicit watch list", async () => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [], watchKey: "" }));
  await act(async () => {});
  act(() => emit({ projectPath: "/discovered/sibling", generation: 12, snapshot }));
  expect(dagStore.getState().runsByProject["/discovered/sibling"]?.[snapshot.runId]).toEqual(snapshot);
});

it("does not let an untagged event bypass an established remote generation", async () => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  let emit: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emit = handler; return vi.fn(); });
  vi.mocked(bridge.watchDagSshProject).mockResolvedValue({ projectPath: key, generation: 15, runs: [] });
  renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
  await act(async () => {});
  act(() => emit({ projectPath: key, snapshot }));
  expect(dagStore.getState().runsByProject[key]).toBeUndefined();
});

it("re-arms remote watcher when ssh project registration succeeds for target workspace and ignores other workspaces", async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(bridge.watchDagSshProject).mockResolvedValue({ projectPath: key, generation: 1, runs: [] });
    renderHook(() => useDagWatchLifecycle({ localRoots: [], remoteTargets: [target], watchKey: key }));
    await act(async () => {});
    expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(1);

    // Event for an unrelated workspace must NOT trigger retry / rearm
    act(() => {
      window.dispatchEvent(
        new CustomEvent("ferryx:ssh-project-registered", {
          detail: { workspaceId: "ssh:unrelated-workspace" },
        }),
      );
    });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(1);

    // Event for the matching target.workspaceId ("workspace") MUST trigger retry / rearm
    act(() => {
      window.dispatchEvent(
        new CustomEvent("ferryx:ssh-project-registered", {
          detail: { workspaceId: target.workspaceId },
        }),
      );
    });
    await act(async () => {
      vi.advanceTimersByTime(2000);
    });
    expect(bridge.watchDagSshProject).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
  }
});

const pairedTarget = { kind: "pairedDaemon", workspaceId: "workspace", remotePath: "/remote:repo" } as const;
const pairedCanonicalKey = "paired:workspace:/remote:repo";

it("subscribes the watch-status channel once the run listener is ready and releases it on unmount", async () => {
  const ready = deferred<() => void>();
  const statusUnlisten = vi.fn();
  vi.mocked(bridge.listenDagRunUpdated).mockReturnValue(ready.promise);
  vi.mocked(bridge.listenDagWatchStatus).mockImplementation(async () => statusUnlisten);
  vi.mocked(bridge.watchDagPairedProject).mockResolvedValue({ projectPath: pairedCanonicalKey, runs: [] });
  const view = renderHook(() => useDagWatchLifecycle({
    localRoots: [], remoteTargets: [pairedTarget], watchKey: pairedCanonicalKey,
  }));
  await act(async () => {});
  expect(bridge.listenDagWatchStatus).not.toHaveBeenCalled();
  await act(async () => { ready.resolve(vi.fn()); await ready.promise; });
  expect(bridge.listenDagWatchStatus).toHaveBeenCalledTimes(1);
  view.unmount();
  await act(async () => {});
  expect(statusUnlisten).toHaveBeenCalledTimes(1);
});

it("records a pushed watch failure for the paired canonical key and rejects stale or untagged generations", async () => {
  const response = deferred<bridge.DagWatchProjectResult>();
  let emitStatus: Parameters<typeof bridge.listenDagWatchStatus>[0] = () => { throw new Error("status listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.listenDagWatchStatus).mockImplementation(async (handler) => { emitStatus = handler; return vi.fn(); });
  vi.mocked(bridge.watchDagPairedProject).mockReturnValue(response.promise);
  renderHook(() => useDagWatchLifecycle({
    localRoots: [], remoteTargets: [pairedTarget], watchKey: pairedCanonicalKey,
  }));
  await act(async () => {});
  // Before the watch response lands there is no generation to compare against, so the push is
  // applied as-is; the authoritative watch result below supersedes it.
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "authentication", message: "key rejected", generation: 7 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("authentication");
  await act(async () => {
    response.resolve({ projectPath: pairedCanonicalKey, generation: 7, runs: [], failure: null });
    await response.promise;
  });
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]).toBeUndefined();
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "authentication", message: "key rejected", generation: 7 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("authentication");
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "unavailable", message: "stale generation", generation: 6 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("authentication");
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "unavailable", message: "untagged" }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("authentication");
});

it("clears a recorded paired failure when a rewatch succeeds and rejects the retired generation's status", async () => {
  let emitStatus: Parameters<typeof bridge.listenDagWatchStatus>[0] = () => { throw new Error("status listener absent"); };
  const firstStatusUnlisten = vi.fn();
  let statusRegistrations = 0;
  vi.mocked(bridge.listenDagRunUpdated).mockResolvedValue(vi.fn());
  vi.mocked(bridge.listenDagWatchStatus).mockImplementation(async (handler) => {
    emitStatus = handler;
    statusRegistrations += 1;
    return statusRegistrations === 1 ? firstStatusUnlisten : vi.fn();
  });
  vi.mocked(bridge.watchDagPairedProject)
    .mockResolvedValueOnce({ projectPath: pairedCanonicalKey, generation: 7, runs: [] })
    .mockResolvedValue({ projectPath: pairedCanonicalKey, generation: 8, runs: [], failure: null });
  const view = renderHook(({ revision }) => useDagWatchLifecycle({
    localRoots: [], remoteTargets: [pairedTarget], watchKey: `${pairedCanonicalKey}:${revision}`,
  }), { initialProps: { revision: 1 } });
  await act(async () => {});
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "unavailable", message: "stream down", generation: 7 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("unavailable");
  view.rerender({ revision: 2 });
  await act(async () => {});
  expect(bridge.watchDagPairedProject).toHaveBeenCalledTimes(2);
  expect(bridge.listenDagWatchStatus).toHaveBeenCalledTimes(2);
  expect(firstStatusUnlisten).toHaveBeenCalledTimes(1);
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]).toBeUndefined();
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "capability_missing", message: "helper offline", generation: 8 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("capability_missing");
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "authentication", message: "retired generation", generation: 7 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("capability_missing");
});

it("clears a recorded paired failure on an accepted snapshot and keeps it for a stale generation", async () => {
  const snapshot = parseDagRunSnapshot(fixture);
  if (!snapshot) throw new Error("invalid fixture");
  let emitStatus: Parameters<typeof bridge.listenDagWatchStatus>[0] = () => { throw new Error("status listener absent"); };
  let emitRun: Parameters<typeof bridge.listenDagRunUpdated>[0] = () => { throw new Error("run listener absent"); };
  vi.mocked(bridge.listenDagRunUpdated).mockImplementation(async (handler) => { emitRun = handler; return vi.fn(); });
  vi.mocked(bridge.listenDagWatchStatus).mockImplementation(async (handler) => { emitStatus = handler; return vi.fn(); });
  vi.mocked(bridge.watchDagPairedProject).mockResolvedValue({ projectPath: pairedCanonicalKey, generation: 7, runs: [] });
  renderHook(() => useDagWatchLifecycle({
    localRoots: [], remoteTargets: [pairedTarget], watchKey: pairedCanonicalKey,
  }));
  await act(async () => {});
  act(() => emitStatus({ projectPath: pairedCanonicalKey, hostId: "host-1", code: "unavailable", message: "stream down", generation: 7 }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("unavailable");
  act(() => emitRun({ projectPath: pairedCanonicalKey, generation: 6, snapshot: { ...snapshot, status: "completed" } }));
  expect(dagStore.getState().runsByProject[pairedCanonicalKey]).toBeUndefined();
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]?.code).toBe("unavailable");
  act(() => emitRun({ projectPath: pairedCanonicalKey, generation: 7, snapshot: { ...snapshot, status: "completed" } }));
  expect(dagStore.getState().watchFailures[pairedCanonicalKey]).toBeUndefined();
  expect(dagStore.getState().runsByProject[pairedCanonicalKey]?.[snapshot.runId]?.status).toBe("completed");
});
