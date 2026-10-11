import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Sidebar } from "./Sidebar";
import { updateListingFailures, worktreeListingFailureStore } from "../state/worktreeListingFailures";

const failureStore = {
  set: (next: Record<string, string>) => updateListingFailures(() => next),
};

const projects = [
  { workspaceId: "default", repoRoot: "/repos/default", gitRoot: "/repos/default" },
  { workspaceId: "qa", repoRoot: "/repos/qa", gitRoot: "/repos/qa" },
];

beforeEach(() => {
  localStorage.clear();
  failureStore.set({});
});
afterEach(() => {
  cleanup();
  worktreeListingFailureStore.reset();
});

describe("Sidebar worktree listing failure badge", () => {
  it("badges only the project whose worktrees could not be loaded, and clears when it recovers", () => {
    render(<Sidebar projects={projects} activeProjectId="default" worktrees={[]} agents={[]} activePath={null}
      onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} onOpenCommandPalette={vi.fn()} />);
    const qa = screen.getByRole("button", { name: "qa" });
    const defaultProject = screen.getByRole("button", { name: "default" });
    expect(within(qa).queryByTestId("project-worktrees-unavailable")).not.toBeInTheDocument();

    act(() => failureStore.set({ qa: "Connection reset by peer" }));
    expect(within(qa).getByTestId("project-worktrees-unavailable")).toHaveAttribute(
      "title",
      "Couldn't load worktrees: Connection reset by peer",
    );
    expect(within(defaultProject).queryByTestId("project-worktrees-unavailable")).not.toBeInTheDocument();

    act(() => failureStore.set({}));
    expect(within(qa).queryByTestId("project-worktrees-unavailable")).not.toBeInTheDocument();
  });
});
