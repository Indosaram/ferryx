import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActiveAgent, Worktree } from "../lib/types";
import {
  WorktreeList,
  isPrimaryWorktree,
  isSameWorktreePath,
  normalizeWorktreePath,
} from "./WorktreeList";
import { SidebarWorktreesEmpty } from "./sidebar/SidebarWorktreeRow";
import { SIDEBAR_EMPTY_TEXT } from "./sidebar/sidebarLabels";

const nativeMenu = vi.hoisted(() => ({
  openNativePopupMenu: vi.fn(),
}));

vi.mock("../lib/nativeMenu", () => ({
  openNativePopupMenu: nativeMenu.openNativePopupMenu,
}));

const toastMocks = vi.hoisted(() => ({ info: vi.fn(), success: vi.fn(), error: vi.fn() }));

vi.mock("sonner", () => ({ toast: toastMocks }));

function lastMenuCall(): { items: Array<{ kind: string; id?: string; label?: string; enabled?: boolean }>; onAction: (id: string) => void } {
  const calls = nativeMenu.openNativePopupMenu.mock.calls;
  const last = calls[calls.length - 1];
  return { items: last[1], onAction: last[3] };
}

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
  it("keeps a dirty worktree row selectable without rendering git dirty status or a refresh control", () => {
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

    expect(screen.queryByText(/Dirty/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /refresh worktree status/i })).not.toBeInTheDocument();
    expect(onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("feature"));
    expect(onSelect).toHaveBeenCalledExactlyOnceWith(worktree);
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
    expect(screen.getByTestId("worktree-status-dot").querySelector('[data-status-state="unread"]')).toBeInTheDocument();

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
    expect(screen.getByTestId("worktree-status-dot").querySelector('[data-status-state="done"]')).toBeInTheDocument();
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

  it("renders no primary badge and still omits the delete action for the repository root", () => {
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

    // Root-worktree status is logic only: no badge text.
    expect(screen.queryByText("primary")).not.toBeInTheDocument();

    // Primary worktree does not render a delete button; only non-primary worktrees expose delete.
    const deleteButtons = screen.getAllByRole("button", { name: "Delete worktree" });
    expect(deleteButtons).toHaveLength(1);
    expect(deleteButtons[0]).toBeEnabled();
  });

  it("triggers worktree creation from the worktree row action when provided", () => {
    const onCreateWorktree = vi.fn();
    render(
      <WorktreeList
        worktrees={[rootWorktree]}
        activePath={rootWorktree.path}
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onCreateWorktree={onCreateWorktree}
        onDelete={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Add worktree" }));
    expect(onCreateWorktree).toHaveBeenCalledWith(rootWorktree);
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

  it("says so and offers to add one when an empty list is given an empty state", () => {
    const onCreateWorktree = vi.fn();
    render(
      <WorktreeList
        worktrees={[]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        emptyState={<SidebarWorktreesEmpty text={SIDEBAR_EMPTY_TEXT.noWorktrees} onCreateWorktree={onCreateWorktree} />}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("No worktrees listed yet.");
    fireEvent.click(screen.getByRole("button", { name: "Add worktree" }));
    expect(onCreateWorktree).toHaveBeenCalledTimes(1);
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
    expect(screen.getByTestId("worktree-status-dot").querySelector('[data-status-state="working"]')).toBeInTheDocument();
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

  it("renders no status dot for an idle worktree", () => {
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("worktree-status-dot")).not.toBeInTheDocument();
    const row = screen.getByText("feature").closest(".group\\/worktree-row")!;
    expect(row.querySelector("[data-status-state]")).toBeNull();
  });

  it("styles the active row with the accent fill and ring rail instead of a border", () => {
    const { rerender } = render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    const inactiveLabel = screen.getByText("feature");
    expect(inactiveLabel.className).toContain("text-worktree-sidebar-foreground");
    expect(inactiveLabel.className).not.toContain("text-[#fafafa]");
    expect(screen.queryByTestId("worktree-rail-active")).not.toBeInTheDocument();

    rerender(
      <WorktreeList
        worktrees={[worktree]}
        activePath={worktree.path}
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
      />,
    );

    const activeLabel = screen.getByText("feature");
    expect(activeLabel.className).toContain("text-foreground");
    const activeRow = activeLabel.closest(".group\\/worktree-row")!;
    expect(activeRow).toHaveClass("bg-worktree-sidebar-accent");
    expect(activeRow.className).not.toContain("border-[");
    expect(activeRow.className).not.toMatch(/(^|\s)border(\s|$)/);
    expect(screen.getByTestId("worktree-rail-active")).toHaveClass("bg-worktree-sidebar-ring");
  });

  it("opens native context menu on right click and triggers deletion from context menu", () => {
    const onDelete = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
      />,
    );

    const row = screen.getByText("feature").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });

    expect(nativeMenu.openNativePopupMenu).toHaveBeenCalledTimes(1);
    const { items, onAction } = lastMenuCall();
    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.label).toBe("Delete Worktree");
    expect(deleteItem?.enabled).toBe(true);

    onAction("delete");
    expect(onDelete).toHaveBeenCalledWith(worktree);
  });

  it("disables Delete Worktree in native context menu for primary worktrees", () => {
    const onDelete = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    render(
      <WorktreeList
        worktrees={[rootWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
      />,
    );

    const row = screen.getByText("main").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });

    const { items } = lastMenuCall();
    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.label).toBe("Delete Worktree");
    expect(deleteItem?.enabled).toBe(false);
  });

  it("includes reset-agent-state in context menu and calls onResetAgentState", () => {
    const onResetAgentState = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    render(
      <WorktreeList
        worktrees={[worktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onResetAgentState={onResetAgentState}
      />,
    );

    const row = screen.getByText("feature").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });

    expect(nativeMenu.openNativePopupMenu).toHaveBeenCalledTimes(1);
    const { items, onAction } = lastMenuCall();
    const resetItem = items.find((item) => item.id === "reset-agent-state");
    expect(resetItem?.label).toBe("Reset Agent State");

    onAction("reset-agent-state");
    expect(onResetAgentState).toHaveBeenCalledWith(worktree);
  });

  it("displays managed slug for remote worktree with orca identity and renders + button", () => {
    const onCreateWorktree = vi.fn();
    const onDelete = vi.fn();
    const remoteWorktree: Worktree = {
      workspaceId: "ssh:opaque-hash",
      path: "/srv/repo/.orca-worktrees/wt-my-slug",
      head: "def456",
      branch: "refs/heads/orca/ssh-123456/my-slug",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Build machine",
    };

    render(
      <WorktreeList
        worktrees={[remoteWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
        onCreateWorktree={onCreateWorktree}
      />,
    );

    // Displays managed slug rather than hostLabel
    expect(screen.getByText("my-slug")).toBeInTheDocument();
    expect(screen.getByTestId("remote-machine-badge")).toHaveTextContent("Build machine");

    // Renders the + button for remote worktree
    const addBtn = screen.getByRole("button", { name: "Add worktree" });
    expect(addBtn).toBeInTheDocument();
    fireEvent.click(addBtn);
    expect(onCreateWorktree).toHaveBeenCalledWith(remoteWorktree);

    // Delete button should be rendered for remote child worktree
    const deleteBtn = screen.getByRole("button", { name: "Delete worktree" });
    expect(deleteBtn).toBeInTheDocument();
    fireEvent.click(deleteBtn);
    expect(onDelete).toHaveBeenCalledWith(remoteWorktree);
  });

  it("enables Delete Worktree in native context menu for remote child worktree", () => {
    const onDelete = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    const remoteWorktree: Worktree = {
      workspaceId: "ssh:opaque-hash",
      path: "/srv/repo/.orca-worktrees/wt-my-slug",
      head: "def456",
      branch: "refs/heads/orca/ssh-123456/my-slug",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Build machine",
    };

    render(
      <WorktreeList
        worktrees={[remoteWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
      />,
    );

    const row = screen.getByText("my-slug").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });

    const { items, onAction } = lastMenuCall();
    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.label).toBe("Delete Worktree");
    expect(deleteItem?.enabled).toBe(true);

    onAction("delete");
    expect(onDelete).toHaveBeenCalledWith(remoteWorktree);
  });

  it("offers Remove Project in native context menu for remote primary repo root", () => {
    const onDelete = vi.fn();
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    const remoteRootWorktree: Worktree = {
      workspaceId: "ssh:opaque-hash",
      path: "/srv/repo",
      head: "abc123",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Build machine",
    };

    render(
      <WorktreeList
        worktrees={[remoteRootWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
      />,
    );

    const row = screen.getByText("main").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 100, clientY: 100 });

    const { items, onAction } = lastMenuCall();
    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.label).toBe("Remove Project");
    expect(deleteItem?.enabled).toBe(true);

    onAction("delete");
    expect(onDelete).toHaveBeenCalledWith(remoteRootWorktree);

    // Hover button should have "Remove project" label
    expect(screen.getByRole("button", { name: "Remove project" })).toBeInTheDocument();
  });

  it("renders branch on the left and hostLabel as machine badge on the right for remote worktree", () => {
    const remoteRootWorktree: Worktree = {
      workspaceId: "ssh:opaque-hash",
      path: "/srv/repo",
      head: "abc123",
      branch: "refs/heads/main",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Build machine",
    };

    render(
      <WorktreeList
        worktrees={[remoteRootWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onCreateWorktree={vi.fn()}
      />,
    );

    expect(screen.getByText("main")).toBeInTheDocument();
    const badge = screen.getByTestId("remote-machine-badge");
    expect(badge).toHaveTextContent("Build machine");
    expect(badge).toHaveClass("truncate");
    expect(screen.queryByText("SSH")).not.toBeInTheDocument();
  });

  it("renders a stale row that explains itself instead of opening, with its actions disabled", () => {
    const onSelect = vi.fn();
    const onCreateWorktree = vi.fn();
    const onDelete = vi.fn();

    const staleWorktree: Worktree & { stale?: boolean; disabled?: boolean } = {
      workspaceId: "ws-123456",
      identity: { wsId: "ws-123456", slug: "stale-worktree" },
      path: "/srv/repo/stale-worktree",
      head: "def456",
      branch: "refs/heads/orca/ws-123456/stale-worktree",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Remote Host",
      hostSummary: "Offline (stale)",
      stale: true,
      disabled: true,
    };

    render(
      <WorktreeList
        worktrees={[staleWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={onSelect}
        onDelete={onDelete}
        onCreateWorktree={onCreateWorktree}
      />,
    );

    const row = screen.getByText("stale-worktree").closest(".group\\/worktree-row")!;
    expect(row).toHaveAttribute("data-stale", "true");

    // The select control stays clickable so it can explain itself, but it must not open the row.
    toastMocks.info.mockClear();
    const selectBtn = row.querySelector("button[data-shortcut-worktree-path]")!;
    expect(selectBtn).toHaveAttribute("aria-disabled", "true");
    expect(selectBtn.getAttribute("title")).toContain("Offline (stale)");
    fireEvent.click(selectBtn);
    expect(onSelect).not.toHaveBeenCalled();
    expect(toastMocks.info).toHaveBeenCalledTimes(1);
    expect(toastMocks.info.mock.calls[0][0]).toContain("reconnects");

    // Add worktree button must be disabled
    const addBtn = screen.getByRole("button", { name: "Add worktree" });
    expect(addBtn).toBeDisabled();
    fireEvent.click(addBtn);
    expect(onCreateWorktree).not.toHaveBeenCalled();

    // Delete worktree button must be disabled
    const deleteBtn = screen.getByRole("button", { name: "Delete worktree" });
    expect(deleteBtn).toBeDisabled();
    fireEvent.click(deleteBtn);
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("keeps the right-click menu on a stale row with only the read-only actions enabled", () => {
    const staleWorktree: Worktree & { stale?: boolean; disabled?: boolean } = {
      workspaceId: "ws-123456",
      identity: { wsId: "ws-123456", slug: "stale-worktree" },
      path: "/srv/repo/stale-worktree",
      head: "def456",
      branch: "refs/heads/orca/ws-123456/stale-worktree",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      hostLabel: "Remote Host",
      hostSummary: "Offline (stale)",
      stale: true,
      disabled: true,
    };

    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    const before = nativeMenu.openNativePopupMenu.mock.calls.length;

    render(
      <WorktreeList
        worktrees={[staleWorktree]}
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onResetAgentState={vi.fn()}
      />,
    );

    const row = screen.getByText("stale-worktree").closest(".group\\/worktree-row")!;
    fireEvent.contextMenu(row, { clientX: 10, clientY: 10 });

    expect(nativeMenu.openNativePopupMenu.mock.calls.length).toBe(before + 1);
    const { items } = lastMenuCall();

    const copyPathItem = items.find((item) => item.id === "copy-path");
    expect(copyPathItem).toBeDefined();
    expect(copyPathItem?.enabled).not.toBe(false);

    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.enabled).toBe(false);

    const resetAgentStateItem = items.find((item) => item.id === "reset-agent-state");
    expect(resetAgentStateItem?.enabled).toBe(false);

    const revealItem = items.find((item) => item.id === "reveal");
    expect(revealItem?.enabled).toBe(false);
  });

  it("identifies primary worktree by matching repoRoot path and makes external non-root worktrees deletable", () => {
    const onDelete = vi.fn();
    const externalWorktree: Worktree = {
      path: "/repo-external/custom-feature",
      head: "ext123",
      branch: "refs/heads/feature/custom-feature", // not an orca/... branch!
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    render(
      <WorktreeList
        worktrees={[rootWorktree, externalWorktree]}
        repoRoot="/repo"
        activePath=""
        agents={[]}
        statuses={{}}
        onSelect={vi.fn()}
        onDelete={onDelete}
      />,
    );

    // rootWorktree matches repoRoot: marked primary, no delete button
    const rootRow = screen.getByText("main").closest(".group\\/worktree-row")!;
    expect(rootRow.textContent).not.toContain("primary");
    expect(rootRow.querySelector('button[aria-label="Delete worktree"]')).toBeNull();

    // externalWorktree path does NOT match repoRoot: NOT primary, delete button enabled
    const extRow = screen.getByText("feature/custom-feature").closest(".group\\/worktree-row")!;
    expect(extRow.textContent).not.toContain("primary");
    const deleteBtn = extRow.querySelector('button[aria-label="Delete worktree"]');
    expect(deleteBtn).not.toBeNull();
    fireEvent.click(deleteBtn!);
    expect(onDelete).toHaveBeenCalledWith(externalWorktree);

    // Context menu on externalWorktree has Delete Worktree enabled
    nativeMenu.openNativePopupMenu.mockResolvedValue(() => undefined);
    fireEvent.contextMenu(extRow, { clientX: 100, clientY: 100 });
    const { items, onAction } = lastMenuCall();
    const deleteItem = items.find((item) => item.id === "delete");
    expect(deleteItem?.label).toBe("Delete Worktree");
    expect(deleteItem?.enabled).toBe(true);

    onAction("delete");
    expect(onDelete).toHaveBeenCalledWith(externalWorktree);
  });

  it("falls back to branch-shape check when no repoRoot is provided", () => {
    const externalWorktree: Worktree = {
      path: "/repo-external/custom-feature",
      head: "ext123",
      branch: "refs/heads/feature/custom-feature",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
    };

    // When repoRoot is not provided, branch-shape fallback applies
    expect(isPrimaryWorktree(rootWorktree)).toBe(true);
    expect(isPrimaryWorktree(externalWorktree)).toBe(true);
    expect(isPrimaryWorktree(worktree)).toBe(false);

    // When repoRoot IS provided, path determines primary
    expect(isPrimaryWorktree(rootWorktree, "/repo")).toBe(true);
    expect(isPrimaryWorktree(externalWorktree, "/repo")).toBe(false);
    expect(isPrimaryWorktree(worktree, "/repo")).toBe(false);
  });

  it("normalizes paths for comparison across trailing separators, slashes, and case", () => {
    // Trailing slashes
    expect(isSameWorktreePath("/repo/main/", "/repo/main")).toBe(true);
    expect(isSameWorktreePath("/repo/main///", "/repo/main")).toBe(true);

    // Windows backslashes
    expect(isSameWorktreePath("C:\\projects\\ferryx", "C:/projects/ferryx")).toBe(true);
    expect(isSameWorktreePath("C:\\projects\\ferryx\\", "C:/projects/ferryx")).toBe(true);

    // Case-insensitivity when enabled (Windows and macOS)
    expect(normalizeWorktreePath("C:\\Projects\\Ferryx", true)).toBe("c:/projects/ferryx");
    expect(isSameWorktreePath("C:\\Projects\\Ferryx", "c:/projects/ferryx", true)).toBe(true);
    expect(isSameWorktreePath("/Volumes/T9-Mac/Repo", "/volumes/t9-mac/repo", true)).toBe(true);

    // Case-preservation when disabled (Linux)
    expect(normalizeWorktreePath("/home/user/Repo", false)).toBe("/home/user/Repo");
    expect(isSameWorktreePath("/home/user/Repo", "/home/user/repo", false)).toBe(false);
  });
});
