import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Sidebar } from "./Sidebar";
import type { RegisteredProject } from "../lib/types";

const native = vi.hoisted(() => ({ openNativePopupMenu: vi.fn(), revealPath: vi.fn() }));
vi.mock("../lib/nativeMenu", () => ({ openNativePopupMenu: native.openNativePopupMenu }));
vi.mock("../lib/tauri", () => ({ revealPath: native.revealPath }));
vi.mock("../lib/sshHosts", () => ({ useSshHosts: () => ({ hosts: [{ id: "build", label: "Build machine" }] }) }));
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
