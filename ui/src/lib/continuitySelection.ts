import type { CommittedDesktopSelection } from "./useDesktopContinuity";
import { isStandbyBackendSessionId } from "./sessionLifecycle";
import type { FocusedTerminalPayload, RegisteredProject } from "./tauri";
import { isTerminalTab } from "./types";
import type { WorkspaceState } from "../state/workspaceStore";
import { collectLeafIds } from "../state/paneTree";

/**
 * Labels the selection with `state.workspaceId` so a selection can never describe one workspace's
 * tab and session under another workspace's id. `useWorkspaceStore` swaps its snapshot during the
 * same render that receives the new `workspaceId`, so the ids normally agree; the mismatch branch
 * is defensive (mid-swap state, a stale store double, a workspace with no snapshot yet) and reports
 * nothing publishable rather than guessing.
 */
export function deriveContinuitySelection(
  state: WorkspaceState,
  activeProject: RegisteredProject,
  focusedTerminalPayload: FocusedTerminalPayload | null,
): CommittedDesktopSelection {
  const workspaceId = state.workspaceId ?? activeProject.workspaceId;
  if (workspaceId !== activeProject.workspaceId) {
    return { workspaceId, tabId: null, leafId: null, worktreePath: null,
      worktreeSlug: null, sessionId: null, local: false };
  }
  const group = state.layout.focusedGroupId ? state.layout.tabGroups?.[state.layout.focusedGroupId] : undefined;
  const tabId = group?.activeTabId ?? state.layout.activeTabId ?? state.layout.tabs[0]?.id ?? null;
  const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
  const layout = tabId ? state.layout.layoutsByTabId?.[tabId] : undefined;
  const leafId = layout?.activeLeafId ?? (layout?.root ? collectLeafIds(layout.root)[0] : null);
  const sessionId = tab && isTerminalTab(tab)
    ? (leafId && layout?.sessionIdsByLeafId?.[leafId]) || tab.sessionId : null;
  const session = sessionId ? state.sessions[sessionId] : undefined;
  return {
    workspaceId,
    tabId,
    leafId,
    worktreePath: session?.worktreePath ?? session?.cwd ?? null,
    worktreeSlug: focusedTerminalPayload?.worktreeSlug ?? null,
    sessionId: session?.backendSessionId && !isStandbyBackendSessionId(session.backendSessionId)
      ? session.backendSessionId : null,
    local: Boolean(tab && isTerminalTab(tab)) && (!activeProject.target || activeProject.target.kind === "local"),
  };
}
