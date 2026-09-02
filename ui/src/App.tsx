import { PanelLeft } from "lucide-react";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { STARTUP_TIMEOUT_MS } from "./lib/startupTimeout";
import { withTimeout } from "./lib/withTimeout";

import { CommandPalette } from "./components/CommandPalette";
import { EmptyWorkspaceView } from "./components/EmptyWorkspaceView";
import { AddProjectDialog, AddWorktreeDialog } from "./components/ProjectDialogs";
import { Sidebar } from "./components/Sidebar";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { ConfirmCloseTabDialog } from "./components/ConfirmCloseTabDialog";
import { TerminalLinkActions } from "./components/TerminalLinkActions";
import { Toaster, toast } from "./components/ui/sonner";
import { IconButton } from "./components/ui/IconButton";
import { useApplyAppearanceSettings } from "./lib/appearanceSettings";
import { workspaceName } from "./lib/branchFilter";
import { newBrowserTabUrl } from "./lib/browserSettings";
import { BROWSER_SHORTCUT_EVENT, onBrowserOpenRequested, type BrowserShortcutAction } from "./lib/browserTauri";
import { useGeneralSettings } from "./lib/generalSettings";
import { NotificationCoordinator, isWindowForegroundFocused } from "./lib/notificationCoordinator";
import { getNativeWindowFocused, startNativeWindowFocusTracking } from "./lib/nativeWindowFocus";
import type { TerminalActivityState } from "./lib/activity";
import { serializeWorkspaceState } from "./lib/sessionPersistence";
import { isMacShortcutPlatform, useShortcuts } from "./lib/shortcuts";
import { initUpdateToasts } from "./lib/updateToast";
import {
  AGENTS_SETTINGS_CHANGED_EVENT,
  detectionTargets,
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
  onSelectWorktreeMenu,
  onNewTerminalTabMenu,
  onRemoteSelectionRequested,
  publishFocusedTerminal,
  registerProject,
  saveSession,
  setBadgeCount,
  spawnTerminal,
  writeTerminal,
  bootTrace,
  listenDagRunUpdated,
  watchDagProject,
  type AgentDetection,
  type FocusedTerminalPayload,
  type RegisteredProject,
  type RemoteSelectionRequestedPayload,
} from "./lib/tauri";
import { reconnectAgentSession } from "./lib/agentReconnect";
import { createAppReconnectDependencies } from "./lib/appReconnectDependencies";
import { replaceExitedShellSession } from "./lib/shellReplacement";
import { enqueueStrictPersistence } from "./lib/persistenceQueue";
import { workspaceReducer } from "./state/workspaceStore";
import { dagStore } from "./state/dagStore";
import type { PersistedWorkspaceSession } from "./lib/types";
import { ensureTerminalEvents } from "./lib/terminalEvents";
import { useTerminalSettings } from "./lib/terminalSettings";
import { resolveWorktreeOwnerId } from "./lib/worktreeOwnership";
import { switchDebug } from "./lib/switchDebug";
import { useInactiveProjectWorktrees } from "./state/inactiveProjectWorktrees";
import {
  worktreeIdentity,
  type DirtyState,
  type WorkspaceTab,
  type Worktree,
} from "./lib/types";
import { checkForUpdate, registerWindowCloseGuard } from "./lib/updater";
import { collectLeafIds, type PaneDirection } from "./state/paneTree";
import { useBrowserSessionHydration } from "./state/browserSessionHydration";
import { preloadWorkspaceSnapshots, useWorkspaceRestore } from "./state/workspaceRestore";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { useWorkspaceStore, type WorkspaceState } from "./state/workspaceStore";

export { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY };
const DEFAULT_PROJECT: RegisteredProject = { workspaceId: DEFAULT_WORKSPACE_ID, repoRoot: ".", gitRoot: null };
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

function recoverProjectBootstrap(session: PersistedWorkspaceSession | null): ProjectBootstrap | null {
  if (!session) return null;

  const projects = Object.values(session.workspaces).reduce<RegisteredProject[]>((recovered, workspace) => {
    if (!workspace.workspaceId || !workspace.repoRoot) return recovered;
    if (!recovered.some((project) => project.workspaceId === workspace.workspaceId)) {
      recovered.push({ workspaceId: workspace.workspaceId, repoRoot: workspace.repoRoot });
    }
    return recovered;
  }, []);
  if (projects.length === 0) return null;

  const activeProjectId = projects.some((project) => project.workspaceId === session.activeWorkspaceId)
    ? session.activeWorkspaceId
    : projects[0].workspaceId;
  return { projects, activeProjectId };
}

