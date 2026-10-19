import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import type { Worktree } from "../lib/types";
import { Sidebar } from "./Sidebar";

const defaultWorktree: Worktree = {
  path: "/repos/default/feature",
  head: "abc123",
  branch: "refs/heads/orca/default/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const qaWorktree: Worktree = {
  ...defaultWorktree,
  path: "/repos/qa/regression",
  branch: "refs/heads/orca/qa/regression",
};

const projects = [
  { workspaceId: "default", repoRoot: "/repos/default", gitRoot: "/repos/default" },
  { workspaceId: "qa", repoRoot: "/repos/qa", gitRoot: "/repos/qa" },
];

function summary(overrides: Partial<ActivitySummary>): ActivitySummary {
  return {
    workingCount: 0,
    waitingCount: 0,
    doneCount: 0,
    runningCount: 0,
    hasWorking: false,
    hasWaiting: false,
    hasDone: false,
    hasUnread: false,
    ...overrides,
  };
}

beforeEach(() => localStorage.clear());
afterEach(cleanup);

describe("Sidebar project activity", () => {
  it("shows project attention for unseen done without a bell unread flag", () => {
    render(<Sidebar projects={projects} activeProjectId="default"
      worktrees={[defaultWorktree]} agents={[]} activePath={defaultWorktree.path}
      activityByWorktreePath={{ [defaultWorktree.path]: summary({ doneCount: 1, hasDone: true }) }}
      onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} onOpenCommandPalette={vi.fn()} />);
    const project = screen.getByRole("button", { name: "default" });
    expect(within(project).getByTestId("project-attention-indicator")).toHaveAttribute("data-attention-state", "done");
  });

  it("shows one state slot per project header: needs input beats running, unread stands alone", () => {
    render(
      <Sidebar
        projects={projects}
        activeProjectId="default"
        worktrees={[defaultWorktree, qaWorktree]}
        agents={[]}
        activePath={defaultWorktree.path}
        activityByWorktreePath={{
          [defaultWorktree.path]: summary({
            workingCount: 2,
            waitingCount: 1,
            runningCount: 2,
            hasWorking: true,
            hasWaiting: true,
          }),
          [qaWorktree.path]: summary({
            doneCount: 1,
            hasDone: true,
            hasUnread: true,
          }),
        }}
        unreadWorktreePaths={{ [qaWorktree.path]: true }}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        onOpenCommandPalette={vi.fn()}
      />,
    );

    const defaultProject = screen.getByRole("button", { name: "default" });
    // (a) Waiting outranks running in one project: a single slot, labelled "needs input".
    expect(within(defaultProject).queryByTestId("project-running-badge")).not.toBeInTheDocument();
    expect(within(defaultProject).getByTestId("project-attention-indicator")).toHaveAttribute(
      "data-attention-state",
      "waiting",
    );
    expect(within(defaultProject).getByTestId("project-attention-indicator")).toHaveTextContent("needs input");

    const qaProject = screen.getByRole("button", { name: "qa" });
    expect(within(qaProject).queryByTestId("project-running-badge")).not.toBeInTheDocument();
    expect(within(qaProject).getByTestId("project-attention-indicator")).toHaveAttribute(
      "data-attention-state",
      "unread",
    );
  });

  it("labels the running slot with the count only when more than one agent runs", () => {
    const { rerender } = render(
      <Sidebar projects={[projects[0]]} activeProjectId="default" worktrees={[defaultWorktree]} agents={[]}
        activePath={defaultWorktree.path}
        activityByWorktreePath={{ [defaultWorktree.path]: summary({ workingCount: 2, runningCount: 2, hasWorking: true }) }}
        onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />,
    );
    expect(within(screen.getByRole("button", { name: "default" })).getByTestId("project-running-badge")).toHaveTextContent("2 running");
    rerender(
      <Sidebar projects={[projects[0]]} activeProjectId="default" worktrees={[defaultWorktree]} agents={[]}
        activePath={defaultWorktree.path}
        activityByWorktreePath={{ [defaultWorktree.path]: summary({ workingCount: 1, runningCount: 1, hasWorking: true }) }}
        onSelectWorktree={vi.fn()} onCreateWorktree={vi.fn()} />,
    );
    expect(within(screen.getByRole("button", { name: "default" })).getByTestId("project-running-badge")).toHaveTextContent(/^running$/);
  });

  it("keeps the per-project add-worktree action outside the project's activity row button", () => {
    const onCreateWorktree = vi.fn();
    render(
      <Sidebar
        projects={projects}
        activeProjectId="default"
        worktrees={[defaultWorktree, qaWorktree]}
        agents={[]}
        activePath={defaultWorktree.path}
        activityByWorktreePath={{
          [defaultWorktree.path]: summary({ workingCount: 1, runningCount: 1, hasWorking: true }),
        }}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={onCreateWorktree}
        onOpenCommandPalette={vi.fn()}
      />,
    );

    // The badge/indicator row button stays clean; creation lives beside it, not inside it.
    const defaultProject = screen.getByRole("button", { name: "default" });
    expect(within(defaultProject).queryByRole("button")).not.toBeInTheDocument();
    expect(within(defaultProject).getByTestId("project-running-badge")).toHaveTextContent(/^running$/);

    fireEvent.click(screen.getByRole("button", { name: "Add worktree to qa" }));
    expect(onCreateWorktree).toHaveBeenCalledExactlyOnceWith(projects[1]);
  });

  it("does not invent project activity for idle worktrees", () => {
    render(
      <Sidebar
        projects={[projects[0]]}
        activeProjectId="default"
        worktrees={[defaultWorktree]}
        agents={[]}
        activePath={defaultWorktree.path}
        activityByWorktreePath={{ [defaultWorktree.path]: summary({}) }}
        onSelectWorktree={vi.fn()}
        onCreateWorktree={vi.fn()}
        onOpenCommandPalette={vi.fn()}
      />,
    );

    const project = screen.getByRole("button", { name: "default" });
    expect(within(project).queryByTestId("project-running-badge")).not.toBeInTheDocument();
    expect(within(project).queryByTestId("project-attention-indicator")).not.toBeInTheDocument();
  });
});
