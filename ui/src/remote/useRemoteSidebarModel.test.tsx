import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { sidebarLiveState, type SidebarModel } from "../components/sidebar/sidebarModel";
import type { SupervisorHttpResponse } from "./remoteSupervisor";
import type { RemoteSessionInventory } from "./remoteSessionInventory";
import { useRemoteSidebarModel, type RemoteSidebarMachine } from "./useRemoteSidebarModel";

/** The real builder, with a switch that makes its next call throw like a builder bug would. */
const builder = vi.hoisted(() => ({ throwNext: false }));
vi.mock("../components/sidebar/sidebarModel", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/sidebar/sidebarModel")>();
  return {
    ...actual,
    buildSidebarModel: (input: Parameters<typeof actual.buildSidebarModel>[0]) => {
      if (builder.throwNext) {
        builder.throwNext = false;
        throw new TypeError("builder bug");
      }
      return actual.buildSidebarModel(input);
    },
  };
});

const json = (status: number, body: unknown): SupervisorHttpResponse => ({ status, body: new TextEncoder().encode(JSON.stringify(body)) });

const published = (revision: number, label: string): SidebarModel => ({
  schemaVersion: 1, revision, generatedAt: "2026-10-24T00:00:00.000Z", source: "desktop",
  projects: [{
    groupId: "ferryx", label, hostLabel: null, members: [{ workspaceId: "ferryx", kind: "local", machineId: null }],
    primaryWorkspaceId: "ferryx", status: null, filterLabels: ["ferryx"], worktreesEmptyHint: null,
    worktrees: [{ key: "sidebar-worktree:ferryx:/code/ferryx", workspaceId: "ferryx", path: "/code/ferryx", worktreeSlug: null,
      label: "ferryx", hostLabel: null, stale: false, listingFailure: null }],
  }],
  collapsedGroupIds: ["ferryx"], advancedFeatures: true, emptyState: null,
});

const REMOTE = "git@github.com:indo/ferryx.git";
/** An older desktop: the sidebar route is missing, the inventory routes answer. */
const inventoryRoutes: Record<string, SupervisorHttpResponse> = {
  "/api/v1/workspace/sidebar": json(404, { code: "NOT_FOUND" }),
  "/api/v1/workspace/projects": json(200, { projects: [
    { workspaceId: "ferryx", repoRoot: "/code/ferryx", gitRoot: "/code/ferryx", gitRemote: REMOTE, availability: "ready" },
    { workspaceId: "gone", repoRoot: "/code/gone", availability: "missing" },
  ] }),
  "/api/v1/workspace/worktrees?workspaceId=ferryx": json(200, { worktrees: [
    { workspaceId: "ferryx", identity: null, path: "/code/ferryx", head: "a", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null, managed: false },
    { workspaceId: "ferryx", identity: { wsId: "ferryx", slug: "relay" }, path: "/code/ferryx/.orca-worktrees/relay", head: "b",
      branch: "orca/ferryx/relay", bare: false, detached: false, locked: null, prunable: null, managed: true },
  ] }),
  "/api/v1/workspace/state": json(200, { projects: [
    { workspaceId: "ssh:box", hostLabel: "Build box", worktrees: [{ worktreeSlug: null, worktreeLabel: "infra" }] },
  ] }),
};

function fakeMachine(routes: () => Record<string, SupervisorHttpResponse>, over: Partial<RemoteSidebarMachine> = {}) {
  const log: string[] = [];
  const handlers = new Set<(frame: unknown) => void>();
  const fetchLike = vi.fn((path: string) => {
    log.push(`fetch ${path}`);
    return Promise.resolve(routes()[path] ?? json(500, {}));
  });
  const machine: RemoteSidebarMachine = {
    machineId: "m1", label: "Studio", fetchLike, inventory: null, selection: null,
    subscribe: (handler) => {
      log.push("subscribe");
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    ...over,
  };
  const emit = (frame: unknown) => act(() => handlers.forEach((handler) => handler(frame)));
  const sidebarFetches = () => fetchLike.mock.calls.filter(([path]) => path === "/api/v1/workspace/sidebar").length;
  return { machine, fetchLike, log, emit, handlers, sidebarFetches };
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  builder.throwNext = false;
});

