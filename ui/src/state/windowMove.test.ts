import { describe, expect, it } from "vitest";

import type { TerminalSession } from "../lib/types";
import { movedEverySession, planTabMove } from "./windowMove";
import type { WorkspaceState } from "./workspaceStore";

function stateWith(options: {
  workspaceId?: string;
  sessions: Record<string, Partial<TerminalSession>>;
  tabs: Array<Record<string, unknown>>;
  layouts?: Record<string, unknown>;
}): WorkspaceState {
  return {
    workspaceId: options.workspaceId ?? "local",
    worktrees: [],
    activeWorktreePath: "/repo",
    sessions: options.sessions,
    layout: { tabs: options.tabs, layoutsByTabId: options.layouts ?? {} },
  } as unknown as WorkspaceState;
}

describe("planTabMove", () => {
  it("collects the backend session of every terminal pane in the tab", () => {
    const plan = planTabMove(
      stateWith({
        sessions: { s1: { backendSessionId: "b1" }, s2: { backendSessionId: "b2" } },
        tabs: [{ id: "t1", label: "a", sessionId: "s1" }],
        layouts: {
          t1: {
            sessionIdsByLeafId: { l1: "s1", l2: "s2" },
            contentsByLeafId: { l1: { kind: "terminal", sessionId: "s1" }, l2: { kind: "terminal", sessionId: "s2" } },
          },
        },
      }),
      "t1",
    );
    expect(plan).toEqual({ ok: true, backendSessionIds: ["b1", "b2"] });
  });

  it("refuses browser tabs, mixed tabs, remote workspaces and sessions without a backend yet", () => {
    const browser = stateWith({ sessions: {}, tabs: [{ id: "t1", kind: "browser", label: "b" }] });
    expect(planTabMove(browser, "t1").ok).toBe(false);

    const mixed = stateWith({
      sessions: { s1: { backendSessionId: "b1" } },
      tabs: [{ id: "t1", label: "a", sessionId: "s1" }],
      layouts: {
        t1: { sessionIdsByLeafId: { l1: "s1", l2: "" }, contentsByLeafId: { l1: { kind: "terminal", sessionId: "s1" }, l2: { kind: "browser" } } },
      },
    });
    expect(planTabMove(mixed, "t1").ok).toBe(false);

    const remote = stateWith({
      workspaceId: "ssh:host:repo",
      sessions: { s1: { backendSessionId: "b1" } },
      tabs: [{ id: "t1", label: "a", sessionId: "s1" }],
    });
    expect(planTabMove(remote, "t1").ok).toBe(false);

    const starting = stateWith({ sessions: { s1: {} }, tabs: [{ id: "t1", label: "a", sessionId: "s1" }] });
    expect(planTabMove(starting, "t1").ok).toBe(false);
    expect(planTabMove(starting, "missing").ok).toBe(false);
  });
});

describe("movedEverySession", () => {
  it("is true only when the backend moved every requested session", () => {
    expect(movedEverySession({ moved: ["b1", "b2"] }, ["b1", "b2"])).toBe(true);
    expect(movedEverySession({ moved: ["b1"] }, ["b1", "b2"])).toBe(false);
  });
});
