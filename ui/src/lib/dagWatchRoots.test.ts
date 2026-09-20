import { describe, expect, it } from "vitest";
import { collectDagWatchRoots, isLocalDagProject, remoteProjectsWatchKey } from "./dagWatchRoots";
import type { TerminalSession } from "./types";

function session(overrides: Partial<TerminalSession> & { id: string; cwd: string }): TerminalSession {
  return {
    workspaceId: "ws",
    worktree: null,
    backendSessionId: `backend-${overrides.id}`,
    lifecycle: "running",
    ...overrides,
  };
}

describe("collectDagWatchRoots", () => {
  it("watches the root of a session opened outside every registered project", () => {
    const roots = collectDagWatchRoots({
      projectRoots: ["/repos/orca-lite"],
      worktreePaths: [],
      sessions: {
        "s-1": session({ id: "s-1", cwd: "/repos/orca-lite" }),
        "s-2": session({ id: "s-2", cwd: "/Users/dev/code" }),
        "s-3": session({ id: "s-3", cwd: "/Volumes/ext/other-repo" }),
      },
    });

    expect(roots).toContain("/Users/dev/code");
    expect(roots).toContain("/Volumes/ext/other-repo");
    expect(roots).toContain("/repos/orca-lite");
  });

  it("keeps the worktree root and the nested cwd of one session", () => {
    const roots = collectDagWatchRoots({
      projectRoots: [],
      worktreePaths: [],
      sessions: [
        session({ id: "s-1", cwd: "/repos/app/ui", worktreePath: "/repos/app" }),
      ],
    });

    expect(roots).toEqual(["/repos/app", "/repos/app/ui"]);
  });

  it("drops blank paths and duplicates across every source", () => {
    const roots = collectDagWatchRoots({
      projectRoots: ["/repos/app", "", "   "],
      worktreePaths: ["/repos/app", "/repos/app-wt"],
      sessions: [session({ id: "s-1", cwd: "/repos/app" })],
    });

    expect(roots).toEqual(["/repos/app", "/repos/app-wt"]);
  });

  describe("isLocalDagProject", () => {
    it("returns true for local projects without a target", () => {
      expect(isLocalDagProject({ target: null })).toBe(true);
      expect(isLocalDagProject({})).toBe(true);
      expect(isLocalDagProject(null)).toBe(true);
      expect(isLocalDagProject(undefined)).toBe(true);
    });

    it("returns false for remote projects (ssh and pairedDaemon)", () => {
      expect(isLocalDagProject({ target: { kind: "ssh" } })).toBe(false);
      expect(isLocalDagProject({ target: { kind: "pairedDaemon" } })).toBe(false);
    });
  });

  describe("remoteProjectsWatchKey", () => {
    it("returns empty string when only local projects exist", () => {
      expect(
        remoteProjectsWatchKey([
          { workspaceId: "ws-1", repoRoot: "/local/repo1", target: null },
          { workspaceId: "ws-2", repoRoot: "/local/repo2" },
        ])
      ).toBe("");
    });

    it("includes both pairedDaemon and ssh projects with stable sorted ordering", () => {
      const key = remoteProjectsWatchKey([
        { workspaceId: "ws-ssh", repoRoot: "/remote/ssh-repo", target: { kind: "ssh" } },
        { workspaceId: "ws-local", repoRoot: "/local/repo", target: null },
        { workspaceId: "ws-paired", repoRoot: "/remote/paired-repo", target: { kind: "pairedDaemon" } },
      ]);

      expect(key).toBe(
        "ws-paired:/remote/paired-repo:pairedDaemon\nws-ssh:/remote/ssh-repo:ssh"
      );
    });
  });
});
