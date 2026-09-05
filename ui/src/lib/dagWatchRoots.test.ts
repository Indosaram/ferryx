import { describe, expect, it } from "vitest";
import { collectDagWatchRoots } from "./dagWatchRoots";
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
});
