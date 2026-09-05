import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, SIDEBAR_WIDTH_STORAGE_KEY, Sidebar } from "./Sidebar";

const nativeMenu = vi.hoisted(() => ({
  openNativePopupMenu: vi.fn(),
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: nativeMenu.openNativePopupMenu,
}));

function lastSidebarMenuCall(): { items: Array<{ kind: string; id?: string; label?: string; enabled?: boolean }>; onAction: (id: string) => void } {
  const calls = nativeMenu.openNativePopupMenu.mock.calls;
  const last = calls[calls.length - 1];
  return { items: last[1], onAction: last[3] };
}

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const defaultWorktree: Worktree = {
  path: "/repos/default/feature",
  head: "def456",
  branch: "refs/heads/orca/default/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const qaWorktree: Worktree = {
  path: "/repos/rorca-qa/regression",
  head: "789abc",
  branch: "refs/heads/orca/rorca-qa/regression",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const accordionProjects = [
  { workspaceId: "default", repoRoot: "/repos/default", gitRoot: "/repos/default" },
  { workspaceId: "rorca-qa", repoRoot: "/repos/rorca-qa", gitRoot: "/repos/rorca-qa" },
];

function projectRow(workspaceId: string) {
  return screen.getByRole("button", { name: workspaceId });
}

function projectToggle(workspaceId: string) {
  return screen.getByRole("button", { name: new RegExp(`^(Expand|Collapse) ${workspaceId}$`) });
}

beforeEach(() => localStorage.clear());
afterEach(cleanup);

const projects = [
  { workspaceId: "maho-workspace", repoRoot: "/repos/maho-workspace", gitRoot: "/repos/maho-workspace" },
  { workspaceId: "content-intel-dashboard", repoRoot: "/repos/content-intel-dashboard", gitRoot: "/repos/content-intel-dashboard" },
];

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    worktrees: [worktree],
    agents: [],
    activePath: worktree.path,
    onSelectWorktree: vi.fn(),
    onCreateWorktree: vi.fn(),
    onAddProject: vi.fn(),
    onOpenSettings: vi.fn(),
    ...overrides,
  } as any;
}

function renderSidebar(overrides: Record<string, unknown> = {}) {
  return render(<Sidebar {...baseProps(overrides)} />);
}

