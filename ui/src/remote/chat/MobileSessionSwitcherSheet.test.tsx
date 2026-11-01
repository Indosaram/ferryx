import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteTerminalTabInfo } from "../../lib/tauri";
import { MobileSessionSwitcherSheet } from "./MobileSessionSwitcherSheet";

beforeEach(cleanup);
afterEach(cleanup);

const sampleTabs: RemoteTerminalTabInfo[] = [
  {
    id: "tab-1",
    label: "Shell 1",
    activityState: "working",
    worktreeLabel: "main",
    worktreeSlug: "main",
  },
  {
    id: "tab-2",
    label: "Review Pane",
    activityState: "waiting",
    worktreeLabel: "feature-branch",
    worktreeSlug: "feature-branch",
    agentType: "omo",
  },
  {
    id: "tab-3",
    label: "Build Done",
    activityState: "done",
    worktreeLabel: "main",
    worktreeSlug: "main",
  },
];

describe("MobileSessionSwitcherSheet", () => {
  it("renders closed when open is false", () => {
    render(
      <MobileSessionSwitcherSheet
        open={false}
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByText("Shell 1")).not.toBeInTheDocument();
  });

  it("renders tabs with labels and correct foreign-worktree descriptions", () => {
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    expect(screen.getByRole("dialog", { name: "Sessions" })).toBeInTheDocument();
    expect(screen.getByText("Shell 1")).toBeInTheDocument();
    expect(screen.getByText("Review Pane")).toBeInTheDocument();
    expect(screen.getByText("Build Done")).toBeInTheDocument();

    // tab-2 is on 'feature-branch' while current is 'main' -> foreign worktree description rendered
    expect(screen.getByText("feature-branch")).toBeInTheDocument();
    // tab-1 and tab-3 are on 'main' -> no foreign worktree description
    expect(screen.queryAllByText("main")).toHaveLength(0);
  });

  it("renders status labels: Working and Needs approval", () => {
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    expect(screen.getByText("Working")).toHaveClass("text-status-working");
    expect(screen.getByText("Needs approval")).toHaveClass("text-status-warning");
    expect(screen.queryByText("Done")).not.toBeInTheDocument();
  });

  it("ui-shell-sheets-r15-1: the agent status is part of each row's accessible name", () => {
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    expect(screen.getByTestId("session-switcher-tab-tab-1")).toHaveAccessibleName(/Working/);
    expect(screen.getByTestId("session-switcher-tab-tab-2")).toHaveAccessibleName(/Approval/);
  });

  it("indicates active tab via aria-current", () => {
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-2"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    const tab1Btn = screen.getByTestId("session-switcher-tab-tab-1");
    const tab2Btn = screen.getByTestId("session-switcher-tab-tab-2");

    expect(tab1Btn).not.toHaveAttribute("aria-current");
    expect(tab2Btn).toHaveAttribute("aria-current", "true");
    expect(tab1Btn).not.toHaveAttribute("aria-selected");
    expect(tab2Btn).not.toHaveAttribute("aria-selected");
  });

  it("disables tabs and New terminal button while pending", () => {
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        pending={true}
        onSelectTab={vi.fn()}
        onNewTerminal={vi.fn()}
      />,
    );

    const tab1Btn = screen.getByTestId("session-switcher-tab-tab-1");
    const newTerminalBtn = screen.getByTestId("session-switcher-new-terminal");

    expect(tab1Btn).toBeDisabled();
    expect(newTerminalBtn).toBeDisabled();
  });

  it("disables New terminal without workspace context but keeps tabs selectable", () => {
    const onNewTerminal = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <MobileSessionSwitcherSheet open onOpenChange={onOpenChange} tabs={sampleTabs}
        disabled onSelectTab={vi.fn()} onNewTerminal={onNewTerminal} />,
    );
    const button = screen.getByTestId("session-switcher-new-terminal");
    expect(button).toBeDisabled();
    expect(screen.getByTestId("session-switcher-tab-tab-1")).not.toBeDisabled();
    fireEvent.click(button);
    expect(onNewTerminal).not.toHaveBeenCalled();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("calls onSelectTab when a tab row is clicked", () => {
    const onSelectTab = vi.fn();
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={vi.fn()}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={onSelectTab}
        onNewTerminal={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTestId("session-switcher-tab-tab-2"));
    expect(onSelectTab).toHaveBeenCalledTimes(1);
    expect(onSelectTab).toHaveBeenCalledWith(sampleTabs[1]);
  });

  it("calls onNewTerminal and closes sheet when footer New terminal button is clicked", () => {
    const onNewTerminal = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <MobileSessionSwitcherSheet
        open
        onOpenChange={onOpenChange}
        tabs={sampleTabs}
        activeTabId="tab-1"
        worktreeLabel="main"
        onSelectTab={vi.fn()}
        onNewTerminal={onNewTerminal}
      />,
    );

    fireEvent.click(screen.getByTestId("session-switcher-new-terminal"));
    expect(onNewTerminal).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

