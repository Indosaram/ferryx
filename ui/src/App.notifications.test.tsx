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

const { act, cleanup, render } = await import("@testing-library/react");
const { afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("./test/setup");

const { saveNotificationSettings } = await import("./lib/notificationSettings");

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

const native = {
  createWorktree: vi.fn(),
  getWorktreeStatus: vi.fn(),
  getInitialProject: vi.fn().mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }),
  listProjectBranches: vi.fn(),
  listWorktrees: vi.fn().mockResolvedValue([]),
  registerProject: vi.fn(),
  signalTerminal: vi.fn(),
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
  listenDagRunUpdated: vi.fn(() => Promise.resolve(() => undefined)),
  watchDagProject: vi.fn((projectPath: string) => Promise.resolve({ projectPath, runs: [] })),
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
  listProjectBranches: native.listProjectBranches,
  listWorktrees: native.listWorktrees,
  registerProject: native.registerProject,
  signalTerminal: native.signalTerminal,
  saveSession: native.saveSession,
  loadSession: native.loadSession,
  clearSession: native.clearSession,
  listTerminalSessions: native.listTerminalSessions,
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
  onTerminalOutput: native.onTerminalOutput,
  publishFocusedTerminal: native.publishFocusedTerminal,
  setBadgeCount: native.setBadgeCount,
  onRemoteSelectionRequested: native.onRemoteSelectionRequested,
  onWorktreeChanged: vi.fn().mockResolvedValue(() => {}),
  toIpcError: (error: unknown) => error,
  isStructuredIpcError: (_error: unknown) => false,
  dispatchNotification: native.dispatchNotification,
  playNotificationSound: native.playNotificationSound,
  onNotificationActivated: native.onNotificationActivated,
  takeNotificationActivations: native.takeNotificationActivations,
}));

const workspaceStoreModule = await import("./state/workspaceStore");
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
    "sess-1": { id: "sess-1", cwd: "/repo/main", worktreePath: "/repo/main" },
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

vi.mock("./components/Sidebar", () => ({
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY: "ferryx.sidebar.collapsedProjects",
  Sidebar: () => <div data-testid="mock-sidebar" />,
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

vi.mock("./components/TerminalSplitView", () => ({
  TerminalSplitView: () => <div data-testid="mock-terminal-split-view" />,
}));

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
    resetWorkspaceRestore();
    localStorage.clear();
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
    clearWorkspaceSnapshot();
    storeSpy?.mockRestore();
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
