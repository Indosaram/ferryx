import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useInactiveProjectWorktrees } from "./inactiveProjectWorktrees";
import type { RegisteredProject, WorktreeChangedPayload } from "../lib/types";

afterEach(cleanup);

it("initially lists an inactive git-backed SSH project's worktrees", async () => {
  const project: RegisteredProject = {
    workspaceId: "ssh:build",
    repoRoot: "/srv/repo",
    gitRoot: "/srv/repo",
    target: { kind: "ssh", hostId: "build" },
  };
  const mockWorktrees = [
    {
      workspaceId: "ssh:build",
      path: "/srv/repo",
      head: "abc",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
    {
      workspaceId: "ssh:build",
      path: "/srv/repo/.orca-worktrees/wt-feat",
      head: "def",
      branch: "refs/heads/orca/ssh-build/feat",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    },
  ];
  const services = {
    registerProject: vi.fn(),
    listWorktrees: vi.fn(async () => mockWorktrees),
  };
  const hook = renderHook(() =>
    useInactiveProjectWorktrees([project], "local", [], services as any),
  );
  await act(async () => {});
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).toHaveBeenCalledWith("ssh:build");
  expect(hook.result.current[project.workspaceId]).toEqual(mockWorktrees);
});

it("never registers an inactive SSH path with local project services and relists on worktree changed", async () => {
  const project: RegisteredProject = { workspaceId: "ssh:build", repoRoot: "/srv/repo", gitRoot: "/srv/repo", target: { kind: "ssh", hostId: "build" } };
  let onChanged!: (payload: WorktreeChangedPayload) => void;
  const services = {
    registerProject: vi.fn(async () => project), listWorktrees: vi.fn(async () => []),
    onWorktreeChanged: vi.fn(async (handler: (payload: WorktreeChangedPayload) => void) => { onChanged = handler; return () => undefined; }),
  };
  const hook = renderHook(({ target }) => useInactiveProjectWorktrees([target], "local", [], services), { initialProps: { target: project } });
  await act(async () => { await services.onWorktreeChanged.mock.results[0].value; });
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).toHaveBeenCalledWith(project.workspaceId);
  services.listWorktrees.mockClear();
  expect(hook.result.current[project.workspaceId]).toEqual([expect.objectContaining({ path: project.repoRoot, workspaceId: project.workspaceId })]);
  act(() => { onChanged({ workspaceId: project.workspaceId, kind: "created", worktree: { wsId: project.workspaceId, slug: "feat" } }); });
  expect(services.listWorktrees).toHaveBeenCalledWith(project.workspaceId);
  expect(services.registerProject).not.toHaveBeenCalled();
  services.listWorktrees.mockClear();
  await act(async () => { hook.rerender({ target: { ...project, repoRoot: "/srv/canonical", target: { kind: "ssh", hostId: "other-host" } } }); });
  expect(hook.result.current[project.workspaceId]).toEqual([expect.objectContaining({ path: "/srv/canonical" })]);
  expect(services.registerProject).not.toHaveBeenCalled();
  expect(services.listWorktrees).toHaveBeenCalledWith(project.workspaceId);
});
