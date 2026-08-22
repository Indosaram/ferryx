import { PanelLeft } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { AddProjectDialog, AddWorktreeDialog } from "./components/ProjectDialogs";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { IconButton } from "./components/ui/IconButton";
import { deserializeWorkspaceState, serializeWorkspaceState } from "./lib/sessionPersistence";
import { isMacShortcutPlatform, useShortcuts } from "./lib/shortcuts";
import {
  ACTIVE_PROJECT_STORAGE_KEY,
  getMigratedItem,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} from "./lib/storageKeys";
import {
  DEFAULT_WORKSPACE_ID,
  getWorktreeStatus,
  loadSession,
  onNewTerminalTabMenu,
  registerProject,
  saveSession,
  spawnTerminal,
  type RegisteredProject,
} from "./lib/tauri";
import { ensureTerminalEvents } from "./lib/terminalEvents";
import { useTerminalSettings } from "./lib/terminalSettings";
import { defaultTauriTransport } from "./lib/terminalTransport/tauriTransport";
import { worktreeIdentity, type DirtyState, type WorkspaceTab, type Worktree } from "./lib/types";
import { registerWindowCloseGuard } from "./lib/updater";
import { collectLeafIds, type PaneDirection } from "./state/paneTree";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { useWorkspaceStore } from "./state/workspaceStore";

export { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY };
const DEFAULT_PROJECT: RegisteredProject = { workspaceId: DEFAULT_WORKSPACE_ID, repoRoot: "." };

