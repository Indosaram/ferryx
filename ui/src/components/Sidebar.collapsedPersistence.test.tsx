import { render, screen } from "@testing-library/react";
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

function sidebar(projects: RegisteredProject[], emptyWorkspaceIds: string[] = []) {
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
});
