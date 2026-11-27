import { StrictMode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LayoutState, TerminalSession, TerminalTab, Worktree } from "../lib/types";
import { getHmrWorkspaceState } from "./hmrWorkspaceState";
import { collectLeafIds } from "./paneTree";
import { getWorkspaceSnapshot } from "./workspaceSnapshotCache";
import {
  useWorkspaceStore,
  withPreMintedActionIds,
  workspaceReducer,
  type WorkspaceAction,
  type WorkspaceServices,
  type WorkspaceState,
} from "./workspaceStore";

afterEach(cleanup);

const WORKSPACE_ID = "ws-deterministic";
const TAB_A = "tab-det-a";
const TAB_B = "tab-det-b";
const GROUP_A = "group-det-a";
const GROUP_B = "group-det-b";
/**
 * Both tabs share one leaf id on purpose: legacy layouts give every tab `leaf-init`, and a
 * shared id is what forces `moveTabIntoPaneSplit` down its remap path (the only place a pane
 * move mints a leaf id).
 */
const SHARED_LEAF = "leaf-shared";

const worktree: Worktree = {
  path: "/repo/det",
  head: "det-head",
  branch: "refs/heads/det",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

function terminalSession(id: string): TerminalSession {
  return {
    id,
    cwd: worktree.path,
    worktreePath: worktree.path,
    workspaceId: WORKSPACE_ID,
    worktree: { wsId: WORKSPACE_ID, slug: "det" },
    backendSessionId: `backend-${id}`,
    lifecycle: "working",
  };
}

const tabA: TerminalTab = { kind: "terminal", id: TAB_A, label: "a", sessionId: "session-a" };
const tabB: TerminalTab = { kind: "terminal", id: TAB_B, label: "b", sessionId: "session-b" };

function paneLayout(sessionId: string): LayoutState["layoutsByTabId"][string] {
  return {
    root: { type: "leaf", leafId: SHARED_LEAF },
    activeLeafId: SHARED_LEAF,
    expandedLeafId: null,
    sessionIdsByLeafId: { [SHARED_LEAF]: sessionId },
    contentsByLeafId: { [SHARED_LEAF]: { kind: "terminal", sessionId } },
  };
}

/** Two terminal tabs in two tab groups: the shape both move actions need. */
function twoGroupState(): WorkspaceState {
  return {
    workspaceId: WORKSPACE_ID,
    worktrees: [worktree],
    activeWorktreePath: worktree.path,
    sessions: { "session-a": terminalSession("session-a"), "session-b": terminalSession("session-b") },
    layout: {
      tabs: [tabA, tabB],
      activeTabId: TAB_A,
      tabGroups: {
        [GROUP_A]: { id: GROUP_A, tabIds: [TAB_A], activeTabId: TAB_A },
        [GROUP_B]: { id: GROUP_B, tabIds: [TAB_B], activeTabId: TAB_B },
      },
      tabGroupLayout: {
        type: "split",
        direction: "horizontal",
        first: { type: "group", groupId: GROUP_A },
        second: { type: "group", groupId: GROUP_B },
        ratio: 0.5,
      },
      focusedGroupId: GROUP_A,
      layoutsByTabId: { [TAB_A]: paneLayout("session-a"), [TAB_B]: paneLayout("session-b") },
    },
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function services(): WorkspaceServices {
  return {
    ensureTerminalEvents: vi.fn(async () => undefined),
    spawnTerminal: vi.fn(async () => "unexpected-backend"),
    getTerminalCwd: vi.fn(async () => null),
    closeTerminal: vi.fn(async () => undefined),
    waitForTerminalExit: vi.fn(async () => undefined),
  };
}

/** The ids a double reduction can disagree on: pane leaves, tab groups and the focused leaf. */
function idShape(state: WorkspaceState, tabId: string) {
  const tabLayout = state.layout.layoutsByTabId[tabId];
  return {
    leaves: tabLayout ? collectLeafIds(tabLayout.root).slice().sort() : [],
    groups: Object.keys(state.layout.tabGroups ?? {}).slice().sort(),
    focusedGroupId: state.layout.focusedGroupId ?? null,
    activeLeafId: tabLayout?.activeLeafId ?? null,
  };
}

function renderStrictStore() {
  return renderHook(
    () => useWorkspaceStore({ workspaceId: WORKSPACE_ID, initialWorktrees: [worktree], services: services() }),
    { wrapper: StrictMode },
  );
}

describe("deterministic dispatch id minting", () => {
  it("keeps the caller-minted SPLIT_PANE leaf identical across React state, the workspace snapshot and the HMR stash", () => {
    const { result } = renderStrictStore();
    act(() => result.current.restoreWorkspace(twoGroupState()));

    const session = terminalSession("session-split");
    act(() =>
      result.current.dispatchWorkspaceAction({
        type: "SPLIT_PANE",
        tabId: TAB_A,
        targetLeafId: SHARED_LEAF,
        direction: "horizontal",
        newLeafId: "leaf-presplit",
        session,
      }),
    );

    const react = idShape(result.current.state, TAB_A);
    const snapshot = getWorkspaceSnapshot(WORKSPACE_ID);
    const hmr = getHmrWorkspaceState(WORKSPACE_ID);

    expect(react.leaves).toEqual(["leaf-presplit", SHARED_LEAF]);
    expect(snapshot).not.toBeNull();
    expect(hmr).not.toBeNull();
    expect(idShape(snapshot!, TAB_A)).toEqual(react);
    expect(idShape(hmr!, TAB_A)).toEqual(react);
  });

  it("mints one ADD_TAB_WITH_SESSION leaf shared by React state, the workspace snapshot and the HMR stash", () => {
    const { result } = renderStrictStore();
    act(() => result.current.restoreWorkspace(twoGroupState()));

    const newTab: TerminalTab = { kind: "terminal", id: "tab-det-c", label: "c", sessionId: "session-c" };
    act(() =>
      result.current.dispatchWorkspaceAction({
        type: "ADD_TAB_WITH_SESSION",
        tab: newTab,
        session: terminalSession("session-c"),
      }),
    );

    const react = idShape(result.current.state, newTab.id);
    // The reducer minted this leaf; a double reduction minting twice would leave React on a leaf
    // the snapshot never saw.
    expect(react.leaves).toHaveLength(1);
    expect(react.leaves[0]).toMatch(/^leaf:/);

    const snapshot = getWorkspaceSnapshot(WORKSPACE_ID);
    const hmr = getHmrWorkspaceState(WORKSPACE_ID);
    expect(idShape(snapshot!, newTab.id)).toEqual(react);
    expect(idShape(hmr!, newTab.id)).toEqual(react);
  });

  it("mints one MOVE_TAB_TO_SPLIT group shared by React state, the workspace snapshot and the HMR stash", () => {
    const { result } = renderStrictStore();
    act(() => result.current.restoreWorkspace(twoGroupState()));

    act(() => result.current.moveTabToSplit(TAB_B, GROUP_A, "horizontal", "second"));

    const react = idShape(result.current.state, TAB_A);
    // The move replaced GROUP_B with a freshly minted group; a double reduction that minted
    // twice would leave React on a group the snapshot never saw.
    expect(react.groups).toHaveLength(2);
    expect(react.groups).toContain(GROUP_A);
    expect(react.groups).not.toContain(GROUP_B);
    expect(react.focusedGroupId).not.toBeNull();

    const snapshot = getWorkspaceSnapshot(WORKSPACE_ID);
    const hmr = getHmrWorkspaceState(WORKSPACE_ID);
    expect(idShape(snapshot!, TAB_A)).toEqual(react);
    expect(idShape(hmr!, TAB_A)).toEqual(react);
    expect(idShape(snapshot!, TAB_B)).toEqual(idShape(result.current.state, TAB_B));
  });

  it("mints one MOVE_TAB_TO_PANE_SPLIT leaf shared by React state, the workspace snapshot and the HMR stash", () => {
    const { result } = renderStrictStore();
    act(() => result.current.restoreWorkspace(twoGroupState()));

    act(() =>
      result.current.moveTabToSplit(TAB_B, GROUP_A, "horizontal", "second", {
        tabId: TAB_A,
        leafId: SHARED_LEAF,
      }),
    );

    const react = idShape(result.current.state, TAB_A);
    // The source tab's leaf collided with the target's, so the merge had to rename it.
    expect(react.leaves).toHaveLength(2);
    expect(react.leaves.filter((leafId) => leafId === SHARED_LEAF)).toHaveLength(1);
    expect(result.current.state.layout.tabs.map((tab) => tab.id)).toEqual([TAB_A]);

    const snapshot = getWorkspaceSnapshot(WORKSPACE_ID);
    const hmr = getHmrWorkspaceState(WORKSPACE_ID);
    expect(idShape(snapshot!, TAB_A)).toEqual(react);
    expect(idShape(hmr!, TAB_A)).toEqual(react);
  });

  it("focuses, in the same tick, the leaf a MOVE_TAB_TO_PANE_SPLIT reported through the snapshot", () => {
    const { result } = renderStrictStore();
    act(() => result.current.restoreWorkspace(twoGroupState()));

    let movedLeafId = "";
    act(() => {
      result.current.moveTabToSplit(TAB_B, GROUP_A, "horizontal", "second", {
        tabId: TAB_A,
        leafId: SHARED_LEAF,
      });
      // Park focus on the surviving original leaf, then read the minted id the way a caller
      // outside React does: from the snapshot cache, not from the hook's state.
      result.current.focusPane(TAB_A, SHARED_LEAF);
      movedLeafId = collectLeafIds(getWorkspaceSnapshot(WORKSPACE_ID)!.layout.layoutsByTabId[TAB_A].root)
        .find((leafId) => leafId !== SHARED_LEAF)!;
      expect(movedLeafId).toBeTruthy();
      result.current.focusPane(TAB_A, movedLeafId);
    });

    // React's committed state must honour the id the snapshot named; if the two reductions had
    // minted different ids this FOCUS_PANE would be a no-op and focus would stay on SHARED_LEAF.
    expect(result.current.state.layout.layoutsByTabId[TAB_A].activeLeafId).toBe(movedLeafId);
    expect(result.current.state.layout.layoutsByTabId[TAB_A].activeLeafId).not.toBe(SHARED_LEAF);
  });

  it("keeps minting unique ids when the reducers are called directly without pre-minted fields", () => {
    const state = twoGroupState();

    const addTab = (tabId: string): WorkspaceAction => ({
      type: "ADD_TAB_WITH_SESSION",
      tab: { kind: "terminal", id: tabId, label: tabId, sessionId: `session-${tabId}` },
      session: terminalSession(`session-${tabId}`),
    });
    const firstLeaf = workspaceReducer(state, addTab("tab-det-c")).layout.layoutsByTabId["tab-det-c"].root;
    const secondLeaf = workspaceReducer(state, addTab("tab-det-c")).layout.layoutsByTabId["tab-det-c"].root;
    expect(firstLeaf.type).toBe("leaf");
    expect(secondLeaf.type).toBe("leaf");
    const firstLeafId = firstLeaf.type === "leaf" ? firstLeaf.leafId : "";
    const secondLeafId = secondLeaf.type === "leaf" ? secondLeaf.leafId : "";
    expect(firstLeafId).toMatch(/^leaf:/);
    expect(secondLeafId).toMatch(/^leaf:/);
    expect(firstLeafId).not.toBe(secondLeafId);

    const moveToSplit: WorkspaceAction = { type: "MOVE_TAB_TO_SPLIT", sourceTabId: TAB_B, targetGroupId: GROUP_A, direction: "horizontal" };
    const firstGroupId = workspaceReducer(state, moveToSplit).layout.focusedGroupId;
    const secondGroupId = workspaceReducer(state, moveToSplit).layout.focusedGroupId;
    expect(firstGroupId).toMatch(/^group:/);
    expect(secondGroupId).toMatch(/^group:/);
    expect(firstGroupId).not.toBe(secondGroupId);

    const moveToPane: WorkspaceAction = {
      type: "MOVE_TAB_TO_PANE_SPLIT",
      sourceTabId: TAB_B,
      targetTabId: TAB_A,
      targetLeafId: SHARED_LEAF,
      direction: "horizontal",
    };
    const movedLeaves = (action: WorkspaceAction) =>
      collectLeafIds(workspaceReducer(state, action).layout.layoutsByTabId[TAB_A].root).slice().sort();
    const firstMoved = movedLeaves(moveToPane);
    const secondMoved = movedLeaves(moveToPane);
    expect(firstMoved).toHaveLength(2);
    expect(firstMoved).not.toEqual(secondMoved);
    // Every id a direct reduction mints stays unique inside its own tree.
    expect(new Set(firstMoved).size).toBe(firstMoved.length);
  });

  it("stamps pre-minted ids once, so a re-stamped action keeps the ids it already carries", () => {
    const moveToSplit: WorkspaceAction = { type: "MOVE_TAB_TO_SPLIT", sourceTabId: TAB_B, targetGroupId: GROUP_A, direction: "horizontal" };
    const stamped = withPreMintedActionIds(moveToSplit);
    expect(stamped.type).toBe("MOVE_TAB_TO_SPLIT");
    if (stamped.type !== "MOVE_TAB_TO_SPLIT") throw new Error("unreachable");
    expect(stamped.newGroupId).toBeTruthy();
    expect(withPreMintedActionIds(stamped)).toBe(stamped);
  });
});