describe("useRemoteSidebarModel", () => {
  it("shows the model the desktop published, subscribing before the first fetch", async () => {
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(3, "ferryx") }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    expect(result.current[0]?.model).toEqual(published(3, "ferryx"));
    expect(m.log.slice(0, 2)).toEqual(["subscribe", "fetch /api/v1/workspace/sidebar"]);
    expect(m.fetchLike).toHaveBeenCalledTimes(1);
  });

  it("builds the model from the inventory when the desktop has published none", async () => {
    const m = fakeMachine(() => ({ ...inventoryRoutes, "/api/v1/workspace/sidebar": json(200, { model: null }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    const model = result.current[0]?.model;
    expect(model?.source).toBe("inventory");
    expect(model?.projects.map((p) => [p.groupId, p.label, p.worktrees.map((w) => w.label)])).toEqual([
      ["ferryx", "ferryx", ["main", "relay"]],
      ["gone", "gone", ["gone"]],
      ["ssh:box", "infra (Build box)", ["infra"]],
    ]);
    expect(model?.projects[1]?.status?.kind).toBe("failure");
  });

  it("falls back to the inventory when the desktop predates the sidebar route", async () => {
    const m = fakeMachine(() => inventoryRoutes);
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    expect(result.current[0]?.model?.source).toBe("inventory");
    expect(result.current[0]?.model?.projects[0]?.worktrees.map((w) => w.worktreeSlug)).toEqual([null, "relay"]);
  });

  it("falls back to the inventory when a published model has malformed nested nodes", async () => {
    const broken = published(5, "ferryx");
    const malformed = { ...broken, projects: [{ ...broken.projects[0], members: [{ workspaceId: 7, kind: "cloud" }],
      worktrees: [{ key: "k", label: null }] }] };
    const m = fakeMachine(() => ({ ...inventoryRoutes, "/api/v1/workspace/sidebar": json(200, { model: malformed }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    expect(result.current[0]?.model?.source).toBe("inventory");
    expect(result.current[0]?.model?.projects[0]?.worktrees.map((w) => w.label)).toEqual(["main", "relay"]);
  });

  it("falls back to the inventory when a published model carries malformed session signals", async () => {
    const malformed = { ...published(6, "ferryx"), signals: { unreadWorktreePaths: { "/code/ferryx": "yes" }, agentStateByPath: {} } };
    const m = fakeMachine(() => ({ ...inventoryRoutes, "/api/v1/workspace/sidebar": json(200, { model: malformed }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    expect(result.current[0]?.model?.source).toBe("inventory");
  });

  it("settles as error when the fallback builder throws, and recovers on the next sidebarChanged", async () => {
    builder.throwNext = true;
    const m = fakeMachine(() => inventoryRoutes);
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));
    await waitFor(() => expect(result.current[0]?.state).toBe("error"));
    expect(result.current[0]?.error?.code).toBe("LOAD_FAILED");

    m.emit({ type: "sidebarChanged", payload: { revision: 2 } });

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));
    expect(result.current[0]?.model?.source).toBe("inventory");
    expect(m.sidebarFetches()).toBe(2);
  });

  it("refetches once per sidebarChanged frame", async () => {
    let revision = 1;
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(revision, `rev ${revision}`) }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));
    await waitFor(() => expect(result.current[0]?.model?.revision).toBe(1));
    revision = 2;

    m.emit(JSON.stringify({ type: "sidebarChanged", payload: { revision: 2 } }));

    await waitFor(() => expect(result.current[0]?.model?.projects[0]?.label).toBe("rev 2"));
    expect(m.sidebarFetches()).toBe(2);
  });

  it("refetches when the connection is replaced after a reconnect", async () => {
    let revision = 1;
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(revision, `rev ${revision}`) }) }));
    const { result, rerender } = renderHook(
      ({ connectionId }) => useRemoteSidebarModel({ machines: [{ ...m.machine, connectionId }] }),
      { initialProps: { connectionId: "1" } },
    );
    await waitFor(() => expect(result.current[0]?.model?.revision).toBe(1));
    revision = 4;

    rerender({ connectionId: "2" });

    await waitFor(() => expect(result.current[0]?.model?.revision).toBe(4));
    expect(m.sidebarFetches()).toBe(2);
    expect(m.handlers.size).toBe(1);
  });

  it("never polls: a minute without events costs no extra fetch", async () => {
    vi.useFakeTimers();
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(1, "ferryx") }) }));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current[0]?.state).toBe("ok");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });

    expect(m.fetchLike).toHaveBeenCalledTimes(1);
  });

  it("keeps the last model and reports offline when a refetch cannot reach the machine", async () => {
    let reachable = true;
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(1, "ferryx") }) }));
    m.fetchLike.mockImplementation((path: string) =>
      reachable ? Promise.resolve(json(200, { model: published(1, path) })) : Promise.reject(new Error("tunnel closed")));
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));
    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));
    reachable = false;

    m.emit({ type: "sidebarChanged", payload: { revision: 2 } });

    await waitFor(() => expect(result.current[0]?.state).toBe("offline"));
    expect(result.current[0]?.model?.revision).toBe(1);
  });

  it("derives live row state from the machine's session inventory", async () => {
    const inventory: RemoteSessionInventory = {
      machineId: "m1", revision: "1", completeness: "complete", unavailableWorkspaceIds: [], fetchedAt: 0,
      entries: [{ machineId: "m1", workspaceId: "ferryx", worktreeSlug: null, worktreeLabel: null, sessionId: "s1", daemonEpoch: null,
        title: "claude", running: true, agentType: "claude", providerSessionId: null, activityState: "waiting" }],
    };
    const m = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(1, "ferryx") }) }),
      { inventory, selection: { workspaceId: "ferryx", worktreeSlug: null } });
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [m.machine] }));

    await waitFor(() => expect(result.current[0]?.state).toBe("ok"));

    const section = result.current[0];
    const live = section?.model && section.signals ? sidebarLiveState(section.model, section.signals, "") : null;
    expect(live?.rows["sidebar-worktree:ferryx:/code/ferryx"]).toEqual({ active: true, slot: "waiting", supervisorTyping: false });
  });

  it("keeps one section per account machine, labelled with the machine name", async () => {
    const a = fakeMachine(() => ({ "/api/v1/workspace/sidebar": json(200, { model: published(1, "alpha") }) }));
    const b = fakeMachine(() => inventoryRoutes, { machineId: "m2", label: "Laptop" });
    const { result } = renderHook(() => useRemoteSidebarModel({ machines: [a.machine, b.machine] }));

    await waitFor(() => expect(result.current.every((section) => section.state === "ok")).toBe(true));

    expect(result.current.map((s) => [s.machineId, s.label, s.model?.source])).toEqual([
      ["m1", "Studio", "desktop"],
      ["m2", "Laptop", "inventory"],
    ]);
  });
});
