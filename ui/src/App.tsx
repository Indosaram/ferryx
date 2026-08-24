import { PanelLeft } from "lucide-react";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { EmptyWorkspaceView } from "./components/EmptyWorkspaceView";
import { AddProjectDialog, AddWorktreeDialog } from "./components/ProjectDialogs";
import { Sidebar } from "./components/Sidebar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { ConfirmCloseTabDialog } from "./components/ConfirmCloseTabDialog";
import { IconButton } from "./components/ui/IconButton";
import { newBrowserTabUrl } from "./lib/browserSettings";
import { useGeneralSettings } from "./lib/generalSettings";
import { serializeWorkspaceState } from "./lib/sessionPersistence";
import { isMacShortcutPlatform, useShortcuts } from "./lib/shortcuts";
import {
  AGENT_CANDIDATES,
  AGENTS_SETTINGS_CHANGED_EVENT,
  getLaunchableAgents,
  loadAgentSettings,
  mergeDetections,
  type AgentSettings,
} from "./lib/agentsSettings";
import {
  ACTIVE_PROJECT_STORAGE_KEY,
  getMigratedItem,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} from "./lib/storageKeys";
import {
  DEFAULT_WORKSPACE_ID,
  detectAgents,
  getInitialProject,
  isTauriRuntime,
  loadSession,
  onCloseTabMenu,
  onNewTerminalTabMenu,
  onRemoteSelectionRequested,
  publishFocusedTerminal,
  registerProject,
  saveSession,
  setBadgeCount,
  spawnTerminal,
  writeTerminal,
  type AgentDetection,
  type FocusedTerminalPayload,
  type RegisteredProject,
  type RemoteSelectionRequestedPayload,
} from "./lib/tauri";
import { ensureTerminalEvents } from "./lib/terminalEvents";
import { useTerminalSettings } from "./lib/terminalSettings";
import { worktreeIdentity, type DirtyState, type WorkspaceTab, type Worktree } from "./lib/types";
import { registerWindowCloseGuard } from "./lib/updater";
import { collectLeafIds, type PaneDirection } from "./state/paneTree";
import { useWorkspaceRestore } from "./state/workspaceRestore";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { useWorkspaceStore, type WorkspaceState } from "./state/workspaceStore";

export { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY };
const DEFAULT_PROJECT: RegisteredProject = { workspaceId: DEFAULT_WORKSPACE_ID, repoRoot: "." };
const SettingsDialog = lazy(() =>
  import("./components/SettingsDialog").then((m) => ({ default: m.SettingsDialog })),
);

type ProjectBootstrap = {
  projects: RegisteredProject[];
  activeProjectId: string;
};

function loadProjectBootstrap(): ProjectBootstrap {
  return { projects: loadProjects(), activeProjectId: loadActiveProjectId() };
}

function isInitialProjectPlaceholder(project: RegisteredProject, startup: RegisteredProject) {
  return (
    project.workspaceId === DEFAULT_WORKSPACE_ID &&
    (project.repoRoot === "." || project.repoRoot === "" || project.repoRoot === startup.repoRoot)
  );
}

function canonicalizeProjectBootstrap(stored: ProjectBootstrap, startup: RegisteredProject): ProjectBootstrap {
  const replacesPlaceholder = stored.projects.some((project) => isInitialProjectPlaceholder(project, startup));
  if (!replacesPlaceholder) return stored;

  const projects = stored.projects.reduce<RegisteredProject[]>((next, project) => {
    const canonical = isInitialProjectPlaceholder(project, startup) ? startup : project;
    if (!next.some((candidate) => candidate.workspaceId === canonical.workspaceId)) next.push(canonical);
    return next;
  }, []);
  const activeProjectId = stored.activeProjectId === DEFAULT_WORKSPACE_ID ? startup.workspaceId : stored.activeProjectId;
  persistProjects(projects);
  persistActiveProjectId(activeProjectId);
  return { projects, activeProjectId };
}

