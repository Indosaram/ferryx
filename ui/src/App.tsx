import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { STARTUP_TIMEOUT_MS } from "./lib/startupTimeout";
import { withTimeout } from "./lib/withTimeout";
import { useDesktopContinuity } from "./lib/useDesktopContinuity";
import { deriveContinuitySelection } from "./lib/continuitySelection";

import { CommandPalette } from "./components/CommandPalette";
import { DesktopActivityBar } from "./components/activity/DesktopActivityBar";
import { BrowserAgentApprovalCard } from "./components/BrowserAgentApprovalCard";
import { EmptyWorkspaceView } from "./components/EmptyWorkspaceView";
import { SshWorkspaceStatus } from "./components/SshWorkspaceStatus";
import { AddProjectDialog, AddWorktreeDialog, RemoveProjectDialog, type WorktreeAgentLaunch } from "./components/ProjectDialogs";
import { ExternalWorktreeImportDialog } from "./components/ExternalWorktreeImportDialog";
import { useExternalWorktreePrompt } from "./state/useExternalWorktreePrompt";
import { Sidebar } from "./components/Sidebar";
import { ShortcutHints } from "./components/ShortcutHints";
import { TerminalSplitView } from "./components/TerminalSplitView";
import { LocalChatPane } from "./components/LocalChatPane";
import { RestartPaneDialog } from "./components/RestartPaneDialog";
import { agentDisplayNameForType } from "./lib/agentTitle";
import { TERMINAL_COPY_MODE_EVENT, type TerminalCopyModeEventDetail } from "./components/TerminalCopyMode";
import { RemoteHostConnection } from "./remote/RemoteApp";
import { RemoteBrowserSharingIndicator } from "./components/RemoteBrowserSharingIndicator";
import { remoteHostStore, selectActiveHost } from "./state/remoteHostStore";
import { pairedHostInventory } from "./lib/pairedHostInventory";
import { WorktreeDeleteDialog } from "./components/WorktreeDeleteDialog";
import { isSameWorktreePath } from "./components/sidebar/sidebarSupervisor";
import { WorktreeDiskDialog } from "./components/WorktreeDiskDialog";
import { AgentHistoryDialog } from "./components/AgentHistoryDialog";
import { agentHistoryProviderLabel, type AgentHistoryEntry } from "./lib/agentHistory";
import { ConfirmCloseTabDialog } from "./components/ConfirmCloseTabDialog";
import { TerminalLinkActions } from "./components/TerminalLinkActions";
import { TerminalFileLinkActions } from "./components/TerminalFileLinkActions";
import { Toaster, toast } from "./components/ui/sonner";
import { cn } from "./lib/cn";
import {
  loadActivityMode,
  saveActivityMode,
  type ActivityMode,
} from "./lib/sidebarPersistence";
import { Button } from "./components/ui/button";
import { IconButton } from "./components/ui/IconButton";
import { copyTextToClipboard } from "./lib/clipboard";
import { cycleAppearanceTheme, openAppearanceSurface } from "./lib/appearanceCommands";
import { useAppearanceSettings, useApplyAppearanceSettings } from "./lib/appearanceSettings";
import { workspaceName } from "./lib/branchFilter";
import { collectDagWatchRoots, isLocalDagProject, remoteProjectsWatchKey } from "./lib/dagWatchRoots";
import { loadBrowserSettings, newBrowserTabUrl } from "./lib/browserSettings";
import { BROWSER_SHORTCUT_EVENT, browserTabIdForBrowserId, closeBrowser, navigateBrowser, onBrowserLinkClicked, onBrowserOpenRequested, onBrowserPopupCloseRequested, onBrowserSessionCreated, onBrowserShortcutRequested, onBrowserTabSwitch, openExternalUrl, popupOpenerLink, reportBrowserAdoption, browserTabSelectIndex, browserWorkspaceSelectIndex, type BrowserReloadOptions, type BrowserShortcutAction, type BrowserShortcutDomEvent } from "./lib/browserTauri";
import { startInboundUrlDelivery } from "./lib/inboundUrls";
import { printBrowser, stepBrowserZoom } from "./lib/browserSystem";
import { browserTabNavigation } from "./lib/browserTabNavigation";
import { useBrowserLibrarySync } from "./lib/browserLibrarySync";
import { listen } from "@tauri-apps/api/event";
import { CLI_OPEN_FILE_EVENT, parseCliOpenFilePayload } from "./lib/cliOpenFile";
import { registerBuiltInBrowserLinkOpener, registerWorktreePathOpener } from "./lib/linkRouting";
import {
  adoptCliBrowserSession,
  closeBrowserSessionFromSplit,
  detachBrowserFromWorktreeSplit,
  focusAdoptedBrowser,
  handleWorktreeDeletedForBrowsers,
  openBrowserInWorktreeSplit,
  resolveBrowserTabSwitch,
} from "./lib/worktreeBrowserBridge";
import { browserStore } from "./state/browserStore";
import {
  BROWSER_MODE_SHORTCUT_EVENT,
  MahoBrowserMiniPane,
  useMahoBrowserRuntime,
  useMahoBrowserShell,
  type BrowserModeShortcutDetail,
} from "./components/browser/maho/MahoBrowserShell";
import { useBrowserAgentActivity } from "./components/browser/maho/MahoAgentControl";
import { useGeneralSettings } from "./lib/generalSettings";
import { NotificationCoordinator, isWindowForegroundFocused } from "./lib/notificationCoordinator";
import { startSupervisorFeed } from "./lib/supervisor";
import { startDaemonAttentionFeed } from "./lib/attentionFeed";
import { usePanelDecisionCount } from "./lib/supervisorPanel/usePanelDecisionCount";
import { getSetupStatus, setupFinished, setupProjectsFrom } from "./lib/supervisorSetup";
import { DesktopSupervisorSetupBanner } from "./components/supervisor/DesktopSupervisorSetupBanner";
import { getNativeWindowFocused, startNativeWindowFocusTracking } from "./lib/nativeWindowFocus";
import { PRIMARY_WINDOW_LABEL, currentWindowLabel, isPrimaryWindow, windowScopedStorageKey } from "./lib/windowIdentity";
import {
  forwardNotificationTarget,
  routeNotificationTarget,
  subscribeForwardedNotificationTargets,
} from "./lib/notificationForwarding";
import {
  appBadgeTotal,
  forgetWindow,
  publishWindowBadgeCount,
  publishWindowSessions,
  shouldThisWindowOwnBadge,
  subscribeWindowStateChanges,
} from "./lib/notificationLeader";
import { planOrphanAdoption } from "./state/orphanAdoption";
import { movedEverySession, planTabMove } from "./state/windowMove";
import { serializeWorkspaceState, sessionPersistenceKey } from "./lib/sessionPersistence";
import { setLocalSplitPersistence } from "./lib/localSplitLifecycle";
import { isMacShortcutPlatform, resolvedShortcuts, routeBrowserModeShortcut, useShortcuts } from "./lib/shortcuts";
import { useShortcutOverrides, useShortcutPreset } from "./lib/shortcutOverrides";
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
import { SetupProgressPanel } from "./components/onboarding/SetupProgressPanel";
import { buildSetupProgress } from "./lib/setupProgress";
import { DaemonConnectionBanner } from "./components/DaemonConnectionBanner";
import { RemoteDesktopConsentDialog } from "./components/settings/remote/RemoteDesktopHostSection";
import { GlobalRemoteDesktopCanvas, liveRemoteDesktopSessionCount } from "./components/remoteDesktop/GlobalRemoteDesktopCanvas";
import { RemoteDesktopActivitySidebar } from "./components/remoteDesktop/RemoteDesktopActivitySidebar";
import { remoteDesktopStore } from "./state/remoteDesktopStore";
import { forwardCapturedRemoteDesktopShortcut } from "./lib/remoteDesktop";
import { InsecureTransportBanner } from "./components/InsecureTransportBanner";
import type { SectionId } from "./components/settings/types";
import type { MachineProjectTarget, RemoteContext } from "./lib/machineNavigation";
import {
  AGENTS_SETTINGS_CHANGED_EVENT,
  detectionTargets,
  getLaunchableAgents,
  loadAgentSettings,
  mergeDetections,
  splitAgentArgs,
  type AgentSettings,
} from "./lib/agentsSettings";
import { chatSupportedAgents, loadLastChatAgentId, pickChatAgent, saveLastChatAgentId } from "./lib/paneChatPreferences";
import {
  ACTIVE_PROJECT_STORAGE_KEY,
  getMigratedItem,
  PROJECTS_STORAGE_KEY,
  SETUP_PROGRESS_HINT_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} from "./lib/storageKeys";
import {
  DEFAULT_WORKSPACE_ID,
  detectAgents,
  getCliLauncherStatus,
  getAccountEnrollmentStatus,
  getInitialProject,
  listRemoteDevices,
  getSystemPermissionsStatus,
  isTauriRuntime,
  listWorktrees,
  loadSession,
  onSessionCorrupted,
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
  onNewWindowMenu,
  onNewTerminalTabMenu,
  onRemoteSelectionRequested,
  publishFocusedTerminal,
  onTerminalRemoteStatus,
  registerProject,
  syncDesktopLocalProjects,
  unregisterProject,
  saveSession,
  setBadgeCount,
  spawnTerminalDetailed,
  closeTerminal,
  retryPairedSession,
  retryTerminalRemoteSession,
  getTerminalRemoteStatus,
  listTerminalSessions,
  discardWindowSession,
  listAppWindows,
  moveSessionsToWindow,
  onWindowSessionsOrphaned,
  onWindowSessionsAdopt,
  openNewWindow,
  takePendingWindowAdoptions,
  type WindowSessionsAdoptPayload,
  toIpcError,
  writeTerminal,
  bootTrace,
  browserRemoteReclaim,
  discoverDagWatchRoots,
  type AgentDetection,
  type FocusedTerminalPayload,
  type RegisteredProject,
  type RemoteSelectionRequestedPayload,
  type SpawnTerminalRequest,
} from "./lib/tauri";
import * as tauriApi from "./lib/tauri";
import { dagRemoteWatchTargets, useDagWatchLifecycle } from "./lib/useDagWatchLifecycle";
import { getCachedSshHosts } from "./lib/sshHosts";
import { useTailscaleTargetsByHost } from "./lib/tailscaleDiscovery";
import { createPairedDaemonProjectAdapter } from "./lib/pairedDaemonProject";
import { reconnectAgentSession } from "./lib/agentReconnect";
import { isPairedWorkspaceId, isRemoteWorkspaceId, registerRemoteProject, toRegisteredProject } from "./lib/remoteProject";
import { previewRequestForSource } from "./lib/filePreviewTabRegistry";
import { healMissingSshRegistrations } from "./lib/sshRegistrationHeal";
import { hasValidProjectTarget, projectRootWorktree, sshProjectWorktrees } from "./lib/projectIdentity";
import { scheduleAgentAutoResume, startAgentAutoResumeWatcher } from "./lib/agentAutoResume";
import { isStandbyBackendSessionId, REMOTE_SESSION_GONE_REASON, setSessionRebindHandler, setSessionSleeping } from "./lib/sessionLifecycle";
import { flashPane } from "./lib/paneFlash";
import { findSessionByBackendId, localBackendSessionId } from "./components/supervisorPanel/findSession";
import { extractNormalizedProviderSession, getAgentReconnectAffordance, getContinuationAffordance, getInPlaceRestartStartup, isExitedContinuationSource } from "./lib/agentResumeAffordance";
import { persistActiveProjectId, persistProjects, persistSidebarOpen } from "./lib/appShellPersistence";
import { createAppReconnectDependencies } from "./lib/appReconnectDependencies";
import { startSshRecovery } from "./lib/sshRecovery";
import { startParkedRemoteRecovery } from "./lib/parkedRemoteRecovery";
import { reconnectLocalSession, replaceExitedShellSession, restartPaneSession } from "./lib/shellReplacement";
import { initialGridFields, measureInitialGeometry } from "./lib/initialPtyGeometry";
import { enqueueStrictPersistence } from "./lib/persistenceQueue";
import { workspaceReducer, requestLocalRecoveryAdmission, localRecoveryBlockReason, type LocalRecoveryAdmissionResult, type WorkspaceAction } from "./state/workspaceStore";
import type { NotificationTarget, PersistedWorkspaceSession, SystemPermissionsStatus } from "./lib/types";
import { staticTabLabel } from "./lib/tabTitle";
import { subscribeNotificationActivations } from "./lib/notificationActivation";
import { ensureTerminalEvents } from "./lib/terminalEvents";
import { useTerminalSettings } from "./lib/terminalSettings";
import { resolveWorktreeOwnerId } from "./lib/worktreeOwnership";
import { createClosedTabStack } from "./lib/closedTabStack";
import { assessClose, reassessConfirmedClose, type CloseTarget } from "./lib/closeGuard";
import { switchDebug } from "./lib/switchDebug";
import { useInactiveProjectWorktrees } from "./state/inactiveProjectWorktrees";
import { useDesktopSidebarPublisher } from "./lib/desktopSidebarSync";
import type { LaneLifecycleState, LanePullRequestView, SidebarLaneLifecycle } from "./lib/sidebarPersistence";
import { worktreeLifecycleIdentity } from "./lib/sidebarPersistence";
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
import { collectLeafIds, type PaneDirection, type PaneView } from "./state/paneTree";
import { usePendingWorktree } from "./state/pendingWorktree";
import { findDirectionalLeafId, type PaneFocusDirection } from "./state/paneGeometry";
import { useBrowserSessionHydration } from "./state/browserSessionHydration";
import { preloadWorkspaceSnapshots, useWorkspaceRestore } from "./state/workspaceRestore";
import { clearHmrWorkspaceState, getHmrWorkspaceState } from "./state/hmrWorkspaceState";
import { clearWorkspaceSnapshot, getWorkspaceSnapshot, listWorkspaceSnapshots, subscribeWorkspaceSnapshots } from "./state/workspaceSnapshotCache";
import { createOwnedWorkspaceAccess } from "./state/workspaceOwnerAccess";
import { emptySidebarWorkspaceIds } from "./state/sidebarWorkspaceState";
import { describeLadderRow, listShortcutWorktrees } from "./state/shortcutLadder";
import { useWorkspaceRuntime } from "./state/workspaceRuntime";
import { layoutSyncHostTransport, useLayoutSync, usePairedLayoutAdapter } from "./state/useLayoutSync";
import { publishPersistedLayouts } from "./state/layoutSync";
import { listPairedProjectWorktrees } from "./state/pairedProjectWorktrees";
import { getTabById, getTabLayoutById, getTabSessionIds, hasNavigableSession, selectGlobalUnreadBadgeCount, selectNotificationWorkspaceLabel, selectSessionActivity, selectWorktreeActivitySummaries, setLocalRecoveryPersistence, wakeLocalSessionRecovery, useWorkspaceStore, LANE_LIFECYCLE_UNDO_MS, type LaneLifecycleAction, type WorkspaceState } from "./state/workspaceStore";
import { activityStateOfGroup, groupOf, type AttentionBySessionId } from "./lib/attentionGroup";

export { ACTIVE_PROJECT_STORAGE_KEY, PROJECTS_STORAGE_KEY, SIDEBAR_OPEN_STORAGE_KEY };
const DEFAULT_PROJECT: RegisteredProject = { workspaceId: DEFAULT_WORKSPACE_ID, repoRoot: ".", gitRoot: null };
const loadSettingsDialog = () =>
  import("./components/SettingsDialog").then((m) => ({ default: m.SettingsDialog }));
const SettingsDialog = lazy(loadSettingsDialog);
const SystemResourcesDialog = lazy(() =>
  import("./components/SystemResourcesDialog").then((m) => ({ default: m.SystemResourcesDialog }))
);
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

export function createDesktopLocalProjectSync() {
  let pending: RegisteredProject[] | null = null;
  let running: Promise<void> | null = null;
  return (projects: RegisteredProject[]): Promise<void> => {
    pending = projects;
    if (running) return running;
    running = (async () => {
      try {
        while (pending !== null) {
          const snapshot = pending;
          pending = null;
          try { await syncDesktopLocalProjects(snapshot.filter((project) =>
            project.repoRoot !== "" && project.repoRoot !== "." &&
            project.workspaceId !== DEFAULT_WORKSPACE_ID &&
            !isRemoteWorkspaceId(project.workspaceId) && !isPairedWorkspaceId(project.workspaceId) &&
            (!project.target || project.target.kind === "local"),
          ).map(({ workspaceId, repoRoot }) => ({ workspaceId, repoRoot }))); }
          catch (error) { if (pending === null) throw error; }
        }
      } finally {
        running = null;
      }
    })();
    return running;
  };
}

const sendDesktopLocalProjects = createDesktopLocalProjectSync();

/** Backend id of a frontend session held in this window's store (mounted or parked workspace). */
function findBackendSessionId(mounted: WorkspaceState, sessionId: string, workspaceId?: string): string | null {
  const candidates = workspaceId && workspaceId !== mounted.workspaceId
    ? [getWorkspaceSnapshot(workspaceId), mounted]
    : [mounted, ...listWorkspaceSnapshots().map(([, snapshot]) => snapshot)];
  for (const candidate of candidates) {
    const backendSessionId = candidate?.sessions[sessionId]?.backendSessionId;
    if (backendSessionId) return backendSessionId;
  }
  return null;
}

function collectBackendSessionIds(states: readonly WorkspaceState[]): Set<string> {
  const ids = new Set<string>();
  for (const candidate of states) {
    for (const session of Object.values(candidate.sessions)) {
      if (session.backendSessionId) ids.add(session.backendSessionId);
    }
  }
  return ids;
}

/**
 * A secondary window ("ws-<id>") lists the same projects in its sidebar but never inherits the
 * primary window's tabs: its active project comes from its own session file (window-scoped
 * load) or its own namespaced selection, and with neither it opens on the workspace picker.
 * Local registrations are shared with the primary window, which already performed them.
 */
export function secondaryWindowBootstrap(
  stored: ProjectBootstrap,
  recovered: ProjectBootstrap | null,
): ProjectBootstrap {
  const projects = [...stored.projects];
  for (const project of recovered?.projects ?? []) {
    if (!projects.some((candidate) => candidate.workspaceId === project.workspaceId)) projects.push(project);
  }
  // A brand-new window has no namespaced selection of its own yet, so it opens on the same
  // project the primary window is working in instead of the workspace picker. Once the user
  // picks another project here, the namespaced key wins and the windows diverge on purpose.
  const candidates = [recovered?.activeProjectId, loadWindowActiveProjectId(), loadActiveProjectId()];
  const activeProjectId = candidates.find((id) => id && projects.some((project) => project.workspaceId === id)) ?? "";
  return { projects, activeProjectId };
}

/** A paired host key is `<relay origin>/host/<machineId>`; a descriptor's target names the bare machine id. */
function pairedHostMachineId(hostId: string): string {
  const marker = hostId.lastIndexOf("/host/");
  return marker < 0 ? hostId : decodeURIComponent(hostId.slice(marker + "/host/".length));
}

/**
 * Platform of the host the active workspace's tabs actually run on, for `TabBar`'s shell presets.
 *
 * A paired host reports its own platform in the capability handshake, and a Tailscale SSH peer
 * reports one when discovery probes it. Everything else (a local project, or an SSH host nobody
 * has probed) is unknown and answers null, which leaves `TabBar` on this desktop's platform.
 */