function mergeRecoveredProjectBootstrap(
  stored: ProjectBootstrap,
  recovered: ProjectBootstrap | null,
  startup: RegisteredProject,
): ProjectBootstrap {
  if (!recovered) return stored;

  const isReducedToStartup =
    stored.projects.length === 1 &&
    stored.projects[0].workspaceId === startup.workspaceId &&
    stored.projects[0].repoRoot === startup.repoRoot &&
    recovered.projects.some((project) => project.workspaceId !== startup.workspaceId);
  const isRepresentedSingleProject =
    stored.projects.length === 1 &&
    recovered.projects.length > 1 &&
    recovered.projects.some((project) => project.workspaceId === stored.projects[0].workspaceId);
  const isUninitialized =
    stored.projects.length === 1 &&
    stored.projects[0].workspaceId === DEFAULT_WORKSPACE_ID &&
    stored.projects[0].repoRoot === ".";
  if (!isUninitialized && !isReducedToStartup && !isRepresentedSingleProject) return stored;

  const projects = stored.projects.filter(
    (project) => project.workspaceId !== DEFAULT_WORKSPACE_ID || (project.repoRoot !== "." && project.repoRoot !== ""),
  );
  for (const recoveredProject of recovered.projects) {
    if (!projects.some((project) => project.workspaceId === recoveredProject.workspaceId)) {
      projects.push(recoveredProject);
    }
  }

  const activeProjectId = projects.some((project) => project.workspaceId === recovered.activeProjectId)
    ? recovered.activeProjectId
    : (projects[0]?.workspaceId ?? stored.activeProjectId);
  return { projects, activeProjectId };
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
  useApplyAppearanceSettings();
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const [bootstrap, setBootstrap] = useState<ProjectBootstrap | null>(() =>
    isNativeRuntime ? null : loadProjectBootstrap(),
  );

  useEffect(() => {
    if (isNativeRuntime) void checkForUpdate();
  }, [isNativeRuntime]);

  useEffect(() => {
    if (!isNativeRuntime) {
      return;
    }
    let cancelled = false;
    void bootTrace("initial.start");
    void withTimeout(getInitialProject(), STARTUP_TIMEOUT_MS, "cmd_project_initial")
      .then(async (startup) => {
        void bootTrace("initial.ok");
        const storedBootstrap = loadProjectBootstrap();
        const savedSession = await loadSession().catch(() => null);
        const recovered = recoverProjectBootstrap(savedSession);
        const prepared = canonicalizeProjectBootstrap(
          mergeRecoveredProjectBootstrap(storedBootstrap, recovered, startup),
          startup,
        );
        if (recovered && prepared !== storedBootstrap) {
          persistProjects(prepared.projects);
          persistActiveProjectId(prepared.activeProjectId);
        }
        await preloadWorkspaceSnapshots(
          prepared.projects.map((project) => project.workspaceId),
          async () => savedSession,
        ).catch(
          (error) => {
            console.warn("Workspace session preload skipped:", error);
          },
        );
        if (!cancelled) setBootstrap(prepared);
      })
      .catch((error) => {
        void bootTrace("initial.error", { message: String(error).slice(0, 200) });
      });
    return () => {
      cancelled = true;
    };
  }, [isNativeRuntime]);

  if (!bootstrap) return <div className="h-screen w-screen bg-background" aria-label="Initializing project" />;

  return (
    <WorkspaceApp
      key={`${bootstrap.activeProjectId}:${bootstrap.projects.map((project) => project.workspaceId).join(",")}`}
      initialProjects={bootstrap.projects}
      initialActiveProjectId={bootstrap.activeProjectId}
    />
  );
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

  // The remote lists every terminal pane regardless of desktop focus, so a browser tab or an
  // absent focus only empties the focus fields below - it never hides the inventory.
  const focusedCandidate = activeTabId
    ? state.layout.tabs.find((tab) => tab.id === activeTabId)
    : undefined;
  const focusedTab =
    focusedCandidate && focusedCandidate.kind !== "browser" ? focusedCandidate : null;

  const tabLayout = focusedTab ? state.layout.layoutsByTabId?.[focusedTab.id] : undefined;
  const activeLeafId =
    tabLayout?.activeLeafId ?? (tabLayout?.root ? collectLeafIds(tabLayout.root)[0] : null);
  const localSessionId = focusedTab
    ? (activeLeafId && tabLayout?.sessionIdsByLeafId?.[activeLeafId]) || focusedTab.sessionId
    : null;
  const session = localSessionId ? state.sessions[localSessionId] : undefined;

  const sessionWorktreePath = session?.worktreePath ?? session?.cwd;
  const foundWorktree = sessionWorktreePath
    ? state.worktrees.find((wt) => wt.path === sessionWorktreePath)
    : (state.worktrees.find((wt) => wt.path === state.activeWorktreePath) ?? null);

  const ident = foundWorktree ? worktreeIdentity(foundWorktree) : null;
  const worktreeSlug = ident?.slug ?? null;
  const worktreeLabel = foundWorktree?.branch
    ? foundWorktree.branch.replace(/^refs\/heads\//, "")
    : (focusedTab?.label ?? null);

  const terminalTabs = state.layout.tabs.flatMap((tab) => {
    if (tab.kind === "browser") return [];
    const layout = state.layout.layoutsByTabId?.[tab.id];
    const leafIds = layout?.root ? collectLeafIds(layout.root) : [];
    const paneSessionIds = leafIds.length > 0
      ? leafIds.map((leafId) => ({ leafId, sessionId: layout?.sessionIdsByLeafId?.[leafId] ?? "" }))
      : [{ leafId: null, sessionId: tab.sessionId }];
    const multiPane = paneSessionIds.length > 1;

    return paneSessionIds.map(({ leafId, sessionId }, index) => {
      const paneSession = sessionId ? state.sessions[sessionId] : undefined;
      const backendSessionId = paneSession?.backendSessionId;
      const panePath = paneSession?.worktreePath ?? paneSession?.cwd;
      // Each pane carries its own worktree so the remote can select across worktrees instead of
      // guessing from whichever context is currently mirrored.
      const paneWorktree = panePath ? state.worktrees.find((wt) => wt.path === panePath) : undefined;
      const paneSlug = (paneWorktree ? worktreeIdentity(paneWorktree) : null)?.slug ?? null;
      const paneLabel = paneWorktree?.branch
        ? paneWorktree.branch.replace(/^refs\/heads\//, "")
        : null;

      const activity = sessionId ? state.activityBySessionId?.[sessionId] : undefined;
      const baseLabel = remoteTabLabel(tab.label);
      return {
        id: remotePaneId(tab.id, multiPane ? leafId : null),
        label: multiPane ? `${baseLabel} (${index + 1})` : baseLabel,
        ...(activity?.state ? { activityState: activity.state } : {}),
        ...(activity?.agentType ? { agentType: activity.agentType } : {}),
        ...(paneSlug ? { worktreeSlug: paneSlug } : {}),
        ...(paneLabel ? { worktreeLabel: paneLabel } : {}),
        ...(backendSessionId ? { sessionId: backendSessionId } : {}),
      };
    });
  });

  if (terminalTabs.length === 0) return null;

  const focusedLayout = focusedTab ? state.layout.layoutsByTabId?.[focusedTab.id] : undefined;
  const focusedLeafIds = focusedLayout?.root ? collectLeafIds(focusedLayout.root) : [];
  const focusedEntryId = focusedTab
    ? remotePaneId(focusedTab.id, focusedLeafIds.length > 1 ? activeLeafId : null)
    : null;

  return {
    workspaceId,
    worktreeSlug: worktreeSlug ?? null,
    worktreeLabel: worktreeLabel ?? null,
    backendSessionId: session?.backendSessionId ?? null,
    activeTabId: focusedEntryId,
    tabId: focusedEntryId,
    tabs: terminalTabs,
    terminalTabs,
  };
}

/** Remote entries address a pane, not just a tab, so a split tab exposes each PTY separately. */
const REMOTE_PANE_SEPARATOR = "::";

export function remotePaneId(tabId: string, leafId: string | null): string {
  return leafId ? `${tabId}${REMOTE_PANE_SEPARATOR}${leafId}` : tabId;
}

export function parseRemotePaneId(entryId: string): { tabId: string; leafId: string | null } {
  const index = entryId.indexOf(REMOTE_PANE_SEPARATOR);
  if (index < 0) return { tabId: entryId, leafId: null };
  return {
    tabId: entryId.slice(0, index),
    leafId: entryId.slice(index + REMOTE_PANE_SEPARATOR.length) || null,
  };
}

function remoteTabLabel(label: string): string {
  const trimmed = label.trim();
  return /^(?:~[/\\]|[/\\]|[a-zA-Z]:[/\\]|file:)/.test(trimmed) ? "Terminal" : trimmed || "Terminal";
}

function isTerminalTabInWorktree(
  state: WorkspaceState,
  worktree: Worktree,
  entryId: string | null,
): boolean {
  if (!entryId) return true;
  const { tabId, leafId } = parseRemotePaneId(entryId);
  const tab = state.layout.tabs.find((candidate) => candidate.id === tabId);
  if (!tab || tab.kind === "browser") return false;
  // A leaf-addressed entry names one pane of a split tab, so the worktree check must
  // follow that pane's own PTY instead of the tab's primary session.
  const sessionId = leafId
    ? state.layout.layoutsByTabId?.[tabId]?.sessionIdsByLeafId?.[leafId]
    : tab.sessionId;
  if (!sessionId) return false;
  const session = state.sessions[sessionId];
  return (session?.worktreePath ?? session?.cwd) === worktree.path;
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

  useEffect(() => {
    startNativeWindowFocusTracking();
  }, []);
  const [registeredProjectId, setRegisteredProjectId] = useState<string | null>(null);
  const [registrationAttempt, setRegistrationAttempt] = useState(0);
  const registeredProjectIdRef = useRef<string | null>(null);
  registeredProjectIdRef.current = registeredProjectId;
  const [pendingBackendRecovery, setPendingBackendRecovery] = useState<{ workspaceId: string; sessionIds: string[] } | null>(
    null,
  );
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
        const results = await detectAgents(detectionTargets(agentSettings));
        if (!cancelled) setAgentDetections(results);
      } catch (error) {
        console.warn("Failed to detect agents:", error);
      }
    }
    void runDetection();
    return () => {
      cancelled = true;
    };
  }, [agentSettings]);

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
    activityNotificationTargets,    markTabUnread,
    markWorktreeUnread,
    subscribeTerminalBell,
    openTab,
    createBrowserTab,
    duplicateBrowserTab,
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
    ensureSessionBackends,
    dispatchWorkspaceAction,
  } = useWorkspaceStore({ workspaceId: activeProject.workspaceId });
  useBrowserSessionHydration(state, activeProject.workspaceId);

  const stateRef = useRef(state);
  stateRef.current = state;

  const markTabUnreadRef = useRef(markTabUnread);
  markTabUnreadRef.current = markTabUnread;
  const markWorktreeUnreadRef = useRef(markWorktreeUnread);
  markWorktreeUnreadRef.current = markWorktreeUnread;

  const coordinatorRef = useRef<NotificationCoordinator | null>(null);
  if (!coordinatorRef.current) {
    coordinatorRef.current = new NotificationCoordinator({
      onMarkTabUnread: (tabId) => markTabUnreadRef.current?.(tabId),
      onMarkWorktreeUnread: (path) => markWorktreeUnreadRef.current?.(path),
      isWindowFocused: () => getNativeWindowFocused() ?? isWindowForegroundFocused(),
    });
  }

  const activityNotificationTargetsRef = useRef(activityNotificationTargets);
  activityNotificationTargetsRef.current = activityNotificationTargets;

  const lastAgentStatesRef = useRef<Map<string, TerminalActivityState>>(new Map());

  useEffect(() => {
    const targets = activityNotificationTargets ?? [];
    for (const target of targets) {
      const previousState = lastAgentStatesRef.current.get(target.sessionId);
      if (previousState !== target.state) {
        lastAgentStatesRef.current.set(target.sessionId, target.state);
        coordinatorRef.current?.handleAgentStateChange({
          sessionId: target.sessionId,
          tabId: target.tabId,
          worktreePath: target.worktreePath,
          worktreeLabel: target.worktreeLabel,
          agentLabel: target.agentLabel,
          terminalTitle: target.terminalTitle,
          nextState: target.state,
        });
      }
    }
  }, [activityNotificationTargets]);

  const handleTerminalBell = useCallback((sessionId: string, tabId: string) => {
    const targets = activityNotificationTargetsRef.current ?? [];
    const target = targets.find(
      (candidate) => candidate.sessionId === sessionId,
    );
    let worktreePath = target?.worktreePath;
    let worktreeLabel = target?.worktreeLabel;
    let terminalTitle = target?.terminalTitle;

    if (!target) {
      const session = stateRef.current.sessions[sessionId];
      const fallbackWorktreePath = session?.worktreePath ?? session?.cwd ?? "";
      const worktree = stateRef.current.worktrees.find(
        (candidate) => candidate.path === fallbackWorktreePath,
      );
      worktreePath = fallbackWorktreePath;
      worktreeLabel = worktree ? workspaceName(worktree) : "";
      terminalTitle = "";
    }

    coordinatorRef.current?.handleTerminalBell({
      sessionId,
      tabId,
      worktreePath,
      worktreeLabel,
      terminalTitle,
    });
  }, []);

  // The bell arrives from the store's global native subscription, not from a pane prop: only the
  // active tab's panes are mounted, and a bell in the tab you are watching is not what notifies.
  useEffect(() => subscribeTerminalBell(handleTerminalBell), [handleTerminalBell, subscribeTerminalBell]);
  useEffect(() => {
    switchDebug("workspace.render", {
      activeProjectId: activeProject.workspaceId,
      stateWorkspaceId: state.workspaceId ?? null,
      activeWorktreePath: state.activeWorktreePath,
      worktreeCount: state.worktrees.length,
      tabCount: state.layout.tabs.length,
      tabIds: state.layout.tabs.map((tab) => tab.id),
      sessionCount: Object.keys(state.sessions).length,
      registeredProjectId,
      recoveredFromHmr,
    });
  }, [
    activeProject.workspaceId,
    recoveredFromHmr,
    registeredProjectId,
    state.activeWorktreePath,
    state.layout.tabs,
    state.sessions,
    state.workspaceId,
    state.worktrees.length,
  ]);
  const plainRootWorktree = useMemo(
    () =>
      activeProject.gitRoot === null
        ? {
            path: activeProject.repoRoot,
            head: "",
            branch: null,
            bare: false,
            detached: false,
            locked: null,
            prunable: null,
          }
        : null,
    [activeProject.gitRoot, activeProject.repoRoot],
  );

  const { runtimeError, refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({
    workspaceId: activeProject.workspaceId,
    activeWorktreePath: state.activeWorktreePath,
    syncWorktrees,
    ensureTabForWorktree,
    // Plain (non-Git) projects have no git worktrees; their folder root acts
    // as the primary "worktree" so a terminal opens there like anywhere else.
    plainRootWorktree,
    registeredWorkspaceId: registeredProjectId,
  });

  const inactiveProjectWorktrees = useInactiveProjectWorktrees(
    projects,
    activeProject.workspaceId,
    state.worktrees,
  );
  const inactiveProjectWorktreesRef = useRef(inactiveProjectWorktrees);

  // Dag journals: watch every known project root and worktree so any omo graph run
  // anywhere lights up the activity badge, regardless of which cwd the app started in.
  const dagWatchedPathsRef = useRef<Set<string>>(new Set());
  const activeWorktreePathsKey = state.worktrees.map((worktree) => worktree.path).join("\n");
  const projectRootsKey = projects.map((project) => project.repoRoot).join("\n");
  useEffect(() => {
    const paths = new Set<string>();
    for (const project of projectsRef.current) paths.add(project.repoRoot);
    for (const worktrees of Object.values(inactiveProjectWorktreesRef.current)) {
      for (const worktree of worktrees) paths.add(worktree.path);
    }
    for (const path of activeWorktreePathsKey.split("\n")) {
      if (path.length > 0) paths.add(path);
    }
    for (const path of paths) {
      if (dagWatchedPathsRef.current.has(path)) continue;
      dagWatchedPathsRef.current.add(path);
      void watchDagProject(path)
        // Events are tagged with the backend's canonical path, so hydrate under
        // that same key or the pane lookup would never find these runs.
        .then(({ projectPath, runs }) => {
          for (const snapshot of runs) dagStore.applySnapshot(projectPath, snapshot);
        })
        .catch(() => undefined);
    }
  }, [inactiveProjectWorktrees, activeWorktreePathsKey, projectRootsKey]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listenDagRunUpdated((event) => {
      dagStore.applySnapshot(event.projectPath, event.snapshot);
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  inactiveProjectWorktreesRef.current = inactiveProjectWorktrees;

  useEffect(() => {
    return initUpdateToasts();
  }, []);

  useEffect(() => {
    if (!runtimeError) return;
    const text = `${runtimeError.code}: ${runtimeError.message}`;
    const details =
      runtimeError.details && Object.keys(runtimeError.details).length > 0
        ? JSON.stringify(runtimeError.details, null, 2)
        : undefined;
    const clipboardText = details ? `${text}\n${details}` : text;
    toast.error(text, {
      description: details,
      duration: Infinity,
      action: {
        label: "Copy",
        onClick: () => {
          void navigator.clipboard
            .writeText(clipboardText)
            .then(() => {
              toast.success("Copied error to clipboard");
            })
            .catch(() => {});
        },
      },
    });
  }, [runtimeError]);

  useEffect(() => {
    let cancelled = false;
    setRegisteredProjectId(null);
    switchDebug("project.register.start", {
      workspaceId: activeProject.workspaceId,
      repoRoot: activeProject.repoRoot,
      registrationAttempt,
    });
    void registerProject({
      workspaceId: activeProject.workspaceId,
      repoPath: activeProject.repoRoot,
    })
      .then(async (registered) => {
        if (cancelled) {
          switchDebug("project.register.ignored", {
            requestedWorkspaceId: activeProject.workspaceId,
            registeredWorkspaceId: registered.workspaceId,
          });
          return;
        }
        // The backend owns one workspace ID per canonical root and returns the
        // existing project when this root is already registered under another
        // ID, so adopt that ID instead of keeping a stale alias.
        const adopted =
          registered.workspaceId !== activeProject.workspaceId && registered.repoRoot === activeProject.repoRoot;
        switchDebug("project.register.success", {
          requestedWorkspaceId: activeProject.workspaceId,
          registeredWorkspaceId: registered.workspaceId,
          adopted,
        });
        setProjects((current) => {
          if (
            !adopted &&
            current.some(
              (candidate) =>
                candidate.workspaceId === registered.workspaceId &&
                candidate.repoRoot === registered.repoRoot &&
                candidate.gitRoot === registered.gitRoot,
            )
          ) {
            return current;
          }
          const replaced = current.map((candidate) =>
            (adopted && candidate.workspaceId === activeProject.workspaceId) ||
            candidate.workspaceId === registered.workspaceId
              ? registered
              : candidate,
          );
          const next = replaced.filter(
            (candidate, index) =>
              replaced.findIndex((other) => other.workspaceId === candidate.workspaceId) === index,
          );
          persistProjects(next);
          return next;
        });
        if (adopted) {
          switchDebug("project.register.adopt", {
            fromWorkspaceId: activeProject.workspaceId,
            toWorkspaceId: registered.workspaceId,
          });
          setActiveProjectId(registered.workspaceId);
          persistActiveProjectId(registered.workspaceId);
          return;
        }
        switchDebug("project.register.refresh.start", {
          workspaceId: activeProject.workspaceId,
        });
        await refreshWorktrees({ allowCreate: false });
        if (cancelled) {
          switchDebug("project.register.refresh.ignored", {
            workspaceId: activeProject.workspaceId,
          });
          return;
        }
        switchDebug("project.register.ready", {
          workspaceId: activeProject.workspaceId,
        });
        setRegisteredProjectId(activeProject.workspaceId);
      })
      .catch((error) => {
        switchDebug("project.register.error", {
          workspaceId: activeProject.workspaceId,
          error: String(error),
          cancelled,
        });
        if (!cancelled) reportRuntimeError(error);
      });
    return () => {
      cancelled = true;
      switchDebug("project.register.cancel", {
        workspaceId: activeProject.workspaceId,
      });
    };
  }, [activeProject.repoRoot, activeProject.workspaceId, registrationAttempt, refreshWorktrees, reportRuntimeError]);

  // A failed registration leaves the runtime gated, so retry when the window
  // regains focus rather than staying empty until the app restarts.
  useEffect(() => {
    const retryRegistration = () => {
      if (registeredProjectIdRef.current === null) setRegistrationAttempt((attempt) => attempt + 1);
    };
    window.addEventListener("focus", retryRegistration);
    return () => window.removeEventListener("focus", retryRegistration);
  }, []);

  const restoreWorkspaceAndReconnect = useCallback(
    (restoredState: WorkspaceState) => {
      restoreWorkspace(restoredState);
      setPendingBackendRecovery({
        workspaceId: activeProjectRef.current.workspaceId,
        sessionIds: Object.values(restoredState.sessions)
          .filter((session) => session.backendSessionId === null)
          .filter((session) => !(session.lifecycle === "exited" && (session.agentType || session.providerSession || session.agentSessionId)))
          .map((session) => session.id),
      });
    },
    [restoreWorkspace],
  );

  // Initial session restore on startup & HMR recovery managed by coordinator.
  useWorkspaceRestore({
    workspaceId: activeProject.workspaceId,
    recoveredFromHmr,
    restoreWorkspace: restoreWorkspaceAndReconnect,
    enabled: registeredProjectId === activeProject.workspaceId,
  });

  useEffect(() => {
    if (registeredProjectId !== activeProject.workspaceId || pendingBackendRecovery === null) return;
    setPendingBackendRecovery(null);
    // Recovery targets sessions of the project that was restored; after a
    // switch those IDs belong to another workspace and must not be respawned.
    if (pendingBackendRecovery.workspaceId !== activeProject.workspaceId) return;
    if (pendingBackendRecovery.sessionIds.length === 0) return;
    void ensureSessionBackends(pendingBackendRecovery.sessionIds).catch(reportRuntimeError);
  }, [activeProject.workspaceId, ensureSessionBackends, pendingBackendRecovery, registeredProjectId, reportRuntimeError]);

  const saveChainRef = useRef<Promise<void>>(Promise.resolve());
  const persistSessionStrict = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    return enqueueStrictPersistence(saveChainRef, async () => {
        const existing = await loadSession().catch(() => null);
        const session = serializeWorkspaceState(
          workspaceId,
          repoRoot,
          currentState,
          existing,
        );
        await saveSession(session);
    });
  }, []);
  const persistSession = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    return persistSessionStrict(workspaceId, repoRoot, currentState).catch((error) => {
      console.error("Failed to save workspace session:", error);
    });
  }, [persistSessionStrict]);

  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      const snapshot = stateRef.current;
      const target = activeProjectRef.current;
      if (snapshot.workspaceId !== undefined && snapshot.workspaceId !== target.workspaceId) return;
      await persistSession(target.workspaceId, target.repoRoot, snapshot);
    });
    return unregister;
  }, [persistSession]);

  useEffect(() => {
    const hasTabs =
      state.layout.tabs.length > 0 ||
      Object.values(state.worktreeLayouts ?? {}).some((l) => l.tabs.length > 0);
    if (state.worktrees.length === 0 || !hasTabs) return;
    // On the render that switches projects, `state` still holds the outgoing
    // project's data. Saving it under the incoming id would overwrite that
    // project's persisted session, so save it under its own owner instead of
    // discarding the newest state of the project being left.
    const owner = state.workspaceId ?? activeProject.workspaceId;
    if (owner !== activeProject.workspaceId) {
      const outgoing = projectsRef.current.find((project) => project.workspaceId === owner);
      if (outgoing) void persistSession(owner, outgoing.repoRoot, state);
      return;
    }
    const timer = setTimeout(() => {
      const snapshot = stateRef.current;
      if (snapshot.workspaceId !== undefined && snapshot.workspaceId !== activeProject.workspaceId) return;
      void persistSession(activeProject.workspaceId, activeProject.repoRoot, snapshot);
    }, 500);
    return () => clearTimeout(timer);
  }, [
    activeProject.repoRoot,
    activeProject.workspaceId,
    persistSession,
    state.layout,
    state.worktreeLayouts,
    state.worktrees,
    state.activeWorktreePath,
    state.sessions,
    state.workspaceId,
  ]);

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
  const [pendingRemoteSlug, setPendingRemoteSlug] = useState<{
    workspaceId: string;
    slug?: string | null;
    tabId?: string | null;
  } | null>(null);

  const focusedTerminalPayload = useMemo(
    () => deriveFocusedTerminal(activeProject.workspaceId, state),
    [activeProject.workspaceId, state],
  );

  useEffect(() => {
    void Promise.resolve(publishFocusedTerminal(focusedTerminalPayload)).catch(reportRuntimeError);
  }, [focusedTerminalPayload, reportRuntimeError]);

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
      const current = activeProjectRef.current;
      if (project.workspaceId === current.workspaceId) {
        switchDebug("project.select.noop", {
          workspaceId: project.workspaceId,
        });
        return;
      }
      const snapshot = stateRef.current;
      switchDebug("project.select.requested", {
        fromWorkspaceId: current.workspaceId,
        toWorkspaceId: project.workspaceId,
        outgoingStateWorkspaceId: snapshot.workspaceId ?? null,
        outgoingActiveWorktreePath: snapshot.activeWorktreePath,
        outgoingTabCount: snapshot.layout.tabs.length,
        outgoingSessionCount: Object.keys(snapshot.sessions).length,
      });
      setActiveProjectId(project.workspaceId);
      persistActiveProjectId(project.workspaceId);
      setWorktreeStatuses({});
      setDeleteTarget(null);
      setPendingWorktreePath(null);
    },
    [],
  );

  const handleReorderProjects = useCallback((orderedWorkspaceIds: string[]) => {
    setProjects((current) => {
      const byId = new Map(current.map((project) => [project.workspaceId, project]));
      const seen = new Set<string>();
      const next: RegisteredProject[] = [];
      for (const workspaceId of orderedWorkspaceIds) {
        const project = byId.get(workspaceId);
        if (!project || seen.has(workspaceId)) continue;
        seen.add(workspaceId);
        next.push(project);
      }
      for (const project of current) {
        if (seen.has(project.workspaceId)) continue;
        seen.add(project.workspaceId);
        next.push(project);
      }
      if (next.every((project, index) => project === current[index])) return current;
      projectsRef.current = next;
      persistProjects(next);
      return next;
    });
  }, []);

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
      const ownerId = resolveWorktreeOwnerId(worktree, projectsRef.current);
      const owner = ownerId
        ? projectsRef.current.find((project) => project.workspaceId === ownerId)
        : undefined;
      switchDebug("worktree.select.requested", {
        currentWorkspaceId: activeProjectRef.current.workspaceId,
        ownerWorkspaceId: owner?.workspaceId ?? null,
        worktreePath: worktree.path,
        pendingCrossProject: Boolean(
          owner && owner.workspaceId !== activeProjectRef.current.workspaceId,
        ),
      });
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
    if (!target) {
      switchDebug("worktree.select.pending", {
        workspaceId: activeProject.workspaceId,
        pendingWorktreePath,
        availableWorktreePaths: state.worktrees.map((worktree) => worktree.path),
      });
      return;
    }
    switchDebug("worktree.select.pending.resolved", {
      workspaceId: activeProject.workspaceId,
      worktreePath: target.path,
      tabCount: state.layout.tabs.length,
    });
    setPendingWorktreePath(null);
    void ensureTabForWorktree(target).catch(reportRuntimeError);
  }, [
    activeProject.workspaceId,
    ensureTabForWorktree,
    pendingWorktreePath,
    reportRuntimeError,
    state.layout.tabs.length,
    state.worktrees,
  ]);

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

  const activateRemoteEntry = useCallback(
    (entryId: string) => {
      const { tabId, leafId } = parseRemotePaneId(entryId);
      activateTab(tabId);
      if (leafId) focusPane(tabId, leafId);
    },
    [activateTab, focusPane],
  );

  useEffect(() => {
    if (!pendingRemoteSlug) return;
    // The slug was queued for one project; after a switch elsewhere it must not
    // open a same-named worktree in whichever project is now active.
    if (pendingRemoteSlug.workspaceId !== activeProject.workspaceId) {
      setPendingRemoteSlug(null);
      return;
    }
    const target = pendingRemoteSlug.slug
      ? matchWorktreeBySlug(state.worktrees, pendingRemoteSlug.slug)
      : (state.worktrees.find((wt) => worktreeIdentity(wt) === null) ??
         state.worktrees.find((wt) => wt.path === activeProject.repoRoot) ??
         state.worktrees[0]);
    if (!target) return;
    const requestedEntryId = pendingRemoteSlug.tabId ?? null;
    setPendingRemoteSlug(null);
    if (!isTerminalTabInWorktree(state, target, requestedEntryId)) return;
    void Promise.resolve(ensureTabForWorktree(target))
      .then(() => {
        if (requestedEntryId) {
          activateRemoteEntry(requestedEntryId);
        }
      })
      .catch(reportRuntimeError);
  }, [activeProject.repoRoot, activeProject.workspaceId, activateRemoteEntry, ensureTabForWorktree, pendingRemoteSlug, reportRuntimeError, state.worktrees]);

  const handleRemoteSelectionRequested = useCallback(
    (payload: RemoteSelectionRequestedPayload) => {
      if (!payload || !payload.workspaceId) return;
      const targetProject = projectsRef.current.find((p) => p.workspaceId === payload.workspaceId);
      const isCurrentProject = activeProjectRef.current.workspaceId === payload.workspaceId;
      const requestedEntryId = payload.tabId ?? payload.activeTabId ?? null;

      if (isCurrentProject) {
        const targetWorktree = payload.worktreeSlug
          ? matchWorktreeBySlug(stateRef.current.worktrees, payload.worktreeSlug)
          : (stateRef.current.worktrees.find((wt) => worktreeIdentity(wt) === null) ??
             stateRef.current.worktrees.find((wt) => wt.path === activeProjectRef.current.repoRoot) ??
             stateRef.current.worktrees[0]);

        if (targetWorktree) {
          if (!isTerminalTabInWorktree(stateRef.current, targetWorktree, requestedEntryId)) return;
          if (targetWorktree.path === stateRef.current.activeWorktreePath) {
            if (requestedEntryId) {
              activateRemoteEntry(requestedEntryId);
            }
          } else {
            void Promise.resolve(ensureTabForWorktree(targetWorktree))
              .then(() => {
                if (requestedEntryId) {
                  activateRemoteEntry(requestedEntryId);
                }
              })
              .catch(reportRuntimeError);
          }
        }
      } else if (targetProject) {
        handleSelectProject(targetProject);
        setPendingRemoteSlug({
          workspaceId: targetProject.workspaceId,
          slug: payload.worktreeSlug ?? null,
          tabId: requestedEntryId,
        });
      }
    },
    [activateRemoteEntry, ensureTabForWorktree, handleSelectProject, reportRuntimeError],
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

  const handleAddTerminalTab = useCallback((shell?: string) => {
    const activeWt = activeWorktreeRef.current;
    if (!activeWt) return;
    void openTab(activeWt, undefined, undefined, shell).catch(reportRuntimeError);
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

  const handleCloseActiveSurface = useCallback(() => {
    const currentState = stateRef.current;
    const activeTabId = currentState.layout.activeTabId;
    if (!activeTabId) return;

    const activeTab = currentState.layout.tabs.find((tab) => tab.id === activeTabId);
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTabId];
    if (activeTab?.kind === "terminal" && activeLayout?.root.type === "split") {
      const activeLeafId = activeLayout.activeLeafId ?? collectLeafIds(activeLayout.root)[0];
      if (activeLeafId) {
        void closePane(activeTabId, activeLeafId).catch(reportRuntimeError);
        return;
      }
    }

    handleCloseTab(activeTabId);
  }, [closePane, handleCloseTab, reportRuntimeError]);

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
      const visible = listVisibleWorktrees(
        projectsRef.current,
        stateRef.current.worktrees,
        activeProjectRef.current.workspaceId,
        inactiveProjectWorktreesRef.current,
      );
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
    (url?: string, profileId?: string) => {
      const targetUrl = url ?? newBrowserTabUrl();
      const task = profileId
        ? createBrowserTab(targetUrl, undefined, { profileId })
        : createBrowserTab(targetUrl);
      void task.catch(reportRuntimeError);
    },
    [createBrowserTab, reportRuntimeError],
  );


  const handleDuplicateBrowserTab = useCallback(
    (tabId: string, profileId?: string) => {
      void duplicateBrowserTab(tabId, profileId).catch(reportRuntimeError);
    },
    [duplicateBrowserTab, reportRuntimeError],
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
      handleCloseActiveSurface();
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleCloseActiveSurface]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserOpenRequested((payload) => {
      void createBrowserTab(payload.targetUrl, undefined, {
        profileId: payload.profileId,
        worktreePath: payload.worktreePath ?? undefined,
      }).catch(reportRuntimeError);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [createBrowserTab, reportRuntimeError]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    // Cmd+1..9 never reaches the webview because the macOS Window menu claims it,
    // so the native key monitor forwards the digit as an event instead.
    void onSelectWorktreeMenu((digit) => {
      handleSelectWorktreeByIndex(digit - 1);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleSelectWorktreeByIndex]);

  const activeShortcutTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? null;
  const browserShortcutsActive = activeShortcutTab?.kind === "browser";
  const dispatchBrowserShortcut = (action: BrowserShortcutAction) => {
    window.dispatchEvent(new CustomEvent(BROWSER_SHORTCUT_EVENT, { detail: { action } }));
  };

  const shortcutHandlers = useMemo(
    () => ({
      "tab.newTerminal": handleAddTerminalTab,
      "tab.newBrowser": () => void createBrowserTab(newBrowserTabUrl()).catch(reportRuntimeError),
      "tab.close": handleCloseActiveSurface,
      "browser.focusAddress": browserShortcutsActive ? () => dispatchBrowserShortcut("focus-address") : undefined,
      "browser.reload": browserShortcutsActive ? () => dispatchBrowserShortcut("reload") : undefined,
      "browser.back": browserShortcutsActive ? () => dispatchBrowserShortcut("back") : undefined,
      "browser.forward": browserShortcutsActive ? () => dispatchBrowserShortcut("forward") : undefined,
      "browser.find": browserShortcutsActive ? () => dispatchBrowserShortcut("find") : undefined,
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
      "terminal.focusNext": browserShortcutsActive ? undefined : () => handleCyclePaneFocus(1),
      "terminal.focusPrevious": browserShortcutsActive ? undefined : () => handleCyclePaneFocus(-1),
      "terminal.search": browserShortcutsActive ? undefined : handleOpenTerminalSearch,
      "sidebar.left.toggle": toggleSidebar,
      "commandPalette.open": handleOpenCommandPalette,
      "settings.toggle": handleToggleSettings,
      "zoom.in": browserShortcutsActive ? undefined : handleZoomIn,
      "zoom.out": browserShortcutsActive ? undefined : handleZoomOut,
      "zoom.reset": browserShortcutsActive ? undefined : handleZoomReset,
    }),
    [
      createBrowserTab,
      handleAddTerminalTab,
      handleCloseActiveSurface,
      handleCyclePaneFocus,
      browserShortcutsActive,
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
      <Toaster />
      <TerminalLinkActions />
      {isSidebarOpen ? (
        <Sidebar
          open={true}
          projects={projects}
          activeProjectId={activeProject.workspaceId}
          worktrees={state.worktrees}
          inactiveProjectWorktrees={inactiveProjectWorktrees}
          agents={agents}
          activePath={activeWorktree?.path || ""}
          statuses={worktreeStatuses}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          onSelectProject={handleSelectProject}
          onReorderProjects={handleReorderProjects}
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
            activityBySessionId={state.activityBySessionId}
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
            onDuplicateBrowserTab={handleDuplicateBrowserTab}
            onSplitPane={handleSplitPane}
            onClosePane={handleClosePane}
            onSetRatio={setPaneRatio}
            onSetGroupRatio={setTabGroupRatio}
            onSwapPanes={swapPanes}
            onFocusPane={focusPane}
            searchLeafId={searchLeafId}
            onCloseSearch={handleCloseSearch}
            onReconnectAgentSession={(sessionId) => {
              void reconnectAgentSession(sessionId, createAppReconnectDependencies({
                getSessions: () => stateRef.current.sessions,
                dispatch: dispatchWorkspaceAction,
                persist: async (result, localSession) => {
                  const current = stateRef.current;
                  const nextState = workspaceReducer(current, {
                    type: "REBIND_SESSION_BACKEND",
                    sessionId: localSession.id,
                    backendSessionId: result.sessionId,
                    cwd: result.session.cwd ?? localSession.cwd,
                    daemonEpoch: result.daemonEpoch,
                  });
                  await persistSessionStrict(activeProject.workspaceId, activeProject.repoRoot, nextState);
                },
              })).catch(reportRuntimeError);
            }}
            onOpenNewShell={(sessionId) => replaceExitedShellSession(sessionId, {
              getSessions: () => stateRef.current.sessions,
              dispatch: dispatchWorkspaceAction,
              persist: async (result, localSession) => {
                const current = stateRef.current;
                const nextState = workspaceReducer(current, {
                  type: "REBIND_SESSION_BACKEND",
                  sessionId: localSession.id,
                  backendSessionId: result.sessionId,
                  cwd: result.session.cwd ?? localSession.cwd,
                  daemonEpoch: result.daemonEpoch,
                });
                await persistSessionStrict(activeProject.workspaceId, activeProject.repoRoot, nextState);
              },
            }).then(() => undefined).catch((error) => {
              reportRuntimeError(error);
              throw error;
            })}
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
          <SettingsDialog open onClose={handleCloseSettings} />
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
    </div>
  );
}

function listVisibleWorktrees(
  projects: RegisteredProject[],
  worktrees: Worktree[],
  activeProjectId: string,
  inactiveProjectWorktrees: Record<string, Worktree[]> = {},
): Worktree[] {
  const collapsed = loadCollapsedProjectIds(projects, activeProjectId);
  // Cmd+N counts the rows the sidebar actually renders, so this must mirror
  // `groupWorktreesByProject`: inactive projects contribute their own listed
  // rows, and ownership comes from the shared resolver rather than a bare
  // branch identity that would attribute every branch-less row to the active
  // project.
  const visible: Worktree[] = [];

  for (const project of projects) {
    if (collapsed.has(project.workspaceId)) continue;
    const owned = worktrees.filter(
      (worktree) => resolveWorktreeOwnerId(worktree, projects, activeProjectId) === project.workspaceId,
    );
    const cached = inactiveProjectWorktrees[project.workspaceId] ?? [];
    let rows =
      project.workspaceId === activeProjectId
        ? (owned.length > 0 ? owned : cached)
        : [...cached, ...owned];
    if (project.gitRoot === null && rows.length === 0) {
      rows = [
        {
          path: project.repoRoot,
          head: "",
          branch: null,
          bare: false,
          detached: false,
          locked: null,
          prunable: null,
        },
      ];
    }
    for (const row of rows) {
      if (visible.some((candidate) => candidate.path === row.path)) continue;
      visible.push(row);
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
    const valid = parsed
      .filter(
        (project) => project && typeof project.workspaceId === "string" && typeof project.repoRoot === "string",
      )
      .map((project) => ({
        workspaceId: project.workspaceId,
        repoRoot: project.repoRoot,
        // Entries persisted before gitRoot existed can only be git projects
        // (the old backend rejected non-git folders), and their repoRoot was
        // already the canonical git root. Only an explicit null means non-git.
        gitRoot:
          typeof project.gitRoot === "string"
            ? project.gitRoot
            : project.gitRoot === null
              ? null
              : project.repoRoot,
      }));
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
