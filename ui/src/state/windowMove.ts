import { localSplitIntent } from "../lib/localSplitLifecycle";
import { isPairedWorkspaceId, isRemoteWorkspaceId } from "../lib/remoteProject";
import type { WindowMoveOutcome } from "../lib/tauri";
import { isTerminalTab } from "../lib/types";
import type { WorkspaceState } from "./workspaceStore";

export type TabMovePlan =
  | { ok: true; backendSessionIds: string[] }
  | { ok: false; reason: string };

/**
 * Which backend sessions a tab hands to another window. Only a tab made of live local terminal
 * panes can move: browser/file/changes panes and remote or paired workspaces have no window-owned
 * native surface to transfer, and a pane that has no backend session yet (or a split still being
 * created) has nothing the target could attach to.
 */
export function planTabMove(state: WorkspaceState, tabId: string): TabMovePlan {
  const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab || !isTerminalTab(tab)) return { ok: false, reason: "Only terminal tabs can move to another window." };
  if (isRemoteWorkspaceId(state.workspaceId) || isPairedWorkspaceId(state.workspaceId)) {
    return { ok: false, reason: "Remote terminals cannot move to another window." };
  }
  const tabLayout = state.layout.layoutsByTabId?.[tabId];
  const localIds = new Set<string>();
  if (tabLayout) {
    for (const [leafId, sessionId] of Object.entries(tabLayout.sessionIdsByLeafId)) {
      const content = tabLayout.contentsByLeafId?.[leafId];
      if (content && content.kind !== "terminal") {
        return { ok: false, reason: "This tab has non-terminal panes and cannot move to another window." };
      }
      if (sessionId) localIds.add(sessionId);
    }
  }
  if (localIds.size === 0) localIds.add(tab.sessionId);
  const backendSessionIds: string[] = [];
  for (const localId of localIds) {
    const session = state.sessions[localId];
    if (!session?.backendSessionId || localSplitIntent(session)) {
      return { ok: false, reason: "This terminal is still starting; try again in a moment." };
    }
    if (!backendSessionIds.includes(session.backendSessionId)) backendSessionIds.push(session.backendSessionId);
  }
  return { ok: true, backendSessionIds };
}

/** The source drops its tab only when the backend handed over every one of its sessions. */
export function movedEverySession(outcome: Pick<WindowMoveOutcome, "moved">, backendSessionIds: readonly string[]): boolean {
  const moved = new Set(outcome.moved);
  return backendSessionIds.every((id) => moved.has(id));
}