export function App() {
  const [projects, setProjects] = useState<RegisteredProject[]>(loadProjects);
  const [activeProjectId, setActiveProjectId] = useState(loadActiveProjectId);
  const activeProject = useMemo(
    () => projects.find((project) => project.workspaceId === activeProjectId) ?? projects[0] ?? DEFAULT_PROJECT,
    [activeProjectId, projects],
  );

  const {
    state,
    agents,
    tabActivity,
    worktreeActivity,
    updateSessionTitleActivity,
    openTab,
    createBrowserTab,
    navigateBrowserTab,
    reloadBrowserTab,
    ensureTabForWorktree,
    closeTab,
    closeOtherTabs,
    closeTabsToRight,
    closeTabsToLeft,
    splitPane,
    moveTabToGroup,
    moveTabToSplit,
    detachPaneToTab,
    closePane,
    activateTab,
    reorderTab,
    renameTab,
    setTabPinned,
    focusPane,
    setPaneRatio,
    setTabGroupRatio,
    swapPanes,
    syncWorktrees,
    restoreWorkspace,
  } = useWorkspaceStore({ workspaceId: activeProject.workspaceId });
  const { runtimeError, refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({
    workspaceId: activeProject.workspaceId,
    activeWorktreePath: state.activeWorktreePath,
    syncWorktrees,
    ensureTabForWorktree,
  });

  useEffect(() => {
    void registerProject({
      workspaceId: activeProject.workspaceId,
      repoPath: activeProject.repoRoot,
    })
      .then(() => refreshWorktrees())
      .catch(reportRuntimeError);
  }, [activeProject.repoRoot, activeProject.workspaceId, refreshWorktrees, reportRuntimeError]);

  // Initial session restore on startup & HMR recovery.
  useEffect(() => {
    let cancelled = false;
    async function restore() {
      try {
        const session = await loadSession();
        if (cancelled || !session) return;

        const liveSessions = await defaultTauriTransport.listSessions().catch(() => []);
        const liveBackendIds = new Set(liveSessions.map((candidate) => candidate.sessionId));
        const restoredState = deserializeWorkspaceState(activeProject.workspaceId, session, liveBackendIds);

        if (restoredState && restoredState.layout.tabs.length > 0) {
          restoreWorkspace(restoredState);

          // A persisted local session can outlive its native PTY. Respawn only the dead
          // native side while preserving the local session/leaf ownership graph.
          const deadSessions = Object.values(restoredState.sessions).filter((candidate) => !candidate.backendSessionId);
          if (deadSessions.length > 0) {
            let hasSpawned = false;
            await ensureTerminalEvents().catch(() => undefined);
            for (const restoredSession of deadSessions) {
              if (cancelled) return;
              try {
                const worktreePath = restoredSession.worktreePath ?? restoredSession.cwd;
                const foundWorktree = restoredState.worktrees.find((worktree) => worktree.path === worktreePath);
                const backendSessionId = await spawnTerminal({
                  workspaceId: activeProject.workspaceId,
                  worktree: foundWorktree ? worktreeIdentity(foundWorktree) : null,
                  cwd: restoredSession.cwd,
                });
                restoredSession.backendSessionId = backendSessionId;
                restoredSession.lifecycle = "working";
                hasSpawned = true;
              } catch (error) {
                console.warn("Failed to respawn terminal session on restore:", error);
              }
            }

            if (hasSpawned && !cancelled) restoreWorkspace({ ...restoredState });
          }
          // Typed v2 restoration already reconstructed every tab. Do not run the legacy
          // per-worktree fallback as well or it can create duplicate terminal tabs.
          return;
        }

        // Legacy best-effort fallback for sessions that cannot be deserialized as a
        // workspace state. Browser records are skipped because they require explicit v2
        // browser metadata and should never be guessed from terminal fields.
        const workspace = session.workspaces?.[activeProject.workspaceId];
        if (!workspace || !workspace.layout?.tabs?.length) return;
        for (const persistedTab of workspace.layout.tabs) {
          if (cancelled) return;
          if (persistedTab.kind === "browser") continue;
          const legacyWorktreePath = persistedTab.worktreePath || workspace.repoRoot;
          const found = workspace.worktrees?.find((candidate) => candidate.path === legacyWorktreePath);
          const worktree: Worktree = found
            ? {
                path: found.path,
                head: found.head,
                branch: found.branch,
                bare: false,
                detached: false,
                locked: found.isLocked ? "locked" : null,
                prunable: null,
              }
            : {
                path: legacyWorktreePath,
                head: "",
                branch: persistedTab.label,
                bare: false,
                detached: false,
                locked: null,
                prunable: null,
              };
          try {
            const alreadyOpen = state.layout.tabs.some((tab) => {
              if (tab.kind === "browser") return false;
              const localSession = state.sessions[tab.sessionId];
              return (localSession?.worktreePath ?? localSession?.cwd) === worktree.path;
            });
            if (!alreadyOpen) await ensureTabForWorktree(worktree);
          } catch {
            // Safe fallback if the terminal for this worktree is already open.
          }
        }
      } catch (error) {
        console.warn("Session restore on boot skipped:", error);
      }
    }
    void restore();
    return () => {
      cancelled = true;
    };
  }, [activeProject.workspaceId, ensureTabForWorktree, restoreWorkspace, state.layout.tabs, state.sessions]);

  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      const session = serializeWorkspaceState(activeProject.workspaceId, activeProject.repoRoot, state);
      await saveSession(session);
    });
    return unregister;
  }, [activeProject.repoRoot, activeProject.workspaceId, state]);

  useEffect(() => {
    if (state.worktrees.length === 0 || state.layout.tabs.length === 0) return;
    const timer = setTimeout(() => {
      const session = serializeWorkspaceState(activeProject.workspaceId, activeProject.repoRoot, state);
      void Promise.resolve(saveSession(session)).catch((error) => console.error("Failed to auto-save session:", error));
    }, 500);
    return () => clearTimeout(timer);
  }, [activeProject.repoRoot, activeProject.workspaceId, state]);

  const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [searchLeafId, setSearchLeafId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpen);
  const [deleteTarget, setDeleteTarget] = useState<Worktree | null>(null);
  const [worktreeStatuses, setWorktreeStatuses] = useState<Record<string, DirtyState | undefined>>({});
  const [pendingWorktreePath, setPendingWorktreePath] = useState<string | null>(null);

  const toggleSidebar = useCallback(() => {
    setIsSidebarOpen((current) => {
      const next = !current;
      persistSidebarOpen(next);
      return next;
    });
  }, []);

  const activeWorktree = useMemo(
    () => state.worktrees.find((worktree) => worktree.path === state.activeWorktreePath) ?? null,
    [state.activeWorktreePath, state.worktrees],
  );

  const handleSelectProject = useCallback(
    (project: RegisteredProject) => {
      if (project.workspaceId === activeProject.workspaceId) return;
      setActiveProjectId(project.workspaceId);
      persistActiveProjectId(project.workspaceId);
      setWorktreeStatuses({});
      setDeleteTarget(null);
      setPendingWorktreePath(null);
    },
    [activeProject.workspaceId],
  );

  const handleRegisteredProject = useCallback((project: RegisteredProject) => {
    setProjects((current) => {
      const next = [...current.filter((candidate) => candidate.workspaceId !== project.workspaceId), project];
      persistProjects(next);
      return next;
    });
    setActiveProjectId(project.workspaceId);
    persistActiveProjectId(project.workspaceId);
    setWorktreeStatuses({});
  }, []);

  const handleSelectWorktree = useCallback(
    (worktree: Worktree) => {
      const ownerId = worktreeIdentity(worktree)?.wsId;
      const owner = ownerId ? projects.find((project) => project.workspaceId === ownerId) : undefined;
      if (owner && owner.workspaceId !== activeProject.workspaceId) {
        handleSelectProject(owner);
        setPendingWorktreePath(worktree.path);
        return;
      }
      void ensureTabForWorktree(worktree).catch(reportRuntimeError);
    },
    [activeProject.workspaceId, ensureTabForWorktree, handleSelectProject, projects, reportRuntimeError],
  );

  useEffect(() => {
    if (!pendingWorktreePath) return;
    const target = state.worktrees.find((worktree) => worktree.path === pendingWorktreePath);
    if (!target) return;
    setPendingWorktreePath(null);
    void ensureTabForWorktree(target).catch(reportRuntimeError);
  }, [ensureTabForWorktree, pendingWorktreePath, reportRuntimeError, state.worktrees]);

  const handleSelectTerminalTab = useCallback(
    (tabId: string) => {
      const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab) return;
      if (tab.kind === "browser") {
        activateTab(tabId);
        return;
      }
      const session = state.sessions[tab.sessionId];
      const sessionWorktreePath = session?.worktreePath ?? session?.cwd;
      const worktree = sessionWorktreePath
        ? state.worktrees.find((candidate) => candidate.path === sessionWorktreePath)
        : undefined;
      if (!worktree || worktree.path === state.activeWorktreePath) {
        activateTab(tabId);
        return;
      }
      void ensureTabForWorktree(worktree)
        .then(() => activateTab(tabId))
        .catch(reportRuntimeError);
    },
    [activateTab, ensureTabForWorktree, reportRuntimeError, state.activeWorktreePath, state.layout.tabs, state.sessions, state.worktrees],
  );

  const handleRefreshWorktreeStatus = useCallback(
    (worktree: Worktree) => {
      const identity = worktreeIdentity(worktree);
      if (!identity) return;
      void getWorktreeStatus({ workspaceId: activeProject.workspaceId, worktree: identity })
        .then((status) => {
          setWorktreeStatuses((current) => ({ ...current, [worktree.path]: status }));
        })
        .catch(reportRuntimeError);
    },
    [activeProject.workspaceId, reportRuntimeError],
  );

  const handleAddTerminalTab = useCallback(() => {
    if (!activeWorktree) return;
    void openTab(activeWorktree).catch(reportRuntimeError);
  }, [activeWorktree, openTab, reportRuntimeError]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onNewTerminalTabMenu(handleAddTerminalTab).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleAddTerminalTab]);

  const handleCloseTab = useCallback(
    (tabId: string) => {
      void closeTab(tabId).catch(reportRuntimeError);
    },
    [closeTab, reportRuntimeError],
  );

  const handleCloseOtherTabs = useCallback(
    (tabId: string) => {
      void closeOtherTabs(tabId).catch(reportRuntimeError);
    },
    [closeOtherTabs, reportRuntimeError],
  );

  const handleCloseTabsToRight = useCallback(
    (tabId: string) => {
      void closeTabsToRight(tabId).catch(reportRuntimeError);
    },
    [closeTabsToRight, reportRuntimeError],
  );

  const handleCloseTabsToLeft = useCallback(
    (tabId: string) => {
      void closeTabsToLeft(tabId).catch(reportRuntimeError);
    },
    [closeTabsToLeft, reportRuntimeError],
  );

  const handleCycleTab = useCallback(
    (offset: number) => {
      const focusedGroup = state.layout.focusedGroupId
        ? state.layout.tabGroups?.[state.layout.focusedGroupId]
        : undefined;
      const tabById = new Map(state.layout.tabs.map((tab) => [tab.id, tab]));
      const tabs: WorkspaceTab[] = focusedGroup
        ? focusedGroup.tabIds.map((tabId) => tabById.get(tabId)).filter((tab): tab is WorkspaceTab => Boolean(tab))
        : state.layout.tabs;
      if (tabs.length < 2) return;
      const currentIndex = Math.max(0, tabs.findIndex((tab) => tab.id === state.layout.activeTabId));
      const nextIndex = (currentIndex + offset + tabs.length) % tabs.length;
      handleSelectTerminalTab(tabs[nextIndex].id);
    },
    [handleSelectTerminalTab, state.layout.activeTabId, state.layout.focusedGroupId, state.layout.tabGroups, state.layout.tabs],
  );

  const handleSplitActive = useCallback(
    (direction: PaneDirection) => {
      const activeTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? state.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") return;
      const activeLayout = state.layout.layoutsByTabId?.[activeTab.id];
      const targetLeafId = activeLayout?.activeLeafId ?? "leaf-default";
      void splitPane(activeTab.id, targetLeafId, direction).catch(reportRuntimeError);
    },
    [reportRuntimeError, splitPane, state.layout.activeTabId, state.layout.layoutsByTabId, state.layout.tabs],
  );

  const handleUnsplitActive = useCallback(() => {
    const activeTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? state.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = state.layout.layoutsByTabId?.[activeTab.id];
    if (!activeLayout || activeLayout.root.type === "leaf") return;
    const activeLeafId = activeLayout.activeLeafId ?? "leaf-default";
    void closePane(activeTab.id, activeLeafId).catch(reportRuntimeError);
  }, [closePane, reportRuntimeError, state.layout.activeTabId, state.layout.layoutsByTabId, state.layout.tabs]);

  const handleCyclePaneFocus = useCallback(
    (offset: number) => {
      const activeTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? state.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") return;
      const activeLayout = state.layout.layoutsByTabId?.[activeTab.id];
      if (!activeLayout) return;
      const leafIds = collectLeafIds(activeLayout.root);
      if (leafIds.length < 2) return;
      const activeLeafId = activeLayout.activeLeafId ?? leafIds[0];
      const currentIndex = Math.max(0, leafIds.indexOf(activeLeafId));
      const nextIndex = (currentIndex + offset + leafIds.length) % leafIds.length;
      focusPane(activeTab.id, leafIds[nextIndex]);
    },
    [focusPane, state.layout.activeTabId, state.layout.layoutsByTabId, state.layout.tabs],
  );

  const handleOpenTerminalSearch = useCallback(() => {
    const activeTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? state.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = state.layout.layoutsByTabId?.[activeTab.id];
    const leafId = activeLayout?.activeLeafId ?? (activeLayout?.root ? collectLeafIds(activeLayout.root)[0] : "leaf-default");
    setSearchLeafId(leafId);
  }, [state.layout.activeTabId, state.layout.layoutsByTabId, state.layout.tabs]);

  const handleSelectWorktreeByIndex = useCallback(
    (index: number) => {
      const visible = listVisibleWorktrees(projects, state.worktrees, activeProject.workspaceId);
      const target = visible[index];
      if (target) handleSelectWorktree(target);
    },
    [activeProject.workspaceId, handleSelectWorktree, projects, state.worktrees],
  );

  const { settings: terminalSettings, updateSettings: updateTerminalSettings } = useTerminalSettings();

  const handleZoomIn = useCallback(() => {
    const nextSize = Math.min(36, terminalSettings.fontSize + 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [terminalSettings.fontSize, updateTerminalSettings]);

  const handleZoomOut = useCallback(() => {
    const nextSize = Math.max(10, terminalSettings.fontSize - 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [terminalSettings.fontSize, updateTerminalSettings]);

  const handleZoomReset = useCallback(() => {
    updateTerminalSettings({ fontSize: null });
  }, [updateTerminalSettings]);

  const handleSelectTerminalTabByIndex = useCallback(
    (index: number) => {
      const focusedGroup = state.layout.focusedGroupId
        ? state.layout.tabGroups?.[state.layout.focusedGroupId]
        : undefined;
      const tabId = focusedGroup?.tabIds[index] ?? state.layout.tabs[index]?.id;
      if (tabId) handleSelectTerminalTab(tabId);
    },
    [handleSelectTerminalTab, state.layout.focusedGroupId, state.layout.tabGroups, state.layout.tabs],
  );

  const shortcutHandlers = useMemo(
    () => ({
      "tab.newTerminal": handleAddTerminalTab,
      "tab.newBrowser": () => void createBrowserTab("http://localhost:3000").catch(reportRuntimeError),
      "tab.close": () => {
        if (state.layout.activeTabId) handleCloseTab(state.layout.activeTabId);
      },
      "tab.next": () => handleCycleTab(1),
      "tab.previous": () => handleCycleTab(-1),
      "tab.select1": () => handleSelectTerminalTabByIndex(0),
      "tab.select2": () => handleSelectTerminalTabByIndex(1),
      "tab.select3": () => handleSelectTerminalTabByIndex(2),
      "tab.select4": () => handleSelectTerminalTabByIndex(3),
      "tab.select5": () => handleSelectTerminalTabByIndex(4),
      "tab.select6": () => handleSelectTerminalTabByIndex(5),
      "tab.select7": () => handleSelectTerminalTabByIndex(6),
      "tab.select8": () => handleSelectTerminalTabByIndex(7),
      "tab.select9": () => handleSelectTerminalTabByIndex(8),
      "workspace.select1": () => handleSelectWorktreeByIndex(0),
      "workspace.select2": () => handleSelectWorktreeByIndex(1),
      "workspace.select3": () => handleSelectWorktreeByIndex(2),
      "workspace.select4": () => handleSelectWorktreeByIndex(3),
      "workspace.select5": () => handleSelectWorktreeByIndex(4),
      "workspace.select6": () => handleSelectWorktreeByIndex(5),
      "workspace.select7": () => handleSelectWorktreeByIndex(6),
      "workspace.select8": () => handleSelectWorktreeByIndex(7),
      "workspace.select9": () => handleSelectWorktreeByIndex(8),
      "terminal.splitRight": () => handleSplitActive("horizontal"),
      "terminal.splitDown": () => handleSplitActive("vertical"),
      "terminal.unsplit": handleUnsplitActive,
      "terminal.focusNext": () => handleCyclePaneFocus(1),
      "terminal.focusPrevious": () => handleCyclePaneFocus(-1),
      "terminal.search": handleOpenTerminalSearch,
      "sidebar.left.toggle": toggleSidebar,
      "commandPalette.open": () => setIsCommandPaletteOpen(true),
      "settings.toggle": () => setIsSettingsOpen((current) => !current),
      "zoom.in": handleZoomIn,
      "zoom.out": handleZoomOut,
      "zoom.reset": handleZoomReset,
    }),
    [
      createBrowserTab,
      handleAddTerminalTab,
      handleCloseTab,
      handleCyclePaneFocus,
      handleCycleTab,
      handleOpenTerminalSearch,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleUnsplitActive,
      handleZoomIn,
      handleZoomOut,
      handleZoomReset,
      reportRuntimeError,
      state.layout.activeTabId,
      toggleSidebar,
    ],
  );
  useShortcuts(shortcutHandlers);

  return (
    <div className="flex h-screen w-screen select-none overflow-hidden bg-background font-sans text-foreground">
      {isSidebarOpen ? (
        <Sidebar
          open={true}
          projects={projects}
          activeProjectId={activeProject.workspaceId}
          worktrees={state.worktrees}
          agents={agents}
          activePath={activeWorktree?.path || ""}
          statuses={worktreeStatuses}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          onSelectProject={handleSelectProject}
          onAddProject={() => setIsAddProjectOpen(true)}
          onSelectWorktree={handleSelectWorktree}
          onCreateWorktree={() => setIsCreateOpen(true)}
          onRefreshWorktreeStatus={handleRefreshWorktreeStatus}
          onDeleteWorktree={setDeleteTarget}
          onOpenCommandPalette={() => setIsCommandPaletteOpen(true)}
          onOpenSettings={() => setIsSettingsOpen(true)}
          onToggle={toggleSidebar}
        />
      ) : (
        <div className="relative w-0 shrink-0 overflow-visible">
          <div className="titlebar-left-floating absolute top-0 left-0 z-20 flex h-titlebar shrink-0 items-center border-b border-r border-border bg-card px-2">
            {isMacShortcutPlatform() ? (
              <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" />
            ) : null}
            <IconButton label="Show sidebar" className="no-drag" size="sm" onClick={toggleSidebar}>
              <PanelLeft className="size-3.5" />
            </IconButton>
          </div>
        </div>
      )}

      <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
        {activeWorktree ? (
          <TerminalSplitView
            layout={state.layout}
            sessions={state.sessions}
            unreadTabIds={state.unreadTabIds}
            activityByTabId={tabActivity}
            onTitleChange={(tabId, title, sessionId) => updateSessionTitleActivity(tabId, title, sessionId)}
            onActivateTab={handleSelectTerminalTab}
            onCloseTab={handleCloseTab}
            onCloseOtherTabs={handleCloseOtherTabs}
            onCloseTabsToRight={handleCloseTabsToRight}
            onCloseTabsToLeft={handleCloseTabsToLeft}
            onReorderTab={reorderTab}
            onMoveTabToGroup={moveTabToGroup}
            onMoveTabToSplit={moveTabToSplit}
            onDetachPaneToTab={detachPaneToTab}
            onRenameTab={renameTab}
            onToggleTabPin={setTabPinned}
            onAddTab={handleAddTerminalTab}
            onAddBrowserTab={(url) => void createBrowserTab(url ?? "http://localhost:3000").catch(reportRuntimeError)}
            onNavigateBrowserTab={(tabId, url) => void navigateBrowserTab(tabId, url).catch(reportRuntimeError)}
            onReloadBrowserTab={(tabId) => void reloadBrowserTab(tabId).catch(reportRuntimeError)}
            onSplitPane={(tabId, leafId, direction, options) => splitPane(tabId, leafId, direction, options).catch(reportRuntimeError)}
            onClosePane={(tabId, leafId) => closePane(tabId, leafId).catch(reportRuntimeError)}
            onSetRatio={setPaneRatio}
            onSetGroupRatio={setTabGroupRatio}
            onSwapPanes={swapPanes}
            onFocusPane={focusPane}
            searchLeafId={searchLeafId}
            onCloseSearch={() => setSearchLeafId(null)}
            leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}
          />
        ) : (
          <div className="flex h-full flex-1 items-center justify-center bg-[#23262d] text-xs text-muted-foreground">
            {runtimeError ? `Workspace unavailable (${runtimeError.code})` : "No workspace available"}
          </div>
        )}
      </main>

      <CommandPalette
        open={isCommandPaletteOpen}
        worktrees={state.worktrees}
        tabs={state.layout.tabs}
        onSelectWorktree={handleSelectWorktree}
        onSelectTab={handleSelectTerminalTab}
        onClose={() => setIsCommandPaletteOpen(false)}
      />
      <SettingsDialog open={isSettingsOpen} onClose={() => setIsSettingsOpen(false)} />
      {isAddProjectOpen ? (
        <AddProjectDialog
          projects={projects}
          onClose={() => setIsAddProjectOpen(false)}
          onRegistered={handleRegisteredProject}
        />
      ) : null}
      {isCreateOpen ? (
        <AddWorktreeDialog
          project={activeProject}
          onClose={() => setIsCreateOpen(false)}
          onCreated={async (worktree) => {
            await refreshWorktrees();
            await ensureTabForWorktree(worktree).catch(reportRuntimeError);
          }}
        />
      ) : null}
      {deleteTarget ? (
        <WorktreeDeleteDialog
          workspaceId={activeProject.workspaceId}
          worktree={deleteTarget}
          onClose={() => setDeleteTarget(null)}
          onDeleted={() => {
            setWorktreeStatuses((current) => {
              const next = { ...current };
              delete next[deleteTarget.path];
              return next;
            });
            void refreshWorktrees();
          }}
        />
      ) : null}

      {runtimeError && activeWorktree ? (
        <div className="pointer-events-none fixed bottom-3 right-3 z-40 max-w-error border border-destructive/30 bg-card/95 px-3 py-2 text-[11px] text-destructive shadow-lg">
          {runtimeError.code}: {runtimeError.message}
        </div>
      ) : null}
    </div>
  );
}

function listVisibleWorktrees(
  projects: RegisteredProject[],
  worktrees: Worktree[],
  activeProjectId: string,
): Worktree[] {
  const collapsed = loadCollapsedProjectIds(projects, activeProjectId);
  const known = new Set(projects.map((project) => project.workspaceId));
  const visible: Worktree[] = [];

  for (const project of projects) {
    if (collapsed.has(project.workspaceId)) continue;
    for (const worktree of worktrees) {
      const ownerId = worktreeIdentity(worktree)?.wsId;
      const owner = ownerId && known.has(ownerId) ? ownerId : activeProjectId;
      if (owner === project.workspaceId) visible.push(worktree);
    }
  }

  return visible;
}

function loadCollapsedProjectIds(projects: RegisteredProject[], activeProjectId: string): Set<string> {
  try {
    const raw = getMigratedItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    if (Array.isArray(parsed)) return new Set(parsed.filter((id): id is string => typeof id === "string"));
    return new Set(projects.filter((project) => project.workspaceId !== activeProjectId).map((project) => project.workspaceId));
  } catch {
    return new Set<string>();
  }
}

function loadProjects(): RegisteredProject[] {
  try {
    const raw = getMigratedItem(PROJECTS_STORAGE_KEY);
    if (!raw) return [DEFAULT_PROJECT];
    const parsed = JSON.parse(raw) as RegisteredProject[];
    if (!Array.isArray(parsed)) return [DEFAULT_PROJECT];
    const valid = parsed.filter(
      (project) => project && typeof project.workspaceId === "string" && typeof project.repoRoot === "string",
    );
    return valid.length > 0 ? valid : [DEFAULT_PROJECT];
  } catch {
    return [DEFAULT_PROJECT];
  }
}

function persistProjects(projects: RegisteredProject[]) {
  try {
    window.localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
  } catch {
    // A local persistence failure must not block native project registration.
  }
}

function loadActiveProjectId() {
  try {
    return getMigratedItem(ACTIVE_PROJECT_STORAGE_KEY) || DEFAULT_WORKSPACE_ID;
  } catch {
    return DEFAULT_WORKSPACE_ID;
  }
}

function persistActiveProjectId(workspaceId: string) {
  try {
    window.localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, workspaceId);
  } catch {
    // The selected project still remains active for this session.
  }
}

function loadSidebarOpen() {
  try {
    const raw = getMigratedItem(SIDEBAR_OPEN_STORAGE_KEY);
    return raw !== null ? raw !== "false" : true;
  } catch {
    return true;
  }
}

function persistSidebarOpen(open: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_OPEN_STORAGE_KEY, String(open));
  } catch {
    // Persistence failure should not break in-memory state.
  }
}

export default App;
