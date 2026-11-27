import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { loadCollapsedProjects } from "../lib/sidebarPersistence";
import type { RegisteredProject } from "../lib/tauri";
import type { Worktree } from "../lib/types";
import { Sidebar, SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY } from "./Sidebar";

function project(workspaceId: string): RegisteredProject {
  return { workspaceId, repoRoot: `/repo/${workspaceId}`, gitRoot: `/repo/${workspaceId}`, registeredAt: "2026-01-01T00:00:00Z" } as RegisteredProject;
}

function worktree(workspaceId: string): Worktree {
  return { path: `/repo/${workspaceId}`, head: "abc", branch: "refs/heads/main", workspaceId } as Worktree;
}

function sidebar(
  projects: RegisteredProject[],
  emptyWorkspaceIds: string[] = [],
  revealGroupRequest: { groupId: string; id: number } | null = null,
) {
  return (
    <Sidebar
      projects={projects}
      activeProjectId="alpha"
      worktrees={[worktree("alpha")]}
      emptyWorkspaceIds={emptyWorkspaceIds}
      agents={[]}
      activePath="/repo/alpha"
      onSelectWorktree={() => undefined}
      onCreateWorktree={() => undefined}
      revealGroupRequest={revealGroupRequest}
    />
  );
}

describe("Sidebar collapsed set persistence", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
  });

  it("persists the collapsed seed of a project registered after mount", () => {
    const { rerender } = render(sidebar([project("alpha")]));

    rerender(sidebar([project("alpha"), project("beta")]));

    expect(screen.getByRole("button", { name: "Expand beta" })).toBeInTheDocument();
    expect([...(loadCollapsedProjects() ?? [])]).toEqual(["beta"]);
  });

  it("persists a group that loses its last worktree as collapsed", () => {
    const { rerender } = render(sidebar([project("alpha")]));

    rerender(sidebar([project("alpha")], ["alpha"]));

    expect(screen.getByRole("button", { name: "Expand alpha" })).toBeInTheDocument();
    expect([...(loadCollapsedProjects() ?? [])]).toEqual(["alpha"]);
  });

  it("expands a collapsed group when a reveal request names it", () => {
    const projects = [project("alpha"), project("beta")];
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(["alpha", "beta"]));
    const { rerender } = render(sidebar(projects));

    expect(screen.getByRole("button", { name: "Expand beta" })).toBeInTheDocument();

    rerender(sidebar(projects, [], { groupId: "beta", id: 1 }));

    expect(screen.getByRole("button", { name: "Collapse beta" })).toBeInTheDocument();
    // The reveal is persisted, so the next mount keeps the group open instead of re-collapsing it.
    expect([...(loadCollapsedProjects() ?? [])]).toEqual(["alpha"]);
  });

  it("reapplies a repeated reveal of the same group", () => {
    const projects = [project("alpha"), project("beta")];
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(["alpha", "beta"]));
    const { rerender } = render(sidebar(projects, [], { groupId: "beta", id: 1 }));

    expect(screen.getByRole("button", { name: "Collapse beta" })).toBeInTheDocument();

    // The user collapses it again; a second request with a new id has to reopen it.
    fireEvent.click(screen.getByRole("button", { name: "Collapse beta" }));
    expect(screen.getByRole("button", { name: "Expand beta" })).toBeInTheDocument();

    rerender(sidebar(projects, [], { groupId: "beta", id: 2 }));
    expect(screen.getByRole("button", { name: "Collapse beta" })).toBeInTheDocument();
  });
});
