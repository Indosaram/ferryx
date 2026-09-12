import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Worktree } from "../lib/types";
import { WorktreeDeleteDialog, type WorktreeDeleteServices } from "./WorktreeDeleteDialog";

const native = vi.hoisted(() => ({
  previewWorktreeDelete: vi.fn(),
  deleteWorktree: vi.fn(),
  deleteWorktreeDestructive: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/tauri")>();
  return {
    ...actual,
    previewWorktreeDelete: native.previewWorktreeDelete,
    deleteWorktree: native.deleteWorktree,
    deleteWorktreeDestructive: native.deleteWorktreeDestructive,
  };
});

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

beforeEach(() => {
  native.previewWorktreeDelete.mockReset();
  native.deleteWorktree.mockReset();
  native.deleteWorktreeDestructive.mockReset();
  native.previewWorktreeDelete.mockResolvedValue(preview);
  native.deleteWorktree.mockResolvedValue(undefined);
  native.deleteWorktreeDestructive.mockResolvedValue(undefined);
});
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
    expect(screen.getByTestId("worktree-delete-divergence")).toHaveAttribute("data-state", "upstream");

    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    await waitFor(() => expect(services.deleteSafe).toHaveBeenCalledWith(worktree));
    expect(onDeleted).toHaveBeenCalledOnce();
  });

  it("contains an absolute hyphenated path and shows an explicit no-upstream divergence state", async () => {
    const selectedWorktree = { ...worktree, path: "/repo/worktrees/orca-ws-main/task-ipc-branch" };
    const services = createServices({
      previewDelete: vi.fn(async () => ({ ...preview, upstream: null, ahead: null, behind: null })),
    });
    render(<WorktreeDeleteDialog worktree={selectedWorktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);

    const path = await screen.findByTestId("worktree-delete-path");
    expect(path).toHaveTextContent(selectedWorktree.path);
    expect(path).toHaveClass("break-all");
    expect(screen.queryByText(/\? ahead · \? behind/)).not.toBeInTheDocument();
    const divergence = await screen.findByTestId("worktree-delete-divergence");
    expect(divergence).toHaveAttribute("data-state", "no-upstream");
    expect(divergence).toHaveTextContent("No upstream");
  });

  it("scopes native preview and safe deletion to the selected registered workspace", async () => {
    render(
      <WorktreeDeleteDialog
        {...({ workspaceId: "project-a", worktree, onClose: vi.fn(), onDeleted: vi.fn() } as any)}
      />,
    );

    await screen.findByText("orca/ws-main/feature");
    expect(native.previewWorktreeDelete).toHaveBeenCalledWith({
      workspaceId: "project-a",
      worktree: { wsId: "ws-main", slug: "feature" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    await waitFor(() => expect(native.deleteWorktree).toHaveBeenCalledWith({
      workspaceId: "project-a",
      worktree: { wsId: "ws-main", slug: "feature" },
      deleteBranch: true,
    }));
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

  it("offers destructive deletion for the DIRTY_WORKTREE error code", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "DIRTY_WORKTREE", message: "uncommitted changes", details: {} };
      }),
    });
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);

    await screen.findByText("orca/ws-main/feature");
    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));
    expect(await screen.findByRole("button", { name: "Delete worktree and discard changes permanently" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and discard changes permanently" }));
    await waitFor(() => expect(services.deleteDestructive).toHaveBeenCalledWith(worktree));
  });

  it("names the files a destructive deletion will discard, with a count and a truncated remainder", async () => {
    // The scope document requires the preview to show the file list AND the count before an
    // irreversible delete: showing only "has uncommitted changes" does not tell the user what
    // they are about to lose.
    const dirtyFiles = [
      { statusCode: " M", path: "src/main.rs" },
      { statusCode: "??", path: "notes/scratch.md" },
      ...Array.from({ length: 9 }, (_, i) => ({ statusCode: " M", path: `src/generated/file-${i}.ts` })),
    ];
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "DIRTY_WORKTREE", message: "uncommitted changes", details: {} };
      }),
    });
    render(
      <WorktreeDeleteDialog
        worktree={worktree}
        services={services}
        dirtyFiles={dirtyFiles}
        onClose={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );

    await screen.findByText("orca/ws-main/feature");
    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));

    const listing = await screen.findByTestId("dirty-file-preview");
    expect(listing).toHaveTextContent("11 files will be discarded");
    // Specific paths, not a generic phrase.
    expect(listing).toHaveTextContent("src/main.rs");
    expect(listing).toHaveTextContent("notes/scratch.md");
    // Bounded so a large dirty worktree cannot push the confirm button off-screen.
    expect(listing).toHaveTextContent("and 3 more");
    expect(listing.querySelectorAll("li")).toHaveLength(8);
  });

  it("omits the file listing when no dirty files were supplied", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "DIRTY_WORKTREE", message: "uncommitted changes", details: {} };
      }),
    });
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);

    await screen.findByText("orca/ws-main/feature");
    fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" }));

    // The destructive path must still be offered; only the listing is absent.
    expect(
      await screen.findByRole("button", { name: "Delete worktree and discard changes permanently" }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("dirty-file-preview")).not.toBeInTheDocument();
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
