import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { SIDEBAR_WIDTH_STORAGE_KEY, Sidebar } from "./Sidebar";

const worktree: Worktree = {
  path: "/repo/main",
  head: "abc123",
  branch: "refs/heads/orca/ws/main",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function renderSidebar() {
  return render(
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
}

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
    renderSidebar();

    fireEvent.click(screen.getByRole("button", { name: "Workspace" }));
    expect(screen.getByTestId("worktree-region")).toHaveFocus();
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
});
