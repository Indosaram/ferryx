import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ACTIVE_PROJECT_STORAGE_KEY,
  PROJECTS_STORAGE_KEY,
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY,
  SIDEBAR_OPEN_STORAGE_KEY,
} from "./lib/storageKeys";

const native = vi.hoisted(() => ({
  createWorktree: vi.fn(),
  getWorktreeStatus: vi.fn(),
  listProjectBranches: vi.fn(),
  registerProject: vi.fn(),
  signalTerminal: vi.fn(),
  saveSession: vi.fn().mockResolvedValue(undefined),
  loadSession: vi.fn().mockResolvedValue(null),
  clearSession: vi.fn().mockResolvedValue(undefined),
  listTerminalSessions: vi.fn().mockResolvedValue([]),
  spawnTerminal: vi.fn().mockResolvedValue("mock-spawn-id"),
  onNewTerminalTabMenu: vi.fn(),
  onTerminalLifecycle: vi.fn().mockResolvedValue(() => {}),
  onTerminalOutput: vi.fn().mockResolvedValue(() => {}),
  menuHandler: null as null | (() => void),
}));

const workspace = vi.hoisted(() => ({
  activateTab: vi.fn(),
  activatePrimary: vi.fn(),
  closeTab: vi.fn(),
  splitPane: vi.fn(),
  closePane: vi.fn(),
  focusPane: vi.fn(),
  setPaneRatio: vi.fn(),
  swapPanes: vi.fn(),
  ensureTabForWorktree: vi.fn().mockResolvedValue(undefined),
  openTab: vi.fn(),
  refreshWorktrees: vi.fn(),
  syncWorktrees: vi.fn(),
  restoreWorkspace: vi.fn(),
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
    ],
  },
}));

vi.mock("./lib/tauri", () => ({
  DEFAULT_WORKSPACE_ID: "default",
  DEFAULT_TERMINAL_FONT_STACK: "monospace",
  getTerminalPreferences: () => Promise.resolve({}),
  createWorktree: native.createWorktree,
  getWorktreeStatus: native.getWorktreeStatus,
  listProjectBranches: native.listProjectBranches,
  registerProject: native.registerProject,
  signalTerminal: native.signalTerminal,
  saveSession: native.saveSession,
  loadSession: native.loadSession,
  clearSession: native.clearSession,
  listTerminalSessions: native.listTerminalSessions,
  spawnTerminal: native.spawnTerminal,
  onNewTerminalTabMenu: native.onNewTerminalTabMenu,
  onTerminalLifecycle: native.onTerminalLifecycle,
  onTerminalOutput: native.onTerminalOutput,
  toIpcError: (error: unknown) => error,
}));

vi.mock("./state/workspaceStore", () => ({
  useWorkspaceStore: () => ({
    state: workspace.storeState,
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
  }),
}));

vi.mock("./state/workspaceRuntime", () => ({
  useWorkspaceRuntime: () => ({
    refreshWorktrees: workspace.refreshWorktrees,
    reportRuntimeError: vi.fn(),
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
    projects = [],
    worktrees = [],
    activeProjectId,
  }: {
    open?: boolean;
    onAddProject?: () => void;
    onCreateWorktree: () => void;
    onSelectProject?: (project: { workspaceId: string }) => void;
    onSelectWorktree?: (worktree: { path: string }) => void;
    onToggle?: () => void;
    projects?: Array<{ workspaceId: string }>;
    worktrees?: Array<{ path: string }>;
    activeProjectId?: string;
  }) => (
    <div data-testid="mock-sidebar" data-open={open}>
      <button type="button" onClick={onToggle}>
        {open ? "Hide sidebar" : "Show sidebar"}
      </button>
      {open ? (
        <>
          <button type="button" onClick={() => onAddProject?.()}>
            Add project
          </button>
          <button type="button" onClick={onCreateWorktree}>
            Add worktree
          </button>
          <span>Active project {activeProjectId}</span>
          {/* Mirrors the real nested tree: every project row, with the active project's worktrees under it. */}
          {projects.map((project) => (
            <div key={project.workspaceId}>
              <button type="button" onClick={() => onSelectProject?.(project)}>
                Project {project.workspaceId}
              </button>
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

vi.mock("./components/CommandPalette", () => ({ CommandPalette: () => null }));
vi.mock("./components/SettingsDialog", () => ({
  SettingsDialog: ({ open, onClose }: { open: boolean; onClose: () => void }) =>
    open ? (
      <div data-testid="settings-dialog">
        <button onClick={onClose}>Close settings</button>
      </div>
    ) : null,
}));
vi.mock("./components/TerminalSplitView", () => ({
  TerminalSplitView: ({ searchLeafId }: { searchLeafId?: string | null }) => (
    <div data-testid="terminal-split-view" data-search-leaf-id={searchLeafId ?? ""} />
  ),
}));
vi.mock("./components/WorktreeDeleteDialog", () => ({ WorktreeDeleteDialog: () => null }));

import { App } from "./App";

afterEach(cleanup);

describe("App project workspace flow", () => {
  beforeEach(() => {
    localStorage.clear();
    native.createWorktree.mockReset();
    native.getWorktreeStatus.mockReset();
    native.listProjectBranches.mockReset();
    native.registerProject.mockReset();
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.signalTerminal.mockReset();
    native.listTerminalSessions.mockReset();
    native.listTerminalSessions.mockResolvedValue([]);
    native.onNewTerminalTabMenu.mockReset();
    native.menuHandler = null;
    native.onNewTerminalTabMenu.mockImplementation(async (handler: () => void) => {
      native.menuHandler = handler;
      return () => {
        if (native.menuHandler === handler) native.menuHandler = null;
      };
    });
    workspace.openTab.mockReset();
    workspace.openTab.mockResolvedValue("tab-new");
    workspace.activatePrimary.mockReset();
    workspace.ensureTabForWorktree.mockReset();
    workspace.ensureTabForWorktree.mockResolvedValue(undefined);
    workspace.refreshWorktrees.mockReset();
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
    fireEvent.click(screen.getByRole("button", { name: "Add worktree" }));

    expect(screen.getByRole("form", { name: "Add Worktree" })).toBeInTheDocument();
    await waitFor(() => expect(native.listProjectBranches).toHaveBeenCalledWith("rorca"));
    expect(screen.getByRole("combobox", { name: "Base branch" })).toBeInTheDocument();
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

  it("re-spawns dead terminal sessions when restoring after app restart", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    native.listTerminalSessions.mockResolvedValue([]);
    native.spawnTerminal.mockResolvedValue("new-spawned-session-id");

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

    await waitFor(() => expect(native.spawnTerminal).toHaveBeenCalled());
    expect(workspace.restoreWorkspace).toHaveBeenCalled();
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

  it("re-spawns only dead sessions when restoring with mixed live and dead sessions across split panes", async () => {
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

    await waitFor(() => expect(native.spawnTerminal).toHaveBeenCalledTimes(1));
    expect(native.spawnTerminal).toHaveBeenCalledWith(expect.objectContaining({
      workspaceId: "default",
    }));
  });

  it("toggles the settings dialog with Cmd+,", async () => {
    native.registerProject.mockResolvedValue({ workspaceId: "default", repoRoot: "." });
    render(<App />);

    expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();

    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(screen.getByTestId("settings-dialog")).toBeInTheDocument();

    fireEvent.keyDown(window, { key: ",", metaKey: true });
    expect(screen.queryByTestId("settings-dialog")).not.toBeInTheDocument();
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
});
