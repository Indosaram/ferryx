import type { WorkspaceState } from "../state/workspaceStore";
import type {
  PersistedLayout,
  PersistedTab,
  PersistedTerminalSession,
  PersistedWorkspace,
  PersistedWorkspaceSession,
  PersistedWorktree,
} from "./types";

export function serializeWorkspaceState(
  workspaceId: string,
  repoRoot: string,
  state: WorkspaceState,
): PersistedWorkspaceSession {
  const persistedWorktrees: PersistedWorktree[] = state.worktrees.map((wt) => ({
    path: wt.path,
    branch: wt.branch ?? "",
    head: wt.head,
    isMain: !wt.branch || wt.branch === "main" || wt.branch === "master",
    isLocked: Boolean(wt.locked),
  }));

  const persistedTabs: PersistedTab[] = state.layout.tabs.map((tab) => {
    const tabLayout = state.layout.layoutsByTabId[tab.id];
    const sessionId = tab.kind === "browser" ? tab.browserId : tab.sessionId;
    const worktreePath = tab.kind === "browser" ? "" : (state.sessions[tab.sessionId]?.cwd ?? "");
    return {
      id: tab.id,
      sessionId,
      label: tab.label,
      worktreePath,
      paneTree: tabLayout?.root,
      sessionIdsByLeafId: tabLayout?.sessionIdsByLeafId,
      activeLeafId: tabLayout?.activeLeafId,
    };
  });

  const persistedLayout: PersistedLayout = {
    splitMode: state.layout.split ?? "none",
    primaryTabId: state.layout.primaryTabId ?? null,
    secondaryTabId: state.layout.secondaryTabId ?? null,
    activeTabId: state.layout.activeTabId ?? null,
    tabs: persistedTabs,
  };

  const persistedTerminalSessions: Record<string, PersistedTerminalSession> = {};
  for (const [id, sess] of Object.entries(state.sessions)) {
    if (!sess) continue;
    persistedTerminalSessions[id] = {
      sessionId: sess.id,
      worktreePath: sess.cwd,
      cwd: sess.cwd,
      createdAt: Date.now(),
    };
  }

  const workspace: PersistedWorkspace = {
    workspaceId,
    repoRoot,
    worktrees: persistedWorktrees,
    activeWorktreePath: state.activeWorktreePath,
    layout: persistedLayout,
    terminalSessions: persistedTerminalSessions,
  };

  return {
    version: 1,
    timestamp: Date.now(),
    activeWorkspaceId: workspaceId,
    workspaces: {
      [workspaceId]: workspace,
    },
  };
}