function useTargetHostPlatform(project: RegisteredProject): string | null {
  const target = project.target;
  const pairedHostId = target?.kind === "pairedDaemon" ? target.hostId : null;
  const sshHostId = target?.kind === "ssh" ? target.hostId : null;
  // The pairing generation pins which host incarnation may answer; a re-pair re-asks.
  const pairedGeneration = useSyncExternalStore(
    remoteHostStore.subscribe,
    () => (pairedHostId ? remoteHostStore.getState().hosts[pairedHostId]?.generation : undefined),
  );
  const tailscaleTargets = useTailscaleTargetsByHost();
  const [pairedPlatform, setPairedPlatform] = useState<string | null>(null);

  useEffect(() => {
    // A different host, or a re-pair (new generation), never inherits the previous answer.
    setPairedPlatform(null);
    if (!pairedHostId || !pairedGeneration) return;
    // Its own adapter: negotiating on the layout-sync adapter would clear that adapter's
    // admission, so a failed probe here could strand layout sync on UNSUPPORTED_CAPABILITY.
    const adapter = createPairedDaemonProjectAdapter({ hostId: pairedHostId, generation: pairedGeneration });
    let cancelled = false;
    void adapter.capabilities().then(
      (capabilities) => {
        if (!cancelled) setPairedPlatform(capabilities.platform);
      },
      () => {
        // An unreachable host leaves the platform unknown rather than guessing one.
        if (!cancelled) setPairedPlatform(null);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [pairedHostId, pairedGeneration]);

  return useMemo(() => {
    if (pairedHostId) return pairedPlatform;
    if (!sshHostId) return null;
    // Discovery reports "posix" or "windows"; "posix" is passed through as probed rather than
    // narrowed to one OS, because the only reader keys off "windows".
    return tailscaleTargets.get(sshHostId)?.environment?.platform ?? null;
  }, [pairedHostId, pairedPlatform, sshHostId, tailscaleTargets]);
}

/**
 * The server-owned sidebar order and per-row lifecycle state, shaped for `buildSidebarModel`.
 *
 * It is a pure derivation of the store (`laneOrder` / `laneLifecycle` / `lanePinnedTabs`), so the
 * sidebar and the publisher cannot disagree about a row's state, and nothing here re-reads the
 * server: `workspaceStore` owns that read and its 5 s undo window.
 */
function useLaneLifecycleModel(
  order: Readonly<Record<string, readonly string[]>> | undefined,
  lifecycle: Readonly<Record<string, LaneLifecycleState>> | undefined,
  pinnedTabs: Readonly<Record<string, readonly string[]>> | undefined,
  pullRequests: Readonly<Record<string, LanePullRequestView | null>> | undefined,
): {
  laneOrder: Record<string, string[]>;
  laneLifecycles: Record<string, SidebarLaneLifecycle>;
} {
  return useMemo(() => {
    const rows: Record<string, SidebarLaneLifecycle> = {};
    for (const [key, state] of Object.entries(lifecycle ?? {})) {
      const pins = pinnedTabs?.[key];
      rows[key] = { state, ...(pins && pins.length > 0 ? { pinnedTabs: pins } : {}), pullRequest: pullRequests?.[key] ?? null };
    }
    const sections: Record<string, string[]> = {};
    for (const [workspaceId, keys] of Object.entries(order ?? {})) {
      if (keys.length > 0) sections[workspaceId] = [...keys];
    }
    return { laneOrder: sections, laneLifecycles: rows };
  }, [order, lifecycle, pinnedTabs, pullRequests]);
}

/**
 * Names the row a lifecycle action applies to and shows the undo affordance.
 *
 * The store holds the 5 s timer and auto-commits on expiry (`beginLaneLifecycle`); nothing here
 * schedules anything. The toast's Undo is the ONLY caller of `cancelLaneLifecycle`, which clears
 * that timer and removes the pending entry, so a cancel inside the window means the transport is
 * never called and no row can have changed.
 */
function useLaneActionHandler(
  projectWorkspaceId: string,
  begin: (request: { worktreeKey: string; workspaceId: string; path: string; action: LaneLifecycleAction; until?: string | null }) => void,
  cancel: (worktreeKey: string) => void,
): (worktree: Worktree, action: LaneLifecycleAction) => void {
  return useCallback((worktree: Worktree, action: LaneLifecycleAction) => {
    if (!worktree.path) return;
    const identity = worktreeIdentity(worktree);
    const { workspaceId, worktreeKey } = worktreeLifecycleIdentity(worktree, projectWorkspaceId);
    begin({ worktreeKey, workspaceId, path: worktree.path, action });
    // The row's own name, without the sidebar's label helper: App already binds a local
    // `worktreeLabel` (line ~783) and importing that name here would shadow it.
    const name = identity?.slug ?? worktree.branch?.replace(/^refs\/heads\//, "") ?? worktree.path;
    toast(`${LANE_ACTION_LABELS[action]} ${name}`, {
      // Exactly the window the store holds, so the button is gone when the write lands.
      duration: LANE_LIFECYCLE_UNDO_MS,
      action: { label: "Undo", onClick: () => cancel(worktreeKey) },
    });
  }, [begin, cancel, projectWorkspaceId]);
}

const LANE_ACTION_LABELS: Record<LaneLifecycleAction, string> = {
  pin: "Pinned",
  settle: "Settled",
  snooze: "Snoozed",
  archive: "Archived",
  activate: "Reactivated",
};

export function App() {
  useApplyAppearanceSettings();
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const [bootstrap, setBootstrap] = useState<ProjectBootstrap | null>(() =>
    isNativeRuntime ? null : loadProjectBootstrap(),
  );

  // Once-per-app startup effects run only in the primary window; a secondary window ("ws-<id>")
  // must not poll for updates or show the Store migration notice a second time.
  const [isPrimary] = useState(() => isPrimaryWindow());

  useEffect(() => {
    if (isNativeRuntime && isPrimary) return startUpdatePolling();
  }, [isNativeRuntime, isPrimary]);

  useEffect(() => {
    if (isNativeRuntime && isPrimary) void maybeShowWindowsStoreMigrationNotice();
  }, [isNativeRuntime, isPrimary]);

  useEffect(() => {
    if (!isNativeRuntime) {
      return;
    }
    let cancelled = false;
    // Register before the first loadSession(): the backend quarantines a corrupt file and emits
    // `session_corrupted` while that command runs, so a listener mounted later (inside
    // WorkspaceApp, which only renders after bootstrap) would miss the one event that says where
    // the person's layout went. Registration is asynchronous, so the load waits for it: starting
    // the load first would race the event it is meant to report.
    const corruption = onSessionCorrupted((backupPath) => {
      toast.error("Session layout could not be read", {
        id: "session-corrupted",
        description: `The unreadable file was kept at ${backupPath}, so a starting-from-empty window can be recovered from it.`,
        duration: 15000,
      });
    });
    const corruptionReady = Promise.resolve(corruption).then(
      (dispose) => dispose,
      (error: unknown) => {
        // A listener that failed to register costs the backup path notice, never the session load.
        console.warn("Session corruption listener unavailable:", error);
        return () => undefined;
      },
    );
    const loadSessionAfterSubscription = async () => {
      // A listener that never settles must not stall startup: loading the session is the priority
      // and the corruption notice is only a notification, so it is given a bounded wait.
      await Promise.race([
        corruptionReady,
        new Promise<void>((resolve) => setTimeout(resolve, 2000)),
      ]);
      return loadSession().catch(() => null);
    };
    void bootTrace("initial.start");
    void withTimeout(getInitialProject(), STARTUP_TIMEOUT_MS, "cmd_project_initial")
      .then(async (startup) => {
        void bootTrace("initial.ok");
        const storedBootstrap = loadProjectBootstrap();
        // loadSession is window-scoped by the backend: a secondary window reads only its own file.
        const savedSession = await loadSessionAfterSubscription();
        const recovered = recoverProjectBootstrap(savedSession);
        const prepared = isPrimary
          ? canonicalizeProjectBootstrap(
              mergeRecoveredProjectBootstrap(storedBootstrap, recovered, startup),
              startup,
            )
          : secondaryWindowBootstrap(storedBootstrap, recovered);
        if (isPrimary && recovered && prepared !== storedBootstrap) {
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
          const savedSession = await loadSessionAfterSubscription();
          const recovered = recoverProjectBootstrap(savedSession);
          if (!isPrimary) {
            const prepared = secondaryWindowBootstrap(storedBootstrap, recovered);
            await preloadWorkspaceSnapshots(
              prepared.projects.map((project) => project.workspaceId),
              async () => savedSession,
            ).catch((preloadError) => {
              console.warn("Workspace session preload fallback skipped:", preloadError);
            });
            if (!cancelled) setBootstrap(prepared);
            return;
          }
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
      void corruptionReady.then((dispose) => dispose());
    };
  }, [isNativeRuntime, isPrimary]);

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
  attentionBySessionId: AttentionBySessionId = {},
): FocusedTerminalPayload | null {
  const attentionInventory = [[workspaceId, state] as const, ...snapshots.filter(([id]) => id !== workspaceId)]
    .flatMap(([id, snapshot]) => {
      const summaries = selectWorktreeActivitySummaries(snapshot, attentionBySessionId);
      return snapshot.worktrees.map((worktree) => {
        const activity = summaries[worktree.path];
        const attention: "waiting" | "done" | "working" | null = activity?.hasWaiting ? "waiting"
          : activity?.hasDone ? "done" : activity?.hasWorking ? "working" : null;
        return { workspaceId: id, worktreeSlug: worktreeIdentity(worktree)?.slug ?? null,
          worktreeLabel: worktree.branch?.replace(/^refs\/heads\//, "") ?? null, state: attention };
      });
    });
  // Every waiting or unseen-done session of every project, so the phone Inbox spans the host.
  const sessionAttention = [[workspaceId, state] as const, ...snapshots.filter(([id]) => id !== workspaceId)]
    .flatMap(([id, snapshot]) =>
      [snapshot.layout, ...Object.values(snapshot.worktreeLayouts ?? {})].flatMap((ownerLayout) =>
        ownerLayout.tabs.filter(isTerminalTab).flatMap((tab) => [...getTabSessionIds(snapshot, tab.id)].flatMap((sessionId) => {
          const activity = selectSessionActivity(snapshot, sessionId, attentionBySessionId);
          const drawn = activity ? activityStateOfGroup(groupOf(activity)) : null;
          const paneSession = snapshot.sessions[sessionId];
          const backendSessionId = publishableBackendSessionId(paneSession?.backendSessionId);
          if (!backendSessionId || !activity || (drawn !== "waiting" && drawn !== "done")) return [];
          const panePath = paneSession.worktreePath ?? paneSession.cwd;
          const paneWorktree = panePath ? snapshot.worktrees.find((wt) => wt.path === panePath) : undefined;
          return [{
            backendSessionId,
            workspaceId: id,
            worktreeSlug: (paneWorktree ? worktreeIdentity(paneWorktree) : null)?.slug ?? null,
            state: drawn,
            ...(activity.agentType ? { agentType: activity.agentType } : {}),
          }];
        })),
      ),
    )
    .filter((entry, index, all) => all.findIndex((other) => other.backendSessionId === entry.backendSessionId) === index);
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
      const panePath = paneSession?.worktreePath ?? paneSession?.cwd;
      // Each pane carries its own worktree so the remote can select across worktrees instead of
      // guessing from whichever context is currently mirrored.
      const paneWorktree = panePath ? state.worktrees.find((wt) => wt.path === panePath) : undefined;
      const paneSlug = (paneWorktree ? worktreeIdentity(paneWorktree) : null)?.slug ?? null;
      const paneLabel = paneWorktree?.branch
        ? paneWorktree.branch.replace(/^refs\/heads\//, "")
        : null;

      const activity = sessionId ? selectSessionActivity(state, sessionId, attentionBySessionId) : undefined;
      // The phone draws what the desktop draws: the pane's group, as the state that group shows.
      const drawnState = activity ? activityStateOfGroup(groupOf(activity)) : null;
      const baseLabel = remoteTabLabel(staticTabLabel(tab));
      const exited = paneSession ? isExitedContinuationSource(paneSession) : false;
      const providerSession = paneSession && exited ? extractNormalizedProviderSession(paneSession) : null;
      const agentType = activity?.agentType ?? (exited ? paneSession?.agentType ?? undefined : undefined);
      const publishedSessionId = publishedPaneSessionId(paneSession, exited);
      return {
        id: remotePaneId(tab.id, multiPane ? leafId : null),
        label: multiPane ? `${baseLabel} (${index + 1})` : baseLabel,
        ...(drawnState ? { activityState: drawnState } : {}),
        ...(agentType ? { agentType } : {}),
        ...(paneSlug ? { worktreeSlug: paneSlug } : {}),
        ...(paneLabel ? { worktreeLabel: paneLabel } : {}),
        ...(publishedSessionId ? { sessionId: publishedSessionId } : {}),
        ...(exited ? { running: false as const } : {}),
        ...(providerSession ? { providerSession } : {}),
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
    ...(sessionAttention.length > 0 ? { sessionAttention } : {}),
    worktreeSlug: worktreeSlug ?? null,
    worktreeLabel: worktreeLabel ?? null,
    backendSessionId: publishableBackendSessionId(session?.backendSessionId) ?? null,
    activeTabId: focusedEntryId,
    tabId: focusedEntryId,
    tabs: terminalTabs,
    terminalTabs,
  };
}

/** No daemon judgment yet (or a store double without one): every pane uses groupOf's state mapping. */
const NO_ATTENTION: AttentionBySessionId = {};

function publishableBackendSessionId(id: string | null | undefined): string | undefined {
  if (!id || isStandbyBackendSessionId(id)) return undefined;
  return id;
}

/** Live backend id, or the retained real id of an exited agent pane. Never a standby id. */
function publishedPaneSessionId(session: TerminalSession | undefined, exited: boolean): string | undefined {
  if (!session) return undefined;
  const live = publishableBackendSessionId(session.backendSessionId);
  if (live) return live;
  if (!exited || !(session.agentType || session.providerSession || session.agentSessionId)) return undefined;
  return publishableBackendSessionId(session.lastBackendSessionId);
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
  const paletteAppearance = useAppearanceSettings();
  const [isNativeRuntime] = useState(() => isTauriRuntime());
  const [isPrimary] = useState(() => isPrimaryWindow());
  // Bumps when another Ferryx window's focus or badge contribution changes (multi-window).
  const [windowStateVersion, bumpWindowStateVersion] = useState(0);
  useEffect(() => subscribeWindowStateChanges(() => bumpWindowStateVersion((version) => version + 1)), []);
  /** The focused Ferryx window, else the primary window: owns app-global badge and remote selection. */
  const isAppStateLeader = useCallback(
    () => shouldThisWindowOwnBadge(isPrimary, getNativeWindowFocused() ?? isWindowForegroundFocused()),
    [isPrimary],
  );
  const activeRemoteHost = useSyncExternalStore(
    remoteHostStore.subscribe,
    () => selectActiveHost(remoteHostStore.getState()),
  );
  const activeRemoteHostRef = useRef(activeRemoteHost);
  activeRemoteHostRef.current = activeRemoteHost;
  const [projects, setProjects] = useState<RegisteredProject[]>(initialProjects);
  const [activeProjectId, setActiveProjectId] = useState(initialActiveProjectId);

  useEffect(() => {
    // Project membership is app-global daemon state; the primary window owns publishing it.
    if (!isNativeRuntime || !isPrimary) return;
    const sync = () => {
      void sendDesktopLocalProjects(projects).catch((error) => {
        console.error("Desktop project membership sync failed:", error);
      });
    };
    sync();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("desktop_local_projects_resync", sync).then((stop) => {
      if (disposed) stop(); else unlisten = stop;
    });
    window.addEventListener("focus", sync);
    return () => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("focus", sync);
    };
  }, [isNativeRuntime, isPrimary, projects]);

  useEffect(() => {
    startNativeWindowFocusTracking();
  }, []);
  const [registeredProjectId, setRegisteredProjectId] = useState<string | null>(null);
  const [registrationAttempt, setRegistrationAttempt] = useState(0);
  const [registrationError, setRegistrationError] = useState<{
    workspaceId: string;
    code: string;
    message: string;
    details?: Record<string, unknown>;
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
  const agentSettingsRef = useRef(agentSettings);
  agentSettingsRef.current = agentSettings;
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
    worktreeActivity = {},
    worktreeActivityByKey,
    attentionBySessionId = NO_ATTENTION,
    unreadBadgeCount: unreadBadgeCountFromStore,
    activityNotificationTargets,
    markTabUnread,
    markWorktreeUnread,
    subscribeTerminalBell,
    subscribeActivityNotification,
    subscribeSessionActivitySeen,
    parkedActivityVersion,
    openTab,
    createBrowserTab,
    openFilePreviewTab,
    openChangesTab,
    adoptBrowserSession,
    adoptBackendSession,
    releaseTabsForMove,
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
    setPaneView,
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
    beginLaneLifecycle,
    cancelLaneLifecycle,
    refreshLaneLifecycles,
  } = useWorkspaceStore({ workspaceId: activeProject.workspaceId });
  const { laneOrder, laneLifecycles } = useLaneLifecycleModel(state.laneOrder, state.laneLifecycle, state.lanePinnedTabs, state.lanePullRequests);
  const handleLaneAction = useLaneActionHandler(activeProject.workspaceId, beginLaneLifecycle, cancelLaneLifecycle);
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

  useEffect(() => subscribeActivityNotification((event) => {
    // Activity edges come from this window's own store, so this window owns the session.
    const backendSessionId = findBackendSessionId(stateRef.current, event.sessionId, event.workspaceId);
    coordinatorRef.current!.handleAgentStateChange({
      ...event,
      nextState: event.state,
      backendSessionId: backendSessionId ?? undefined,
      ownsSession: true,
    });
  }), [subscribeActivityNotification]);

  useEffect(
    () =>
      subscribeSessionActivitySeen
        ? subscribeSessionActivitySeen((event) => {
            coordinatorRef.current?.withdrawForSession(event.workspaceId, event.sessionId);
            const backendSessionId = findBackendSessionId(stateRef.current, event.sessionId, event.workspaceId);
            if (backendSessionId) {
              try {
                const ack = tauriApi.acknowledgeAgentOutcome;
                if (typeof ack === "function") {
                  void ack(backendSessionId).catch(() => undefined);
                }
              } catch {
                // Partial test mocks of `./lib/tauri` may omit `acknowledgeAgentOutcome`.
              }
            }
          })
        : undefined,
    [subscribeSessionActivitySeen],
  );

  // The supervisor snapshot feeds the setup banner and the panes' supervision; start it once.
  useEffect(() => { void startSupervisorFeed(); }, []);
  // The daemon's attention groups drive the badge and the agent notifications.
  useEffect(() => { void startDaemonAttentionFeed(); }, []);

  const handleTerminalBell = useCallback((sessionId: string, tabId: string, eventTarget: import("./state/workspaceStore").ActivityNotificationTarget | undefined) => {
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

    // The bell resolved against this window's own store (mounted or parked), so it owns the session.
    const backendSessionId = findBackendSessionId(stateRef.current, sessionId, target?.workspaceId);
    coordinatorRef.current!.handleTerminalBell({
      backendSessionId: backendSessionId ?? undefined,
      ownsSession: true,
      workspaceId: target?.workspaceId,
      workspaceLabel,
      sessionId,
      tabId,
      worktreePath,
      worktreeLabel,
      terminalTitle,
    });
  }, []);

  // The bell arrives from the store's global native subscription, not from a pane prop: only the
  // active tab's panes are mounted, and a bell in the tab you are watching is not what notifies.
  useEffect(
    () => subscribeTerminalBell((sessionId, tabId, target) => handleTerminalBell(sessionId, tabId, target)),
    [handleTerminalBell, subscribeTerminalBell],
  );

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
            const listed = await listPairedProjectWorktrees(activeProject).catch((error: unknown) => {
              // Runtime refresh reports errors without rejecting registration; retain the paired
              // failure here so its Retry and settings actions remain available.
              if (activeProjectRef.current.workspaceId === activeProject.workspaceId) {
                const ipcError = toIpcError(error);
                setRegistrationError({ workspaceId: activeProject.workspaceId, code: ipcError.code,
                  message: ipcError.message, details: ipcError.details });
              }
              throw error;
            });
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
  // Without this the host's history recorder never learns history is on: it starts disabled, so
  // no visit is ever written and the library stays empty (audit run 42341850075).
  useBrowserLibrarySync(isNativeRuntime, reportRuntimeError);
  // The global browser store's startup migration + live sync, the auto-archive policy and the
  // mini link opener are app-lifetime: they belong to the store, not to the mode on screen.
  useMahoBrowserRuntime(isNativeRuntime);

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
        // gitRoot follows the folder: a later `git init` or removed `.git` flips it.
        if (!previous || (previous.gitRoot === registered.gitRoot &&
            previous.gitRemote === registered.gitRemote &&
            previous.gitCommonDir === registered.gitCommonDir)) return current;
        const next = [...current];
        next[index] = {
          ...previous,
          gitRoot: registered.gitRoot,
          gitRemote: registered.gitRemote,
          gitCommonDir: registered.gitCommonDir,
        };
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
            details: ipcError.details,
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
          ((!isRemoteWorkspaceId(session.workspaceId) && !isPairedWorkspaceId(session.workspaceId)) ||
            // A remote pane whose backend was confirmed gone resumes its agent like a local one.
            session.backendUnavailableReason === REMOTE_SESSION_GONE_REASON),
      );

      const shellRecoverySessionIds: string[] = [];
      let hasResumableAgents = false;
      for (const session of deadSessions) {
        const isAgent = Boolean(session.agentType || session.providerSession || session.agentSessionId);
        const hasRetainedRecoveryProvenance = Boolean(
          session.recoveryKey ||
          session.remoteConnectionState === "reconnecting" ||
          session.reconnectLifecycle != null ||
          session.recoveryToken != null ||
          session.backendUnavailableReason != null
        );
        if (!isAgent) {
          // Only eagerly spawn fallback shells for null backendSessionIds, not standby sessions
          if (!recoveredFromHmr && session.backendSessionId === null && !hasRetainedRecoveryProvenance &&
              !isRemoteWorkspaceId(session.workspaceId) && !isPairedWorkspaceId(session.workspaceId)) {
            shellRecoverySessionIds.push(session.id);
          }
        } else {
          const affordance = getAgentReconnectAffordance(session, restoredState.sessions);
          if (affordance.canReconnect) {
            hasResumableAgents = true;
          } else if (!recoveredFromHmr && session.backendSessionId === null && !hasRetainedRecoveryProvenance) {
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
    reconcileLocalSessions: () => wakeLocalSessionRecovery(activeProject.workspaceId),
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
  const persistSessionStrict = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState, current?: () => boolean) => {
    return enqueueStrictPersistence(saveChainRef, async () => {
        if (current && !current()) throw new Error("Recovery admission cancelled");
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
        if (current && !current()) throw new Error("Recovery admission cancelled");
        cachedLoadedSessionRef.current = session;
        await saveSession(session);
        // Layout sync: workspaces this window is not showing still have to reach the daemon, or
        // a paired client could never sync them (it never publishes over revision 0).
        if (isNativeRuntime && isPrimary) {
          void publishPersistedLayouts(
            Object.values(session.workspaces),
            layoutSyncHostTransport,
            activeProjectRef.current?.workspaceId ?? null,
          ).catch((error) => console.error("Layout sync publish failed:", error));
        }
    });
  }, [isNativeRuntime, isPrimary]);
  useEffect(() => {
    setLocalRecoveryPersistence((workspaceId, snapshot, current) => {
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === workspaceId);
      if (!project) return Promise.reject(new Error("Recovery workspace is unavailable"));
      return persistSessionStrict(workspaceId, project.repoRoot, snapshot, current);
    });
  }, [persistSessionStrict]);
  const persistSession = useCallback((workspaceId: string, repoRoot: string, currentState: WorkspaceState) => {
    return persistSessionStrict(workspaceId, repoRoot, currentState).catch((error) => {
      console.error("Failed to save workspace session:", error);
    });
  }, [persistSessionStrict]);

  // Layout sync: after a local layout change the host publishes it (300ms debounce) and every
  // window long-polls for remote changes. A remote document only reshapes this view.
  const pairedLayoutAdapterForSync = usePairedLayoutAdapter(activeProject);
  const targetHostPlatform = useTargetHostPlatform(activeProject);
  useLayoutSync({
    project: activeProject,
    state,
    dispatch: dispatchWorkspaceAction,
    adapter: pairedLayoutAdapterForSync,
    enabled: isNativeRuntime
      && registeredProjectId === activeProject.workspaceId
      && workspaceRestoreStatus === "restored"
      && (activeProject.target?.kind !== "pairedDaemon" || pairedMachineFeaturesEnabled),
    onError: (error) => console.error("Layout sync failed:", error),
  });

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
      const ownerWorkspaceId = activeProject.workspaceId;
      const ownerRepoRoot = activeProject.repoRoot;
      const owner = createOwnedWorkspaceAccess(ownerWorkspaceId, {
        isMounted: () => (stateRef.current.workspaceId ?? activeProjectRef.current.workspaceId) === ownerWorkspaceId,
        getState: () => stateRef.current,
        dispatch: dispatchWorkspaceAction,
      });
      return reconnectAgentSession(
        sessionId,
        createAppReconnectDependencies({
          getSessions: () => {
            const current = owner.getState()?.sessions ?? {};
            if (current[sessionId]) return current;
            return { ...lastRestoredSessionsRef.current, ...current };
          },
          dispatch: owner.dispatch,
          persist: async (result, localSession) => {
            const current = owner.getState();
            if (!current) throw new Error("Reconnect workspace is unavailable");
            const nextState = workspaceReducer(current, {
              type: "REBIND_SESSION_BACKEND",
              sessionId: localSession.id,
              backendSessionId: result.sessionId,
              cwd: result.session.cwd ?? localSession.cwd,
              daemonEpoch: result.daemonEpoch,
              incarnation: result.session.incarnation ?? null,
              clearAgent: false,
            });
            await persistSessionStrict(ownerWorkspaceId, ownerRepoRoot, nextState);
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

  const handleOpenNewShell = useCallback(
    (sessionId: string) => {
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
        // Restart reuses the mounted pane, so its measured viewport sizes the new PTY.
        spawn: async (request, options) => {
          const grid = await measureInitialGeometry({ kind: "session", sessionId }).catch(() => null);
          return spawnTerminalDetailed({ ...request, ...initialGridFields(grid) }, options);
        },
        persist: async (result, localSession) => {
          const current = stateRef.current;
          const nextState = workspaceReducer(current, {
            type: "REBIND_SESSION_BACKEND",
            sessionId: localSession.id,
            backendSessionId: result.sessionId,
            cwd: result.session.cwd ?? localSession.cwd,
            daemonEpoch: result.daemonEpoch,
            incarnation: result.session.incarnation ?? null,
            clearAgent: isAgent,
          });
          await persistSessionStrict(activeProject.workspaceId, activeProject.repoRoot, nextState);
        },
      }, { clearAgent: isAgent }).then(() => undefined).catch((error) => {
        reportRuntimeError(error);
        throw error;
      });
    },
    [activeProject.repoRoot, activeProject.workspaceId, dispatchWorkspaceAction, persistSessionStrict, reportRuntimeError],
  );

  /**
   * Start the pane's process over, keeping the pane, its tab and its local session id.
   *
   * A plain shell restarts immediately. An agent pane asks first, because restarting it destroys
   * the running conversation: the dialog offers resuming that provider session or a plain shell.
   */
  const runPaneRestart = useCallback(
    (sessionId: string, startup?: SpawnTerminalRequest["startup"], dropAgent?: boolean) => {
      const session = stateRef.current.sessions[sessionId];
      // An agent pane keeps its identity only when the restart continues its conversation; a
      // plain-shell restart is destructive and the dialog has already said so.
      const clearAgent = Boolean(session?.agentType && (dropAgent === true || !startup));
      if (clearAgent) dispatchWorkspaceAction({ type: "RESET_AGENT_STATE", sessionId });
      return restartPaneSession(
        sessionId,
        {
          getSessions: () => stateRef.current.sessions,
          dispatch: dispatchWorkspaceAction,
          // The pane stays mounted through a restart, so its measured viewport sizes the new PTY.
          spawn: async (request, options) => {
            const grid = await measureInitialGeometry({ kind: "session", sessionId }).catch(() => null);
            return spawnTerminalDetailed({ ...request, ...initialGridFields(grid) }, options);
          },
          persist: async (result, localSession) => {
            const current = stateRef.current;
            const nextState = workspaceReducer(current, {
              type: "REBIND_SESSION_BACKEND",
              sessionId: localSession.id,
              backendSessionId: result.sessionId,
              cwd: result.session.cwd ?? localSession.cwd,
              daemonEpoch: result.daemonEpoch,
              incarnation: result.session.incarnation ?? null,
              clearAgent,
            });
            await persistSessionStrict(activeProject.workspaceId, activeProject.repoRoot, nextState);
          },
        },
        { clearAgent, startup: startup ?? undefined },
      ).then(() => undefined);
    },
    [activeProject.repoRoot, activeProject.workspaceId, dispatchWorkspaceAction, persistSessionStrict],
  );

  const handleRestartPane = useCallback(
    (sessionId: string) => {
      const session = stateRef.current.sessions[sessionId];
      // Restart replaces this pane's process, so its own conversation is resumable even while it
      // is live: the exit-state continuation aside only covers opening a *second* pane.
      const inPlace = session ? getInPlaceRestartStartup(session) : null;
      // A pane whose backend is already gone (remote or paired included) restarts on the existing
      // continuation path; resuming that same conversation still beats dropping the agent.
      const continuation = !inPlace && session
        ? getContinuationAffordance(session, {
            sessions: Object.values(stateRef.current.sessions),
            activityState: stateRef.current.activityBySessionId?.[sessionId]?.state,
            terminalContinueV1: true,
          })
        : null;
      const resumeMode = continuation?.availableModes.includes("resume")
        ? "resume"
        : continuation?.availableModes[0];
      const resumeStartup = inPlace ?? (resumeMode ? continuation?.startup(resumeMode) ?? null : null);
      if (!session?.agentType && !resumeStartup) return runPaneRestart(sessionId);
      setRestartPaneRequest({
        sessionId,
        agentName: agentDisplayNameForType(session.agentType ?? undefined) ?? session.agentType ?? "Agent",
        canResume: resumeStartup !== null,
        resumeStartup,
      });
      return Promise.resolve();
    },
    [runPaneRestart],
  );

  const restartFocusedPane = useCallback(() => {
    const current = stateRef.current;
    const tabId = current.layout.activeTabId;
    const candidate = tabId ? current.layout.tabs.find((tab) => tab.id === tabId) : undefined;
    const terminalTab = candidate && isTerminalTab(candidate) ? candidate : null;
    if (!terminalTab) return;
    const ownerLayout = current.layout.layoutsByTabId?.[terminalTab.id];
    const activeLeaf =
      ownerLayout?.activeLeafId ?? (ownerLayout?.root ? collectLeafIds(ownerLayout.root)[0] : null);
    const sessionId =
      (activeLeaf && ownerLayout?.sessionIdsByLeafId?.[activeLeaf]) || terminalTab.sessionId;
    if (sessionId) void handleRestartPane(sessionId);
  }, [handleRestartPane]);

  const handleReconnectLocalSession = useCallback(
    async (sessionId: string) => {
      await reconnectLocalSession(sessionId, {
        getSessions: () => stateRef.current.sessions,
        recover: () => wakeLocalSessionRecovery(activeProjectRef.current.workspaceId, sessionId),
        list: listTerminalSessions,
        dispatch: dispatchWorkspaceAction,
        persist: async (session) => {
          const current = stateRef.current;
          const nextState = workspaceReducer(current, {
            type: "REBIND_SESSION_BACKEND",
            sessionId,
            backendSessionId: session.backendSessionId!,
            cwd: session.cwd,
            daemonEpoch: session.daemonEpoch,
            ...(session.incarnation != null ? { incarnation: session.incarnation } : {}),
          });
          const project = activeProjectRef.current;
          if (project) {
            await persistSessionStrict(project.workspaceId, project.repoRoot, nextState).catch((err) => {
              console.error("Failed to persist reconnected session:", err);
            });
          }
        },
        openNewShell: handleOpenNewShell,
      });
    },
    [dispatchWorkspaceAction, handleOpenNewShell, persistSessionStrict],
  );

  // One coalesced retry per SSH owner tuple, shared by every waiter (the Reconnect button and
  // each pane's identity refresh). Waiters hold it through `acquire()`; the retry is cancelled
  // only when the last holder releases, so a pane effect re-running never cancels a reconnect
  // the user is still waiting on.
  const reconnectingSshRef = useRef<Map<string, {
    promise: Promise<import("./lib/types").TerminalRemoteStatus>;
    voidPromise?: Promise<void>;
    acquire: () => () => void;
  }>>(new Map());
  const getOrCreateSshOwnerRetryOperation = useCallback((sessionId: string): {
    promise: Promise<import("./lib/types").TerminalRemoteStatus>;
    voidPromise?: Promise<void>;
    acquire: () => () => void;
  } | null => {
    const session = stateRef.current.sessions[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
    if (!session?.backendSessionId) return null;
    const project = activeProjectRef.current;
    const captured = {
      workspaceId: project?.workspaceId ?? session.workspaceId,
      backendSessionId: session.backendSessionId,
      daemonEpoch: session.daemonEpoch,
      incarnation: session.incarnation,
      recoveryKey: session.recoveryKey,
      recoveryToken: session.recoveryToken,
    };
    const key = JSON.stringify([captured.workspaceId, sessionId, captured.backendSessionId, captured.daemonEpoch,
      captured.incarnation, captured.recoveryKey, captured.recoveryToken]);
    const existing = reconnectingSshRef.current.get(key);
    if (existing) return existing;
    let unsubscribe: (() => void) | undefined;
    let connectedStatus: import("./lib/types").TerminalRemoteStatus | undefined;
    let retryAdmitted = false;
    let settled = false;
    let cancelled = false;
    let resolveConnected!: (status: import("./lib/types").TerminalRemoteStatus) => void;
    let rejectConnected!: (error: unknown) => void;
    const connected = new Promise<import("./lib/types").TerminalRemoteStatus>((resolve, reject) => {
      resolveConnected = resolve;
      rejectConnected = reject;
    });
    const cancel = () => {
      cancelled = true;
      if (!settled) {
        settled = true;
        unsubscribe?.();
        rejectConnected({ code: "RECOVERY_CANCELLED", message: "SSH recovery was cancelled" });
      }
    };
    const settleIfConnected = () => {
      if (cancelled || settled || !retryAdmitted || !connectedStatus) return;
      const currentProject = activeProjectRef.current;
      const current = stateRef.current.sessions[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
      if (currentProject?.workspaceId !== captured.workspaceId || !current || current.backendSessionId !== captured.backendSessionId ||
          current.daemonEpoch !== captured.daemonEpoch || current.incarnation !== captured.incarnation ||
          current.recoveryKey !== captured.recoveryKey || current.recoveryToken !== captured.recoveryToken) {
        settled = true;
        unsubscribe?.();
        rejectConnected({ code: "RECOVERY_OWNER_CHANGED", message: "SSH recovery owner changed before reconnect completed" });
        return;
      }
      settled = true;
      unsubscribe?.();
      resolveConnected(connectedStatus);
    };
    const subscriptionReady = onTerminalRemoteStatus((status) => {
      if (status.sessionId !== captured.backendSessionId || status.state !== "connected") return;
      connectedStatus = status;
      settleIfConnected();
    }).then((stop) => {
      unsubscribe = stop;
      if (settled) stop();
    });
    void subscriptionReady.then(async () => {
      if (cancelled || settled) return;
      const res = await retryTerminalRemoteSession(captured.backendSessionId);
      if (cancelled || settled) return;
      if (res.type !== "retryRemoteSessionOk") throw { code: "INVALID_RESPONSE", message: "SSH retry was not admitted" };
      retryAdmitted = true;
      settleIfConnected();
      if (cancelled || settled || connectedStatus) return;
      // A daemon retry on an already-connected session is a no-op that emits no status event
      // (remote.rs retry returns early), so read the current status once instead of waiting forever.
      // A failed read is not a failed retry: keep waiting for the stream event as before.
      let current: import("./lib/types").RemoteSessionStatusResponse | null = null;
      try {
        current = await getTerminalRemoteStatus(captured.backendSessionId);
      } catch (error) {
        console.warn("SSH status read after retry failed; waiting for status event", error);
      }
      if (cancelled || settled || connectedStatus || current?.details?.state !== "connected") return;
      connectedStatus = {
        sessionId: captured.backendSessionId,
        state: "connected",
        generation: current.details.generation,
        failure: current.details.failure,
        replayGap: current.details.replayGap,
      };
      settleIfConnected();
    }).catch((error) => {
      if (!cancelled && !settled) {
        settled = true;
        unsubscribe?.();
        rejectConnected(error);
      }
      reportRuntimeError(error);
    });
    const task = connected;
    let holders = 0;
    const acquire = () => {
      holders += 1;
      let released = false;
      return () => {
        if (released) return;
        released = true;
        holders -= 1;
        if (holders === 0) cancel();
      };
    };
    const operation: {
      promise: Promise<import("./lib/types").TerminalRemoteStatus>;
      voidPromise?: Promise<void>;
      acquire: () => () => void;
    } = { promise: task, acquire };
    reconnectingSshRef.current.set(key, operation);
    void task.finally(() => {
      if (reconnectingSshRef.current.get(key) === operation) reconnectingSshRef.current.delete(key);
    }).catch(() => undefined);
    return operation;
  }, [reportRuntimeError]);

  const handleReconnectSshSession = useCallback(
    (sessionId: string) => {
      const op = getOrCreateSshOwnerRetryOperation(sessionId);
      if (!op) return Promise.reject({ code: "SESSION_NOT_FOUND", message: "Retained SSH backend is unavailable" });
      // The button holds the retry until it settles; it never cancels it.
      const release = op.acquire();
      void op.promise.then(release, release);
      op.voidPromise ??= op.promise.then(() => undefined);
      return op.voidPromise;
    },
    [getOrCreateSshOwnerRetryOperation],
  );

  // Only the mounted project has a live reducer; every other project lives in its snapshot. These
  // route reads and writes to whichever of the two owns the project right now, so recovery can run
  // for a project that is not on screen and keep working if the user switches to it mid-flight.
  const ownedWorkspaceAccess = useCallback((workspaceId: string) => createOwnedWorkspaceAccess(workspaceId, {
    isMounted: () => (stateRef.current.workspaceId ?? activeProjectRef.current.workspaceId) === workspaceId,
    getState: () => stateRef.current,
    dispatch: dispatchWorkspaceAction,
  }), [dispatchWorkspaceAction]);
  const workspaceStateFor = useCallback(
    (workspaceId: string): WorkspaceState | null => ownedWorkspaceAccess(workspaceId).getState(),
    [ownedWorkspaceAccess],
  );
  const dispatchToWorkspace = useCallback(
    (workspaceId: string, action: WorkspaceAction) => ownedWorkspaceAccess(workspaceId).dispatch(action),
    [ownedWorkspaceAccess],
  );

  const handleRefreshSessionIdentity = useCallback(
    (
      sessionId: string,
      onResult: (result: LocalRecoveryAdmissionResult) => void,
      /** A project other than the mounted one; only paired recovery supports it. */
      targetWorkspaceId?: string,
    ): (() => void) => {
      const project = targetWorkspaceId
        ? projectsRef.current.find((candidate) => candidate.workspaceId === targetWorkspaceId)
        : activeProjectRef.current;
      if (!project) {
        onResult({ kind: "cancelled" }); return () => {};
      }
      const sessionsOf = () => workspaceStateFor(project.workspaceId)?.sessions ?? {};
      const session = sessionsOf()[sessionId] ?? lastRestoredSessionsRef.current[sessionId];

      if (project.target?.kind === "pairedDaemon" || isPairedWorkspaceId(project.workspaceId)) {
        if (!session?.backendSessionId) {
          onResult({ kind: "cancelled" });
          return () => {};
        }
        const captured = {
          workspaceId: project.workspaceId,
          backendSessionId: session.backendSessionId,
          daemonEpoch: session.daemonEpoch ?? null,
          incarnation: session.incarnation ?? null,
          recoveryKey: session.recoveryKey,
          recoveryToken: session.recoveryToken,
          remoteGeneration: typeof session.remoteGeneration === "number" ? session.remoteGeneration : null,
        };
        let live = true;
        let settled = false;
        let unlistenFn: (() => void) | undefined;
        let timer: ReturnType<typeof setTimeout> | null = null;

        const stopListening = () => {
          if (unlistenFn) {
            unlistenFn();
            unlistenFn = undefined;
          }
        };

        const cleanup = () => {
          if (timer != null) {
            clearTimeout(timer);
            timer = null;
          }
          stopListening();
        };

        const checkCurrent = () => {
          const current = sessionsOf()[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
          return (
            projectsRef.current.some((candidate) => candidate.workspaceId === captured.workspaceId) &&
            current?.backendSessionId === captured.backendSessionId &&
            (current?.daemonEpoch ?? null) === captured.daemonEpoch &&
            (current?.incarnation ?? null) === captured.incarnation &&
            current?.recoveryKey === captured.recoveryKey &&
            current?.recoveryToken === captured.recoveryToken
          );
        };

        const settle = (result: LocalRecoveryAdmissionResult) => {
          if (!live || settled) return;
          settled = true;
          cleanup();
          onResult(result);
        };

        onResult({ kind: "pending" });
        let retryAdmitted = false;
        let pendingStatus: import("./lib/types").TerminalRemoteStatus | undefined;
        let admissionIdentity: import("./lib/types").PairedSessionActorIdentity | null = null;

        const handleStatus = (status: import("./lib/types").TerminalRemoteStatus) => {
          if (!live || settled || !checkCurrent()) return;
          if (status.sessionId !== captured.backendSessionId) return;
          if (!Number.isSafeInteger(status.generation) || status.generation < 0 ||
              status.generation < (captured.remoteGeneration ?? 0)) return;
          const descriptor = status.descriptor;
          const pairedDescriptor = descriptor && "hostId" in descriptor ? descriptor : null;
          const target = pairedDescriptor?.target;
          const expectedTarget = project.target;
          if (expectedTarget?.kind !== "pairedDaemon") {
            return;
          }
          if (!pairedDescriptor || !target || !("machineId" in target) ||
              !captured.daemonEpoch || !captured.incarnation ||
              pairedDescriptor.backendSessionId !== captured.backendSessionId ||
              pairedDescriptor.incarnation !== captured.incarnation || pairedDescriptor.daemonEpoch !== captured.daemonEpoch ||
              pairedDescriptor.hostId !== expectedTarget.hostId || target.machineId !== pairedHostMachineId(expectedTarget.hostId) ||
              target.daemonEpoch !== captured.daemonEpoch || !target.sessionId) return;

          if (status.state === "disconnected" && status.failure?.kind === "disconnected") {
            // Exact codes from paired_daemon::Proxy::recovery_failed, not transport error prose.
            const reason = status.failure.message;
            switch (reason) {
              case "SESSION_EXPIRED": {
                // The owning host says this exact target (descriptor tuple checked above) expired:
                // the backend is gone, so the pane recovers like a local one instead of blocking.
                if (admissionIdentity && (admissionIdentity.backendSessionId !== captured.backendSessionId ||
                    admissionIdentity.daemonEpoch !== captured.daemonEpoch || admissionIdentity.incarnation !== captured.incarnation)) return;
                dispatchToWorkspace(captured.workspaceId, { type: "REMOTE_SESSION_GONE", sessionId, backendSessionId: captured.backendSessionId });
                settle({ kind: "exited" });
                return;
              }
              case "CONTROL_CONFLICT": case "STALE_GENERATION":
              case "PAIRED_HOST_STALE_GENERATION": case "PAIRED_HOST_UNAUTHORIZED":
              case "UNAUTHORIZED": case "PERMISSION_DENIED": case "PAIRED_HOST_WRONG_MACHINE":
              case "PAIRED_HOST_INVALID_RESPONSE": case "PAIRED_HOST_MIGRATION_PENDING": case "UNSUPPORTED_PROTOCOL": {
                if (admissionIdentity && (admissionIdentity.backendSessionId !== captured.backendSessionId ||
                    admissionIdentity.daemonEpoch !== captured.daemonEpoch || admissionIdentity.incarnation !== captured.incarnation ||
                    (admissionIdentity.generation != null && (!Number.isSafeInteger(admissionIdentity.generation) ||
                      admissionIdentity.generation < 0 || status.generation < admissionIdentity.generation)))) return;
                const current = workspaceStateFor(captured.workspaceId);
                const retained = current?.sessions[sessionId];
                if (!current || !retained) return;
                dispatchToWorkspace(captured.workspaceId, { type: "RESTORE_WORKSPACE", state: { ...current, sessions: {
                  ...current.sessions, [sessionId]: { ...retained,
                    recoveryState: { phase: "blocked", reason }, backendUnavailableReason: reason,
                    remoteConnectionState: "disconnected", reconnectLifecycle: "failed",
                    remoteFailure: status.failure,
                  },
                } } });
                settle({ kind: "blocked", reason });
                return;
              }
            }
          }
          if (status.state !== "connected") return;
          if (!retryAdmitted) {
            pendingStatus = status;
            return;
          }
          if (!pairedDescriptor || !target || !("machineId" in target) || !admissionIdentity ||
              admissionIdentity.backendSessionId !== captured.backendSessionId ||
              admissionIdentity.daemonEpoch !== captured.daemonEpoch || admissionIdentity.incarnation !== captured.incarnation ||
              (admissionIdentity.generation != null &&
                (!Number.isSafeInteger(admissionIdentity.generation) || admissionIdentity.generation < 0)) ||
              pairedDescriptor.backendSessionId !== admissionIdentity.backendSessionId ||
              pairedDescriptor.incarnation !== admissionIdentity.incarnation || pairedDescriptor.daemonEpoch !== admissionIdentity.daemonEpoch ||
              status.generation < (admissionIdentity.generation ?? 0) ||
              pairedDescriptor.hostId !== expectedTarget.hostId ||
              target.machineId !== pairedHostMachineId(expectedTarget.hostId) || target.daemonEpoch !== captured.daemonEpoch ||
              !target.sessionId) return;

          const currentGeneration = status.generation;
          if (currentGeneration <= 0 || currentGeneration < (admissionIdentity.generation ?? 0)) return;
          const current = sessionsOf()[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
          if (!current) return;
          settle({
            kind: "ready",
            session: {
              ...current,
              daemonEpoch: captured.daemonEpoch,
              incarnation: captured.incarnation,
              remoteGeneration: currentGeneration,
              remoteConnectionState: "connected",
            },
          });
        };

        const subscriptionReady = onTerminalRemoteStatus((status) => {
          handleStatus(status);
        }).then((unlisten) => {
          unlistenFn = unlisten;
          if (!live || settled) {
            stopListening();
          }
        }).catch(() => {
          if (!live || settled) return;
          settle({ kind: "cancelled" });
        });

        timer = setTimeout(() => {
          if (!settled && live) {
            settle({ kind: "cancelled" });
          }
        }, 15000);

        void subscriptionReady.then(() => {
          if (!live || settled || !checkCurrent()) return;
          return retryPairedSession({
            backendSessionId: captured.backendSessionId,
            daemonEpoch: captured.daemonEpoch ?? "",
            incarnation: captured.incarnation ?? "",
          });
        }).then((outcome) => {
          if (!outcome || !live || settled || !checkCurrent()) return;
          if (outcome.blocked || outcome.staleTuple) {
            // A block carries the owner's own reason (a revoked pairing, a control conflict, a
            // protocol rejection); a stale tuple is a genuine identity mismatch. Naming the real
            // cause is the difference between an actionable message and "identity conflict" for
            // every distinct failure.
            const reason = localRecoveryBlockReason(outcome.blockedCode);
            settle({ kind: "blocked", reason });
          } else if (outcome.affirmativeExit) {
            // retry_owned matched the exact (backend, epoch, incarnation) tuple before reporting
            // exit: the remote process is gone. Recover the pane on the same host.
            dispatchToWorkspace(captured.workspaceId, { type: "REMOTE_SESSION_GONE", sessionId, backendSessionId: captured.backendSessionId });
            settle({ kind: "exited" });
          } else if (outcome.admitted) {
            admissionIdentity = outcome.actorIdentity ?? null;
            retryAdmitted = true;
            if (pendingStatus) {
              handleStatus(pendingStatus);
            }
          }
        }).catch(() => {
          if (!live || settled) return;
          settle({ kind: "cancelled" });
        });

        return () => {
          live = false;
          cleanup();
        };
      }

      if (targetWorkspaceId) {
        onResult({ kind: "cancelled" }); return () => {};
      }

      if (project.target?.kind === "ssh" || isRemoteWorkspaceId(project.workspaceId)) {
        if (!session?.backendSessionId) { onResult({ kind: "cancelled" }); return () => {}; }
        const captured = {
          backendSessionId: session.backendSessionId,
          daemonEpoch: session.daemonEpoch,
          incarnation: session.incarnation,
          recoveryKey: session.recoveryKey,
          recoveryToken: session.recoveryToken,
        };
        const operation = getOrCreateSshOwnerRetryOperation(sessionId);
        if (!operation) { onResult({ kind: "cancelled" }); return () => {}; }
        let live = true;
        const release = operation.acquire();
        onResult({ kind: "pending" });
        const waiting = operation.promise;
        const stopListening = () => {
          live = false;
          release();
        };
        void waiting.then((status) => {
          if (!live || activeProjectRef.current.workspaceId !== session.workspaceId) return;
          const current = stateRef.current.sessions[sessionId] ?? lastRestoredSessionsRef.current[sessionId];
          if (!current || current.backendSessionId !== captured.backendSessionId ||
              current.daemonEpoch !== captured.daemonEpoch || current.incarnation !== captured.incarnation ||
              current.recoveryKey !== captured.recoveryKey || current.recoveryToken !== captured.recoveryToken) {
            onResult({ kind: "cancelled" });
            return;
          }
          onResult({ kind: "ready", session: { ...current, remoteConnectionState: "connected", remoteGeneration: status.generation } });
        }).catch(() => { if (live) onResult({ kind: "cancelled" }); });
        return () => { live = false; stopListening(); };
      }

      return requestLocalRecoveryAdmission(project.workspaceId, sessionId, true, onResult);
    },
    [getOrCreateSshOwnerRetryOperation, dispatchToWorkspace, workspaceStateFor],
  );

  // A paired pane recovers through its machine's daemon (retry_owned via the paired identity
  // refresh), never through the SSH helper's retryTerminalRemoteSession.
  const handleReconnectPairedSession = useCallback(
    (sessionId: string): Promise<void> => new Promise<void>((resolve, reject) => {
      let dispose: (() => void) | null = null;
      let settled = false;
      dispose = handleRefreshSessionIdentity(sessionId, (result) => {
        switch (result.kind) {
          case "pending":
            return;
          case "ready":
          case "exited":
            settled = true;
            resolve();
            break;
          case "blocked":
            settled = true;
            reject({ code: result.reason, message: result.reason });
            break;
          case "cancelled":
            settled = true;
            reject({ code: "RECOVERY_CANCELLED", message: "Paired session recovery did not complete" });
            break;
          default: {
            const unreachable: never = result;
            return unreachable;
          }
        }
        dispose?.();
      });
      if (settled) dispose();
    }),
    [handleRefreshSessionIdentity],
  );

  const activeAutoResumeCancelRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    return () => {
      activeAutoResumeCancelRef.current?.();
      activeAutoResumeCancelRef.current = null;
    };
  }, [activeProject.workspaceId]);

  useEffect(() => {
    setSessionRebindHandler(async (sessionId, backendSessionId, cwd, daemonEpoch, incarnation) => {
      const current = stateRef.current;
      const nextState = workspaceReducer(current, {
        type: "REBIND_SESSION_BACKEND",
        sessionId,
        backendSessionId,
        cwd,
        daemonEpoch,
        ...(incarnation != null ? { incarnation } : {}),
      });
      dispatchWorkspaceAction({
        type: "REBIND_SESSION_BACKEND",
        sessionId,
        backendSessionId,
        cwd,
        daemonEpoch,
        ...(incarnation != null ? { incarnation } : {}),
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

    activeAutoResumeCancelRef.current?.();
    activeAutoResumeCancelRef.current = scheduleAgentAutoResume({
      workspaceId: activeProject.workspaceId,
      state: pending.state,
      recoveredFromHmr,
      // An agent conversation is always resumed: the restore policy only decides when plain
      // shells start, since a pane left on "Session disconnected" has to be reopened by hand.
      ignorePolicy: true,
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

  // Projects that are not on screen keep their state only as a snapshot, so the mounted store's
  // resume above never reaches them. The watcher resumes every project's agents, local, SSH and
  // paired: the unmounted ones at startup, and any remote agent whose host later reports its
  // session gone. A reboot therefore brings every project back without opening each one.
  const agentAutoResumeStartedRef = useRef(false);
  const agentAutoResumeStopRef = useRef<(() => void) | null>(null);
  useEffect(() => () => agentAutoResumeStopRef.current?.(), []);
  useEffect(() => {
    if (agentAutoResumeStartedRef.current || recoveredFromHmr) return;
    if (registeredProjectId !== activeProject.workspaceId || workspaceRestoreStatus !== "restored") return;
    agentAutoResumeStartedRef.current = true;

    const isMounted = (workspaceId: string) =>
      (stateRef.current.workspaceId ?? activeProjectRef.current.workspaceId) === workspaceId;
    const stateOf = workspaceStateFor;
    const dispatchTo = dispatchToWorkspace;

    const stopParkedRemoteRecovery = startParkedRemoteRecovery({
      projects: projectsRef.current,
      isMounted,
      getState: stateOf,
      dispatch: dispatchTo,
      subscribe: subscribeWorkspaceSnapshots,
      startSsh: (workspaceId, sessions) => {
        const recovery = startSshRecovery({
          sessions,
          dispatch: (action) => dispatchTo(workspaceId, action),
          onError: (error) => console.error("Parked SSH session reconciliation failed:", error),
        });
        void recovery.ready.catch((error) => console.error("Parked SSH status subscription failed:", error));
        return recovery.stop;
      },
      refreshPaired: (workspaceId, sessionId, onReady) =>
        handleRefreshSessionIdentity(sessionId, (result) => {
          if (result.kind === "ready") onReady(result.session);
        }, workspaceId),
    });

    const stopWatcher = startAgentAutoResumeWatcher({
      listWorkspaceIds: () => projectsRef.current.map((project) => project.workspaceId),
      getState: stateOf,
      mountedWorkspaceId: () => stateRef.current.workspaceId ?? activeProjectRef.current.workspaceId,
      subscribe: subscribeWorkspaceSnapshots,
      reconnect: (workspaceId, sessionId) => {
        const project = projectsRef.current.find((candidate) => candidate.workspaceId === workspaceId);
        if (!project) return Promise.resolve();
        dispatchTo(workspaceId, { type: "SUPPRESS_NEXT_ATTENTION", sessionId });
        return reconnectAgentSession(
          sessionId,
          createAppReconnectDependencies({
            getSessions: () => stateOf(workspaceId)?.sessions ?? {},
            dispatch: (action) => dispatchTo(workspaceId, action),
            persist: async (result, localSession) => {
              const current = stateOf(workspaceId);
              if (!current) return;
              const nextState = workspaceReducer(current, {
                type: "REBIND_SESSION_BACKEND",
                sessionId: localSession.id,
                backendSessionId: result.sessionId,
                cwd: result.session.cwd ?? localSession.cwd,
                daemonEpoch: result.daemonEpoch,
                incarnation: result.session.incarnation ?? null,
                clearAgent: false,
              });
              await persistSessionStrict(workspaceId, project.repoRoot, nextState);
            },
          }),
        ).then(() => setSessionSleeping(sessionId, false));
      },
    });
    agentAutoResumeStopRef.current = () => {
      stopWatcher();
      stopParkedRemoteRecovery();
    };
    // Started once per app launch, the first moment the mounted project has restored.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [registeredProjectId, activeProject.workspaceId, workspaceRestoreStatus]);

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

  // Tracks workspaces that had at least one open tab in this window so closing the last tab
  // still persists the empty layout (preventing closed SSH/local tabs from resurrecting on
  // restart) while un-restored initial empty renders remain blocked from clobbering disk.
  const hadTabsByWorkspaceRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    const hasTabs =
      state.layout.tabs.length > 0 ||
      Object.values(state.worktreeLayouts ?? {}).some((l) => l.tabs.length > 0);
    const owner = state.workspaceId ?? activeProject.workspaceId;
    if (hasTabs) {
      hadTabsByWorkspaceRef.current.add(owner);
    }
    if (state.worktrees.length === 0 || (!hasTabs && !hadTabsByWorkspaceRef.current.has(owner))) return;
    // On the render that switches projects, `state` still holds the outgoing
    // project's data. Saving it under the incoming id would overwrite that
    // project's persisted session, so save it under its own owner instead of
    // discarding the newest state of the project being left.
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
  const [addProjectTarget, setAddProjectTarget] = useState<MachineProjectTarget | undefined>(undefined);
  const [createTargetProject, setCreateTargetProject] = useState<RegisteredProject | null>(null);
  const [pendingProjectRemove, setPendingProjectRemove] = useState<RegisteredProject | null>(null);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isResourcesOpen, setIsResourcesOpen] = useState(false);
  const [onboardingSteps, setOnboardingSteps] = useState<OnboardingStepId[] | null>(null);
  const [onboardingInitialStepIndex, setOnboardingInitialStepIndex] = useState(0);
  const [onboardingDoneSteps, setOnboardingDoneSteps] = useState<readonly OnboardingStepId[]>([]);
  // Populated once per launch so the sidebar can show setup progress after the first project.
  const [setupProgressCtx, setSetupProgressCtx] = useState<OnboardingContext | null>(null);
  // Mirrors what the wizard persisted, so the sidebar panel re-derives when a step is recorded
  // instead of reading storage from a memo that cannot observe the write.
  const [onboardingRecordedSteps, setOnboardingRecordedSteps] = useState<readonly OnboardingStepId[]>(
    () => loadOnboardingState().completedSteps
  );
  // A dismissed setup panel is permanent until the wizard is rerun (Settings > Permissions).
  const [setupProgressDismissed, setSetupProgressDismissed] = useState(() => {
    try {
      return window.localStorage.getItem(SETUP_PROGRESS_HINT_STORAGE_KEY) === "dismissed";
    } catch {
      return false;
    }
  });
  // A local project added from the wizard's first-project step reopens the wizard on the
  // "try" step, which offers to create a first worktree there with the default agent.
  const [onboardingAddedProject, setOnboardingAddedProject] = useState<RegisteredProject | null>(null);
  const onboardingResumeRef = useRef<{ steps: OnboardingStepId[] } | null>(null);
  const [onboardingPermissions, setOnboardingPermissions] = useState<SystemPermissionsStatus | null>(null);
  const [whatsNew, setWhatsNew] = useState<WhatsNewEntry | null>(null);
  const [settingsInitialSection, setSettingsInitialSection] = useState<SectionId | undefined>(undefined);
  const [settingsRemoteContext, setSettingsRemoteContext] = useState<RemoteContext | undefined>(undefined);
  const [searchLeafId, setSearchLeafId] = useState<string | null>(null);
  // A pane restart that needs the user to pick between resuming its agent and a plain shell.
  const [restartPaneRequest, setRestartPaneRequest] = useState<{
    sessionId: string;
    agentName: string;
    canResume: boolean;
    resumeStartup: SpawnTerminalRequest["startup"] | null;
  } | null>(null);
  const [isSidebarOpen, setIsSidebarOpen] = useState(loadSidebarOpen);
  // The Activity Bar's mode: the rail is always visible, so this (not the drawer width) decides what
  // the main area shows. Persisted per window under `ferryx.activity.mode`.
  const [activityMode, setActivityMode] = useState<ActivityMode>(loadActivityMode);
  // Each mode's drawer remembers its own width. The Browser and Remote Desktop drawers own their
  // resize handles; these read the same keys so their width survives that work.

  // The Remote Desktop canvas shows one host at a time; the drawer and its tabs both select it.
  const [remoteDesktopHostId, setRemoteDesktopHostId] = useState<string | null>(null);
  const remoteDesktopSessionCount = useSyncExternalStore(
    remoteDesktopStore.subscribe,
    () => liveRemoteDesktopSessionCount(remoteDesktopStore.getState()),
    () => 0,
  );
  const selectActivityMode = useCallback((mode: ActivityMode) => {
    setActivityMode(mode);
    saveActivityMode(mode);
    // A mode with a drawer is useless with the drawer collapsed, so selecting it reveals the
    // drawer; `Cmd+B` still collapses it afterwards without touching the rail.
    if (!isSidebarOpen) {
      setIsSidebarOpen(true);
      persistSidebarOpen(true);
    }
  }, [isSidebarOpen]);
  const [deleteTarget, setDeleteTarget] = useState<Worktree | null>(null);
  const deleteOwnerId = deleteTarget ? resolveWorktreeOwnerId(deleteTarget, projects, activeProject.workspaceId) : undefined;
  const deleteOwnerProject = projects.find((p) => p.workspaceId === deleteOwnerId);
  const [diskManageProject, setDiskManageProject] = useState<RegisteredProject | null>(null);
  const [historyProject, setHistoryProject] = useState<RegisteredProject | null>(null);
  // The collapsed sidebar's badge counts the Supervisor panel's decisions for the owner.
  const supervisorDecisionCount = usePanelDecisionCount();
  // The Worktrees rail mode's badge: agents that finished or are waiting for input in any lane.
  const worktreeAgentAttention = useMemo(
    () => Object.values(worktreeActivityByKey ?? worktreeActivity).reduce(
      (sum, summary) => sum + (summary ? summary.waitingCount + summary.doneCount : 0),
      0,
    ),
    [worktreeActivity, worktreeActivityByKey],
  );
  useDesktopSidebarPublisher({
    // One desktop sidebar model per app (consumed by remote clients): the primary publishes it.
    enabled: isNativeRuntime && isPrimary,
    projects,
    activeProjectId: activeProject.workspaceId,
    worktrees: state.worktrees,
    inactiveProjectWorktrees,
    emptyWorkspaceIds: emptySidebarWorkspaceIds(projects, activeProject.workspaceId, state, listWorkspaceSnapshots()),
    activityByWorktreePath: worktreeActivity,
    ...(worktreeActivityByKey ? { activityByWorktreeKey: worktreeActivityByKey } : {}),
    unreadWorktreePaths: state.unreadWorktreePaths,
    agents,
    ...(laneOrder ? { laneOrder } : {}),
    ...(laneLifecycles ? { laneLifecycles } : {}),
  });
  const [pendingClose, setPendingClose] = useState<{
    target: CloseTarget;
    label: string;
    /** What the dialog calls this close. A single-leaf pane close removes the whole tab. */
    kind: "pane" | "tab" | "batch";
    tabCount: number;
    liveSessionCount: number;
    runningAgentCount: number;
  } | null>(null);
  const [worktreeStatuses, setWorktreeStatuses] = useState<Record<string, DirtyState | undefined>>({});
  // The agent chosen in the Add Worktree dialog for a worktree created in a non-active project
  // starts once the project switch lands the pending worktree.
  const { pendingWorktree, setPendingWorktree, pendingWorktreeAgentRef, setPendingWorktreePath } =
    usePendingWorktree();
  const launchAgentByIdInWorktreeRef = useRef<(worktree: Worktree, agentId: string) => Promise<void>>(
    async () => undefined,
  );
  const pendingWorktreePath = pendingWorktree?.path ?? null;
  const [pendingRemoteSlug, setPendingRemoteSlug] = useState<{
    workspaceId: string;
    createTerminal?: boolean;
    slug?: string | null;
    tabId?: string | null;
  } | null>(null);
  const [pendingNotificationTarget, setPendingNotificationTarget] = useState<NotificationTarget | null>(null);

  const focusedTerminalPayload = useMemo(
    () => deriveFocusedTerminal(activeProject.workspaceId, state, listWorkspaceSnapshots(), attentionBySessionId),
    [activeProject.workspaceId, state, parkedActivityVersion, attentionBySessionId],
  );
  const continuitySelection = useMemo(
    () => deriveContinuitySelection(state, activeProject, focusedTerminalPayload),
    [activeProject, state, focusedTerminalPayload],
  );
  // Read by the focus handler, which must compare against the committed selection without
  // re-creating itself on every selection change.
  const continuitySelectionRef = useRef(continuitySelection);
  continuitySelectionRef.current = continuitySelection;
  const recordContinuityIntent = useDesktopContinuity({
    enabled: isNativeRuntime,
    leader: isAppStateLeader(),
    selection: continuitySelection,
    /* Continuity is an account convenience layered over local work, so its failures are not
       runtime errors: a signed-in machine that is offline, a relay that has not been upgraded,
       and a rejected publication are all expected and must not raise a runtime error per click.
       Losing the CAS race is already a normal outcome inside the publisher. */
    onError: (error: unknown) => console.warn("[continuity] desktop selection publish failed", error),
  });
  const handleContinuityFocusPane = useCallback((tabId: string, leafId: string) => {
    /* Focusing the pane that is already the committed selection is not a new intent: publishing
       it again would start a GET and a PUT, bump the account revision, and make a phone
       publication in flight lose its CAS for no change at all. */
    if (continuitySelectionRef.current.tabId !== tabId || continuitySelectionRef.current.leafId !== leafId) {
      recordContinuityIntent({ workspaceId: activeProjectRef.current.workspaceId, tabId, leafId });
    }
    focusPane(tabId, leafId);
  }, [focusPane, recordContinuityIntent]);
  // Pane frames and tab dots draw the daemon's group for each pane's session (groupOf).
  const paneActivityBySessionId = useMemo(
    () => Object.keys(attentionBySessionId).length === 0
      ? state.activityBySessionId
      : Object.fromEntries(Object.keys(state.sessions).flatMap((sessionId) => {
          const activity = selectSessionActivity(state, sessionId, attentionBySessionId);
          return activity ? [[sessionId, activity] as const] : [];
        })),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reads only sessions and activity.
    [state.sessions, state.activityBySessionId, attentionBySessionId],
  );
  const publishedTerminalPayloadKey = useRef<string | null>(null);
  const [focusedTerminalRetry, setFocusedTerminalRetry] = useState(0);

  useEffect(() => {
    // One active-desktop selection per app: the focused window publishes it, else the primary.
    if (!isAppStateLeader()) {
      publishedTerminalPayloadKey.current = null;
      return;
    }
    const key = JSON.stringify(focusedTerminalPayload);
    if (publishedTerminalPayloadKey.current === key) return;
    publishedTerminalPayloadKey.current = key;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let settled = false;
    void Promise.resolve(publishFocusedTerminal(focusedTerminalPayload)).catch((error: unknown) => {
      // An unpublished selection must not count as published, or the remote view keeps the old
      // one until the focus moves again.
      if (publishedTerminalPayloadKey.current === key) publishedTerminalPayloadKey.current = null;
      const code = toIpcError(error).code;
      // A daemon handing over to its successor answers busy for a moment; the selection is
      // published again once it settles instead of surfacing an error.
      if (code === "DAEMON_BUSY" || code === "HANDOVER_BUSY") {
        if (!settled) retry = setTimeout(() => setFocusedTerminalRetry((value) => value + 1), 1000);
        return;
      }
      reportRuntimeError(error);
    });
    return () => {
      settled = true;
      clearTimeout(retry);
    };
  }, [focusedTerminalPayload, focusedTerminalRetry, isAppStateLeader, reportRuntimeError, windowStateVersion]);

  const unreadBadgeCount = useMemo(
    () => unreadBadgeCountFromStore ?? selectGlobalUnreadBadgeCount(state, activeProject.workspaceId),
    [unreadBadgeCountFromStore, state, activeProject.workspaceId],
  );

  // Every window broadcasts its own unread count; only the badge leader (focused window, else the
  // primary) writes the app-wide total to the dock, so windows never overwrite each other.
  useEffect(() => {
    publishWindowBadgeCount(unreadBadgeCount);
    if (!isAppStateLeader()) return;
    void setBadgeCount(appBadgeTotal(unreadBadgeCount)).catch(reportRuntimeError);
  }, [isAppStateLeader, reportRuntimeError, unreadBadgeCount, windowStateVersion]);

  // Announce the backend sessions this window holds so a broadcast event for one of them is
  // notified only here (see notificationLeader.ts).
  const ownedBackendSessionKey = useMemo(
    () => [...collectBackendSessionIds([state, ...listWorkspaceSnapshots().map(([, snapshot]) => snapshot)])].sort().join("\u0000"),
    [state, parkedActivityVersion],
  );
  useEffect(() => {
    publishWindowSessions(ownedBackendSessionKey ? ownedBackendSessionKey.split("\u0000") : []);
  }, [ownedBackendSessionKey]);

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

  const handleExplicitSelectProject = useCallback((project: RegisteredProject) => {
    recordContinuityIntent({ workspaceId: project.workspaceId });
    handleSelectProject(project);
  }, [handleSelectProject, recordContinuityIntent]);

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

  const externalWorktreePrompt = useExternalWorktreePrompt(activeProject, registeredProjectId, projects);
  const handleImportedExternalWorktrees = useCallback((imported: RegisteredProject[]) => {
    setProjects((current) => {
      const next = [
        ...current.filter((candidate) => !imported.some((project) => project.workspaceId === candidate.workspaceId)),
        ...imported,
      ];
      projectsRef.current = next;
      persistProjects(next);
      return next;
    });
    // An import is meant to make the checkout usable right away, so adopt the first one exactly
    // as Add Project does; otherwise the rows appear but nothing opens.
    const adopted = imported[0];
    if (adopted) {
      setActiveProjectId(adopted.workspaceId);
      persistActiveProjectId(adopted.workspaceId);
      setWorktreeStatuses({});
    }
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
        recordContinuityIntent({ workspaceId: owner.workspaceId, worktreePath: worktree.path });
        setPendingWorktree(worktree);
        return;
      }
      recordContinuityIntent({ workspaceId: activeProjectRef.current.workspaceId, worktreePath: worktree.path });
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
    [ensureTabForWorktree, handleSelectProject, runTabOperation, recordContinuityIntent],
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
    const pendingAgent = pendingWorktreeAgentRef.current;
    pendingWorktreeAgentRef.current = null;
    const agentId = pendingAgent?.path === target.path ? pendingAgent.agentId : null;
    runTabOperation(async () => {
      await ensureTabForWorktree(target);
      if (agentId) await launchAgentByIdInWorktreeRef.current(target, agentId);
    });
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
      recordContinuityIntent({ workspaceId: activeProjectRef.current.workspaceId, tabId });
      if (!isTerminalTab(tab)) {
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
    [activateTab, ensureTabForWorktree, reportRuntimeError, recordContinuityIntent],
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
      // Every window receives the request; only the window holding the remote selection acts.
      if (!isAppStateLeader()) return;
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
    [activateRemoteEntry, dispatchWorkspaceAction, ensureTabForWorktree, handleSelectProject, isAppStateLeader, reportRuntimeError],
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

  // A click that names a session with no pane here is not a dead end: the tab can be closed while
  // its PTY keeps running in the daemon. The queue lives further down (next to the adoption drain),
  // so it is reached through a ref; the drain reports it when the daemon no longer runs it.
  const requeueUnnavigableSessionRef = useRef<(target: NotificationTarget) => void>(() => {});

  const handleNotificationTarget = useCallback(
    (target: NotificationTarget) => {
      if (!target || !target.workspaceId || !target.sessionId) return;
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === target.workspaceId);
      // An unregistered/removed project is stale: never navigate or fall back to another pane.
      if (!project) return;
      if (activeProjectRef.current.workspaceId === target.workspaceId) {
        // No pane here shows the session. If the daemon still runs it, the tab was closed while
        // the PTY kept running: re-attach it through the adoption queue instead of doing nothing.
        // Only a session the daemon no longer has is reported as gone.
        if (!hasNavigableSession(stateRef.current, target.sessionId)) {
          requeueUnnavigableSessionRef.current(target);
          return;
        }
        dispatchWorkspaceAction({ type: "FOCUS_EXISTING_SESSION", sessionId: target.sessionId });
        return;
      }
      // Cross-project: inspect the target workspace's cached state BEFORE switching. A stale or
      // closed session must not yank the user into another project. Match how the workspace will
      // mount (HMR first, then the persisted snapshot).
      const cached = getHmrWorkspaceState(target.workspaceId) ?? getWorkspaceSnapshot(target.workspaceId);
      if (!cached || !hasNavigableSession(cached, target.sessionId)) {
        requeueUnnavigableSessionRef.current(target);
        return;
      }
      handleSelectProject(project);
      setPendingNotificationTarget(target);
    },
    [dispatchWorkspaceAction, handleSelectProject],
  );

  // A Supervisor panel row names a session by `<hostId>:<backendSessionId>`; focus the pane that
  // runs it through the same path a notification uses (no unmount, no new surface). A session of
  // this computer that no pane shows yet (an agent or the supervisor started it) is attached as an
  // active tab through the window-adoption queue: nothing is spawned or closed.
  const handleFocusSupervisorSession = useCallback((sessionRef: string) => {
    const target = findSessionByBackendId(sessionRef, [
      [activeProjectRef.current.workspaceId, stateRef.current],
      ...listWorkspaceSnapshots(),
    ]);
    if (!target) {
      const backendSessionId = localBackendSessionId(sessionRef);
      if (backendSessionId) {
        // A tab of an SSH or paired project runs on that host: a session of this computer opened
        // there would be treated as a remote one.
        const targetKind = activeProjectRef.current.target?.kind;
        if (targetKind === "ssh" || targetKind === "pairedDaemon") {
          toast.info("Open a project on this computer to show this session");
          return;
        }
        // A tab needs a worktree of an open project; without one the adoption would wait forever.
        if (stateRef.current.worktrees.length === 0) {
          toast.info("Open a project to show this session");
          return;
        }
        setPendingAdoptions((queue) => [
          ...queue,
          { backendSessionIds: [backendSessionId], activate: true, attemptGenerations: {}, attempts: 0, origin: "supervisor" },
        ]);
        // The adoption runs asynchronously over the daemon, so without this the click looks like
        // a no-op until the tab appears (or until the miss toast fires).
        toast.info("Opening this session…");
        return;
      }
      toast.info("That session is not open on this computer");
      return;
    }
    // A row can name a pane this window cannot focus (its tab closed while the agent kept
    // running). Say so instead of returning silently through the notification path.
    if (target.workspaceId === activeProjectRef.current.workspaceId) {
      if (!hasNavigableSession(stateRef.current, target.sessionId)) {
        toast.info("That session's tab is closed. Open it again to see it here.");
        return;
      }
    } else {
      const cached = getHmrWorkspaceState(target.workspaceId) ?? getWorkspaceSnapshot(target.workspaceId);
      if (!cached || !hasNavigableSession(cached, target.sessionId)) {
        toast.info("That session's tab is closed. Open it again to see it here.");
        return;
      }
    }
    handleNotificationTarget(target);
    flashPane(target.sessionId);
  }, [handleNotificationTarget]);

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
  // This window's store holds the clicked session: same checks as handleNotificationTarget, so a
  // click is only ever navigated by the window that can actually focus it.
  const ownsNotificationTarget = useCallback((target: NotificationTarget) => {
    if (!target?.workspaceId || !target.sessionId) return false;
    if (!projectsRef.current.some((candidate) => candidate.workspaceId === target.workspaceId)) return false;
    const live = target.workspaceId === activeProjectRef.current.workspaceId
      ? stateRef.current
      : getHmrWorkspaceState(target.workspaceId) ?? getWorkspaceSnapshot(target.workspaceId);
    return Boolean(live && hasNavigableSession(live, target.sessionId));
  }, []);
  // The activation queue is app-global and draining consumes it, so only the primary window
  // drains (exactly once). It navigates the clicks it owns and forwards the rest to the other
  // windows, where the owner navigates and raises itself; a secondary window never drains.
  useEffect(
    () => isPrimary
      ? subscribeNotificationActivations(
        (target) => routeNotificationTarget({
          target,
          ownsTarget: ownsNotificationTarget,
          navigate: (owned) => handleNotificationTargetRef.current(owned),
          forward: (foreign) => forwardNotificationTarget(currentWindowLabel(), foreign),
        }),
        undefined,
        (error) => reportRuntimeErrorRef.current(error),
      )
      : undefined,
    [isPrimary, ownsNotificationTarget],
  );
  useEffect(
    () => isNativeRuntime && !isPrimary
      ? subscribeForwardedNotificationTargets(
        currentWindowLabel(),
        ownsNotificationTarget,
        (target) => handleNotificationTargetRef.current(target),
      )
      : undefined,
    [isNativeRuntime, isPrimary, ownsNotificationTarget],
  );

  const refuseTerminalOpen = useCallback((reason: string, message: string) => {
    switchDebug("terminal.open.refused", { workspaceId: activeProjectRef.current.workspaceId, reason });
    toast.warning(message);
  }, []);

  const handleAddTerminalTab = useCallback((shell?: string) => {
    // Every refusal below used to return silently, so a click on "New Terminal" could do nothing
    // with no trace. Say why, and record it so a release log shows which guard fired.
    if (activeRemoteHostRef.current) {
      refuseTerminalOpen("remote-host-active", "Terminal was not opened because a remote host is active.");
      return;
    }
    if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) {
      refuseTerminalOpen("paired-features-disabled", "Terminal was not opened: paired machine features are not enabled.");
      return;
    }
    if (
      (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
      registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
    ) {
      refuseTerminalOpen("project-not-registered", "Terminal was not opened: this project is still connecting.");
      return;
    }
    const activeWt = activeWorktreeRef.current ?? stateRef.current.worktrees[0];
    if (!activeWt) {
      refuseTerminalOpen("no-active-worktree", "Terminal was not opened: no worktree is selected.");
      return;
    }
    runTabOperation(async () => {
      const tabId = await openTab(activeWt, undefined, undefined, shell);
      // openTab discards the new terminal when the workspace changed while it was spawning.
      if (tabId === null) toast.warning("Terminal was not opened because the workspace changed while it was starting. Try again.");
      return tabId;
    });
  }, [openTab, refuseTerminalOpen, runTabOperation]);

  // Starts an agent in a new tab of the given worktree of the active project, which need not be
  // the active worktree (a freshly created one is not selected yet).
  const launchAgentInWorktree = useCallback(
    async (
      targetWorktree: Worktree,
      agent: { name: string; command: string; args: string },
      options: { view?: PaneView } = {},
    ) => {
      if (activeRemoteHostRef.current) {
        refuseTerminalOpen("remote-host-active", "Terminal was not opened because a remote host is active.");
        return;
      }
      if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) {
        refuseTerminalOpen("paired-features-disabled", "Terminal was not opened: paired machine features are not enabled.");
        return;
      }
      if (
        (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
        registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
      ) {
        refuseTerminalOpen("project-not-registered", "Terminal was not opened: this project is still connecting.");
        return;
      }
      try {
        await ensureTerminalEvents().catch(() => undefined);
        const agentGrid = await measureInitialGeometry({
          kind: "newTab",
          groupId: stateRef.current.layout.focusedGroupId ?? null,
        }).catch(() => null);
        const spawnRequest = {
          workspaceId: activeProjectRef.current.workspaceId,
          worktree: worktreeIdentity(targetWorktree),
          cwd: targetWorktree.path,
          ...initialGridFields(agentGrid),
        };
        const fullCommand = `${agent.command} ${agent.args}`.trim();
        // argv[0] is the configured command verbatim (it may be a path with spaces or
        // backslashes); only the argument string is split.
        const command = agent.command.trim();
        const args = splitAgentArgs(agent.args);
        // The agent is the pane's startup command, so it cannot race the shell's own startup the
        // way typed input does (PSReadLine drops early keystrokes). A backend that refuses the
        // startup kind (an older daemon, a paired host) still gets the agent typed into a shell.
        let typeCommand = false;
        let spawned;
        try {
          spawned = await spawnTerminalDetailed(
            command ? { ...spawnRequest, startup: { kind: "agentLaunch", command, args } } : spawnRequest,
          );
        } catch (error) {
          if (!command || toIpcError(error).code !== "UNSUPPORTED") throw error;
          spawned = await spawnTerminalDetailed(spawnRequest);
          typeCommand = true;
        }
        const backendSessionId = spawned.sessionId;
        const label = agent.name.charAt(0).toUpperCase() + agent.name.slice(1);
        // The pane needs the daemon epoch and incarnation to form its attach tuple; a bare id
        // leaves the tab blank and unable to accept input.
        const tabId = await openTab(targetWorktree, label, {
          sessionId: backendSessionId,
          ...(options.view === "chat" ? { agentType: agent.name } : {}),
          daemonEpoch: spawned.daemonEpoch ?? null,
          incarnation: spawned.session?.incarnation ?? null,
        }, ...(options.view === "chat" ? [undefined, "chat"] as const : [] as const));
        // openTab does not own a pre-spawned backend, so a discarded tab would orphan the PTY.
        if (tabId === null) {
          await closeTerminal(backendSessionId);
          toast.warning("Terminal was not opened because the workspace changed while it was starting. Try again.");
          return;
        }
        if (typeCommand) {
          await writeTerminal({ sessionId: backendSessionId, data: `${fullCommand}\r` });
        }
        if (options.view === "chat") {
          saveLastChatAgentId(agent.name);
        }
      } catch (error) {
        reportRuntimeError(error);
      }
    },
    [openTab, refuseTerminalOpen, reportRuntimeError],
  );

  const handleLaunchAgent = useCallback(
    async (agent: { name: string; command: string; args: string }) => {
      const targetWorktree = activeWorktreeRef.current ?? stateRef.current.worktrees[0];
      if (!targetWorktree) {
        refuseTerminalOpen("no-active-worktree", "Terminal was not opened: no worktree is selected.");
        return;
      }
      await launchAgentInWorktree(targetWorktree, agent);
    },
    [launchAgentInWorktree, refuseTerminalOpen],
  );

  // Chat view reads the agent's local native history, so it is offered only for local projects.
  const chatViewAvailable =
    !activeRemoteHost &&
    activeProject.target?.kind !== "ssh" &&
    activeProject.target?.kind !== "pairedDaemon" &&
    !isRemoteWorkspaceId(activeProject.workspaceId) &&
    !isPairedWorkspaceId(activeProject.workspaceId);
  const chatViewAvailableRef = useRef(chatViewAvailable);
  chatViewAvailableRef.current = chatViewAvailable;

  const handleNewChat = useCallback(
    async (agent: { name: string; command: string; args: string }) => {
      if (!chatViewAvailableRef.current) {
        refuseTerminalOpen("chat-unavailable", "Chat view is only available for local projects.");
        return;
      }
      const targetWorktree = activeWorktreeRef.current ?? stateRef.current.worktrees[0];
      if (!targetWorktree) {
        refuseTerminalOpen("no-active-worktree", "Terminal was not opened: no worktree is selected.");
        return;
      }
      await launchAgentInWorktree(targetWorktree, agent, { view: "chat" });
    },
    [launchAgentInWorktree, refuseTerminalOpen],
  );

  const handleNewChatShortcut = useCallback(() => {
    if (!chatViewAvailableRef.current) {
      refuseTerminalOpen("chat-unavailable", "Chat view is only available for local projects.");
      return;
    }
    const agent = pickChatAgent(
      chatSupportedAgents(getLaunchableAgents(resolvedAgentsRef.current)),
      loadLastChatAgentId(),
      agentSettingsRef.current.defaultAgentId,
    );
    if (!agent) {
      toast.warning("No chat-capable agent is enabled. Enable one in Agent settings.");
      return;
    }
    void handleNewChat(agent);
  }, [handleNewChat, refuseTerminalOpen]);

  // Resolves a dialog's agent choice against the agents launchable right now.
  const launchAgentByIdInWorktree = useCallback(
    async (targetWorktree: Worktree, agentId: string) => {
      const agent = getLaunchableAgents(resolvedAgentsRef.current).find((candidate) => candidate.name === agentId);
      if (!agent) {
        toast.warning(`${agentId} is not available, so the new worktree opened with a shell only.`);
        return;
      }
      await launchAgentInWorktree(targetWorktree, agent);
    },
    [launchAgentInWorktree],
  );
  launchAgentByIdInWorktreeRef.current = launchAgentByIdInWorktree;

  // Opens a just-created worktree in its tab and starts the chosen agent there; shared by
  // the Add Worktree dialog and the onboarding first-worktree step.
  // Reopening setup with projects already present tries the active project, when it is local.
  const onboardingTryProject =
    onboardingAddedProject ??
    (activeProject !== DEFAULT_PROJECT && (activeProject.target?.kind ?? "local") === "local" ? activeProject : null);
  // Local projects supervisor setup maps (onboarding step and setup banner).
  const setupProjects = useMemo(() => setupProjectsFrom(projects), [projects]);

  const handleWorktreeCreated = useCallback(
    async (owner: RegisteredProject, worktree: Worktree, launch?: WorktreeAgentLaunch) => {
      if (activeRemoteHostRef.current) return;
      if (owner.workspaceId !== activeProjectRef.current.workspaceId) {
        // The project switch clears any pending worktree and its agent, so record the agent after it.
        handleSelectProject(owner);
        pendingWorktreeAgentRef.current = launch ? { path: worktree.path, agentId: launch.agentId } : null;
        setPendingWorktree(worktree);
        return;
      }
      await refreshWorktrees();
      await ensureTabForWorktree(worktree).catch(reportRuntimeError);
      if (launch) await launchAgentByIdInWorktree(worktree, launch.agentId);
    },
    [ensureTabForWorktree, handleSelectProject, launchAgentByIdInWorktree, refreshWorktrees, reportRuntimeError],
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onNewTerminalTabMenu(() => {
      // Delegate even while a remote host is active: handleAddTerminalTab() refuses with a visible
      // reason, where returning here left the native "New Tab" menu item doing nothing at all.
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
  const closingTargetsRef = useRef(new Set<string>());

  const performClose = useCallback(
    (target: CloseTarget) => {
      const key = target.kind === "pane"
        ? `pane:${target.tabId}:${target.leafId}`
        : target.kind === "batch"
          ? `batch:${target.mode}:${target.tabId}`
          : `tab:${target.tabId}`;
      if (closingTargetsRef.current.has(key)) return;
      closingTargetsRef.current.add(key);
      const settle = () => closingTargetsRef.current.delete(key);

      if (target.kind === "batch") {
        const batch = target.mode === "others"
          ? closeOtherTabs(target.tabId)
          : target.mode === "right"
            ? closeTabsToRight(target.tabId)
            : closeTabsToLeft(target.tabId);
        void batch.catch(reportRuntimeError).finally(settle);
        return;
      }

      // A pane close stays a pane close: the store's `closePane` delegates to `closeTab`
      // itself when the leaf is the tab's only pane, so promoting here would change which
      // store command runs for every split-less tab.
      if (target.kind === "pane") {
        void closePane(target.tabId, target.leafId).catch(reportRuntimeError).finally(settle);
        return;
      }

      const currentState = stateRef.current;
      const tab = getTabById(currentState, target.tabId);
      if (tab?.kind === "browser" && tab.url) {
        closedTabStackRef.current.push({
          kind: "browser",
          url: tab.url,
          profileId: tab.profileId,
          worktreePath: tab.worktreePath,
        });
      } else if (tab?.kind === "file") {
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
      void closeTab(target.tabId).catch(reportRuntimeError).finally(settle);
    },
    [closeOtherTabs, closePane, closeTab, closeTabsToLeft, closeTabsToRight, reportRuntimeError],
  );

  const requestClose = useCallback(
    (target: CloseTarget) => {
      if (activeRemoteHostRef.current) return;
      const currentState = stateRef.current;
      const runningSessionIds = new Set<string>();
      for (const [sessionId, activity] of Object.entries(currentState.activityBySessionId ?? {})) {
        if (activity?.state === "working" || activity?.state === "waiting") runningSessionIds.add(sessionId);
      }
      const labelForTab = (tabId: string) => {
        const tab = getTabById(stateRef.current, tabId);
        if (!tab) return "";
        return isTerminalTab(tab) ? staticTabLabel(tab) : tab.label;
      };
      const assessment = assessClose(
        { state: currentState, confirmAlways: generalSettings.confirmCloseTab, runningSessionIds, labelForTab },
        target,
      );
      if (assessment.decision === "refuse") return;
      if (assessment.decision === "close") {
        performClose(assessment.target);
        return;
      }
      setPendingClose({
        target: assessment.target,
        label: assessment.label,
        // A pane that is the tab's only leaf closes the tab, so it is presented as a tab close.
        kind: assessment.target.kind === "pane"
          && getTabLayoutById(stateRef.current, assessment.target.tabId)?.root.type === "leaf"
          ? "tab"
          : assessment.target.kind,
        tabCount: assessment.tabCount,
        liveSessionCount: assessment.liveSessions.length,
        runningAgentCount: assessment.runningAgentCount,
      });
    },
    [generalSettings.confirmCloseTab, performClose],
  );

  const handleCloseTab = useCallback(
    (tabId: string) => {
      requestClose({ kind: "tab", tabId });
    },
    [requestClose],
  );

  const handleClosePane = useCallback(
    (tabId: string, leafId: string) => {
      requestClose({ kind: "pane", tabId, leafId });
    },
    [requestClose],
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

  const handleBrowserPrint = useCallback(
    (browserId: string) => {
      void printBrowser(browserId).catch(reportRuntimeError);
    },
    [reportRuntimeError],
  );

  const handleBrowserZoom = useCallback(
    async (browserId: string, direction: "in" | "out" | "reset") => {
      try {
        // The host owns the zoom ladder and the per-site record: stepping here instead would walk
        // 10% increments past 110% and never remember the site's zoom (audit run 42341850075).
        await stepBrowserZoom(browserId, direction);
      } catch (err) {
        reportRuntimeError(err);
      }
    },
    [reportRuntimeError],
  );

  const handleCloseActiveSurface = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const currentState = stateRef.current;
    const activeTabId = currentState.layout.activeTabId;
    if (!activeTabId) return;

    // Any tab kind can host a split: a browser tab with a terminal pane, a file tab with a
    // browser pane. Closing the surface means closing the focused pane when there is one, and
    // the whole tab only when the pane tree is a single leaf — except for a browser tab, whose
    // webview is owned by the tab and always closes whole.
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === activeTabId);
    if (!activeTab) return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTabId];
    if (activeTab.kind !== "browser" && activeLayout) {
      const activeLeafId = activeLayout.activeLeafId ?? collectLeafIds(activeLayout.root)[0];
      if (activeLeafId) {
        handleClosePane(activeTabId, activeLeafId);
        return;
      }
    }

    handleCloseTab(activeTabId);
  }, [handleClosePane, handleCloseTab]);

  const handleConfirmTabClose = useCallback(() => {
    if (!pendingClose) return;
    const confirmed = pendingClose;
    setPendingClose(null);
    // Re-assess against live state: a session may have exited, or the target may already be
    // gone, while the dialog was open. Executing a stale target would close something the
    // user never saw.
    const runningSessionIds = new Set<string>();
    for (const [sessionId, activity] of Object.entries(stateRef.current.activityBySessionId ?? {})) {
      if (activity?.state === "working" || activity?.state === "waiting") runningSessionIds.add(sessionId);
    }
    const stillValid = reassessConfirmedClose(
      { state: stateRef.current, confirmAlways: true, runningSessionIds, labelForTab: () => confirmed.label },
      confirmed.target,
    );
    if (!stillValid) return;
    performClose(confirmed.target);
  }, [pendingClose, performClose]);

  const handleCancelTabClose = useCallback(() => setPendingClose(null), []);

  const handleCloseOtherTabs = useCallback(
    (tabId: string) => {
      requestClose({ kind: "batch", mode: "others", tabId });
    },
    [requestClose],
  );

  const handleCloseTabsToRight = useCallback(
    (tabId: string) => {
      requestClose({ kind: "batch", mode: "right", tabId });
    },
    [requestClose],
  );

  const handleCloseTabsToLeft = useCallback(
    (tabId: string) => {
      requestClose({ kind: "batch", mode: "left", tabId });
    },
    [requestClose],
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
      if (!activeTab) {
        switchDebug("split.active.refused.no-terminal-tab", { reason: "no-active-tab" });
        return;
      }
      const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
      if (!activeLayout) {
        switchDebug("split.active.fallback.leaf-default.layout-missing", { reason: `tabId=${activeTab.id}` });
      } else if (!activeLayout.activeLeafId) {
        switchDebug("split.active.fallback.leaf-default.active-leaf-missing", { reason: `tabId=${activeTab.id}` });
      }
      // Splitting is a pane action, so it targets the focused leaf whatever the tab kind is. The
      // store still refuses a source leaf that hosts no terminal session (a browser pane), which
      // is where that decision belongs.
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
    requestClose({ kind: "pane", tabId: activeTab.id, leafId: activeLeafId });
  }, [requestClose]);

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
      handleContinuityFocusPane(activeTab.id, leafIds[nextIndex]);
    },
    [handleContinuityFocusPane],
  );

  const handleDirectionalPaneFocus = useCallback(
    (direction: PaneFocusDirection) => {
      if (activeRemoteHostRef.current) return;
      const currentState = stateRef.current;
      const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
      if (!activeTab || activeTab.kind === "browser") return;
      const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
      if (!activeLayout || activeLayout.root.type === "leaf") return;
      const activeLeafId = activeLayout.activeLeafId ?? collectLeafIds(activeLayout.root)[0];
      const targetLeafId = findDirectionalLeafId(activeLayout.root, activeLeafId, direction);
      if (targetLeafId) handleContinuityFocusPane(activeTab.id, targetLeafId);
    },
    [handleContinuityFocusPane],
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

  // Desktop only: the remote web client has no native surface to select on.
  const handleToggleTerminalCopyMode = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    const currentState = stateRef.current;
    const activeTab = currentState.layout.tabs.find((tab) => tab.id === currentState.layout.activeTabId) ?? currentState.layout.tabs[0];
    if (!activeTab || !isTerminalTab(activeTab)) return;
    const activeLayout = currentState.layout.layoutsByTabId?.[activeTab.id];
    const leafId = activeLayout?.activeLeafId ?? (activeLayout?.root ? collectLeafIds(activeLayout.root)[0] : null);
    const sessionId = (leafId && activeLayout?.sessionIdsByLeafId?.[leafId]) || activeTab.sessionId;
    if (!sessionId) return;
    window.dispatchEvent(new CustomEvent<TerminalCopyModeEventDetail>(TERMINAL_COPY_MODE_EVENT, { detail: { sessionId } }));
  }, []);

  const handleSelectWorktreeByIndex = useCallback(
    (index: number) => {
      if (activeRemoteHostRef.current) {
        switchDebug("worktree.shortcut.ignored", { reason: "remote-host", index });
        return;
      }
      const ladder = listShortcutWorktrees({
        projects: projectsRef.current,
        activeProjectId: activeProjectRef.current.workspaceId,
        activeWorktrees: stateRef.current.worktrees,
        inactiveProjectWorktrees: inactiveProjectWorktreesRef.current,
      });
      const target = ladder[index];
      switchDebug("worktree.shortcut.select", {
        index,
        ladderLength: ladder.length,
        target: describeLadderRow(target),
      });
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
    setAddProjectTarget(undefined);
    setIsAddProjectOpen(true);
  }, []);
  const handleOpenSshProject = useCallback((hostId: string) => {
    if (activeRemoteHostRef.current) return;
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
    setAddProjectHostId(hostId);
    setAddProjectTarget(undefined);
    setIsAddProjectOpen(true);
  }, []);
  // Settings > Remote hands one machine to Remote Desktop mode: connect it in the keep-alive store
  // and switch the shell there instead of opening a worktree tab that would unmount on switch.
  const handleOpenRemoteDesktop = useCallback((hostId: string, label: string) => {
    if (activeRemoteHostRef.current) return;
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
    setSettingsRemoteContext(undefined);
    setRemoteDesktopHostId(hostId);
    selectActivityMode("remoteDesktop");
    void remoteDesktopStore.connect(hostId, label);
  }, [selectActivityMode]);
  // Settings > Remote hands one machine to Add Project; the dialog routes pairedDaemon to its form.
  const handleOpenMachineProject = useCallback((target: MachineProjectTarget) => {
    if (activeRemoteHostRef.current) return;
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
    setSettingsRemoteContext(undefined);
    setAddProjectHostId(undefined);
    setAddProjectTarget(target);
    setIsAddProjectOpen(true);
  }, []);
  const handleCloseAddProject = useCallback(() => {
    onboardingResumeRef.current = null;
    setIsAddProjectOpen(false);
    setAddProjectTarget(undefined);
  }, []);
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
  const handleShowChanges = useCallback(
    (worktree: Worktree) => {
      if (activeRemoteHostRef.current) return;
      const ownerId = resolveWorktreeOwnerId(worktree, projectsRef.current, activeProjectRef.current.workspaceId);
      const owner = projectsRef.current.find((project) => project.workspaceId === ownerId);
      if (!owner || (owner.target && owner.target.kind !== "local") || owner.gitRoot === null) return;
      if (owner.workspaceId !== activeProjectRef.current.workspaceId) {
        // Tabs live in the owning project's store; switch there first, then open Changes again.
        handleSelectWorktree(worktree);
        return;
      }
      openChangesTab(worktree, owner.workspaceId);
    },
    [handleSelectWorktree, openChangesTab],
  );
  const activeWorktreeCanShowChanges = Boolean(
    activeWorktree && activeProject.gitRoot !== null && (!activeProject.target || activeProject.target.kind === "local"),
  );
  const handleShowActiveWorktreeChanges = useCallback(() => {
    const worktree = activeWorktreeRef.current;
    if (worktree) handleShowChanges(worktree);
  }, [handleShowChanges]);
  const handleOpenCommandPalette = useCallback(() => {
    if (activeRemoteHostRef.current) return;
    setIsCommandPaletteOpen(true);
  }, []);
  const handleCloseCommandPalette = useCallback(() => setIsCommandPaletteOpen(false), []);
  // Flush the in-memory layout before cloning it: the normal save is debounced, so opening a
  // window immediately after editing tabs must not clone a stale file. Restored panes re-attach
  // to the same daemon PTYs with a newer attach generation. Closing a window never kills those
  // PTYs; ownership follows the latest attach and any remaining sessions use the orphan handoff.
  const handleNewWindow = useCallback(() => {
    void (async () => {
      const snapshot = stateRef.current;
      const active = activeProjectRef.current;
      const owner = snapshot.workspaceId ?? active.workspaceId;
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === owner) ?? active;
      await persistSessionStrict(owner, project.repoRoot, snapshot);
      await openNewWindow(currentWindowLabel());
    })().catch(reportRuntimeErrorRef.current);
  }, [persistSessionStrict]);

  useEffect(() => {
    let active = true;
    let unlisten: () => void = () => undefined;
    void onNewWindowMenu(handleNewWindow).then((dispose) => {
      if (!active) dispose();
      else unlisten = dispose;
    });
    return () => {
      active = false;
      unlisten();
    };
  }, [handleNewWindow]);

  // A closed secondary window hands its live backend sessions to this window (the shell lane
  // emits `window_sessions_orphaned` to the primary/focused window). They are re-attached as
  // background tabs by backendSessionId; nothing is spawned or killed. The closed window's
  // session file is discarded only after every live session found a tab here.
  const adoptBackendSessionRef = useRef(adoptBackendSession);
  adoptBackendSessionRef.current = adoptBackendSession;
  useEffect(() => {
    if (!isNativeRuntime) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let chain: Promise<void> = Promise.resolve();
    const adopt = async (fromLabel: string, sessionIds: string[]) => {
      forgetWindow(fromLabel);
      const live = await listTerminalSessions();
      if (disposed) return;
      const plan = planOrphanAdoption(sessionIds, stateRef.current, live);
      switchDebug("window.orphans.plan", {
        fromLabel,
        adopt: plan.adopt.map((entry) => entry.backend.sessionId),
        alreadyAttached: plan.alreadyAttached,
        missing: plan.missing,
      });
      let adoptedAll = true;
      for (const entry of plan.adopt) {
        if (disposed) return;
        try {
          const tabId = await adoptBackendSessionRef.current(entry.worktree, entry.backend);
          if (tabId === null) adoptedAll = false;
        } catch (error) {
          adoptedAll = false;
          reportRuntimeErrorRef.current(error);
        }
      }
      // The primary window's session file is the app's long-lived session: the backend refuses to
      // discard it (a clone saves into its own file). Asking anyway would surface a spurious
      // runtime error every time the primary window closes behind a surviving clone.
      if (adoptedAll && !disposed && fromLabel !== PRIMARY_WINDOW_LABEL) {
        await discardWindowSession(fromLabel);
      }
    };
    void onWindowSessionsOrphaned(({ fromLabel, sessionIds }) => {
      chain = chain
        .then(() => adopt(fromLabel, sessionIds))
        .catch((error: unknown) => reportRuntimeErrorRef.current(error));
    }).then((stop) => {
      if (disposed) stop(); else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isNativeRuntime]);

  useEffect(() => {
    if (activeRemoteHost) {
      setIsCommandPaletteOpen(false);
    }
  }, [activeRemoteHost]);

  // Sessions another window moved here (`window_sessions_adopt`). The listener is registered
  // first and the boot-time queue drained once afterwards, so a request that arrived before this
  // window booted is applied exactly once. Each request waits for its workspace to be mounted,
  // then re-attaches by backendSessionId as a tab; nothing is spawned or closed.
  // `origin: "supervisor"` marks a Supervisor panel row's request: nothing was moved between windows.
  const [pendingAdoptions, setPendingAdoptions] = useState<
    Array<WindowSessionsAdoptPayload & { attempts: number; origin?: "supervisor" | "reopen" }>
  >([]);
  const adoptionBusyRef = useRef(false);
  const [adoptionTick, bumpAdoptionTick] = useState(0);
  requeueUnnavigableSessionRef.current = (target: NotificationTarget) => {
    const project = projectsRef.current.find((candidate) => candidate.workspaceId === target.workspaceId);
    if (!project) return;
    if (project.target?.kind === "ssh" || project.target?.kind === "pairedDaemon") {
      toast.info("Open a project on this computer to show this session");
      return;
    }
    const known =
      target.workspaceId === activeProjectRef.current.workspaceId
        ? stateRef.current
        : getHmrWorkspaceState(target.workspaceId) ?? getWorkspaceSnapshot(target.workspaceId);
    if (!known || known.worktrees.length === 0) {
      toast.info("Open a project to show this session");
      return;
    }
    // A notification names a session by its frontend id, while the daemon lists sessions by their
    // backend id. Queueing the frontend id made every requeue miss the inventory and report a
    // still-running session as exited, so resolve it through the session this workspace recorded.
    // An id the workspace no longer knows is passed through unchanged: it may already be a backend
    // id (a notification persisted by an older build), and the inventory is the only judge then.
    const recorded = known.sessions[target.sessionId];
    const backendSessionId = recorded ? recorded.backendSessionId : target.sessionId;
    if (!backendSessionId) {
      toast.info("That session has already exited.");
      return;
    }
    setPendingAdoptions((queue) => [
      ...queue,
      {
        backendSessionIds: [backendSessionId],
        activate: true,
        workspaceId: target.workspaceId,
        attemptGenerations: {},
        attempts: 0,
        origin: "reopen",
      },
    ]);
    // The adoption runs asynchronously over the daemon; without this the click looks like a no-op.
    toast.info("Opening this session…");
  };
  useEffect(() => {
    if (!isNativeRuntime) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const enqueue = (payload: WindowSessionsAdoptPayload) => {
      if (!disposed) setPendingAdoptions((queue) => [...queue, { ...payload, attempts: 0 }]);
    };
    void onWindowSessionsAdopt(enqueue)
      .then(async (stop) => {
        if (disposed) {
          stop();
          return;
        }
        unlisten = stop;
        for (const payload of await takePendingWindowAdoptions()) enqueue(payload);
      })
      .catch((error: unknown) => reportRuntimeErrorRef.current(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isNativeRuntime]);
  useEffect(() => {
    const head = pendingAdoptions[0];
    if (!head || adoptionBusyRef.current) return;
    if (head.workspaceId && head.workspaceId !== activeProject.workspaceId) {
      const project = projectsRef.current.find((candidate) => candidate.workspaceId === head.workspaceId);
      if (project) {
        handleSelectProject(project);
        return;
      }
      // The source project is unknown here: adopt into the project this window shows.
      setPendingAdoptions((queue) => [{ ...queue[0]!, workspaceId: undefined }, ...queue.slice(1)]);
      return;
    }
    if (
      state.workspaceId !== activeProject.workspaceId ||
      workspaceRestoreStatus === "idle" ||
      workspaceRestoreStatus === "loading"
    ) return;
    adoptionBusyRef.current = true;
    setPendingAdoptions((queue) => queue.slice(1));
    void (async () => {
      const live = await listTerminalSessions();
      const plan = planOrphanAdoption(head.backendSessionIds, stateRef.current, live, head.attemptGenerations);
      switchDebug("window.adopt.plan", {
        adopt: plan.adopt.map((entry) => entry.backend.sessionId),
        alreadyAttached: plan.alreadyAttached,
        missing: plan.missing,
      });
      const retry: string[] = [];
      let activateNext = head.activate;
      for (const entry of plan.adopt) {
        const tabId = await adoptBackendSessionRef.current(entry.worktree, entry.backend, undefined, { activate: activateNext });
        if (tabId === null) retry.push(entry.backend.sessionId);
        else activateNext = false;
      }
      if (plan.missing.length > 0) {
        toast.warning(
          head.origin === "supervisor"
            ? "That session is no longer running."
            : head.origin === "reopen"
              ? "That session has already exited."
              : "A terminal moved to this window is no longer running.",
        );
      }
      // The workspace changed while adopting: the backend already made this window the owner, so
      // try again once it settles rather than leave the session without a tab.
      if (retry.length > 0 && head.attempts < 3) {
        setPendingAdoptions((queue) => [
          ...queue,
          { ...head, backendSessionIds: retry, workspaceId: activeProjectRef.current.workspaceId, attempts: head.attempts + 1 },
        ]);
      } else if (retry.length > 0) {
        toast.warning("A terminal moved to this window could not be opened.");
      }
    })()
      .catch((error: unknown) => reportRuntimeErrorRef.current(error))
      .finally(() => {
        adoptionBusyRef.current = false;
        bumpAdoptionTick((tick) => tick + 1);
      });
  }, [pendingAdoptions, adoptionTick, activeProject.workspaceId, state.workspaceId, workspaceRestoreStatus, handleSelectProject]);

  // "Move to Window": the backend flips ownership and tells the target to adopt; only after it
  // answers does this window drop the tab. The daemon session is never closed here.
  const handleMoveTabToWindow = useCallback((tabId: string, targetLabel: string | null) => {
    const plan = planTabMove(stateRef.current, tabId);
    if (!plan.ok) {
      toast.warning(plan.reason);
      return;
    }
    void moveSessionsToWindow({
      targetLabel,
      backendSessionIds: plan.backendSessionIds,
      workspaceId: stateRef.current.workspaceId ?? activeProjectRef.current.workspaceId,
    })
      .then((outcome) => {
        releaseTabsForMove([tabId]);
        if (!movedEverySession(outcome, plan.backendSessionIds)) {
          toast.warning("Some terminals in this tab belong to another window and stayed there.");
        }
      })
      .catch((error: unknown) => reportRuntimeErrorRef.current(error));
  }, [releaseTabsForMove]);
  const handleListOtherWindows = useCallback(
    async () => (await listAppWindows()).filter((label) => label !== currentWindowLabel()),
    [],
  );
  const handleOpenSettings = useCallback((section?: SectionId, remoteContext?: RemoteContext) => {
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
    setSettingsRemoteContext(validSection === "remote" ? remoteContext : undefined);
    setIsSettingsOpen(true);
  }, []);
  handleOpenSettingsRef.current = handleOpenSettings;
  const handleOpenSshSettings = useCallback(() => handleOpenSettings("ssh"), [handleOpenSettings]);
  const handleCloseSettings = useCallback(() => {
    setIsSettingsOpen(false);
    setSettingsInitialSection(undefined);
    setSettingsRemoteContext(undefined);
  }, []);

  useEffect(() => {
    if (!isNativeRuntime || !isPrimary || activeRemoteHostRef.current) return;
    let cancelled = false;
    const timer = setTimeout(async () => {
      if (activeRemoteHostRef.current) return;
      const [permissions, cli, enrollment, devices, setup] = await Promise.all([
        getSystemPermissionsStatus().catch(() => null),
        getCliLauncherStatus().catch(() => null),
        getAccountEnrollmentStatus().catch(() => null),
        listRemoteDevices().catch(() => null),
        getSetupStatus(),
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
        pairedDeviceCount: devices ? devices.filter((device) => !device.revoked).length : null,
        supervisorReady: setup.kind === "ok" ? setupFinished(setup.status) : null,
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
  }, [isNativeRuntime, isPrimary]);

  useEffect(() => {
    let cancelled = false;
    const handleOpenOnboarding = async () => {
      const [permissions, cli, enrollment, devices, setup] = await Promise.all([
        getSystemPermissionsStatus().catch(() => null),
        getCliLauncherStatus().catch(() => null),
        getAccountEnrollmentStatus().catch(() => null),
        listRemoteDevices().catch(() => null),
        getSetupStatus(),
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
        pairedDeviceCount: devices ? devices.filter((device) => !device.revoked).length : null,
        supervisorReady: setup.kind === "ok" ? setupFinished(setup.status) : null,
      };
      setOnboardingDoneSteps(satisfiedOnboardingSteps(loadOnboardingState(), ctx));
      setOnboardingRecordedSteps(loadOnboardingState().completedSteps);
      // Re-read the dismissal so "Show Welcome Setup" (which clears the key) brings the sidebar
      // panel back on the same event that reopens the wizard.
      try {
        setSetupProgressDismissed(window.localStorage.getItem(SETUP_PROGRESS_HINT_STORAGE_KEY) === "dismissed");
      } catch {
        // Storage unavailable: keep whatever state this launch already resolved.
      }
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
    if (!isNativeRuntime || !isPrimary) return;
    return startWhatsNewRecorder();
  }, [isNativeRuntime, isPrimary]);

  useEffect(() => {
    if (!isNativeRuntime || !isPrimary) return;
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
  }, [isNativeRuntime, isPrimary]);

  const handleToggleSettings = useCallback(() => {
    preloadSettingsDialog();
    setIsSettingsOpen((current) => !current);
  }, []);
  // The Supervisor panel is the attention surface now; the old Inbox shortcut reveals the sidebar.
  const handleRevealSupervisor = useCallback(() => {
    selectActivityMode("supervisor");
  }, [selectActivityMode]);
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
    // Opening the confirmation must not detach anything: cancelling has to leave every split
    // exactly as it was. The detach runs in the dialog's onDeleted, after deletion is confirmed.
    setDeleteTarget(worktree);
  }, []);
  const handleAddBrowserTab = useCallback(
    (url?: string, profileId?: string) => {
      const targetUrl = url ?? newBrowserTabUrl();
      const task = profileId
        ? createBrowserTab(targetUrl, undefined, { profileId })
        : createBrowserTab(targetUrl);
      void task.then((tabId) => {
        if (!tabId) return;
        // Register the new worktree browser tab in the global store with its `worktreeTag`, so
        // Maho/Browser mode can show it and deleting the worktree only detaches it.
        const currentState = stateRef.current;
        const tab = currentState.layout.tabs.find((candidate) => candidate.id === tabId);
        if (!tab || tab.kind !== "browser") return;
        const worktreePath = tab.worktreePath ?? currentState.activeWorktreePath;
        const worktree = worktreePath
          ? currentState.worktrees.find((candidate) => candidate.path === worktreePath)
          : undefined;
        const identity = worktree ? worktreeIdentity(worktree) : null;
        const workspaceId = identity?.wsId ?? worktree?.workspaceId;
        if (!worktreePath || !workspaceId) return;
        const worktreeLabel = identity?.slug ?? tab.worktreeLabel;
        // A standalone browser tab has no pane leaves: the attachment names the tab itself so
        // `resolveBrowserTabSwitch` and `detachAllForWorkspace` can still find it.
        openBrowserInWorktreeSplit({
          browserId: tab.browserId,
          url: tab.url,
          title: tab.title ?? null,
          workspaceId,
          worktreePath,
          ...(worktreeLabel ? { worktreeLabel } : {}),
          tabId,
          leafId: "root",
        }, browserStore);
      }).catch(reportRuntimeError);
    },
    [createBrowserTab, reportRuntimeError],
  );

  /**
   * Split browser `×`: Detach by default. Two steps, and no backend close:
   *  1. drop the split reference (the live tab stays in the global store), then
   *  2. remove the leaf from the layout so the pane actually disappears.
   * Step 2 dispatches the reducer action directly instead of calling `closePane`, because
   * `closePane` tears the native webview down once the last layout reference is gone - exactly
   * what detaching must not do.
   */
  const handleDetachBrowserPane = useCallback(
    (browserId: string, tabId: string, leafId: string) => {
      detachBrowserFromWorktreeSplit(browserId, browserStore);
      const ownerTab = stateRef.current.layout.tabs.find((tab) => tab.id === tabId);
      if (ownerTab && isTerminalTab(ownerTab)) {
        // The browser pane lives inside a terminal-kind tab: drop just that leaf.
        dispatchWorkspaceAction({ type: "CLOSE_PANE", tabId, leafId });
      } else {
        // A standalone browser tab cannot lose one leaf (the reducer refuses), so the tab itself
        // goes; the session survives in browserStore for Browser mode.
        dispatchWorkspaceAction({ type: "CLOSE_TAB", tabId });
      }
      toast.success("Browser tab detached", { description: "It stays open in Browser mode." });
    },
    [dispatchWorkspaceAction],
  );

  /** Context menu "Open in Browser fullscreen": the same browserId, no reload. */
  const handleOpenBrowserFullscreen = useCallback(
    (browserId: string) => {
      const tab = browserStore.getState().tabs[browserId];
      browserStore.touch(browserId);
      if (tab) browserStore.setActiveSpace(tab.spaceId);
      selectActivityMode("browser");
    },
    [selectActivityMode],
  );

  /** Context menu "Close browser session": the only split path that destroys the session. */
  const handleCloseBrowserSession = useCallback(
    (browserId: string) => {
      closeBrowserSessionFromSplit(browserId, (id) => {
        browserTabNavigation.forget(id);
        void closeBrowser(id).catch(reportRuntimeError);
      }, browserStore);
    },
    [reportRuntimeError],
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
    (tabId: string, leafId: string, direction: PaneDirection, options?: { position?: "first" | "second"; startup?: SpawnTerminalRequest["startup"]; view?: PaneView; agentType?: string }) => {
      if (activeRemoteHostRef.current) {
        refuseTerminalOpen("remote-host-active", "Pane was not split because a remote host is active.");
        return;
      }
      if (activeProjectRef.current.target?.kind === "pairedDaemon" && remoteHostStore.getState().machineFeaturesEnabled !== true) {
        refuseTerminalOpen("paired-features-disabled", "Pane was not split: paired machine features are not enabled.");
        return;
      }
      if (
        (activeProjectRef.current.target?.kind === "ssh" || activeProjectRef.current.target?.kind === "pairedDaemon") &&
        registeredProjectIdRef.current !== activeProjectRef.current.workspaceId
      ) {
        refuseTerminalOpen("project-not-registered", "Pane was not split: this project is still connecting.");
        return;
      }
      const pending = splitPane(tabId, leafId, direction, options);
      if (options?.startup?.kind === "agentContinue") return pending;
      void pending.catch(reportRuntimeError);
    },
    [refuseTerminalOpen, reportRuntimeError, splitPane],
  );

  const handleSplitChat = useCallback(
    (tabId: string, direction: PaneDirection, agent: { name: string; command: string; args: string }) => {
      if (!chatViewAvailableRef.current) return;
      const command = agent.command.trim();
      if (!command) return;
      const tabLayout = stateRef.current.layout.layoutsByTabId?.[tabId];
      if (!tabLayout) return;
      const leafId = tabLayout.activeLeafId ?? collectLeafIds(tabLayout.root)[0];
      if (!leafId) return;
      saveLastChatAgentId(agent.name);
      handleSplitPane(tabId, leafId, direction, {
        startup: { kind: "agentLaunch", command, args: splitAgentArgs(agent.args) },
        view: "chat",
        agentType: agent.name,
      });
    },
    [handleSplitPane],
  );

  const handleSetPaneView = useCallback(
    (tabId: string, leafId: string, view: PaneView) => {
      // Leaving chat is always allowed; entering it is local-only.
      if (view === "chat" && !chatViewAvailableRef.current) return;
      setPaneView(tabId, leafId, view);
    },
    [setPaneView],
  );

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    void onCloseTabMenu(() => {
      if (activeRemoteHostRef.current) return;
      if (forwardCapturedRemoteDesktopShortcut("cmdW")) return;
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

  // An http(s) link handed to Ferryx by the OS (Ferryx set as the default browser, so `open -a`
  // and a link click in another app arrive as an Apple Event) is queued by the backend and
  // announced with an empty event. Nothing opened it before: the queue could be drained but never
  // was, so the link was delivered and silently lost. Every window runs this, which is safe
  // because the take is atomic on the backend: the first window to drain gets the URL and the
  // others get nothing, so the focused window opens it once and no duplicate tab can appear.
  const inboundOpenRef = useRef<(url: string) => void>((url) => {
    void createBrowserTab(url).catch(reportRuntimeError);
  });
  inboundOpenRef.current = (url) => {
    void createBrowserTab(url).catch(reportRuntimeError);
  };
  const inboundDeliveryRef = useRef<ReturnType<typeof startInboundUrlDelivery> | null>(null);
  useEffect(() => {
    if (!isNativeRuntime) return;
    const delivery = startInboundUrlDelivery({
      canDrain: () => !activeRemoteHostRef.current,
      open: (url) => inboundOpenRef.current(url),
    });
    inboundDeliveryRef.current = delivery;
    return () => {
      delivery.dispose();
      inboundDeliveryRef.current = null;
    };
  }, [isNativeRuntime]);

  useEffect(() => {
    if (!activeRemoteHost) inboundDeliveryRef.current?.trigger();
  }, [activeRemoteHost]);

  useEffect(() => {
    const onOpenFilePreview = (event: Event) => {
      const detail = (event as CustomEvent<{
        source: Parameters<typeof openFilePreviewTab>[0];
        request: Parameters<typeof openFilePreviewTab>[1];
      }>).detail;
      if (!detail?.source || !detail.request) return;
      // A remote link was just re-fetched into the same cache path: re-read it.
      openFilePreviewTab(detail.source, previewRequestForSource(detail.source, detail.request));
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

      // `ferryx open` names an absolute LOCAL file the backend already validated, so it
      // carries no backend session: the empty id takes the backend's no-session path
      // instead of a fake id whose failed lookup would be indistinguishable from a
      // dead remote shell. The focused pane is kept only for focus restoration.
      const source: Parameters<typeof openFilePreviewTab>[0] =
        focusedTab && activeLeafId && localSessionId
          ? {
              leafId: activeLeafId,
              sessionId: localSessionId,
              backendSessionId: "",
              workspaceId: session?.workspaceId ?? activeWorkspaceId,
            }
          : {
              leafId: "cli",
              sessionId: "cli",
              backendSessionId: "",
              workspaceId: activeWorkspaceId ?? null,
            };

      const request: Parameters<typeof openFilePreviewTab>[1] = {
        path: parsed.path,
        backendSessionId: source.backendSessionId,
        line: parsed.line ?? null,
        col: parsed.col ?? null,
      };

      openFilePreviewTab(source, request);
      // Routed to the focused window (emit_to); only that window opens the file.
    }, { target: currentWindowLabel() }).then((cleanup) => {
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
      // The store already holds a live tab for this session (a split pane or a Maho tab), so the
      // session is on screen even when no worktree layout tab needs to be created for it.
      const adoptedTabId = adoptBrowserSession(payload.browser, payload.workspaceId ?? undefined);
      if (!adoptedTabId) {
        toast.warning("Browser tab was not shown: session could not be adopted.");
        void reportBrowserAdoption(payload.browser.browserId, false, "adopt-failed").catch(reportRuntimeError);
        return;
      }
      // Also adopt into the global store: Browser mode shows the same session in every Activity
      // mode, with no layout tab and no reload. Store-only work, so the CLI ack stays inside 3s.
      adoptCliBrowserSession(payload.browser, payload.workspaceId ?? undefined, null, browserStore);
      // Make the adopted session the active tab of its Space, so switching to Browser mode lands
      // on the page the CLI/agent just opened.
      focusAdoptedBrowser(payload.browser.browserId, browserStore);
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
      const resolved = resolveBrowserTabSwitch(browserId, browserStore, stateRef.current.layout);
      if (resolved.target === "split") {
        handleSelectTerminalTab(resolved.tabId);
        if (resolved.leafId) handleContinuityFocusPane(resolved.tabId, resolved.leafId);
        return;
      }
      if (resolved.target === "browser-mode") {
        browserStore.touch(resolved.browserId);
        browserStore.setActiveSpace(resolved.spaceId);
        selectActivityMode("browser");
        return;
      }
      toast.warning(`Browser tab for ${browserId} is not open in this window.`);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(reportRuntimeError);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handleContinuityFocusPane, handleSelectTerminalTab, reportRuntimeError, selectActivityMode]);

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
      if (routeSplitEventToBrowserRef.current("right")) return;
      handleSplitActive("horizontal");
    }).then((dispose) => {
      if (cancelled) dispose();
      else unlistenSplitRight = dispose;
    });

    void onSplitDownMenu(() => {
      if (activeRemoteHostRef.current) return;
      if (routeSplitEventToBrowserRef.current("down")) return;
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
  /**
   * The macOS monitor claims Cmd+D and emits menu_split_right, so a browser tab never sees the
   * bookmark chord itself. Browser actions deliberately pre-empt app chords (shortcuts.ts),
   * so on a browser tab that event means bookmark-page. Cmd+Shift+D has no browser binding, so
   * the down direction still falls through to the split handler, which refuses browser tabs.
   * Returns true when the browser took the event.
   */
  const routeSplitEventToBrowser = (direction: "right" | "down"): boolean => {
    if (direction === "down" || !browserShortcutsActive) return false;
    dispatchBrowserShortcut("bookmark-page");
    return true;
  };
  const routeSplitEventToBrowserRef = useRef(routeSplitEventToBrowser);
  routeSplitEventToBrowserRef.current = routeSplitEventToBrowser;

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
      // The page owns OS focus, so Ctrl/Cmd+P reaches the app only as this native chord.
      if ((action as string) === "print") {
        const targetId = payload.browserId || shortcutBrowserId;
        if (targetId) handleBrowserPrint(targetId);
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
    handleBrowserPrint,
    createBrowserTab,
    shortcutBrowserId,
    reportRuntimeError,
  ]);

  /**
   * Browser mode owns the tab chords a browser user expects. Registered before `useShortcuts`
   * (capture phase, earlier listener) so it wins the keystroke, and it prevents the default so the
   * app dispatcher skips the same chord. Worktrees mode is untouched: the router returns null and
   * Ctrl+Tab stays `tab.next`, Cmd+Shift+I stays Supervisor.
   *
   * The router only decides *which* chord fired; the surfaces that act on it (the Maho drawer's
   * Command Bar, the Ctrl+Tab card switcher, closing the Space's active tab) live in
   * `MahoBrowserShell`, so the action travels as the shell's DOM event rather than being
   * re-implemented here. One handler means the key path and any other emitter cannot drift.
   */
  useEffect(() => {
    if (activeRemoteHost) return;
    const isMac = isMacShortcutPlatform();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const action = routeBrowserModeShortcut(event, activityMode, isMac);
      if (!action) return;
      if (event.target instanceof Element && event.target.closest("[data-shortcut-recorder]")) return;
      event.preventDefault();
      window.dispatchEvent(new CustomEvent<BrowserModeShortcutDetail>(BROWSER_MODE_SHORTCUT_EVENT, {
        detail: { action },
      }));
    };
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [
    activeRemoteHost,
    activityMode,
  ]);
  const shortcutHandlers = useMemo(
    () => {
      if (activeRemoteHost) {
        return {
          // Keep the binding live so the refusal is visible: without a handler the shortcut is
          // dropped before dispatch and Mod+T appears to do nothing at all.
          "tab.newTerminal": handleAddTerminalTab,
          "sidebar.left.toggle": toggleSidebar,
          "settings.toggle": handleToggleSettings,
          "notifications.toggle": handleRevealSupervisor,
        };
      }
      return {
        "tab.newTerminal": handleAddTerminalTab,
        "tab.newChat": chatViewAvailable ? handleNewChatShortcut : undefined,
        "tab.newBrowser": () => void createBrowserTab(newBrowserTabUrl()).catch(reportRuntimeError),
        "tab.reopenClosed": handleReopenClosedBrowserTab,
        "tab.close": handleCloseActiveSurface,
        "browser.focusAddress": browserShortcutsActive ? () => dispatchBrowserShortcut("focus-address") : undefined,
        "browser.reload": browserShortcutsActive ? () => dispatchBrowserShortcut("reload") : undefined,
        "browser.hardReload": browserShortcutsActive ? () => dispatchBrowserShortcut("reload-hard") : undefined,
        "browser.back": browserShortcutsActive ? () => dispatchBrowserShortcut("back") : undefined,
        "browser.forward": browserShortcutsActive ? () => dispatchBrowserShortcut("forward") : undefined,
        "browser.find": browserShortcutsActive ? () => dispatchBrowserShortcut("find") : undefined,
        "browser.bookmarkPage": browserShortcutsActive ? () => dispatchBrowserShortcut("bookmark-page") : undefined,
        "browser.print": browserShortcutsActive && shortcutBrowserId ? () => handleBrowserPrint(shortcutBrowserId) : undefined,
        "browser.zoomIn": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "in") : undefined,
        "browser.zoomOut": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "out") : undefined,
        "browser.zoomReset": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "reset") : undefined,
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
        "terminal.focusLeft": browserShortcutsActive ? undefined : () => handleDirectionalPaneFocus("left"),
        "terminal.focusRight": browserShortcutsActive ? undefined : () => handleDirectionalPaneFocus("right"),
        "terminal.focusUp": browserShortcutsActive ? undefined : () => handleDirectionalPaneFocus("up"),
        "terminal.focusDown": browserShortcutsActive ? undefined : () => handleDirectionalPaneFocus("down"),
        "terminal.search": browserShortcutsActive ? undefined : handleOpenTerminalSearch,
        "terminal.copyMode": browserShortcutsActive ? undefined : handleToggleTerminalCopyMode,
        "terminal.restart": browserShortcutsActive ? undefined : restartFocusedPane,
        "sidebar.left.toggle": toggleSidebar,
        "project.add": handleOpenAddProject,
        "commandPalette.open": handleOpenCommandPalette,
        "window.new": isNativeRuntime ? handleNewWindow : undefined,
        "settings.toggle": handleToggleSettings,
        "appearance.cycleTheme": () => cycleAppearanceTheme(paletteAppearance.settings, paletteAppearance.updateSettings),
        "appearance.openSettings": () => openAppearanceSurface(() => handleOpenSettings("appearance")),
        "notifications.toggle": handleRevealSupervisor,
        "zoom.in": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "in") : handleZoomIn,
        "zoom.out": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "out") : handleZoomOut,
        "zoom.reset": browserShortcutsActive && shortcutBrowserId ? () => void handleBrowserZoom(shortcutBrowserId, "reset") : handleZoomReset,
        "activity.mode1": () => selectActivityMode("supervisor"),
        "activity.mode2": () => selectActivityMode("worktrees"),
        "activity.mode3": () => selectActivityMode("browser"),
        "activity.mode4": () => selectActivityMode("remoteDesktop"),
      };
    },
    [
      activeRemoteHost,
      createBrowserTab,
      handleAddTerminalTab,
      chatViewAvailable,
      handleNewChatShortcut,
      handleCloseActiveSurface,
      handleCyclePaneFocus,
      handleDirectionalPaneFocus,
      browserShortcutsActive,
      shortcutBrowserId,
      handleCycleTab,
      handleOpenAddProject,
      handleOpenCommandPalette,
      handleNewWindow,
      isNativeRuntime,
      handleOpenTerminalSearch,
      handleToggleTerminalCopyMode,
      handleRestartPane,
      restartFocusedPane,
      handleSelectTerminalTabByIndex,
      handleSelectWorktreeByIndex,
      handleSplitActive,
      handleRevealSupervisor,
      handleToggleSettings,
      handleOpenSettings,
      paletteAppearance.settings,
      paletteAppearance.updateSettings,
      handleUnsplitActive,
      handleZoomIn,
      handleZoomOut,
      handleZoomReset,
      handleReopenClosedBrowserTab,
      handleBrowserZoom,
      reportRuntimeError,
      toggleSidebar,
      selectActivityMode,
    ],
  );
  const { overrides: shortcutOverrides } = useShortcutOverrides();
  const { preset: shortcutPreset } = useShortcutPreset();
  useShortcuts(shortcutHandlers, { overrides: shortcutOverrides, preset: shortcutPreset });

  const setupProgressItems = useMemo(() => {
    if (!setupProgressCtx) return [];
    return buildSetupProgress(
      { ...setupProgressCtx, agents: resolvedAgents, projectCount: projects.length },
      onboardingRecordedSteps
    );
  }, [onboardingRecordedSteps, projects.length, resolvedAgents, setupProgressCtx]);

  const dismissSetupProgress = useCallback(() => {
    setSetupProgressDismissed(true);
    try {
      window.localStorage.setItem(SETUP_PROGRESS_HINT_STORAGE_KEY, "dismissed");
    } catch {
      // A read-only storage area only costs the dismissal, never the panel itself.
    }
  }, []);

  useEffect(() => {
    if (!isNativeRuntime || !isPrimary || activeRemoteHost) return;
    let cancelled = false;
    const refresh = () => {
      void Promise.all([
        getCliLauncherStatus().catch(() => null),
        getAccountEnrollmentStatus().catch(() => null),
        listRemoteDevices().catch(() => null),
        getSetupStatus(),
      ]).then(([cli, enrollment, devices, setup]) => {
        if (cancelled) return;
        setSetupProgressCtx({
          permissions: null,
          agents: resolvedAgentsRef.current,
          cli,
          projectCount: projectsRef.current.length,
          accountLinked: enrollment?.enrolled === true,
          pairedDeviceCount: devices ? devices.filter((device) => !device.revoked).length : null,
          supervisorReady: setup.kind === "ok" ? setupFinished(setup.status) : null,
        });
      });
    };
    refresh();
    // Refreshing on focus catches goals satisfied in another window (CLI install, pairing, sign-in)
    // without polling; the wizard's own open event covers the in-app path.
    const events = ["focus", OPEN_ONBOARDING_EVENT] as const;
    for (const name of events) window.addEventListener(name, refresh);
    return () => {
      cancelled = true;
      for (const name of events) window.removeEventListener(name, refresh);
    };
  }, [activeRemoteHost, isNativeRuntime, isPrimary, projects.length, resolvedAgents]);

  const sshHostId = activeProject.target?.kind === "ssh" ? activeProject.target.hostId : null;
  const openRemoteMachineSettings = () => handleOpenSettings("remote", {
    page: "machines",
    filter: activeProject.target?.kind === "pairedDaemon" ? "paired" : "ssh",
    machine: sshHostId ? { kind: "ssh", hostId: sshHostId } : undefined,
  });
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

  // The worktree/terminal workspace stays mounted in every mode: unmounting it would tear down
  // PTY attachments and native surfaces, which is exactly the reload this Activity Bar exists to
  // avoid. Browser and Remote Desktop only hide it and draw their own canvas beside it.
  const worktreeDrawerMode = activityMode === "supervisor" || activityMode === "worktrees" ? activityMode : null;
  const workspaceSurfaceVisible = worktreeDrawerMode !== null;
  // Browser mode's whole surface lives in one hook: the Maho drawer, the canvas, the Command Bar
  // and the Ctrl+Tab switcher. Splitting it this way keeps the always-mounted worktree surface
  // exactly where it was while still giving every Browser surface one owner of its state.
  const browserActivity = useBrowserAgentActivity();
  const mahoBrowser = useMahoBrowserShell({
    active: activityMode === "browser",
    drawerOpen: isSidebarOpen,
    activity: browserActivity,
    onRevealBrowser: handleOpenBrowserFullscreen,
  });
  return (
    <div className="flex h-screen w-screen select-none overflow-hidden bg-background font-sans text-foreground">
      <Toaster />
      {isNativeRuntime ? <BrowserAgentApprovalCard /> : null}
      <ShortcutHints
        preset={shortcutPreset}
        enabledActions={resolvedShortcuts(shortcutOverrides, shortcutPreset).filter((shortcut) => shortcutHandlers[shortcut.id]).map((shortcut) => shortcut.id)}
        getContext={() => {
          const current = stateRef.current;
          const group = current.layout.focusedGroupId ? current.layout.tabGroups?.[current.layout.focusedGroupId] : undefined;
          const activeTab = current.layout.tabs.find((tab) => tab.id === current.layout.activeTabId);
          const paneLayout = activeTab ? current.layout.layoutsByTabId?.[activeTab.id] : undefined;
          return {
            tabIds: Array.from({ length: 9 }, (_, index) => group?.tabIds[index] ?? current.layout.tabs[index]?.id ?? ""),
            closeTabId: activeTab && !activeTab.pinned && (!paneLayout || paneLayout.root.type === "leaf") ? activeTab.id : null,
            worktrees: listShortcutWorktrees({
              projects: projectsRef.current,
              activeProjectId: activeProjectRef.current.workspaceId,
              activeWorktrees: current.worktrees,
              inactiveProjectWorktrees: inactiveProjectWorktreesRef.current,
            }),
          };
        }}
      />
      <TerminalLinkActions />
      <TerminalFileLinkActions />
      <DesktopActivityBar
        mode={activityMode}
        onModeChange={selectActivityMode}
        supervisorBadgeTestId="collapsed-attention-badge"
        supervisorBadge={supervisorDecisionCount > 0 ? supervisorDecisionCount : null}
        worktreesBadge={worktreeAgentAttention > 0 ? worktreeAgentAttention : null}
        remoteDesktopBadge={remoteDesktopSessionCount > 0 ? remoteDesktopSessionCount : null}
        browserBadge={browserActivity.badgeCount > 0 ? browserActivity.badgeCount : null}
        sidebarOpen={isSidebarOpen}
        onToggleSidebar={toggleSidebar}
        onOpenResources={() => setIsResourcesOpen(true)}
        onOpenSettings={handleOpenSettings}
        isMac={isMacShortcutPlatform()}
      />
      {isSidebarOpen && worktreeDrawerMode ? (
        <Sidebar
          open={true}
          widthMode={worktreeDrawerMode}
          view={worktreeDrawerMode}
          hideViewTabs
          // The rail owns the traffic-light clearance.
          isMac={false}
          projects={projects}
          activeProjectId={activeProject.workspaceId}
          registeredProjectId={registeredProjectId}
          worktrees={state.worktrees}
          inactiveProjectWorktrees={inactiveProjectWorktrees}
          emptyWorkspaceIds={emptySidebarWorkspaceIds(projects, activeProject.workspaceId, state, listWorkspaceSnapshots())}
          agents={agents}
          activePath={activeWorktree?.path || ""}
          statuses={worktreeStatuses}
          unreadWorktreePaths={state.unreadWorktreePaths}
          activityByWorktreePath={worktreeActivity}
          {...(worktreeActivityByKey ? { activityByWorktreeKey: worktreeActivityByKey } : {})}
          {...(laneOrder ? { laneOrder } : {})}
          {...(laneLifecycles ? { laneLifecycles } : {})}
          onLaneAction={handleLaneAction}
          onRefreshLaneOrder={refreshLaneLifecycles}
          onSelectProject={handleExplicitSelectProject}
          onReorderProjects={handleReorderProjects}
          onAddProject={handleOpenAddProject}
          onRemoveProject={setPendingProjectRemove}
          onSelectWorktree={handleSelectWorktree}
          onCreateWorktree={handleOpenCreateWorktree}
          onDeleteWorktree={handleDeleteWorktree}
          onResetAgentState={handleResetWorktreeAgentState}
          onShowChanges={handleShowChanges}
          onManageDisk={setDiskManageProject}
          onOpenHistory={setHistoryProject}
          onOpenSettings={handleOpenSettings}
          onOpenResources={() => setIsResourcesOpen(true)}

          onFocusSupervisorSession={handleFocusSupervisorSession}
          footerSlot={
            !activeRemoteHost && setupProgressCtx && !setupProgressDismissed ? (
              <SetupProgressPanel
                items={setupProgressItems}
                onDismiss={dismissSetupProgress}
                onOpenSetup={() => {
                  window.dispatchEvent(new CustomEvent(OPEN_ONBOARDING_EVENT));
                }}
              />
            ) : null
          }
          onToggle={toggleSidebar}
        />
      ) : null}
      {mahoBrowser.drawer}
      {isSidebarOpen && activityMode === "remoteDesktop" ? (
        <RemoteDesktopActivitySidebar activeHostId={remoteDesktopHostId} onSelectHost={setRemoteDesktopHostId} />
      ) : null}

      <main
        data-testid="workspace-surface"
        hidden={!workspaceSurfaceVisible}
        className={cn(
          "h-full min-w-0 flex-1 flex-col overflow-hidden bg-background",
          workspaceSurfaceVisible ? "flex" : "hidden",
        )}
      >
        <InsecureTransportBanner />
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
              details: registrationError.details,
            }}
            onRetry={() => {
              setRegistrationError(null);
              setRegistrationAttempt((attempt) => attempt + 1);
            }}
          />
        ) : null}
        {/* An SSH project with saved tabs keeps its panes on screen instead of the full-pane status,
            and a paired project has no such status at all: both still need a retryable error. */}
        {!activeRemoteHost &&
        registrationError?.workspaceId === activeProject.workspaceId &&
        ((activeProject.target?.kind === "ssh" && !showSshStatus) || activeProject.target?.kind === "pairedDaemon") ? (
          <div
            role="alert"
            data-testid="remote-registration-error"
            className="flex items-start gap-3 border-b border-border bg-card px-4 py-3"
          >
            <div className="min-w-0 flex-1">
              <div className="text-[13px] font-medium text-foreground">
                {activeProject.target?.kind === "ssh" ? "SSH connection failed" : "Paired machine unavailable"}
              </div>
              <div className="mt-0.5 select-text break-words text-[12px] text-muted-foreground">
                {registrationError.code}: {registrationError.message}
              </div>
            </div>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="h-7 shrink-0 text-[11px]"
              onClick={() => {
                setRegistrationError(null);
                setRegistrationAttempt((attempt) => attempt + 1);
              }}
            >
              Retry
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 shrink-0 text-[11px]"
              onClick={openRemoteMachineSettings}
            >
              Open Remote Settings
            </Button>
          </div>
        ) : null}
        {!activeRemoteHost ? <DesktopSupervisorSetupBanner projects={setupProjects} className="mx-3 mt-2" /> : null}
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
            onOpenSettings={openRemoteMachineSettings}
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
            targetHostPlatform={targetHostPlatform}
            sessions={state.sessions}
            unreadTabIds={state.unreadTabIds}
            activityByTabId={tabActivity}
            activityBySessionId={paneActivityBySessionId}
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
            onMoveTabToWindow={isNativeRuntime ? handleMoveTabToWindow : undefined}
            listOtherWindows={isNativeRuntime ? handleListOtherWindows : undefined}
            onAddTab={handleAddTerminalTab}
            onAddBrowserTab={handleAddBrowserTab}
            onOpenSettings={handleOpenSettings}
            agents={launchableAgents}
            onLaunchAgent={handleLaunchAgent}
            onNewChat={chatViewAvailable ? handleNewChat : undefined}
            onSplitChat={chatViewAvailable ? handleSplitChat : undefined}
            onSetPaneView={handleSetPaneView}
            chatViewAvailable={chatViewAvailable}
            renderChatPane={(args) => <LocalChatPane {...args} />}
            defaultAgentId={agentSettings.defaultAgentId}
            onNavigateBrowserTab={handleNavigateBrowserTab}
            onReloadBrowserTab={handleReloadBrowserTab}
            onDuplicateBrowserTab={handleDuplicateBrowserTab}
            onSplitPane={handleSplitPane}
            onClosePane={handleClosePane}
            onDetachBrowserPane={handleDetachBrowserPane}
            onOpenBrowserFullscreen={handleOpenBrowserFullscreen}
            onCloseBrowserSession={handleCloseBrowserSession}
            onSetRatio={setPaneRatio}
            onSetGroupRatio={setTabGroupRatio}
            onEqualizePaneRun={equalizePaneRun}
            onEqualizeGroupRun={equalizeTabGroupRun}
            onSwapPanes={swapPanes}
            onFocusPane={handleContinuityFocusPane}
            searchLeafId={searchLeafId}
            onCloseSearch={handleCloseSearch}
            onReconnectAgentSession={async (sessionId) => {
              await handleReconnectAgentSession(sessionId);
            }}
            onReconnectSshSession={handleReconnectSshSession}
            onReconnectPairedSession={handleReconnectPairedSession}
            onReconnectLocalSession={handleReconnectLocalSession}
            onRefreshSessionIdentity={handleRefreshSessionIdentity}
            onOpenNewShell={handleOpenNewShell}
            onRestartPane={handleRestartPane}
            onBackendSessionUnavailable={(sessionId, backendSessionId, reason, bindingKey) => {
              markBackendSessionUnavailable(sessionId, backendSessionId, reason, bindingKey);
            }}
            leadingSpacer={0}
          />
        ) : (
          <div className="flex h-full flex-1 items-center justify-center bg-background text-xs text-muted-foreground">
            {runtimeError ? `Workspace unavailable (${runtimeError.code})` : "No workspace available"}
          </div>
        )}
      </main>
      {/* The Transient Mini Pane's slot outside Browser mode: Shift+click on a terminal link peeks
          beside the terminal, in the same right split a worktree browser pane uses. Browser mode
          renders the mini inside its own canvas overlay instead, so one `browserId` is only ever
          on one surface. */}
      <MahoBrowserMiniPane visible={workspaceSurfaceVisible} />

      {mahoBrowser.canvas}
      {/* Mounted in every mode: switching to a worktree or the browser only hides the canvas, so a
          live remote desktop session never disconnects (GAP-7). */}
      <GlobalRemoteDesktopCanvas
        active={activityMode === "remoteDesktop"}
        activeHostId={remoteDesktopHostId}
        onSelectHost={setRemoteDesktopHostId}
      />

      {isCommandPaletteOpen && !activeRemoteHost ? (
        <CommandPalette
          open={true}
          worktrees={state.worktrees}
          tabs={state.layout.tabs}
          onSelectWorktree={handleSelectWorktree}
          onSelectTab={handleSelectTerminalTab}
          onShowChanges={activeWorktreeCanShowChanges ? handleShowActiveWorktreeChanges : undefined}
          onNewWindow={isNativeRuntime ? handleNewWindow : undefined}
          appearance={{ ...paletteAppearance, onOpen: () => handleOpenSettings("appearance") }}
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
            onOpenSshProject={handleOpenSshProject} onOpenMachineProject={handleOpenMachineProject}
            onOpenRemoteDesktop={handleOpenRemoteDesktop}
            remoteContext={settingsRemoteContext} />
        </Suspense>
      ) : null}
      {isResourcesOpen ? (
        <Suspense fallback={null}>
          <SystemResourcesDialog onClose={() => setIsResourcesOpen(false)} />
        </Suspense>
      ) : null}
      <RemoteDesktopConsentDialog />
      {!activeRemoteHost && onboardingSteps && onboardingSteps.length > 0 ? (
        <Suspense fallback={null}>
          <WelcomeWizard
            steps={onboardingSteps}
            initialStepIndex={onboardingInitialStepIndex}
            doneSteps={onboardingDoneSteps}
            agents={resolvedAgents}
            isMac={isMacShortcutPlatform()}
            onStepCompleted={(step) => {
              setOnboardingRecordedSteps(markOnboardingStepsCompleted([step]).completedSteps);
            }}
            onFinish={() => {
              setOnboardingAddedProject(null);
              setOnboardingSteps(null);
            }}
            onSkip={() => {
              dismissOnboarding();
              setOnboardingAddedProject(null);
              setOnboardingSteps(null);
            }}
            onRemindLater={() => {
              setOnboardingAddedProject(null);
              setOnboardingSteps(null);
            }}
            tryProject={onboardingTryProject}
            supervisorProjects={setupProjects}
            defaultAgentId={agentSettings.defaultAgentId}
            onTryWorktreeCreated={(worktree, launch) =>
              onboardingTryProject ? handleWorktreeCreated(onboardingTryProject, worktree, launch) : undefined}
            onAddProject={() => {
              onboardingResumeRef.current = { steps: onboardingSteps };
              setOnboardingSteps(null);
              handleOpenAddProject();
            }}
          />
        </Suspense>
      ) : null}
      {restartPaneRequest ? (
        <RestartPaneDialog
          agentName={restartPaneRequest.agentName}
          canResume={restartPaneRequest.canResume}
          onCancel={() => setRestartPaneRequest(null)}
          onResume={() => {
            const target = restartPaneRequest;
            setRestartPaneRequest(null);
            void runPaneRestart(target.sessionId, target.resumeStartup ?? undefined).catch(reportRuntimeError);
          }}
          onFreshShell={() => {
            const target = restartPaneRequest;
            setRestartPaneRequest(null);
            void runPaneRestart(target.sessionId, undefined, true).catch(reportRuntimeError);
          }}
        />
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
      {externalWorktreePrompt.prompt && !activeRemoteHost ? (
        <ExternalWorktreeImportDialog
          key={externalWorktreePrompt.prompt.workspaceId}
          workspaceId={externalWorktreePrompt.prompt.workspaceId}
          worktrees={externalWorktreePrompt.prompt.worktrees}
          projects={projects}
          onImported={handleImportedExternalWorktrees}
          onLater={externalWorktreePrompt.later}
          onClose={externalWorktreePrompt.close}
        />
      ) : null}
      {isAddProjectOpen ? (
        <AddProjectDialog
          projects={projects}
          initialHostId={addProjectHostId}
          initialTarget={addProjectTarget}
          onClose={handleCloseAddProject}
          onRegistered={(project) => {
            handleRegisteredProject(project);
            const resume = onboardingResumeRef.current;
            onboardingResumeRef.current = null;
            if (!resume || project.target?.kind === "ssh" || project.target?.kind === "pairedDaemon") return;
            setOnboardingAddedProject(project);
            setOnboardingInitialStepIndex(Math.max(0, resume.steps.indexOf("try")));
            setOnboardingDoneSteps((done) => (done.includes("project") ? done : [...done, "project"]));
            setOnboardingSteps(resume.steps);
          }}
          onOpenSettings={handleOpenSshSettings}
        />
      ) : null}
      {isCreateOpen ? (
        <AddWorktreeDialog
          project={createTargetProject ?? activeProject}
          onClose={handleCloseCreateWorktree}
          agents={launchableAgents}
          defaultAgentId={agentSettings.defaultAgentId}
          onCreated={(worktree, launch) => handleWorktreeCreated(createTargetProject ?? activeProject, worktree, launch)}
        />
      ) : null}
      {deleteTarget ? (
        <WorktreeDeleteDialog
          workspaceId={deleteOwnerId ?? activeProject.workspaceId}
          project={deleteOwnerProject}
          worktree={deleteTarget}
          onClose={handleCloseDeleteTarget}
          onDeleted={() => {
            // The deletion is confirmed and done: only now drop this worktree's split references.
            // The browser tabs themselves stay in the global store, so Browser mode keeps them.
            const deletedWorkspaceId = deleteTarget ? worktreeIdentity(deleteTarget)?.wsId ?? deleteTarget.workspaceId : undefined;
            if (deletedWorkspaceId) handleWorktreeDeletedForBrowsers(deletedWorkspaceId, browserStore);
            setWorktreeStatuses((current) => {
              const next = { ...current };
              delete next[deleteTarget.path];
              return next;
            });
            if (activeWorktree && isSameWorktreePath(activeWorktree.path, deleteTarget.path)) {
              const remaining = state.worktrees.filter((w) => !isSameWorktreePath(w.path, deleteTarget.path));
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
      {pendingClose ? (
        <ConfirmCloseTabDialog
          tabLabel={pendingClose.label}
          kind={pendingClose.kind}
          tabCount={pendingClose.tabCount}
          liveSessionCount={pendingClose.liveSessionCount}
          activeAgentCount={pendingClose.runningAgentCount}
          onCancel={handleCancelTabClose}
          onConfirm={handleConfirmTabClose}
        />
      ) : null}
      {diskManageProject ? (
        <WorktreeDiskDialog
          workspaceId={diskManageProject.workspaceId}
          projectName={diskManageProject.workspaceId}
          repoRoot={diskManageProject.repoRoot}
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
              const grid = await measureInitialGeometry({
                kind: "newTab",
                groupId: stateRef.current.layout.focusedGroupId ?? null,
              }).catch(() => null);
              const spawned = await spawnTerminalDetailed({
                workspaceId: project.workspaceId,
                worktree: targetWorktree ? worktreeIdentity(targetWorktree) : null,
                cwd: entry.cwd || targetWorktree?.path || project.repoRoot,
                ...initialGridFields(grid),
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
              const label = agentHistoryProviderLabel(entry.provider);
              const tabId = targetWorktree
                ? await openTab(targetWorktree, label, {
                    sessionId: spawned.sessionId,
                    daemonEpoch: spawned.daemonEpoch ?? null,
                    incarnation: spawned.session?.incarnation ?? null,
                  })
                : null;
              // openTab does not own a pre-spawned backend, so a discarded tab would orphan the
              // PTY; a missing worktree would leave the resumed agent running with nothing on screen.
              if (tabId === null) {
                await closeTerminal(spawned.sessionId);
                toast.warning("The agent could not be resumed into a tab, so its session was closed.");
                return;
              }
              setHistoryProject(null);
            } catch (error) {
              reportRuntimeError(error);
            }
          }}
        />
      ) : null}
      {/* Browser mode's overlay surfaces (Command Bar, Ctrl+Tab switcher). They render nothing
          while closed, and their `role="dialog"` is what occludes the native page underneath. */}
      {mahoBrowser.overlays}
    </div>
  );
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

function loadActiveProjectId() {
  try {
    return getMigratedItem(ACTIVE_PROJECT_STORAGE_KEY) || DEFAULT_WORKSPACE_ID;
  } catch {
    return DEFAULT_WORKSPACE_ID;
  }
}

/** A secondary window's own active project selection (namespaced key); null for the primary. */
function loadWindowActiveProjectId(): string | null {
  const key = windowScopedStorageKey(ACTIVE_PROJECT_STORAGE_KEY);
  if (key === ACTIVE_PROJECT_STORAGE_KEY) return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
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

export default App;
