import { Inbox, PanelLeft } from "lucide-react";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { STARTUP_TIMEOUT_MS } from "./lib/startupTimeout";
import { withTimeout } from "./lib/withTimeout";

import { CommandPalette } from "./components/CommandPalette";
import { EmptyWorkspaceView } from "./components/EmptyWorkspaceView";
import { SshWorkspaceStatus } from "./components/SshWorkspaceStatus";
import { AddProjectDialog, AddWorktreeDialog, RemoveProjectDialog } from "./components/ProjectDialogs";
import { Sidebar, type SidebarAttention } from "./components/Sidebar";
import { ShortcutHints } from "./components/ShortcutHints";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { RemoteHostConnection } from "./remote/RemoteApp";
import { RemoteBrowserSharingIndicator } from "./components/RemoteBrowserSharingIndicator";
import { remoteHostStore, selectActiveHost } from "./state/remoteHostStore";
import { pairedHostInventory } from "./lib/pairedHostInventory";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { WorktreeDiskDialog } from "./components/WorktreeDiskDialog";
import { AgentHistoryDialog } from "./components/AgentHistoryDialog";
import { buildAttentionRows, liveActivityLookup, type AttentionRow } from "./features/ferryx/attention/attentionModel";
import type { AgentHistoryEntry } from "./lib/agentHistory";
import { ConfirmCloseTabDialog } from "./components/ConfirmCloseTabDialog";
import { TerminalLinkActions } from "./components/TerminalLinkActions";
import { TerminalFileLinkActions } from "./components/TerminalFileLinkActions";
import { Toaster, toast } from "./components/ui/sonner";
import { IconButton } from "./components/ui/IconButton";
import { copyTextToClipboard } from "./lib/clipboard";
import { useApplyAppearanceSettings } from "./lib/appearanceSettings";
import { workspaceName } from "./lib/branchFilter";
import { collectDagWatchRoots, isLocalDagProject, remoteProjectsWatchKey } from "./lib/dagWatchRoots";
import { loadBrowserSettings, newBrowserTabUrl } from "./lib/browserSettings";
import { BROWSER_SHORTCUT_EVENT, browserTabIdForBrowserId, getBrowserState, navigateBrowser, onBrowserLinkClicked, onBrowserOpenRequested, onBrowserPopupCloseRequested, onBrowserSessionCreated, onBrowserShortcutRequested, onBrowserTabSwitch, openExternalUrl, popupOpenerLink, reportBrowserAdoption, setBrowserZoom, browserTabSelectIndex, browserWorkspaceSelectIndex, type BrowserReloadOptions, type BrowserShortcutAction, type BrowserShortcutDomEvent } from "./lib/browserTauri";
import { listen } from "@tauri-apps/api/event";
import { CLI_OPEN_FILE_EVENT, parseCliOpenFilePayload } from "./lib/cliOpenFile";
import { registerBuiltInBrowserLinkOpener, registerWorktreePathOpener } from "./lib/linkRouting";
import { useGeneralSettings } from "./lib/generalSettings";
import { NotificationCoordinator, isWindowForegroundFocused } from "./lib/notificationCoordinator";
import { isNotificationTargetObserved, wireActivityRecording, wireBellRecording, type RecordingListener, type RecordingTarget } from "./lib/notificationCenter/activityRecording";
import { notificationCenterStore } from "./lib/notificationCenter/notificationCenterStore";
import { notificationEntryId } from "./lib/notificationCenter/types";
import { getNativeWindowFocused, startNativeWindowFocusTracking } from "./lib/nativeWindowFocus";
import { serializeWorkspaceState, sessionPersistenceKey } from "./lib/sessionPersistence";
import { setLocalSplitPersistence } from "./lib/localSplitLifecycle";
import { isMacShortcutPlatform, SHORTCUTS, useShortcuts } from "./lib/shortcuts";
import { initUpdateToasts } from "./lib/updateToast";
// Wave 3a cross-platform onboarding, release notes, and getting started checklist
import {
  OPEN_ONBOARDING_EVENT,
  dismissOnboarding,
  initialWizardStepIndex,
  loadOnboardingState,
  markOnboardingStepsCompleted,
  satisfiedOnboardingSteps,
  shouldAutoOpenOnboarding,
  visiblePermissionKeys,
  wizardSteps,
  type OnboardingContext,
  type OnboardingStepId,
} from "./lib/onboarding";
import {
  markWhatsNewSeen,
  resolveWhatsNew,
  startWhatsNewRecorder,
  type WhatsNewEntry,
} from "./lib/whatsNew";
import { showNotificationPermissionHint } from "./lib/notificationPermissionHint";
import { getCurrentVersion } from "./lib/updater";
import { GettingStartedChecklist } from "./components/onboarding/GettingStartedChecklist";
import { DaemonConnectionBanner } from "./components/DaemonConnectionBanner";
import type { SectionId } from "./components/settings/types";
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
  closeTerminal,
  detectAgents,
  getCliLauncherStatus,
  getAccountEnrollmentStatus,
  getInitialProject,
  getSystemPermissionsStatus,
  isTauriRuntime,
  listWorktrees,
  loadSession,
  onWorktreeChanged,
  onCloseTabMenu,
  onSelectWorktreeMenu,
  onSelectTabMenu,
  onNextTabMenu,
  onPrevTabMenu,
  onSplitRightMenu,
  onSplitDownMenu,
  onCommandPaletteMenu,
  onToggleSidebarMenu,
  onOpenSettingsMenu,
  onNewTerminalTabMenu,
  onRemoteSelectionRequested,
  publishFocusedTerminal,
  registerProject,
  unregisterProject,
  saveSession,
  setBadgeCount,
  spawnTerminal,
  spawnTerminalDetailed,
  retryTerminalRemoteSession,
  toIpcError,
  writeTerminal,
  bootTrace,
  browserRemoteReclaim,
  discoverDagWatchRoots,
  type AgentDetection,
  type FocusedTerminalPayload,
  type RegisteredProject,
  type RemoteSelectionRequestedPayload,
} from "./lib/tauri";
import { safeRandomUUID } from "./lib/uuid";
import { dagRemoteWatchTargets, useDagWatchLifecycle } from "./lib/useDagWatchLifecycle";
import { getCachedSshHosts } from "./lib/sshHosts";
import { reconnectAgentSession } from "./lib/agentReconnect";
import { isPairedWorkspaceId, isRemoteWorkspaceId, registerRemoteProject, toRegisteredProject } from "./lib/remoteProject";
import { healMissingSshRegistrations } from "./lib/sshRegistrationHeal";
import { hasValidProjectTarget, projectRootWorktree, sshProjectWorktrees } from "./lib/projectIdentity";
import { groupProjects } from "./lib/projectGrouping";
import { scheduleAgentAutoResume } from "./lib/agentAutoResume";
import { isStandbyBackendSessionId, setSessionRebindHandler } from "./lib/sessionLifecycle";
import { flashPane } from "./lib/paneFlash";
import { getAgentReconnectAffordance } from "./lib/agentResumeAffordance";
import { createAppReconnectDependencies } from "./lib/appReconnectDependencies";
import { replaceExitedShellSession } from "./lib/shellReplacement";
import { enqueueStrictPersistence } from "./lib/persistenceQueue";
import { workspaceReducer } from "./state/workspaceStore";
import type { NotificationTarget, PersistedWorkspaceSession, SystemPermissionsStatus } from "./lib/types";
import { staticTabLabel } from "./lib/tabTitle";
import { subscribeNotificationActivations } from "./lib/notificationActivation";
import { ensureTerminalEvents } from "./lib/terminalEvents";
import { useTerminalSettings } from "./lib/terminalSettings";
import { resolveWorktreeOwnerId } from "./lib/worktreeOwnership";
import { createClosedTabStack } from "./lib/closedTabStack";
import { switchDebug } from "./lib/switchDebug";
import { useInactiveProjectWorktrees } from "./state/inactiveProjectWorktrees";
import {
  isTerminalTab,
  worktreeIdentity,
  type DirtyState,
  type TerminalSession,
  type WorkspaceTab,
  type Worktree,
} from "./lib/types";
import { registerWindowCloseGuard, startUpdatePolling } from "./lib/updater";
import { maybeShowWindowsStoreMigrationNotice } from "./lib/windowsStoreMigration";
import { collectLeafIds, type PaneDirection } from "./state/paneTree";
import { useBrowserSessionHydration } from "./state/browserSessionHydration";
import { preloadWorkspaceSnapshots, useWorkspaceRestore } from "./state/workspaceRestore";
import { clearHmrWorkspaceState, getHmrWorkspaceState } from "./state/hmrWorkspaceState";
import { clearWorkspaceSnapshot, getWorkspaceSnapshot, listWorkspaceSnapshots } from "./state/workspaceSnapshotCache";
import { emptySidebarWorkspaceIds, worktreeHasOpenTabs } from "./state/sidebarWorkspaceState";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { listPairedProjectWorktrees } from "./state/pairedProjectWorktrees";
import { getTabSessionIds, hasNavigableSession, selectGlobalUnreadBadgeCount, selectNotificationWorkspaceLabel, selectWorktreeActivitySummaries, useWorkspaceStore, type WorkspaceState } from "./state/workspaceStore";

export { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY };
type InboxNavigationTarget = NotificationTarget & { revision?: number };

function acknowledgeNotificationTarget(target: InboxNavigationTarget): void {
  notificationCenterStore.markEntriesRead([{
    id: notificationEntryId(target.workspaceId, target.sessionId), expectedRevision: target.revision,
  }]);
}

const DEFAULT_PROJECT: RegisteredProject = { workspaceId: DEFAULT_WORKSPACE_ID, repoRoot: ".", gitRoot: null };
const loadSettingsDialog = () =>
  import("./components/SettingsDialog").then((m) => ({ default: m.SettingsDialog }));
const SettingsDialog = lazy(loadSettingsDialog);
const WelcomeWizard = lazy(() =>
  import("./components/onboarding/WelcomeWizard").then((m) => ({
    default: m.WelcomeWizard,
  }))
);
const WhatsNewDialog = lazy(() =>
  import("./components/onboarding/WhatsNewDialog").then((m) => ({
    default: m.WhatsNewDialog,
  }))
);
let settingsDialogPreloaded = false;
const preloadSettingsDialog = () => {
  if (settingsDialogPreloaded) return;
  settingsDialogPreloaded = true;
  void loadSettingsDialog();
};

type ProjectBootstrap = {
  projects: RegisteredProject[];
  activeProjectId: string;
};

function loadProjectBootstrap(): ProjectBootstrap {
  return { projects: loadProjects(), activeProjectId: loadActiveProjectId() };
}

