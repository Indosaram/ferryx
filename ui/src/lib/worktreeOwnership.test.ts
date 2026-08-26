import { describe, expect, it } from "vitest";

import type { RegisteredProject, Worktree } from "./types";
import { resolveWorktreeOwnerId } from "./worktreeOwnership";

function project(workspaceId: string, repoRoot: string): RegisteredProject {
  return { workspaceId, repoRoot, gitRoot: repoRoot };
}

function worktree(path: string, branch: string | null): Worktree {
  return { path, head: "", branch, bare: false, detached: false, locked: null, prunable: null };
}

describe("resolveWorktreeOwnerId", () => {
  it("routes a branch-identified worktree to the project named by its branch", () => {
    const projects = [project("alpha", "/repos/alpha"), project("beta", "/repos/beta")];
    const row = worktree("/repos/alpha/.orca-worktrees/wt-x", "refs/heads/orca/beta/x");

    expect(resolveWorktreeOwnerId(row, projects, "alpha")).toBe("beta");
  });

  it("prefers the deepest owning root when one repoRoot is a prefix of another", () => {
    const projects = [project("outer", "/repos/outer"), project("inner", "/repos/outer/inner")];
    const row = worktree("/repos/outer/inner/sub", null);

    expect(resolveWorktreeOwnerId(row, projects, "outer")).toBe("inner");
  });

  it("does not treat a sibling directory sharing a name prefix as owned", () => {
    const projects = [project("alpha", "/repos/alpha")];
    const row = worktree("/repos/alpha-sibling/nested", null);

    expect(resolveWorktreeOwnerId(row, projects, "fallback")).toBe("fallback");
  });

  it("tolerates trailing slashes on a registered root", () => {
    const projects = [project("alpha", "/repos/alpha/")];
    const row = worktree("/repos/alpha/nested", null);

    expect(resolveWorktreeOwnerId(row, projects, "fallback")).toBe("alpha");
  });

  it("ignores a branch naming another project when the row is that project's own root", () => {
    const projects = [project("alpha", "/repos/alpha"), project("beta", "/repos/beta")];
    const row = worktree("/repos/alpha", "refs/heads/orca/beta/mislabeled");

    expect(resolveWorktreeOwnerId(row, projects, "beta")).toBe("alpha");
  });
});
