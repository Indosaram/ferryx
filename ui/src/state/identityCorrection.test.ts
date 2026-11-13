import { beforeEach, describe, expect, it } from "vitest";
import { createLayoutState } from "./layout";
import { resetAttentionEngagementClocksForTests, workspaceReducer, type WorkspaceState } from "./workspaceStore";
import { migrateOmoLaunchedByMhc } from "../lib/sessionPersistence";

// The engagement/episode clocks are module-level; tests reuse session ids, so reset per test.
beforeEach(() => resetAttentionEngagementClocksForTests());

const MHC = { key: "session_id" as const, id: "conv-1", launcher: "mhc" };
const NO_LAUNCHER = { key: "session_id" as const, id: "conv-1" };

function fixture(agentType: string | null): WorkspaceState {
  let state: WorkspaceState = {
    workspaceId: "identity-test",
    worktrees: [{ path: "/repo", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo",
    sessions: {},
    layout: createLayoutState(),
    unreadTabIds: {},
    unreadWorktreePaths: {},
  };
  state = workspaceReducer(state, {
    type: "ADD_TAB_WITH_SESSION",
    tab: { id: "a", label: "a", sessionId: "a" },
    session: {
      id: "a",
      cwd: "/repo",
      workspaceId: "identity-test",
      backendSessionId: "backend-a",
      worktree: null,
      lifecycle: "working",
      ...(agentType ? { agentType } : {}),
    },
  });
  return state;
}

function screen(
  state: WorkspaceState,
  overrides: Partial<{
    state: "working" | "blocked" | "idle";
    agentType: string;
    providerSession: typeof MHC | typeof NO_LAUNCHER;
    isSnapshot: boolean;
  }> = {},
): WorkspaceState {
  return workspaceReducer(state, {
    type: "SESSION_SCREEN_ACTIVITY",
    tabId: "a",
    sessionId: "a",
    state: overrides.state ?? "idle",
    ruleId: "test",
    manifestId: overrides.agentType ?? "omo",
    ...(overrides.agentType ? { agentType: overrides.agentType } : {}),
    ...(overrides.providerSession ? { providerSession: overrides.providerSession } : {}),
    ...(overrides.isSnapshot ? { isSnapshot: true } : {}),
  });
}

describe("identity correction in the reducer", () => {
  it("corrects an idle omo pane to maho while it already reads done", () => {
    let state = fixture("omo");
    state = screen(state, { state: "working" });
    state = screen(state, { state: "idle" });
    const activityBefore = state.activityBySessionId?.a;
    expect(activityBefore.state).toBe("done");

    state = screen(state, { state: "idle", agentType: "maho", providerSession: MHC });
    expect(state.sessions.a.agentType).toBe("maho");
    expect(state.sessions.a.providerSession).toEqual(MHC);
    expect(state.activityBySessionId?.a).toEqual(activityBefore);
    expect(state.unreadTabIds).toEqual({});
  });

  it("does not correct without a launcher witness on the same report", () => {
    let state = fixture("omo");
    state = screen(state, { state: "idle", agentType: "maho", providerSession: NO_LAUNCHER });
    expect(state.sessions.a.agentType).toBe("omo");
  });

  it("does not correct on a report that carries no provider at all", () => {
    let state = fixture("omo");
    state = screen(state, { state: "idle", agentType: "maho" });
    expect(state.sessions.a.agentType).toBe("omo");
  });

  it("does not correct from a stale witness alone", () => {
    let state = fixture("omo");
    state = { ...state, sessions: { ...state.sessions, a: { ...state.sessions.a, providerSession: MHC } } };
    state = screen(state, { state: "idle", agentType: "maho" });
    expect(state.sessions.a.agentType).toBe("omo");
  });

  it("leaves an unrelated pair alone", () => {
    let state = fixture("claude");
    state = screen(state, { state: "idle", agentType: "maho", providerSession: MHC });
    expect(state.sessions.a.agentType).toBe("claude");
  });

  it("never flips maho back to omo", () => {
    let state = fixture("maho");
    state = screen(state, { state: "idle", agentType: "omo", providerSession: MHC });
    expect(state.sessions.a.agentType).toBe("maho");
  });

  it("carries the corrected identity into the activity record", () => {
    let state = fixture("omo");
    state = screen(state, { state: "working" });
    state = screen(state, { state: "idle", agentType: "maho", providerSession: MHC });
    expect(state.activityBySessionId?.a.agentType).toBe("maho");
  });

  it("corrects on a snapshot report too, keeping the snapshot's seen semantics", () => {
    let state = fixture("omo");
    state = screen(state, { state: "idle", agentType: "maho", providerSession: MHC, isSnapshot: true });
    expect(state.sessions.a.agentType).toBe("maho");
    expect(state.activityBySessionId?.a.seen).toBe(true);
  });
});

describe("identity correction through APPLY_PROVIDER_SESSION", () => {
  it("replaces a stored omo with the reported maho when the launcher proves it", () => {
    let state = fixture("omo");
    state = workspaceReducer(state, {
      type: "APPLY_PROVIDER_SESSION",
      sessionId: "a",
      agentType: "maho",
      providerSession: MHC,
    });
    expect(state.sessions.a.agentType).toBe("maho");
  });

  it("keeps initial adoption unchanged, launcher or not", () => {
    let state = fixture(null);
    state = workspaceReducer(state, {
      type: "APPLY_PROVIDER_SESSION",
      sessionId: "a",
      agentType: "maho",
      providerSession: NO_LAUNCHER,
    });
    expect(state.sessions.a.agentType).toBe("maho");
  });
});

describe("persisted record migration", () => {
  it("migrates only the record whose own launcher witness says mhc", () => {
    expect(migrateOmoLaunchedByMhc("omo", MHC)).toBe("maho");
    expect(migrateOmoLaunchedByMhc(" OMO ", MHC)).toBe("maho");
  });

  it("leaves every other record as it is", () => {
    expect(migrateOmoLaunchedByMhc("omo", NO_LAUNCHER)).toBe("omo");
    expect(migrateOmoLaunchedByMhc("omo", null)).toBe("omo");
    expect(migrateOmoLaunchedByMhc("maho", MHC)).toBe("maho");
    expect(migrateOmoLaunchedByMhc("claude", MHC)).toBe("claude");
    expect(migrateOmoLaunchedByMhc(null, MHC)).toBeNull();
    expect(migrateOmoLaunchedByMhc(undefined, MHC)).toBeUndefined();
  });

  it("is idempotent", () => {
    const once = migrateOmoLaunchedByMhc("omo", MHC);
    expect(migrateOmoLaunchedByMhc(once, MHC)).toBe("maho");
  });
});
