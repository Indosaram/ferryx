import { JSDOM } from "jsdom";

if (typeof window === "undefined") {
  const dom = new JSDOM("<!DOCTYPE html><html><body></body></html>", { url: "http://localhost:3000" });
  globalThis.window = dom.window as unknown as Window & typeof globalThis;
  globalThis.document = dom.window.document;
  globalThis.navigator = dom.window.navigator;
  globalThis.localStorage = dom.window.localStorage;
  globalThis.sessionStorage = dom.window.sessionStorage;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Element = dom.window.Element;
  globalThis.Node = dom.window.Node;
  globalThis.Event = dom.window.Event;
  globalThis.CustomEvent = dom.window.CustomEvent;
  globalThis.MouseEvent = dom.window.MouseEvent;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.dispatchEvent = dom.window.dispatchEvent.bind(dom.window);
  globalThis.addEventListener = dom.window.addEventListener.bind(dom.window);
  globalThis.removeEventListener = dom.window.removeEventListener.bind(dom.window);
}

const { act, cleanup, fireEvent, render, renderHook } = await import("@testing-library/react");
const { afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("./test/setup");

const { saveNotificationSettings } = await import("./lib/notificationSettings");
const { isMacShortcutPlatform } = await import("./lib/shortcuts");

let nativeFocusChanged: ((event: { payload: boolean }) => void) | null = null;
let resolveNativeFocusTrackingReady: (() => void) | null = null;
const nativeFocusTrackingReady = new Promise<void>((resolve) => {
  resolveNativeFocusTrackingReady = resolve;
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFocused: async () => false,
    onFocusChanged: async (callback: (event: { payload: boolean }) => void) => {
      nativeFocusChanged = callback;
      resolveNativeFocusTrackingReady?.();
      return () => {};
    },
  }),
}));

let notificationActivationListener: (() => void) | null = null;
let activationQueue: Array<{ workspaceId: string; sessionId: string }> = [];

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => { resolve = resolvePromise; });
  return { promise, resolve };
}

const grantedPermission = {
  status: "granted" as const, granted: true, canRequest: false,
  canOpenSettings: true, description: "",
};

// The Supervisor panel's snapshot and event feed, which the collapsed sidebar's badge counts.
const panel = {
  load: vi.fn<() => Promise<import("./lib/supervisorPanel/contract").PanelSnapshot>>(),
  subscribe: vi.fn<(handler: (event: import("./lib/supervisorPanel/contract").PanelEvent) => void) => Promise<() => void>>(),
  handlers: [] as Array<(event: import("./lib/supervisorPanel/contract").PanelEvent) => void>,
};

const native = {
  createWorktree: vi.fn(),
  getWorktreeStatus: vi.fn(),
  getInitialProject: vi.fn().mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }),
  listWorktrees: vi.fn().mockResolvedValue([]),
  registerProject: vi.fn(),
  signalTerminal: vi.fn(),
  resetAgentState: vi.fn<(sessionId: string) => Promise<void>>(),
  saveSession: vi.fn().mockResolvedValue(undefined),
  loadSession: vi.fn().mockResolvedValue(null),
  clearSession: vi.fn().mockResolvedValue(undefined),
  listTerminalSessions: vi.fn().mockResolvedValue([]),
  spawnTerminal: vi.fn().mockResolvedValue("mock-spawn-id"),
  detectAgents: vi.fn().mockResolvedValue([]),
  writeTerminal: vi.fn().mockResolvedValue(undefined),
  isTauriRuntime: vi.fn(() => true),
  onNewTerminalTabMenu: vi.fn().mockResolvedValue(() => {}),
  onCloseTabMenu: vi.fn().mockResolvedValue(() => {}),
  onSelectTabMenu: vi.fn().mockResolvedValue(() => {}),
  onNextTabMenu: vi.fn().mockResolvedValue(() => {}),
  onPrevTabMenu: vi.fn().mockResolvedValue(() => {}),
  onSplitRightMenu: vi.fn().mockResolvedValue(() => {}),
  onSplitDownMenu: vi.fn().mockResolvedValue(() => {}),
  onCommandPaletteMenu: vi.fn().mockResolvedValue(() => {}),
  onToggleSidebarMenu: vi.fn().mockResolvedValue(() => {}),
  onOpenSettingsMenu: vi.fn().mockResolvedValue(() => {}),
  onSelectWorktreeMenu: vi.fn().mockResolvedValue(() => {}),
  onTerminalLifecycle: vi.fn().mockResolvedValue(() => {}),
  onTerminalOutput: vi.fn().mockResolvedValue(() => {}),
  publishFocusedTerminal: vi.fn().mockResolvedValue(undefined),
  setBadgeCount: vi.fn().mockResolvedValue({ supported: true, count: 0 }),
  onRemoteSelectionRequested: vi.fn().mockResolvedValue(() => {}),
  onWorktreeChanged: vi.fn().mockResolvedValue(() => {}),
  toIpcError: (error: unknown) => error,
  isStructuredIpcError: (_error: unknown) => false,
  dispatchNotification: vi.fn().mockResolvedValue(undefined),
  playNotificationSound: vi.fn().mockResolvedValue(undefined),
  onNotificationActivated: vi.fn(async (cb: () => void) => {
    notificationActivationListener = cb;
    return () => {
      notificationActivationListener = null;
    };
  }),
  takeNotificationActivations: vi.fn(async () => {
    const drained = activationQueue;
    activationQueue = [];
    return drained;
  }),
};


