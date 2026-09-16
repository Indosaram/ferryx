import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useInactiveProjectWorktrees } from "./inactiveProjectWorktrees";
import { remoteHostStore } from "./remoteHostStore";
import { projectRootWorktree } from "../lib/projectIdentity";
import type { RegisteredProject, WorktreeChangedPayload } from "../lib/types";

const adapter = vi.hoisted(() => ({ capabilities: vi.fn(), projects: vi.fn(), worktrees: vi.fn() }));
vi.mock("../lib/pairedDaemonProject", () => ({ createPairedDaemonProjectAdapter: vi.fn(() => adapter) }));
afterEach(() => { cleanup(); remoteHostStore.reset(); vi.clearAllMocks(); vi.useRealTimers(); });
const project: RegisteredProject = { workspaceId: `daemon:${"a".repeat(64)}`, remoteWorkspaceId: "repo", repoRoot: "/srv/repo", gitRoot: "/srv/repo", target: { kind: "pairedDaemon", hostId: "a" } };

it("loads paired owners without local registration and preserves rows on partial or failed host refresh", async () => {
  remoteHostStore.setState(s => ({ ...s, nativeStatus: "ready", machineFeaturesEnabled: true, hosts: { a: { hostId: "a", machineId: "machine-a", generation: "1", name: "Alpha", address: "", transport: "relay", authStatus: "paired", grantScope: "machine", online: true } } }));
  adapter.capabilities.mockResolvedValue({});
  adapter.projects.mockResolvedValue({ completeness: "complete", unavailableWorkspaceIds: [], projects: [{ ...project, availability: "ready" }] });
  const row = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature" };
  adapter.worktrees.mockResolvedValue({ worktrees: [row] });
  let changed!: (payload: WorktreeChangedPayload) => void;
  const services = { registerProject: vi.fn(async () => project), listWorktrees: vi.fn(async () => []), onWorktreeChanged: vi.fn(async (handler: typeof changed) => { changed = handler; return () => {}; }) };
  const hook = renderHook(() => useInactiveProjectWorktrees([project], "local", [], services));
  await act(async () => { await services.onWorktreeChanged.mock.results[0].value; });
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).not.toHaveBeenCalled();
  expect(adapter.worktrees).toHaveBeenCalledWith("repo");
  expect(hook.result.current[project.workspaceId]).toEqual([{ ...row, workspaceId: project.workspaceId }]);
  const retained = hook.result.current[project.workspaceId];
  adapter.projects.mockResolvedValue({ completeness: "partial", unavailableWorkspaceIds: [project.workspaceId], projects: [] });
  await act(async () => { changed({ workspaceId: project.workspaceId, kind: "created", worktree: { wsId: "repo", slug: "feature" } }); });
  expect(hook.result.current[project.workspaceId]?.[0]?.path).toBe("/srv/repo/feature");
  expect((hook.result.current[project.workspaceId]?.[0] as any)?.stale).toBe(true);
  adapter.projects.mockRejectedValue(new Error("OFFLINE"));
  await act(async () => { changed({ workspaceId: project.workspaceId, kind: "pruned", worktree: { wsId: "repo", slug: "feature" } }); });
  expect(hook.result.current[project.workspaceId]?.[0]?.path).toBe("/srv/repo/feature");
  expect((hook.result.current[project.workspaceId]?.[0] as any)?.stale).toBe(true);
  expect(services.listWorktrees).not.toHaveBeenCalled();
});

