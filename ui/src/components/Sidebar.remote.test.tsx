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
  fireEvent.click(screen.getByRole("button", { name: `${project.repoRoot} SSH root` }));
  expect(onSelectWorktree).toHaveBeenCalledWith(expect.objectContaining({ workspaceId: project.workspaceId, path: project.repoRoot, branch: null }));
});

it("disables remote Git and reveal menu actions and rejects even stale native callbacks", async () => {
  const onCreateWorktree = vi.fn();
  render(<Sidebar projects={[project]} activeProjectId={project.workspaceId} worktrees={[]} agents={[]} activePath="" onSelectWorktree={vi.fn()} onCreateWorktree={onCreateWorktree} />);
  await act(async () => { fireEvent.contextMenu(screen.getByRole("button", { name: "repo (Build machine)" })); });
  const [, entries, , onAction] = native.openNativePopupMenu.mock.calls[0];
  expect(entries).toEqual(expect.arrayContaining([
    expect.objectContaining({ id: "add-worktree", enabled: false }),
    expect.objectContaining({ id: "reveal", enabled: false }),
  ]));
  act(() => { onAction("reveal"); onAction("add-worktree"); });
  expect(native.revealPath).not.toHaveBeenCalled();
  expect(onCreateWorktree).not.toHaveBeenCalled();
  native.openNativePopupMenu.mockClear();
  fireEvent.contextMenu(screen.getByRole("button", { name: `${project.repoRoot} SSH root` }));
  expect(native.openNativePopupMenu).not.toHaveBeenCalled();
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
  };
  const remoteProject: RegisteredProject = {
    workspaceId: "ssh:remote-app",
    repoRoot: "/srv/my-app",
    gitRoot: "/srv/my-app",
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
  };
  const remoteProject: RegisteredProject = {
    workspaceId: "ssh:shared-remote",
    repoRoot: "/srv/shared-app",
    gitRoot: "/srv/shared-app",
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
