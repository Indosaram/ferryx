import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";
import type { RegisteredProject } from "../lib/types";

const native = vi.hoisted(() => ({ openNativePopupMenu: vi.fn(), revealPath: vi.fn() }));
vi.mock("../lib/nativeMenu", () => ({ openNativePopupMenu: native.openNativePopupMenu }));
vi.mock("../lib/tauri", () => ({ revealPath: native.revealPath }));
vi.mock("../lib/sshHosts", () => ({
  useSshHosts: () => ({ hosts: [{ id: "build", label: "Build machine" }] }),
  getCachedSshHosts: () => [{ id: "build", label: "Build machine" }],
}));
const project: RegisteredProject = { workspaceId: "ssh:opaque-hash", repoRoot: "/srv/repo", gitRoot: "/srv/repo", target: { kind: "ssh", hostId: "build" } };
beforeEach(() => { localStorage.clear(); vi.clearAllMocks(); native.openNativePopupMenu.mockResolvedValue(() => undefined); });
afterEach(cleanup);

it("renders a host-labelled remote root and selects its explicit backend workspace identity", () => {
  const onSelectWorktree = vi.fn();
  render(<Sidebar projects={[project]} activeProjectId={project.workspaceId} worktrees={[]} agents={[]} activePath="" onSelectWorktree={onSelectWorktree} onCreateWorktree={vi.fn()} />);
  expect(screen.getByRole("button", { name: "repo (Build machine)" })).toBeInTheDocument();
  expect(screen.queryByText(project.workspaceId)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "repo Build machine" }));
  expect(onSelectWorktree).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: project.workspaceId, path: project.repoRoot, branch: null }));
});

it("renders remote worktree branch name on the left and machine badge on the right", () => {
  const branchProject: RegisteredProject = {
    workspaceId: "ssh:branch-hash",
    repoRoot: "/srv/backend-app",
    gitRoot: "/srv/backend-app",
    gitBranch: "feature/auth-flow",
    target: { kind: "ssh", hostId: "build" },
  };
  render(
    <Sidebar
      projects={[branchProject]}
      activeProjectId={branchProject.workspaceId}
      worktrees={[]}
      agents={[]}
      activePath=""
      onSelectWorktree={vi.fn()}
      onCreateWorktree={vi.fn()}
    />,
  );
  expect(screen.getByRole("button", { name: "feature/auth-flow Build machine" })).toBeInTheDocument();
  const badge = screen.getByTestId("remote-machine-badge");
  expect(badge).toHaveTextContent("Build machine");
  expect(badge).toHaveClass("truncate");
});

it("renders a host-labelled remote root for a Windows path with backslashes", () => {
  const winProject: RegisteredProject = {
    workspaceId: "ssh:win-hash",
    repoRoot: "C:\\Users\\sook\\work\\coinbase-scalper",
    gitRoot: "C:\\Users\\sook\\work\\coinbase-scalper",
    target: { kind: "ssh", hostId: "build" },
  };
  render(
    <Sidebar
      projects={[winProject]}
      activeProjectId={winProject.workspaceId}
      worktrees={[]}
      agents={[]}
      activePath=""
      onSelectWorktree={vi.fn()}
      onCreateWorktree={vi.fn()}
    />,
  );
  expect(screen.getByRole("button", { name: "coinbase-scalper (Build machine)" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "C:\\Users\\sook\\work\\coinbase-scalper (Build machine)" })).not.toBeInTheDocument();
});

