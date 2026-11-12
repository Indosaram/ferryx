import { describe, expect, it } from "vitest";

import type { AgentSpawnedEvent } from "../lib/agentSpawned";
import { createLayoutState } from "./layout";
import { collectLeafIds, type PaneNode } from "./paneTree";
import { planSpawnedAgentAdoption, workspaceReducer, type WorkspaceState } from "./workspaceStore";

const WORKSPACE = "spawn-ws";

function fixture(): WorkspaceState {
  let state: WorkspaceState = {
    workspaceId: WORKSPACE,
    worktrees: [{ path: "/repo", head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    activeWorktreePath: "/repo",
    sessions: {}, layout: createLayoutState(), unreadTabIds: {}, unreadWorktreePaths: {},
  };
  state = workspaceReducer(state, { type: "ADD_TAB_WITH_SESSION",
    tab: { id: "tab-parent", label: "parent", sessionId: "parent" },
    session: { id: "parent", cwd: "/repo", worktreePath: "/repo", workspaceId: WORKSPACE, backendSessionId: "backend-parent", worktree: null, lifecycle: "working" },
  });
  return state;
}

function spawned(overrides: Partial<AgentSpawnedEvent> = {}): AgentSpawnedEvent {
  return {
    sessionId: "backend-child",
    parentSessionId: "backend-parent",
    depth: 1,
    workspaceId: WORKSPACE,
    cwd: "/repo",
    agent: "claude",
    name: "helper",
    daemonEpoch: "7",
    ...overrides,
  };
}

const identity = { incarnation: "inc-1", frontendSessionId: "child", newLeafId: "leaf-child" };

function parentLeafId(state: WorkspaceState): string {
  const layout = state.layout.layoutsByTabId?.["tab-parent"];
  if (!layout) throw new Error("the parent tab has no pane layout");
  const leaf = Object.entries(layout.sessionIdsByLeafId).find(([, sessionId]) => sessionId === "parent");
  if (!leaf) throw new Error("the parent has no leaf");
  return leaf[0];
}

describe("spawned agent adoption", () => {
  it("adds the spawned pane as a split beside the parent leaf, attached to the live backend session", () => {
    const state = fixture();
    const parentLeaf = parentLeafId(state);
    const plan = planSpawnedAgentAdoption(state, spawned(), identity);
    expect(plan?.kind).toBe("split");
    if (plan?.kind !== "split") return;
    const next = workspaceReducer(state, plan.action);

    const layout = next.layout.layoutsByTabId?.["tab-parent"];
    expect(layout).toBeDefined();
    const root = layout!.root as PaneNode;
    expect(root.type).toBe("split");
    if (root.type !== "split") return;
    // The parent keeps its leaf; the child is its direct sibling, to the right.
    expect(root.direction).toBe("horizontal");
    expect(root.first).toMatchObject({ type: "leaf", leafId: parentLeaf });
    expect(root.second).toMatchObject({ type: "leaf", leafId: "leaf-child" });
    expect(collectLeafIds(root)).toEqual([parentLeaf, "leaf-child"]);
    expect(layout!.sessionIdsByLeafId["leaf-child"]).toBe("child");

    expect(next.sessions.child).toMatchObject({
      backendSessionId: "backend-child",
      workspaceId: WORKSPACE,
      worktreePath: "/repo",
      agentType: "claude",
      daemonEpoch: "7",
      incarnation: "inc-1",
    });
    // Adoption never touches the parent's own session.
    expect(next.sessions.parent).toEqual(state.sessions.parent);
  });

  it("leaves a pane this window already shows, an unknown parent, a root spawn and another project alone", () => {
    const state = fixture();
    const plan = planSpawnedAgentAdoption(state, spawned(), identity);
    if (plan?.kind !== "split") throw new Error("expected a split");
    const shown = workspaceReducer(state, plan.action);
    expect(planSpawnedAgentAdoption(shown, spawned(), { ...identity, frontendSessionId: "again" })).toBeNull();
    expect(planSpawnedAgentAdoption(state, spawned({ parentSessionId: "backend-elsewhere" }), identity)).toBeNull();
    expect(planSpawnedAgentAdoption(state, spawned({ parentSessionId: null }), identity)).toBeNull();
    expect(planSpawnedAgentAdoption(state, spawned({ workspaceId: "other-ws" }), identity)).toBeNull();
  });
});
