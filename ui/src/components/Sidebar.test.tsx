import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { Sidebar } from "./Sidebar";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

afterEach(cleanup);

describe("Sidebar navigation", () => {
  it("removes agent orchestration affordances and opens search through the real callback", () => {
    const onOpenCommandPalette = vi.fn();
    render(
      <Sidebar
        worktrees={[worktree]}
        agents={[]}
        activePath={worktree.path}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        onOpenCommandPalette={onOpenCommandPalette}
        onOpenSettings={vi.fn()}
      />,
    );

    expect(screen.queryByText("Agents")).not.toBeInTheDocument();
    expect(screen.queryByText("Active agents")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /search workspaces/i }));
    expect(onOpenCommandPalette).toHaveBeenCalledOnce();
  });

  it("focuses the worktree region from Workspace", () => {
    render(
      <Sidebar
        worktrees={[worktree]}
        agents={[]}
        activePath={worktree.path}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        onOpenCommandPalette={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    expect(screen.getByTestId("worktree-region")).toHaveFocus();
  });
});