it("retains cached rows with stale/offline freshness metadata on disconnect, invalidates on TTL, and clears staleness on authoritative reconnect at generation N+1", async () => {
  vi.useFakeTimers();
  remoteHostStore.setState(s => ({
    ...s,
    nativeStatus: "ready",
    machineFeaturesEnabled: true,
    hosts: {
      a: {
        hostId: "a",
        machineId: "machine-a",
        generation: "1",
        name: "Alpha",
        address: "",
        transport: "relay",
        authStatus: "paired",
        grantScope: "machine",
        online: true,
      },
    },
  }));
  adapter.capabilities.mockResolvedValue({});
  adapter.projects.mockResolvedValue({
    completeness: "complete",
    unavailableWorkspaceIds: [],
    projects: [{ ...project, availability: "ready" }],
  });
  const rowGen1 = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature-gen1" };
  adapter.worktrees.mockResolvedValue({ worktrees: [rowGen1] });

  const services = {
    registerProject: vi.fn(async () => project),
    listWorktrees: vi.fn(async () => []),
    pairedWorktreeTtlMs: 30_000,
  };

  const hook = renderHook(() => useInactiveProjectWorktrees([project], "local", [], services));
  await act(async () => {});

  // Generation N rows cached and actionable
  expect(hook.result.current[project.workspaceId]).toEqual([{ ...rowGen1, workspaceId: project.workspaceId }]);
  expect((hook.result.current[project.workspaceId]?.[0] as any)?.stale).toBeFalsy();
  expect((hook.result.current[project.workspaceId]?.[0] as any)?.freshness).toBeUndefined();

  // 2. Host goes offline (loader returns null)
  await act(async () => {
    remoteHostStore.setState(s => ({
      ...s,
      hosts: {
        a: {
          ...s.hosts.a,
          online: false,
        },
      },
    }));
  });

  // Cached rows must be preserved AND marked stale/offline with freshness metadata, actionable rows disabled
  const offlineRows = hook.result.current[project.workspaceId];
  expect(offlineRows).toHaveLength(1);
  expect(offlineRows[0].path).toBe("/srv/repo/feature-gen1");
  expect((offlineRows[0] as any).stale).toBe(true);
  expect((offlineRows[0] as any).offline).toBe(true);
  expect((offlineRows[0] as any).disabled).toBe(true);
  expect((offlineRows[0] as any).freshness).toEqual(expect.objectContaining({
    stale: true,
    offline: true,
    generation: "1",
  }));

  // 3. TTL invalidation: while remaining offline, virtual time passes TTL
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  // After TTL expires, stale rows are bounded and invalidated
  expect(hook.result.current[project.workspaceId]).toEqual([]);

  // 4. Reconnect with authoritative data at generation N+1 (generation: "2", online: true)
  const rowGen2 = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature-gen2" };
  adapter.worktrees.mockResolvedValue({ worktrees: [rowGen2] });
  adapter.projects.mockResolvedValue({
    completeness: "complete",
    unavailableWorkspaceIds: [],
    projects: [{ ...project, availability: "ready" }],
  });
  await act(async () => {
    remoteHostStore.setState(s => ({
      ...s,
      hosts: {
        a: {
          ...s.hosts.a,
          generation: "2",
          online: true,
        },
      },
    }));
  });

  // Rows replaced with authoritative generation 2 data, freshness metadata cleared
  const reconnectedRows = hook.result.current[project.workspaceId];
  expect(reconnectedRows).toHaveLength(1);
  expect(reconnectedRows[0].path).toBe("/srv/repo/feature-gen2");
  expect((reconnectedRows[0] as any).stale).toBeFalsy();
  expect((reconnectedRows[0] as any).offline).toBeFalsy();
  expect((reconnectedRows[0] as any).disabled).toBeFalsy();
  expect((reconnectedRows[0] as any).freshness).toBeUndefined();
});

