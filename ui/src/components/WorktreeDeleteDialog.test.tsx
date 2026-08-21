import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { WorktreeDeleteDialog, type WorktreeDeleteServices } from "./WorktreeDeleteDialog";

const worktree: Worktree = {
  path: "/repo/feature",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const preview = {
  branch: "orca/ws-main/feature",
  head: "abc123def456",
  upstream: "origin/orca/ws-main/feature",
  merged: false,
  ahead: 2,
  behind: 1,
};

function createServices(overrides: Partial<WorktreeDeleteServices> = {}): WorktreeDeleteServices {
  return {
    previewDelete: vi.fn(async () => preview),
    deleteSafe: vi.fn(async () => undefined),
    deleteDestructive: vi.fn(async () => undefined),
    ...overrides,
  };
}

afterEach(cleanup);

describe("WorktreeDeleteDialog", () => {
  it("shows branch safety metadata before safe deletion", async () => {
    const services = createServices();
    const onDeleted = vi.fn();
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={onDeleted} />);

    expect(await screen.findByText("orca/ws-main/feature")).toBeInTheDocument();
    expect(screen.getByText("abc123def456")).toBeInTheDocument();
    expect(screen.getByText("origin/orca/ws-main/feature")).toBeInTheDocument();
    expect(screen.getByText(/not merged/i)).toBeInTheDocument();
    expect(screen.getByText(/2 ahead/i)).toBeInTheDocument();
    expect(screen.getByText(/1 behind/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    await waitFor(() => expect(services.deleteSafe).toHaveBeenCalledWith(worktree));
    expect(onDeleted).toHaveBeenCalledOnce();
  });

  it("offers destructive deletion only for the UNMERGED_BRANCH error code", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "UNMERGED_BRANCH", message: "opaque backend wording", details: {} };
      }),
    });
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);

    await screen.findByText("orca/ws-main/feature");
    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    expect(await screen.findByRole("button", { name: "Delete unmerged branch permanently" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete unmerged branch permanently" }));
    await waitFor(() => expect(services.deleteDestructive).toHaveBeenCalledWith(worktree));
  });

  it("does not infer destructive deletion from an error message", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "GIT_ERROR", message: "unmerged branch text must not drive UI", details: {} };
      }),
    });
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);

    await screen.findByText("orca/ws-main/feature");
    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    await screen.findByText("GIT_ERROR");
    expect(screen.queryByRole("button", { name: "Delete unmerged branch permanently" })).not.toBeInTheDocument();
  });
});
