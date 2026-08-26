import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { RegisteredProject, Worktree } from "../lib/types";
import {
  useInactiveProjectWorktrees,
  type InactiveProjectWorktreeServices,
} from "./inactiveProjectWorktrees";

const gitProject: RegisteredProject = {
  workspaceId: "orca-lite",
  repoRoot: "/Users/dev/orca-lite",
  gitRoot: "/Users/dev/orca-lite",
};

const plainProject: RegisteredProject = {
  workspaceId: "superwiki-mail-otp",
  repoRoot: "/Users/dev/superwiki-mail-otp",
  gitRoot: null,
};

const mainWorktree: Worktree = {
  path: "/Users/dev/orca-lite",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function createServices(overrides?: Partial<InactiveProjectWorktreeServices>) {
  return {
    registerProject: vi.fn(async (request: { workspaceId: string; repoPath: string }) => ({
      workspaceId: request.workspaceId,
      repoRoot: request.repoPath,
      gitRoot: request.workspaceId === "orca-lite" ? request.repoPath : null,
    })),
    listWorktrees: vi.fn(async (workspaceId: string) => (workspaceId === "orca-lite" ? [mainWorktree] : [])),
    ...overrides,
  } satisfies InactiveProjectWorktreeServices;
}

describe("useInactiveProjectWorktrees", () => {
  it("registers then lists worktrees for inactive projects only", async () => {
    const services = createServices();
    const { result } = renderHook(() =>
      useInactiveProjectWorktrees([gitProject, plainProject], plainProject.workspaceId, [], services),
    );

    await waitFor(() => expect(result.current[gitProject.workspaceId]).toEqual([mainWorktree]));

    expect(services.registerProject).toHaveBeenCalledWith({
      workspaceId: gitProject.workspaceId,
      repoPath: gitProject.repoRoot,
    });
    expect(services.listWorktrees).toHaveBeenCalledTimes(1);
    expect(services.listWorktrees).toHaveBeenCalledWith(gitProject.workspaceId);
    expect(result.current[plainProject.workspaceId]).toBeUndefined();
  });

  it("falls back to the folder root for inactive plain projects", async () => {
    const services = createServices();
    const { result } = renderHook(() =>
      useInactiveProjectWorktrees([gitProject, plainProject], gitProject.workspaceId, [], services),
    );

    await waitFor(() =>
      expect(result.current[plainProject.workspaceId]).toEqual([
        {
          path: plainProject.repoRoot,
          head: "",
          branch: null,
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
      ]),
    );
  });

  it("does not list worktrees for a project whose registration reports a root conflict", async () => {
    const services = createServices({
      registerProject: vi.fn(async () => {
        throw { code: "WORKSPACE_ALREADY_REGISTERED", message: "already registered" };
      }),
    });

    const { result } = renderHook(() =>
      useInactiveProjectWorktrees([gitProject, plainProject], "superwiki-mail-otp", [], services),
    );

    await waitFor(() => expect(services.registerProject).toHaveBeenCalled());
    expect(services.listWorktrees).not.toHaveBeenCalledWith("orca-lite");
    expect(result.current["orca-lite"] ?? []).toEqual([]);
  });

  it("keeps other projects usable when one listing fails", async () => {
    const services = createServices({
      listWorktrees: vi.fn(async (workspaceId: string) => {
        if (workspaceId === "orca-lite") throw new Error("WORKSPACE_NOT_FOUND");
        return [];
      }),
    });
    const { result } = renderHook(() =>
      useInactiveProjectWorktrees([gitProject, plainProject], "other", [], services),
    );

    await waitFor(() => expect(result.current[gitProject.workspaceId]).toEqual([]));
    expect(result.current[plainProject.workspaceId]).toHaveLength(1);
  });

  it("retains outgoing active project rows in cache immediately after active project changes before async listing resolves", async () => {
    let resolveGitProjectListing: (value: Worktree[]) => void = () => {
      throw new Error("resolveGitProjectListing not initialized");
    };
    const gitProjectListingPromise = new Promise<Worktree[]>((resolve) => {
      resolveGitProjectListing = resolve;
    });

    const services = createServices({
      listWorktrees: vi.fn(async (workspaceId: string) => {
        if (workspaceId === gitProject.workspaceId) {
          return gitProjectListingPromise;
        }
        return [];
      }),
    });

    const { result, rerender } = renderHook(
      ({ activeId, activeWorktrees }) =>
        useInactiveProjectWorktrees([gitProject, plainProject], activeId, activeWorktrees, services),
      {
        initialProps: {
          activeId: gitProject.workspaceId,
          activeWorktrees: [mainWorktree],
        },
      },
    );

    rerender({
      activeId: plainProject.workspaceId,
      activeWorktrees: [],
    });

    expect(result.current[gitProject.workspaceId]).toEqual([mainWorktree]);

    resolveGitProjectListing([mainWorktree]);
    await waitFor(() => expect(result.current[gitProject.workspaceId]).toEqual([mainWorktree]));
  });
});