export function recoverProjectBootstrap(session: PersistedWorkspaceSession | null): ProjectBootstrap | null {
  if (!session) return null;

  const projects = Object.values(session.workspaces).reduce<RegisteredProject[]>((recovered, workspace) => {
    if (!workspace.workspaceId || !workspace.repoRoot || !hasValidProjectTarget(workspace)) return recovered;
    if (!recovered.some((project) => project.workspaceId === workspace.workspaceId)) {
      const metadata = {
        workspaceId: workspace.workspaceId, repoRoot: workspace.repoRoot,
        gitRoot: workspace.gitRoot,
        gitRemote: workspace.gitRemote, gitCommonDir: workspace.gitCommonDir,
        gitBranch: workspace.gitBranch, gitHead: workspace.gitHead,
        hostLabel: workspace.hostLabel,
      };
      if (workspace.target?.kind === "pairedDaemon") {
        if (typeof workspace.remoteWorkspaceId !== "string") throw new Error("INVALID_PAIRED_PROJECT_METADATA");
        recovered.push({ ...metadata, target: workspace.target, remoteWorkspaceId: workspace.remoteWorkspaceId });
      } else {
        recovered.push({ ...metadata, target: workspace.target });
      }
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
    project.target?.kind !== "ssh" &&
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

const healedSshWorkspaceIds = new Set<string>();

function triggerSshRegistrationHeal(projects: RegisteredProject[]): void {
  if (!isTauriRuntime()) return;
  void healMissingSshRegistrations(projects, {
    hasRegistered: (id) => healedSshWorkspaceIds.has(id),
    register: async (req) => {
      const res = await registerRemoteProject(req);
      healedSshWorkspaceIds.add(req.workspaceId);
      return res;
    },
  }).catch((err) => {
    console.warn("SSH workspace registration heal skipped:", err);
  });
}

const healedLocalWorkspaceIds = new Set<string>();

// Terminals in a project whose registration failed cannot spawn (WORKSPACE_NOT_FOUND), so
// the failure is shown with its reason and a retry instead of only reaching the console.
export function reportLocalRegistrationFailure(project: RegisteredProject, error: unknown): void {
  const name = project.repoRoot.split(/[\\/]/).filter(Boolean).pop() ?? project.workspaceId;
  const reason = error instanceof Error ? error.message : String(error);
  toast.error(`Couldn't open project "${name}"`, {
    id: `local-registration:${project.workspaceId}`,
    description: `New terminals in this project will fail until it registers. ${reason}`,
    duration: Infinity,
    action: {
      label: "Retry",
      onClick: () => {
        void ensureLocalProjectsRegistered([project]);
      },
    },
  });
}

// Restored workspace tabs spawn as soon as the shell mounts, so every stored local project
// must be registered before the bootstrap is published; otherwise the spawn races the
// registration and fails with WORKSPACE_NOT_FOUND.
export async function ensureLocalProjectsRegistered(projects: RegisteredProject[]): Promise<void> {
  if (!isTauriRuntime()) return;
  const pending = projects.filter(
    (project) =>
      project.repoRoot !== "" &&
      project.repoRoot !== "." &&
      project.workspaceId !== DEFAULT_WORKSPACE_ID &&
      !isRemoteWorkspaceId(project.workspaceId) &&
      !isPairedWorkspaceId(project.workspaceId) &&
      !healedLocalWorkspaceIds.has(project.workspaceId),
  );
  await Promise.all(
    pending.map(async (project) => {
      try {
        await registerProject({ workspaceId: project.workspaceId, repoPath: project.repoRoot });
        healedLocalWorkspaceIds.add(project.workspaceId);
      } catch (error) {
        console.warn("Local workspace registration skipped:", project.workspaceId, error);
        reportLocalRegistrationFailure(project, error);
      }
    }),
  );
}

export function App() {
  useApplyAppearanceSettings();
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const [bootstrap, setBootstrap] = useState<ProjectBootstrap | null>(() =>
    isNativeRuntime ? null : loadProjectBootstrap(),
  );

  useEffect(() => {
    if (isNativeRuntime) return startUpdatePolling();
  }, [isNativeRuntime]);

  useEffect(() => {
    if (isNativeRuntime) void maybeShowWindowsStoreMigrationNotice();
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
        triggerSshRegistrationHeal(prepared.projects);
        await withTimeout(
          ensureLocalProjectsRegistered(prepared.projects),
          STARTUP_TIMEOUT_MS,
          "local workspace registration heal",
        ).catch((error) => {
          console.warn("Local workspace registration heal skipped:", error);
        });
        if (!cancelled) setBootstrap(prepared);
      })
      .catch(async (error) => {
        const errorMsg = (error as { message?: string } | null)?.message ?? String(error);
        void bootTrace("initial.error", { message: String(errorMsg).slice(0, 200), details: error });
        if (!cancelled) {
          const storedBootstrap = loadProjectBootstrap();
          const savedSession = await loadSession().catch(() => null);
          const recovered = recoverProjectBootstrap(savedSession);
          // In native runtime, if there are no real stored or recovered projects,
          // do NOT resurrect an unregistrable phantom "default" workspace that causes
          // WORKSPACE_NOT_FOUND on spawn; show the genuine empty state instead.
          const realProjects = storedBootstrap.projects.filter(
            (p) => p.workspaceId !== DEFAULT_WORKSPACE_ID && p.repoRoot !== ".",
          );
          const recoveredProjects = recovered?.projects.filter(
            (p) => p.workspaceId !== DEFAULT_WORKSPACE_ID && p.repoRoot !== ".",
          ) ?? [];
          const initialProject = realProjects[0] ?? recoveredProjects[0];
          const prepared: ProjectBootstrap = initialProject
            ? canonicalizeProjectBootstrap(
                mergeRecoveredProjectBootstrap(
                  { projects: realProjects, activeProjectId: storedBootstrap.activeProjectId },
                  recovered,
                  initialProject,
                ),
                initialProject,
              )
            : { projects: [], activeProjectId: "" };
          await preloadWorkspaceSnapshots(
            prepared.projects.map((project) => project.workspaceId),
            async () => savedSession,
          ).catch((preloadError) => {
            console.warn("Workspace session preload fallback skipped:", preloadError);
          });
          triggerSshRegistrationHeal(prepared.projects);
          await withTimeout(
            ensureLocalProjectsRegistered(prepared.projects),
            STARTUP_TIMEOUT_MS,
            "local workspace registration heal",
          ).catch((error) => {
            console.warn("Local workspace registration heal skipped:", error);
          });
          if (!cancelled) setBootstrap(prepared);
        }
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
  snapshots: ReadonlyArray<readonly [string, WorkspaceState]> = [],
): FocusedTerminalPayload | null {
  const attentionInventory = [[workspaceId, state] as const, ...snapshots.filter(([id]) => id !== workspaceId)]
    .flatMap(([id, snapshot]) => {
      const summaries = selectWorktreeActivitySummaries(snapshot);
      return snapshot.worktrees.map((worktree) => {
        const activity = summaries[worktree.path];
        const attention: "waiting" | "done" | "working" | null = activity?.hasWaiting ? "waiting"
          : activity?.hasDone ? "done" : activity?.hasWorking ? "working" : null;
        return { workspaceId: id, worktreeSlug: worktreeIdentity(worktree)?.slug ?? null,
          worktreeLabel: worktree.branch?.replace(/^refs\/heads\//, "") ?? null, state: attention };
      });
    });
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
    focusedCandidate && isTerminalTab(focusedCandidate) ? focusedCandidate : null;

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

  const terminalTabs = [state.layout, ...Object.values(state.worktreeLayouts ?? {})].flatMap((ownerLayout) => ownerLayout.tabs.flatMap((tab) => {
    if (!isTerminalTab(tab)) return [];
    const layout = ownerLayout.layoutsByTabId?.[tab.id];
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
      const baseLabel = remoteTabLabel(staticTabLabel(tab));
      return {
        id: remotePaneId(tab.id, multiPane ? leafId : null),
        label: multiPane ? `${baseLabel} (${index + 1})` : baseLabel,
        ...(activity && (activity.state === "working" || !activity.seen) ? { activityState: activity.state } : {}),
        ...(activity?.agentType ? { agentType: activity.agentType } : {}),
        ...(paneSlug ? { worktreeSlug: paneSlug } : {}),
        ...(paneLabel ? { worktreeLabel: paneLabel } : {}),
        ...(backendSessionId ? { sessionId: backendSessionId } : {}),
      };
    });
  }));

  if (terminalTabs.length === 0 && !attentionInventory.some((entry) => entry.state !== null)) return null;

  const focusedLayout = focusedTab ? state.layout.layoutsByTabId?.[focusedTab.id] : undefined;
  const focusedLeafIds = focusedLayout?.root ? collectLeafIds(focusedLayout.root) : [];
  const focusedEntryId = focusedTab
    ? remotePaneId(focusedTab.id, focusedLeafIds.length > 1 ? activeLeafId : null)
    : null;

  return {
    workspaceId,
    attentionInventory,
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
  if (!tab || !isTerminalTab(tab)) return false;
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
    const lastComponent = wt.path.split(/[\\/]/).filter(Boolean).pop();
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
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const activeRemoteHost = useSyncExternalStore(
    remoteHostStore.subscribe,
    () => selectActiveHost(remoteHostStore.getState()),
  );
  const activeRemoteHostRef = useRef(activeRemoteHost);
  activeRemoteHostRef.current = activeRemoteHost;
  const [projects, setProjects] = useState<RegisteredProject[]>(initialProjects);
  const [activeProjectId, setActiveProjectId] = useState(initialActiveProjectId);

  useEffect(() => {
    startNativeWindowFocusTracking();
  }, []);
  const [registeredProjectId, setRegisteredProjectId] = useState<string | null>(null);
  const [registrationAttempt, setRegistrationAttempt] = useState(0);
  const [registrationError, setRegistrationError] = useState<{
    workspaceId: string;
    code: string;
    message: string;
  } | null>(null);
  const [sshTabOperation, setSshTabOperation] = useState<{
    workspaceId: string;
    error?: string;
    retry: () => void;
  } | null>(null);
  const sshTabOperationRef = useRef<object | null>(null);
  const registeredProjectIdRef = useRef<string | null>(null);
  registeredProjectIdRef.current = registeredProjectId;
  const [pendingBackendRecovery, setPendingBackendRecovery] = useState<{ workspaceId: string; sessionIds: string[] } | null>(
    null,
  );
  const [pendingAgentAutoResume, setPendingAgentAutoResume] = useState<{
    workspaceId: string;
    state: WorkspaceState;
  } | null>(null);
  const lastRestoredSessionsRef = useRef<Record<string, TerminalSession>>({});
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
  const resolvedAgentsRef = useRef(resolvedAgents);
  resolvedAgentsRef.current = resolvedAgents;

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
    unreadBadgeCount: unreadBadgeCountFromStore,
    activityNotificationTargets,
    markTabUnread,
    markWorktreeUnread,
    subscribeTerminalBell,
    subscribeActivityNotification,
    parkedActivityVersion,
    openTab,
    createBrowserTab,
    openFilePreviewTab,
    adoptBrowserSession,
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
    equalizePaneRun,
    equalizeTabGroupRun,
    swapPanes,
    syncWorktrees,
    restoreWorkspace,
    resetAgentState,
    resetWorktreeAgentState,
    ensureSessionBackends,
    dispatchWorkspaceAction,
    markBackendSessionUnavailable,
  } = useWorkspaceStore({ workspaceId: activeProject.workspaceId });
  useBrowserSessionHydration(state, activeProject.workspaceId);

  const stateRef = useRef(state);
  stateRef.current = state;

  const markTabUnreadRef = useRef(markTabUnread);
  markTabUnreadRef.current = markTabUnread;
  const markWorktreeUnreadRef = useRef(markWorktreeUnread);
  markWorktreeUnreadRef.current = markWorktreeUnread;

  const reportRuntimeErrorRef = useRef<(err: unknown) => void>(() => {});
  const handleOpenSettingsRef = useRef<(section?: SectionId) => void>(() => {});

  const coordinatorRef = useRef<NotificationCoordinator | null>(null);
  if (!coordinatorRef.current) {
    coordinatorRef.current = new NotificationCoordinator({
      onMarkTabUnread: (tabId, owner) => markTabUnreadRef.current?.(tabId, owner),
      onMarkWorktreeUnread: (path, owner) => markWorktreeUnreadRef.current?.(path, owner),
      isWindowFocused: () => getNativeWindowFocused() ?? isWindowForegroundFocused(),
      onError: (err) => reportRuntimeErrorRef.current(err),
      onPermissionUnavailable: () =>
        showNotificationPermissionHint(() => handleOpenSettingsRef.current("notifications")),
    });
  }

  const activityNotificationTargetsRef = useRef(activityNotificationTargets);
  activityNotificationTargetsRef.current = activityNotificationTargets;

  const isNotificationObserved = useCallback((target: RecordingTarget) => isNotificationTargetObserved(
    stateRef.current, target, getNativeWindowFocused() ?? isWindowForegroundFocused(),
  ), []);

  const handleResetTabAgentState = useCallback(
    async (tabId: string) => {
      const tab = state.layout.tabs.find((t) => t.id === tabId);
      if (!tab || tab.kind === "browser") return;
      const sessionIds = getTabSessionIds(state, tab.id);
      const results = await Promise.allSettled([...sessionIds].map(resetAgentState));
      const failures = results.filter((result) => result.status === "rejected");
      if (failures.length > 0) {
        toast.error(`Agent state reset failed for ${failures.length} of ${sessionIds.size} sessions`);
      } else {
        toast.success("Agent state reset");
      }
    },
    [state, resetAgentState],
  );

  const handleResetWorktreeAgentState = useCallback(
    async (worktree: Worktree) => {
      try {
        await resetWorktreeAgentState(worktree.path);
        toast.success("Agent state reset");
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "Failed to reset agent state");
      }
    },
    [resetWorktreeAgentState],
  );

  useEffect(() => wireActivityRecording({
    events: (record) => subscribeActivityNotification((event) => {
      const decision = coordinatorRef.current!.handleAgentStateChange({ ...event, nextState: event.state });
      record({ ...event, workspaceId: event.workspaceId ?? stateRef.current.workspaceId }, decision);
    }),
    isObserved: isNotificationObserved,
  }), [subscribeActivityNotification, isNotificationObserved]);

  const handleTerminalBell = useCallback((sessionId: string, tabId: string, eventTarget: import("./state/workspaceStore").ActivityNotificationTarget | undefined, record: RecordingListener<RecordingTarget>) => {
    const targets = activityNotificationTargetsRef.current ?? [];
    const target = eventTarget ?? targets.find(
      (candidate) => candidate.sessionId === sessionId,
    );
    let workspaceLabel = target?.workspaceLabel;
    let worktreePath = target?.worktreePath;
    let worktreeLabel = target?.worktreeLabel;
    let terminalTitle = target?.terminalTitle;

    if (!target) {
      workspaceLabel = selectNotificationWorkspaceLabel(stateRef.current);
      const session = stateRef.current.sessions[sessionId];
      const fallbackWorktreePath = session?.worktreePath ?? session?.cwd ?? "";
      const worktree = stateRef.current.worktrees.find(
        (candidate) => candidate.path === fallbackWorktreePath,
      );
      worktreePath = fallbackWorktreePath;
      worktreeLabel = worktree ? workspaceName(worktree) : "";
      terminalTitle = "";
    }

    const decision = coordinatorRef.current!.handleTerminalBell({
      workspaceId: target?.workspaceId,
      workspaceLabel,
      sessionId,
      tabId,
      worktreePath,
      worktreeLabel,
      terminalTitle,
    });
    record({
      workspaceId: target?.workspaceId ?? stateRef.current.workspaceId,
      workspaceLabel, sessionId, tabId, worktreeLabel: worktreeLabel ?? "",
      terminalTitle: terminalTitle ?? "", agentLabel: target?.agentLabel,
    }, decision);
  }, []);

  // The bell arrives from the store's global native subscription, not from a pane prop: only the
  // active tab's panes are mounted, and a bell in the tab you are watching is not what notifies.
  useEffect(() => wireBellRecording({
    events: (record) => subscribeTerminalBell((sessionId, tabId, target) => handleTerminalBell(sessionId, tabId, target, record)),
    isObserved: isNotificationObserved,
  }), [handleTerminalBell, subscribeTerminalBell, isNotificationObserved]);

  // Acknowledge session notifications when user interacts with a pane (focus, input, paste, navigation)
  useEffect(() => {
    const onSessionInteracted = (event: Event) => {
      const detail = (event as CustomEvent<{ sessionId?: string; revision?: number }>).detail;
      const sessionId = detail?.sessionId;
      if (!sessionId || !state.workspaceId) return;
      if (!state.sessions[sessionId]) return;

      const entryId = notificationEntryId(state.workspaceId, sessionId);
      const entry = notificationCenterStore.getSnapshot().entries.find((e) => e.id === entryId);
      if (entry && !("seen" in entry.read)) {
        notificationCenterStore.markEntriesRead([{ id: entry.id, expectedRevision: entry.revision }]);
      }
    };

    window.addEventListener("ferryx:session-interacted", onSessionInteracted);
    return () => window.removeEventListener("ferryx:session-interacted", onSessionInteracted);
  }, [state.workspaceId, state.sessions]);
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
    () => {
      if (projects.length === 0) return null;
      const target = activeProject.target;
      if (activeProject.gitRoot === null || target?.kind === "ssh" || target?.kind === "pairedDaemon") {
        const hostLabel = target?.kind === "ssh"
          ? (typeof getCachedSshHosts === "function"
              ? getCachedSshHosts()?.find((h) => h.id === target.hostId)?.label ?? target.hostId
              : target.hostId)
          : target?.kind === "pairedDaemon"
          ? (remoteHostStore.getState().hosts[target.hostId]?.displayName ?? "Remote")
          : undefined;
        return projectRootWorktree(activeProject, hostLabel);
      }
      return null;
    },
    [activeProject],
  );

  const { runtimeError, refreshWorktrees, reportRuntimeError } = useWorkspaceRuntime({
    workspaceId: activeProject.workspaceId,
    activeWorktreePath: state.activeWorktreePath,
    syncWorktrees,
    ensureTabForWorktree,
    // Plain (non-Git) projects have no git worktrees; their folder root acts
    // as the primary "worktree" so a terminal opens there like anywhere else.
    plainRootWorktree,
    rootOnly: activeProject.target?.kind === "ssh" && activeProject.gitRoot === null,
    registeredWorkspaceId: registeredProjectId,
    services: activeProject.target?.kind === "pairedDaemon"
      ? {
          ensureTerminalEvents,
          listWorktrees: async () => {
            const listed = await listPairedProjectWorktrees(activeProject);
            return listed ?? [];
          },
          onWorktreeChanged,
          isTauriRuntime,
        }
      : activeProject.target?.kind === "ssh"
      ? {
          ensureTerminalEvents,
          listWorktrees: async (workspaceId: string) => sshProjectWorktrees(activeProject, await listWorktrees(workspaceId)),
          onWorktreeChanged,
          isTauriRuntime,
        }
      : undefined,
  });
  reportRuntimeErrorRef.current = reportRuntimeError;

  useEffect(() => {
    setSshTabOperation(null);
    return () => { sshTabOperationRef.current = null; };
  }, [activeProject.workspaceId]);

  const runTabOperation = useCallback((operation: () => Promise<unknown>) => {
    const project = activeProjectRef.current;
    if (project.target?.kind !== "ssh" || stateRef.current.layout.tabs.length > 0) {
      void operation().catch(reportRuntimeError);
      return;
    }
    const request = {};
    sshTabOperationRef.current = request;
    const retry = () => runTabOperation(operation);
    setSshTabOperation({ workspaceId: project.workspaceId, retry });
    void operation().then(() => {
      if (sshTabOperationRef.current === request) setSshTabOperation(null);
    }).catch((error: unknown) => {
      if (sshTabOperationRef.current !== request || activeProjectRef.current.workspaceId !== project.workspaceId) return;
      const ipcError = toIpcError(error);
      setSshTabOperation({ workspaceId: project.workspaceId, error: `${ipcError.code}: ${ipcError.message}`, retry });
      reportRuntimeError(error);
    });
  }, [reportRuntimeError]);

  const inactiveProjectWorktrees = useInactiveProjectWorktrees(
    projects,
    activeProject.workspaceId,
    state.worktrees,
    undefined,
    (registered) => {
      setProjects((current) => {
        const index = current.findIndex((project) =>
          project.workspaceId === registered.workspaceId &&
          project.repoRoot === registered.repoRoot &&
          project.target?.kind !== "ssh");
        const previous = current[index];
        if (!previous || (previous.gitRemote === registered.gitRemote &&
            previous.gitCommonDir === registered.gitCommonDir)) return current;
        const next = [...current];
        next[index] = { ...previous, gitRemote: registered.gitRemote, gitCommonDir: registered.gitCommonDir };
        persistProjects(next);
        return next;
      });
    },
  );
  const inactiveProjectWorktreesRef = useRef(inactiveProjectWorktrees);

  // Dag journals: watch every known project root, worktree and live session root so any omo
  // graph run anywhere lights up the activity badge, regardless of which cwd the app started in.
  const stateProject = projects.find((project) => project.workspaceId === (state.workspaceId ?? activeProject.workspaceId));
  const isRemoteState = !isLocalDagProject(stateProject);
  const activeWorktreePathsKey = isRemoteState ? "" : state.worktrees.map((worktree) => worktree.path).join("\n");
  const projectRootsKey = projects.filter(isLocalDagProject).map((project) => project.repoRoot).join("\n");
  const remoteProjectsKey = useMemo(() => remoteProjectsWatchKey(projects), [projects]);
  const sessionDagRootsKey = useMemo(
    () =>
      isRemoteState ? "" : collectDagWatchRoots({ projectRoots: [], worktreePaths: [], sessions: state.sessions })
        .sort()
        .join("\n"),
    [isRemoteState, state.sessions],
  );
  const discoveredDagRootsRef = useRef<string[]>([]);
  const [discoveredDagRootsKey, setDiscoveredDagRootsKey] = useState("");
  useEffect(() => {
    if (isRemoteState || projectRootsKey === "") return;
    let disposed = false;
    void discoverDagWatchRoots(projectRootsKey.split("\n").filter(Boolean))
      .then((roots) => {
        if (disposed) return;
        discoveredDagRootsRef.current = roots;
        setDiscoveredDagRootsKey(roots.slice().sort().join("\n"));
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
    };
  }, [isRemoteState, projectRootsKey]);
  const dagLocalRoots = useMemo(
    () => [
      ...collectDagWatchRoots({
        projectRoots: projectRootsKey.split("\n"),
        worktreePaths: [
          ...Object.entries(inactiveProjectWorktrees).flatMap(([workspaceId, worktrees]) =>
            projects.some((project) => project.workspaceId === workspaceId && isLocalDagProject(project))
              ? worktrees.map((worktree) => worktree.path) : [],
          ),
          ...activeWorktreePathsKey.split("\n"),
          ...sessionDagRootsKey.split("\n"),
        ],
        sessions: [],
      }),
      ...discoveredDagRootsRef.current,
    ],
    // discoveredDagRootsKey tracks the ref's content; the ref itself holds the exact roots.
    [projects, inactiveProjectWorktrees, projectRootsKey, activeWorktreePathsKey, sessionDagRootsKey, discoveredDagRootsKey],
  );
  const dagRemoteTargets = useMemo(() => dagRemoteWatchTargets(projects), [projects]);
  useDagWatchLifecycle({
    localRoots: dagLocalRoots,
    remoteTargets: dagRemoteTargets,
    watchKey: `${dagLocalRoots.join("\n")}\u0001${remoteProjectsKey}`,
  });
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
          void copyTextToClipboard(clipboardText).then((ok) => {
            if (ok) toast.success("Copied error to clipboard");
            else toast.error("Failed to copy error to clipboard");
          });
        },
      },
    });
  }, [runtimeError]);

  // Registration re-runs when the active project object changes identity (a
  // successful registration enriches hostLabel/branch fields via setProjects).
  // Key the effect on stable target identity so equivalent projects do not
  // re-register behind the user's back.
  const pairedMachineFeaturesEnabled = useSyncExternalStore(remoteHostStore.subscribe, remoteHostStore.getState).machineFeaturesEnabled === true;
  const pairedTerminalsUnavailable = activeProject.target?.kind === "pairedDaemon" && !pairedMachineFeaturesEnabled;
  const activeProjectTargetKey = activeProject.target?.kind === "ssh"
    ? `ssh:${activeProject.target.hostId}`
    : activeProject.target?.kind ?? "none";
  useEffect(() => {
    let cancelled = false;
    setRegisteredProjectId(null);
    setRegistrationError(null);
    const isPlaceholder =
      projects.length === 1 &&
      projects[0].workspaceId === DEFAULT_WORKSPACE_ID &&
      projects[0].target?.kind !== "ssh" &&
      projects[0].repoRoot === ".";
    // With no user projects left there is nothing to register: the fallback
    // DEFAULT_PROJECT (".") must never be silently (re-)registered behind the
    // user's back after they removed every project.
    if (projects.length === 0 || isPlaceholder) {
      switchDebug("project.register.skipped", { reason: "no-projects" });
      return;
    }
    switchDebug("project.register.start", {
      workspaceId: activeProject.workspaceId,
      repoRoot: activeProject.repoRoot,
      registrationAttempt,
    });
    // Paired references are already registered by their owning daemon. Their
    // paths must never be canonicalized or registered on this desktop.
    if (activeProject.target?.kind === "pairedDaemon" && !pairedMachineFeaturesEnabled) return;
    const registration = activeProject.target?.kind === "pairedDaemon"
      ? Promise.resolve(activeProject)
      : activeProject.target?.kind === "ssh"
      ? registerRemoteProject({
          workspaceId: activeProject.workspaceId,
          hostId: activeProject.target.hostId,
          repoPath: activeProject.repoRoot,
        }).then(toRegisteredProject)
      : registerProject({ workspaceId: activeProject.workspaceId, repoPath: activeProject.repoRoot });
    void registration
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
          registered.workspaceId !== activeProject.workspaceId &&
          (activeProject.target?.kind === "ssh" || registered.repoRoot === activeProject.repoRoot);
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
                candidate.gitRoot === registered.gitRoot &&
                candidate.gitRemote === registered.gitRemote &&
                candidate.gitCommonDir === registered.gitCommonDir &&
                candidate.gitBranch === registered.gitBranch &&
                candidate.gitHead === registered.gitHead &&
                candidate.hostLabel === registered.hostLabel &&
                JSON.stringify(candidate.target) === JSON.stringify(registered.target),
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
        // Canonical-path changes re-render the runtime's root before it syncs.
        if (registered.repoRoot !== activeProject.repoRoot) return;
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
        if (!cancelled) {
          const ipcError = toIpcError(error);
          setRegistrationError({
            workspaceId: activeProject.workspaceId,
            code: ipcError.code,
            message: ipcError.message,
          });
          reportRuntimeError(error);
        }
      });
    return () => {
      cancelled = true;
      switchDebug("project.register.cancel", {
        workspaceId: activeProject.workspaceId,
      });
    };
  }, [activeProject.repoRoot, activeProject.workspaceId, activeProjectTargetKey, pairedMachineFeaturesEnabled, projects.length, registrationAttempt, refreshWorktrees, reportRuntimeError]);

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
      lastRestoredSessionsRef.current = restoredState.sessions;
      restoreWorkspace(restoredState);

      const deadSessions = Object.values(restoredState.sessions).filter(
        (session) =>
          (session.backendSessionId === null || isStandbyBackendSessionId(session.backendSessionId)) &&
          !isRemoteWorkspaceId(session.workspaceId),
      );

      const shellRecoverySessionIds: string[] = [];
      let hasResumableAgents = false;
      for (const session of deadSessions) {
        const isAgent = Boolean(session.agentType || session.providerSession || session.agentSessionId);
        if (!isAgent) {
          // Only eagerly spawn fallback shells for null backendSessionIds, not standby sessions
          if (!recoveredFromHmr && session.backendSessionId === null) {
            shellRecoverySessionIds.push(session.id);
          }
        } else {
          const affordance = getAgentReconnectAffordance(session, restoredState.sessions);
          if (affordance.canReconnect) {
            hasResumableAgents = true;
          } else if (!recoveredFromHmr && session.backendSessionId === null) {
            shellRecoverySessionIds.push(session.id);
          }
        }
      }

      if (shellRecoverySessionIds.length > 0) {
        setPendingBackendRecovery({
          workspaceId: activeProjectRef.current.workspaceId,
          sessionIds: shellRecoverySessionIds,
        });
      }
      if (hasResumableAgents) {
        setPendingAgentAutoResume({
          workspaceId: activeProjectRef.current.workspaceId,
          state: restoredState,
        });
      }
    },
    [recoveredFromHmr, restoreWorkspace],
  );

  // Initial session restore on startup & HMR recovery managed by coordinator.
  const workspaceRestoreStatus = useWorkspaceRestore({
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
    void ensureSessionBackends?.(pendingBackendRecovery.sessionIds)?.catch(reportRuntimeError);
  }, [activeProject.workspaceId, ensureSessionBackends, pendingBackendRecovery, registeredProjectId, reportRuntimeError]);

  const saveChainRef = useRef<Promise<void>>(Promise.resolve());
  const cachedLoadedSessionRef = useRef<PersistedWorkspaceSession | null>(null);
  const persistSessionStrict = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    return enqueueStrictPersistence(saveChainRef, async () => {
        let existing = cachedLoadedSessionRef.current;
        if (!existing) {
          existing = await loadSession().catch(() => null);
        }
        const session = serializeWorkspaceState(
          workspaceId,
          repoRoot,
          currentState,
          existing,
          projectsRef.current.find((project) => project.workspaceId === workspaceId),
        );
        cachedLoadedSessionRef.current = session;
        await saveSession(session);
    });
  }, []);
  const persistSession = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    return persistSessionStrict(workspaceId, repoRoot, currentState).catch((error) => {
      console.error("Failed to save workspace session:", error);
    });
  }, [persistSessionStrict]);

  useEffect(() => {
    setLocalSplitPersistence((owner) => {
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === owner.workspaceId);
      const snapshot = getWorkspaceSnapshot(owner.workspaceId);
      if (!project || !snapshot) return Promise.reject(new Error("Split persistence owner is unavailable"));
      return persistSessionStrict(owner.workspaceId, project.repoRoot, {
        ...snapshot, sessions: { ...snapshot.sessions, [owner.id]: owner },
      });
    });
    return () => setLocalSplitPersistence(undefined);
  }, [persistSessionStrict]);

  const handleReconnectAgentSession = useCallback(
    (sessionId: string, options?: { silent?: boolean }) => {
      return reconnectAgentSession(
        sessionId,
        createAppReconnectDependencies({
          getSessions: () => {
            const current = stateRef.current.sessions;
            if (current[sessionId]) return current;
            return { ...lastRestoredSessionsRef.current, ...current };
          },
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
        }),
      ).catch((error) => {
        if (!options?.silent) {
          reportRuntimeError(error);
        }
        throw error;
      });
    },
    [activeProject.repoRoot, activeProject.workspaceId, dispatchWorkspaceAction, persistSessionStrict, reportRuntimeError],
  );

  const reconnectingSshRef = useRef<Map<string, Promise<void>>>(new Map());
  const handleReconnectSshSession = useCallback(
    (sessionId: string) => {
      const existing = reconnectingSshRef.current.get(sessionId);
      if (existing) return existing;
      const task = (async () => {
        const current = stateRef.current.sessions;
        const session = current[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
        if (!session) return;
        const targetWorkspaceId = session.workspaceId;
        if (session.backendSessionId) {
          try {
            const res = await retryTerminalRemoteSession(session.backendSessionId);
            if (res.type === "retryRemoteSessionOk") {
              return;
            }
          } catch {
            // Retry failed or backend session no longer valid on daemon; fall through to respawn
          }
        }
        if (stateRef.current.workspaceId !== targetWorkspaceId || !stateRef.current.sessions[sessionId]) return;
        let spawned: Awaited<ReturnType<typeof spawnTerminalDetailed>> | null = null;
        let adopted = false;
        try {
          spawned = await spawnTerminalDetailed({
            workspaceId: session.workspaceId,
            worktree: session.worktree,
            cwd: session.cwd,
            clientRequestId: `ssh-reconnect-${safeRandomUUID()}`,
            startup: null,
          });
          const latest = stateRef.current;
          if (latest.workspaceId !== targetWorkspaceId || !latest.sessions[sessionId]) {
            if (spawned) await closeTerminal(spawned.sessionId).catch(() => undefined);
            return;
          }
          const nextState = workspaceReducer(stateRef.current, {
            type: "REBIND_SESSION_BACKEND",
            sessionId,
            backendSessionId: spawned.sessionId,
            cwd: spawned.session.cwd ?? session.cwd,
            daemonEpoch: spawned.daemonEpoch,
          });
          dispatchWorkspaceAction({
            type: "REBIND_SESSION_BACKEND",
            sessionId,
            backendSessionId: spawned.sessionId,
            cwd: spawned.session.cwd ?? session.cwd,
            daemonEpoch: spawned.daemonEpoch,
          });
          adopted = true;
          try {
            await persistSessionStrict(targetWorkspaceId, activeProject.repoRoot, nextState);
          } catch (persistError) {
            reportRuntimeError(persistError);
          }
        } catch (error) {
          if (spawned && !adopted) await closeTerminal(spawned.sessionId).catch(() => undefined);
          reportRuntimeError(error);
          throw error;
        }
      })();
      reconnectingSshRef.current.set(sessionId, task);
      void task
        .catch(() => undefined)
        .then(() => {
          if (reconnectingSshRef.current.get(sessionId) === task) reconnectingSshRef.current.delete(sessionId);
        });
      return task;
    },
    [activeProject.repoRoot, dispatchWorkspaceAction, persistSessionStrict, reportRuntimeError],
  );

  const activeAutoResumeCancelRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      activeAutoResumeCancelRef.current?.();
      activeAutoResumeCancelRef.current = null;
    };
  }, [activeProject.workspaceId]);

  useEffect(() => {
    setSessionRebindHandler(async (sessionId, backendSessionId, cwd, daemonEpoch) => {
      const current = stateRef.current;
      const nextState = workspaceReducer(current, {
        type: "REBIND_SESSION_BACKEND",
        sessionId,
        backendSessionId,
        cwd,
        daemonEpoch,
      });
      dispatchWorkspaceAction({
        type: "REBIND_SESSION_BACKEND",
        sessionId,
        backendSessionId,
        cwd,
        daemonEpoch,
      });
      const project = activeProjectRef.current;
      if (project) {
        await persistSessionStrict(project.workspaceId, project.repoRoot, nextState).catch((err) => {
          console.error("Failed to persist rebound session:", err);
        });
      }
    });
    return () => {
      setSessionRebindHandler(null);
    };
  }, [dispatchWorkspaceAction, persistSessionStrict]);

  useEffect(() => {
    if (registeredProjectId !== activeProject.workspaceId || pendingAgentAutoResume === null) return;
    const pending = pendingAgentAutoResume;
    setPendingAgentAutoResume(null);
    if (pending.workspaceId !== activeProject.workspaceId) return;
    if (activeProject.target?.kind === "ssh") return;

    activeAutoResumeCancelRef.current?.();
    activeAutoResumeCancelRef.current = scheduleAgentAutoResume({
      workspaceId: activeProject.workspaceId,
      state: pending.state,
      recoveredFromHmr,
      reconnect: async (sessionId) => {
        // The app, not the user, initiated this resume; the working→idle blip it
        // produces when the agent lands back at its prompt is not user attention.
        dispatchWorkspaceAction({ type: "SUPPRESS_NEXT_ATTENTION", sessionId });
        try {
          await handleReconnectAgentSession(sessionId, { silent: true });
        } catch {
          // If auto-resume fails in background, fallback to fresh shell in that worktree
          // so user never lands on a dead "Session disconnected / Reconnect" screen.
          await ensureSessionBackends?.([sessionId], { fallbackToShell: true })?.catch((fallbackError) => {
            reportRuntimeError(fallbackError);
          });
        }
      },
    });
  }, [activeProject.workspaceId, dispatchWorkspaceAction, ensureSessionBackends, handleReconnectAgentSession, pendingAgentAutoResume, recoveredFromHmr, registeredProjectId, reportRuntimeError]);

  useEffect(() => {
    const unregister = registerWindowCloseGuard(async () => {
      const snapshot = stateRef.current;
      const target = activeProjectRef.current;
      if (snapshot.workspaceId !== undefined && snapshot.workspaceId !== target.workspaceId) return;
      await persistSession(target.workspaceId, target.repoRoot, snapshot);
    });
    return unregister;
  }, [persistSession]);

  // Saves are scheduled off this key, so it has to carry the agent resume identity too: `/new`
  // rotates providerSession without touching the backend id or the lifecycle, and a rotation that
  // does not schedule a save only reaches disk if some unrelated change saves later.
  const persistedSessionsKey = useMemo(() => sessionPersistenceKey(state.sessions), [state.sessions]);

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
    persistedSessionsKey,
    state.layout,
    state.worktreeLayouts,
    state.worktrees,
    state.activeWorktreePath,
    state.workspaceId,
  ]);

  const [isAddProjectOpen, setIsAddProjectOpen] = useState(false);
  const [addProjectHostId, setAddProjectHostId] = useState<string | undefined>(undefined);
  const [createTargetProject, setCreateTargetProject] = useState<RegisteredProject | null>(null);
  const [pendingProjectRemove, setPendingProjectRemove] = useState<RegisteredProject | null>(null);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isInboxOpen, setIsInboxOpen] = useState(false);
  const [onboardingSteps, setOnboardingSteps] = useState<OnboardingStepId[] | null>(null);
  const [onboardingInitialStepIndex, setOnboardingInitialStepIndex] = useState(0);
  const [onboardingDoneSteps, setOnboardingDoneSteps] = useState<readonly OnboardingStepId[]>([]);
  const [onboardingPermissions, setOnboardingPermissions] = useState<SystemPermissionsStatus | null>(null);
  const [whatsNew, setWhatsNew] = useState<WhatsNewEntry | null>(null);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SectionId | undefined>(undefined);
  const [searchLeafId, setSearchLeafId] = useState<string | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpen);
  const [deleteTarget, setDeleteTarget] = useState<Worktree | null>(null);
  const deleteOwnerId = deleteTarget ? resolveWorktreeOwnerId(deleteTarget, projects, activeProject.workspaceId) : undefined;
  const deleteOwnerProject = projects.find((p) => p.workspaceId === deleteOwnerId);
  const [diskManageProject, setDiskManageProject] = useState<RegisteredProject | null>(null);
  const [historyProject, setHistoryProject] = useState<RegisteredProject | null>(null);
  const notificationInbox = useSyncExternalStore(notificationCenterStore.subscribe, notificationCenterStore.getSnapshot);
  const attentionRows = useMemo(
    () => buildAttentionRows(notificationInbox.entries, liveActivityLookup(state, listWorkspaceSnapshots())),
    [notificationInbox, state, parkedActivityVersion],
  );
  const openSessionCount = Object.keys(state.sessions).length + listWorkspaceSnapshots()
    .filter(([workspaceId]) => workspaceId !== activeProject.workspaceId)
    .reduce((count, [, snapshot]) => count + Object.keys(snapshot.sessions).length, 0);
  const [pendingTabClose, setPendingTabClose] = useState<{
    kind: "pane" | "tab";
    tabId: string;
    leafId?: string;
    label: string;
    activeAgentCount: number;
  } | null>(null);
  const [worktreeStatuses, setWorktreeStatuses] = useState<Record<string, DirtyState | undefined>>({});
  const [pendingWorktree, setPendingWorktree] = useState<Worktree | null>(null);
  const pendingWorktreePath = pendingWorktree?.path ?? null;
  const setPendingWorktreePath = useCallback((path: string | null) => {
    if (path === null) {
      setPendingWorktree(null);
    } else {
      setPendingWorktree((current) => (current?.path === path ? current : { path, head: "", branch: null, bare: false, detached: false, locked: null, prunable: null }));
    }
  }, []);
  const [pendingRemoteSlug, setPendingRemoteSlug] = useState<{
    workspaceId: string;
    createTerminal?: boolean;
    slug?: string | null;
    tabId?: string | null;
  } | null>(null);
  const [pendingNotificationTarget, setPendingNotificationTarget] = useState<InboxNavigationTarget | null>(null);

  const focusedTerminalPayload = useMemo(
    () => deriveFocusedTerminal(activeProject.workspaceId, state, listWorkspaceSnapshots()),
    [activeProject.workspaceId, state, parkedActivityVersion],
  );
  const publishedTerminalPayloadKey = useRef<string | null>(null);

  useEffect(() => {
    const key = JSON.stringify(focusedTerminalPayload);
    if (publishedTerminalPayloadKey.current === key) return;
    publishedTerminalPayloadKey.current = key;
    void Promise.resolve(publishFocusedTerminal(focusedTerminalPayload)).catch(reportRuntimeError);
  }, [focusedTerminalPayload, reportRuntimeError]);

  const unreadBadgeCount = useMemo(
    () => unreadBadgeCountFromStore ?? selectGlobalUnreadBadgeCount(state, activeProject.workspaceId),
    [unreadBadgeCountFromStore, state, activeProject.workspaceId],
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
      if (project.target?.kind === "pairedDaemon" && remoteHostStore.getState().nativeStatus === "unavailable") {
        void pairedHostInventory.refresh();
      }
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
    const current = projectsRef.current;
    const next = [...current.filter((candidate) => candidate.workspaceId !== project.workspaceId && !(
      candidate.repoRoot === project.repoRoot &&
      (candidate.target?.kind === "ssh"
        ? project.target?.kind === "ssh" && candidate.target.hostId === project.target.hostId
        : project.target?.kind !== "ssh")
    )), project];
    persistProjects(next);
    setProjects(next);
    setActiveProjectId(project.workspaceId);
    persistActiveProjectId(project.workspaceId);
    setWorktreeStatuses({});
  }, []);

  const handleConfirmRemoveProject = useCallback(async () => {
    if (!pendingProjectRemove) return;
    const target = pendingProjectRemove;
    setPendingProjectRemove(null);
    const current = projectsRef.current;
    const next = current.filter((candidate) => candidate.workspaceId !== target.workspaceId);
    persistProjects(next);
    setProjects(next);
    if (target.workspaceId === activeProjectId) {
      const nextActiveId = next[0]?.workspaceId ?? "";
      setActiveProjectId(nextActiveId);
      persistActiveProjectId(nextActiveId);
      setWorktreeStatuses({});
    }
    clearWorkspaceSnapshot(target.workspaceId);
    clearHmrWorkspaceState(target.workspaceId);
    try {
      await unregisterProject({ workspaceId: target.workspaceId });
      toast.success(`Removed project ${target.workspaceId}`);
    } catch (err) {
      reportRuntimeError(err);
    }
  }, [activeProjectId, pendingProjectRemove, reportRuntimeError]);

  const handleSelectWorktree = useCallback(
    (worktree: Worktree) => {
      if (activeRemoteHostRef.current) return;
      if (projectsRef.current.length === 0) return;
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
        setPendingWorktree(worktree);
        return;
      }
      if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().nativeStatus === "unavailable") {
        void pairedHostInventory.refresh();
        setPendingWorktree(worktree);
        return;
      }
      if (
        (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
        registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
      ) {
        setPendingWorktree(worktree);
        return;
      }
      runTabOperation(() => ensureTabForWorktree(worktree));
    },
    [ensureTabForWorktree, handleSelectProject, runTabOperation],
  );

  useEffect(() => {
    if (!pendingWorktree) return;
    if (activeRemoteHostRef.current) return;
    if (
      (activeProject.target?.kind === "ssh" || activeProject.target?.kind === "pairedDaemon") &&
      (registeredProjectId !== activeProject.workspaceId ||
        workspaceRestoreStatus === "idle" ||
        workspaceRestoreStatus === "loading")
    ) {
      return;
    }
    const target =
      state.worktrees.find((worktree) => worktree.path === pendingWorktree.path) ??
      inactiveProjectWorktrees[activeProject.workspaceId]?.find((worktree) => worktree.path === pendingWorktree.path) ??
      pendingWorktree;

    switchDebug("worktree.select.pending.resolved", {
      workspaceId: activeProject.workspaceId,
      worktreePath: target.path,
      tabCount: state.layout.tabs.length,
    });
    setPendingWorktree(null);
    runTabOperation(() => ensureTabForWorktree(target));
  }, [
    activeProject.workspaceId,
    activeProject.target,
    ensureTabForWorktree,
    inactiveProjectWorktrees,
    pendingWorktree,
    registeredProjectId,
    workspaceRestoreStatus,
    runTabOperation,
    reportRuntimeError,
    state.layout.tabs.length,
    state.worktrees,
  ]);

  const handleSelectTerminalTab = useCallback(
    (tabId: string) => {
      if (activeRemoteHostRef.current) return;
      const currentState = stateRef.current;
      const tab = currentState.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab) return;
      if (tab.kind === "browser" || tab.kind === "file") {
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
    if (state.workspaceId && state.workspaceId !== pendingRemoteSlug.workspaceId) return;
    if ((activeProject.target?.kind === "ssh" || activeProject.target?.kind === "pairedDaemon") &&
      (registeredProjectId !== activeProject.workspaceId ||
        workspaceRestoreStatus === "idle" || workspaceRestoreStatus === "loading")) return;
    const target = pendingRemoteSlug.slug
      ? matchWorktreeBySlug(state.worktrees, pendingRemoteSlug.slug)
      : (state.worktrees.find((wt) => worktreeIdentity(wt) === null) ??
         state.worktrees.find((wt) => wt.path === activeProject.repoRoot) ??
         state.worktrees[0]);
    if (!target) return;
    const requestedEntryId = pendingRemoteSlug.tabId ?? null;
    setPendingRemoteSlug(null);
    if (pendingRemoteSlug.createTerminal) {
      runTabOperation(() => openTab(target));
      return;
    }
    if (!isTerminalTabInWorktree(state, target, requestedEntryId)) return;
    runTabOperation(() => Promise.resolve(ensureTabForWorktree(target))
      .then(() => {
        if (requestedEntryId) {
          activateRemoteEntry(requestedEntryId);
        }
      }));
  }, [activeProject.repoRoot, activeProject.target, activeProject.workspaceId, activateRemoteEntry, ensureTabForWorktree, openTab, pendingRemoteSlug, registeredProjectId, runTabOperation, state.worktrees, state.workspaceId, workspaceRestoreStatus]);

  const handleRemoteSelectionRequested = useCallback(
    (payload: RemoteSelectionRequestedPayload) => {
      if (!payload || !payload.workspaceId) return;
      const targetProject = projectsRef.current.find((p) => p.workspaceId === payload.workspaceId);
      const isCurrentProject = activeProjectRef.current.workspaceId === payload.workspaceId;
      const requestedEntryId = payload.tabId ?? payload.activeTabId ?? null;
      if (payload.createTerminal && (requestedEntryId || payload.sessionId)) return;

      if (payload.sessionId && !requestedEntryId) {
        if (!targetProject) return;
        const snapshot = isCurrentProject ? stateRef.current
          : getHmrWorkspaceState(payload.workspaceId) ?? getWorkspaceSnapshot(payload.workspaceId);
        const session = snapshot && Object.values(snapshot.sessions)
          .find((candidate) => candidate.backendSessionId === payload.sessionId);
        if (!snapshot || !session || !hasNavigableSession(snapshot, session.id)) return;
        if (isCurrentProject) {
          dispatchWorkspaceAction({ type: "FOCUS_EXISTING_SESSION", sessionId: session.id });
        } else {
          handleSelectProject(targetProject);
          setPendingNotificationTarget({ workspaceId: payload.workspaceId, sessionId: session.id });
        }
        return;
      }

      if (isCurrentProject) {
        if (!requestedEntryId) {
          setPendingRemoteSlug({ workspaceId: payload.workspaceId, slug: payload.worktreeSlug ?? null, createTerminal: payload.createTerminal });
          return;
        }
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
          createTerminal: payload.createTerminal,
        });
      }
    },
    [activateRemoteEntry, dispatchWorkspaceAction, ensureTabForWorktree, handleSelectProject, reportRuntimeError],
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

  const handleNotificationTarget = useCallback(
    (target: InboxNavigationTarget) => {
      if (!target || !target.workspaceId || !target.sessionId) return;
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === target.workspaceId);
      // An unregistered/removed project is stale: never navigate or fall back to another pane.
      if (!project) return;
      if (activeProjectRef.current.workspaceId === target.workspaceId) {
        // A closed session with no layout leaf is stale: reject without spawning. The reducer
        // no-ops on the same check, but guarding here avoids a needless dispatch.
        if (!hasNavigableSession(stateRef.current, target.sessionId)) return;
        dispatchWorkspaceAction({ type: "FOCUS_EXISTING_SESSION", sessionId: target.sessionId });
        acknowledgeNotificationTarget(target);
        return;
      }
      // Cross-project: inspect the target workspace's cached state BEFORE switching. A stale or
      // closed session must not yank the user into another project. Match how the workspace will
      // mount (HMR first, then the persisted snapshot).
      const cached = getHmrWorkspaceState(target.workspaceId) ?? getWorkspaceSnapshot(target.workspaceId);
      if (!cached || !hasNavigableSession(cached, target.sessionId)) return;
      handleSelectProject(project);
      setPendingNotificationTarget(target);
    },
    [dispatchWorkspaceAction, handleSelectProject],
  );

  const handleOpenAttentionRow = useCallback((row: AttentionRow) => {
    const project = projectsRef.current.find((candidate) => candidate.workspaceId === row.workspaceId);
    const liveState = row.workspaceId === activeProjectRef.current.workspaceId
      ? stateRef.current
      : getHmrWorkspaceState(row.workspaceId) ?? getWorkspaceSnapshot(row.workspaceId);
    if (!project || !liveState || !hasNavigableSession(liveState, row.sessionId)) {
      // A row that can no longer take you anywhere is noise; drop it instead of leaving a dead click.
      notificationCenterStore.dismissSession(row.workspaceId, row.sessionId);
      toast.info("Session closed; notification cleared");
      return;
    }
    handleNotificationTarget({ workspaceId: row.workspaceId, sessionId: row.sessionId, revision: row.revision });
    flashPane(row.sessionId);
  }, [handleNotificationTarget]);

  const sidebarAttention = useMemo<SidebarAttention>(() => ({
    rows: attentionRows,
    onOpen: handleOpenAttentionRow,
    onDismiss: (row) => notificationCenterStore.markEntriesRead([{ id: row.id, expectedRevision: row.revision }]),
    onDismissAll: () => notificationCenterStore.markEntriesRead(
      attentionRows.map((row) => ({ id: row.id, expectedRevision: row.revision })),
    ),
    openSessionCount,
    inboxOpen: isInboxOpen,
    onInboxOpenChange: setIsInboxOpen,
  }), [attentionRows, handleOpenAttentionRow, isInboxOpen, openSessionCount]);

  useEffect(() => {
    if (!pendingNotificationTarget) return;
    if (pendingNotificationTarget.workspaceId !== activeProject.workspaceId) {
      setPendingNotificationTarget(null);
      return;
    }
    // Apply only after the desired workspace state is mounted; applying against the outgoing
    // state would focus the wrong pane.
    if (state.workspaceId !== pendingNotificationTarget.workspaceId) return;
    // Now mounted: navigate if still navigable, otherwise drop the pending target so a session
    // that closed during the switch neither lingers nor triggers a fallback selection.
    const sessionId = pendingNotificationTarget.sessionId;
    setPendingNotificationTarget(null);
    if (hasNavigableSession(state, sessionId)) {
      dispatchWorkspaceAction({ type: "FOCUS_EXISTING_SESSION", sessionId });
      acknowledgeNotificationTarget(pendingNotificationTarget);
    }
  }, [
    activeProject.workspaceId,
    dispatchWorkspaceAction,
    pendingNotificationTarget,
    state.sessions,
    state.workspaceId,
  ]);

  const handleNotificationTargetRef = useRef(handleNotificationTarget);
  handleNotificationTargetRef.current = handleNotificationTarget;
  useEffect(
    () =>
      subscribeNotificationActivations(
        (target) => handleNotificationTargetRef.current(target),
        undefined,
        (error) => reportRuntimeErrorRef.current(error),
      ),
    [],
  );

  const handleAddTerminalTab = useCallback((shell?: string) => {
    if (activeRemoteHostRef.current) return;
    if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) return;
    if (
      (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
      registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
    ) {
      return;
    }
    const activeWt = activeWorktreeRef.current;
    if (!activeWt) return;
    runTabOperation(() => openTab(activeWt, undefined, undefined, shell));
  }, [openTab, runTabOperation]);

  const handleLaunchAgent = useCallback(
    async (agent: { name: string; command: string; args: string }) => {
      if (activeRemoteHostRef.current) return;
      if (
        (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
        registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
      ) {
        return;
      }
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
    void onNewTerminalTabMenu(() => {
      if (activeRemoteHostRef.current) return;
      handleAddTerminalTab();
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlisten = dispose;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handleAddTerminalTab]);

  const closedTabStackRef = useRef(createClosedTabStack());

  const handleCloseTab = useCallback(
    (tabId: string) => {
      if (activeRemoteHostRef.current) return;
      const tab = stateRef.current.layout.tabs.find((candidate) => candidate.id === tabId);
      if (!tab || tab.pinned) return;
      if (tab.kind === "browser" && tab.url) {
        closedTabStackRef.current.push({
          kind: "browser",
          url: tab.url,
          profileId: tab.profileId,
          worktreePath: tab.worktreePath,
        });
      } else if (tab.kind === "file") {
        closedTabStackRef.current.push({
          kind: "file",
          source: {
            leafId: tab.previewId,
            sessionId: tab.previewId,
            backendSessionId: tab.backendSessionId,
            workspaceId: tab.workspaceId,
          },
          request: {
            path: tab.path,
            backendSessionId: tab.backendSessionId,
            line: tab.line,
            col: tab.col,
          },
        });
      }
      const currentState = stateRef.current;
      const sessionIds = tab.kind === "terminal"
        ? Object.values(currentState.layout.layoutsByTabId?.[tabId]?.sessionIdsByLeafId ?? {}).filter(Boolean)
        : [];
      const activeAgentCount = sessionIds.filter((sessionId) => {
        const activity = currentState.activityBySessionId?.[sessionId];
        return activity?.state === "working" || activity?.state === "waiting";
      }).length;
      if (activeAgentCount > 0 || generalSettings.confirmCloseTab) {
        setPendingTabClose({ kind: "tab", tabId: tab.id, label: isTerminalTab(tab) ? staticTabLabel(tab) : tab.label, activeAgentCount });
        return;
      }
      void closeTab(tabId).catch(reportRuntimeError);
    },
    [closeTab, generalSettings.confirmCloseTab, reportRuntimeError],
  );

  const handleReopenClosedBrowserTab = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const entry = closedTabStackRef.current.pop();
    if (!entry) return;
    if (entry.kind === "file") {
      openFilePreviewTab(entry.source, entry.request);
      return;
    }
    void createBrowserTab(entry.url, undefined, {
      profileId: entry.profileId,
      worktreePath: entry.worktreePath,
    }).catch(reportRuntimeError);
  }, [createBrowserTab, openFilePreviewTab, reportRuntimeError]);

  const handleBrowserZoom = useCallback(
    async (browserId: string, direction: "in" | "out" | "reset") => {
      try {
        if (direction === "reset") {
          await setBrowserZoom(browserId, 1.0);
          return;
        }
        const state = await getBrowserState(browserId);
        const current = typeof state?.zoomFactor === "number" && !isNaN(state.zoomFactor) ? state.zoomFactor : 1.0;
        const next = direction === "in"
          ? Math.min(5.0, Number((current + 0.1).toFixed(2)))
          : Math.max(0.25, Number((current - 0.1).toFixed(2)));
        await setBrowserZoom(browserId, next);
      } catch (err) {
        reportRuntimeError(err);
      }
    },
    [reportRuntimeError],
  );

  const handleClosePane = useCallback(
    (tabId: string, leafId: string) => {
      if (activeRemoteHostRef.current) return;
      const currentState = stateRef.current;
      const tabLayout = currentState.layout.layoutsByTabId?.[tabId];
      if (tabLayout?.root.type === "leaf") {
        const tab = currentState.layout.tabs.find((candidate) => candidate.id === tabId);
        const sessionIds = tab?.kind === "terminal"
          ? Object.values(tabLayout.sessionIdsByLeafId ?? {}).filter(Boolean)
          : [];
        const activeAgentCount = sessionIds.filter((sessionId) => {
          const activity = currentState.activityBySessionId?.[sessionId];
          return activity?.state === "working" || activity?.state === "waiting";
        }).length;
        if (activeAgentCount > 0 || (generalSettings.confirmCloseTab && !tab?.pinned)) {
          setPendingTabClose({ kind: "tab", tabId, leafId, label: tab && isTerminalTab(tab) ? staticTabLabel(tab) : (tab?.label ?? ""), activeAgentCount });
          return;
        }
      }
      void closePane(tabId, leafId).catch(reportRuntimeError);
    },
    [closePane, generalSettings.confirmCloseTab, reportRuntimeError],
  );

  const handleCloseActiveSurface = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const currentState = stateRef.current;
    const activeTabId = currentState.layout.activeTabId;
    if (!activeTabId) return;

    const activeTab = currentState.layout.tabs.find((tab) => tab.id === activeTabId);
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTabId];
    if (activeTab && activeTab.kind !== "browser" && activeLayout) {
      const activeLeafId = activeLayout.activeLeafId ?? collectLeafIds(activeLayout.root)[0];
      if (activeLeafId) {
        handleClosePane(activeTabId, activeLeafId);
        return;
      }
    }

    handleCloseTab(activeTabId);
  }, [handleClosePane, handleCloseTab]);

  const handleConfirmTabClose = useCallback(() => {
    if (!pendingTabClose) return;
    const { tabId, leafId } = pendingTabClose;
    setPendingTabClose(null);
    if (leafId) {
      void closePane(tabId, leafId).catch(reportRuntimeError);
    } else {
      void closeTab(tabId).catch(reportRuntimeError);
    }
  }, [closePane, closeTab, pendingTabClose, reportRuntimeError]);

  const handleCloseOtherTabs = useCallback(
    (tabId: string) => {
      if (activeRemoteHostRef.current) return;
      void closeOtherTabs(tabId).catch(reportRuntimeError);
    },
    [closeOtherTabs, reportRuntimeError],
  );

  const handleCloseTabsToRight = useCallback(
    (tabId: string) => {
      if (activeRemoteHostRef.current) return;
      void closeTabsToRight(tabId).catch(reportRuntimeError);
    },
    [closeTabsToRight, reportRuntimeError],
  );

  const handleCloseTabsToLeft = useCallback(
    (tabId: string) => {
      if (activeRemoteHostRef.current) return;
      void closeTabsToLeft(tabId).catch(reportRuntimeError);
    },
    [closeTabsToLeft, reportRuntimeError],
  );

  const handleCycleTab = useCallback(
    (offset: number) => {
      if (activeRemoteHostRef.current) return;
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
      if (activeRemoteHostRef.current) {
        switchDebug("split.active.refused.remote-host", { reason: `direction=${direction}` });
        return;
      }
      if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) {
        switchDebug("split.active.refused.paired-feature-gated", { reason: `direction=${direction}` });
        return;
      }
      const currentState = stateRef.current;
      const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") {
        switchDebug("split.active.refused.no-terminal-tab", {
          reason: activeTab ? `tab-kind=${activeTab.kind}` : "no-active-tab",
        });
        return;
      }
      const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
      if (!activeLayout) {
        switchDebug("split.active.fallback.leaf-default.layout-missing", { reason: `tabId=${activeTab.id}` });
      } else if (!activeLayout.activeLeafId) {
        switchDebug("split.active.fallback.leaf-default.active-leaf-missing", { reason: `tabId=${activeTab.id}` });
      }
      const targetLeafId = activeLayout?.activeLeafId ??
        (activeLayout?.root ? collectLeafIds(activeLayout.root)[0] : "leaf-default");
      switchDebug("split.active.requested", {
        reason: `direction=${direction} targetLeafId=${targetLeafId}`,
      });
      void splitPane(activeTab.id, targetLeafId, direction).catch(reportRuntimeError);
    },
    [reportRuntimeError, splitPane],
  );

  const handleUnsplitActive = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const currentState = stateRef.current;
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
    if (!activeLayout || activeLayout.root.type === "leaf") return;
    const activeLeafId = activeLayout.activeLeafId ?? collectLeafIds(activeLayout.root)[0];
    void closePane(activeTab.id, activeLeafId).catch(reportRuntimeError);
  }, [closePane, reportRuntimeError]);

  const handleCyclePaneFocus = useCallback(
    (offset: number) => {
      if (activeRemoteHostRef.current) return;
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
    if (activeRemoteHostRef.current) return;
    const currentState = stateRef.current;
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
    if (!activeTab || activeTab.kind === "browser") return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
    const leafId = activeLayout?.activeLeafId ?? (activeLayout?.root ? collectLeafIds(activeLayout.root)[0] : "leaf-default");
    setSearchLeafId(leafId);
  }, []);

  const handleSelectWorktreeByIndex = useCallback(
    (index: number) => {
      if (activeRemoteHostRef.current) return;
      const visible = listShortcutWorktrees(
        projectsRef.current,
        stateRef.current.worktrees,
        activeProjectRef.current.workspaceId,
        inactiveProjectWorktreesRef.current,
        stateRef.current,
        listWorkspaceSnapshots(),
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
    if (activeRemoteHostRef.current) return;
    const nextSize = Math.min(36, terminalSettingsRef.current.fontSize + 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [updateTerminalSettings]);

  const handleZoomOut = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const nextSize = Math.max(10, terminalSettingsRef.current.fontSize - 1);
    updateTerminalSettings({ fontSize: nextSize });
  }, [updateTerminalSettings]);

  const handleZoomReset = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    updateTerminalSettings({ fontSize: null });
  }, [updateTerminalSettings]);

  const handleSelectTerminalTabByIndex = useCallback(
    (index: number) => {
      if (activeRemoteHostRef.current) return;
      const currentState = stateRef.current;
      const focusedGroup = currentState.layout.focusedGroupId
        ? currentState.layout.tabGroups?.[currentState.layout.focusedGroupId]
        : undefined;
      const tabId = focusedGroup?.tabIds[index] ?? currentState.layout.tabs[index]?.id;
      if (tabId) handleSelectTerminalTab(tabId);
    },
    [handleSelectTerminalTab],
  );

  const handleOpenAddProject = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    setAddProjectHostId(undefined);
    setIsAddProjectOpen(true);
  }, []);
  const handleOpenSshProject = useCallback((hostId: string) => {
    if (activeRemoteHostRef.current) return;
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
    setAddProjectHostId(hostId);
    setIsAddProjectOpen(true);
  }, []);
  const handleCloseAddProject = useCallback(() => setIsAddProjectOpen(false), []);
  const handleOpenCreateWorktree = useCallback((project?: RegisteredProject) => {
    if (activeRemoteHostRef.current) return;
    const target = project ?? activeProjectRef.current;
    setCreateTargetProject(target);
    setIsCreateOpen(true);
  }, []);
  const handleCloseCreateWorktree = useCallback(() => {
    setIsCreateOpen(false);
    setCreateTargetProject(null);
  }, []);
  const handleOpenCommandPalette = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    setIsCommandPaletteOpen(true);
  }, []);
  const handleCloseCommandPalette = useCallback(() => setIsCommandPaletteOpen(false), []);

  useEffect(() => {
    if (activeRemoteHost) {
      setIsCommandPaletteOpen(false);
    }
  }, [activeRemoteHost]);
  const handleOpenSettings = useCallback((section?: SectionId) => {
    preloadSettingsDialog();
    const validSection =
      typeof section === "string" &&
      [
        "general",
        "appearance",
        "terminal",
        "shortcuts",
        "agents",
        "browser",
        "notifications",
        "remote",
        "ssh",
        "permissions",
      ].includes(section)
        ? (section as SectionId)
        : undefined;
    setSettingsInitialSection(validSection);
    setIsSettingsOpen(true);
  }, []);
  handleOpenSettingsRef.current = handleOpenSettings;
  const handleOpenSshSettings = useCallback(() => handleOpenSettings("ssh"), [handleOpenSettings]);
  const handleCloseSettings = useCallback(() => {
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
  }, []);

  useEffect(() => {
    if (!isNativeRuntime || activeRemoteHostRef.current) return;
    let cancelled = false;
    const timer = setTimeout(async () => {
      if (activeRemoteHostRef.current) return;
      const [permissions, cli, enrollment] = await Promise.all([
        getSystemPermissionsStatus().catch(() => null),
        getCliLauncherStatus().catch(() => null),
        getAccountEnrollmentStatus().catch(() => null),
      ]);
      if (cancelled) return;
      if (permissions) {
        setOnboardingPermissions(permissions);
      }
      const ctx: OnboardingContext = {
        permissions,
        agents: resolvedAgentsRef.current,
        cli,
        projectCount: projectsRef.current.length,
        accountLinked: enrollment?.enrolled === true,
      };
      const state = loadOnboardingState();
      if (shouldAutoOpenOnboarding(state, ctx)) {
        const wizard = wizardSteps(ctx);
        setOnboardingDoneSteps(satisfiedOnboardingSteps(state, ctx));
        setOnboardingInitialStepIndex(initialWizardStepIndex(wizard, state, ctx));
        setOnboardingSteps(wizard);
      }
    }, 1200);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [isNativeRuntime]);

  useEffect(() => {
    let cancelled = false;
    const handleOpenOnboarding = async () => {
      const [permissions, cli, enrollment] = await Promise.all([
        getSystemPermissionsStatus().catch(() => null),
        getCliLauncherStatus().catch(() => null),
        getAccountEnrollmentStatus().catch(() => null),
      ]);
      if (cancelled) return;
      if (permissions) {
        setOnboardingPermissions(permissions);
      }
      const ctx: OnboardingContext = {
        permissions,
        agents: resolvedAgentsRef.current,
        cli,
        projectCount: projectsRef.current.length,
        accountLinked: enrollment?.enrolled === true,
      };
      setOnboardingDoneSteps(satisfiedOnboardingSteps(loadOnboardingState(), ctx));
      setOnboardingInitialStepIndex(0);
      setOnboardingSteps(wizardSteps(ctx));
    };
    window.addEventListener(OPEN_ONBOARDING_EVENT, handleOpenOnboarding);
    return () => {
      cancelled = true;
      window.removeEventListener(OPEN_ONBOARDING_EVENT, handleOpenOnboarding);
    };
  }, []);

  useEffect(() => {
    if (!isNativeRuntime) return;
    return startWhatsNewRecorder();
  }, [isNativeRuntime]);

  useEffect(() => {
    if (!isNativeRuntime) return;
    let cancelled = false;
    void getCurrentVersion().then((version) => {
      if (cancelled || !version) return;
      const entry = resolveWhatsNew(version);
      if (!cancelled && entry) {
        setWhatsNew(entry);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [isNativeRuntime]);

  const handleToggleSettings = useCallback(() => {
    preloadSettingsDialog();
    setIsSettingsOpen((current) => !current);
  }, []);
  const handleToggleNotificationCenter = useCallback(() => {
    // The inbox lives inside the sidebar, so reaching it from a collapsed sidebar opens both.
    if (!isSidebarOpen) {
      setIsInboxOpen(true);
      toggleSidebar();
      return;
    }
    setIsInboxOpen((open) => !open);
  }, [isSidebarOpen, toggleSidebar]);
  const handleCloseSearch = useCallback(() => setSearchLeafId(null), []);
  const handleCloseDeleteTarget = useCallback(() => setDeleteTarget(null), []);
  const handleDeleteWorktree = useCallback((worktree: Worktree) => {
    const isRemote = Boolean(worktree.workspaceId?.startsWith("ssh:"));
    const isPrimary = worktreeIdentity(worktree) === null;
    if (isRemote && isPrimary) {
      const project = projectsRef.current.find((p) => p.workspaceId === worktree.workspaceId);
      if (project) {
        setPendingProjectRemove(project);
        return;
      }
    }
    setDeleteTarget(worktree);
  }, []);
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
    (tabId: string, url: string, browserId?: string) => {
      void navigateBrowserTab(tabId, url, browserId).catch(reportRuntimeError);
    },
    [navigateBrowserTab, reportRuntimeError],
  );

  const handleReloadBrowserTab = useCallback(
    (tabId: string, browserId?: string, options?: BrowserReloadOptions) => {
      void reloadBrowserTab(tabId, browserId, options).catch(reportRuntimeError);
    },
    [reloadBrowserTab, reportRuntimeError],
  );

  const handleSplitPane = useCallback(
    (tabId: string, leafId: string, direction: PaneDirection, options?: { position?: "first" | "second" }) => {
      if (activeRemoteHostRef.current) return;
      if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) return;
      if (
        (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
        registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
      ) {
        return;
      }
      void splitPane(tabId, leafId, direction, options).catch(reportRuntimeError);
    },
    [reportRuntimeError, splitPane],
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onCloseTabMenu(() => {
      if (activeRemoteHostRef.current) return;
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
      if (activeRemoteHostRef.current) return;
      void createBrowserTab(payload.targetUrl, undefined, {
        profileId: payload.profileId,
        worktreePath: payload.worktreePath ?? undefined,
        opener: popupOpenerLink(payload),
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
    let disposed = false;
    let unlisten: (() => void) | undefined;
    // A popup's own page called window.close(): close the tab the host created for it. That
    // close path is also what tells the opener its handle is closed, so a library polling
    // popup.closed sees the cancel instead of waiting forever.
    void onBrowserPopupCloseRequested((payload) => {
      if (activeRemoteHostRef.current) return;
      const tabs = [
        ...stateRef.current.layout.tabs,
        ...Object.values(stateRef.current.worktreeLayouts ?? {}).flatMap((layout) => layout.tabs),
      ];
      const tabId = browserTabIdForBrowserId(tabs, payload.browserId);
      if (!tabId) return;
      void closeTab(tabId).catch(reportRuntimeError);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [closeTab, reportRuntimeError]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserLinkClicked((payload) => {
      if (activeRemoteHostRef.current) return;
      const settings = loadBrowserSettings();
      const target = payload.modifier ? settings.modifierClickTarget : settings.linkClickTarget;
      if (target === "external") {
        void openExternalUrl(payload.targetUrl).catch(reportRuntimeError);
        return;
      }
      if (payload.modifier) {
        void createBrowserTab(payload.targetUrl, undefined, {
          profileId: payload.profileId,
          worktreePath: payload.worktreePath ?? undefined,
        }).catch(reportRuntimeError);
        return;
      }
      void navigateBrowser(payload.browserId, payload.targetUrl).catch(reportRuntimeError);
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
    const onOpenFilePreview = (event: Event) => {
      const detail = (event as CustomEvent<{
        source: Parameters<typeof openFilePreviewTab>[0];
        request: Parameters<typeof openFilePreviewTab>[1];
      }>).detail;
      if (!detail?.source || !detail.request) return;
      openFilePreviewTab(detail.source, detail.request);
    };
    window.addEventListener("ferryx:open-file-preview", onOpenFilePreview);
    return () => window.removeEventListener("ferryx:open-file-preview", onOpenFilePreview);
  }, [openFilePreviewTab]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<unknown>(CLI_OPEN_FILE_EVENT, (event) => {
      const parsed = parseCliOpenFilePayload(event.payload);
      if (!parsed) return;

      const currentState = stateRef.current;
      const activeWorkspaceId =
        currentState.workspaceId ?? activeProjectRef.current?.workspaceId ?? null;
      const focusedGroup = currentState.layout.focusedGroupId
        ? currentState.layout.tabGroups?.[currentState.layout.focusedGroupId]
        : undefined;
      const activeTabId =
        focusedGroup?.activeTabId ?? currentState.layout.activeTabId ?? currentState.layout.tabs[0]?.id ?? null;
      const focusedCandidate = activeTabId
        ? currentState.layout.tabs.find((tab) => tab.id === activeTabId)
        : undefined;
      const focusedTab =
        focusedCandidate && isTerminalTab(focusedCandidate) ? focusedCandidate : null;
      const tabLayout = focusedTab ? currentState.layout.layoutsByTabId?.[focusedTab.id] : undefined;
      const activeLeafId =
        tabLayout?.activeLeafId ?? (tabLayout?.root ? collectLeafIds(tabLayout.root)[0] : null);
      const localSessionId = focusedTab
        ? (activeLeafId && tabLayout?.sessionIdsByLeafId?.[activeLeafId]) || focusedTab.sessionId
        : null;
      const session = localSessionId ? currentState.sessions[localSessionId] : undefined;

      const source: Parameters<typeof openFilePreviewTab>[0] =
        focusedTab && activeLeafId && localSessionId && session?.backendSessionId
          ? {
              leafId: activeLeafId,
              sessionId: localSessionId,
              backendSessionId: session.backendSessionId,
              workspaceId: session.workspaceId ?? activeWorkspaceId,
            }
          : {
              leafId: "cli",
              sessionId: "cli",
              backendSessionId: "cli",
              workspaceId: activeWorkspaceId ?? null,
            };

      const request: Parameters<typeof openFilePreviewTab>[1] = {
        path: parsed.path,
        backendSessionId: source.backendSessionId,
        line: parsed.line ?? null,
        col: parsed.col ?? null,
      };

      openFilePreviewTab(source, request);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [openFilePreviewTab, reportRuntimeError]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserSessionCreated((payload) => {
      if (activeRemoteHostRef.current) {
        toast.warning("Browser tab was not shown because a remote host is active.");
        void reportBrowserAdoption(payload.browser.browserId, false, "remote-host-active").catch(reportRuntimeError);
        return;
      }
      const adoptedTabId = adoptBrowserSession(payload.browser, payload.workspaceId ?? undefined);
      if (!adoptedTabId) {
        toast.warning("Browser tab was not shown: session could not be adopted.");
        void reportBrowserAdoption(payload.browser.browserId, false, "adopt-failed").catch(reportRuntimeError);
        return;
      }
      void reportBrowserAdoption(payload.browser.browserId, true).catch(reportRuntimeError);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [adoptBrowserSession, reportRuntimeError]);

  // `ferryx browser tab switch` names a browser session; only this window knows which layout tab
  // owns it, and the CLI's tab index is a different index space from the layout tab order.
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserTabSwitch((payload) => {
      const { browserId } = payload;
      const owner = stateRef.current.layout.tabs.find((tab) =>
        tab.kind === "browser"
          ? tab.browserId === browserId
          : Object.values(stateRef.current.layout.layoutsByTabId[tab.id]?.contentsByLeafId ?? {}).some(
              (content) =>
                content.kind === "browser" &&
                (content.browser?.browserId ?? content.browserId) === browserId,
            ),
      );
      if (!owner) {
        toast.warning(`Browser tab for ${browserId} is not open in this window.`);
        return;
      }
      handleSelectTerminalTab(owner.id);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handleSelectTerminalTab, reportRuntimeError]);

  // Terminal links and markdown editors route through routeHttpLink, which needs a live
  // opener to reach the built-in browser; without this registration every link silently
  // falls back to the system browser.
  const [remoteBrowserSharingActive, setRemoteBrowserSharingActive] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    if (isTauriRuntime()) {
      import("@tauri-apps/api/event").then(({ listen }) => {
        listen<{ isSharing?: boolean; active?: boolean }>("browser-remote-sharing", (ev) => {
          setRemoteBrowserSharingActive(Boolean(ev.payload?.isSharing ?? ev.payload?.active ?? true));
        }).then((u) => {
          unlisten = u;
        }).catch(() => {});
      }).catch(() => {});
    }

    const handleWindowSharing = (e: Event) => {
      const detail = (e as CustomEvent<{ isSharing?: boolean; active?: boolean }>).detail;
      if (typeof detail === "boolean") {
        setRemoteBrowserSharingActive(detail);
      } else if (detail && typeof detail === "object") {
        setRemoteBrowserSharingActive(Boolean(detail.isSharing ?? detail.active ?? true));
      }
    };
    window.addEventListener("remote-browser-sharing", handleWindowSharing);
    return () => {
      unlisten?.();
      window.removeEventListener("remote-browser-sharing", handleWindowSharing);
    };
  }, []);

  const handleReclaimRemoteBrowser = useCallback(async () => {
    await browserRemoteReclaim();
    setRemoteBrowserSharingActive(false);
  }, []);

  useEffect(() => {
    return registerBuiltInBrowserLinkOpener((url) => {
      if (activeRemoteHostRef.current) return;
      void createBrowserTab(url).catch(reportRuntimeError);
    });
  }, [createBrowserTab, reportRuntimeError]);

  useEffect(() => {
    return registerWorktreePathOpener((path) => {
      if (activeRemoteHostRef.current) return false;
      const norm = (p: string) => {
        const cleaned = p.replace(/[\\/]+$/, "").replace(/\\/g, "/");
        return /^[A-Za-z]:/.test(cleaned) ? cleaned.toLowerCase() : cleaned;
      };
      const target = norm(path);
      const all = [
        ...stateRef.current.worktrees,
        ...Object.values(inactiveProjectWorktreesRef.current ?? {}).flat(),
      ];
      const match = all.find((w) => norm(w.path) === target);
      if (!match) return false;
      handleSelectWorktree(match);
      return true;
    });
  }, [handleSelectWorktree]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    // Cmd+1..9 never reaches the webview because the macOS Window menu claims it,
    // so the native key monitor forwards the digit as an event instead.
    void onSelectWorktreeMenu((digit) => {
      if (activeRemoteHostRef.current) return;
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

  useEffect(() => {
    let unlistenSelectTab: (() => void) | null = null;
    let unlistenNextTab: (() => void) | null = null;
    let unlistenPrevTab: (() => void) | null = null;
    let unlistenSplitRight: (() => void) | null = null;
    let unlistenSplitDown: (() => void) | null = null;
    let unlistenCmdPalette: (() => void) | null = null;
    let unlistenSidebar: (() => void) | null = null;
    let unlistenSettings: (() => void) | null = null;
    let cancelled = false;

    void onSelectTabMenu((digit) => {
      if (activeRemoteHostRef.current) return;
      handleSelectTerminalTabByIndex(digit - 1);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSelectTab = dispose;
    });

    void onNextTabMenu(() => {
      if (activeRemoteHostRef.current) return;
      handleCycleTab(1);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenNextTab = dispose;
    });

    void onPrevTabMenu(() => {
      if (activeRemoteHostRef.current) return;
      handleCycleTab(-1);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenPrevTab = dispose;
    });

    void onSplitRightMenu(() => {
      if (activeRemoteHostRef.current) return;
      handleSplitActive("horizontal");
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSplitRight = dispose;
    });

    void onSplitDownMenu(() => {
      if (activeRemoteHostRef.current) return;
      handleSplitActive("vertical");
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSplitDown = dispose;
    });

    void onCommandPaletteMenu(() => {
      if (activeRemoteHostRef.current) return;
      setIsCommandPaletteOpen(true);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenCmdPalette = dispose;
    });

    void onToggleSidebarMenu(() => {
      toggleSidebar();
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSidebar = dispose;
    });

    void onOpenSettingsMenu(() => {
      setIsSettingsOpen(true);
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSettings = dispose;
    });

    return () => {
      cancelled = true;
      unlistenSelectTab?.();
      unlistenNextTab?.();
      unlistenPrevTab?.();
      unlistenSplitRight?.();
      unlistenSplitDown?.();
      unlistenCmdPalette?.();
      unlistenSidebar?.();
      unlistenSettings?.();
    };
  }, [
    handleSelectTerminalTabByIndex,
    handleCycleTab,
    handleSplitActive,
    setIsCommandPaletteOpen,
    toggleSidebar,
    setIsSettingsOpen,
  ]);

  const activeShortcutTab = state.layout.tabs.find((tab) => tab.id === state.layout.activeTabId) ?? null;
  const shortcutLayout = activeShortcutTab ? state.layout.layoutsByTabId?.[activeShortcutTab.id] : undefined;
  const shortcutLeafId = shortcutLayout?.activeLeafId ?? (shortcutLayout ? collectLeafIds(shortcutLayout.root)[0] : undefined);
  const shortcutContent = shortcutLeafId ? shortcutLayout?.contentsByLeafId?.[shortcutLeafId] : undefined;
  // Match PaneRenderer: explicit leaf content wins; absent content inherits the tab kind.
  const shortcutBrowserId = shortcutContent
    ? shortcutContent.kind === "browser" ? shortcutContent.browser?.browserId ?? shortcutContent.browserId : undefined
    : activeShortcutTab?.kind === "browser" ? activeShortcutTab.browserId : undefined;
  const browserShortcutsActive = Boolean(shortcutBrowserId);
  const dispatchBrowserShortcut = (action: BrowserShortcutAction) => {
    if (!shortcutBrowserId) return;
    window.dispatchEvent(new CustomEvent(BROWSER_SHORTCUT_EVENT, {
      detail: { browserId: shortcutBrowserId, action },
    }) satisfies BrowserShortcutDomEvent);
  };

  // Child browser webviews own OS focus, so the main window never sees these
  // The embedded browser webview owns native keyboard focus while active, which
  // stops the main application webview from receiving DOM keydown events. The
  // guest bridge forwards app and tab shortcuts as shortcut events; route them
  // here (page actions like find/reload/focus-address are consumed by
  // BrowserPane/BrowserToolbar listeners on the same event).
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void onBrowserShortcutRequested((payload) => {
      if (activeRemoteHostRef.current) return;
      const action = payload.action;
      if (action === "tab-next") {
        handleCycleTab(1);
        return;
      }
      if (action === "tab-previous") {
        handleCycleTab(-1);
        return;
      }
      if (action === "tab-new-terminal") {
        handleAddTerminalTab();
        return;
      }
      if ((action as string) === "tab-new-browser") {
        void createBrowserTab(newBrowserTabUrl()).catch(reportRuntimeError);
        return;
      }
      if ((action as string) === "tab-reopen-closed") {
        handleReopenClosedBrowserTab();
        return;
      }
      if ((action as string) === "zoom-in") {
        const targetId = payload.browserId || shortcutBrowserId;
        if (targetId) void handleBrowserZoom(targetId, "in");
        return;
      }
      if ((action as string) === "zoom-out") {
        const targetId = payload.browserId || shortcutBrowserId;
        if (targetId) void handleBrowserZoom(targetId, "out");
        return;
      }
      if ((action as string) === "zoom-reset") {
        const targetId = payload.browserId || shortcutBrowserId;
        if (targetId) void handleBrowserZoom(targetId, "reset");
        return;
      }
      if (action === "tab-close") {
        handleCloseActiveSurface();
        return;
      }
      if (action === "command-palette") {
        setIsCommandPaletteOpen(true);
        return;
      }
      if (action === "sidebar-toggle") {
        toggleSidebar();
        return;
      }
      if (action === "settings-toggle") {
        setIsSettingsOpen(true);
        return;
      }
      if (action === "split-right") {
        handleSplitActive("horizontal");
        return;
      }
      if (action === "split-down") {
        handleSplitActive("vertical");
        return;
      }
      const selectIndex = browserTabSelectIndex(action);
      if (selectIndex !== null) {
        handleSelectTerminalTabByIndex(selectIndex);
        return;
      }
      const wsIndex = browserWorkspaceSelectIndex(action);
      if (wsIndex !== null) {
        handleSelectWorktreeByIndex(wsIndex);
        return;
      }
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [
    handleCycleTab,
    handleSelectTerminalTabByIndex,
    handleAddTerminalTab,
    handleCloseActiveSurface,
    toggleSidebar,
    handleSplitActive,
    handleSelectWorktreeByIndex,
    handleReopenClosedBrowserTab,
    handleBrowserZoom,
    createBrowserTab,
    shortcutBrowserId,
    reportRuntimeError,
  ]);

  const shortcutHandlers = useMemo(
    () => {
      if (activeRemoteHost) {
        return {
          "sidebar.left.toggle": toggleSidebar,
          "settings.toggle": handleToggleSettings,
          "notifications.toggle": handleToggleNotificationCenter,
        };
      }
      return {
        "tab.newTerminal": handleAddTerminalTab,
        "tab.newBrowser": () => void createBrowserTab(newBrowserTabUrl()).catch(reportRuntimeError),
        "tab.reopenClosed": handleReopenClosedBrowserTab,
        "tab.close": handleCloseActiveSurface,
        "browser.focusAddress": browserShortcutsActive ? () => dispatchBrowserShortcut("focus-address") : undefined,
        "browser.reload": browserShortcutsActive ? () => dispatchBrowserShortcut("reload") : undefined,
        "browser.hardReload": browserShortcutsActive ? () => dispatchBrowserShortcut("reload-hard") : undefined,
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
        "project.add": handleOpenAddProject,
        "commandPalette.open": handleOpenCommandPalette,
        "settings.toggle": handleToggleSettings,
        "notifications.toggle": handleToggleNotificationCenter,
        "zoom.in": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "in") : handleZoomIn,
        "zoom.out": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "out") : handleZoomOut,
        "zoom.reset": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "reset") : handleZoomReset,
      };
    },
    [
      activeRemoteHost,
      createBrowserTab,
      handleAddTerminalTab,
      handleCloseActiveSurface,
      handleCyclePaneFocus,
      browserShortcutsActive,
      shortcutBrowserId,
      handleCycleTab,
      handleOpenAddProject,
      handleOpenCommandPalette,
      handleOpenTerminalSearch,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleToggleNotificationCenter,
      handleToggleSettings,
      handleUnsplitActive,
      handleZoomIn,
      handleZoomOut,
      handleZoomReset,
      handleReopenClosedBrowserTab,
      handleBrowserZoom,
      reportRuntimeError,
      toggleSidebar,
    ],
  );
  useShortcuts(shortcutHandlers);

  const sshHostId = activeProject.target?.kind === "ssh" ? activeProject.target.hostId : null;
  const activeRegistrationError = registrationError?.workspaceId === activeProject.workspaceId
    ? `${registrationError.code}: ${registrationError.message}`
    : undefined;
  const permissionsSummary = useMemo(() => {
    if (!onboardingPermissions) return null;
    const keys = visiblePermissionKeys(onboardingPermissions);
    if (keys.length === 0) return null;
    const granted = keys.filter((k) => onboardingPermissions[k]?.granted).length;
    return { granted, total: keys.length };
  }, [onboardingPermissions]);
  const activeSshTabOperation = sshTabOperation?.workspaceId === activeProject.workspaceId ? sshTabOperation : null;
  const sshInitializing = registeredProjectId !== activeProject.workspaceId ||
    workspaceRestoreStatus === "idle" || workspaceRestoreStatus === "loading" ||
    pendingWorktreePath !== null || pendingRemoteSlug?.workspaceId === activeProject.workspaceId ||
    activeSshTabOperation !== null;
  const showSshStatus = activeProject.target?.kind === "ssh" &&
    (state.workspaceId !== activeProject.workspaceId || state.layout.tabs.length === 0) &&
    (sshInitializing || activeRegistrationError);

  return (
    <div className="flex h-screen w-screen select-none overflow-hidden bg-background font-sans text-foreground">
      <Toaster />
      <ShortcutHints
        enabledActions={SHORTCUTS.filter((shortcut) => shortcutHandlers[shortcut.id]).map((shortcut) => shortcut.id)}
        getContext={() => {
          const current = stateRef.current;
          const group = current.layout.focusedGroupId ? current.layout.tabGroups?.[current.layout.focusedGroupId] : undefined;
          const activeTab = current.layout.tabs.find((tab) => tab.id === current.layout.activeTabId);
          const paneLayout = activeTab ? current.layout.layoutsByTabId?.[activeTab.id] : undefined;
          return {
            tabIds: Array.from({ length: 9 }, (_, index) => group?.tabIds[index] ?? current.layout.tabs[index]?.id ?? ""),
            closeTabId: activeTab && !activeTab.pinned && (!paneLayout || paneLayout.root.type === "leaf") ? activeTab.id : null,
            worktrees: listShortcutWorktrees(
              projectsRef.current, current.worktrees, activeProjectRef.current.workspaceId,
              inactiveProjectWorktreesRef.current,
              current,
              listWorkspaceSnapshots(),
            ),
          };
        }}
      />
      <TerminalLinkActions />
      <TerminalFileLinkActions />
      {isSidebarOpen ? (
        <Sidebar
          open={true}
          projects={projects}
          activeProjectId={activeProject.workspaceId}
          worktrees={state.worktrees}
          inactiveProjectWorktrees={inactiveProjectWorktrees}
          emptyWorkspaceIds={emptySidebarWorkspaceIds(projects, activeProject.workspaceId, state, listWorkspaceSnapshots())}
          agents={agents}
          activePath={activeWorktree?.path || ""}
          statuses={worktreeStatuses}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          onSelectProject={handleSelectProject}
          onReorderProjects={handleReorderProjects}
          onAddProject={handleOpenAddProject}
          onRemoveProject={setPendingProjectRemove}
          onSelectWorktree={handleSelectWorktree}
          onCreateWorktree={handleOpenCreateWorktree}
          onDeleteWorktree={handleDeleteWorktree}
          onResetAgentState={handleResetWorktreeAgentState}
          onManageDisk={setDiskManageProject}
          onOpenHistory={setHistoryProject}
          onOpenSettings={handleOpenSettings}
          attention={sidebarAttention}
          onToggle={toggleSidebar}
        />
      ) : (
        <div className="relative w-0 shrink-0 overflow-visible">
          <div className="titlebar-left-floating absolute top-0 left-0 z-20 flex h-titlebar shrink-0 items-center border-b border-r border-border bg-card px-2">
            {isMacShortcutPlatform() ? (
              <div data-testid="titlebar-traffic-light-pad" className="w-[72px] shrink-0" aria-hidden="true" />
            ) : null}
            <IconButton data-shortcut="sidebar.left.toggle" label="Show sidebar" className="no-drag" size="sm" onClick={toggleSidebar}>
              <PanelLeft className="size-3.5" />
            </IconButton>
            <IconButton
              data-shortcut="notifications.toggle"
              label={attentionRows.length > 0 ? `Open inbox (${attentionRows.length})` : "Open inbox"}
              className="no-drag relative"
              size="sm"
              onClick={handleToggleNotificationCenter}
            >
              <Inbox className="size-3.5" />
              {attentionRows.length > 0 ? (
                <span
                  data-testid="collapsed-attention-badge"
                  className="absolute -right-0.5 -top-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-status-warning px-1 text-[9px] font-semibold leading-none text-black"
                >
                  {attentionRows.length > 9 ? "9+" : attentionRows.length}
                </span>
              ) : null}
            </IconButton>
          </div>
        </div>
      )}

      <main className="flex h-full min-w-0 flex-1 flex-col overflow-hidden bg-background">
        <RemoteBrowserSharingIndicator
          isSharing={remoteBrowserSharingActive}
          onReclaim={handleReclaimRemoteBrowser}
        />
        {!activeRemoteHost && pairedTerminalsUnavailable ? <div role="alert" className="px-4 py-3 text-sm text-muted-foreground">
          Paired daemon terminal support is unavailable. Enable paired projects in Settings with a compatible native proxy. Saved tabs and panes are preserved.
        </div> : null}
        {!activeRemoteHost &&
        activeProject.target?.kind !== "ssh" &&
        activeProject.target?.kind !== "pairedDaemon" &&
        registrationError?.workspaceId === activeProject.workspaceId ? (
          <DaemonConnectionBanner
            error={{
              code: registrationError.code,
              message: registrationError.message,
            }}
            onRetry={() => {
              setRegistrationError(null);
              setRegistrationAttempt((attempt) => attempt + 1);
            }}
          />
        ) : null}
        {activeRemoteHost ? (
          <div className="flex-1 flex flex-col min-h-0 bg-background overflow-hidden">
            <RemoteHostConnection
              key={activeRemoteHost.hostId}
              hostId={activeRemoteHost.hostId}
              relayUrl={activeRemoteHost.relayOrigin || activeRemoteHost.address}
              readUrlHints={false}
            />
          </div>
        ) : projects.length === 0 ? (
          <GettingStartedChecklist
            onAddProject={handleOpenAddProject}
            onConnectMachine={() => handleOpenSettings("remote")}
            onOpenWelcome={() => {
              window.dispatchEvent(new CustomEvent(OPEN_ONBOARDING_EVENT));
            }}
            permissionsSummary={permissionsSummary}
          />
        ) : showSshStatus && activeProject.target?.kind === "ssh" ? (
          <SshWorkspaceStatus
            hostLabel={getCachedSshHosts()?.find((host) => host.id === sshHostId)?.label ?? activeProject.target.hostId}
            message={registeredProjectId !== activeProject.workspaceId ? "Connecting via SSH..."
              : workspaceRestoreStatus === "idle" || workspaceRestoreStatus === "loading" ? "Restoring workspace..."
              : "Opening terminal..."}
            error={activeRegistrationError ?? activeSshTabOperation?.error}
            onRetry={() => {
              if (activeSshTabOperation?.error) activeSshTabOperation.retry();
              else {
                setRegistrationError(null);
                setRegistrationAttempt((attempt) => attempt + 1);
              }
            }}
          />
        ) : activeWorktree && state.layout.tabs.length === 0 ? (
          <EmptyWorkspaceView
            onNewTerminal={handleAddTerminalTab}
            onNewBrowserTab={handleAddBrowserTab}
          />
        ) : activeWorktree ? (
          <TerminalSplitView
            layout={state.layout}
            fileTabWorktreePath={activeWorktree.path}
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
            onResetAgentState={handleResetTabAgentState}
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
            onEqualizePaneRun={equalizePaneRun}
            onEqualizeGroupRun={equalizeTabGroupRun}
            onSwapPanes={swapPanes}
            onFocusPane={focusPane}
            searchLeafId={searchLeafId}
            onCloseSearch={handleCloseSearch}
            onReconnectAgentSession={async (sessionId) => {
              await handleReconnectAgentSession(sessionId);
            }}
            onReconnectSshSession={handleReconnectSshSession}
            onOpenNewShell={(sessionId) => {
              const session = stateRef.current.sessions[sessionId];
              const isAgent = Boolean(
                session?.agentType ||
                session?.providerSession ||
                stateRef.current.activityBySessionId?.[sessionId]?.isAgent
              );
              if (session?.agentType) {
                dispatchWorkspaceAction({ type: "RESET_AGENT_STATE", sessionId });
              }
              return replaceExitedShellSession(sessionId, {
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
                    clearAgent: isAgent,
                  });
                  await persistSessionStrict(activeProject.workspaceId, activeProject.repoRoot, nextState);
                },
              }, { clearAgent: isAgent }).then(() => undefined).catch((error) => {
                reportRuntimeError(error);
                throw error;
              });
            }}
            onBackendSessionUnavailable={(sessionId, backendSessionId, reason, bindingKey) => {
              markBackendSessionUnavailable(sessionId, backendSessionId, reason, bindingKey);
              const session = stateRef.current.sessions[sessionId];
              if (
                session &&
                isPairedWorkspaceId(session.workspaceId) &&
                !session.agentType &&
                !session.providerSession
              ) {
                setPendingBackendRecovery({
                  workspaceId: activeProjectRef.current.workspaceId,
                  sessionIds: [sessionId],
                });
              }
            }}
            leadingSpacer={isSidebarOpen ? 0 : isMacShortcutPlatform() ? 108 : 36}
          />
        ) : (
          <div className="flex h-full flex-1 items-center justify-center bg-background text-xs text-muted-foreground">
            {runtimeError ? `Workspace unavailable (${runtimeError.code})` : "No workspace available"}
          </div>
        )}
      </main>

      {isCommandPaletteOpen && !activeRemoteHost ? (
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
        <Suspense
          fallback={
            <div data-testid="settings-dialog" role="dialog" aria-label="Settings" className="fixed inset-0 z-50 flex overflow-hidden bg-background">
              <div className="w-[280px] shrink-0 border-r border-border" />
              <div className="min-w-0 flex-1" />
            </div>
          }
        >
          <SettingsDialog open initialSection={settingsInitialSection} onClose={handleCloseSettings}
            onOpenSshProject={handleOpenSshProject} />
        </Suspense>
      ) : null}
      {!activeRemoteHost && onboardingSteps && onboardingSteps.length > 0 ? (
        <Suspense fallback={null}>
          <WelcomeWizard
            steps={onboardingSteps}
            initialStepIndex={onboardingInitialStepIndex}
            doneSteps={onboardingDoneSteps}
            permissionsStatus={onboardingPermissions}
            agents={resolvedAgents}
            isMac={isMacShortcutPlatform()}
            onStepCompleted={(step) => markOnboardingStepsCompleted([step])}
            onFinish={() => setOnboardingSteps(null)}
            onSkip={() => {
              dismissOnboarding();
              setOnboardingSteps(null);
            }}
            onRemindLater={() => setOnboardingSteps(null)}
            onAddProject={() => {
              setOnboardingSteps(null);
              handleOpenAddProject();
            }}
            onConnectMachine={() => {
              setOnboardingSteps(null);
              handleOpenSettings("remote");
            }}
          />
        </Suspense>
      ) : null}
      {!activeRemoteHost && whatsNew && (!onboardingSteps || onboardingSteps.length === 0) ? (
        <Suspense fallback={null}>
          <WhatsNewDialog
            version={whatsNew.version}
            notes={whatsNew.notes}
            onClose={() => {
              markWhatsNewSeen(whatsNew.version);
              setWhatsNew(null);
            }}
          />
        </Suspense>
      ) : null}
      {isAddProjectOpen ? (
        <AddProjectDialog
          projects={projects}
          initialHostId={addProjectHostId}
          onClose={handleCloseAddProject}
          onRegistered={handleRegisteredProject}
          onOpenSettings={handleOpenSshSettings}
        />
      ) : null}
      {isCreateOpen ? (
        <AddWorktreeDialog
          project={createTargetProject ?? activeProject}
          onClose={handleCloseCreateWorktree}
          onCreated={async (worktree) => {
            if (activeRemoteHostRef.current) return;
            const owner = createTargetProject ?? activeProject;
            if (owner.workspaceId !== activeProject.workspaceId) {
              handleSelectProject(owner);
              setPendingWorktree(worktree);
              return;
            }
            await refreshWorktrees();
            await ensureTabForWorktree(worktree).catch(reportRuntimeError);
          }}
        />
      ) : null}
      {deleteTarget ? (
        <WorktreeDeleteDialog
          workspaceId={deleteOwnerId ?? activeProject.workspaceId}
          project={deleteOwnerProject}
          worktree={deleteTarget}
          onClose={handleCloseDeleteTarget}
          onDeleted={() => {
            setWorktreeStatuses((current) => {
              const next = { ...current };
              delete next[deleteTarget.path];
              return next;
            });
            if (activeWorktree?.path === deleteTarget.path) {
              const remaining = state.worktrees.filter((w) => w.path !== deleteTarget.path);
              const ownerId = deleteOwnerId;
              const ownerProject = deleteOwnerProject;
              const fallback =
                (ownerProject ? remaining.find((w) => w.path === ownerProject.repoRoot) : undefined) ??
                (ownerProject
                  ? remaining.find((w) => resolveWorktreeOwnerId(w, projects, activeProject.workspaceId) === ownerId)
                  : undefined) ??
                remaining[0];
              if (fallback) {
                handleSelectWorktree(fallback);
              }
            }
            void refreshWorktrees();
          }}
        />
      ) : null}
      {pendingProjectRemove ? (
        <RemoveProjectDialog
          project={pendingProjectRemove}
          onClose={() => setPendingProjectRemove(null)}
          onConfirm={handleConfirmRemoveProject}
        />
      ) : null}
      {pendingTabClose ? (
        <ConfirmCloseTabDialog
          tabLabel={pendingTabClose.label}
          kind={pendingTabClose.kind}
          activeAgentCount={pendingTabClose.activeAgentCount}
          onCancel={handleCancelTabClose}
          onConfirm={handleConfirmTabClose}
        />
      ) : null}
      {diskManageProject ? (
        <WorktreeDiskDialog
          workspaceId={diskManageProject.workspaceId}
          projectName={diskManageProject.workspaceId}
          onClose={() => setDiskManageProject(null)}
        />
      ) : null}
      {historyProject ? (
        <AgentHistoryDialog
          workspaceId={historyProject.workspaceId}
          projectName={historyProject.workspaceId}
          cwd={historyProject.repoRoot || null}
          onClose={() => setHistoryProject(null)}
          onResume={async (entry: AgentHistoryEntry) => {
            const project = historyProject;
            try {
              const targetWorktree =
                (project.workspaceId === activeProjectRef.current.workspaceId ? activeWorktreeRef.current : null) ??
                stateRef.current.worktrees.find((wt) => resolveWorktreeOwnerId(wt, projectsRef.current, project.workspaceId) === project.workspaceId) ??
                stateRef.current.worktrees[0];
              await ensureTerminalEvents().catch(() => undefined);
              const backendSessionId = await spawnTerminal({
                workspaceId: project.workspaceId,
                worktree: targetWorktree ? worktreeIdentity(targetWorktree) : null,
                cwd: entry.cwd || targetWorktree?.path || project.repoRoot,
                startup: {
                  kind: "agentResume",
                  agentType: entry.provider,
                  // AgentProviderSession rejects an explicit null transcriptPath, so drop it
                  // rather than widening the wire type for one optional field.
                  providerSession: {
                    key: entry.providerSession.key,
                    id: entry.providerSession.id,
                    ...(typeof entry.providerSession.transcriptPath === "string"
                      ? { transcriptPath: entry.providerSession.transcriptPath }
                      : {}),
                  },
                },
              });
              const label = entry.provider.charAt(0).toUpperCase() + entry.provider.slice(1);
              if (targetWorktree) {
                await openTab(targetWorktree, label, backendSessionId);
              }
              setHistoryProject(null);
            } catch (error) {
              reportRuntimeError(error);
            }
          }}
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
  emptyWorkspaceIds: readonly string[] = [],
): Worktree[] {
  const groups = groupProjects(projects);
  const collapsed = loadCollapsedProjectIds(projects, activeProjectId);
  const visible: Worktree[] = [];

  for (const group of groups) {
    if (collapsed.has(group.groupId)) continue;
    if (group.memberProjects.every((member) => emptyWorkspaceIds.includes(member.workspaceId))) continue;
    const project = group.primaryProject;
    const owned = worktrees.filter(
      (worktree) => resolveWorktreeOwnerId(worktree, projects, activeProjectId) === project.workspaceId,
    );
    const cached = inactiveProjectWorktrees[project.workspaceId] ?? [];
    let rows =
      project.workspaceId === activeProjectId
        ? (owned.length > 0 ? owned : cached)
        : [...cached, ...owned];
    if (project.target?.kind === "ssh" && group.memberProjects.length === 1) {
      if (!rows.some((candidate) => candidate.path === project.repoRoot && candidate.workspaceId === project.workspaceId)) {
        const target = project.target;
        const hostLabel = typeof getCachedSshHosts === "function"
          ? getCachedSshHosts()?.find((h) => h.id === target.hostId)?.label ?? target.hostId
          : target.hostId;
        rows.push(projectRootWorktree(project, hostLabel));
      }
    } else {
      if (project.gitRoot === null && rows.length === 0) rows = [projectRootWorktree(project)];
      for (const member of group.memberProjects) {
        const target = member.target;
        if (member.workspaceId !== project.workspaceId && target?.kind !== "ssh") {
          const memberRows = member.workspaceId === activeProjectId
            ? worktrees.filter((row) => resolveWorktreeOwnerId(row, projects, activeProjectId) === member.workspaceId)
            : inactiveProjectWorktrees[member.workspaceId] ?? [];
          rows.push(...memberRows.map((row) => ({ ...row, workspaceId: row.workspaceId ?? member.workspaceId })));
        }
        if (target?.kind === "ssh") {
          if (!rows.some((candidate) => candidate.path === member.repoRoot && candidate.workspaceId === member.workspaceId)) {
            const hostLabel = typeof getCachedSshHosts === "function"
              ? getCachedSshHosts()?.find((h) => h.id === target.hostId)?.label ?? target.hostId
              : target.hostId;
            rows.push(projectRootWorktree(member, hostLabel));
          }
        }
      }
    }
    for (const row of rows) {
      if (visible.some((candidate) => candidate.path === row.path && candidate.workspaceId === row.workspaceId)) continue;
      visible.push(row);
    }
  }

  return visible;
}

/**
 * Shortcut navigation (Cmd+1..9) targets only worktrees whose layouts currently
 * own tabs; tabless worktrees stay listed in the sidebar but off the shortcut
 * ladder so a digit never lands on an empty workspace.
 */
function listShortcutWorktrees(
  projects: RegisteredProject[],
  worktrees: Worktree[],
  activeProjectId: string,
  inactiveProjectWorktrees: Record<string, Worktree[]>,
  liveState: WorkspaceState,
  snapshots: ReadonlyArray<readonly [string, WorkspaceState]>,
): Worktree[] {
  const visible = listVisibleWorktrees(
    projects, worktrees, activeProjectId, inactiveProjectWorktrees,
    emptySidebarWorkspaceIds(projects, activeProjectId, liveState, snapshots),
  );
  const states = new Map(snapshots);
  states.set(liveState.workspaceId ?? activeProjectId, liveState);
  return visible.filter((row) =>
    worktreeHasOpenTabs(
      states.get(resolveWorktreeOwnerId(row, projects, activeProjectId) ?? (liveState.workspaceId ?? activeProjectId)),
      row.path,
    ),
  );
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

export function loadProjects(): RegisteredProject[] {
  try {
    const raw = getMigratedItem(PROJECTS_STORAGE_KEY);
    if (!raw) return [DEFAULT_PROJECT];
    const parsed = JSON.parse(raw) as RegisteredProject[];
    if (!Array.isArray(parsed)) return [DEFAULT_PROJECT];
    // An explicitly stored empty array means the user removed every project on
    // purpose; boot into the genuine empty state instead of resurrecting the
    // default startup project.
    if (parsed.length === 0) return [];
    const valid = parsed
      .filter(
        (project) =>
          project &&
          typeof project.workspaceId === "string" &&
          typeof project.repoRoot === "string" &&
          hasValidProjectTarget(project) &&
          (project.repoRoot !== "/" || project.target?.kind === "ssh" || project.target?.kind === "pairedDaemon") &&
          project.repoRoot !== "\\",
      )
      .map((project): RegisteredProject => {
        const metadata = {
        workspaceId: project.workspaceId,
        repoRoot: project.repoRoot,
        hostLabel: typeof project.hostLabel === "string" ? project.hostLabel : undefined,
        gitCommonDir: typeof project.gitCommonDir === "string" ? project.gitCommonDir : undefined,
        gitRemote:
          typeof project.gitRemote === "string"
            ? project.gitRemote
            : project.gitRemote === null
              ? null
              : undefined,
        // Entries persisted before gitRoot existed can only be git projects
        // (the old backend rejected non-git folders), and their repoRoot was
        // already the canonical git root. Only an explicit null means non-git.
        gitRoot:
          typeof project.gitRoot === "string"
            ? project.gitRoot
            : project.gitRoot === null
              ? null
              : project.repoRoot,
        };
        if (project.target?.kind === "pairedDaemon") {
          return { ...metadata, target: project.target, remoteWorkspaceId: project.remoteWorkspaceId as string };
        }
        return { ...metadata, target: project.target };
      });
    if (valid.length !== parsed.length) console.error("Ignored invalid stored project records; invalid targets cannot be opened locally.");
    return valid;
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