describe("Sidebar navigation", () => {
  it("renders cached worktrees for a newly active project while active store rows are temporarily empty", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const cachedWorktree: Worktree = {
      path: "/repos/alpha/feature",
      head: "123456",
      branch: "refs/heads/orca/alpha/feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
    ];

    renderSidebar({
      projects,
      activeProjectId: "alpha",
      worktrees: [],
      inactiveProjectWorktrees: { alpha: [cachedWorktree] },
    });

    const alphaList = screen.getByRole("list", { name: "alpha worktrees" });
    expect(within(alphaList).getByRole("button", { name: /feature/ })).toBeInTheDocument();
  });

  it("lists an inactive project's own rows so index-based selection can reach them", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const projects = [
      { workspaceId: "alpha", repoRoot: "/repos/alpha", gitRoot: "/repos/alpha" },
      { workspaceId: "plain", repoRoot: "/repos/plain", gitRoot: null },
    ];
    const alphaRoot: Worktree = {
      path: "/repos/alpha",
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    renderSidebar({
      projects,
      activeProjectId: "alpha",
      worktrees: [alphaRoot],
      activePath: alphaRoot.path,
    });

    // Both groups render rows, so Cmd+N ordering spans alpha then plain.
    expect(within(screen.getByRole("list", { name: "alpha worktrees" })).getAllByRole("button")).not.toHaveLength(0);
    expect(within(screen.getByRole("list", { name: "plain worktrees" })).getByText("plain")).toBeInTheDocument();
  });

  it("renders the folder row for an active non-Git project even before the store lists it", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const plainProject = { workspaceId: "plain", repoRoot: "/repos/plain", gitRoot: null };

    // A plain project has no git worktrees, so the store list is empty. The
    // sidebar must still offer its folder root as a selectable row.
    renderSidebar({
      projects: [plainProject],
      activeProjectId: "plain",
      worktrees: [],
      activePath: "",
    });

    const plainList = screen.getByRole("list", { name: "plain worktrees" });
    expect(within(plainList).getByText("plain")).toBeInTheDocument();
  });

  it("attributes a branch-less root worktree to the project that owns its path, not the active project", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const mixedProjects = [
      { workspaceId: "orca-lite", repoRoot: "/repos/orca-lite", gitRoot: "/repos/orca-lite" },
      { workspaceId: "superwiki", repoRoot: "/repos/superwiki", gitRoot: null },
    ];
    const superwikiRoot: Worktree = {
      path: "/repos/superwiki",
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    renderSidebar({
      projects: mixedProjects,
      activeProjectId: "orca-lite",
      worktrees: [superwikiRoot],
      activePath: superwikiRoot.path,
    });

    const superwikiList = screen.getByRole("list", { name: "superwiki worktrees" });
    expect(within(superwikiList).getByText("superwiki")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "orca-lite worktrees" })).not.toBeInTheDocument();
  });

  it("collapses and expands a project from its name button, not just the chevron", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const onSelectProject = vi.fn();
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree],
      onSelectProject,
    });

    expect(screen.getByRole("list", { name: "default worktrees" })).toBeInTheDocument();

    fireEvent.click(projectRow("default"));

    expect(screen.queryByRole("list", { name: "default worktrees" })).not.toBeInTheDocument();
    expect(onSelectProject).toHaveBeenCalledTimes(1);

    fireEvent.click(projectRow("default"));

    expect(screen.getByRole("list", { name: "default worktrees" })).toBeInTheDocument();
  });

  it("does not highlight an inactive project's row when the active path is stale", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const qaRow = {
      path: "/repos/rorca-qa/regression",
      head: "789abc",
      branch: "refs/heads/orca/rorca-qa/regression",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    } as any;

    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [],
      inactiveProjectWorktrees: { "rorca-qa": [qaRow] },
      activePath: qaRow.path,
    });

    const qaList = screen.getByRole("list", { name: "rorca-qa worktrees" });
    expect(within(qaList).queryByRole("button", { current: true })).toBeNull();
  });

  it("highlights the active worktree only inside the project that owns it", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    const sharedPath = "/repos/shared-folder";
    const activeRoot = {
      path: sharedPath,
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    } as any;

    renderSidebar({
      projects: [
        { workspaceId: "alpha", repoRoot: sharedPath, gitRoot: null },
        { workspaceId: "beta", repoRoot: "/repos/beta", gitRoot: "/repos/beta" },
      ],
      activeProjectId: "alpha",
      worktrees: [activeRoot],
      inactiveProjectWorktrees: { beta: [activeRoot] },
      activePath: sharedPath,
    });

    const alphaList = screen.getByRole("list", { name: "alpha worktrees" });
    const betaList = screen.getByRole("list", { name: "beta worktrees" });

    expect(within(alphaList).getByRole("button", { current: true })).toBeInTheDocument();
    expect(within(betaList).queryByRole("button", { current: true })).toBeNull();
  });

  it("lists inactive project worktrees instead of the empty-state message", () => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify([]));
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree],
      inactiveProjectWorktrees: { "rorca-qa": [qaWorktree] },
    });

    const qaList = screen.getByRole("list", { name: "rorca-qa worktrees" });
    expect(within(qaList).getByText("regression")).toBeInTheDocument();
    expect(screen.queryByText(/No Git worktrees/i)).not.toBeInTheDocument();
  });

  it("uses the 236px parity width by default", () => {
    renderSidebar();
    expect(screen.getByRole("complementary")).toHaveStyle({ width: "236px" });
  });

  it("keeps only global actions in the sidebar titlebar", () => {
    const onAddProject = vi.fn();
    const onCreateWorktree = vi.fn();
    renderSidebar({ projects, activeProjectId: "maho-workspace", onAddProject, onCreateWorktree });

    expect(screen.queryByText("Agents")).not.toBeInTheDocument();
    expect(screen.queryByText("Active agents")).not.toBeInTheDocument();
    expect(screen.queryByText("Workspace")).not.toBeInTheDocument();
    expect(screen.queryByText("Search workspaces")).not.toBeInTheDocument();
    expect(screen.queryByText("Projects")).not.toBeInTheDocument();

    const titlebar = screen.getByTestId("sidebar-drag-region");
    expect(within(titlebar).getByRole("button", { name: "Hide sidebar" })).toBeInTheDocument();

    // Worktree creation is a per-project action, so the global titlebar must not offer it.
    expect(within(titlebar).queryByRole("button", { name: /add worktree/i })).not.toBeInTheDocument();

    fireEvent.click(within(titlebar).getByRole("button", { name: "Add project" }));
    expect(onAddProject).toHaveBeenCalledOnce();
    expect(onCreateWorktree).not.toHaveBeenCalled();
  });

  it("gives every project row its own add-worktree action bound to that exact project", () => {
    const onCreateWorktree = vi.fn();
    const onSelectProject = vi.fn();
    renderSidebar({ projects, activeProjectId: "maho-workspace", onCreateWorktree, onSelectProject });

    for (const project of projects) {
      expect(screen.getByRole("button", { name: `Add worktree to ${project.workspaceId}` })).toBeInTheDocument();
    }

    // The collapsed, non-active project still exposes creation, and passes its own project object.
    fireEvent.click(screen.getByRole("button", { name: "Add worktree to content-intel-dashboard" }));
    expect(onCreateWorktree).toHaveBeenCalledExactlyOnceWith(projects[1]);

    onCreateWorktree.mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Add worktree to maho-workspace" }));
    expect(onCreateWorktree).toHaveBeenCalledExactlyOnceWith(projects[0]);

    // Creating from a project row is not a selection or a collapse toggle.
    expect(onSelectProject).not.toHaveBeenCalled();
    expect(projectToggle("content-intel-dashboard")).toHaveAttribute("aria-expanded", "false");
  });

  it("nests each project's worktrees inside that project's own tree group", () => {
    renderSidebar({ projects, activeProjectId: "maho-workspace" });

    expect(screen.queryByText("Projects")).not.toBeInTheDocument();
    expect(screen.queryByText("Worktrees")).not.toBeInTheDocument();

    const activeToggle = projectToggle("maho-workspace");
    const collapsedToggle = projectToggle("content-intel-dashboard");
    expect(activeToggle).toHaveAttribute("aria-expanded", "true");
    expect(collapsedToggle).toHaveAttribute("aria-expanded", "false");

    // The worktree list is a descendant of the expanded project's group, not a sibling section.
    const group = screen.getByRole("list", { name: "maho-workspace worktrees" });
    expect(projectRow("maho-workspace").closest("div")?.parentElement).toContainElement(group);
    expect(group).toContainElement(screen.getByRole("button", { name: /main/ }));

    // The collapsed project renders no worktrees of its own.
    expect(screen.queryByRole("list", { name: "content-intel-dashboard worktrees" })).not.toBeInTheDocument();
  });

  it("selects a project from its tree row and a worktree from inside the nested group", () => {
    const onSelectProject = vi.fn();
    const onSelectWorktree = vi.fn();
    renderSidebar({ projects, activeProjectId: "maho-workspace", onSelectProject, onSelectWorktree });

    fireEvent.click(projectRow("content-intel-dashboard"));
    expect(onSelectProject).toHaveBeenCalledWith(projects[1]);

    const group = screen.getByRole("list", { name: "maho-workspace worktrees" });
    fireEvent.click(within(group).getByRole("button", { name: /main/ }));
    expect(onSelectWorktree).toHaveBeenCalledWith(worktree);
  });

  it("keeps each project's open state independent when toggling default or rorca-qa", () => {
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
      activePath: defaultWorktree.path,
    });

    expect(projectToggle("default")).toHaveAttribute("aria-expanded", "true");
    expect(projectToggle("rorca-qa")).toHaveAttribute("aria-expanded", "false");

    // Opening rorca-qa leaves default open.
    fireEvent.click(projectToggle("rorca-qa"));
    expect(projectToggle("rorca-qa")).toHaveAttribute("aria-expanded", "true");
    expect(projectToggle("default")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("list", { name: "default worktrees" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "rorca-qa worktrees" })).toBeInTheDocument();

    // Closing default leaves rorca-qa open.
    fireEvent.click(projectToggle("default"));
    expect(projectToggle("default")).toHaveAttribute("aria-expanded", "false");
    expect(projectToggle("rorca-qa")).toHaveAttribute("aria-expanded", "true");
    expect(screen.queryByRole("list", { name: "default worktrees" })).not.toBeInTheDocument();
    expect(screen.getByRole("list", { name: "rorca-qa worktrees" })).toBeInTheDocument();

    // Reopening default does not close rorca-qa.
    fireEvent.click(projectToggle("default"));
    expect(projectToggle("default")).toHaveAttribute("aria-expanded", "true");
    expect(projectToggle("rorca-qa")).toHaveAttribute("aria-expanded", "true");
  });

  it("toggling a project does not change which project is active", () => {
    const onSelectProject = vi.fn();
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
      onSelectProject,
    });

    fireEvent.click(projectToggle("rorca-qa"));
    expect(onSelectProject).not.toHaveBeenCalled();
    expect(projectRow("default")).toHaveAttribute("aria-current", "true");
    expect(projectRow("rorca-qa")).not.toHaveAttribute("aria-current");
  });

  it("persists collapsed projects across remounts", () => {
    const { unmount } = renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
    });

    fireEvent.click(projectToggle("default"));
    fireEvent.click(projectToggle("rorca-qa"));
    expect(JSON.parse(localStorage.getItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY) ?? "[]")).toEqual(["default"]);

    unmount();
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
    });

    expect(projectToggle("default")).toHaveAttribute("aria-expanded", "false");
    expect(projectToggle("rorca-qa")).toHaveAttribute("aria-expanded", "true");
  });

  it("routes each worktree under the project that owns its branch", () => {
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
      activePath: defaultWorktree.path,
    });

    fireEvent.click(projectToggle("rorca-qa"));

    const defaultGroup = screen.getByRole("list", { name: "default worktrees" });
    const qaGroup = screen.getByRole("list", { name: "rorca-qa worktrees" });

    expect(within(defaultGroup).getByRole("button", { name: /feature/ })).toBeInTheDocument();
    expect(within(defaultGroup).queryByRole("button", { name: /regression/ })).not.toBeInTheDocument();
    expect(within(qaGroup).getByRole("button", { name: /regression/ })).toBeInTheDocument();
    expect(within(qaGroup).queryByRole("button", { name: /feature/ })).not.toBeInTheDocument();
  });

  it("hooks worktree creation and deletion into the owning project group", () => {
    const onCreateWorktree = vi.fn();
    const onDeleteWorktree = vi.fn();
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
      activePath: defaultWorktree.path,
      onCreateWorktree,
      onDeleteWorktree,
    });

    fireEvent.click(projectToggle("rorca-qa"));
    const qaGroup = screen.getByRole("list", { name: "rorca-qa worktrees" });

    fireEvent.click(within(qaGroup).getByRole("button", { name: "Delete worktree" }));
    expect(onDeleteWorktree).toHaveBeenCalledWith(qaWorktree);

    fireEvent.click(screen.getByRole("button", { name: "Add worktree to rorca-qa" }));
    expect(onCreateWorktree).toHaveBeenCalledExactlyOnceWith(accordionProjects[1]);
  });

  it("drops the per-worktree status refresh control", () => {
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree, qaWorktree],
      activePath: defaultWorktree.path,
      statuses: { [defaultWorktree.path]: { isDirty: true, files: [{ statusCode: "M", path: "src/main.ts" }] } },
    });

    expect(screen.queryByRole("button", { name: /refresh worktree status/i })).not.toBeInTheDocument();

    // Dirty state and deletion survive the refresh removal.
    expect(screen.getByText("Dirty · 1 file")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete worktree" })).toBeInTheDocument();
  });

  it("renders no empty-state row or message for a project with no worktrees", () => {
    const onCreateWorktree = vi.fn();
    renderSidebar({
      projects: accordionProjects,
      activeProjectId: "default",
      worktrees: [defaultWorktree],
      onCreateWorktree,
    });

    fireEvent.click(projectToggle("rorca-qa"));

    expect(screen.queryByRole("list", { name: "rorca-qa worktrees" })).not.toBeInTheDocument();
    expect(screen.queryByText(/No Git worktrees/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /create the first worktree/i })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Add worktree to rorca-qa" }));
    expect(onCreateWorktree).toHaveBeenCalledExactlyOnceWith(accordionProjects[1]);
  });

  it("marks the active worktree inside the nested group as current", () => {
    renderSidebar({ projects, activeProjectId: "maho-workspace", activePath: worktree.path });

    const group = screen.getByRole("list", { name: "maho-workspace worktrees" });
    expect(within(group).getByRole("button", { name: /main/ })).toHaveAttribute("aria-current", "true");
  });

  it("keeps all titlebar actions out of the Tauri drag region", () => {
    renderSidebar();
    const dragBackground = screen.getByTestId("sidebar-drag-region");
    expect(dragBackground).toHaveAttribute("data-tauri-drag-region");
    for (const name of ["Hide sidebar", "Add project"]) {
      expect(screen.getByRole("button", { name })).toHaveClass("no-drag");
    }
    expect(screen.getByRole("separator", { name: "Resize sidebar" })).toHaveClass("no-drag");
  });

  it("restores, drags, clamps, and persists sidebar width", () => {
    localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, "390");
    renderSidebar();

    const sidebar = screen.getByRole("complementary");
    const handle = screen.getByRole("separator", { name: "Resize sidebar" });
    expect(sidebar).toHaveStyle({ width: "390px" });

    fireEvent.pointerDown(handle, { clientX: 390, pointerId: 1 });
    fireEvent.pointerMove(window, { clientX: 600, pointerId: 1 });
    expect(sidebar).toHaveStyle({ width: "420px" });
    fireEvent.pointerUp(window, { clientX: 600, pointerId: 1 });
    expect(localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe("420");

    fireEvent.pointerDown(handle, { clientX: 420, pointerId: 2 });
    fireEvent.pointerMove(window, { clientX: 40, pointerId: 2 });
    expect(sidebar).toHaveStyle({ width: "220px" });
    fireEvent.pointerUp(window, { clientX: 40, pointerId: 2 });
    expect(localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe("220");
  });

  it("renders exactly one visible sidebar divider without redundant aside border", () => {
    renderSidebar();
    const sidebar = screen.getByRole("complementary");
    expect(sidebar).not.toHaveClass("border-r");
    expect(sidebar).not.toHaveClass("border-worktree-sidebar-border");

    const separator = screen.getByRole("separator", { name: "Resize sidebar" });
    expect(separator).toBeInTheDocument();
    expect(separator.querySelector(".bg-worktree-sidebar-border")).toBeInTheDocument();
  });

  it("calls onToggle or onHide when Hide sidebar button is clicked", () => {
    const onToggle = vi.fn();
    renderSidebar({ onToggle });

    fireEvent.click(screen.getByRole("button", { name: "Hide sidebar" }));
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it("renders null when open is false", () => {
    const { container } = renderSidebar({ open: false });
    expect(container).toBeEmptyDOMElement();
  });

  it("initializes knownProjectsRef lazily without allocating a new Set on every render (F-shell-03)", () => {
    const sourcePath = resolve(__dirname, "Sidebar.tsx");
    const source = readFileSync(sourcePath, "utf-8");

    // When: checking Sidebar knownProjectsRef initialization
    // Then: it must not allocate new Set(projects.map(...)) as an eager useRef parameter
    expect(source).not.toContain("useRef<Set<string>>(new Set");
    expect(source).not.toMatch(/useRef<Set<string>>\(\s*new Set/);
  });

  it("hides add-worktree button when project gitRoot is null and shows it when gitRoot is set", () => {
    const mixedProjects = [
      { workspaceId: "git-proj", repoRoot: "/repos/git-proj", gitRoot: "/repos/git-proj" },
      { workspaceId: "plain-proj", repoRoot: "/repos/plain-proj", gitRoot: null },
    ];
    renderSidebar({ projects: mixedProjects, activeProjectId: "git-proj" });

    expect(screen.getByRole("button", { name: "Add worktree to git-proj" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add worktree to plain-proj" })).not.toBeInTheDocument();
  });

  it("triggers onRemoveProject from project native context menu Remove Project action", () => {
    const onRemoveProject = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    const testProjects = [
      { workspaceId: "proj-1", repoRoot: "/repos/proj-1", gitRoot: "/repos/proj-1" },
    ];
    renderSidebar({ projects: testProjects, onRemoveProject });

    const projectButton = screen.getByRole("button", { name: "proj-1" });
    fireEvent.contextMenu(projectButton, { clientX: 50, clientY: 50 });

    expect(nativeMenu.openNativePopupMenu).toHaveBeenCalledTimes(1);
    const { items, onAction } = lastSidebarMenuCall();
    const removeItem = items.find((item) => item.id === "remove");
    expect(removeItem?.label).toBe("Remove Project");
    expect(removeItem?.enabled).toBe(true);

    onAction("remove");
    expect(onRemoveProject).toHaveBeenCalledWith(testProjects[0]);
  });

  it("triggers onRemoveProject from project row trash icon button", () => {
    const onRemoveProject = vi.fn();
    const testProjects = [
      { workspaceId: "proj-1", repoRoot: "/repos/proj-1", gitRoot: "/repos/proj-1" },
    ];
    renderSidebar({ projects: testProjects, onRemoveProject });

    const trashBtn = screen.getByRole("button", { name: "Remove project proj-1" });
    expect(trashBtn).toBeInTheDocument();

    fireEvent.click(trashBtn);
    expect(onRemoveProject).toHaveBeenCalledWith(testProjects[0]);
  });
});