it("renders clean project name without host label for multi-machine remote project group", () => {
  const winProject: RegisteredProject = {
    workspaceId: "ssh:win-hash",
    repoRoot: "C:\\Users\\sook\\work\\PirateTalk",
    gitRoot: "C:\\Users\\sook\\work\\PirateTalk",
    gitRemote: "https://github.com/Indosaram/PirateTalk.git",
    target: { kind: "ssh", hostId: "build" },
  };
  const linuxProject: RegisteredProject = {
    workspaceId: "ssh:linux-hash",
    repoRoot: "/home/indo/projects/PirateTalk",
    gitRoot: "/home/indo/projects/PirateTalk",
    gitRemote: "https://github.com/Indosaram/PirateTalk.git",
    target: { kind: "ssh", hostId: "build" },
  };
  render(
    <Sidebar
      projects={[winProject, linuxProject]}
      activeProjectId={winProject.workspaceId}
      worktrees={[]}
      agents={[]}
      activePath=""
      onSelectWorktree={vi.fn()}
      onCreateWorktree={vi.fn()}
    />,
  );
  expect(screen.getByRole("button", { name: "PirateTalk" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "PirateTalk (Build machine)" })).not.toBeInTheDocument();
});

it("shows an enabled Add Worktree menu item for an SSH project with gitRoot and triggers creation", async () => {
  const onCreateWorktree = vi.fn();
  render(<Sidebar projects={[project]} activeProjectId={project.workspaceId} worktrees={[]} agents={[]} activePath="" onSelectWorktree={vi.fn()} onCreateWorktree={onCreateWorktree} />);
  await act(async () => { fireEvent.contextMenu(screen.getByRole("button", { name: "repo (Build machine)" })); });
  const [, entries, , onAction] = native.openNativePopupMenu.mock.calls[0];
  expect(entries).toEqual(expect.arrayContaining([
    expect.objectContaining({ id: "add-worktree", label: "Add Worktree", enabled: true }),
    expect.objectContaining({ id: "reveal", label: "Local reveal unavailable over SSH", enabled: false }),
  ]));
  act(() => { onAction("reveal"); onAction("add-worktree"); });
  expect(native.revealPath).not.toHaveBeenCalled();
  expect(onCreateWorktree).toHaveBeenCalledOnce();
  native.openNativePopupMenu.mockClear();
  fireEvent.contextMenu(screen.getByRole("button", { name: "repo Build machine" }));
  expect(native.openNativePopupMenu).not.toHaveBeenCalled();
});

