import { describe, expect, it } from "vitest";
import { worktreeIdentity, type Worktree } from "./types";

describe("worktreeIdentity", () => {
  it("uses explicit worktree identity when available", () => {
    const wt: Worktree = {
      workspaceId: "daemon:1234",
      identity: { wsId: "remote-ws", slug: "custom-slug" },
      path: "/srv/repo/.orca-worktrees/wt-custom-slug",
      head: "abc",
      branch: "feature-branch",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    expect(worktreeIdentity(wt)).toEqual({ wsId: "remote-ws", slug: "custom-slug" });
  });

  it("extracts identity from orca branch convention when explicit identity is absent", () => {
    const wt: Worktree = {
      workspaceId: "daemon:1234",
      path: "/srv/repo/.orca-worktrees/wt-feat",
      head: "abc",
      branch: "refs/heads/orca/proj-1/feat",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    expect(worktreeIdentity(wt)).toEqual({ wsId: "proj-1", slug: "feat" });
  });

  it("returns null for non-orca branches when explicit identity is absent", () => {
    const wt: Worktree = {
      path: "/srv/repo",
      head: "abc",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    expect(worktreeIdentity(wt)).toBeNull();
  });
});
