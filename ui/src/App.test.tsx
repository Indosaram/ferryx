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

const { readFileSync } = await import("node:fs");
const { resolve } = await import("node:path");

const { act, cleanup, fireEvent, render, screen, waitFor, within } = await import("@testing-library/react");
const { afterEach, beforeEach, describe, expect, it, vi } = await import("vitest");
await import("./test/setup");

const { saveBrowserSettings } = await import("./lib/browserSettings");
const { saveAgentSettings } = await import("./lib/agentsSettings");
const {
  ACTIVE_PROJECT_STORAGE_KEY,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} = await import("./lib/storageKeys");

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
  onNewTerminalTabMenu: vi.fn(),
  onCloseTabMenu: vi.fn(),
  onSelectWorktreeMenu: vi.fn(),
  selectWorktreeMenuHandler: null as ((digit: number) => void) | null,
  onTerminalLifecycle: vi.fn().mockResolvedValue(() => {}),
  onTerminalOutput: vi.fn().mockResolvedValue(() => {}),
  menuHandler: null as null | (() => void),
  closeMenuHandler: null as null | (() => void),
  publishFocusedTerminal: vi.fn().mockResolvedValue(undefined),
  setBadgeCount: vi.fn().mockResolvedValue({ supported: true, count: 0 }),
  onRemoteSelectionRequested: vi.fn(),
  remoteSelectionHandler: null as null | ((payload: any) => void),
};

const updater = {
  checkForUpdate: vi.fn(),
};

const workspace = {
  activateTab: vi.fn(),
  activatePrimary: vi.fn(),
  closeTab: vi.fn().mockResolvedValue(undefined),
  splitPane: vi.fn().mockResolvedValue(undefined),
  closePane: vi.fn().mockResolvedValue(undefined),
  focusPane: vi.fn(),
  setPaneRatio: vi.fn(),
  swapPanes: vi.fn(),
  ensureTabForWorktree: vi.fn().mockResolvedValue(undefined),
  ensureSessionBackends: vi.fn().mockResolvedValue(undefined),
  openTab: vi.fn(),
  refreshWorktrees: vi.fn(),
  syncWorktrees: vi.fn(),
  restoreWorkspace: vi.fn(),
  createBrowserTab: vi.fn().mockResolvedValue("browser-tab-1"),
  reportRuntimeError: vi.fn(),
  storeState: {
    activeWorktreePath: "/repo/main",
    layout: {
      activeTabId: "tab-1",
      layoutsByTabId: {
        "tab-1": { root: { type: "leaf" as const, leafId: "leaf-1" }, activeLeafId: "leaf-1", expandedLeafId: null, sessionIdsByLeafId: { "leaf-1": "sess-1" } },
      },
      tabs: [
        { id: "tab-1", label: "main", sessionId: "sess-1" },
        { id: "tab-2", label: "feature", sessionId: "sess-2" },
        { id: "tab-3", label: "bugfix", sessionId: "sess-3" },
        { id: "tab-4", label: "docs", sessionId: "sess-4" },
      ],
    },
    sessions: {
      "sess-1": { id: "sess-1", cwd: "/repo/main" },
      "sess-2": { id: "sess-2", cwd: "/repo/feature" },
      "sess-3": { id: "sess-3", cwd: "/repo/bugfix" },
      "sess-4": { id: "sess-4", cwd: "/repo/docs" },
    },
    worktrees: [
      { path: "/repo/main", branch: "refs/heads/main" },
      { path: "/repo/feature", branch: "refs/heads/feature" },
      { path: "/repo/bugfix", branch: "refs/heads/bugfix" },
      { path: "/repo/docs", branch: "refs/heads/docs" },
    ] as Array<{ path: string; branch: string | null }>,
    unreadTabIds: {} as Record<string, boolean>,
  },
};

vi.mock("./lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  DEFAULT_TERMINAL_FONT_STACK: "monospace",
  getTerminalPreferences: () => Promise.resolve({}),
  createWorktree: native.createWorktree,
  getWorktreeStatus: native.getWorktreeStatus,
  previewWorktreeDelete: vi.fn(),
  deleteWorktree: vi.fn(),
  deleteWorktreeDestructive: vi.fn(),
  getInitialProject: native.getInitialProject,
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
  onSelectWorktreeMenu: native.onSelectWorktreeMenu,
  onTerminalLifecycle: native.onTerminalLifecycle,
  onTerminalOutput: native.onTerminalOutput,
  publishFocusedTerminal: native.publishFocusedTerminal,
  setBadgeCount: native.setBadgeCount,
  onRemoteSelectionRequested: native.onRemoteSelectionRequested,
  onWorktreeChanged: vi.fn().mockResolvedValue(() => {}),
  toIpcError: (error: unknown) => error,
  isStructuredIpcError: (_error: unknown) => false,
}));

vi.mock("./lib/updater", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./lib/updater")>();
  return {
    ...actual,
    checkForUpdate: updater.checkForUpdate,
  };
});

const workspaceStoreModule = await import("./state/workspaceStore");
let storeSpy: any;

vi.mock("./state/workspaceRuntime", () => ({
  useWorkspaceRuntime: () => ({
    refreshWorktrees: workspace.refreshWorktrees,
    reportRuntimeError: workspace.reportRuntimeError,
    runtimeError: null,
  }),
}));

vi.mock("./components/Sidebar", () => ({
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY: "ferryx.sidebar.collapsedProjects",
  Sidebar: ({
    open = true,
    onAddProject,
    onCreateWorktree,
    onSelectProject,
    onSelectWorktree,
    onToggle,
    onOpenSettings,
    projects = [],
    worktrees = [],
    activeProjectId,
  }: {
    open?: boolean;
    onAddProject?: () => void;
    onCreateWorktree: (project?: { workspaceId: string }) => void;
    onSelectProject?: (project: { workspaceId: string }) => void;
    onSelectWorktree?: (worktree: { path: string }) => void;
    onToggle?: () => void;
    onOpenSettings?: () => void;
    projects?: Array<{ workspaceId: string; gitRoot?: string | null }>;
    worktrees?: Array<{ path: string }>;
    activeProjectId?: string;
  }) => (
    <div data-testid="mock-sidebar" data-open={open}>
      <button type="button" onClick={onToggle}>
        {open ? "Hide sidebar" : "Show sidebar"}
      </button>
      <button type="button" onClick={() => onOpenSettings?.()}>
        Open settings
      </button>
      {open ? (
        <>
          <button type="button" onClick={() => onAddProject?.()}>
            Add project
          </button>
          <span>Active project {activeProjectId}</span>
          {/* Mirrors the real nested tree: every project row, with the active project's worktrees under it. */}
          {projects.map((project) => (
            <div key={project.workspaceId}>
              <button type="button" onClick={() => onSelectProject?.(project)}>
                Project {project.workspaceId}
              </button>
              {/* Mirrors the real Sidebar: non-git projects expose no worktree creation. */}
              {project.gitRoot !== null ? (
                <button type="button" onClick={() => onCreateWorktree(project)}>
                  Add worktree to {project.workspaceId}
                </button>
              ) : null}
              {project.workspaceId === activeProjectId
                ? worktrees.map((worktree) => (
                    <button key={worktree.path} type="button" onClick={() => onSelectWorktree?.(worktree)}>
                      Worktree {worktree.path}
                    </button>
                  ))
                : null}
            </div>
          ))}
        </>
      ) : null}
    </div>
  ),
}));

vi.mock("./components/CommandPalette", () => ({
  CommandPalette: ({ open }: { open: boolean }) =>
    open ? <div data-testid="command-palette" /> : null,
}));
vi.mock("./components/SettingsDialog", () => ({
  SettingsDialog: ({ open, onClose, projects, activeProjectId, activeWorktree, onSelectProject, onAddProject, onAddWorktree }: any) =>
    open ? (
      <div data-testid="settings-dialog" data-projects={JSON.stringify(projects)} data-active-project-id={activeProjectId} data-active-worktree={activeWorktree?.path ?? ""} data-has-select-project={String(Boolean(onSelectProject))} data-has-add-project={String(Boolean(onAddProject))} data-has-add-worktree={String(Boolean(onAddWorktree))}>
        <button onClick={onClose}>Close settings</button>
      </div>
    ) : null,
}));
vi.mock("./components/TerminalSplitView", async () => {
  const { useState } = await import("react");
  const { NewTabPopover } = await import("./components/NewTabPopover");
  return {
    TerminalSplitView: ({
      searchLeafId,
      agents,
      onLaunchAgent,
      onAddBrowserTab,
      defaultAgentId,
    }: {
      searchLeafId?: string | null;
      agents?: Array<{ name: string; command: string; args: string }>;
      onLaunchAgent?: (agent: { name: string; command: string; args: string }) => void;
      onAddBrowserTab?: (url?: string) => void;
      defaultAgentId?: string | null;
    }) => {
      const [open, setOpen] = useState(false);
      return (
        <div data-testid="terminal-split-view" data-search-leaf-id={searchLeafId ?? ""}>
          <button type="button" onClick={() => setOpen(true)}>
            New tab
          </button>
          <NewTabPopover
            open={open}
            onClose={() => setOpen(false)}
            onNewTerminal={() => setOpen(false)}
            onNewBrowser={(url) => {
              setOpen(false);
              onAddBrowserTab?.(url);
            }}
            agents={agents}
            onLaunchAgent={onLaunchAgent}
            defaultAgentId={defaultAgentId}
          />
          {agents?.map((agent) => (
            <button key={agent.name} type="button" onClick={() => onLaunchAgent?.(agent)}>
              Launch {agent.name}
            </button>
          ))}
        </div>
      );
    },
  };
});
vi.mock("./components/WorktreeDeleteDialog", () => ({ WorktreeDeleteDialog: () => null }));

const { App } = await import("./App");
const { resetWorkspaceRestore } = await import("./state/workspaceRestore");

afterEach(() => {
  cleanup();
  storeSpy?.mockRestore();
});

