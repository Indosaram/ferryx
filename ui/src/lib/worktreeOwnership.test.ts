import { describe, expect, it } from "vitest";

import type { RegisteredProject, Worktree } from "./types";
import { resolveWorktreeOwnerId } from "./worktreeOwnership";

function project(workspaceId: string, repoRoot: string): RegisteredProject {
  return { workspaceId, repoRoot, gitRoot: repoRoot };
}

function remoteProject(workspaceId: string, repoRoot: string, hostId: string): RegisteredProject {
  return { workspaceId, repoRoot, gitRoot: repoRoot, target: { kind: "ssh", hostId } };
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

  it("resolves ownership for nested drive-letter backslash paths and avoids sibling prefixes", () => {
    const projects = [
      project("outer", "C:\\repos\\outer"),
      project("inner", "C:\\repos\\outer\\inner\\"),
    ];

    expect(
      resolveWorktreeOwnerId(worktree("C:\\repos\\outer\\inner\\sub", null), projects, "outer"),
    ).toBe("inner");
    expect(
      resolveWorktreeOwnerId(worktree("C:\\repos\\outer-sibling\\nested", null), projects, "fallback"),
    ).toBe("fallback");
  });

  it("resolves ownership for UNC paths and avoids sibling prefixes", () => {
    const projects = [
      project("outer", "\\\\server\\share\\outer"),
      project("inner", "\\\\server\\share\\outer\\inner\\"),
    ];

    expect(
      resolveWorktreeOwnerId(worktree("\\\\server\\share\\outer\\inner\\sub", null), projects, "outer"),
    ).toBe("inner");
    expect(
      resolveWorktreeOwnerId(
        worktree("\\\\server\\share\\outer-sibling\\nested", null),
        projects,
        "fallback",
      ),
    ).toBe("fallback");
  });

  it("recovers a missing owner id for a Windows SSH row from its unique registered remote root", () => {
    const projects = [
      project("local-group", "/Users/indo/work/coinbase-scalper"),
      remoteProject("ssh:windows-project", "C:/Users/sook/work/coinbase-scalper", "maho-win"),
    ];
    const row = worktree("C:/Users/sook/work/coinbase-scalper", null);

    expect(resolveWorktreeOwnerId(row, projects, "local-group")).toBe("ssh:windows-project");
  });

  it("does not guess between remote hosts when the same remote path is registered twice", () => {
    const projects = [
      remoteProject("ssh:win-a", "C:/Users/sook/work/repo", "win-a"),
      remoteProject("ssh:win-b", "C:/Users/sook/work/repo", "win-b"),
    ];
    const row = worktree("C:/Users/sook/work/repo", null);

    expect(resolveWorktreeOwnerId(row, projects, "fallback")).toBe("fallback");
  });
});
