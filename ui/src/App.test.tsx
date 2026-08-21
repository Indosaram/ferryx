import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  createWorktree: vi.fn(),
  getWorktreeStatus: vi.fn(),
  listProjectBranches: vi.fn(),
  registerProject: vi.fn(),
  signalTerminal: vi.fn(),
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
  createWorktree: native.createWorktree,
  getWorktreeStatus: native.getWorktreeStatus,
  listProjectBranches: native.listProjectBranches,
  registerProject: native.registerProject,
  signalTerminal: native.signalTerminal,
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
  SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY: "rorca.sidebar.collapsedProjects",
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
vi.mock("./components/SettingsDialog", () => ({ SettingsDialog: () => null }));
vi.mock("./components/TerminalSplitView", () => ({ TerminalSplitView: () => null }));
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
    native.signalTerminal.mockReset();
    workspace.activatePrimary.mockReset();
    workspace.ensureTabForWorktree.mockReset();
    workspace.ensureTabForWorktree.mockResolvedValue(undefined);
    workspace.refreshWorktrees.mockReset();
  });

  it("re-registers the active persisted project before refreshing its worktrees", async () => {
    const project = { workspaceId: "rorca", repoRoot: "/repos/rorca" };
    localStorage.setItem("rorca.projects", JSON.stringify([project]));
    localStorage.setItem("rorca.active-project", project.workspaceId);
    native.registerProject.mockResolvedValue(project);

    render(<App />);

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: project.workspaceId,
        repoPath: project.repoRoot,
      }),
    );
    await waitFor(() => expect(workspace.refreshWorktrees).toHaveBeenCalledOnce());
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
    localStorage.setItem("rorca.projects", JSON.stringify(projects));
    localStorage.setItem("rorca.active-project", "alpha");
    native.registerProject.mockImplementation(async ({ workspaceId }: { workspaceId: string }) => ({
      workspaceId,
      repoRoot: `/repos/${workspaceId}`,
    }));

    render(<App />);

    expect(screen.getByText("Active project alpha")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Project beta" }));

    await waitFor(() => expect(screen.getByText("Active project beta")).toBeInTheDocument());
    expect(localStorage.getItem("rorca.active-project")).toBe("beta");
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
    localStorage.setItem("rorca.projects", JSON.stringify(projects));
    localStorage.setItem("rorca.active-project", "alpha");
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
    localStorage.setItem("rorca.projects", JSON.stringify([project]));
    localStorage.setItem("rorca.active-project", "alpha");
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
    localStorage.setItem("rorca.projects", JSON.stringify(projects));
    localStorage.setItem("rorca.active-project", "alpha");
    // Both projects expanded in the sidebar accordion.
    localStorage.setItem("rorca.sidebar.collapsedProjects", JSON.stringify([]));
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
      localStorage.setItem("rorca.sidebar.collapsedProjects", JSON.stringify(["alpha"]));
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
    localStorage.setItem("rorca.projects", JSON.stringify([{ workspaceId: "default", repoRoot: "." }]));
    localStorage.setItem("rorca.active-project", "default");
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
    expect(localStorage.getItem("orca.sidebar.open")).toBe("false");

    // Toggle on via Show sidebar button in header
    fireEvent.click(screen.getByRole("button", { name: "Show sidebar" }));
    expect(screen.getByTestId("mock-sidebar")).toBeInTheDocument();
    expect(localStorage.getItem("orca.sidebar.open")).toBe("true");

    // Toggle off via Hide sidebar button in sidebar
    fireEvent.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(screen.queryByTestId("mock-sidebar")).not.toBeInTheDocument();
    expect(localStorage.getItem("orca.sidebar.open")).toBe("false");

    unmount();

    // Rerender loads persisted false state
    render(<App />);
    expect(screen.queryByTestId("mock-sidebar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show sidebar" })).toBeInTheDocument();
  });
});
