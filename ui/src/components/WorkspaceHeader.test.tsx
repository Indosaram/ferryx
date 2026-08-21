import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { WorkspaceHeader } from "./WorkspaceHeader";

const worktree: Worktree = {
  path: "/repo/feature",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

afterEach(cleanup);

describe("WorkspaceHeader", () => {
  it("exposes a real terminal interrupt action", () => {
    const onInterrupt = vi.fn();
    render(<WorkspaceHeader worktree={worktree} onInterrupt={onInterrupt} canInterrupt />);

    fireEvent.click(screen.getByRole("button", { name: "Interrupt terminal" }));
    expect(onInterrupt).toHaveBeenCalledOnce();
  });

  it("disables interrupt when there is no backend terminal session", () => {
    render(<WorkspaceHeader worktree={worktree} onInterrupt={vi.fn()} canInterrupt={false} />);
    expect(screen.getByRole("button", { name: "Interrupt terminal" })).toBeDisabled();
  });
});