it("disables Add Worktree for a remote project without gitRoot", async () => {
  const noGitProject: RegisteredProject = { ...project, gitRoot: null };
  render(<Sidebar projects={[noGitProject]} activeProjectId={noGitProject.workspaceId} worktrees={[]} agents={[]} activePath="" onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
  await act(async () => { fireEvent.contextMenu(screen.getByRole("button", { name: "repo (Build machine)" })); });
  const [, entries] = native.openNativePopupMenu.mock.calls[0];
  expect(entries).toEqual(expect.arrayContaining([
    expect.objectContaining({ id: "add-worktree", enabled: false }),
  ]));
});

it("falls back to the configured host ID rather than a workspace hash when the host is missing", () => {
  render(<Sidebar projects={[{ ...project, target: { kind: "ssh", hostId: "removed" } }]} activeProjectId={project.workspaceId} worktrees={[]} agents={[]} activePath="" onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />);
  expect(screen.getByRole("button", { name: "repo (removed)" })).toBeInTheDocument();
});

it("groups matching remote project under existing local project as a remote worktree", () => {
  const localProject: RegisteredProject = {
    workspaceId: "my-app",
    repoRoot: "/Users/dev/my-app",
    gitRoot: "/Users/dev/my-app",
    gitRemote: "https://github.com/org/my-app.git",
  };
  const remoteProject: RegisteredProject = {
    workspaceId: "ssh:remote-app",
    repoRoot: "/srv/my-app",
    gitRoot: "/srv/my-app",
    gitRemote: "https://github.com/org/my-app.git",
    target: { kind: "ssh", hostId: "build" },
  };
  const localWorktree = {
    path: "/Users/dev/my-app",
    branch: "main",
    head: "111",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
  const onSelectWorktree = vi.fn();

  render(
    <Sidebar
      projects={[localProject, remoteProject]}
      activeProjectId={localProject.workspaceId}
      worktrees={[localWorktree]}
      agents={[]}
      activePath={localWorktree.path}
      onSelectWorktree={onSelectWorktree}
      onCreateWorktree={vi.fn()}
    />,
  );

  expect(screen.getByRole("button", { name: "my-app" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "my-app (Build machine)" })).not.toBeInTheDocument();

  expect(screen.getByText("main")).toBeInTheDocument();
  expect(screen.getByText("Build machine")).toBeInTheDocument();
  expect(screen.getByText("/srv/my-app")).toBeInTheDocument();

  fireEvent.click(screen.getByText("Build machine"));
  expect(onSelectWorktree).toHaveBeenCalledWith(
    expect.objectContaining({
      workspaceId: remoteProject.workspaceId,
      path: remoteProject.repoRoot,
    }),
  );
});

it("groups remote project under local project when git remote matches even if folder names differ", () => {
  const localProject: RegisteredProject = {
    workspaceId: "frontend-local",
    repoRoot: "/Users/dev/frontend-local",
    gitRoot: "/Users/dev/frontend-local",
    gitRemote: "git@github.com:org/frontend.git",
  };
  const remoteProject: RegisteredProject = {
    workspaceId: "ssh:frontend-remote",
    repoRoot: "/var/www/frontend-prod",
    gitRoot: "/var/www/frontend-prod",
    gitRemote: "https://github.com/org/frontend",
    target: { kind: "ssh", hostId: "build" },
  };
  const onSelectWorktree = vi.fn();

  render(
    <Sidebar
      projects={[localProject, remoteProject]}
      activeProjectId={localProject.workspaceId}
      worktrees={[]}
      agents={[]}
      activePath=""
      onSelectWorktree={onSelectWorktree}
      onCreateWorktree={vi.fn()}
    />,
  );

  expect(screen.getByRole("button", { name: "frontend-local" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "frontend-prod (Build machine)" })).not.toBeInTheDocument();

  expect(screen.getByText("Build machine")).toBeInTheDocument();
  expect(screen.getByText("/var/www/frontend-prod")).toBeInTheDocument();
});

it("preserves local worktrees in the sidebar and accurately highlights remote worktree when active", () => {
  const localProject: RegisteredProject = {
    workspaceId: "shared-app",
    repoRoot: "/srv/shared-app",
    gitRoot: "/srv/shared-app",
    gitRemote: "https://github.com/org/shared-app.git",
  };
  const remoteProject: RegisteredProject = {
    workspaceId: "ssh:shared-remote",
    repoRoot: "/srv/shared-app",
    gitRoot: "/srv/shared-app",
    gitRemote: "https://github.com/org/shared-app.git",
    target: { kind: "ssh", hostId: "build" },
  };
  const localWorktree = {
    path: "/srv/shared-app",
    branch: "main",
    head: "111",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
  const remoteWorktree = {
    path: "/srv/shared-app",
    branch: null,
    head: "222",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
    workspaceId: remoteProject.workspaceId,
    hostLabel: "Build machine",
  };

  render(
    <Sidebar
      projects={[localProject, remoteProject]}
      activeProjectId={remoteProject.workspaceId}
      worktrees={[remoteWorktree]}
      inactiveProjectWorktrees={{ [localProject.workspaceId]: [localWorktree] }}
      agents={[]}
      activePath={remoteProject.repoRoot}
      onSelectWorktree={vi.fn()}
      onCreateWorktree={vi.fn()}
    />,
  );

  // Both local and remote worktrees must appear in the unified project group
  expect(screen.getByText("main")).toBeInTheDocument();
  expect(screen.getByText("Build machine")).toBeInTheDocument();

  // Remote worktree is active, local worktree is NOT active despite identical paths
  const localRow = screen.getByRole("button", { name: /main/ });
  expect(localRow).not.toHaveAttribute("aria-current");

  const remoteRow = screen.getByRole("button", { name: /Build machine/ });
  expect(remoteRow).toHaveAttribute("aria-current", "true");
});