describe("App project workspace flow", () => {
  beforeEach(() => {
    resetWorkspaceRestore();
    storeSpy = vi.spyOn(workspaceStoreModule, "useWorkspaceStore").mockImplementation((() => ({
      state: workspace.storeState,
      recoveredFromHmr: false,
      agents: [],
      activateTab: workspace.activateTab,
      closeTab: workspace.closeTab,
      splitPane: workspace.splitPane,
      closePane: workspace.closePane,
      focusPane: workspace.focusPane,
      setPaneRatio: workspace.setPaneRatio,
      swapPanes: workspace.swapPanes,
      ensureTabForWorktree: workspace.ensureTabForWorktree,
      ensureSessionBackends: workspace.ensureSessionBackends,
      openTab: workspace.openTab,
      syncWorktrees: workspace.syncWorktrees,
      restoreWorkspace: workspace.restoreWorkspace,
      createBrowserTab: workspace.createBrowserTab,
      subscribeTerminalBell: () => () => undefined,
    })) as any);
    localStorage.clear();
    native.createWorktree.mockReset();
    native.getWorktreeStatus.mockReset();
    native.getInitialProject.mockReset();
    native.getInitialProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
    native.listProjectBranches.mockReset();
    native.listWorktrees.mockReset();
    native.listWorktrees.mockResolvedValue([]);
    native.registerProject.mockReset();
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.signalTerminal.mockReset();
    native.loadSession.mockReset();
    native.loadSession.mockResolvedValue(null);
    native.listTerminalSessions.mockReset();
    native.listTerminalSessions.mockResolvedValue([]);
    native.spawnTerminal.mockReset();
    native.spawnTerminal.mockResolvedValue("mock-spawn-id");
    native.onNewTerminalTabMenu.mockReset();
    native.onSelectWorktreeMenu.mockReset();
    native.selectWorktreeMenuHandler = null;
    native.onSelectWorktreeMenu.mockImplementation(async (handler: (digit: number) => void) => {
      native.selectWorktreeMenuHandler = handler;
      return () => {
        if (native.selectWorktreeMenuHandler === handler) native.selectWorktreeMenuHandler = null;
      };
    });
    native.onCloseTabMenu.mockReset();
    native.menuHandler = null;
    native.closeMenuHandler = null;
    native.onNewTerminalTabMenu.mockImplementation(async (handler: () => void) => {
      native.menuHandler = handler;
      return () => {
        if (native.menuHandler === handler) native.menuHandler = null;
      };
    });
    native.onCloseTabMenu.mockImplementation(async (handler: () => void) => {
      native.closeMenuHandler = handler;
      return () => {
        if (native.closeMenuHandler === handler) native.closeMenuHandler = null;
      };
    });
    native.onRemoteSelectionRequested.mockReset();
    updater.checkForUpdate.mockReset();
    updater.checkForUpdate.mockResolvedValue(undefined);
    native.remoteSelectionHandler = null;
    native.onRemoteSelectionRequested.mockImplementation(async (handler: (payload: any) => void) => {
      native.remoteSelectionHandler = handler;
      return () => {
        if (native.remoteSelectionHandler === handler) native.remoteSelectionHandler = null;
      };
    });
    native.publishFocusedTerminal.mockReset();
    native.publishFocusedTerminal.mockResolvedValue(undefined);
    native.setBadgeCount.mockReset();
    native.setBadgeCount.mockResolvedValue({ supported: true, count: 0 });
    workspace.reportRuntimeError.mockReset();
    workspace.openTab.mockReset();
    workspace.openTab.mockResolvedValue("tab-new");
    workspace.closeTab.mockReset();
    workspace.closeTab.mockResolvedValue(undefined);
    workspace.closePane.mockReset();
    workspace.closePane.mockResolvedValue(undefined);
    workspace.activatePrimary.mockReset();
    workspace.ensureTabForWorktree.mockReset();
    workspace.ensureTabForWorktree.mockResolvedValue(undefined);
    workspace.ensureSessionBackends.mockReset();
    workspace.ensureSessionBackends.mockResolvedValue(undefined);
    workspace.refreshWorktrees.mockReset();
    workspace.restoreWorkspace.mockReset();
    workspace.restoreWorkspace.mockResolvedValue(undefined);
    workspace.createBrowserTab.mockReset();
    workspace.createBrowserTab.mockResolvedValue("browser-tab-1");
    native.detectAgents.mockReset();
    native.detectAgents.mockResolvedValue([]);
    native.writeTerminal.mockReset();
    native.writeTerminal.mockResolvedValue(undefined);
    native.isTauriRuntime.mockReset();
    native.isTauriRuntime.mockReturnValue(false);
  });

  it("routes the native Cmd+T menu accelerator through the normal new-terminal callback", async () => {
    render(<App />);

    await waitFor(() => expect(native.onNewTerminalTabMenu).toHaveBeenCalledOnce());
    expect(native.menuHandler).toBeTypeOf("function");
    native.menuHandler?.();

    await waitFor(() =>
      expect(workspace.openTab).toHaveBeenCalledWith(expect.objectContaining({ path: "/repo/main" })),
    );
  });

  it("routes the native Cmd+W menu accelerator to close the active tab", async () => {
    render(<App />);

    await waitFor(() => expect(native.onCloseTabMenu).toHaveBeenCalledOnce());
    expect(native.closeMenuHandler).toBeTypeOf("function");
    native.closeMenuHandler?.();

    await waitFor(() => expect(workspace.closeTab).toHaveBeenCalledWith("tab-1"));
  });

  it("routes the native Cmd+W menu accelerator to close the focused pane in a split terminal tab", async () => {
    const previousLayout = workspace.storeState.layout;
    workspace.storeState.layout = {
      ...previousLayout,
      tabs: previousLayout.tabs.map((tab) => (tab.id === "tab-1" ? { ...tab, kind: "terminal" } : tab)),
      layoutsByTabId: {
        ...previousLayout.layoutsByTabId,
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-1", "leaf-2": "sess-2" },
        },
      },
    } as any;

    try {
      render(<App />);

      await waitFor(() => expect(native.onCloseTabMenu).toHaveBeenCalledOnce());
      native.closeMenuHandler?.();

      await waitFor(() => expect(workspace.closePane).toHaveBeenCalledWith("tab-1", "leaf-2"));
      expect(workspace.closeTab).not.toHaveBeenCalled();
    } finally {
      workspace.storeState.layout = previousLayout;
    }
  });

  it("routes the web Cmd+W shortcut to close the focused pane in a split terminal tab", async () => {
    const previousLayout = workspace.storeState.layout;
    workspace.storeState.layout = {
      ...previousLayout,
      tabs: previousLayout.tabs.map((tab) => (tab.id === "tab-1" ? { ...tab, kind: "terminal" } : tab)),
      layoutsByTabId: {
        ...previousLayout.layoutsByTabId,
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-2",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-1", "leaf-2": "sess-2" },
        },
      },
    } as any;

    try {
      render(<App />);

      fireEvent.keyDown(window, { key: "w", metaKey: true });

      await waitFor(() => expect(workspace.closePane).toHaveBeenCalledWith("tab-1", "leaf-2"));
      expect(workspace.closeTab).not.toHaveBeenCalled();
    } finally {
      workspace.storeState.layout = previousLayout;
    }
  });

  it("routes the native Cmd+W menu accelerator to close an active browser tab", async () => {
    const prevLayout = workspace.storeState.layout;
    workspace.storeState.layout = {
      ...prevLayout,
      activeTabId: "browser-tab-1",
      tabs: [
        {
          id: "browser-tab-1",
          kind: "browser",
          label: "Ferryx Docs",
          url: "https://example.com",
          browserId: "browser-1",
        },
        ...prevLayout.tabs,
      ],
    } as any;

    try {
      render(<App />);

      await waitFor(() => expect(native.onCloseTabMenu).toHaveBeenCalledOnce());
      expect(native.closeMenuHandler).toBeTypeOf("function");
      native.closeMenuHandler?.();

      await waitFor(() => expect(workspace.closeTab).toHaveBeenCalledWith("browser-tab-1"));
    } finally {
      workspace.storeState.layout = prevLayout;
    }
  });

  it("shows tab close confirmation when confirmCloseTab is enabled, cancelling on reject and closing on confirm", async () => {
    localStorage.setItem("ferryx.settings.general", JSON.stringify({ confirmCloseTab: true }));
    render(<App />);

    await waitFor(() => expect(native.onCloseTabMenu).toHaveBeenCalledOnce());
    native.closeMenuHandler?.();

    // Dialog appears
    const dialog = await screen.findByRole("dialog", { name: /close tab/i });
    expect(dialog).toBeInTheDocument();
    expect(workspace.closeTab).not.toHaveBeenCalled();

    // Cancel does not close
    const cancelButton = screen.getByRole("button", { name: /cancel/i });
    fireEvent.click(cancelButton);
    expect(screen.queryByRole("dialog", { name: /close tab/i })).not.toBeInTheDocument();
    expect(workspace.closeTab).not.toHaveBeenCalled();

    // Trigger again and confirm
    native.closeMenuHandler?.();
    const dialog2 = await screen.findByRole("dialog", { name: /close tab/i });
    expect(dialog2).toBeInTheDocument();

    const confirmButton = within(dialog2).getByRole("button", { name: /close tab/i });
    fireEvent.click(confirmButton);

    await waitFor(() => expect(workspace.closeTab).toHaveBeenCalledWith("tab-1"));
  });

  it("uses the canonical folder-derived project on an empty native startup", async () => {
    native.isTauriRuntime.mockReturnValue(true);

    render(<App />);

    await waitFor(() => expect(native.getInitialProject).toHaveBeenCalledOnce());
    expect(await screen.findByText("Active project orca-lite")).toBeInTheDocument();
    expect(localStorage.getItem(PROJECTS_STORAGE_KEY)).toBe(JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("orca-lite");
  });

  it("checks for a signed update when the native app starts", async () => {
    native.isTauriRuntime.mockReturnValue(true);

    render(<App />);

    await waitFor(() => expect(updater.checkForUpdate).toHaveBeenCalledOnce());
  });

  it("restores projects lost from WebView storage using the native session catalog", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
    native.loadSession.mockResolvedValue({
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "beta",
      workspaces: {
        alpha: {
          workspaceId: "alpha",
          repoRoot: "/repos/alpha",
          worktrees: [],
          activeWorktreePath: "/repos/alpha",
          layout: {
            splitMode: "none",
            primaryTabId: null,
            secondaryTabId: null,
            activeTabId: null,
            tabs: [],
          },
          terminalSessions: {},
        },
        beta: {
          workspaceId: "beta",
          repoRoot: "/repos/beta",
          worktrees: [],
          activeWorktreePath: "/repos/beta",
          layout: {
            splitMode: "none",
            primaryTabId: null,
            secondaryTabId: null,
            activeTabId: null,
            tabs: [],
          },
          terminalSessions: {},
        },
      },
    });

    render(<App />);

    expect(await screen.findByText("Active project beta")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Project alpha" })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY) ?? "[]")).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }),
        expect.objectContaining({ workspaceId: "alpha", repoRoot: "/repos/alpha" }),
        expect.objectContaining({ workspaceId: "beta", repoRoot: "/repos/beta" }),
      ]),
    );
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("beta");
  });

  it("does not restore historical projects into an existing multi-project catalog", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    localStorage.setItem(
      PROJECTS_STORAGE_KEY,
      JSON.stringify([
        { workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" },
        { workspaceId: "current", repoRoot: "/repos/current" },
      ]),
    );
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "current");
    native.loadSession.mockResolvedValue({
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "historical",
      workspaces: {
        historical: {
          workspaceId: "historical",
          repoRoot: "/repos/historical",
          worktrees: [],
          activeWorktreePath: "/repos/historical",
          layout: {
            splitMode: "none",
            primaryTabId: null,
            secondaryTabId: null,
            activeTabId: null,
            tabs: [],
          },
          terminalSessions: {},
        },
      },
    });

    render(<App />);

    expect(await screen.findByText("Active project current")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Project historical" })).not.toBeInTheDocument();
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("current");
  });

  it("keeps the native shell initializing until persisted workspace tabs are preloaded", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    let resolveSession!: (session: null) => void;
    const sessionPromise = new Promise<null>((resolve) => {
      resolveSession = resolve;
    });
    native.loadSession.mockImplementation(() => sessionPromise);

    render(<App />);

    await waitFor(() => expect(native.loadSession).toHaveBeenCalled());
    expect(screen.getByLabelText("Initializing project")).toBeInTheDocument();
    expect(screen.queryByText("Active project orca-lite")).not.toBeInTheDocument();

    await act(async () => {
      resolveSession(null);
      await sessionPromise;
    });

    expect(await screen.findByText("Active project orca-lite")).toBeInTheDocument();
  });

  it("migrates the legacy default placeholder to the canonical native project", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "default", repoRoot: "." }]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "default");

    render(<App />);

    expect(await screen.findByText("Active project orca-lite")).toBeInTheDocument();
    expect(localStorage.getItem(PROJECTS_STORAGE_KEY)).toBe(JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("orca-lite");
  });

  it("migrates legacy rorca/orca storage keys to ferryx namespace on startup (F10)", async () => {
    const project = { workspaceId: "migrated-proj", repoRoot: "/repos/migrated" };
    localStorage.setItem("rorca.projects", JSON.stringify([project]));
    localStorage.setItem("rorca.active-project", project.workspaceId);
    localStorage.setItem("orca.sidebar.open", "false");
    native.registerProject.mockResolvedValue(project);

    render(<App />);

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: project.workspaceId,
        repoPath: project.repoRoot,
      }),
    );

    expect(localStorage.getItem(PROJECTS_STORAGE_KEY)).toBe(JSON.stringify([project]));
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe(project.workspaceId);
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");
  });

  it("heals a legacy project entry (no gitRoot) into a git-backed project on startup", async () => {
    const legacy = { workspaceId: "legacy-git", repoRoot: "/repos/legacy-git" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([legacy]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, legacy.workspaceId);
    native.registerProject.mockResolvedValue({
      workspaceId: legacy.workspaceId,
      repoRoot: legacy.repoRoot,
      gitRoot: legacy.repoRoot,
    });

    render(<App />);

    expect(await screen.findByText("Active project legacy-git")).toBeInTheDocument();
    expect(screen.queryByText("Worktrees are unavailable for non-Git projects.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add worktree to legacy-git" })).toBeInTheDocument();
    const persisted = JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY) ?? "[]");
    expect(persisted).toEqual([legacy]);
  });

  it("re-registers the active persisted project before refreshing its worktrees", async () => {
    const project = { workspaceId: "rorca", repoRoot: "/repos/rorca" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([project]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, project.workspaceId);
    native.registerProject.mockResolvedValue(project);

    render(<App />);

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: project.workspaceId,
        repoPath: project.repoRoot,
      }),
    );
    await waitFor(() => expect(workspace.refreshWorktrees).toHaveBeenCalled());
  });

  it("opens a registered project's real branch-dropdown worktree flow", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "rorca", repoRoot: "/repos/rorca" });
    native.listProjectBranches.mockResolvedValue([{ name: "main", isCurrent: true }]);

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add project" }));
    expect(screen.getByRole("form", { name: "Add Project" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Workspace id" }), { target: { value: "rorca" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Repository path" }), { target: { value: "/repos/rorca" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() => expect(screen.getByText("Project rorca")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add worktree to rorca" }));

    expect(screen.getByRole("form", { name: "Add Worktree" })).toBeInTheDocument();
    await waitFor(() => expect(native.listProjectBranches).toHaveBeenCalledWith("rorca"));
    expect(screen.getByRole("combobox", { name: "Base branch" })).toBeInTheDocument();
  });

  it("opens worktree creation for the clicked non-active project", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));
    native.listProjectBranches.mockResolvedValue([{ name: "beta-main", isCurrent: true }]);

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Add worktree to beta" }));

    expect(await screen.findByRole("heading", { name: "Add Worktree · beta" })).toBeInTheDocument();
    await waitFor(() => expect(native.listProjectBranches).toHaveBeenCalledWith("beta"));
  });

  it("switches to the non-active project before opening its created worktree", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    const betaWorktree = { path: "/repos/beta/.orca-worktrees/beta/new-worktree", branch: "refs/heads/orca/beta/new-worktree" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));
    native.listProjectBranches.mockResolvedValue([{ name: "beta-main", isCurrent: true }]);
    native.createWorktree.mockResolvedValue(betaWorktree);
    workspace.refreshWorktrees.mockImplementation(() => {
      if (!workspace.storeState.worktrees.some((worktree: { path: string }) => worktree.path === betaWorktree.path)) {
        workspace.storeState.worktrees.push(betaWorktree);
      }
    });

    try {
      render(<App />);
      fireEvent.click(screen.getByRole("button", { name: "Add worktree to beta" }));
      await screen.findByRole("heading", { name: "Add Worktree · beta" });

      fireEvent.change(screen.getByRole("textbox", { name: "Worktree slug" }), { target: { value: "new-worktree" } });
      fireEvent.click(screen.getByRole("button", { name: "Create Worktree" }));

      await waitFor(() =>
        expect(native.createWorktree).toHaveBeenCalledWith({
          workspaceId: "beta",
          worktree: { wsId: "beta", slug: "new-worktree" },
          baseRef: "beta-main",
        }),
      );
      expect(await screen.findByText("Active project beta")).toBeInTheDocument();
      await waitFor(() => expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(betaWorktree));
    } finally {
      workspace.storeState.worktrees = workspace.storeState.worktrees.filter(
        (worktree: { path: string }) => worktree.path !== betaWorktree.path,
      );
    }
  });

  it("switches the active project when selecting a project row in the nested tree", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));

    render(<App />);

    expect(screen.getByText("Active project alpha")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Project beta" }));

    await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("beta");
    // Switching projects re-registers the newly selected project so its worktrees load.
    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({ workspaceId: "beta", repoPath: "/repos/beta" }),
    );
  });

  it("shows the original project's worktrees again after switching away and clicking back", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: `/repos/${workspaceId}`,
    }));

    const listedByWorkspace: Record<string, Array<{ path: string; branch: string }>> = {
      alpha: [{ path: "/repos/alpha", branch: "refs/heads/main" }],
      beta: [{ path: "/repos/beta", branch: "refs/heads/main" }],
    };
    const original = workspace.storeState.worktrees;
    workspace.refreshWorktrees.mockImplementation(async () => {
      const active = screen.queryByText(/^Active project /)?.textContent?.replace("Active project ", "") ?? "alpha";
      workspace.storeState.worktrees = listedByWorkspace[active] ?? [];
    });

    try {
      render(<App />);
      await waitFor(() => expect(screen.getByRole("button", { name: "Worktree /repos/alpha" })).toBeInTheDocument());

      fireEvent.click(screen.getByRole("button", { name: "Project beta" }));
      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());

      fireEvent.click(screen.getByRole("button", { name: "Project alpha" }));
      await waitFor(() => expect(screen.getByText("Active project alpha")).toBeInTheDocument());

      await waitFor(() =>
        expect(screen.getByRole("button", { name: "Worktree /repos/alpha" })).toBeInTheDocument(),
      );
    } finally {
      workspace.storeState.worktrees = original;
      workspace.refreshWorktrees.mockReset();
    }
  });

  it("adopts the canonical workspaceId the backend returns for an already-registered root", async () => {
    const stale = { workspaceId: "default", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([stale]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "default");
    native.registerProject.mockResolvedValue({
      workspaceId: "alpha",
      repoRoot: "/repos/alpha",
      gitRoot: "/repos/alpha",
    });

    render(<App />);

    await waitFor(() => expect(screen.getByText("Active project alpha")).toBeInTheDocument());
    expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("alpha");
    const persisted = JSON.parse(localStorage.getItem(PROJECTS_STORAGE_KEY) ?? "[]");
    expect(persisted.map((project: { workspaceId: string }) => project.workspaceId)).toEqual(["alpha"]);
  });

  it("retries registration on window focus after a transient failure instead of gating forever", async () => {
    const project = { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([project]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockRejectedValueOnce({ code: "DAEMON_UNAVAILABLE", message: "daemon down" });
    native.registerProject.mockResolvedValue(project);

    render(<App />);

    await waitFor(() => expect(native.registerProject).toHaveBeenCalledTimes(1));

    fireEvent(window, new Event("focus"));

    await waitFor(() => expect(native.registerProject).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(workspace.refreshWorktrees).toHaveBeenCalled());
  });

  it("persists the outgoing project's session when switching away", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: `/repos/${workspaceId}`,
    }));
    (workspace.storeState as { workspaceId?: string }).workspaceId = "alpha";

    try {
      render(<App />);
      await waitFor(() => expect(screen.getByText("Active project alpha")).toBeInTheDocument());
      native.saveSession.mockClear();

      fireEvent.click(screen.getByRole("button", { name: "Project beta" }));

      await waitFor(() =>
        expect(native.saveSession).toHaveBeenCalledWith(
          expect.objectContaining({ activeWorkspaceId: "alpha" }),
        ),
      );
    } finally {
      delete (workspace.storeState as { workspaceId?: string }).workspaceId;
    }
  });

  it("discards a parked worktree selection that belongs to a project no longer active", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: `/repos/${workspaceId}`,
    }));

    workspace.storeState.worktrees.push({ path: "/repos/beta/feature", branch: "refs/heads/orca/beta/feature" });

    try {
      render(<App />);

      fireEvent.click(screen.getByRole("button", { name: "Worktree /repos/beta/feature" }));
      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());

      workspace.ensureTabForWorktree.mockClear();
      fireEvent.click(screen.getByRole("button", { name: "Project alpha" }));
      await waitFor(() => expect(screen.getByText("Active project alpha")).toBeInTheDocument());

      expect(workspace.ensureTabForWorktree).not.toHaveBeenCalledWith(
        expect.objectContaining({ path: "/repos/beta/feature" }),
      );
    } finally {
      workspace.storeState.worktrees.pop();
    }
  });

  it("surfaces a genuine same-id different-root registration conflict instead of silently listing", async () => {
    const project = { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([project]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockRejectedValue({
      code: "WORKSPACE_ALREADY_REGISTERED",
      message: "Workspace 'alpha' is already registered",
    });

    render(<App />);

    await waitFor(() =>
      expect(workspace.reportRuntimeError).toHaveBeenCalledWith(
        expect.objectContaining({ code: "WORKSPACE_ALREADY_REGISTERED" }),
      ),
    );
    expect(workspace.refreshWorktrees).not.toHaveBeenCalled();
  });

  it("switches project instead of activating when the worktree belongs to another project", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));

    // A worktree owned by "beta" surfaced while "alpha" is active.
    workspace.storeState.worktrees.push({ path: "/repo/beta-feature", branch: "refs/heads/orca/beta/feature" });

    try {
      render(<App />);

      fireEvent.click(screen.getByRole("button", { name: "Worktree /repo/beta-feature" }));

      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
      // Once its owning project is active, the parked selection focuses that worktree's tab.
      await waitFor(() =>
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith({
          path: "/repo/beta-feature",
          branch: "refs/heads/orca/beta/feature",
        }),
      );
    } finally {
      workspace.storeState.worktrees.pop();
    }
  });

  it("switches project for a branch-less worktree nested under another project's root", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: null },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: workspaceId === "alpha" ? `/repos/${workspaceId}` : null,
    }));

    workspace.storeState.worktrees.push({ path: "/repos/beta/nested", branch: null });

    try {
      render(<App />);

      fireEvent.click(screen.getByRole("button", { name: "Worktree /repos/beta/nested" }));

      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
    } finally {
      workspace.storeState.worktrees.pop();
    }
  });

  it("activates a worktree directly when it belongs to the active project", async () => {
    const project = { workspaceId: "alpha", repoRoot: "/repos/alpha" };
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([project]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    native.registerProject.mockResolvedValue(project);

    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Worktree /repo/feature" }));

    await waitFor(() =>
      expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[1]),
    );
  });

  it("walks visible worktrees top to bottom across expanded projects with Cmd+1..", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    // Both projects expanded in the sidebar accordion.
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));

    // "alpha" owns the four unbranded worktrees (the active project's own list); the last two
    // belong to "beta" and render below them in the tree.
    const betaFeature = { path: "/repo/beta-feature", branch: "refs/heads/orca/beta/feature" };
    const betaDocs = { path: "/repo/beta-docs", branch: "refs/heads/orca/beta/docs" };
    workspace.storeState.worktrees.push(betaFeature, betaDocs);

    try {
      render(<App />);

      // Indexes 1..4 stay inside the first (active) project, which is listed first.
      fireEvent.keyDown(window, { key: "1", metaKey: true });
      expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[0]);

      fireEvent.keyDown(window, { key: "4", metaKey: true });
      expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[3]);

      cleanup();
      workspace.ensureTabForWorktree.mockClear();

      // Collapsing "alpha" removes its four worktrees from the tree, so "beta"'s worktrees move up
      // to positions 1 and 2 and the same keys now cross into the other project.
      localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(["alpha"]));
      render(<App />);

      fireEvent.keyDown(window, { key: "2", metaKey: true });

      // Crossing projects switches the active project and still focuses that worktree's tab.
      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
      await waitFor(() => expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(betaDocs));
    } finally {
      workspace.storeState.worktrees.length = 4;
    }
  });

  it("skips a collapsed project's rows when counting Cmd+N positions", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    // beta expanded, alpha collapsed: only beta's rows are visible.
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(["alpha"]));
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: `/repos/${workspaceId}`,
    }));

    // A branch-less row living under beta's root: only path-based ownership
    // attributes it to beta. Branch-only logic hands it to the active project.
    const betaRoot = { path: "/repos/beta/nested", branch: null };
    workspace.storeState.worktrees.push(betaRoot);

    try {
      render(<App />);

      // Cmd+1 must hit beta's row, not alpha's hidden rows.
      fireEvent.keyDown(window, { key: "1", metaKey: true });

      await waitFor(() =>
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/repos/beta/nested" }),
        ),
      );
    } finally {
      workspace.storeState.worktrees.length = 4;
    }
  });

  it("selects a worktree when macOS forwards Cmd+N through the native key monitor", async () => {
    localStorage.setItem(
      PROJECTS_STORAGE_KEY,
      JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo", gitRoot: "/repo" }]),
    );
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    native.registerProject.mockResolvedValue({
      workspaceId: "orca-lite",
      repoRoot: "/repo",
      gitRoot: "/repo",
    });

    render(<App />);
    await waitFor(() => expect(native.onSelectWorktreeMenu).toHaveBeenCalled());

    // The Window menu swallows the keydown, so the digit arrives as a native event.
    act(() => {
      native.selectWorktreeMenuHandler!(2);
    });

    await waitFor(() =>
      expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
        expect.objectContaining({ path: "/repo/feature" }),
      ),
    );
  });

  it("targets the synthesized root row of a visible non-Git project via native Cmd+digit", async () => {
    const originalWorktrees = [...workspace.storeState.worktrees];
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "plain-docs", repoRoot: "/notes/docs", gitRoot: null },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));

    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: workspaceId === "alpha" ? "/repos/alpha" : "/notes/docs",
      gitRoot: workspaceId === "alpha" ? "/repos/alpha" : null,
    }));

    const alphaMain = {
      path: "/repos/alpha/main",
      branch: "refs/heads/main",
      head: "",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    workspace.storeState.worktrees = [alphaMain];

    try {
      render(<App />);
      await waitFor(() => expect(native.onSelectWorktreeMenu).toHaveBeenCalled());

      // alpha has 1 row (digit 1). plain-docs is visible with no git worktrees,
      // so its synthesized folder root is row 2 (digit 2).
      const handler = native.selectWorktreeMenuHandler;
      if (!handler) throw new Error("Expected selectWorktreeMenuHandler to be registered");
      act(() => {
        handler(2);
      });

      // Targeting the synthesized plain-folder root switches to the plain project.
      await waitFor(() => expect(screen.getByText("Active project plain-docs")).toBeInTheDocument());

      cleanup();
      workspace.ensureTabForWorktree.mockClear();

      // When the non-Git project is active and has no git rows, digit 1 targets its synthesized root row directly.
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "plain-docs");
      workspace.storeState.worktrees = [];
      render(<App />);
      await waitFor(() => expect(native.onSelectWorktreeMenu).toHaveBeenCalled());

      const activeHandler = native.selectWorktreeMenuHandler;
      if (!activeHandler) throw new Error("Expected selectWorktreeMenuHandler to be registered");
      act(() => {
        activeHandler(1);
      });

      await waitFor(() =>
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/notes/docs", branch: null }),
        ),
      );
    } finally {
      workspace.storeState.worktrees = originalWorktrees;
    }
  });

  it("targets inactive cached rows before extra owned rows matching Sidebar top-to-bottom order via native Cmd+digit", async () => {
    const originalWorktrees = [...workspace.storeState.worktrees];
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "alpha");
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));

    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
      gitRoot: `/repos/${workspaceId}`,
    }));

    const alphaMain = {
      path: "/repos/alpha/main",
      branch: "refs/heads/main",
      head: "",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const betaExtra = {
      path: "/repos/beta/extra-owned",
      branch: "refs/heads/orca/beta/extra-owned",
      head: "",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const betaCached1 = {
      path: "/repos/beta/cached-first",
      branch: "refs/heads/cached-1",
      head: "",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const betaCached2 = {
      path: "/repos/beta/cached-second",
      branch: "refs/heads/cached-2",
      head: "",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    workspace.storeState.worktrees = [alphaMain, betaExtra, betaCached1, betaCached2];

    native.listWorktrees.mockImplementation(async (workspaceId: string) => {
      if (workspaceId === "beta") return [betaCached1, betaCached2];
      return [];
    });

    try {
      render(<App />);
      await waitFor(() => expect(native.onSelectWorktreeMenu).toHaveBeenCalled());
      await waitFor(() => expect(native.listWorktrees).toHaveBeenCalledWith("beta"));

      // Top-to-bottom Sidebar order is:
      // Digit 1: alpha -> /repos/alpha/main
      // Digit 2: beta  -> /repos/beta/cached-first  (cached row 1)
      // Digit 3: beta  -> /repos/beta/cached-second (cached row 2)
      // Digit 4: beta  -> /repos/beta/extra-owned   (extra owned row)
      const handler = native.selectWorktreeMenuHandler;
      if (!handler) throw new Error("Expected selectWorktreeMenuHandler to be registered");
      act(() => {
        handler(2);
      });

      await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
      await waitFor(() =>
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/repos/beta/cached-first" }),
        ),
      );
    } finally {
      workspace.storeState.worktrees = originalWorktrees;
    }
  });

  it("keeps out-of-range Cmd+N presses a no-op", async () => {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "default", repoRoot: "." }]));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "default");
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    // Only four worktrees are visible, so Cmd+5..9 target nothing.
    for (const key of ["5", "6", "7", "8", "9"]) {
      fireEvent.keyDown(window, { key, metaKey: true });
    }

    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
    expect(workspace.activatePrimary).not.toHaveBeenCalled();
  });

  it("navigates workspaces with Cmd+1..4 and terminal tabs with Ctrl+1..4 ignoring out-of-range keys", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    // Cmd+1..4 selects workspace indexes 1..4 (worktrees 0..3)
    fireEvent.keyDown(window, { key: "1", metaKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[0]);

    fireEvent.keyDown(window, { key: "2", metaKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[1]);

    fireEvent.keyDown(window, { key: "3", metaKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[2]);

    fireEvent.keyDown(window, { key: "4", metaKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[3]);

    // Ctrl+1..4 selects terminal tab indexes 1..4
    workspace.activatePrimary.mockClear();
    workspace.ensureTabForWorktree.mockClear();

    // Tab 1 is currently active worktree ("/repo/main"), so it directly activates primary tab
    fireEvent.keyDown(window, { key: "1", ctrlKey: true });
    expect(workspace.activateTab).toHaveBeenCalledWith("tab-1");

    // Tab 2 has cwd "/repo/feature", so handleSelectTerminalTab ensures tab for worktree and activates
    fireEvent.keyDown(window, { key: "2", ctrlKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[1]);

    fireEvent.keyDown(window, { key: "3", ctrlKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[2]);

    fireEvent.keyDown(window, { key: "4", ctrlKey: true });
    expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(workspace.storeState.worktrees[3]);

    // Out of range (index 5) or unmodified number typing is harmless
    workspace.activatePrimary.mockClear();
    workspace.ensureTabForWorktree.mockClear();

    fireEvent.keyDown(window, { key: "5", metaKey: true });
    fireEvent.keyDown(window, { key: "5", ctrlKey: true });
    fireEvent.keyDown(window, { key: "1" });
    fireEvent.keyDown(window, { key: "2" });

    expect(workspace.activatePrimary).not.toHaveBeenCalled();
    expect(workspace.ensureTabForWorktree).not.toHaveBeenCalled();
  });

  it("toggles sidebar visibility with Cmd+B and persists state across mounts", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    const { unmount } = render(<App />);

    expect(screen.getByTestId("mock-sidebar")).toHaveAttribute("data-open", "true");
    expect(screen.getByRole("button", { name: "Hide sidebar" })).toBeInTheDocument();

    // Toggle off via Cmd+B shortcut
    fireEvent.keyDown(window, { key: "b", metaKey: true });
    expect(screen.queryByTestId("mock-sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show sidebar" })).toBeInTheDocument();
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");

    // Toggle on via Show sidebar button in header
    fireEvent.click(screen.getByRole("button", { name: "Show sidebar" }));
    expect(screen.getByTestId("mock-sidebar")).toBeInTheDocument();
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("true");

    // Toggle off via Hide sidebar button in sidebar
    fireEvent.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(screen.queryByTestId("mock-sidebar")).not.toBeInTheDocument();
    expect(localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY)).toBe("false");

    unmount();

    // Rerender loads persisted false state
    render(<App />);
    expect(screen.queryByTestId("mock-sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show sidebar" })).toBeInTheDocument();
  });

  it("requests fresh backends for restored tabs whose daemon sessions are gone", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([]);
    native.spawnTerminal.mockResolvedValue("new-spawned-session-id");

    const savedSession = {
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "orca-lite",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "dead-sess-1", label: "main", worktreePath: "/repo/main" }],
          },
          terminalSessions: {
            "dead-sess-1": {
              sessionId: "dead-sess-1",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(savedSession as any);

    render(<App />);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    await waitFor(() => expect(workspace.ensureSessionBackends).toHaveBeenCalledWith(["dead-sess-1"]));
    expect(native.spawnTerminal).not.toHaveBeenCalled();

    const restoredState = workspace.restoreWorkspace.mock.calls[0]?.[0];
    expect(restoredState).toBeDefined();
    expect(restoredState.sessions["dead-sess-1"]).toMatchObject({
      id: "dead-sess-1",
      backendSessionId: null,
      lifecycle: "exited",
      cwd: "/repo/main",
    });
  });

  it("waits until project registration and authoritative worktree refresh complete before restoring workspace and recovering stale session backends", async () => {
    let resolveRegister!: (value: any) => void;
    const registerPromise = new Promise((resolve) => {
      resolveRegister = resolve;
    });
    native.registerProject.mockImplementation(() => registerPromise);

    let resolveRefreshWorktrees!: () => void;
    const refreshWorktreesPromise = new Promise<void>((resolve) => {
      resolveRefreshWorktrees = resolve;
    });
    workspace.refreshWorktrees.mockImplementation(() => refreshWorktreesPromise);

    const savedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "dead-sess-1", label: "main", worktreePath: "/repo/main" }],
          },
          terminalSessions: {
            "dead-sess-1": {
              sessionId: "dead-sess-1",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(savedSession as any);
    native.listTerminalSessions.mockResolvedValue([]);
    workspace.restoreWorkspace.mockClear();
    workspace.ensureSessionBackends.mockClear();

    render(<App />);

    // 1. While project registration is in flight, restore must not begin
    expect(native.registerProject).toHaveBeenCalled();
    expect(workspace.refreshWorktrees).not.toHaveBeenCalled();
    expect(workspace.restoreWorkspace).not.toHaveBeenCalled();
    expect(workspace.ensureSessionBackends).not.toHaveBeenCalled();

    // 2. Resolve project registration - worktree refresh begins, but restore must still wait
    await act(async () => {
      resolveRegister({ workspaceId: "default", repoRoot: "." });
    });
    expect(workspace.refreshWorktrees).toHaveBeenCalled();
    expect(workspace.restoreWorkspace).not.toHaveBeenCalled();
    expect(workspace.ensureSessionBackends).not.toHaveBeenCalled();

    // 3. Resolve worktree refresh - now readiness gate is satisfied and restore + session recovery execute
    await act(async () => {
      resolveRefreshWorktrees();
    });

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    await waitFor(() => expect(workspace.ensureSessionBackends).toHaveBeenCalledWith(["dead-sess-1"]));

    const restoredState = workspace.restoreWorkspace.mock.calls[0]?.[0];
    expect(restoredState).toBeDefined();
    expect(restoredState.sessions["dead-sess-1"]).toMatchObject({
      id: "dead-sess-1",
      backendSessionId: null,
      lifecycle: "exited",
    });
  });

  it("restores persisted terminal sessions only once when restored state changes effect dependencies", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([]);
    native.spawnTerminal.mockResolvedValue("new-spawned-session-id");
    native.loadSession.mockResolvedValue({
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none",
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "dead-sess-1", label: "main", worktreePath: "/repo/main" }],
          },
          terminalSessions: {
            "dead-sess-1": {
              sessionId: "dead-sess-1",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    });

    const { rerender } = render(<App />);
    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalledTimes(1));
    expect(native.spawnTerminal).not.toHaveBeenCalled();

    workspace.storeState.layout = { ...workspace.storeState.layout, tabs: [...workspace.storeState.layout.tabs] };
    workspace.storeState.sessions = { ...workspace.storeState.sessions };
    rerender(<App />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(native.loadSession).toHaveBeenCalledTimes(1);
    expect(workspace.restoreWorkspace).toHaveBeenCalledTimes(1);
    expect(native.spawnTerminal).not.toHaveBeenCalled();
  });

  it("preserves live backend sessions when restoring during dev/HMR", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([{ sessionId: "live-sess-1", worktreePath: "/repo/main" }]);
    native.spawnTerminal.mockClear();

    const savedSession = {
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "live-sess-1", label: "main", worktreePath: "/repo/main" }],
          },
          terminalSessions: {
            "live-sess-1": {
              sessionId: "live-sess-1",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(savedSession as any);

    render(<App />);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    expect(native.spawnTerminal).not.toHaveBeenCalled();
  });

  it("reconciles live and missing sessions across split panes without auto-respawning dead sessions", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([{ sessionId: "live-sess-1", worktreePath: "/repo/main" }]);
    native.spawnTerminal.mockClear();
    native.spawnTerminal.mockResolvedValue("respawned-sess-2");

    const savedSession = {
      version: 1,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "horizontal" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "live-sess-1", label: "main", worktreePath: "/repo/main" }],
            layoutsByTabId: {
              "tab-1": {
                root: {
                  type: "split",
                  direction: "horizontal",
                  first: { type: "leaf", leafId: "leaf-1" },
                  second: { type: "leaf", leafId: "leaf-2" },
                  ratio: 0.5,
                },
                activeLeafId: "leaf-1",
                sessionIdsByLeafId: {
                  "leaf-1": "live-sess-1",
                  "leaf-2": "dead-sess-2",
                },
              },
            },
          },
          terminalSessions: {
            "live-sess-1": {
              sessionId: "live-sess-1",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
            "dead-sess-2": {
              sessionId: "dead-sess-2",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(savedSession as any);

    render(<App />);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    expect(native.spawnTerminal).not.toHaveBeenCalled();

    const restoredState = workspace.restoreWorkspace.mock.calls[0]?.[0];
    expect(restoredState).toBeDefined();
    expect(restoredState.sessions["live-sess-1"]).toMatchObject({
      id: "live-sess-1",
      backendSessionId: "live-sess-1",
      lifecycle: "working",
    });
    expect(restoredState.sessions["dead-sess-2"]).toMatchObject({
      id: "dead-sess-2",
      backendSessionId: null,
      lifecycle: "exited",
    });
  });

  it("reconciles daemonEpoch and lastOutputSequence on startup, marking epoch mismatch as exited", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([
      { sessionId: "live-backend-1", daemonEpoch: "epoch-10" },
      { sessionId: "live-backend-2", daemonEpoch: "epoch-10" },
    ]);
    native.spawnTerminal.mockClear();

    const savedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [
              { id: "tab-1", sessionId: "sess-matching", label: "main", kind: "terminal" as const },
              { id: "tab-2", sessionId: "sess-mismatch", label: "feat", kind: "terminal" as const },
            ],
          },
          terminalSessions: {
            "sess-matching": {
              localSessionId: "sess-matching",
              backendSessionId: "live-backend-1",
              daemonEpoch: "epoch-10",
              lastOutputSequence: "777",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
            "sess-mismatch": {
              localSessionId: "sess-mismatch",
              backendSessionId: "live-backend-2",
              daemonEpoch: "epoch-OLD",
              lastOutputSequence: "100",
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(savedSession as any);

    render(<App />);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    expect(native.spawnTerminal).not.toHaveBeenCalled();

    const restoredState = workspace.restoreWorkspace.mock.calls[0]?.[0];
    expect(restoredState).toBeDefined();
    // Matching epoch + live backend ID -> preserved
    expect(restoredState.sessions["sess-matching"]).toMatchObject({
      id: "sess-matching",
      backendSessionId: "live-backend-1",
      lifecycle: "working",
      daemonEpoch: "epoch-10",
      // Cold restore starts with an empty terminal; replay must not be suppressed.
      lastOutputSequence: null,
    });
    // Mismatching epoch -> exited / lost
    expect(restoredState.sessions["sess-mismatch"]).toMatchObject({
      id: "sess-mismatch",
      backendSessionId: null,
      lifecycle: "exited",
      daemonEpoch: null,
      lastOutputSequence: null,
    });
  });

  it("toggles the settings dialog with Cmd+,", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(await screen.findByTestId("settings-dialog")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: ",", metaKey: true });
    await waitFor(() => expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument());
  });

  it("activates in-terminal search on active pane with Cmd+F", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    expect(screen.getByTestId("terminal-split-view")).toHaveAttribute("data-search-leaf-id", "");

    fireEvent.keyDown(window, { key: "f", metaKey: true });
    expect(screen.getByTestId("terminal-split-view")).toHaveAttribute("data-search-leaf-id", "leaf-1");
  });

  it("moves pane focus next and previous with Cmd+] and Cmd+[", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    // Configure storeState with a split layout of two leaves
    const prevLayout = workspace.storeState.layout;
    workspace.storeState.layout = {
      ...prevLayout,
      layoutsByTabId: {
        "tab-1": {
          root: {
            type: "split",
            direction: "horizontal",
            first: { type: "leaf", leafId: "leaf-1" },
            second: { type: "leaf", leafId: "leaf-2" },
            ratio: 0.5,
          },
          activeLeafId: "leaf-1",
          expandedLeafId: null,
          sessionIdsByLeafId: { "leaf-1": "sess-1", "leaf-2": "sess-2" },
        },
      },
    } as any;

    render(<App />);

    workspace.focusPane.mockClear();
    fireEvent.keyDown(window, { key: "]", metaKey: true });
    expect(workspace.focusPane).toHaveBeenCalledWith("tab-1", "leaf-2");

    workspace.focusPane.mockClear();
    fireEvent.keyDown(window, { key: "[", metaKey: true });
    expect(workspace.focusPane).toHaveBeenCalledWith("tab-1", "leaf-2");

    // Restore storeState
    workspace.storeState.layout = prevLayout;
  });

  it("detects agents on mount and launches agent via spawnTerminal and writeTerminal", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    native.detectAgents.mockResolvedValue([
      { name: "claude", available: true },
      { name: "codex", available: false },
    ]);
    native.spawnTerminal.mockResolvedValue("spawn-claude-session");

    render(<App />);

    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole("button", { name: "Launch claude" })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "Launch claude" }));

    await waitFor(() => {
      expect(native.spawnTerminal).toHaveBeenCalledWith(
        expect.objectContaining({
          workspaceId: "orca-lite",
          cwd: "/repo/main",
        }),
      );
    });

    await waitFor(() => {
      expect(workspace.openTab).toHaveBeenCalledWith(
        expect.objectContaining({ path: "/repo/main" }),
        "Claude",
        "spawn-claude-session",
      );
    });

    await waitFor(() => {
      expect(native.writeTerminal).toHaveBeenCalledWith({
        sessionId: "spawn-claude-session",
        data: "claude\r",
      });
    });
  });

  it("surfaces the stored defaultAgentId first with a Default label in the New Tab agent list", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    native.detectAgents.mockResolvedValue([
      { name: "claude", available: true },
      { name: "aider", available: true },
    ]);
    saveAgentSettings({ version: 1, defaultAgentId: "aider", overrides: {} });

    render(<App />);

    await waitFor(() => expect(native.detectAgents).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole("button", { name: "Launch aider" })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "New tab" }));
    const buttons = within(screen.getByText("AGENTS").parentElement as HTMLElement).getAllByRole("button");
    expect(buttons[0]).toHaveTextContent("Aider");
    expect(within(buttons[0]).getByText("Default")).toBeVisible();
    expect(buttons[1]).toHaveTextContent("Claude");
    expect(within(buttons[1]).queryByText("Default")).not.toBeInTheDocument();
  });

  it("opens a browser tab at the configured homepage from the new-tab menu", async () => {
    saveBrowserSettings({ homePage: "https://example.com/start" });
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Browser Tab/i }));

    await waitFor(() => {
      expect(workspace.createBrowserTab).toHaveBeenCalledWith("https://example.com/start");
    });
  });

  it("opens a blank browser tab from the new-tab menu when no homepage is set", async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "New tab" }));
    fireEvent.click(screen.getByRole("button", { name: /New Browser Tab/i }));

    await waitFor(() => {
      expect(workspace.createBrowserTab).toHaveBeenCalledWith("about:blank");
    });
  });

  it("preserves every registered workspace when saving session instead of clobbering inactive workspaces", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "beta");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));

    const preloadedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "alpha",
      workspaces: {
        alpha: {
          workspaceId: "alpha",
          repoRoot: "/repos/alpha",
          worktrees: [{ path: "/repos/alpha", branch: "main", head: "111", isMain: true, isLocked: false }],
          activeWorktreePath: "/repos/alpha",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-alpha-1",
            secondaryTabId: null,
            activeTabId: "tab-alpha-1",
            tabs: [{ id: "tab-alpha-1", sessionId: "sess-alpha-1", label: "main", worktreePath: "/repos/alpha" }],
          },
          terminalSessions: {
            "sess-alpha-1": {
              localSessionId: "sess-alpha-1",
              backendSessionId: "backend-alpha-1",
              worktreePath: "/repos/alpha",
              cwd: "/repos/alpha",
              createdAt: Date.now(),
            },
          },
        },
      },
    };
    native.loadSession.mockResolvedValue(preloadedSession as any);
    native.saveSession.mockClear();

    const { flushCloseGuards } = await import("./lib/updater");

    render(<App />);

    expect(await screen.findByText("Active project beta")).toBeInTheDocument();

    await act(async () => {
      await flushCloseGuards();
    });

    expect(native.saveSession).toHaveBeenCalled();
    const lastSaved = native.saveSession.mock.lastCall?.[0];
    expect(lastSaved?.workspaces).toHaveProperty("alpha");
    expect(lastSaved?.workspaces).toHaveProperty("beta");
  });

  it("passes live workspace state and project callbacks to SettingsDialog", async () => {
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(projects));
    localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "beta");
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Open settings" }));
    const dialog = await screen.findByTestId("settings-dialog");
    expect(dialog.dataset.projects).toBe(JSON.stringify(projects));
    expect(dialog.dataset.activeProjectId).toBe("beta");
    expect(dialog.dataset.activeWorktree).toBe("/repo/main");
    expect(dialog.dataset.hasSelectProject).toBe("true");
    expect(dialog.dataset.hasAddProject).toBe("true");
    expect(dialog.dataset.hasAddWorktree).toBe("true");
  });

  it("conditionally mounts SettingsDialog and CommandPalette only when open", () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();
    expect(screen.queryByTestId("command-palette")).not.toBeInTheDocument();
  });

  it("lazy-loads SettingsDialog via React.lazy with Suspense", () => {
    const appSource = readFileSync(resolve(import.meta.dirname ?? ".", "App.tsx"), "utf-8");
    expect(appSource).toMatch(/lazy\(\s*\(\)\s*=>\s*import\(\s*["']\.\/components\/SettingsDialog["']\s*\)/);
    expect(appSource).not.toMatch(/^import\s+\{[^}]*SettingsDialog[^}]*\}\s+from\s+["']\.\/components\/SettingsDialog["']/m);
  });

  it("does not list state in registerWindowCloseGuard effect dependencies", () => {
    const appSource = readFileSync(resolve(import.meta.dirname ?? ".", "App.tsx"), "utf-8");
    const match = appSource.match(/registerWindowCloseGuard[\s\S]*?\},[\s\S]*?\[([\s\S]*?)\]\);/);
    expect(match).toBeTruthy();
    const deps = match ? match[1] : "";
    expect(deps).not.toMatch(/\bstate\b/);
  });

  it("does not pass inline arrow functions for onOpenSettings or onCloseSearch", () => {
    const appSource = readFileSync(resolve(import.meta.dirname ?? ".", "App.tsx"), "utf-8");
    expect(appSource).not.toMatch(/onOpenSettings=\{\(\)\s*=>/);
    expect(appSource).not.toMatch(/onCloseSearch=\{\(\)\s*=>/);
  });

  it("preserves layout ownership and marks dead sessions exited across active and parked layouts without auto-respawning", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "/repo/main" });
    native.listTerminalSessions.mockResolvedValue([]);
    const spawnedSessions: Record<string, string> = {
      "/repo/main/backend": "respawned-main-pty",
      "/repo/feature/packages/ui": "respawned-feature-pty",
    };
    native.spawnTerminal.mockImplementation(async ({ cwd }: { cwd: string }) => {
      return spawnedSessions[cwd] ?? "respawned-fallback-pty";
    });

    let resolveSession!: (session: any) => void;
    const sessionPromise = new Promise((resolve) => {
      resolveSession = resolve;
    });
    native.loadSession.mockImplementation(() => sessionPromise);

    const persistedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: "/repo/main",
          worktrees: [
            { path: "/repo/main", branch: "refs/heads/main", head: "111", isMain: true, isLocked: false },
            { path: "/repo/feature", branch: "refs/heads/orca/default/feature-branch", head: "222", isMain: false, isLocked: false },
          ],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-main",
            secondaryTabId: null,
            activeTabId: "tab-main",
            tabs: [
              {
                id: "tab-main",
                label: "main-term",
                kind: "terminal" as const,
                terminal: {
                  primarySessionId: "sess-main-dead",
                  paneTree: { type: "leaf" as const, leafId: "leaf-main" },
                  sessionIdsByLeafId: { "leaf-main": "sess-main-dead" },
                  activeLeafId: "leaf-main",
                  expandedLeafId: null,
                },
              },
            ],
          },
          worktreeLayouts: {
            "/repo/feature": {
              splitMode: "none" as const,
              primaryTabId: "tab-feature",
              secondaryTabId: null,
              activeTabId: "tab-feature",
              tabs: [
                {
                  id: "tab-feature",
                  label: "feature-term",
                  kind: "terminal" as const,
                  terminal: {
                    primarySessionId: "sess-feature-dead",
                    paneTree: { type: "leaf" as const, leafId: "leaf-feature" },
                    sessionIdsByLeafId: { "leaf-feature": "sess-feature-dead" },
                    activeLeafId: "leaf-feature",
                    expandedLeafId: null,
                  },
                },
              ],
            },
          },
          terminalSessions: {
            "sess-main-dead": {
              localSessionId: "sess-main-dead",
              backendSessionId: null,
              worktreePath: "/repo/main",
              cwd: "/repo/main/backend",
              createdAt: Date.now(),
            },
            "sess-feature-dead": {
              localSessionId: "sess-feature-dead",
              backendSessionId: null,
              worktreePath: "/repo/feature",
              cwd: "/repo/feature/packages/ui",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    render(<App />);

    // Establish exact baseline after non-restore mount effects settle
    await waitFor(() => expect(native.registerProject).toHaveBeenCalled());
    native.spawnTerminal.mockClear();
    workspace.restoreWorkspace.mockClear();

    // Trigger the controlled restore seam
    resolveSession(persistedSession);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalledTimes(1));
    expect(native.spawnTerminal).not.toHaveBeenCalled();

    const finalRestoreCall = workspace.restoreWorkspace.mock.calls[0]?.[0];
    expect(finalRestoreCall).toBeDefined();
    expect(finalRestoreCall.activeWorktreePath).toBe("/repo/main");

    // Verify layout ownership is preserved across active and parked layouts
    expect(finalRestoreCall.layout.tabs[0].id).toBe("tab-main");
    expect(finalRestoreCall.layout.layoutsByTabId["tab-main"].sessionIdsByLeafId).toEqual({
      "leaf-main": "sess-main-dead",
    });
    expect(finalRestoreCall.worktreeLayouts["/repo/feature"].tabs[0].id).toBe("tab-feature");
    expect(finalRestoreCall.worktreeLayouts["/repo/feature"].layoutsByTabId["tab-feature"].sessionIdsByLeafId).toEqual({
      "leaf-feature": "sess-feature-dead",
    });

    // Verify both dead sessions were preserved with null backendSessionId and exited lifecycle without respawn
    expect(finalRestoreCall.sessions["sess-main-dead"]).toMatchObject({
      id: "sess-main-dead",
      cwd: "/repo/main/backend",
      worktreePath: "/repo/main",
      worktree: null,
      backendSessionId: null,
      lifecycle: "exited",
    });
    expect(finalRestoreCall.sessions["sess-feature-dead"]).toMatchObject({
      id: "sess-feature-dead",
      cwd: "/repo/feature/packages/ui",
      worktreePath: "/repo/feature",
      worktree: { wsId: "default", slug: "feature-branch" },
      backendSessionId: null,
      lifecycle: "exited",
    });
  });

  it("successfully completes session restore under React StrictMode double-mount without skipping", async () => {
    const { StrictMode } = await import("react");
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([]);
    native.spawnTerminal.mockClear();

    let resolveSession!: (session: any) => void;
    const sessionPromise = new Promise((resolve) => {
      resolveSession = resolve;
    });
    native.loadSession.mockImplementation(() => sessionPromise);

    const savedSession = {
      version: 2,
      timestamp: Date.now(),
      activeWorkspaceId: "default",
      workspaces: {
        default: {
          workspaceId: "default",
          repoRoot: ".",
          worktrees: [{ path: "/repo/main", branch: "main", head: "abc", isMain: true, isLocked: false }],
          activeWorktreePath: "/repo/main",
          layout: {
            splitMode: "none" as const,
            primaryTabId: "tab-1",
            secondaryTabId: null,
            activeTabId: "tab-1",
            tabs: [{ id: "tab-1", sessionId: "sess-1", label: "main", kind: "terminal" as const }],
          },
          terminalSessions: {
            "sess-1": {
              localSessionId: "sess-1",
              backendSessionId: null,
              worktreePath: "/repo/main",
              cwd: "/repo/main",
              createdAt: Date.now(),
            },
          },
        },
      },
    };

    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );

    // In StrictMode, mount 1 starts async loadSession, cleanup cancels mount 1, mount 2 starts.
    // When loadSession resolves, mount 2 must complete restore and invoke restoreWorkspace.
    resolveSession(savedSession);

    await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
    expect(native.spawnTerminal).not.toHaveBeenCalled();
  });

  it("skips native disk session restore when store was hydrated from HMR handoff and no HMR state exists", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "/repo/main" });
    native.loadSession.mockClear();
    workspace.restoreWorkspace.mockClear();

    storeSpy.mockImplementation((() => ({
      state: workspace.storeState,
      recoveredFromHmr: true,
      agents: [],
      activateTab: workspace.activateTab,
      closeTab: workspace.closeTab,
      splitPane: workspace.splitPane,
      closePane: workspace.closePane,
      focusPane: workspace.focusPane,
      setPaneRatio: workspace.setPaneRatio,
      swapPanes: workspace.swapPanes,
      ensureTabForWorktree: workspace.ensureTabForWorktree,
      openTab: workspace.openTab,
      syncWorktrees: workspace.syncWorktrees,
      restoreWorkspace: workspace.restoreWorkspace,
      ensureSessionBackends: workspace.ensureSessionBackends,
      createBrowserTab: workspace.createBrowserTab,
      subscribeTerminalBell: () => () => undefined,
    })) as any);

    render(<App />);

    await waitFor(() => expect(native.registerProject).toHaveBeenCalled());
    expect(native.loadSession).not.toHaveBeenCalled();
    expect(workspace.restoreWorkspace).not.toHaveBeenCalled();
  });

  it("reconciles stale sessions and triggers ensureSessionBackends on HMR handoff when daemon restarted", async () => {
    const { setHmrWorkspaceState, clearHmrWorkspaceState } = await import("./state/hmrWorkspaceState");
    const workspaceId = "default";
    const hmrState = {
      ...workspace.storeState,
      sessions: {
        "dead-sess-1": {
          id: "dead-sess-1",
          backendSessionId: "stale-backend-pty-888",
          worktreePath: "/repo/main",
          cwd: "/repo/main",
          lifecycle: "working" as const,
        },
      },
      layout: {
        ...workspace.storeState.layout,
        tabs: [{ id: "tab-1", sessionId: "dead-sess-1", label: "main" }],
        layoutsByTabId: {
          "tab-1": {
            root: { type: "leaf" as const, leafId: "leaf-1" },
            activeLeafId: "leaf-1",
            expandedLeafId: null,
            sessionIdsByLeafId: { "leaf-1": "dead-sess-1" },
            contentsByLeafId: {
              "leaf-1": { kind: "terminal" as const, sessionId: "dead-sess-1" },
            },
          },
        },
      },
    };

    setHmrWorkspaceState(workspaceId, hmrState as any);
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "/repo/main" });
    native.listTerminalSessions.mockResolvedValue([]);
    native.loadSession.mockClear();
    workspace.restoreWorkspace.mockClear();
    workspace.ensureSessionBackends.mockClear();

    storeSpy.mockImplementation((() => ({
      state: hmrState,
      recoveredFromHmr: true,
      agents: [],
      activateTab: workspace.activateTab,
      closeTab: workspace.closeTab,
      splitPane: workspace.splitPane,
      closePane: workspace.closePane,
      focusPane: workspace.focusPane,
      setPaneRatio: workspace.setPaneRatio,
      swapPanes: workspace.swapPanes,
      ensureTabForWorktree: workspace.ensureTabForWorktree,
      openTab: workspace.openTab,
      syncWorktrees: workspace.syncWorktrees,
      restoreWorkspace: workspace.restoreWorkspace,
      ensureSessionBackends: workspace.ensureSessionBackends,
      createBrowserTab: workspace.createBrowserTab,
      subscribeTerminalBell: () => () => undefined,
    })) as any);

    try {
      render(<App />);

      await waitFor(() => expect(workspace.restoreWorkspace).toHaveBeenCalled());
      await waitFor(() => expect(workspace.ensureSessionBackends).toHaveBeenCalledWith(["dead-sess-1"]));
      expect(native.loadSession).not.toHaveBeenCalled();

      const restoredState = workspace.restoreWorkspace.mock.calls[0]?.[0];
      expect(restoredState).toBeDefined();
      expect(restoredState.sessions["dead-sess-1"]).toMatchObject({
        id: "dead-sess-1",
        backendSessionId: null,
        lifecycle: "exited",
      });
    } finally {
      clearHmrWorkspaceState(workspaceId);
    }
  });

  describe("Ferryx desktop focused-terminal publisher and remote selection listener", () => {
    it("derives focused terminal payload through focused group -> active tab -> active leaf -> session -> backendSessionId", async () => {
      const { deriveFocusedTerminal } = await import("./App");
      const sampleState: any = {
        activeWorktreePath: "/repo/main",
        layout: {
          focusedGroupId: "group-2",
          tabGroups: {
            "group-1": { id: "group-1", tabIds: ["tab-1"], activeTabId: "tab-1" },
            "group-2": { id: "group-2", tabIds: ["tab-2", "tab-3"], activeTabId: "tab-3" },
          },
          activeTabId: "tab-1",
          tabs: [
            { id: "tab-1", label: "main", sessionId: "sess-1" },
            { id: "tab-2", label: "feature", sessionId: "sess-2" },
            { id: "tab-3", label: "split-term", sessionId: "sess-3" },
          ],
          layoutsByTabId: {
            "tab-3": {
              root: {
                type: "split",
                direction: "horizontal",
                first: { type: "leaf", leafId: "leaf-left" },
                second: { type: "leaf", leafId: "leaf-right" },
                ratio: 0.5,
              },
              activeLeafId: "leaf-right",
              expandedLeafId: null,
              sessionIdsByLeafId: {
                "leaf-left": "sess-3a",
                "leaf-right": "sess-3b",
              },
            },
          },
        },
        sessions: {
          "sess-1": { id: "sess-1", backendSessionId: "backend-pty-1", cwd: "/repo/main", worktreePath: "/repo/main" },
          "sess-2": { id: "sess-2", backendSessionId: "backend-pty-2", cwd: "/repo/feature", worktreePath: "/repo/feature" },
          "sess-3a": { id: "sess-3a", backendSessionId: "backend-pty-3a", cwd: "/repo/main", worktreePath: "/repo/main" },
          "sess-3b": { id: "sess-3b", backendSessionId: "backend-pty-3b", cwd: "/repo/feature", worktreePath: "/repo/feature" },
        },
        worktrees: [
          { path: "/repo/main", branch: "refs/heads/main" },
          { path: "/repo/feature", branch: "refs/heads/orca/orca-lite/feature-branch" },
        ],
      };

      const result = deriveFocusedTerminal("orca-lite", sampleState);
      expect(result).toEqual({
        workspaceId: "orca-lite",
        worktreeSlug: "feature-branch",
        worktreeLabel: "orca/orca-lite/feature-branch",
        backendSessionId: "backend-pty-3b",
        activeTabId: "tab-3",
        tabId: "tab-3",
        tabs: [
          { id: "tab-2", label: "feature" },
          { id: "tab-3", label: "split-term" },
        ],
        terminalTabs: [
          { id: "tab-2", label: "feature" },
          { id: "tab-3", label: "split-term" },
        ],
      });

      // Verify no absolute paths in derived result
      const serialized = JSON.stringify(result);
      expect(serialized).not.toContain("/repo/");
    });

    it("publishes only same-worktree terminal tabs with safe labels", async () => {
      const { deriveFocusedTerminal } = await import("./App");
      const result = deriveFocusedTerminal("orca-lite", {
        activeWorktreePath: "/repo/main",
        layout: {
          activeTabId: "tab-active",
          tabs: [
            { id: "tab-active", label: "Editor", sessionId: "session-active" },
            { id: "tab-sibling", label: "/Users/alice/private", sessionId: "session-sibling" },
            { id: "tab-other-worktree", label: "Other worktree", sessionId: "session-other" },
            { id: "browser-tab", label: "Browser", kind: "browser" },
          ],
          layoutsByTabId: {},
        },
        sessions: {
          "session-active": { id: "session-active", backendSessionId: "pty-active", worktreePath: "/repo/main" },
          "session-sibling": { id: "session-sibling", backendSessionId: "pty-sibling", worktreePath: "/repo/main" },
          "session-other": { id: "session-other", backendSessionId: "pty-other", worktreePath: "/repo/feature" },
        },
        worktrees: [
          { path: "/repo/main", branch: "refs/heads/main" },
          { path: "/repo/feature", branch: "refs/heads/orca/orca-lite/feature" },
        ],
      } as any);

      expect(result?.terminalTabs).toEqual([
        { id: "tab-active", label: "Editor" },
        { id: "tab-sibling", label: "Terminal" },
      ]);
      expect(JSON.stringify(result)).not.toContain("/Users/alice/private");
      expect(JSON.stringify(result)).not.toContain("tab-other-worktree");
    });

    it("publishes the primary worktree with no managed slug", async () => {
      const { deriveFocusedTerminal } = await import("./App");
      const result = deriveFocusedTerminal("orca-lite", {
        activeWorktreePath: "/repo/main",
        layout: {
          activeTabId: "tab-main",
          tabs: [{ id: "tab-main", label: "main", sessionId: "session-main" }],
          layoutsByTabId: {},
        },
        sessions: {
          "session-main": { id: "session-main", backendSessionId: "pty-main", worktreePath: "/repo/main" },
        },
        worktrees: [{ path: "/repo/main", branch: "refs/heads/main" }],
      } as any);

      expect(result?.worktreeSlug).toBeNull();
      expect(result?.worktreeLabel).toBe("main");
    });

    it("derives per-tab activityState and agentType when present in activityBySessionId", async () => {
      const { deriveFocusedTerminal } = await import("./App");
      const sampleState: any = {
        activeWorktreePath: "/repo/main",
        layout: {
          activeTabId: "tab-1",
          tabs: [
            { id: "tab-1", label: "main", sessionId: "sess-1" },
            { id: "tab-2", label: "feature", sessionId: "sess-2" },
            { id: "tab-3", label: "plain", sessionId: "sess-3" },
          ],
          layoutsByTabId: {},
        },
        sessions: {
          "sess-1": { id: "sess-1", backendSessionId: "pty-1", cwd: "/repo/main" },
          "sess-2": { id: "sess-2", backendSessionId: "pty-2", cwd: "/repo/main" },
          "sess-3": { id: "sess-3", backendSessionId: "pty-3", cwd: "/repo/main" },
        },
        worktrees: [{ path: "/repo/main", branch: "refs/heads/main" }],
        activityBySessionId: {
          "sess-1": {
            state: "waiting",
            title: "Claude Waiting",
            isAgent: true,
            agentType: "claude",
          },
          "sess-2": {
            state: "working",
            title: "Codex Working",
            isAgent: true,
            agentType: "codex",
          },
        },
      };

      const result = deriveFocusedTerminal("orca-lite", sampleState);
      expect(result?.terminalTabs).toEqual([
        { id: "tab-1", label: "main", activityState: "waiting", agentType: "claude" },
        { id: "tab-2", label: "feature", activityState: "working", agentType: "codex" },
        { id: "tab-3", label: "plain" },
      ]);
      expect(result?.tabs).toEqual(result?.terminalTabs);
      expect(result?.terminalTabs?.[2]?.activityState).toBeUndefined();
      expect(result?.terminalTabs?.[2]?.agentType).toBeUndefined();
      expect(result?.terminalTabs?.[2] ? "activityState" in result.terminalTabs[2] : true).toBe(false);
      expect(result?.terminalTabs?.[2] ? "agentType" in result.terminalTabs[2] : true).toBe(false);
    });

    it("clears active terminal (returns null) when active tab is a browser tab", async () => {
      const { deriveFocusedTerminal } = await import("./App");
      const sampleState: any = {
        activeWorktreePath: "/repo/main",
        layout: {
          activeTabId: "browser-tab-1",
          tabs: [
            { id: "tab-1", label: "main", sessionId: "sess-1" },
            { id: "browser-tab-1", label: "Web Preview", kind: "browser", browserId: "b-1", url: "http://localhost:3000" },
          ],
          layoutsByTabId: {},
        },
        sessions: {
          "sess-1": { id: "sess-1", backendSessionId: "backend-pty-1", cwd: "/repo/main" },
        },
        worktrees: [{ path: "/repo/main", branch: "refs/heads/main" }],
      };

      const result = deriveFocusedTerminal("orca-lite", sampleState);
      expect(result).toBeNull();
    });

    it("publishes focused terminal to native IPC on mount and layout changes", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
      const storeWithSession = {
        ...workspace.storeState,
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: "pty-focused-live",
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
        },
      };

      storeSpy.mockImplementation((() => ({
        state: storeWithSession,
        recoveredFromHmr: false,
        agents: [],
        activateTab: workspace.activateTab,
        closeTab: workspace.closeTab,
        splitPane: workspace.splitPane,
        closePane: workspace.closePane,
        focusPane: workspace.focusPane,
        setPaneRatio: workspace.setPaneRatio,
        swapPanes: workspace.swapPanes,
        ensureTabForWorktree: workspace.ensureTabForWorktree,
        openTab: workspace.openTab,
        syncWorktrees: workspace.syncWorktrees,
        restoreWorkspace: workspace.restoreWorkspace,
        createBrowserTab: workspace.createBrowserTab,
        subscribeTerminalBell: () => () => undefined,
      })) as any);

      render(<App />);

      await waitFor(() => expect(native.publishFocusedTerminal).toHaveBeenCalled());
      const lastCall = native.publishFocusedTerminal.mock.calls.at(-1)?.[0];
      expect(lastCall).toEqual({
        workspaceId: "orca-lite",
        worktreeSlug: null,
        worktreeLabel: "main",
        backendSessionId: "pty-focused-live",
        activeTabId: "tab-1",
        tabId: "tab-1",
        tabs: [
          { id: "tab-1", label: "main" },
        ],
        terminalTabs: [
          { id: "tab-1", label: "main" },
        ],
      });
      // Absolute path must not leak
      expect(JSON.stringify(lastCall)).not.toContain("/repo/");
    });

    it("publishes per-tab agent activity state and agent type to native IPC", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      const storeWithActivity = {
        ...workspace.storeState,
        layout: {
          ...workspace.storeState.layout,
          tabs: [
            { id: "tab-1", label: "main", sessionId: "sess-1" },
            { id: "tab-2", label: "feature", sessionId: "sess-2" },
            { id: "tab-3", label: "docs", sessionId: "sess-3" },
          ],
        },
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: "pty-1",
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
          "sess-2": {
            id: "sess-2",
            backendSessionId: "pty-2",
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
          "sess-3": {
            id: "sess-3",
            backendSessionId: "pty-3",
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
        },
        activityBySessionId: {
          "sess-1": {
            state: "waiting" as const,
            title: "Claude Waiting",
            isAgent: true,
            agentType: "claude",
          },
          "sess-2": {
            state: "working" as const,
            title: "Codex Working",
            isAgent: true,
            agentType: "codex",
          },
        },
      };

      storeSpy.mockImplementation(() => ({
        state: storeWithActivity,
        recoveredFromHmr: false,
        agents: [],
        activateTab: workspace.activateTab,
        closeTab: workspace.closeTab,
        splitPane: workspace.splitPane,
        closePane: workspace.closePane,
        focusPane: workspace.focusPane,
        setPaneRatio: workspace.setPaneRatio,
        swapPanes: workspace.swapPanes,
        ensureTabForWorktree: workspace.ensureTabForWorktree,
        ensureSessionBackends: workspace.ensureSessionBackends,
        openTab: workspace.openTab,
        syncWorktrees: workspace.syncWorktrees,
        restoreWorkspace: workspace.restoreWorkspace,
        createBrowserTab: workspace.createBrowserTab,
        subscribeTerminalBell: () => () => undefined,
      }));

      render(<App />);

      await waitFor(() => expect(native.publishFocusedTerminal).toHaveBeenCalled());
      const lastCall = native.publishFocusedTerminal.mock.calls.at(-1)?.[0];
      expect(lastCall).toEqual({
        workspaceId: "orca-lite",
        worktreeSlug: null,
        worktreeLabel: "main",
        backendSessionId: "pty-1",
        activeTabId: "tab-1",
        tabId: "tab-1",
        tabs: [
          { id: "tab-1", label: "main", activityState: "waiting", agentType: "claude" },
          { id: "tab-2", label: "feature", activityState: "working", agentType: "codex" },
          { id: "tab-3", label: "docs" },
        ],
        terminalTabs: [
          { id: "tab-1", label: "main", activityState: "waiting", agentType: "claude" },
          { id: "tab-2", label: "feature", activityState: "working", agentType: "codex" },
          { id: "tab-3", label: "docs" },
        ],
      });
      expect(lastCall?.terminalTabs?.[0]?.activityState).toBe("waiting");
      expect(lastCall?.terminalTabs?.[0]?.agentType).toBe("claude");
      expect(lastCall?.terminalTabs?.[1]?.activityState).toBe("working");
      expect(lastCall?.terminalTabs?.[1]?.agentType).toBe("codex");
      expect(lastCall?.terminalTabs?.[2]?.activityState).toBeUndefined();
      expect(lastCall?.terminalTabs?.[2]?.agentType).toBeUndefined();
      expect(lastCall?.terminalTabs?.[2] ? "activityState" in lastCall.terminalTabs[2] : true).toBe(false);
      expect(lastCall?.terminalTabs?.[2] ? "agentType" in lastCall.terminalTabs[2] : true).toBe(false);
    });

    it("publishes real backendSessionId after session backend is bound or rebound", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      let currentStoreState = {
        ...workspace.storeState,
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: null as string | null,
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
        },
      };

      const mockStore = () => ({
        state: currentStoreState,
        recoveredFromHmr: false,
        agents: [],
        activateTab: workspace.activateTab,
        closeTab: workspace.closeTab,
        splitPane: workspace.splitPane,
        closePane: workspace.closePane,
        focusPane: workspace.focusPane,
        setPaneRatio: workspace.setPaneRatio,
        swapPanes: workspace.swapPanes,
        ensureTabForWorktree: workspace.ensureTabForWorktree,
        ensureSessionBackends: workspace.ensureSessionBackends,
        openTab: workspace.openTab,
        syncWorktrees: workspace.syncWorktrees,
        restoreWorkspace: workspace.restoreWorkspace,
        createBrowserTab: workspace.createBrowserTab,
        subscribeTerminalBell: () => () => undefined,
      });

      storeSpy.mockImplementation(mockStore);

      const { rerender } = render(<App />);

      await waitFor(() => expect(native.publishFocusedTerminal).toHaveBeenCalled());
      expect(native.publishFocusedTerminal.mock.calls.at(-1)?.[0]?.backendSessionId).toBeNull();

      // Simulate backend binding/rebinding (e.g. ensureSessionBackends or REBIND_SESSION_BACKEND)
      currentStoreState = {
        ...currentStoreState,
        sessions: {
          "sess-1": {
            id: "sess-1",
            backendSessionId: "pty-rebound-live-123",
            cwd: "/repo/main",
            worktreePath: "/repo/main",
          },
        },
      };

      rerender(<App />);

      await waitFor(() => {
        const lastCall = native.publishFocusedTerminal.mock.calls.at(-1)?.[0];
        expect(lastCall?.backendSessionId).toBe("pty-rebound-live-123");
      });
      expect(native.publishFocusedTerminal.mock.calls.at(-1)?.[0]?.backendSessionId).not.toBeNull();
    });

    it("routes publishFocusedTerminal rejection to reportRuntimeError", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
      const publishError = new Error("Gateway publish rejected");
      native.publishFocusedTerminal.mockRejectedValue(publishError);

      render(<App />);

      await waitFor(() => expect(workspace.reportRuntimeError).toHaveBeenCalledWith(publishError));
    });

    it("handles native remote_selection_requested and activates requested worktree context", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      render(<App />);

      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());
      expect(native.remoteSelectionHandler).toBeTypeOf("function");

      // Dispatch remote selection for worktree slug 'feature'
      act(() => {
        native.remoteSelectionHandler!({
          workspaceId: "orca-lite",
          worktreeSlug: "feature",
        });
      });

      await waitFor(() => {
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/repo/feature" }),
        );
      });
    });

    it("handles same-worktree tab selection request and activates exact tab", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      render(<App />);

      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());
      expect(native.remoteSelectionHandler).toBeTypeOf("function");

      workspace.activateTab.mockClear();
      const previousCwd = workspace.storeState.sessions["sess-2"].cwd;
      workspace.storeState.sessions["sess-2"].cwd = "/repo/main";

      // Dispatch remote selection for same active worktree with specific tabId
      act(() => {
        native.remoteSelectionHandler!({
          workspaceId: "orca-lite",
          worktreeSlug: "main",
          tabId: "tab-2",
        });
      });

      await waitFor(() => {
        expect(workspace.activateTab).toHaveBeenCalledWith("tab-2");
      });
      workspace.storeState.sessions["sess-2"].cwd = previousCwd;
    });

    it("handles primary worktree selection within current project when on secondary worktree", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      // Simulate currently active on secondary worktree
      const prevActivePath = workspace.storeState.activeWorktreePath;
      workspace.storeState.activeWorktreePath = "/repo/feature";

      render(<App />);

      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());
      workspace.ensureTabForWorktree.mockClear();
      workspace.activateTab.mockClear();

      // Dispatch remote selection for project without worktreeSlug (selecting primary worktree) and with tabId
      act(() => {
        native.remoteSelectionHandler!({
          workspaceId: "orca-lite",
          tabId: "tab-1",
        });
      });

      await waitFor(() => {
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/repo/main" }),
        );
      });

      workspace.storeState.activeWorktreePath = prevActivePath;
    });

    it("handles cross-worktree selection with tabId activating tab after worktree switch", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      render(<App />);

      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());
      workspace.ensureTabForWorktree.mockClear();
      workspace.activateTab.mockClear();

      act(() => {
        native.remoteSelectionHandler!({
          workspaceId: "orca-lite",
          worktreeSlug: "feature",
          tabId: "tab-2",
        });
      });

      await waitFor(() => {
        expect(workspace.ensureTabForWorktree).toHaveBeenCalledWith(
          expect.objectContaining({ path: "/repo/feature" }),
        );
        expect(workspace.activateTab).toHaveBeenCalledWith("tab-2");
      });
    });

    it("handles cross-project remote selection by switching project and then activating worktree", async () => {
      localStorage.setItem(
        PROJECTS_STORAGE_KEY,
        JSON.stringify([
          { workspaceId: "project-1", repoRoot: "/repo/p1" },
          { workspaceId: "project-2", repoRoot: "/repo/p2" },
        ]),
      );
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "project-1");

      render(<App />);

      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());

      act(() => {
        native.remoteSelectionHandler!({
          workspaceId: "project-2",
        });
      });

      await waitFor(() => {
        expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("project-2");
      });
    });

    it("drops a queued remote slug when the user switches to a third project first", async () => {
      localStorage.setItem(
        PROJECTS_STORAGE_KEY,
        JSON.stringify([
          { workspaceId: "project-1", repoRoot: "/repo/p1" },
          { workspaceId: "project-2", repoRoot: "/repo/p2" },
          { workspaceId: "project-3", repoRoot: "/repo/p3" },
        ]),
      );
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "project-1");

      render(<App />);
      await waitFor(() => expect(native.onRemoteSelectionRequested).toHaveBeenCalled());
      workspace.ensureTabForWorktree.mockClear();

      // Remote asks for project-2's "feature" worktree, so the slug is queued
      // while project-2 registers. Before that queue drains the user lands on
      // project-3, which happens to own a worktree with the same slug. Both
      // land in one batch, so the queued slug is already stale when the effect
      // that consumes it first runs.
      await act(async () => {
        native.remoteSelectionHandler!({ workspaceId: "project-2", worktreeSlug: "feature" });
        native.remoteSelectionHandler!({ workspaceId: "project-3" });
      });

      await waitFor(() => expect(localStorage.getItem(ACTIVE_PROJECT_STORAGE_KEY)).toBe("project-3"));

      expect(workspace.ensureTabForWorktree).not.toHaveBeenCalledWith(
        expect.objectContaining({ path: "/repo/feature" }),
      );
    });
  });

  describe("badge count synchronization", () => {
    it("synchronizes badge count from unique true unreadTabIds in workspace state", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

      storeSpy.mockImplementation(() => ({
        state: {
          ...workspace.storeState,
          unreadTabIds: { "tab-1": true, "tab-2": true, "tab-3": false },
        },
        dispatch: vi.fn(),
        openTab: workspace.openTab,
        ensureTabForWorktree: workspace.ensureTabForWorktree,
        activateTab: workspace.activateTab,
        closeTab: workspace.closeTab,
        splitPane: workspace.splitPane,
        closePane: workspace.closePane,
        focusPane: workspace.focusPane,
        setPaneRatio: workspace.setPaneRatio,
        swapPanes: workspace.swapPanes,
        syncWorktrees: workspace.syncWorktrees,
        restoreWorkspace: workspace.restoreWorkspace,
        createBrowserTab: workspace.createBrowserTab,
        subscribeTerminalBell: () => () => undefined,
      }));

      render(<App />);

      await waitFor(() => expect(native.setBadgeCount).toHaveBeenCalledWith(2));
    });

    it("routes native badge synchronization errors to reportRuntimeError", async () => {
      localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify([{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }]));
      localStorage.setItem(ACTIVE_PROJECT_STORAGE_KEY, "orca-lite");
      native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
      const badgeError = new Error("Native badge unsupported");
      native.setBadgeCount.mockRejectedValue(badgeError);

      storeSpy.mockImplementation(() => ({
        state: {
          ...workspace.storeState,
          unreadTabIds: { "tab-1": true },
        },
        dispatch: vi.fn(),
        openTab: workspace.openTab,
        ensureTabForWorktree: workspace.ensureTabForWorktree,
        activateTab: workspace.activateTab,
        closeTab: workspace.closeTab,
        splitPane: workspace.splitPane,
        closePane: workspace.closePane,
        focusPane: workspace.focusPane,
        setPaneRatio: workspace.setPaneRatio,
        swapPanes: workspace.swapPanes,
        syncWorktrees: workspace.syncWorktrees,
        restoreWorkspace: workspace.restoreWorkspace,
        createBrowserTab: workspace.createBrowserTab,
        subscribeTerminalBell: () => () => undefined,
      }));

      render(<App />);

      await waitFor(() => expect(workspace.reportRuntimeError).toHaveBeenCalledWith(badgeError));
    });
  });
});
