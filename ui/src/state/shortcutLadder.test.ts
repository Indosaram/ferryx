import { describe, expect, it } from "vitest";

import type { RegisteredProject, Worktree } from "../lib/types";
import { listShortcutWorktrees } from "./shortcutLadder";

function project(workspaceId: string, repoRoot: string, gitRoot: string | null = null): RegisteredProject {
  return { workspaceId, repoRoot, gitRoot, gitRemote: null, gitBranch: null, gitHead: null };
}

function worktree(path: string, workspaceId?: string): Worktree {
  return {
    workspaceId,
    path,
    head: "abc123",
    branch: "main",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

const projects = [
  project("ferryx", "/repo/ferryx"),
  project("mahoquot", "/repo/mahoquot"),
  project("noveling", "/repo/noveling"),
];

describe("listShortcutWorktrees", () => {
  it("keeps collapsed projects on the ladder so digits still reach them", () => {
    const ladder = listShortcutWorktrees({
      projects,
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx"), worktree("/repo/ferryx-wt/a")],
      inactiveProjectWorktrees: {
        mahoquot: [worktree("/repo/mahoquot")],
        noveling: [worktree("/repo/noveling")],
      },
    });

    expect(ladder.map((row) => row.path)).toEqual([
      "/repo/ferryx",
      "/repo/ferryx-wt/a",
      "/repo/mahoquot",
      "/repo/noveling",
    ]);
  });

  it("keeps a project on the ladder when no snapshot or listing has loaded", () => {
    const ladder = listShortcutWorktrees({
      projects,
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx")],
      inactiveProjectWorktrees: {},
    });

    expect(ladder.map((row) => row.path)).toEqual([
      "/repo/ferryx",
      "/repo/mahoquot",
      "/repo/noveling",
    ]);
  });

  it("gives every worktree a tabless or tabbed state the same slot", () => {
    const withoutTabs = listShortcutWorktrees({
      projects: projects.slice(0, 2),
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx")],
      inactiveProjectWorktrees: {},
    });
    const withTabs = listShortcutWorktrees({
      projects: projects.slice(0, 2),
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx")],
      inactiveProjectWorktrees: { mahoquot: [worktree("/repo/mahoquot/wt-1")] },
    });

    expect(withoutTabs.map((row) => row.path)).toEqual(["/repo/ferryx", "/repo/mahoquot"]);
    expect(withTabs.map((row) => row.path)).toEqual(["/repo/ferryx", "/repo/mahoquot/wt-1"]);
  });

  it("adds an idle git project only once its listing arrives", () => {
    const gitProjects = [
      project("ferryx", "/repo/ferryx", "/repo/ferryx"),
      project("mahoquot", "/repo/mahoquot", "/repo/mahoquot"),
    ];

    const beforeListing = listShortcutWorktrees({
      projects: gitProjects,
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx")],
      inactiveProjectWorktrees: {},
    });

    // No phantom row in front of the active project's first digit.
    expect(beforeListing.map((row) => row.path)).toEqual(["/repo/ferryx"]);

    const afterListing = listShortcutWorktrees({
      projects: gitProjects,
      activeProjectId: "ferryx",
      activeWorktrees: [worktree("/repo/ferryx")],
      inactiveProjectWorktrees: { mahoquot: [worktree("/repo/mahoquot"), worktree("/repo/mahoquot/wt-1")] },
    });

    // Digit 2 means mahoquot's root and never renumbers the active project's rows.
    expect(afterListing.map((row) => row.path)).toEqual([
      "/repo/ferryx",
      "/repo/mahoquot",
      "/repo/mahoquot/wt-1",
    ]);
  });
});
