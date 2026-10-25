// The remote's activity dots, project slots and footer must come from what the desktop draws,
// through the real desktop-to-remote path: the desktop's seen-aware worktreeActivity (mounted
// workspace over parked snapshots), reduced into the published snapshot, sent as JSON, read by the
// remote next to whatever its own session inventory learned. The three cases are the divergences
// the F4 round-2 probe found when the remote rebuilt activity from its inventory.
import { describe, expect, it } from "vitest";

import { deriveFocusedTerminal } from "../../App";
import { directSidebarInventory } from "../../remote/directSidebarInventory";
import type { RemoteWorkspaceModel } from "../../remote/RemoteSessionList";
import { remoteSidebarSignals } from "../../remote/remoteSidebarLive";
import { applySessionActivity, observationsFromContextTabs, type RemoteSessionInventory } from "../../remote/remoteSessionInventory";
import { selectWorktreeActivitySummaries, type WorkspaceState } from "../../state/workspaceStore";
import { sidebarLiveState, type SidebarModel } from "./sidebarModel";
import { sidebarSessionSignals } from "./sidebarSessionSignals";

type Activity = "working" | "waiting" | "done";

/** One project with one root worktree and one agent session in the given state. */
function workspace(id: string, state: Activity, seen = false): WorkspaceState {
  return {
    workspaceId: id, activeWorktreePath: `/${id}`, unreadTabIds: {}, unreadWorktreePaths: {},
    worktrees: [{ path: `/${id}`, head: "abc", branch: "main", bare: false, detached: false, locked: null, prunable: null }],
    sessions: { [id]: { id, cwd: `/${id}`, workspaceId: id, backendSessionId: `backend-${id}`, worktree: null, lifecycle: "working" } },
    layout: { tabs: [{ id, label: id, sessionId: id }], activeTabId: id, layoutsByTabId: {} },
    activityBySessionId: { [id]: { state, seen, title: "", isAgent: true } },
  } as unknown as WorkspaceState;
}

const node = (id: string) => ({ key: `${id}:/${id}`, workspaceId: id, path: `/${id}`, worktreeSlug: null, label: "main", hostLabel: null, stale: false, listingFailure: null });

const MODEL: SidebarModel = {
  schemaVersion: 1, revision: 1, generatedAt: "", source: "desktop", collapsedGroupIds: [], advancedFeatures: true, emptyState: null,
  projects: ["ferryx", "site"].map((id) => ({
    groupId: id, label: id, hostLabel: null, members: [{ workspaceId: id, kind: "local" as const, machineId: null }],
    primaryWorkspaceId: id, status: null, filterLabels: [id], worktrees: [node(id)], worktreesEmptyHint: null,
  })),
};

/** The session inventory the remote holds for this machine (no activity of its own yet). */
function idleInventory(): RemoteSessionInventory {
  return {
    machineId: "m", revision: "1", completeness: "complete", unavailableWorkspaceIds: [], fetchedAt: 0,
    entries: ["ferryx", "site"].map((id) => ({
      machineId: "m", workspaceId: id, worktreeSlug: null, worktreeLabel: null, sessionId: `backend-${id}`, daemonEpoch: null,
      title: null, running: true, agentType: null, providerSessionId: null, activityState: null,
    })),
  };
}

/** worktreeActivity as useWorkspaceStore builds it: parked snapshots, overlaid by the mounted workspace. */
function desktopActivity(mounted: WorkspaceState, parked: readonly WorkspaceState[] = []) {
  return Object.assign({}, ...parked.map(selectWorktreeActivitySummaries), selectWorktreeActivitySummaries(mounted));
}

/** What the desktop Sidebar draws: the raw activity plus the reduced signals it also publishes. */
function desktopLive(activity: ReturnType<typeof desktopActivity>) {
  return sidebarLiveState(MODEL, { activityByWorktreePath: activity, ...sidebarSessionSignals(MODEL, { activityByWorktreePath: activity }) }, "");
}

/** What the remote draws: the published snapshot after the gateway's JSON round trip, plus its inventory. */
function remoteLive(activity: ReturnType<typeof desktopActivity>, inventory: RemoteSessionInventory | null) {
  const published = JSON.parse(JSON.stringify({ ...MODEL, signals: sidebarSessionSignals(MODEL, { activityByWorktreePath: activity }) })) as SidebarModel;
  return sidebarLiveState(published, remoteSidebarSignals(published, inventory, null), "");
}

describe("published desktop activity drives the remote sidebar", () => {
  it("shows a waiting agent in a parked project the same on both apps", () => {
    const mounted = workspace("ferryx", "working");
    const parked = workspace("site", "waiting");
    const activity = desktopActivity(mounted, [parked]);
    // The desktop's focused-terminal publish carries only the mounted workspace's tabs.
    const inventory = applySessionActivity(idleInventory(), observationsFromContextTabs(deriveFocusedTerminal("ferryx", mounted, [["site", parked]])?.terminalTabs));

    const desktop = desktopLive(activity);
    expect(desktop.rows["site:/site"]?.slot).toBe("waiting");
    expect(desktop.footer).toBe("1 running · 1 needs input");
    expect(remoteLive(activity, inventory)).toEqual(desktop);
  });

  it("clears a done agent on the remote once the desktop user has seen it", () => {
    const before = workspace("ferryx", "done");
    const after = workspace("ferryx", "done", true);
    let inventory = applySessionActivity(idleInventory(), observationsFromContextTabs(deriveFocusedTerminal("ferryx", before)?.terminalTabs));
    inventory = applySessionActivity(inventory, observationsFromContextTabs(deriveFocusedTerminal("ferryx", after)?.terminalTabs));
    const activity = desktopActivity(after);

    const desktop = desktopLive(activity);
    expect(desktop.rows["ferryx:/ferryx"]?.slot).toBeNull();
    expect(desktop.projects.ferryx).toEqual({ kind: "none" });
    expect(remoteLive(activity, inventory)).toEqual(desktop);
  });

  it("counts one working agent once in direct mode", () => {
    const mounted = workspace("ferryx", "working");
    const activity = desktopActivity(mounted);
    // A direct pairing declares the same session twice: as the option rollup and as the pane tab.
    const direct = directSidebarInventory({
      context: { workspaceId: "ferryx", worktreeSlug: null, worktreeLabel: null, terminalTabs: [{ id: "t", label: "t", sessionId: "backend-ferryx", activityState: "working" }] },
      options: [{ workspaceId: "ferryx", worktreeSlug: null, worktreeLabel: null, attention: "working" }],
    } as unknown as RemoteWorkspaceModel);

    const desktop = desktopLive(activity);
    expect(desktop.projects.ferryx).toEqual({ kind: "working", state: "working", testId: "project-running-badge" });
    expect(remoteLive(activity, direct)).toEqual(desktop);
  });

  it("still rebuilds activity from the inventory for a snapshot that publishes none", () => {
    const legacy: SidebarModel = { ...MODEL, signals: { unreadWorktreePaths: {}, agentStateByPath: {} } };
    const inventory = { ...idleInventory(), entries: idleInventory().entries.map((entry) => ({ ...entry, activityState: "waiting" as const })) };

    const live = sidebarLiveState(legacy, remoteSidebarSignals(legacy, inventory, null), "");

    expect(live.rows["site:/site"]?.slot).toBe("waiting");
    expect(live.footer).toBe("2 needs input");
  });
});
