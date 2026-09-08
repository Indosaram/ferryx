import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RegisteredProject, Worktree } from "../lib/types";
import { Sidebar, SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY } from "./Sidebar";

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, "[]");
});
afterEach(cleanup);

const main: RegisteredProject = {
  workspaceId: "main-project", repoRoot: "/repo/app",
  gitRemote: "https://github.com/team/app.git", gitCommonDir: "/repo/app/.git",
};
const linked: RegisteredProject = {
  workspaceId: "feature-project", repoRoot: "/worktrees/feature",
  gitCommonDir: "/repo/app/.git",
};
function row(project: RegisteredProject, branch: string): Worktree {
  return {
    path: project.repoRoot, branch: `refs/heads/${branch}`, head: "abc",
    bare: false, detached: false, locked: null, prunable: null,
  };
}

describe("Sidebar Git project identity", () => {
  it("keeps every grouped checkout visible and routes its click to its owning workspace", () => {
    const onSelectWorktree = vi.fn();
    render(<Sidebar
      projects={[main, linked]} activeProjectId={main.workspaceId}
      worktrees={[row(main, "main")]}
      inactiveProjectWorktrees={{ [linked.workspaceId]: [row(linked, "feature")] }}
      agents={[]} activePath={main.repoRoot}
      onSelectWorktree={onSelectWorktree} onCreateWorktree={() => undefined}
    />);
    const feature = screen.getByRole("button", { name: /^feature\b/ });
    fireEvent.click(feature);
    expect(onSelectWorktree).toHaveBeenCalledWith(expect.objectContaining({
      path: linked.repoRoot, workspaceId: linked.workspaceId,
    }));
    expect(screen.queryByRole("button", { name: /^(Expand|Collapse) feature$/ })).toBeNull();
  });

  it("groups differently named remote checkouts but preserves each SSH target on click", () => {
    const remote: RegisteredProject = {
      workspaceId: "ssh:linux", repoRoot: "/srv/different-checkout",
      gitRemote: "git@github.com:team/app.git",
      target: { kind: "ssh", hostId: "linux" },
    };
    const onSelectWorktree = vi.fn();
    render(<Sidebar
      projects={[main, remote]} activeProjectId={main.workspaceId}
      worktrees={[row(main, "main")]} agents={[]} activePath={main.repoRoot}
      onSelectWorktree={onSelectWorktree} onCreateWorktree={() => undefined}
    />);
    const target = screen.getByTitle(`Remote SSH root: ${remote.repoRoot} (linux)`);
    fireEvent.click(target);
    expect(onSelectWorktree).toHaveBeenCalledWith(expect.objectContaining({
      path: remote.repoRoot, workspaceId: remote.workspaceId,
    }));
  });
});
