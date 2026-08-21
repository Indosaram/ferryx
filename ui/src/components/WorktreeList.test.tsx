import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { WorktreeList } from "./WorktreeList";

const worktree: Worktree = {
  path: "/repo/feature",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const rootWorktree: Worktree = {
  path: "/repo",
  head: "abc123",
  branch: "refs/heads/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

afterEach(cleanup);

describe("WorktreeList actions", () => {
  it("refreshes and renders worktree dirty status without selecting the card", () => {
    const onSelect = vi.fn();
    const onRefreshStatus = vi.fn();
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath={worktree.path}
        agents={[]}
        statuses={{ [worktree.path]: { isDirty: true, files: [{ statusCode: "M", path: "src/main.ts" }] } }}
        onSelect={onSelect}
        onCreate={vi.fn()}
        onRefreshStatus={onRefreshStatus}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByText("Dirty · 1 file")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh worktree status" }));
    expect(onRefreshStatus).toHaveBeenCalledWith(worktree);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("opens deletion from the worktree card action", () => {
    const onDelete = vi.fn();
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath={worktree.path}
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onCreate={vi.fn()}
        onRefreshStatus={vi.fn()}
        onDelete={onDelete}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete worktree" }));
    expect(onDelete).toHaveBeenCalledWith(worktree);
  });

  it("badges the repository root as primary and blocks deleting it", () => {
    render(
      <WorktreeList
        worktrees={[rootWorktree, worktree]}
        activePath={rootWorktree.path}
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onCreate={vi.fn()}
        onRefreshStatus={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    // Only the root worktree carries the primary badge.
    expect(screen.getAllByText("primary")).toHaveLength(1);

    const [rootDelete, featureDelete] = screen.getAllByRole("button", { name: "Delete worktree" });
    expect(rootDelete).toBeDisabled();
    expect(featureDelete).toBeEnabled();
  });

  it("offers worktree creation from the empty state", () => {
    const onCreate = vi.fn();
    render(
      <WorktreeList
        worktrees={[]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onCreate={onCreate}
        onRefreshStatus={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /create the first worktree/i }));
    expect(onCreate).toHaveBeenCalledOnce();
  });
});
