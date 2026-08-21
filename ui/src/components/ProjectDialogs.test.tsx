import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  registerProject: vi.fn(),
  listProjectBranches: vi.fn(),
  createWorktree: vi.fn(),
}));

vi.mock("../lib/tauri", () => native);

import { AddProjectDialog, AddWorktreeDialog } from "./ProjectDialogs";

afterEach(cleanup);
beforeEach(() => {
  native.registerProject.mockReset();
  native.listProjectBranches.mockReset();
  native.createWorktree.mockReset();
});

describe("project/worktree flows", () => {
  it("registers Add Project through the typed native contract", async () => {
    const onRegistered = vi.fn();
    native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={onRegistered} />);
    fireEvent.change(screen.getByLabelText("Workspace id"), { target: { value: "orca-lite" } });
    fireEvent.change(screen.getByLabelText("Repository path"), { target: { value: "/repo/orca-lite" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() => expect(native.registerProject).toHaveBeenCalledWith({
      workspaceId: "orca-lite",
      repoPath: "/repo/orca-lite",
    }));
    expect(onRegistered).toHaveBeenCalledWith({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
  });

  it("loads local branches and uses a real branch dropdown for Add Worktree", async () => {
    native.listProjectBranches.mockResolvedValue([
      { name: "develop", isCurrent: false },
      { name: "main", isCurrent: true },
    ]);
    native.createWorktree.mockResolvedValue({ path: "/repo/worktrees/feature-ui" });
    const onCreated = vi.fn();

    render(
      <AddWorktreeDialog
        project={{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" }}
        onClose={vi.fn()}
        onCreated={onCreated}
      />,
    );

    await waitFor(() => expect(native.listProjectBranches).toHaveBeenCalledWith("orca-lite"));
    const branch = screen.getByRole("combobox", { name: "Base branch" });
    expect(branch.tagName).toBe("SELECT");
    expect(screen.getByRole("option", { name: "main (current)" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Base branch" })).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Worktree slug"), { target: { value: "feature-ui" } });
    fireEvent.change(branch, { target: { value: "develop" } });
    fireEvent.click(screen.getByRole("button", { name: "Create Worktree" }));

    await waitFor(() => expect(native.createWorktree).toHaveBeenCalledWith({
      workspaceId: "orca-lite",
      worktree: { wsId: "orca-lite", slug: "feature-ui" },
      baseRef: "develop",
    }));
    expect(onCreated).toHaveBeenCalled();
  });
});