import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActiveAgent, Worktree } from "../lib/types";
import { WorkspaceHeader } from "./WorkspaceHeader";

const nativeWindow = vi.hoisted(() => ({
  startDragging: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => nativeWindow,
}));

const worktree: Worktree = {
  path: "/repo/feature",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const agent: ActiveAgent = {
  id: "agent-1",
  name: "Shell",
  task: "feature",
  state: "working",
  worktreePath: worktree.path,
  sessionId: "backend-1",
  worktree: { wsId: "ws-main", slug: "feature" },
};

afterEach(cleanup);

describe("WorkspaceHeader", () => {
  it("is a compact drag background with interactive content excluded from dragging", () => {
    render(<WorkspaceHeader worktree={worktree} agent={agent} />);

    const header = screen.getByRole("banner");
    expect(header).toHaveAttribute("data-tauri-drag-region");
    expect(header).toHaveClass("h-titlebar");
    expect(screen.getByTestId("workspace-agent-chip")).toHaveClass("no-drag");
  });

  it("does not expose global split or interrupt terminal actions", () => {
    render(<WorkspaceHeader worktree={worktree} />);

    expect(screen.queryByRole("button", { name: /split terminal/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /interrupt terminal/i })).not.toBeInTheDocument();
  });

  it("starts native window dragging from its noninteractive titlebar background", () => {
    render(<WorkspaceHeader worktree={worktree} agent={agent} />);

    fireEvent.pointerDown(screen.getByRole("banner"), { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();

    fireEvent.pointerDown(screen.getByTestId("workspace-agent-chip"), { button: 0 });
    expect(nativeWindow.startDragging).toHaveBeenCalledOnce();
  });

  it("renders toggle button when sidebar is closed and calls onToggleSidebar", () => {
    const onToggleSidebar = vi.fn();
    const { rerender } = render(
      <WorkspaceHeader worktree={worktree} agent={agent} sidebarOpen={true} onToggleSidebar={onToggleSidebar} isMac={true} />,
    );

    expect(screen.queryByRole("button", { name: "Show sidebar" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("titlebar-traffic-light-pad")).not.toBeInTheDocument();

    rerender(
      <WorkspaceHeader worktree={worktree} agent={agent} sidebarOpen={false} onToggleSidebar={onToggleSidebar} isMac={true} />,
    );

    expect(screen.getByTestId("titlebar-traffic-light-pad")).toBeInTheDocument();
    const toggleBtn = screen.getByRole("button", { name: "Show sidebar" });
    expect(toggleBtn).toBeInTheDocument();
    expect(toggleBtn).toHaveClass("no-drag");

    fireEvent.click(toggleBtn);
    expect(onToggleSidebar).toHaveBeenCalledOnce();
  });
});
