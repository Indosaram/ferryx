import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useInactiveProjectWorktrees } from "./inactiveProjectWorktrees";
import { remoteHostStore } from "./remoteHostStore";
import { projectRootWorktree } from "../lib/projectIdentity";
import type { RegisteredProject, WorktreeChangedPayload } from "../lib/types";

const adapter = vi.hoisted(() => ({ capabilities: vi.fn(), projects: vi.fn(), worktrees: vi.fn() }));
vi.mock("../lib/pairedDaemonProject", () => ({ createPairedDaemonProjectAdapter: vi.fn(() => adapter) }));
afterEach(() => { cleanup(); remoteHostStore.reset(); vi.clearAllMocks(); });
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
  expect(hook.result.current[project.workspaceId]).toBe(retained);
  adapter.projects.mockRejectedValue(new Error("OFFLINE"));
  await act(async () => { changed({ workspaceId: project.workspaceId, kind: "pruned", worktree: { wsId: "repo", slug: "feature" } }); });
  expect(hook.result.current[project.workspaceId]).toBe(retained);
  expect(services.listWorktrees).not.toHaveBeenCalled();
});