vi.mock("./lib/tauri", () => ({
  listExternalWorktrees: vi.fn(async () => []),
  discardWindowSession: vi.fn(async () => undefined),
  onWindowSessionsOrphaned: vi.fn(async () => () => undefined),
  onWindowSessionsAdopt: vi.fn(async () => () => undefined),
  takePendingWindowAdoptions: vi.fn(async () => []),
  moveSessionsToWindow: vi.fn(async () => ({ targetLabel: "ws-new", moved: [], skipped: [], queued: false })),
  listAppWindows: vi.fn(async () => []),
  openNewWindow: vi.fn(async () => undefined),
  listenDagRunUpdated: vi.fn(() => Promise.resolve(() => undefined)),
  listenDagWatchStatus: vi.fn(async () => () => undefined),
  // The notification coordinator re-consults the authoritative OS permission status before it
  // trusts an earlier denial, so the mock has to answer it or every dispatch rejects.
  getNotificationPermissionStatus: vi.fn(async () => ({ authorization: "authorized" })),
  requestNotificationPermission: vi.fn(async () => ({ granted: true })),
  watchDagProject: vi.fn((projectPath: string) => Promise.resolve({ projectPath, runs: [] })),
  watchDagPairedProject: vi.fn((workspaceId: string, remotePath: string) =>
    Promise.resolve({ projectPath: `paired:${workspaceId}:${remotePath}`, runs: [] }),
  ),
  watchDagSshProject: vi.fn((workspaceId: string, remotePath: string) =>
    Promise.resolve({ projectPath: `ssh:${workspaceId}:${remotePath}`, runs: [] }),
  ),
  unwatchDagProject: vi.fn(() => Promise.resolve()),
  discoverDagWatchRoots: vi.fn(() => Promise.resolve([])),
  DEFAULT_WORKSPACE_ID: "default",
  DEFAULT_TERMINAL_FONT_STACK: "monospace",
  getTerminalPreferences: () => Promise.resolve({}),
  createWorktree: native.createWorktree,
  getWorktreeStatus: native.getWorktreeStatus,
  previewWorktreeDelete: vi.fn(),
  deleteWorktree: vi.fn(),
  deleteWorktreeDestructive: vi.fn(),
  getInitialProject: native.getInitialProject,
  getSystemPermissionsStatus: vi.fn(async () => ({
    platform: "macos", allGranted: true,
    fullDiskAccess: grantedPermission,
    accessibility: grantedPermission,
    notifications: grantedPermission,
  } satisfies import("./lib/types").SystemPermissionsStatus)),
  listWorktrees: native.listWorktrees,
  registerProject: native.registerProject,
  signalTerminal: native.signalTerminal,
  resetAgentState: native.resetAgentState,
  saveSession: native.saveSession,
  loadSession: native.loadSession,
  clearSession: native.clearSession,
  listTerminalSessions: native.listTerminalSessions,
  getHostBootTimeMs: vi.fn(async () => null),
  spawnTerminal: native.spawnTerminal,
  closeTerminal: vi.fn(),
  getTerminalCwd: vi.fn(),
  resizeTerminal: vi.fn(),
  waitForTerminalExit: vi.fn(),
  detectAgents: native.detectAgents,
  writeTerminal: native.writeTerminal,
  isTauriRuntime: native.isTauriRuntime,
  onNewTerminalTabMenu: native.onNewTerminalTabMenu,
  onCloseTabMenu: native.onCloseTabMenu,
  onSelectTabMenu: native.onSelectTabMenu,
  onNextTabMenu: native.onNextTabMenu,
  onPrevTabMenu: native.onPrevTabMenu,
  onSplitRightMenu: native.onSplitRightMenu,
  onSplitDownMenu: native.onSplitDownMenu,
  onCommandPaletteMenu: native.onCommandPaletteMenu,
  onToggleSidebarMenu: native.onToggleSidebarMenu,
  onOpenSettingsMenu: native.onOpenSettingsMenu,
  onSelectWorktreeMenu: native.onSelectWorktreeMenu,
  onTerminalLifecycle: native.onTerminalLifecycle,
  onNativeTerminalTitle: async () => () => {},
  onNativeTerminalBell: async () => () => {},
  onNativeTerminalAgentState: async () => () => {},
  onNativeTerminalFocus: async () => () => {},
  onTerminalOutput: native.onTerminalOutput,
  publishFocusedTerminal: native.publishFocusedTerminal,
  setBadgeCount: native.setBadgeCount,
  onRemoteSelectionRequested: native.onRemoteSelectionRequested,
  onWorktreeChanged: vi.fn().mockResolvedValue(() => {}),
  bootTrace: vi.fn(async () => undefined),
  setNativeTerminalDomOverlayInput: vi.fn(async () => undefined),
  getCliLauncherStatus: vi.fn(async () => ({
    launcherPath: "~/.local/bin/ferryx",
    isInstalled: false,
    isSymlink: false,
    currentTarget: null,
    activeExecutable: null,
    isSupported: false,
  })),
  installCliLauncher: vi.fn(async () => ({
    launcherPath: "~/.local/bin/ferryx",
    isInstalled: true,
    isSymlink: true,
    currentTarget: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
    activeExecutable: "/Applications/Ferryx.app/Contents/MacOS/ferryx",
    isSupported: true,
  })),
  toIpcError: (error: unknown) => error,
  isStructuredIpcError: (_error: unknown) => false,
  dispatchNotification: native.dispatchNotification,
  playNotificationSound: native.playNotificationSound,
  onNotificationActivated: native.onNotificationActivated,
  takeNotificationActivations: native.takeNotificationActivations,
  getSupervisorPanel: panel.load,
  onSupervisorPanelEvent: panel.subscribe,
}));

const workspaceStoreModule = await import("./state/workspaceStore");
const realUseWorkspaceStore = workspaceStoreModule.useWorkspaceStore;
const { clearHmrWorkspaceState } = await import("./state/hmrWorkspaceState");
const { toast } = await import("sonner");
type ActivityNotificationTarget = import("./state/workspaceStore").ActivityNotificationTarget;
type ActivityNotificationEvent = import("./state/workspaceStore").ActivityNotificationEvent;
const activityListeners = new Set<(event: ActivityNotificationEvent) => void>();
function emitActivityTargets(): void {
  for (const target of currentActivityTargets) {
    for (const listener of activityListeners) listener({ ...target, previousState: "working" });
  }
}