it("replaces stale rows immediately on reconnect at generation N+1 before TTL expires", async () => {
  remoteHostStore.setState(s => ({
    ...s,
    nativeStatus: "ready",
    machineFeaturesEnabled: true,
    hosts: {
      a: {
        hostId: "a",
        machineId: "machine-a",
        generation: "1",
        name: "Alpha",
        address: "",
        transport: "relay",
        authStatus: "paired",
        grantScope: "machine",
        online: true,
      },
    },
  }));
  adapter.capabilities.mockResolvedValue({});
  adapter.projects.mockResolvedValue({
    completeness: "complete",
    unavailableWorkspaceIds: [],
    projects: [{ ...project, availability: "ready" }],
  });
  const rowGen1 = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature-gen1" };
  adapter.worktrees.mockResolvedValue({ worktrees: [rowGen1] });

  const services = {
    registerProject: vi.fn(async () => project),
    listWorktrees: vi.fn(async () => []),
  };

  const hook = renderHook(() => useInactiveProjectWorktrees([project], "local", [], services));
  await act(async () => {});

  expect(hook.result.current[project.workspaceId]).toEqual([{ ...rowGen1, workspaceId: project.workspaceId }]);

  // Host goes offline
  await act(async () => {
    remoteHostStore.setState(s => ({
      ...s,
      hosts: {
        a: {
          ...s.hosts.a,
          online: false,
        },
      },
    }));
  });

  expect((hook.result.current[project.workspaceId]?.[0] as any)?.stale).toBe(true);

  // Reconnect before TTL
  const rowGen2 = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature-gen2" };
  adapter.worktrees.mockResolvedValue({ worktrees: [rowGen2] });
  adapter.projects.mockResolvedValue({
    completeness: "complete",
    unavailableWorkspaceIds: [],
    projects: [{ ...project, availability: "ready" }],
  });
  await act(async () => {
    remoteHostStore.setState(s => ({
      ...s,
      hosts: {
        a: {
          ...s.hosts.a,
          generation: "2",
          online: true,
        },
      },
    }));
  });

  const reconnectedRows = hook.result.current[project.workspaceId];
  expect(reconnectedRows).toHaveLength(1);
  expect(reconnectedRows[0].path).toBe("/srv/repo/feature-gen2");
  expect((reconnectedRows[0] as any).stale).toBeFalsy();
  expect((reconnectedRows[0] as any).freshness).toBeUndefined();
});

it("transitions cached rows to stale on event-driven relist failure (null/error) and evicts on TTL", async () => {
  vi.useFakeTimers();
  remoteHostStore.setState(s => ({
    ...s,
    nativeStatus: "ready",
    machineFeaturesEnabled: true,
    hosts: {
      a: {
        hostId: "a",
        machineId: "machine-a",
        generation: "1",
        name: "Alpha",
        address: "",
        transport: "relay",
        authStatus: "paired",
        grantScope: "machine",
        online: true,
      },
    },
  }));
  adapter.capabilities.mockResolvedValue({});
  adapter.projects.mockResolvedValue({ completeness: "complete", unavailableWorkspaceIds: [], projects: [{ ...project, availability: "ready" }] });
  const row = { ...projectRootWorktree(project), workspaceId: "repo", path: "/srv/repo/feature" };
  adapter.worktrees.mockResolvedValue({ worktrees: [row] });

  let changed!: (payload: WorktreeChangedPayload) => void;
  const services = {
    registerProject: vi.fn(async () => project),
    listWorktrees: vi.fn(async () => []),
    onWorktreeChanged: vi.fn(async (handler: typeof changed) => { changed = handler; return () => {}; }),
    pairedWorktreeTtlMs: 30_000,
  };

  const hook = renderHook(() => useInactiveProjectWorktrees([project], "local", [], services));
  await act(async () => { await services.onWorktreeChanged.mock.results[0].value; });

  expect(hook.result.current[project.workspaceId]).toEqual([{ ...row, workspaceId: project.workspaceId }]);

  // Now an event arrives, but adapter fails (returns null because incomplete/unavailable)
  adapter.projects.mockResolvedValue({ completeness: "partial", unavailableWorkspaceIds: [project.workspaceId], projects: [] });

  await act(async () => {
    changed({ workspaceId: project.workspaceId, kind: "created", worktree: { wsId: "repo", slug: "feature" } });
  });

  // Cached rows MUST transition to stale/disabled with freshness metadata
  const staleRows = hook.result.current[project.workspaceId];
  expect(staleRows).toHaveLength(1);
  expect((staleRows[0] as any).stale).toBe(true);
  expect((staleRows[0] as any).disabled).toBe(true);
  expect((staleRows[0] as any).freshness?.stale).toBe(true);

  // TTL expiration
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  expect(hook.result.current[project.workspaceId]).toEqual([]);
});
