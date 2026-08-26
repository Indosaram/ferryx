import { describe, expect, it } from "vitest";

import { branchName, displayWorkspaceTitle, workspaceName } from "./branchFilter";
import type { Worktree } from "./types";

function worktree(overrides: Partial<Worktree> = {}): Worktree {
  return {
    path: "/Users/dev/code/project",
    head: "0123456789abcdef0123456789abcdef01234567",
    branch: "refs/heads/main",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
    ...overrides,
  };
}

describe("branchName", () => {
  it("strips the refs/heads/ prefix from a fully qualified ref", () => {
    expect(branchName(worktree({ branch: "refs/heads/feature/login" }))).toBe("feature/login");
  });

  it("passes through a short branch name unchanged", () => {
    expect(branchName(worktree({ branch: "develop" }))).toBe("develop");
  });

  it("reports a detached HEAD when no branch is attached", () => {
    expect(branchName(worktree({ branch: null }))).toBe("detached HEAD");
  });

  it("only strips the prefix at the start so a nested ref segment survives", () => {
    expect(branchName(worktree({ branch: "refs/heads/wip/refs/heads/odd" }))).toBe(
      "wip/refs/heads/odd",
    );
  });
});

describe("displayWorkspaceTitle", () => {
  it("reduces a managed orca branch to its slug", () => {
    expect(displayWorkspaceTitle(worktree({ branch: "refs/heads/orca/ws-7/add-auth" }))).toBe(
      "add-auth",
    );
  });

  it("keeps the remaining segments of a managed branch whose slug contains a slash", () => {
    expect(displayWorkspaceTitle(worktree({ branch: "orca/ws-7/feature/deep/nest" }))).toBe(
      "feature/deep/nest",
    );
  });

  it("does not treat a two-segment orca branch as managed", () => {
    expect(displayWorkspaceTitle(worktree({ branch: "orca/ws-7" }))).toBe("orca/ws-7");
  });

  it("shows the folder basename for a plain-folder project with no branch", () => {
    expect(
      displayWorkspaceTitle(worktree({ branch: null, path: "/Users/dev/code/notes-app" })),
    ).toBe("notes-app");
  });

  it("ignores trailing slashes when deriving the folder basename", () => {
    expect(
      displayWorkspaceTitle(worktree({ branch: null, path: "/Users/dev/code/notes-app///" })),
    ).toBe("notes-app");
  });

  it("falls back to main when a branchless worktree has no usable basename", () => {
    expect(displayWorkspaceTitle(worktree({ branch: null, path: "/" }))).toBe("main");
  });

  it("returns the branch name for an ordinary feature branch", () => {
    expect(displayWorkspaceTitle(worktree({ branch: "refs/heads/release/2026" }))).toBe(
      "release/2026",
    );
  });

  it.each(["ferryx", "rorca", "orca-lite"])(
    "hides the local repo folder name %s behind main",
    (branch) => {
      expect(displayWorkspaceTitle(worktree({ branch }))).toBe("main");
    },
  );

  it("exposes workspaceName as an alias of displayWorkspaceTitle", () => {
    expect(workspaceName).toBe(displayWorkspaceTitle);
  });
});
