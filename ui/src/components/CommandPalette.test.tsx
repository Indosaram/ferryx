import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TerminalTab, Worktree } from "../lib/types";
import { CommandPalette } from "./CommandPalette";

const worktrees: Worktree[] = [
  {
    path: "/repo/main",
    head: "abc123",
    branch: "refs/heads/orca/ws/main",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
  {
    path: "/repo/feature",
    head: "def456",
    branch: "refs/heads/orca/ws/feature-search",
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  },
];

const tabs: TerminalTab[] = [
  { id: "tab-main", label: "main", sessionId: "session-main" },
  { id: "tab-feature", label: "feature shell", sessionId: "session-feature" },
];

describe("CommandPalette", () => {
  it("lists worktrees and terminal tabs and switches the selected item", () => {
    const onSelectWorktree = vi.fn();
    const onSelectTab = vi.fn();
    const onClose = vi.fn();

    render(
      <CommandPalette
        open
        worktrees={worktrees}
        tabs={tabs}
        onSelectWorktree={onSelectWorktree}
        onSelectTab={onSelectTab}
        onClose={onClose}
      />,
    );

    expect(screen.getByRole("dialog", { name: "Command palette" })).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Search worktrees and terminal tabs"), { target: { value: "feature" } });
    fireEvent.click(screen.getByRole("button", { name: /feature-search/i }));
    expect(onSelectWorktree).toHaveBeenCalledWith(worktrees[1]);
    expect(onClose).toHaveBeenCalledOnce();

    fireEvent.change(screen.getByPlaceholderText("Search worktrees and terminal tabs"), { target: { value: "shell" } });
    fireEvent.click(screen.getByRole("button", { name: /feature shell/i }));
    expect(onSelectTab).toHaveBeenCalledWith("tab-feature");
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(
      <CommandPalette open worktrees={worktrees} tabs={tabs} onSelectWorktree={vi.fn()} onSelectTab={vi.fn()} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
