import type { InventorySnapshot } from "../../../lib/scopedContracts";
import type { Agent } from "./client";
import type { WorkspaceState } from "../../../state/workspaceStore";

export type DesktopWorkspace = { workspaceId: string; hostId: string; state: WorkspaceState };

function getLocalSessionKeyForAgent(agent: Agent, workspace: DesktopWorkspace): string | null {
  const sessions = workspace.state.sessions ?? {};
  return (
    Object.keys(sessions).find(
      (sessionId) => (sessions[sessionId]?.backendSessionId ?? sessionId) === agent.target.backendSessionId,
    ) ?? null
  );
}

/** The workspace-local session key that owns this row, or null when the workspace is unknown. */
export function resolveLocalSessionKey(agent: Agent, workspaces: DesktopWorkspace[]): string | null {
  const workspace = workspaces.find((w) => w.workspaceId === agent.workspaceId);
  if (!workspace) return null;
  return getLocalSessionKeyForAgent(agent, workspace);
}

const STATE_RANK: Record<string, number> = {
  waiting: 0,
  exited: 1,
  done: 2,
};

function stateRank(state: string): number {
  return STATE_RANK[state] ?? 3;
}

/**
 * One Agent row per tracked session.
 * Order: state rank, then label.localeCompare, then workspaceId.localeCompare, then hostId.localeCompare,
 * then target.backendSessionId.localeCompare.
 */
export function buildDesktopInventory(
  workspaces: DesktopWorkspace[],
  unavailableHosts: readonly string[] = [],
): InventorySnapshot<Agent> {
  const items: Agent[] = [];
  for (const workspace of workspaces) {
    const tabLabels = new Map<string, string>();
    for (const tab of workspace.state.layout?.tabs ?? []) {
      const label = tab?.label?.trim();
      if (label && "sessionId" in tab && typeof tab.sessionId === "string") {
        tabLabels.set(tab.sessionId, label);
      }
    }
    const sessions = workspace.state.sessions ?? {};
    for (const [sessionId, session] of Object.entries(sessions)) {
      const activity = workspace.state.activityBySessionId?.[sessionId];
      if (!activity) continue;
      const state = session.lifecycle === "exited" ? "exited" : activity.state;
      items.push({
        target: { hostId: workspace.hostId, ownerId: workspace.workspaceId, epoch: "0", backendSessionId: session.backendSessionId ?? sessionId },
        workspaceId: workspace.workspaceId,
        label: tabLabels.get(sessionId) ?? sessionId,
        state,
        revision: 0,
        source: { kind: "terminalDetection", detector: "agent_detect" },
      });
    }
  }
  items.sort((a, b) => {
    const rankDiff = stateRank(a.state) - stateRank(b.state);
    if (rankDiff !== 0) return rankDiff;
    const labelDiff = a.label.localeCompare(b.label);
    if (labelDiff !== 0) return labelDiff;
    const workspaceDiff = a.workspaceId.localeCompare(b.workspaceId);
    if (workspaceDiff !== 0) return workspaceDiff;
    const hostDiff = a.target.hostId.localeCompare(b.target.hostId);
    if (hostDiff !== 0) return hostDiff;
    return a.target.backendSessionId.localeCompare(b.target.backendSessionId);
  });
  const completeness = unavailableHosts.length > 0 ? "partial" : "complete";
  const finalUnavailableHosts = unavailableHosts.length > 0 ? [...unavailableHosts] : [];
  return { revision: items.length, items: items.map((item, index) => ({ ...item, revision: index + 1 })), completeness, unavailableHosts: finalUnavailableHosts };
}

/** True when the session needs the user: finished and never seen. */
export function isUnreadAgent(agent: Agent, workspaces: DesktopWorkspace[]): boolean {
  const workspace = workspaces.find((w) => w.workspaceId === agent.workspaceId);
  if (!workspace) return false;
  const resolvedKey = getLocalSessionKeyForAgent(agent, workspace);
  if (!resolvedKey) return false;
  const activity = workspace.state.activityBySessionId?.[resolvedKey];
  if (!activity) return false;
  return activity.state === "done" && activity.seen !== true;
}
