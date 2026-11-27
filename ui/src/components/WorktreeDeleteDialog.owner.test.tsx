import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BranchDeletionPreview, RegisteredProject, Worktree } from "../lib/types";
import { remoteHostStore } from "../state/remoteHostStore";
import { WorktreeDeleteDialog } from "./WorktreeDeleteDialog";

const native = vi.hoisted(() => ({ previewWorktreeDelete: vi.fn(), deleteWorktree: vi.fn(), deleteWorktreeDestructive: vi.fn() }));
vi.mock("../lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/tauri")>(), ...native,
}));

// The paired adapter is captured when the dialog opens; a stub keeps this suite about the
// dialog's ownership routing instead of re-testing the adapter's revision protocol.
const paired = vi.hoisted(() => ({
  createPairedWorktreeActions: vi.fn(),
  previewDelete: vi.fn(),
  deleteSafe: vi.fn(),
  deleteDestructive: vi.fn(),
}));
vi.mock("../lib/pairedWorktreeActions", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/pairedWorktreeActions")>(),
  createPairedWorktreeActions: paired.createPairedWorktreeActions,
}));

beforeEach(() => {
  // No paired host is enrolled unless a test says otherwise, so the real adapter must fail
  // closed instead of picking up a host another test left behind.
  remoteHostStore.reset();
  native.previewWorktreeDelete.mockReset();
  native.deleteWorktree.mockReset();
  native.deleteWorktreeDestructive.mockReset();
  paired.createPairedWorktreeActions.mockReset();
  paired.previewDelete.mockReset();
  paired.deleteSafe.mockReset();
  paired.deleteDestructive.mockReset();
});
afterEach(cleanup);

const pairedWorktree: Worktree = {
  workspaceId: `daemon:${"a".repeat(64)}`, path: "/srv/repo/feature", head: "abc",
  branch: "refs/heads/orca/remote-project/feature", bare: false, detached: false, locked: null, prunable: null,
};

const pairedProject: RegisteredProject = {
  workspaceId: `daemon:${"a".repeat(64)}`, repoRoot: "/srv/repo", gitRoot: null,
  target: { kind: "pairedDaemon", hostId: "host-a" }, remoteWorkspaceId: "remote-project",
};

const pairedPreview: BranchDeletionPreview = {
  branch: "orca/remote-project/feature", head: "abc", upstream: null, merged: false,
  ahead: null, behind: null, dirtyState: { isDirty: false, files: [] }, missing: false,
};

function stubPairedAdapter() {
  paired.createPairedWorktreeActions.mockReturnValue({
    previewDelete: paired.previewDelete,
    deleteSafe: paired.deleteSafe,
    deleteDestructive: paired.deleteDestructive,
  });
}

describe("delete dialog missing paired owner", () => {
  it.each([undefined, "local-row", `daemon:${"b".repeat(64)}`])("fails closed when the row workspace is %s", async (rowWorkspaceId) => {
    native.previewWorktreeDelete.mockResolvedValue({ branch: "orca/remote-project/feature", head: "abc", upstream: null, merged: true, ahead: null, behind: null });
    await act(async () => {
      render(<WorktreeDeleteDialog workspaceId={`daemon:${"a".repeat(64)}`} worktree={{
        workspaceId: rowWorkspaceId, path: "/srv/repo/feature", head: "abc",
        branch: "refs/heads/orca/remote-project/feature", bare: false, detached: false, locked: null, prunable: null,
      }} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });
    expect(native.previewWorktreeDelete).not.toHaveBeenCalled();
    expect(screen.getByText("PAIRED_OWNER_REQUIRED")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete remote worktree" })).toBeDisabled();
    expect(native.deleteWorktree).not.toHaveBeenCalled();
    expect(native.deleteWorktreeDestructive).not.toHaveBeenCalled();
  });
});

describe("delete dialog with the paired owner present", () => {
  it("previews and deletes through the owning machine adapter, never the local or SSH path", async () => {
    stubPairedAdapter();
    paired.previewDelete.mockResolvedValue(pairedPreview);
    paired.deleteSafe.mockResolvedValue(undefined);
    const onDeleted = vi.fn();
    await act(async () => {
      render(<WorktreeDeleteDialog project={pairedProject} worktree={pairedWorktree} onClose={vi.fn()} onDeleted={onDeleted} />);
    });

    expect(paired.createPairedWorktreeActions).toHaveBeenCalledWith(pairedProject);
    expect(paired.previewDelete).toHaveBeenCalledWith(pairedWorktree);
    expect(native.previewWorktreeDelete).not.toHaveBeenCalled();
    // Paired previews carry branch metadata only, so force deletion is never offered.
    expect(screen.queryByRole("button", { name: /discard changes permanently/ })).not.toBeInTheDocument();

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Delete remote worktree" }));
    });
    expect(paired.deleteSafe).toHaveBeenCalledWith(pairedWorktree);
    expect(paired.deleteDestructive).not.toHaveBeenCalled();
    expect(native.deleteWorktree).not.toHaveBeenCalled();
    expect(onDeleted).toHaveBeenCalledOnce();
  });

  it("keeps the paired preview failure visible without claiming the branch is merged", async () => {
    stubPairedAdapter();
    paired.previewDelete.mockRejectedValue(new Error("owning machine offline"));
    await act(async () => {
      render(<WorktreeDeleteDialog project={pairedProject} worktree={pairedWorktree} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    expect(await screen.findByTestId("worktree-delete-preview-error")).toBeInTheDocument();
    expect(screen.queryByText("Merged")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete remote worktree" })).toBeDisabled();
    expect(native.previewWorktreeDelete).not.toHaveBeenCalled();
  });

  it("reports an unavailable owning machine instead of falling back to Local or SSH", async () => {
    // The real adapter, with no paired host enrolled in the store: the dialog must surface
    // PAIRED_HOST_UNAVAILABLE and stay inert rather than reroute the delete to this machine.
    const actual = await vi.importActual<typeof import("../lib/pairedWorktreeActions")>("../lib/pairedWorktreeActions");
    paired.createPairedWorktreeActions.mockImplementation(actual.createPairedWorktreeActions);
    await act(async () => {
      render(<WorktreeDeleteDialog project={pairedProject} worktree={pairedWorktree} onClose={vi.fn()} onDeleted={vi.fn()} />);
    });

    expect(await screen.findByText("PAIRED_HOST_UNAVAILABLE")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete remote worktree" })).toBeDisabled();
    expect(native.previewWorktreeDelete).not.toHaveBeenCalled();
    expect(native.deleteWorktree).not.toHaveBeenCalled();
    expect(native.deleteWorktreeDestructive).not.toHaveBeenCalled();
  });
});
