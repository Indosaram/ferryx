import "./worktree-disk-test-dom";
import "@testing-library/jest-dom/vitest";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { BranchDeletionPreview, Worktree } from "../lib/types";
import { WorktreeDeleteDialog, type WorktreeDeleteServices } from "./WorktreeDeleteDialog";

const native = {
  previewWorktreeDelete: vi.fn(),
  deleteWorktree: vi.fn(),
  deleteWorktreeDestructive: vi.fn(),
  deleteRemoteWorktree: vi.fn(),
};

const worktree: Worktree = {
  path: "/repo/feature",
  head: "abc123",
  branch: "refs/heads/orca/ws-main/feature",
  bare: false,
  detached: false,
  locked: null,
  prunable: null,
};

const preview: BranchDeletionPreview = {
  branch: "orca/ws-main/feature",
  head: "abc123def456",
  upstream: "origin/orca/ws-main/feature",
  merged: false,
  ahead: 2,
  behind: 1,
  dirtyState: { isDirty: false, files: [] },
  missing: false,
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
  (window as any).isTauri = true;
  native.previewWorktreeDelete.mockReset();
  native.deleteWorktree.mockReset();
  native.deleteWorktreeDestructive.mockReset();
  native.deleteRemoteWorktree.mockReset();
  native.previewWorktreeDelete.mockResolvedValue(preview);
  native.deleteWorktree.mockResolvedValue(undefined);
  native.deleteWorktreeDestructive.mockResolvedValue(undefined);
  native.deleteRemoteWorktree.mockResolvedValue(undefined);
  // Intercept only IPC: default services and native request routing stay real,
  // with no module mocks that could leak into the disk/lifecycle suites in Bun.
  mockIPC((command, args) => {
    if (command === "cmd_ssh_delete_remote_worktree") {
      return native.deleteRemoteWorktree(args);
    }
    if (!args || !("request" in args)) throw new Error(`Missing IPC request: ${command}`);
    switch (command) {
      case "cmd_worktree_delete_preview": return native.previewWorktreeDelete(args?.request);
      case "cmd_worktree_delete": return native.deleteWorktree(args?.request);
      case "cmd_worktree_delete_destructive": return native.deleteWorktreeDestructive(args?.request);
      default: throw new Error(`Unexpected IPC command: ${command}`);
    }
  });
});
afterEach(() => {
  delete (window as any).isTauri;
  cleanup();
  clearMocks();
});

