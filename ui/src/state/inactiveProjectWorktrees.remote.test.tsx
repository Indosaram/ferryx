import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useInactiveProjectWorktrees } from "./inactiveProjectWorktrees";
import type { RegisteredProject, WorktreeChangedPayload } from "../lib/types";

afterEach(cleanup);
it("never registers or lists an inactive SSH path with local project services", async () => {
  const project: RegisteredProject = { workspaceId: "ssh:build", repoRoot: "/srv/repo", gitRoot: "/srv/repo", target: { kind: "ssh", hostId: "build" } };
  let onChanged!: (payload: WorktreeChangedPayload) => void;
  const services = {
    registerProject: vi.fn(async () => project), listWorktrees: vi.fn(async () => []),
    onWorktreeChanged: vi.fn(async (handler: (payload: WorktreeChangedPayload) => void) => { onChanged = handler; return () => undefined; }),
  };
  const hook = renderHook(({ target }) => useInactiveProjectWorktrees([target], "local", [], services), { initialProps: { target: project } });
  await act(async () => { await services.onWorktreeChanged.mock.results[0].value; });
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).not.toHaveBeenCalled();
  expect(hook.result.current[project.workspaceId]).toEqual([expect.objectContaining({ path: project.repoRoot, workspaceId: project.workspaceId })]);
  act(() => { onChanged({ workspaceId: project.workspaceId, kind: "deleted", worktree: { wsId: project.workspaceId, slug: "root" } }); });
  expect(services.listWorktrees).not.toHaveBeenCalled();
  await act(async () => { hook.rerender({ target: { ...project, repoRoot: "/srv/canonical", target: { kind: "ssh", hostId: "other-host" } } }); });
  expect(hook.result.current[project.workspaceId]).toEqual([expect.objectContaining({ path: "/srv/canonical" })]);
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).not.toHaveBeenCalled();
});
