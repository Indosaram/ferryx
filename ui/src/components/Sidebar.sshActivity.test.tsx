import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { summarizeActivities, type TerminalActivityState } from "../lib/activity";
import type { RegisteredProject } from "../lib/types";
import { Sidebar } from "./Sidebar";

vi.mock("../lib/sshHosts", () => ({
  useSshHosts: () => ({ hosts: [{ id: "build", label: "Build machine" }] }),
  getCachedSshHosts: () => [{ id: "build", label: "Build machine" }],
}));

beforeEach(() => localStorage.clear());
afterEach(cleanup);

it.each(["/srv/repo", "C:\\work\\repo"])(
  "keeps SSH working visible on the root and collapsed header for %s",
  (path) => {
    const project: RegisteredProject = {
      workspaceId: "ssh:build",
      repoRoot: path,
      gitRoot: path,
      target: { kind: "ssh", hostId: "build" },
    };
    const onSelectWorktree = vi.fn();
    render(
      <Sidebar projects={[project]} activeProjectId={project.workspaceId}
        worktrees={[]} agents={[]} activePath={path}
        activityByWorktreePath={{ [path]: summarizeActivities([{ state: "working", title: "omo", isAgent: true }]) }}
        onSelectWorktree={onSelectWorktree} onCreateWorktree={vi.fn()} />,
    );
    const header = screen.getByTestId("project-running-badge").closest("button");
    if (!header) throw new Error("Missing project header");
    const root = document.querySelector('[data-shortcut-workspace-id="ssh:build"]');
    if (!root) throw new Error("Missing SSH worktree row");
    expect(root.querySelector('[data-status-state="working"]')).toHaveClass("animate-spin");
    expect(header.querySelector('[data-status-state="working"]')).toHaveClass("animate-spin");
    fireEvent.click(root);
    expect(onSelectWorktree).toHaveBeenCalledWith(expect.objectContaining({
      workspaceId: project.workspaceId, path,
    }));
    fireEvent.click(header);
    expect(document.querySelector('[data-shortcut-workspace-id="ssh:build"]')).not.toBeInTheDocument();
    expect(header.querySelector('[data-status-state="working"]')).toHaveClass("animate-spin");
  },
);

it.each(["waiting", "done"] satisfies TerminalActivityState[])(
  "shows unseen %s on the SSH root instead of a stale spinner",
  (state) => {
    const project: RegisteredProject = {
      workspaceId: "ssh:build", repoRoot: "/srv/repo", gitRoot: "/srv/repo",
      target: { kind: "ssh", hostId: "build" },
    };
    const props = {
      projects: [project], activeProjectId: project.workspaceId,
      worktrees: [], agents: [], activePath: project.repoRoot,
      onSelectWorktree: vi.fn(), onCreateWorktree: vi.fn(),
    };
    const { rerender } = render(<Sidebar {...props} activityByWorktreePath={{
      [project.repoRoot]: summarizeActivities([{ state: "working", title: "omo", isAgent: true }]),
    }} />);
    rerender(<Sidebar {...props} activityByWorktreePath={{
      [project.repoRoot]: summarizeActivities([
        { state: "working", title: "another agent", isAgent: true },
        { state, title: "omo", isAgent: true, seen: false },
      ]),
    }} />);
    const root = document.querySelector('[data-shortcut-workspace-id="ssh:build"]');
    if (!root) throw new Error("Missing SSH worktree row");
    expect(root.querySelector(`[data-status-state="${state}"]`)).toBeInTheDocument();
    expect(root.querySelector('[data-status-state="working"]')).not.toBeInTheDocument();
    const header = screen.getByTestId("project-attention-indicator").closest("button");
    if (!header) throw new Error("Missing project header");
    expect(header.querySelector(`[data-status-state="${state}"]`)).toBeInTheDocument();
    expect(header.querySelector('[data-status-state="working"]')).not.toBeInTheDocument();
  },
);