describe("WorktreeDeleteDialog", () => {
  it("gates destructive deletion after preview rejection", async () => {
    const services = createServices({ previewDelete: vi.fn(async () => { throw { code: "GIT_ERROR", message: "preview unavailable" }; }) });
    await act(async () => {
    await act(async () => {
      render(<WorktreeDeleteDialog worktree={worktree} services={services} initialDirty onClose={vi.fn()} onDeleted={vi.fn()} />);
    });
    });
    for (const button of screen.getAllByRole("button").filter((b) => b.textContent?.startsWith("Delete"))) {
      expect(button).toBeDisabled();
    await act(async () => { fireEvent.click(button); });
    }
    expect(services.deleteDestructive).not.toHaveBeenCalled();
  });

  it("uses fresh dirty and unmerged preview instead of cached clean props", async () => {
    const services = createServices({ previewDelete: vi.fn(async () => ({ ...preview, dirtyState: { isDirty: true, files: [{ statusCode: "??", path: "fresh.txt" }] } })) });
    await act(async () => { render(<WorktreeDeleteDialog worktree={worktree} services={services} dirtyFiles={[{ statusCode: " M", path: "stale.txt" }]} onClose={vi.fn()} onDeleted={vi.fn()} />); });
    expect(screen.getByTestId("dirty-file-preview")).toHaveTextContent("fresh.txt");
    expect(screen.getByTestId("dirty-file-preview")).not.toHaveTextContent("stale.txt");
    expect(screen.getByRole("button", { name: "Delete worktree and discard changes permanently" })).toBeEnabled();
  });

  it("invalidates preview and refreshes current losses when safe deletion discovers new changes", async () => {
    let resolveRefresh!: (value: typeof preview) => void;
    const refreshed = new Promise<typeof preview>((resolve) => { resolveRefresh = resolve; });
    const services = createServices({
      previewDelete: vi.fn().mockResolvedValueOnce(preview).mockReturnValueOnce(refreshed),
      deleteSafe: vi.fn(async () => { throw { code: "UNMERGED_BRANCH", message: "changed" }; }),
    });
    await act(async () => { render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    expect(services.previewDelete).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "Delete unmerged branch permanently" })).toBeDisabled();
    await act(async () => { resolveRefresh({ ...preview, dirtyState: { isDirty: true, files: [{ statusCode: "??", path: "new-loss.txt" }] } } as typeof preview); await refreshed; });
    expect(screen.getByTestId("dirty-file-preview")).toHaveTextContent("new-loss.txt");
  });
  it("shows branch safety metadata before safe deletion", async () => {
    const services = createServices();
    const onDeleted = vi.fn();
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={onDeleted} />);
    });

    expect(screen.getByText("orca/ws-main/feature")).toBeInTheDocument();
    expect(screen.getByText("abc123def456")).toBeInTheDocument();
    expect(screen.getByText("origin/orca/ws-main/feature")).toBeInTheDocument();
    expect(screen.getByText(/not merged/i)).toBeInTheDocument();
    expect(screen.getByText(/2 ahead/i)).toBeInTheDocument();
    expect(screen.getByText(/1 behind/i)).toBeInTheDocument();
    expect(screen.getByTestId("worktree-delete-divergence")).toHaveAttribute("data-state", "upstream");

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    expect(services.deleteSafe).toHaveBeenCalledWith(worktree);
    expect(onDeleted).toHaveBeenCalledOnce();
  });

  it("contains an absolute hyphenated path and shows an explicit no-upstream divergence state", async () => {
    const selectedWorktree = { ...worktree, path: "/repo/worktrees/orca-ws-main/task-ipc-branch" };
    const services = createServices({
      previewDelete: vi.fn(async () => ({ ...preview, upstream: null, ahead: null, behind: null })),
    });
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={selectedWorktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    const path = screen.getByTestId("worktree-delete-path");
    expect(path).toHaveTextContent(selectedWorktree.path);
    expect(path).toHaveClass("break-all");
    expect(screen.queryByText(/\? ahead · \? behind/)).not.toBeInTheDocument();
    const divergence = screen.getByTestId("worktree-delete-divergence");
    expect(divergence).toHaveAttribute("data-state", "no-upstream");
    expect(divergence).toHaveTextContent("No upstream");
  });

  it("scopes native preview and safe deletion to the selected registered workspace", async () => {
    await act(async () => {
    render(
      <WorktreeDeleteDialog
        {...({ workspaceId: "project-a", worktree, onClose: vi.fn(), onDeleted: vi.fn() } as any)}
      />,
    );
    });

    screen.getByText("orca/ws-main/feature");
    expect(native.previewWorktreeDelete).toHaveBeenCalledWith({
      workspaceId: "project-a",
      worktree: { wsId: "ws-main", slug: "feature" },
    });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    expect(native.deleteWorktree).toHaveBeenCalledWith({
      workspaceId: "project-a",
      worktree: { wsId: "ws-main", slug: "feature" },
      deleteBranch: true,
    });
  });

  it("offers destructive deletion only for the UNMERGED_BRANCH error code", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "UNMERGED_BRANCH", message: "opaque backend wording", details: {} };
      }),
    });
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    screen.getByText("orca/ws-main/feature");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    expect(screen.getByRole("button", { name: "Delete unmerged branch permanently" })).toBeInTheDocument();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete unmerged branch permanently" })); });
    expect(services.deleteDestructive).toHaveBeenCalledWith(worktree);
  });

  it("offers destructive deletion for the DIRTY_WORKTREE error code", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "DIRTY_WORKTREE", message: "uncommitted changes", details: {} };
      }),
    });
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    screen.getByText("orca/ws-main/feature");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    expect(screen.getByRole("button", { name: "Delete worktree and discard changes permanently" })).toBeInTheDocument();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and discard changes permanently" })); });
    expect(services.deleteDestructive).toHaveBeenCalledWith(worktree);
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
      previewDelete: vi.fn().mockResolvedValueOnce(preview).mockResolvedValueOnce({ ...preview, dirtyState: { isDirty: true, files: dirtyFiles } }),
      deleteSafe: vi.fn(async () => {
        throw { code: "DIRTY_WORKTREE", message: "uncommitted changes", details: {} };
      }),
    });
    await act(async () => {
    render(
      <WorktreeDeleteDialog
        worktree={worktree}
        services={services}
        dirtyFiles={dirtyFiles}
        onClose={vi.fn()}
        onDeleted={vi.fn()}
      />,
    );
    });

    screen.getByText("orca/ws-main/feature");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });

    const listing = screen.getByTestId("dirty-file-preview");
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
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    screen.getByText("orca/ws-main/feature");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });

    // The destructive path must still be offered; only the listing is absent.
    expect(
      screen.getByRole("button", { name: "Delete worktree and discard changes permanently" }),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("dirty-file-preview")).not.toBeInTheDocument();
  });

  it("does not infer destructive deletion from an error message", async () => {
    const services = createServices({
      deleteSafe: vi.fn(async () => {
        throw { code: "GIT_ERROR", message: "unmerged branch text must not drive UI", details: {} };
      }),
    });
    await act(async () => {
    render(<WorktreeDeleteDialog worktree={worktree} services={services} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    screen.getByText("orca/ws-main/feature");
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Delete worktree and branch" })); });
    screen.getByText("GIT_ERROR");
    expect(screen.queryByRole("button", { name: "Delete unmerged branch permanently" })).not.toBeInTheDocument();
  });

  it("deletes a remote SSH worktree using cmd_ssh_delete_remote_worktree", async () => {
    const remoteWorktree: Worktree = {
      path: "/srv/repo/.orca-worktrees/wt-feat-remote",
      head: "abc1234",
      branch: "refs/heads/orca/ssh-123456/feat-remote",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      workspaceId: "ssh:host-1",
    };
    const onDeleted = vi.fn();
    const onClose = vi.fn();

    await act(async () => {
      render(
        <WorktreeDeleteDialog
          workspaceId="ssh:host-1"
          worktree={remoteWorktree}
          onClose={onClose}
          onDeleted={onDeleted}
        />,
      );
    });

    expect(screen.getByText("orca/ssh-123456/feat-remote")).toBeInTheDocument();
    const deleteBtn = screen.getByRole("button", { name: "Delete remote worktree" });
    expect(deleteBtn).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(deleteBtn);
    });

    expect(native.deleteRemoteWorktree).toHaveBeenCalledWith({
      workspaceId: "ssh:host-1",
      path: "/srv/repo/.orca-worktrees/wt-feat-remote",
      force: false,
    });
    expect(onDeleted).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("offers destructive deletion for remote SSH worktree with force: true", async () => {
    native.deleteRemoteWorktree
      .mockRejectedValueOnce({
        code: "DIRTY_WORKTREE",
        message: "Worktree contains modified or untracked files",
        details: {},
      })
      .mockResolvedValueOnce(undefined);

    const remoteWorktree: Worktree = {
      path: "/srv/repo/.orca-worktrees/wt-feat-remote",
      head: "abc1234",
      branch: "refs/heads/orca/ssh-123456/feat-remote",
      bare: false,
      detached: false,
      locked: null,
      prunable: null,
      workspaceId: "ssh:host-1",
    };
    const onDeleted = vi.fn();
    const onClose = vi.fn();

    await act(async () => {
      render(
        <WorktreeDeleteDialog
          workspaceId="ssh:host-1"
          worktree={remoteWorktree}
          onClose={onClose}
          onDeleted={onDeleted}
        />,
      );
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Delete remote worktree" }));
    });

    const destructiveBtn = screen.getByRole("button", {
      name: "Delete worktree and discard changes permanently",
    });
    expect(destructiveBtn).toBeInTheDocument();

    await act(async () => {
      fireEvent.click(destructiveBtn);
    });

    expect(native.deleteRemoteWorktree).toHaveBeenLastCalledWith({
      workspaceId: "ssh:host-1",
      path: "/srv/repo/.orca-worktrees/wt-feat-remote",
      force: true,
    });
    expect(onDeleted).toHaveBeenCalledOnce();
  });
});
