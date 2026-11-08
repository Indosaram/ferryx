import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import type { Worktree } from "../lib/types";
import { SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, Sidebar } from "./Sidebar";

function worktreeAt(slug: string): Worktree {
  return {
    path: `/repos/ferryx-wt/${slug}`,
    head: "abc123",
    branch: `refs/heads/orca/ferryx/${slug}`,
    bare: false,
    detached: false,
    locked: null,
    prunable: null,
  };
}

const mainRow = worktreeAt("main");
const relayRow = worktreeAt("relay-latency");
const sidebarRow = worktreeAt("sidebar-redesign");

const projects = [
  { workspaceId: "ferryx", repoRoot: "/repos/ferryx", gitRoot: "/repos/ferryx" },
  { workspaceId: "site", repoRoot: "/repos/site", gitRoot: "/repos/site" },
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

function props(overrides: Record<string, unknown> = {}) {
  return {
    projects,
    activeProjectId: "ferryx",
    worktrees: [mainRow, relayRow, sidebarRow],
    agents: [],
    activePath: mainRow.path,
    onSelectWorktree: vi.fn(),
    onCreateWorktree: vi.fn(),
    ...overrides,
  } as any;
}

beforeEach(() => localStorage.clear());
afterEach(cleanup);

describe("Sidebar top: view segment", () => {
  it("offers only Supervisor and Worktrees, the panel first, and toggles the tree through them", async () => {
    render(<Sidebar {...props({ onFocusSupervisorSession: vi.fn() })} />);

    // Role/name queries stay scoped to the small tablist and titlebar subtrees: whole-document
    // accessible-name scans over the dnd-kit tree are what made this test slow.
    const tablist = () => screen.getByRole("tablist");
    const titlebar = () => screen.getByTestId("sidebar-drag-region");
    expect(tablist()).toHaveAttribute("aria-label", "Sidebar view");
    expect(within(tablist()).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Supervisor", "Worktrees"]);
    expect(within(tablist()).getByRole("tab", { name: "Supervisor" })).toHaveAttribute("aria-selected", "true");
    expect(within(tablist()).getByRole("tab", { name: "Worktrees" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("worktree-region")).toHaveAttribute("hidden");
    expect(await screen.findByTestId("supervisor-panel")).toBeInTheDocument();
    expect(within(titlebar()).queryByRole("button", { name: "Mark all read" })).toBeNull();

    fireEvent.click(within(tablist()).getByRole("tab", { name: "Worktrees" }));
    expect(within(tablist()).getByRole("tab", { name: "Worktrees" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("worktree-region")).not.toHaveAttribute("hidden");
    // The panel stays mounted behind the tree, so coming back shows it at once.
    expect(screen.getByTestId("supervisor-panel")).not.toBeVisible();

    // Arrow-key roving moves selection back to the panel.
    fireEvent.keyDown(within(tablist()).getByRole("tab", { name: "Worktrees" }), { key: "ArrowLeft" });
    expect(within(tablist()).getByRole("tab", { name: "Supervisor" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("supervisor-panel")).toBeVisible();
    expect(screen.queryByText("Loading supervisor…")).toBeNull();
  });
});

describe("Sidebar top: worktree filter", () => {
  beforeEach(() => {
    localStorage.setItem(SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, JSON.stringify(["ferryx", "site"]));
  });

  it("shows a Projects header counting project groups", () => {
    render(<Sidebar {...props()} />);
    expect(screen.getByText("Projects").parentElement).toHaveTextContent("Projects2");
  });

  it("hides non-matching rows and expands the project owning a match", () => {
    render(<Sidebar {...props()} />);
    expect(screen.queryByRole("list", { name: "ferryx worktrees" })).toBeNull();

    fireEvent.change(screen.getByRole("searchbox", { name: "Filter worktrees" }), { target: { value: "RELAY" } });

    expect(screen.queryByRole("button", { name: "site" })).toBeNull();
    expect(screen.getByRole("button", { name: "Collapse ferryx" })).toHaveAttribute("aria-expanded", "true");
    const list = screen.getByRole("list", { name: "ferryx worktrees" });
    expect(within(list).getByText("relay-latency")).toBeInTheDocument();
    expect(within(list).queryByText("main")).toBeNull();
    expect(within(list).queryByText("sidebar-redesign")).toBeNull();
  });

  it("clears the filter on Escape and restores every project", () => {
    render(<Sidebar {...props()} />);
    const input = screen.getByRole("searchbox", { name: "Filter worktrees" });
    fireEvent.change(input, { target: { value: "relay" } });
    expect(screen.queryByRole("button", { name: "site" })).toBeNull();

    fireEvent.keyDown(input, { key: "Escape" });

    expect(input).toHaveValue("");
    expect(screen.getByRole("button", { name: "site" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "ferryx worktrees" })).toBeNull();
  });
});

describe("Sidebar status footer", () => {
  it("aggregates running and needs-input worktrees across visible projects", () => {
    render(
      <Sidebar
        {...props({
          activityByWorktreePath: {
            [mainRow.path]: summary({ workingCount: 1, runningCount: 1, hasWorking: true }),
            [sidebarRow.path]: summary({ workingCount: 1, runningCount: 1, hasWorking: true }),
            [relayRow.path]: summary({ waitingCount: 1, hasWaiting: true }),
          },
        })}
      />,
    );
    expect(screen.getByTestId("sidebar-status-footer")).toHaveTextContent(/^2 running · 1 needs input$/);
  });

  it("omits zero parts and is empty when nothing runs or waits", () => {
    const view = render(
      <Sidebar {...props({ activityByWorktreePath: { [relayRow.path]: summary({ waitingCount: 1, hasWaiting: true }) } })} />,
    );
    expect(screen.getByTestId("sidebar-status-footer")).toHaveTextContent(/^1 needs input$/);

    view.rerender(<Sidebar {...props()} />);
    expect(screen.getByTestId("sidebar-status-footer")).toHaveTextContent(/^$/);
  });
});
