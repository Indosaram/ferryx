import { PanelLeft } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { AddProjectDialog, AddWorktreeDialog } from "./components/ProjectDialogs";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar, SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY } from "./components/Sidebar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { IconButton } from "./components/ui/IconButton";
import { serializeWorkspaceState } from "./lib/sessionPersistence";
import { isMacShortcutPlatform, useShortcuts } from "./lib/shortcuts";
import { DEFAULT_WORKSPACE_ID, getWorktreeStatus, loadSession, registerProject, saveSession, type RegisteredProject } from "./lib/tauri";
import { worktreeIdentity, type DirtyState, type Worktree } from "./lib/types";
import { registerWindowCloseGuard } from "./lib/updater";
import type { PaneDirection } from "./state/paneTree";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { useWorkspaceStore } from "./state/workspaceStore";

const PROJECTS_STORAGE_KEY = "rorca.projects";
const ACTIVE_PROJECT_STORAGE_KEY = "rorca.active-project";
export const SIDEBAR_OPEN_STORAGE_KEY = "orca.sidebar.open";
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
    closePane,
    activateTab,
    reorderTab,
    renameTab,
    setTabPinned,
    focusPane,
    setPaneRatio,
    swapPanes,
    syncWorktrees,
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

  // Initial session restore on startup
  useEffect(() => {
    let cancelled = false;
    async function restore() {
      try {
        const session = await loadSession();
        if (cancelled || !session) return;
        const ws = session.workspaces?.[activeProject.workspaceId];
        if (!ws || !ws.layout?.tabs?.length) return;

        for (const tab of ws.layout.tabs) {
          if (cancelled) return;
          const found = ws.worktrees?.find((candidate: any) => candidate.path === tab.worktreePath);
          const wt: Worktree = found ? {
            path: found.path,
            head: found.head,
            branch: found.branch,
            bare: false,
            detached: false,
            locked: found.isLocked ? "locked" : null,
            prunable: null,
          } : {
            path: tab.worktreePath || ws.repoRoot,
            head: "",
            branch: tab.label,
            bare: false,
            detached: false,
            locked: null,
            prunable: null,
          };
          try {
            await ensureTabForWorktree(wt);
          } catch {
            // Safe fallback if worktree terminal is already open
          }
        }
      } catch (error) {
        console.warn("Session restore on boot skipped:", error);
      }
    }
    const timer = setTimeout(() => {
      void restore();
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [activeProject.workspaceId, ensureTabForWorktree]);

  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      const session = serializeWorkspaceState(
        activeProject.workspaceId,
        activeProject.repoRoot,
        state,
      );
      await saveSession(session);
    });
    return unregister;
  }, [activeProject.repoRoot, activeProject.workspaceId, state]);

  // Debounced auto-save on workspace state changes (500ms)
  useEffect(() => {
    // Do not save if tabs are empty (prevents overwriting session on unmounted / empty initial render)
    if (state.worktrees.length === 0 || state.layout.tabs.length === 0) return;
    const timer = setTimeout(() => {
      const session = serializeWorkspaceState(
        activeProject.workspaceId,
        activeProject.repoRoot,
        state,
      );
      void Promise.resolve(saveSession(session)).catch((error) =>
        console.error("Failed to auto-save session:", error),
      );
    }, 500);
    return () => clearTimeout(timer);
  }, [activeProject.repoRoot, activeProject.workspaceId, state]);

  const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpen);
  const [deleteTarget, setDeleteTarget] = useState<Worktree | null>(null);
  const [worktreeStatuses, setWorktreeStatuses] = useState<Record<string, DirtyState | undefined>>({});
  // A worktree picked from another project can only be activated once that project's worktrees
  // have loaded into the store, so the request is parked here until the path shows up.
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
      // Any selection parked for the previous project is abandoned by this switch.
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
      // A worktree selected from another project's tree must switch the active project first,
      // because the workspace store only ever holds the active project's worktrees.
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
      const worktree = session ? state.worktrees.find((candidate) => candidate.path === session.cwd) : undefined;
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
      const tabs = state.layout.tabs;
      if (tabs.length < 2) return;
      const currentIndex = Math.max(0, tabs.findIndex((tab) => tab.id === state.layout.activeTabId));
      const nextIndex = (currentIndex + offset + tabs.length) % tabs.length;
      handleSelectTerminalTab(tabs[nextIndex].id);
    },
    [handleSelectTerminalTab, state.layout.activeTabId, state.layout.tabs],
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

  // Cmd+1..9 walks the sidebar tree top to bottom: every expanded project contributes its
  // worktrees in order, so the numbering matches exactly what the user can see. The accordion
  // state lives in the sidebar and is read at press time so it can never go stale here.
  const handleSelectWorktreeByIndex = useCallback(
    (index: number) => {
      const visible = listVisibleWorktrees(projects, state.worktrees, activeProject.workspaceId);
      const target = visible[index];
      if (target) handleSelectWorktree(target);
    },
    [activeProject.workspaceId, handleSelectWorktree, projects, state.worktrees],
  );

  const handleSelectTerminalTabByIndex = useCallback(
    (index: number) => {
      const target = state.layout.tabs[index];
      if (target) handleSelectTerminalTab(target.id);
    },
    [handleSelectTerminalTab, state.layout.tabs],
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
      "workspace.select1": () => handleSelectWorktreeByIndex(0),
      "workspace.select2": () => handleSelectWorktreeByIndex(1),
      "workspace.select3": () => handleSelectWorktreeByIndex(2),
      "workspace.select4": () => handleSelectWorktreeByIndex(3),
      "terminal.splitRight": () => handleSplitActive("horizontal"),
      "terminal.splitDown": () => handleSplitActive("vertical"),
      "terminal.unsplit": handleUnsplitActive,
      "sidebar.left.toggle": toggleSidebar,
      "commandPalette.open": () => setIsCommandPaletteOpen(true),
    }),
    [
      createBrowserTab,
      handleAddTerminalTab,
      handleCloseTab,
      handleCycleTab,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleUnsplitActive,
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
            onActivateTab={handleSelectTerminalTab}
            onCloseTab={handleCloseTab}
            onCloseOtherTabs={handleCloseOtherTabs}
            onCloseTabsToRight={handleCloseTabsToRight}
            onCloseTabsToLeft={handleCloseTabsToLeft}
            onReorderTab={reorderTab}
            onRenameTab={renameTab}
            onToggleTabPin={setTabPinned}
            onAddTab={handleAddTerminalTab}
            onAddBrowserTab={(url) => void createBrowserTab(url ?? "http://localhost:3000").catch(reportRuntimeError)}
            onNavigateBrowserTab={(tabId, url) => void navigateBrowserTab(tabId, url).catch(reportRuntimeError)}
            onReloadBrowserTab={(tabId) => void reloadBrowserTab(tabId).catch(reportRuntimeError)}
            onSplitPane={(tabId: string, leafId: string, direction: PaneDirection) => splitPane(tabId, leafId, direction).catch(reportRuntimeError)}
            onClosePane={(tabId: string, leafId: string) => closePane(tabId, leafId).catch(reportRuntimeError)}
            onSetRatio={setPaneRatio}
            onSwapPanes={swapPanes}
            onFocusPane={focusPane}
            onTitleChange={updateSessionTitleActivity}
            leadingSpacer={isSidebarOpen ? 0 : (isMacShortcutPlatform() ? 108 : 36)}
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
        <AddProjectDialog onClose={() => setIsAddProjectOpen(false)} onRegistered={handleRegisteredProject} />
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

/**
 * Flattens the sidebar tree into the worktree order the user actually sees: projects in order,
 * each expanded project followed by its own worktrees. Collapsed projects contribute nothing.
 */
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
      // Worktrees name their owning project in the `orca/<wsId>/<slug>` branch; anything else
      // belongs to the active project, the only one whose worktrees the store holds.
      const ownerId = worktreeIdentity(worktree)?.wsId;
      const owner = ownerId && known.has(ownerId) ? ownerId : activeProjectId;
      if (owner === project.workspaceId) visible.push(worktree);
    }
  }

  return visible;
}

/** Mirrors the sidebar accordion defaults: persisted state wins, otherwise only the active project is expanded. */
function loadCollapsedProjectIds(projects: RegisteredProject[], activeProjectId: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    if (Array.isArray(parsed)) return new Set(parsed.filter((id): id is string => typeof id === "string"));
    // Nothing persisted yet: the sidebar starts with only the active project expanded.
    return new Set(
      projects.filter((project) => project.workspaceId !== activeProjectId).map((project) => project.workspaceId),
    );
  } catch {
    return new Set<string>();
  }
}

function loadProjects(): RegisteredProject[] {
  try {
    const raw = window.localStorage.getItem(PROJECTS_STORAGE_KEY);
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
    return window.localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY) || DEFAULT_WORKSPACE_ID;
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
    const raw = window.localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY);
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
