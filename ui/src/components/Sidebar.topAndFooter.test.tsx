import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ActivitySummary } from "../lib/activity";
import type { Worktree } from "../lib/types";
import { SIDEBAR_COLLAPSED_PROJECTS_STORAGE_KEY, Sidebar, type SidebarAttention } from "./Sidebar";

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

function attentionProps(overrides: Partial<SidebarAttention> = {}): SidebarAttention {
  return {
    rows: [{
      id: "row-1", revision: 1, workspaceId: "ferryx", sessionId: "sess-1", state: "needs-you",
      who: "omo", location: "ferryx / main", text: "Ship it now?", at: Date.now(),
    }],
    onOpen: vi.fn(),
    onDismiss: vi.fn(),
    onDismissAll: vi.fn(),
    openSessionCount: 3,
    inboxOpen: false,
    onInboxOpenChange: vi.fn(),
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

describe("Sidebar top: inbox segment", () => {
  it("toggles the worktree region and aria-selected through the Inbox tab", () => {
    const attention = attentionProps();
    const view = render(<Sidebar {...props({ attention })} />);

    // Role/name queries stay scoped to the small tablist and titlebar subtrees: whole-document
    // accessible-name scans over the dnd-kit tree are what made this test slow.
    const tablist = () => screen.getByRole("tablist");
    const titlebar = () => screen.getByTestId("sidebar-drag-region");
    expect(tablist()).toHaveAttribute("aria-label", "Sidebar view");
    const worktreesTab = within(tablist()).getByRole("tab", { name: "Worktrees" });
    const inboxTab = within(tablist()).getByRole("tab", { name: "Inbox (1)" });
    expect(worktreesTab).toHaveAttribute("aria-selected", "true");
    expect(inboxTab).toHaveAttribute("aria-selected", "false");
    expect(inboxTab).toHaveAttribute("data-shortcut", "notifications.toggle");
    expect(within(inboxTab).getByTestId("sidebar-attention-badge")).toHaveTextContent("1");
    expect(screen.getByTestId("worktree-region")).not.toHaveAttribute("hidden");
    expect(within(titlebar()).queryByRole("button", { name: "Mark all read" })).toBeNull();

    fireEvent.click(inboxTab);
    expect(attention.onInboxOpenChange).toHaveBeenCalledWith(true);

    view.rerender(<Sidebar {...props({ attention: { ...attention, inboxOpen: true } })} />);
    expect(within(tablist()).getByRole("tab", { name: "Inbox (1)" })).toHaveAttribute("aria-selected", "true");
    expect(within(tablist()).getByRole("tab", { name: "Worktrees" })).toHaveAttribute("aria-selected", "false");
    expect(screen.getByTestId("worktree-region")).toHaveAttribute("hidden");
    expect(within(titlebar()).getByRole("button", { name: "Mark all read" })).toBeInTheDocument();

    // Arrow-key roving moves selection back to Worktrees.
    fireEvent.keyDown(within(tablist()).getByRole("tab", { name: "Inbox (1)" }), { key: "ArrowLeft" });
    expect(attention.onInboxOpenChange).toHaveBeenLastCalledWith(false);
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
