import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActiveAgent, Worktree } from "../lib/types";
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
  it("renders worktree dirty status without a manual refresh control", () => {
    const onSelect = vi.fn();
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath={worktree.path}
        agents={[]}
        statuses={{ [worktree.path]: { isDirty: true, files: [{ statusCode: "M", path: "src/main.ts" }] } }}
        onSelect={onSelect}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByText("Dirty · 1 file")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /refresh worktree status/i })).not.toBeInTheDocument();
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("renders the aggregated child activity as one live StatusDot", () => {
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        unreadWorktreePaths={{ [worktree.path]: true }}
        activityByWorktreePath={{
          [worktree.path]: {
            workingCount: 2,
            waitingCount: 1,
            doneCount: 1,
            runningCount: 2,
            hasWorking: true,
            hasWaiting: true,
            hasDone: true,
            hasUnread: true,
          },
        }}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    const dot = screen.getByTestId("worktree-status-dot");
    expect(dot).toHaveAttribute("data-activity-state", "waiting");
    expect(dot.querySelector('[data-status-state="waiting"]')).toBeInTheDocument();
    expect(dot.querySelectorAll("[data-status-state]")).toHaveLength(1);
  });

  it("uses unread ahead of done, but clears unread presentation for the active worktree", () => {
    const activity = {
      workingCount: 0,
      waitingCount: 0,
      doneCount: 1,
      runningCount: 0,
      hasWorking: false,
      hasWaiting: false,
      hasDone: true,
      hasUnread: true,
    };
    const { rerender } = render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        activityByWorktreePath={{ [worktree.path]: activity }}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(screen.getByTestId("worktree-status-dot")).toHaveAttribute("data-activity-state", "unread");

    rerender(
      <WorktreeList
        worktrees={[worktree]}
        activePath={worktree.path}
        agents={[]}
        statuses={{}}
        activityByWorktreePath={{ [worktree.path]: activity }}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );
    expect(screen.getByTestId("worktree-status-dot")).toHaveAttribute("data-activity-state", "done");
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
        onDelete={vi.fn()}
      />,
    );

    // Only the root worktree carries the primary badge.
    expect(screen.getAllByText("primary")).toHaveLength(1);

    const [rootDelete, featureDelete] = screen.getAllByRole("button", { name: "Delete worktree" });
    expect(rootDelete).toBeDisabled();
    expect(featureDelete).toBeEnabled();
  });

  it("renders nothing when a project has no worktrees", () => {
    const { container } = render(
      <WorktreeList
        worktrees={[]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByText(/No Git worktrees/i)).toBeNull();
  });

  it("normalizes ferryx and rorca branch names to main (F11)", () => {
    const ferryxWorktree: Worktree = {
      path: "/repo/ferryx",
      head: "def456",
      branch: "refs/heads/ferryx",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };
    const rorcaWorktree: Worktree = {
      path: "/repo/rorca",
      head: "ghi789",
      branch: "refs/heads/rorca",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    render(
      <WorktreeList
        worktrees={[ferryxWorktree, rorcaWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    const mainLabels = screen.getAllByText("main");
    expect(mainLabels).toHaveLength(2);
  });

  it("indexes active agents by path with Map lookup instead of agents.find linear scan (F-shell-02)", () => {
    const sourcePath = resolve(__dirname, "WorktreeList.tsx");
    const source = readFileSync(sourcePath, "utf-8");

    // When: checking WorktreeList implementation
    // Then: it must not perform linear search agents.find inside render loop
    expect(source).not.toContain("agents.find");
    expect(source).toContain("agentsByPath");
  });

  it("keeps the agent-driven status dot but renders no agent text line", () => {
    const activeAgent: ActiveAgent = {
      id: "agent-1",
      name: "Coder",
      task: "Fix perf regressions",
      worktreePath: worktree.path,
      state: "working",
      worktree: null,
      sessionId: "sess-1",
    };

    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[activeAgent]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.queryByText("Coder")).not.toBeInTheDocument();
    expect(screen.queryByText("Fix perf regressions")).not.toBeInTheDocument();
    expect(screen.getByTestId("worktree-status-dot")).toHaveAttribute("data-activity-state", "working");
  });

  it("renders plain-folder root rows like normal worktrees without an unavailable message", () => {
    const plainRoot: Worktree = {
      path: "/Users/dev/superwiki-mail-otp",
      head: "",
      branch: null,
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    render(
      <WorktreeList
        worktrees={[plainRoot]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.getByText("superwiki-mail-otp")).toBeInTheDocument();
    expect(screen.queryByText(/unavailable for non-Git/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/detached HEAD/i)).not.toBeInTheDocument();
  });
});