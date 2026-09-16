import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorktreeDeleteDialog } from "./WorktreeDeleteDialog";

const native = vi.hoisted(() => ({ previewWorktreeDelete: vi.fn(), deleteWorktree: vi.fn(), deleteWorktreeDestructive: vi.fn() }));
vi.mock("../lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("../lib/tauri")>(), ...native,
}));
afterEach(cleanup);

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