const markTabUnread = vi.fn();
const markWorktreeUnread = vi.fn();
const markSessionActivitySeen = vi.fn();
const dispatchWorkspaceAction = vi.fn();
// The real store memoizes this callback. Recreating it per render would restart
// restoration whenever the subscribed restore status changes.
const restoreWorkspace = vi.fn();
/**
 * The bell reaches App through the store's global native subscription, so the test fires it the
 * same way the store does rather than through a pane prop that only the foreground tab would have.
 */
const bellListeners = new Set<(sessionId: string, tabId: string, target?: ActivityNotificationTarget) => void>();
function emitTerminalBell(sessionId = "sess-1", tabId = "tab-1", target?: ActivityNotificationTarget): void {
  for (const listener of bellListeners) listener(sessionId, tabId, target);
}

const storeState = {
  activeWorktreePath: "/repo/main",
  layout: {
    activeTabId: "tab-1",
    layoutsByTabId: {
      "tab-1": {
        root: { type: "leaf" as const, leafId: "leaf-1" },
        activeLeafId: "leaf-1",
        expandedLeafId: null,
        sessionIdsByLeafId: { "leaf-1": "sess-1" },
      },
    },
    tabs: [
      { id: "tab-1", label: "main", sessionId: "sess-1" },
      { id: "tab-2", label: "feature", sessionId: "sess-2" },
    ],
  },
  sessions: {
    "sess-1": { id: "sess-1", cwd: "/repo/main", worktreePath: "/repo/main", backendSessionId: "backend-sess-1" },
    "sess-2": { id: "sess-2", cwd: "/repo/feature", worktreePath: "/repo/feature" },
  },
  worktrees: [
    { path: "/repo/main", branch: "refs/heads/main" },
    { path: "/repo/feature", branch: "refs/heads/feature" },
  ],
  unreadTabIds: {} as Record<string, boolean>,
  unreadWorktreePaths: {} as Record<string, boolean>,
};

let currentActivityTargets: ActivityNotificationTarget[] = [];
let storeSpy: any;

const runtime = {
  refreshWorktrees: vi.fn(),
  reportRuntimeError: vi.fn(),
  runtimeError: null,
};

vi.mock("./state/workspaceRuntime", () => ({
  useWorkspaceRuntime: () => runtime,
}));

let sidebarProps: {
  onResetAgentState?: (worktree: import("./lib/types").Worktree) => Promise<void>;
  onFocusSupervisorSession?: (sessionRef: string) => void;
  attention?: unknown;
} = {};
vi.mock("./components/Sidebar", () => ({
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY: "ferryx.sidebar.collapsedProjects",
  Sidebar: (props: typeof sidebarProps) => {
    sidebarProps = props;
    return <div data-testid="mock-sidebar" />;
  },
}));

vi.mock("./components/CommandPalette", () => ({
  CommandPalette: () => null,
}));

vi.mock("./components/SettingsDialog", () => ({
  SettingsDialog: () => null,
}));

vi.mock("./components/WorktreeDeleteDialog", () => ({
  WorktreeDeleteDialog: () => null,
}));

let terminalProps: { onResetAgentState?: (tabId: string) => Promise<void> } = {};
vi.mock("./components/TerminalSplitView", () => ({
  TerminalSplitView: (props: typeof terminalProps) => {
    terminalProps = props;
    return <div data-testid="mock-terminal-split-view" />;
  },
}));

const flashedSessions = vi.hoisted(() => [] as string[]);
const paneFlashSessions = () => [...flashedSessions];
vi.mock("./lib/paneFlash", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./lib/paneFlash")>()),
  flashPane: (sessionId: string) => { flashedSessions.push(sessionId); },
}));

const { emptyPanelSnapshot } = await import("./lib/supervisorPanel/emptySnapshot");
const { panelSnapshotFixture } = await import("./lib/supervisorPanel/fixtures");
const { App } = await import("./App");
const { getWorkspaceRestoreStatus, resetWorkspaceRestore } = await import("./state/workspaceRestore");
const { getWorkspaceSnapshot, setWorkspaceSnapshot, clearWorkspaceSnapshot } = await import("./state/workspaceSnapshotCache");
const { PROJECTS_STORAGE_KEY, ACTIVE_PROJECT_STORAGE_KEY } = await import("./lib/storageKeys");

function parkedProjectSnapshot(sessionId: string | null): import("./state/workspaceStore").WorkspaceState {
  const sessions = sessionId
    ? {
        [sessionId]: {
          id: sessionId,
          cwd: "/repo/other",
          worktreePath: "/repo/other",
          workspaceId: "other",
          worktree: null,
          backendSessionId: "backend-other",
          lifecycle: "working" as const,
        },
      }
    : {};
  return {
    workspaceId: "other",
    worktrees: [
      { path: "/repo/other", head: "", branch: "refs/heads/main", bare: false, detached: false, locked: null, prunable: null },
    ],
    activeWorktreePath: "/repo/other",
    sessions,
    layout: {
      tabs: sessionId ? [{ id: "tab-r", label: "r", sessionId }] : [],
      activeTabId: sessionId ? "tab-r" : null,
      layoutsByTabId: sessionId
        ? {
            "tab-r": {
              root: { type: "leaf" as const, leafId: "leaf-r" },
              activeLeafId: "leaf-r",
              expandedLeafId: null,
              sessionIdsByLeafId: { "leaf-r": sessionId },
            },
          }
        : {},
    },
    worktreeLayouts: {},
    unreadTabIds: {},
    unreadWorktreePaths: {},
    activityBySessionId: {},
  };
}

function seedTwoProjects() {
  localStorage.setItem(
    PROJECTS_STORAGE_KEY,
    JSON.stringify([
      { workspaceId: "default", repoRoot: ".", gitRoot: null },
      { workspaceId: "other", repoRoot: "/repo/other" },
    ]),
  );
  localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "default");
}