export function App() {
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const [bootstrap, setBootstrap] = useState<ProjectBootstrap | null>(() =>
    isNativeRuntime ? null : loadProjectBootstrap(),
  );
  const [bootstrapError, setBootstrapError] = useState<string | null>(null);

  useEffect(() => {
    if (!isNativeRuntime) return;
    let cancelled = false;
    void getInitialProject()
      .then((startup) => {
        if (!cancelled) setBootstrap(canonicalizeProjectBootstrap(loadProjectBootstrap(), startup));
      })
      .catch((error) => {
        if (!cancelled) setBootstrapError(error instanceof Error ? error.message : String(error));
      });
    return () => {
      cancelled = true;
    };
  }, [isNativeRuntime]);

  if (bootstrapError) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background text-xs text-destructive">
        Unable to initialize project: {bootstrapError}
      </div>
    );
  }
  if (!bootstrap) return <div className="h-screen w-screen bg-background" aria-label="Initializing project" />;

  return <WorkspaceApp initialProjects={bootstrap.projects} initialActiveProjectId={bootstrap.activeProjectId} />;
}

export function deriveFocusedTerminal(
  workspaceId: string,
  state: WorkspaceState,
): FocusedTerminalPayload | null {
  const focusedGroup = state.layout.focusedGroupId
    ? state.layout.tabGroups?.[state.layout.focusedGroupId]
    : undefined;
  const activeTabId =
    focusedGroup?.activeTabId ?? state.layout.activeTabId ?? state.layout.tabs[0]?.id ?? null;
  if (!activeTabId) return null;

  const activeTab = state.layout.tabs.find((tab) => tab.id === activeTabId);
  if (!activeTab || activeTab.kind === "browser") return null;

  const tabLayout = state.layout.layoutsByTabId?.[activeTab.id];
  const activeLeafId =
    tabLayout?.activeLeafId ?? (tabLayout?.root ? collectLeafIds(tabLayout.root)[0] : null);
  const localSessionId =
    (activeLeafId && tabLayout?.sessionIdsByLeafId?.[activeLeafId]) || activeTab.sessionId;
  const session = localSessionId ? state.sessions[localSessionId] : undefined;

  const sessionWorktreePath = session?.worktreePath ?? session?.cwd;
  const foundWorktree = sessionWorktreePath
    ? state.worktrees.find((wt) => wt.path === sessionWorktreePath)
    : (state.worktrees.find((wt) => wt.path === state.activeWorktreePath) ?? null);

  const ident = foundWorktree ? worktreeIdentity(foundWorktree) : null;
  const worktreeSlug =
    ident?.slug ??
    (foundWorktree?.branch ? foundWorktree.branch.replace(/^refs\/heads\//, "") : null);
  const worktreeLabel = foundWorktree?.branch
    ? foundWorktree.branch.replace(/^refs\/heads\//, "")
    : activeTab.label;

  return {
    workspaceId,
    worktreeSlug: worktreeSlug ?? null,
    worktreeLabel: worktreeLabel ?? null,
    backendSessionId: session?.backendSessionId ?? null,
  };
}

function matchWorktreeBySlug(worktrees: Worktree[], slug: string): Worktree | undefined {
  return worktrees.find((wt) => {
    const ident = worktreeIdentity(wt);
    if (ident && (ident.slug === slug || ident.slug.endsWith("/" + slug))) return true;
    const branchName = wt.branch?.replace(/^refs\/heads\//, "");
    if (branchName === slug || branchName?.endsWith("/" + slug)) return true;
    const lastComponent = wt.path.split("/").filter(Boolean).pop();
    if (lastComponent === slug) return true;
    return false;
  });
}

function WorkspaceApp({
  initialProjects,
  initialActiveProjectId,
}: {
  initialProjects: RegisteredProject[];
  initialActiveProjectId: string;
}) {
  const [projects, setProjects] = useState<RegisteredProject[]>(initialProjects);
  const [activeProjectId, setActiveProjectId] = useState(initialActiveProjectId);
  const { settings: generalSettings } = useGeneralSettings();
  const activeProject = useMemo(
    () => projects.find((project) => project.workspaceId === activeProjectId) ?? projects[0] ?? DEFAULT_PROJECT,
    [activeProjectId, projects],
  );

  const projectsRef = useRef(projects);
  projectsRef.current = projects;
  const activeProjectRef = useRef(activeProject);
  activeProjectRef.current = activeProject;

  const [agentSettings, setAgentSettings] = useState<AgentSettings>(loadAgentSettings);
  const [agentDetections, setAgentDetections] = useState<AgentDetection[]>([]);

  useEffect(() => {
    let cancelled = false;
    async function runDetection() {
      if (!isTauriRuntime()) return;
      try {
        const results = await detectAgents([...AGENT_CANDIDATES]);
        if (!cancelled) setAgentDetections(results);
      } catch (error) {
        console.warn("Failed to detect agents:", error);
      }
    }
    void runDetection();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const handleSettingsChange = () => {
      setAgentSettings(loadAgentSettings());
    };
    window.addEventListener(AGENTS_SETTINGS_CHANGED_EVENT, handleSettingsChange);
    return () => window.removeEventListener(AGENTS_SETTINGS_CHANGED_EVENT, handleSettingsChange);
  }, []);

  const resolvedAgents = useMemo(
    () => mergeDetections(agentSettings, agentDetections),
    [agentSettings, agentDetections],
  );

  const launchableAgents = useMemo(
    () => getLaunchableAgents(resolvedAgents),
    [resolvedAgents],
  );

  const {
    state,
    recoveredFromHmr,
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
  const stateRef = useRef(state);
  stateRef.current = state;
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

  // Initial session restore on startup & HMR recovery managed by coordinator.
  useWorkspaceRestore({
    workspaceId: activeProject.workspaceId,
    recoveredFromHmr,
    restoreWorkspace,
  });

  const saveChainRef = useRef<Promise<void>>(Promise.resolve());
  const persistSession = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    saveChainRef.current = saveChainRef.current
      .then(async () => {
        const existing = await loadSession().catch(() => null);
        const session = serializeWorkspaceState(
          workspaceId,
          repoRoot,
          currentState,
          existing,
        );
        await saveSession(session);
      })
      .catch((error) => {
        console.error("Failed to save workspace session:", error);
      });
    return saveChainRef.current;
  }, []);

  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      await persistSession(activeProjectRef.current.workspaceId, activeProjectRef.current.repoRoot, stateRef.current);
    });
    return unregister;
  }, [persistSession]);

  useEffect(() => {
    const hasTabs =
      state.layout.tabs.length > 0 ||
      Object.values(state.worktreeLayouts ?? {}).some((l) => l.tabs.length > 0);
    if (state.worktrees.length === 0 || !hasTabs) return;
    const timer = setTimeout(() => {
      void persistSession(activeProject.workspaceId, activeProject.repoRoot, stateRef.current);
    }, 500);
    return () => clearTimeout(timer);
  }, [activeProject.repoRoot, activeProject.workspaceId, persistSession, state]);

  const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);
  const [createTargetProject, setCreateTargetProject] = useState<RegisteredProject | null>(null);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [searchLeafId, setSearchLeafId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpen);
  const [deleteTarget, setDeleteTarget] = useState<Worktree | null>(null);
  const [pendingTabClose, setPendingTabClose] = useState<{ id: string; label: string } | null>(null);
  const [worktreeStatuses, setWorktreeStatuses] = useState<Record<string, DirtyState | undefined>>({});
  const [pendingWorktreePath, setPendingWorktreePath] = useState<string | null>(null);
  const [pendingRemoteSlug, setPendingRemoteSlug] = useState<string | null>(null);

  const focusedTerminalPayload = useMemo(
    () => deriveFocusedTerminal(activeProject.workspaceId, state),
    [activeProject.workspaceId, state],
  );

  useEffect(() => {
    void publishFocusedTerminal(focusedTerminalPayload);
  }, [focusedTerminalPayload]);

  const unreadBadgeCount = useMemo(
    () => Object.values(state.unreadTabIds ?? {}).filter(Boolean).length,
    [state.unreadTabIds],
  );

  useEffect(() => {
    void setBadgeCount(unreadBadgeCount).catch(reportRuntimeError);
  }, [reportRuntimeError, unreadBadgeCount]);

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
  const activeWorktreeRef = useRef(activeWorktree);
  activeWorktreeRef.current = activeWorktree;

  const handleSelectProject = useCallback(
    (project: RegisteredProject) => {
      if (project.workspaceId === activeProjectRef.current.workspaceId) return;
      setActiveProjectId(project.workspaceId);
      persistActiveProjectId(project.workspaceId);
      setWorktreeStatuses({});
      setDeleteTarget(null);
      setPendingWorktreePath(null);
    },
    [],
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
      const owner = ownerId ? projectsRef.current.find((project) => project.workspaceId === ownerId) : undefined;
      if (owner && owner.workspaceId !== activeProjectRef.current.workspaceId) {
        handleSelectProject(owner);
        setPendingWorktreePath(worktree.path);
        return;
      }
      void ensureTabForWorktree(worktree).catch(reportRuntimeError);
    },
    [ensureTabForWorktree, handleSelectProject, reportRuntimeError],
  );

  useEffect(() => {
    if (!pendingWorktreePath) return;
    const target = state.worktrees.find((worktree) => worktree.path === pendingWorktreePath);
    if (!target) return;
    setPendingWorktreePath(null);
    void ensureTabForWorktree(target).catch(reportRuntimeError);
  }, [ensureTabForWorktree, pendingWorktreePath, reportRuntimeError, state.worktrees]);

  useEffect(() => {
    if (!pendingRemoteSlug) return;
    const target = matchWorktreeBySlug(state.worktrees, pendingRemoteSlug);
    if (!target) return;
    setPendingRemoteSlug(null);
    void ensureTabForWorktree(target).catch(reportRuntimeError);
  }, [ensureTabForWorktree, pendingRemoteSlug, reportRuntimeError, state.worktrees]);

  const handleRemoteSelectionRequested = useCallback(
    (payload: RemoteSelectionRequestedPayload) => {
      if (!payload || !payload.workspaceId) return;
      const targetProject = projectsRef.current.find((p) => p.workspaceId === payload.workspaceId);
      const isCurrentProject = activeProjectRef.current.workspaceId === payload.workspaceId;

      if (payload.worktreeSlug) {
        if (isCurrentProject) {
          const targetWorktree = matchWorktreeBySlug(stateRef.current.worktrees, payload.worktreeSlug);
          if (targetWorktree) {
            handleSelectWorktree(targetWorktree);
          }
        } else if (targetProject) {
          handleSelectProject(targetProject);
          setPendingRemoteSlug(payload.worktreeSlug);
        }
      } else {
        if (!isCurrentProject && targetProject) {
          handleSelectProject(targetProject);
        }
      }
    },
    [handleSelectProject, handleSelectWorktree],
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onRemoteSelectionRequested(handleRemoteSelectionRequested).then((dispose: () => void) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleRemoteSelectionRequested]);

  const handleSelectTerminalTab = useCallback(
    (tabId: string) => {
      const currentState = stateRef.current;
      const tab = currentState.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab) return;
      if (tab.kind === "browser") {
        activateTab(tabId);
        return;
      }
      const session = currentState.sessions[tab.sessionId];
      const sessionWorktreePath = session?.worktreePath ?? session?.cwd;
      const worktree = sessionWorktreePath
        ? currentState.worktrees.find((candidate) => candidate.path === sessionWorktreePath)
        : undefined;
      if (!worktree || worktree.path === currentState.activeWorktreePath) {
        activateTab(tabId);
        return;
      }
      void ensureTabForWorktree(worktree)
        .then(() => activateTab(tabId))
        .catch(reportRuntimeError);
    },
    [activateTab, ensureTabForWorktree, reportRuntimeError],
  );

  const handleAddTerminalTab = useCallback(() => {
    const activeWt = activeWorktreeRef.current;
    if (!activeWt) return;
    void openTab(activeWt).catch(reportRuntimeError);
  }, [openTab, reportRuntimeError]);

  const handleLaunchAgent = useCallback(
    async (agent: { name: string; command: string; args: string }) => {
      try {
        const targetWorktree = activeWorktreeRef.current ?? stateRef.current.worktrees[0];
        if (!targetWorktree) return;
        await ensureTerminalEvents().catch(() => undefined);
        const backendSessionId = await spawnTerminal({
          workspaceId: activeProjectRef.current.workspaceId,
          worktree: worktreeIdentity(targetWorktree),
          cwd: targetWorktree.path,
        });
        const label = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
        await openTab(targetWorktree, label, backendSessionId);
        const fullCommand = `${agent.command} ${agent.args}`.trim();
        if (fullCommand) {
          await writeTerminal({ sessionId: backendSessionId, data: `${fullCommand}\r` });
        }
      } catch (error) {
        reportRuntimeError(error);
      }
    },
    [openTab, reportRuntimeError],
  );

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
      const tab = stateRef.current.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.pinned) return;
      if (generalSettings.confirmCloseTab) {
        setPendingTabClose({ id: tab.id, label: tab.label });
        return;
      }
      void closeTab(tabId).catch(reportRuntimeError);
    },
    [closeTab, generalSettings.confirmCloseTab, reportRuntimeError],
  );

  const handleConfirmTabClose = useCallback(() => {
    if (!pendingTabClose) return;
    const tabId = pendingTabClose.id;
    setPendingTabClose(null);
    void closeTab(tabId).catch(reportRuntimeError);
  }, [closeTab, pendingTabClose, reportRuntimeError]);

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
      const currentState = stateRef.current;
      const focusedGroup = currentState.layout.focusedGroupId
        ? currentState.layout.tabGroups?.[currentState.layout.focusedGroupId]
        : undefined;
      const tabById = new Map(currentState.layout.tabs.map((tab) => [tab.id, tab]));
      const tabs: WorkspaceTab[] = focusedGroup
        ? focusedGroup.tabIds.map((tabId) => tabById.get(tabId)).filter((tab): tab is WorkspaceTab => Boolean(tab))
        : currentState.layout.tabs;
      if (tabs.length < 2) return;
      const currentIndex = Math.max(0, tabs.findIndex((tab) => tab.id === currentState.layout.activeTabId));
      const nextIndex = (currentIndex + offset + tabs.length) % tabs.length;
      handleSelectTerminalTab(tabs[nextIndex].id);
    },
    [handleSelectTerminalTab],
  );

  const handleSplitActive = useCallback(
    (direction: PaneDirection) => {
      const currentState = stateRef.current;
      const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") return;
      const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
      const targetLeafId = activeLayout?.activeLeafId ?? "leaf-default";
      void splitPane(activeTab.id, targetLeafId, direction).catch(reportRuntimeError);
    },
    [reportRuntimeError, splitPane],
  );

  const handleUnsplitActive = useCallback(() => {
    const currentState = stateRef.current;
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
    if (!activeLayout || activeLayout.root.type === "leaf") return;
    const activeLeafId = activeLayout.activeLeafId ?? "leaf-default";
    void closePane(activeTab.id, activeLeafId).catch(reportRuntimeError);
  }, [closePane, reportRuntimeError]);

  const handleCyclePaneFocus = useCallback(
    (offset: number) => {
      const currentState = stateRef.current;
      const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") return;
      const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
      if (!activeLayout) return;
      const leafIds = collectLeafIds(activeLayout.root);
      if (leafIds.length < 2) return;
      const activeLeafId = activeLayout.activeLeafId ?? leafIds[0];
      const currentIndex = Math.max(0, leafIds.indexOf(activeLeafId));
      const nextIndex = (currentIndex + offset + leafIds.length) % leafIds.length;
      focusPane(activeTab.id, leafIds[nextIndex]);
    },
    [focusPane],
  );

  const handleOpenTerminalSearch = useCallback(() => {
    const currentState = stateRef.current;
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
    const leafId = activeLayout?.activeLeafId ?? (activeLayout?.root ? collectLeafIds(activeLayout.root)[0] : "leaf-default");
    setSearchLeafId(leafId);
  }, []);

  const handleSelectWorktreeByIndex = useCallback(
    (index: number) => {
      const visible = listVisibleWorktrees(projectsRef.current, stateRef.current.worktrees, activeProjectRef.current.workspaceId);
      const target = visible[index];
      if (target) handleSelectWorktree(target);
    },
    [handleSelectWorktree],
  );

  const { settings: terminalSettings, updateSettings: updateTerminalSettings } = useTerminalSettings();
  const terminalSettingsRef = useRef(terminalSettings);
  terminalSettingsRef.current = terminalSettings;

  const handleZoomIn = useCallback(() => {
    const nextSize = Math.min(36, terminalSettingsRef.current.fontSize + 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [updateTerminalSettings]);

  const handleZoomOut = useCallback(() => {
    const nextSize = Math.max(10, terminalSettingsRef.current.fontSize - 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [updateTerminalSettings]);

  const handleZoomReset = useCallback(() => {
    updateTerminalSettings({ fontSize: null });
  }, [updateTerminalSettings]);

  const handleSelectTerminalTabByIndex = useCallback(
    (index: number) => {
      const currentState = stateRef.current;
      const focusedGroup = currentState.layout.focusedGroupId
        ? currentState.layout.tabGroups?.[currentState.layout.focusedGroupId]
        : undefined;
      const tabId = focusedGroup?.tabIds[index] ?? currentState.layout.tabs[index]?.id;
      if (tabId) handleSelectTerminalTab(tabId);
    },
    [handleSelectTerminalTab],
  );

  const handleOpenAddProject = useCallback(() => setIsAddProjectOpen(true), []);
  const handleCloseAddProject = useCallback(() => setIsAddProjectOpen(false), []);
  const handleOpenCreateWorktree = useCallback((project?: RegisteredProject) => {
    setCreateTargetProject(project ?? activeProjectRef.current);
    setIsCreateOpen(true);
  }, []);
  const handleCloseCreateWorktree = useCallback(() => {
    setIsCreateOpen(false);
    setCreateTargetProject(null);
  }, []);
  const handleOpenCommandPalette = useCallback(() => setIsCommandPaletteOpen(true), []);
  const handleCloseCommandPalette = useCallback(() => setIsCommandPaletteOpen(false), []);
  const handleOpenSettings = useCallback(() => setIsSettingsOpen(true), []);
  const handleCloseSettings = useCallback(() => setIsSettingsOpen(false), []);
  const handleToggleSettings = useCallback(() => setIsSettingsOpen((current) => !current), []);
  const handleCloseSearch = useCallback(() => setSearchLeafId(null), []);
  const handleCloseDeleteTarget = useCallback(() => setDeleteTarget(null), []);
  const handleCancelTabClose = useCallback(() => setPendingTabClose(null), []);

  const handleAddBrowserTab = useCallback(
    (url?: string) => {
      void createBrowserTab(url ?? newBrowserTabUrl()).catch(reportRuntimeError);
    },
    [createBrowserTab, reportRuntimeError],
  );

  const handleNavigateBrowserTab = useCallback(
    (tabId: string, url: string) => {
      void navigateBrowserTab(tabId, url).catch(reportRuntimeError);
    },
    [navigateBrowserTab, reportRuntimeError],
  );

  const handleReloadBrowserTab = useCallback(
    (tabId: string) => {
      void reloadBrowserTab(tabId).catch(reportRuntimeError);
    },
    [reloadBrowserTab, reportRuntimeError],
  );

  const handleSplitPane = useCallback(
    (tabId: string, leafId: string, direction: PaneDirection, options?: { position?: "first" | "second" }) => {
      void splitPane(tabId, leafId, direction, options).catch(reportRuntimeError);
    },
    [reportRuntimeError, splitPane],
  );

  const handleClosePane = useCallback(
    (tabId: string, leafId: string) => {
      void closePane(tabId, leafId).catch(reportRuntimeError);
    },
    [closePane, reportRuntimeError],
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onCloseTabMenu(() => {
      const activeTabId = stateRef.current.layout.activeTabId;
      if (activeTabId) handleCloseTab(activeTabId);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleCloseTab]);

  const shortcutHandlers = useMemo(
    () => ({
      "tab.newTerminal": handleAddTerminalTab,
      "tab.newBrowser": () => void createBrowserTab(newBrowserTabUrl()).catch(reportRuntimeError),
      "tab.close": () => {
        const activeTabId = stateRef.current.layout.activeTabId;
        if (activeTabId) handleCloseTab(activeTabId);
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
      "commandPalette.open": handleOpenCommandPalette,
      "settings.toggle": handleToggleSettings,
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
      handleOpenCommandPalette,
      handleOpenTerminalSearch,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleToggleSettings,
      handleUnsplitActive,
      handleZoomIn,
      handleZoomOut,
      handleZoomReset,
      reportRuntimeError,
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
          onAddProject={handleOpenAddProject}
          onSelectWorktree={handleSelectWorktree}
          onCreateWorktree={handleOpenCreateWorktree}
          onDeleteWorktree={setDeleteTarget}
          onOpenSettings={handleOpenSettings}
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
        {activeWorktree && state.layout.tabs.length === 0 ? (
          <EmptyWorkspaceView
            onNewTerminal={handleAddTerminalTab}
            onNewBrowserTab={handleAddBrowserTab}
          />
        ) : activeWorktree ? (
          <TerminalSplitView
            layout={state.layout}
            sessions={state.sessions}
            unreadTabIds={state.unreadTabIds}
            activityByTabId={tabActivity}
            onTitleChange={updateSessionTitleActivity}
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
            onAddBrowserTab={handleAddBrowserTab}
            onOpenSettings={handleOpenSettings}
            agents={launchableAgents}
            onLaunchAgent={handleLaunchAgent}
            defaultAgentId={agentSettings.defaultAgentId}
            onNavigateBrowserTab={handleNavigateBrowserTab}
            onReloadBrowserTab={handleReloadBrowserTab}
            onSplitPane={handleSplitPane}
            onClosePane={handleClosePane}
            onSetRatio={setPaneRatio}
            onSetGroupRatio={setTabGroupRatio}
            onSwapPanes={swapPanes}
            onFocusPane={focusPane}
            searchLeafId={searchLeafId}
            onCloseSearch={handleCloseSearch}
            leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}
          />
        ) : (
          <div className="flex h-full flex-1 items-center justify-center bg-background text-xs text-muted-foreground">
            {runtimeError ? `Workspace unavailable (${runtimeError.code})` : "No workspace available"}
          </div>
        )}
      </main>

      {isCommandPaletteOpen ? (
        <CommandPalette
          open={true}
          worktrees={state.worktrees}
          tabs={state.layout.tabs}
          onSelectWorktree={handleSelectWorktree}
          onSelectTab={handleSelectTerminalTab}
          onClose={handleCloseCommandPalette}
        />
      ) : null}
      {isSettingsOpen ? (
        <Suspense fallback={null}>
          <SettingsDialog
            open
            onClose={handleCloseSettings}
            projects={projects}
            activeProjectId={activeProject.workspaceId}
            activeWorktree={activeWorktree}
            onSelectProject={handleSelectProject}
            onAddProject={handleOpenAddProject}
            onAddWorktree={handleOpenCreateWorktree}
          />
        </Suspense>
      ) : null}
      {isAddProjectOpen ? (
        <AddProjectDialog
          projects={projects}
          onClose={handleCloseAddProject}
          onRegistered={handleRegisteredProject}
        />
      ) : null}
      {isCreateOpen ? (
        <AddWorktreeDialog
          project={createTargetProject ?? activeProject}
          onClose={handleCloseCreateWorktree}
          onCreated={async (worktree) => {
            const owner = createTargetProject ?? activeProject;
            if (owner.workspaceId !== activeProject.workspaceId) {
              handleSelectProject(owner);
              setPendingWorktreePath(worktree.path);
              return;
            }
            await refreshWorktrees();
            await ensureTabForWorktree(worktree).catch(reportRuntimeError);
          }}
        />
      ) : null}
      {deleteTarget ? (
        <WorktreeDeleteDialog
          workspaceId={activeProject.workspaceId}
          worktree={deleteTarget}
          onClose={handleCloseDeleteTarget}
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
      {pendingTabClose ? (
        <ConfirmCloseTabDialog
          tabLabel={pendingTabClose.label}
          onCancel={handleCancelTabClose}
          onConfirm={handleConfirmTabClose}
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