describe("App notification coordinator wiring", () => {
  beforeEach(() => {
    flashedSessions.length = 0;
    resetWorkspaceRestore();
    localStorage.clear();
    panel.handlers.length = 0;
    panel.load.mockReset();
    panel.load.mockResolvedValue(emptyPanelSnapshot());
    panel.subscribe.mockReset();
    panel.subscribe.mockImplementation(async (handler) => {
      panel.handlers.push(handler);
      return () => undefined;
    });
    sidebarProps = {};
    saveNotificationSettings({ enabled: true, terminalBell: true, agentTaskComplete: true });
    native.isTauriRuntime.mockReset();
    native.isTauriRuntime.mockReturnValue(false);
    native.getInitialProject.mockReset();
    native.getInitialProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
    native.registerProject.mockReset();
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listWorktrees.mockReset();
    native.listWorktrees.mockResolvedValue([]);
    native.loadSession.mockReset();
    native.loadSession.mockResolvedValue(null);
    native.listTerminalSessions.mockReset();
    native.listTerminalSessions.mockResolvedValue([]);
    native.setBadgeCount.mockReset();
    native.setBadgeCount.mockResolvedValue({ supported: true, count: 0 });
    native.onNewTerminalTabMenu.mockReset();
    native.onNewTerminalTabMenu.mockResolvedValue(() => {});
    native.onCloseTabMenu.mockReset();
    native.onCloseTabMenu.mockResolvedValue(() => {});
    native.onSelectTabMenu.mockReset();
    native.onSelectTabMenu.mockResolvedValue(() => {});
    native.onNextTabMenu.mockReset();
    native.onNextTabMenu.mockResolvedValue(() => {});
    native.onPrevTabMenu.mockReset();
    native.onPrevTabMenu.mockResolvedValue(() => {});
    native.onSplitRightMenu.mockReset();
    native.onSplitRightMenu.mockResolvedValue(() => {});
    native.onSplitDownMenu.mockReset();
    native.onSplitDownMenu.mockResolvedValue(() => {});
    native.onCommandPaletteMenu.mockReset();
    native.onCommandPaletteMenu.mockResolvedValue(() => {});
    native.onToggleSidebarMenu.mockReset();
    native.onToggleSidebarMenu.mockResolvedValue(() => {});
    native.onOpenSettingsMenu.mockReset();
    native.onOpenSettingsMenu.mockResolvedValue(() => {});
    native.onSelectWorktreeMenu.mockReset();
    native.onSelectWorktreeMenu.mockResolvedValue(() => {});
    native.onTerminalLifecycle.mockReset();
    native.onTerminalLifecycle.mockResolvedValue(() => {});
    native.onTerminalOutput.mockReset();
    native.onTerminalOutput.mockResolvedValue(() => {});
    native.onRemoteSelectionRequested.mockReset();
    native.onRemoteSelectionRequested.mockResolvedValue(() => {});
    native.dispatchNotification.mockReset();
    native.playNotificationSound.mockReset();
    markTabUnread.mockReset();
    markWorktreeUnread.mockReset();
    markSessionActivitySeen.mockReset();
    dispatchWorkspaceAction.mockReset();
    restoreWorkspace.mockReset();
    native.onNotificationActivated.mockClear();
    native.takeNotificationActivations.mockClear();
    notificationActivationListener = null;
    activationQueue = [];
    bellListeners.clear();
    activityListeners.clear();
    currentActivityTargets = [];

    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");

    storeSpy = vi.spyOn(workspaceStoreModule, "useWorkspaceStore").mockImplementation((options) => ({
      state: getWorkspaceSnapshot(options?.workspaceId ?? "default") ?? { ...storeState, workspaceId: options?.workspaceId ?? "default" },
      recoveredFromHmr: false,
      agents: [],
      tabActivity: {},
      worktreeActivity: {},
      activityNotificationTargets: currentActivityTargets,
      subscribeActivityNotification: (listener: (event: ActivityNotificationEvent) => void) => {
        activityListeners.add(listener);
        return () => { activityListeners.delete(listener); };
      },
      markTabUnread,
      markWorktreeUnread,
      markSessionActivitySeen,
      clearTabUnread: vi.fn(),
      clearWorktreeUnread: vi.fn(),
      activateTab: vi.fn(),
      closeTab: vi.fn(),
      closeOtherTabs: vi.fn(),
      closeTabsToRight: vi.fn(),
      closeTabsToLeft: vi.fn(),
      splitPane: vi.fn(),
      moveTabToGroup: vi.fn(),
      moveTabToSplit: vi.fn(),
      detachPaneToTab: vi.fn(),
      closePane: vi.fn(),
      reorderTab: vi.fn(),
      renameTab: vi.fn(),
      setTabPinned: vi.fn(),
      focusPane: vi.fn(),
      setPaneRatio: vi.fn(),
      setTabGroupRatio: vi.fn(),
      swapPanes: vi.fn(),
      ensureTabForWorktree: vi.fn().mockResolvedValue(undefined),
      ensureSessionBackends: vi.fn().mockResolvedValue(undefined),
      openTab: vi.fn(),
      createBrowserTab: vi.fn().mockResolvedValue("browser-tab-1"),
      navigateBrowserTab: vi.fn().mockResolvedValue(undefined),
      reloadBrowserTab: vi.fn().mockResolvedValue(undefined),
      openWorkspacePortInBrowser: vi.fn().mockResolvedValue("browser-tab-1"),
      syncWorktrees: vi.fn(),
      restoreWorkspace,
      updateSessionTitleActivity: vi.fn(),
      subscribeTerminalBell: (listener: (sessionId: string, tabId: string, target?: ActivityNotificationTarget) => void) => {
        bellListeners.add(listener);
        return () => {
          bellListeners.delete(listener);
        };
      },
      dispatchWorkspaceAction,
    } as any));
  });

  afterEach(() => {
    cleanup();
    clearHmrWorkspaceState();
    clearWorkspaceSnapshot();
    storeSpy?.mockRestore();
  });

  function resetFixture(count: number) {
    clearHmrWorkspaceState();
    native.resetAgentState.mockReset();
    const snapshot = parkedProjectSnapshot("sess-1");
    snapshot.workspaceId = "default";
    snapshot.sessions = Object.fromEntries(Array.from({ length: count }, (_, index) => {
      const id = `sess-${index + 1}`;
      return [id, { ...snapshot.sessions["sess-1"], id, workspaceId: "default", backendSessionId: `backend-${index + 1}` }];
    }));
    const paneLayout = snapshot.layout.layoutsByTabId["tab-r"];
    paneLayout.sessionIdsByLeafId = Object.fromEntries(
      Object.keys(snapshot.sessions).map((id, index) => [`leaf-${index}`, id]),
    );
    paneLayout.activeLeafId = "leaf-0";
    paneLayout.root = { type: "leaf", leafId: "leaf-0" };
    for (let index = 1; index < count; index++) {
      paneLayout.root = {
        type: "split", direction: "horizontal", ratio: 0.5,
        first: paneLayout.root, second: { type: "leaf", leafId: `leaf-${index}` },
      };
    }
    snapshot.activityBySessionId = Object.fromEntries(Object.keys(snapshot.sessions).map((id) => [
      id, { state: "waiting", title: "Claude", isAgent: true, seen: false },
    ]));
    const hook = renderHook(() => realUseWorkspaceStore({
      workspaceId: "default",
      services: {
        ensureTerminalEvents: async () => {},
        spawnTerminal: vi.fn(), getTerminalCwd: vi.fn(), closeTerminal: vi.fn(), waitForTerminalExit: vi.fn(),
      },
    }));
    act(() => hook.result.current.restoreWorkspace(snapshot));
    return hook;
  }

  it.each([false, true])("manual reset outcome store rejection propagates (grouped=%s)", async (grouped) => {
    const hook = resetFixture(grouped ? 3 : 1);
    const failure = { code: "IPC_UNAVAILABLE", message: "offline" };
    let reject!: (reason: unknown) => void;
    const pending = new Promise<void>((_resolve, fail) => { reject = fail; });
    native.resetAgentState.mockImplementation((id) => id === "backend-1" ? pending : Promise.resolve());
    let outcome!: Promise<PromiseSettledResult<void>[]>;
    act(() => {
      outcome = Promise.allSettled([grouped
        ? hook.result.current.resetWorktreeAgentState("/repo/other")
        : hook.result.current.resetAgentState("sess-1")]);
    });
    const retainedWhilePending = hook.result.current.state.activityBySessionId?.["sess-1"];
    let settled!: PromiseSettledResult<void>[];
    await act(async () => { reject(failure); settled = await outcome; });
    expect(settled[0].status).toBe("rejected");
    expect(retainedWhilePending?.state).toBe("waiting");
    expect(hook.result.current.state.activityBySessionId?.["sess-1"]?.state).toBe("waiting");
    if (grouped) {
      if (settled[0].status === "rejected") {
        expect(settled[0].reason.results).toEqual([
          { sessionId: "sess-1", status: "rejected", reason: failure },
          { sessionId: "sess-2", status: "fulfilled", value: undefined },
          { sessionId: "sess-3", status: "fulfilled", value: undefined },
        ]);
      }
      expect(native.resetAgentState.mock.calls.map(([id]) => id)).toEqual(["backend-1", "backend-2", "backend-3"]);
      expect(hook.result.current.state.activityBySessionId?.["sess-2"]).toBeUndefined();
      expect(hook.result.current.state.activityBySessionId?.["sess-3"]).toBeUndefined();
    } else if (settled[0].status === "rejected") {
      expect(settled[0].reason).toBe(failure);
    }
  });

  it.each([
    ["tab", 1, 0], ["tab", 1, 1], ["tab", 3, 1], ["tab", 3, 3], ["tab", 3, 0],
    ["worktree", 1, 0], ["worktree", 1, 1], ["worktree", 3, 1], ["worktree", 3, 3], ["worktree", 3, 0],
  ] as const)("manual reset outcome routes %s notifications (%i sessions, %i failures)", async (surface, count, failures) => {
    const hook = resetFixture(count);
    const original = storeSpy.getMockImplementation();
    storeSpy.mockImplementation((options: { workspaceId: string }) => ({
      ...original(options), state: hook.result.current.state,
      resetAgentState: hook.result.current.resetAgentState,
      resetWorktreeAgentState: hook.result.current.resetWorktreeAgentState,
    }));
    let resolve!: () => void;
    const gate = new Promise<void>((done) => { resolve = done; });
    native.resetAgentState.mockImplementation(async (id) => {
      await gate;
      if (Number(id.split("-")[1]) <= failures) throw { code: "IPC_UNAVAILABLE", message: "offline" };
    });
    const success = vi.spyOn(toast, "success").mockReturnValue("success");
    const error = vi.spyOn(toast, "error").mockReturnValue("error");
    await act(async () => { render(<App />); });
    let operation!: Promise<void>;
    act(() => {
      operation = surface === "tab"
        ? terminalProps.onResetAgentState!("tab-r")
        : sidebarProps.onResetAgentState!(hook.result.current.state.worktrees[0]);
    });
    expect(success).not.toHaveBeenCalled();
    expect(error).not.toHaveBeenCalled();
    await act(async () => { resolve(); await operation; });
    expect(success).toHaveBeenCalledTimes(failures === 0 ? 1 : 0);
    expect(error).toHaveBeenCalledTimes(failures === 0 ? 0 : 1);
    expect(native.resetAgentState).toHaveBeenCalledTimes(count);
    for (let index = 1; index <= count; index++) {
      expect(Boolean(hook.result.current.state.activityBySessionId?.[`sess-${index}`])).toBe(index <= failures);
      expect(hook.result.current.state.sessions[`sess-${index}`].backendSessionId).toBe(`backend-${index}`);
    }
  });

  it("CRITERION 2: clicking the bell button while the window is UNFOCUSED calls dispatchNotification with source: 'terminal-bell' and marks tab and worktree unread", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);

    render(<App />);

    emitTerminalBell();

    expect(native.dispatchNotification).toHaveBeenCalledTimes(1);
    expect(native.dispatchNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "terminal-bell",
        workspaceLabel: "default",
      }),
    );
    expect(native.playNotificationSound).toHaveBeenCalledTimes(1);
    expect(markTabUnread).toHaveBeenCalledWith("tab-1", undefined);
    expect(markWorktreeUnread).toHaveBeenCalledWith("/repo/main", undefined);
  });

  it("starts DAG subscriptions when the registered project list is remote-only", async () => {
    const dagBridge = await import("./lib/tauri");
    const pairedWorkspace = `daemon:${"a".repeat(64)}`;
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([
      { workspaceId: "ssh:build:dag", repoRoot: "/srv/dag", target: { kind: "ssh", hostId: "build" } },
      { workspaceId: pairedWorkspace, repoRoot: "/srv/paired-dag", remoteWorkspaceId: "remote-dag", target: { kind: "pairedDaemon", hostId: "host" } },
    ]));
    await act(async () => { render(<App />); });
    expect(dagBridge.watchDagSshProject).toHaveBeenCalledWith("ssh:build:dag", "/srv/dag");
    expect(dagBridge.watchDagPairedProject).toHaveBeenCalledWith(pairedWorkspace, "/srv/paired-dag");
  });

  it.each([
    { repoRoot: "/srv/Orca/", hostLabel: "Build machine", grouped: false, expected: "Orca (Build machine)" },
    { repoRoot: "C:\\work\\Orca\\", hostLabel: "Build machine", grouped: false, expected: "Orca (Build machine)" },
    { repoRoot: "/", hostLabel: undefined, grouped: false, expected: "/ (build)" },
    { repoRoot: "/srv/Orca", hostLabel: "Build machine", grouped: true, expected: "Orca" },
  ])("forwards SSH labels for agent completion and both bell paths: $expected at $repoRoot", async ({ repoRoot, hostLabel, grouped, expected }) => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const workspaceId = "ssh:build:orca";
    const gitRemote = "git@github.com:example/orca.git";
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([
      { workspaceId: "default", repoRoot: ".", gitRoot: null },
      { workspaceId, repoRoot, hostLabel, target: { kind: "ssh", hostId: "build" }, gitRemote },
      ...(grouped ? [{ workspaceId: "local-orca", repoRoot: "/local/orca", gitRemote }] : []),
    ]));
    const snapshot = parkedProjectSnapshot("sess-remote");
    snapshot.workspaceId = workspaceId;
    snapshot.activityBySessionId = {
      "sess-remote": { state: "done", title: "codex run", isAgent: true, agentType: "codex" },
    };
    const [target] = workspaceStoreModule.selectActivityNotificationTargets(snapshot);
    expect(target).toMatchObject({ workspaceId, workspaceLabel: expected });
    currentActivityTargets = [target];

    render(<App />);
    await nativeFocusTrackingReady;
    nativeFocusChanged?.({ payload: false });
    emitActivityTargets();
    expect(native.dispatchNotification).toHaveBeenLastCalledWith(expect.objectContaining({
      source: "agent-task-complete", workspaceLabel: expected,
      target: { workspaceId, sessionId: "sess-remote" },
    }));

    // Explicit event targets belong to the emitting workspace, not the mounted project.
    emitTerminalBell("bell-remote", "tab-r", { ...target, sessionId: "bell-remote" });
    expect(native.dispatchNotification).toHaveBeenLastCalledWith(expect.objectContaining({
      source: "terminal-bell", workspaceLabel: expected,
      target: { workspaceId, sessionId: "bell-remote" },
    }));

    cleanup();
    currentActivityTargets = [];
    setWorkspaceSnapshot("default", snapshot);
    render(<App />);
    emitTerminalBell("sess-remote", "tab-r");
    expect(native.dispatchNotification).toHaveBeenLastCalledWith(expect.objectContaining({
      source: "terminal-bell", workspaceLabel: expected,
    }));
  });

  it("CRITERION 2b: clicking the bell button while the window is FOCUSED calls dispatchNotification ZERO times", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);

    render(<App />);
    await nativeFocusTrackingReady;
    nativeFocusChanged?.({ payload: true });

    emitTerminalBell();

    expect(native.dispatchNotification).not.toHaveBeenCalled();
    expect(native.playNotificationSound).not.toHaveBeenCalled();
    expect(markTabUnread).not.toHaveBeenCalled();
    expect(markWorktreeUnread).not.toHaveBeenCalled();
  });

  it("CRITERION 1: a background agent transitioning working -> done calls dispatchNotification with source: 'agent-task-complete' while unfocused", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "working",
      },
    ];

    const { rerender } = render(<App />);
    await nativeFocusTrackingReady;
    nativeFocusChanged?.({ payload: false });

    expect(native.dispatchNotification).not.toHaveBeenCalled();

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "done",
      },
    ];

    emitActivityTargets();
    rerender(<App />);

    expect(native.dispatchNotification).toHaveBeenCalledTimes(1);
    expect(native.dispatchNotification).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "agent-task-complete",
        worktreeLabel: "main",
        terminalTitle: "codex run",
        agentLabel: "codex",
      }),
    );
    expect(native.playNotificationSound).toHaveBeenCalledTimes(1);
    expect(markTabUnread).toHaveBeenCalledWith("tab-1", undefined);
    expect(markWorktreeUnread).toHaveBeenCalledWith("/repo/main", undefined);
  });

  it("uses native background focus when the DOM incorrectly reports focused", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "working",
      },
    ];

    const { rerender } = render(<App />);
    await nativeFocusTrackingReady;
    nativeFocusChanged?.({ payload: false });

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "done",
      },
    ];

    emitActivityTargets();
    rerender(<App />);

    expect(native.dispatchNotification).toHaveBeenCalledWith(
      expect.objectContaining({ source: "agent-task-complete" }),
    );
    expect(markTabUnread).toHaveBeenCalledWith("tab-1", undefined);
    expect(markWorktreeUnread).toHaveBeenCalledWith("/repo/main", undefined);
  });

  it("CRITERION 1b: a background agent transitioning working -> done while FOCUSED dispatches nothing", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "working",
      },
    ];

    const { rerender } = render(<App />);
    await nativeFocusTrackingReady;
    nativeFocusChanged?.({ payload: true });

    expect(native.dispatchNotification).not.toHaveBeenCalled();

    currentActivityTargets = [
      {
        sessionId: "sess-1",
        tabId: "tab-1",
        worktreePath: "/repo/main",
        worktreeLabel: "main",
        agentLabel: "codex",
        terminalTitle: "codex run",
        state: "done",
      },
    ];

    emitActivityTargets();
    rerender(<App />);

    expect(native.dispatchNotification).not.toHaveBeenCalled();
    expect(native.playNotificationSound).not.toHaveBeenCalled();
    expect(markTabUnread).not.toHaveBeenCalled();
    expect(markWorktreeUnread).not.toHaveBeenCalled();
  });

  describe("supervisor panel navigation and the collapsed badge", () => {
    // A Supervisor panel row names its session `<hostId>:<backendSessionId>`; the desktop focuses
    // the open pane behind it through the same path a notification uses. A session no open pane
    // runs has a backend id nothing resolves, so its row can lead nowhere.
    const openBackendIds: Record<string, string> = { "default/sess-1": "backend-sess-1", "other/sess-remote": "backend-other" };
    const backendOf = (workspaceId: string, sessionId: string) =>
      openBackendIds[`${workspaceId}/${sessionId}`] ?? `backend-gone-${workspaceId}-${sessionId}`;
    const clickRow = async (workspaceId: string, sessionId: string) => {
      await act(async () => { sidebarProps.onFocusSupervisorSession!(`mac:${backendOf(workspaceId, sessionId)}`); });
    };

    it("focuses the pane behind a clicked panel row", async () => {
      await act(async () => { render(<App />); });
      await clickRow("default", "sess-1");
      expect(dispatchWorkspaceAction).toHaveBeenCalledWith({ type: "FOCUS_EXISTING_SESSION", sessionId: "sess-1" });
      expect(paneFlashSessions()).toEqual(["sess-1"]);
    });

    it.each([ ["ghost", "sess-1"], ["default", "closed"], ["other", "closed"] ])(
      "does not navigate when target %s/%s is gone", async (workspaceId, sessionId) => {
        seedTwoProjects();
        setWorkspaceSnapshot("other", parkedProjectSnapshot(null));
        await act(async () => { render(<App />); });
        dispatchWorkspaceAction.mockClear();
        await clickRow(workspaceId, sessionId);
        expect(dispatchWorkspaceAction).not.toHaveBeenCalled();
        expect(paneFlashSessions()).toEqual([]);
      },
    );

    it("focuses a cross-project row after the target workspace mounts", async () => {
      seedTwoProjects();
      setWorkspaceSnapshot("other", parkedProjectSnapshot("sess-remote"));
      await act(async () => { render(<App />); });
      await clickRow("other", "sess-remote");
      expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("other");
      expect(dispatchWorkspaceAction).toHaveBeenCalledWith({ type: "FOCUS_EXISTING_SESSION", sessionId: "sess-remote" });
    });

    it("does not focus a cross-project row that closed while its workspace was mounting", async () => {
      seedTwoProjects();
      setWorkspaceSnapshot("other", parkedProjectSnapshot("sess-remote"));
      await act(async () => { render(<App />); });
      const original = storeSpy.getMockImplementation();
      let changed = false;
      storeSpy.mockImplementation((options: { workspaceId: string }) => {
        if (options.workspaceId === "other" && !changed) {
          changed = true;
          setWorkspaceSnapshot("other", parkedProjectSnapshot(null));
        }
        return original(options);
      });
      dispatchWorkspaceAction.mockClear();
      await clickRow("other", "sess-remote");
      expect(changed).toBe(true);
      expect(dispatchWorkspaceAction).not.toHaveBeenCalled();
    });

    it("mounts the Supervisor panel as the sidebar and offers no Inbox state", async () => {
      const view = render(<App />);
      expect(typeof sidebarProps.onFocusSupervisorSession).toBe("function");
      expect(sidebarProps.attention).toBeUndefined();
      expect(view.queryByTestId("attention-inbox")).toBeNull();
    });

    // Defect 2: non-mac platforms use ctrlKey instead of metaKey
    it("reveals the collapsed sidebar with the notifications shortcut instead of opening an inbox", async () => {
      const isMac = isMacShortcutPlatform();
      const view = render(<App />);
      const key = (code: string, shift: boolean) => act(() => {
        window.dispatchEvent(new KeyboardEvent("keydown", {
          key: code.slice(-1).toLowerCase(), code, metaKey: isMac, ctrlKey: !isMac, shiftKey: shift, bubbles: true, cancelable: true,
        }));
      });
      key("KeyB", false);
      expect(view.queryByTestId("mock-sidebar")).toBeNull();

      key("KeyI", true);
      expect(view.getByTestId("mock-sidebar")).toBeInTheDocument();

      // Already open: the shortcut leaves it open.
      key("KeyI", true);
      expect(view.getByTestId("mock-sidebar")).toBeInTheDocument();
    });

    const collapse = () => act(() => {
      const isMac = isMacShortcutPlatform();
      window.dispatchEvent(new KeyboardEvent("keydown", {
        key: "b", code: "KeyB", metaKey: isMac, ctrlKey: !isMac, bubbles: true, cancelable: true,
      }));
    });

    it("counts the panel's decisions on the collapsed sidebar and reopens the sidebar from it", async () => {
      panel.load.mockResolvedValue(panelSnapshotFixture);
      const view = render(<App />);
      await act(async () => { await Promise.all(panel.load.mock.results.map((result) => result.value)); });
      collapse();
      expect(view.queryByTestId("mock-sidebar")).toBeNull();
      expect(view.getByTestId("collapsed-attention-badge")).toHaveTextContent(String(panelSnapshotFixture.decisions.length));

      fireEvent.click(view.getByRole("button", { name: `Open supervisor (${panelSnapshotFixture.decisions.length})` }));

      expect(view.getByTestId("mock-sidebar")).toBeInTheDocument();
    });

    it("follows panel decision events and ignores agent completions, which are not decisions", async () => {
      currentActivityTargets = [{
        workspaceId: "default", workspaceLabel: "Project", sessionId: "sess-1", tabId: "tab-1",
        worktreePath: "/repo/main", worktreeLabel: "main", agentLabel: "Codex", terminalTitle: "Task", state: "done",
      }];
      const view = render(<App />);
      await act(async () => { await Promise.all(panel.load.mock.results.map((result) => result.value)); });
      await nativeFocusTrackingReady;
      nativeFocusChanged?.({ payload: false });
      act(() => { emitActivityTargets(); });
      collapse();
      // A finished agent is a desktop notification, not a decision for the owner.
      expect(view.queryByTestId("collapsed-attention-badge")).toBeNull();
      expect(view.getByRole("button", { name: "Open supervisor" })).toBeInTheDocument();

      const [decision] = panelSnapshotFixture.decisions;
      act(() => { panel.handlers.forEach((handler) => handler({ type: "panel.decision", sessionRef: decision.sessionRef, decision })); });
      expect(view.getByTestId("collapsed-attention-badge")).toHaveTextContent("1");

      act(() => { panel.handlers.forEach((handler) => handler({ type: "panel.decision", sessionRef: decision.sessionRef, decision: null })); });
      expect(view.queryByTestId("collapsed-attention-badge")).toBeNull();
    });

    it("writes no per-desktop notification history to storage", async () => {
      currentActivityTargets = [{
        workspaceId: "default", workspaceLabel: "Project", sessionId: "sess-1", tabId: "tab-1",
        worktreePath: "/repo/main", worktreeLabel: "main", agentLabel: "Codex", terminalTitle: "Task", state: "done",
      }];
      await act(async () => { render(<App />); });
      await nativeFocusTrackingReady;
      nativeFocusChanged?.({ payload: false });
      act(() => { emitActivityTargets(); emitTerminalBell(); });
      window.dispatchEvent(new Event("pagehide"));
      const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i) ?? "");
      expect(keys.filter((key) => key.includes("notifications.history"))).toEqual([]);
    });
  });

  describe("activation navigation", () => {
    it("recovers a startup click and focuses the active project's target", async () => {
      activationQueue = [{ workspaceId: "default", sessionId: "sess-1" }];
      await act(async () => { render(<App />); });
      expect(dispatchWorkspaceAction).toHaveBeenCalledWith({ type: "FOCUS_EXISTING_SESSION", sessionId: "sess-1" });
      expect(native.onNotificationActivated.mock.invocationCallOrder[0])
        .toBeLessThan(native.takeNotificationActivations.mock.invocationCallOrder[0]);
    });

    it("navigates on a live click by draining the same queue", async () => {
      await act(async () => { render(<App />); });
      dispatchWorkspaceAction.mockClear();
      activationQueue = [{ workspaceId: "default", sessionId: "sess-2" }];
      await act(async () => { notificationActivationListener?.(); });
      expect(dispatchWorkspaceAction).toHaveBeenCalledWith({ type: "FOCUS_EXISTING_SESSION", sessionId: "sess-2" });
    });

    it.each([
      { workspaceId: "ghost-project", sessionId: "sess-1" },
      { workspaceId: "default", sessionId: "ghost-session" },
    ])("rejects a stale click $workspaceId/$sessionId without navigating", async (target) => {
      await act(async () => { render(<App />); });
      dispatchWorkspaceAction.mockClear();
      activationQueue = [target];
      await act(async () => { notificationActivationListener?.(); });
      expect(native.takeNotificationActivations).toHaveBeenCalledTimes(2);
      expect(dispatchWorkspaceAction).not.toHaveBeenCalled();
    });

    it("switches to a parked project and focuses its target after the workspace mounts", async () => {
      seedTwoProjects();
      setWorkspaceSnapshot("other", parkedProjectSnapshot("sess-remote"));
      activationQueue = [{ workspaceId: "other", sessionId: "sess-remote" }];
      await act(async () => { render(<App />); });
      expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("other");
      expect(dispatchWorkspaceAction).toHaveBeenCalledWith({ type: "FOCUS_EXISTING_SESSION", sessionId: "sess-remote" });
    });

    it("does not switch projects for a closed target in another project", async () => {
      seedTwoProjects();
      setWorkspaceSnapshot("other", parkedProjectSnapshot(null));
      const restoreStarted = deferred<void>();
      const savedSession = deferred<null>();
      native.loadSession.mockImplementation(() => {
        restoreStarted.resolve();
        return savedSession.promise;
      });
      await act(async () => { render(<App />); });
      await restoreStarted.promise;
      expect(getWorkspaceRestoreStatus("default")).toBe("loading");
      await act(async () => { savedSession.resolve(null); await savedSession.promise; });
      expect(getWorkspaceRestoreStatus("default")).toBe("restored");
      expect(native.loadSession).toHaveBeenCalledTimes(1);

      dispatchWorkspaceAction.mockClear();
      const drainStarted = deferred<void>();
      const activations = deferred<typeof activationQueue>();
      native.takeNotificationActivations.mockImplementationOnce(() => {
        drainStarted.resolve();
        return activations.promise;
      });
      expect(notificationActivationListener).not.toBeNull();
      await act(async () => { notificationActivationListener!(); await drainStarted.promise; });
      await act(async () => {
        activations.resolve([{ workspaceId: "other", sessionId: "sess-remote" }]);
        await activations.promise;
      });
      expect(native.takeNotificationActivations).toHaveBeenCalledTimes(2);
      expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).not.toBe("other");
      expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("default");
      expect(dispatchWorkspaceAction).not.toHaveBeenCalled();
    });
  });
});
