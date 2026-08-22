import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  isTauriRuntime: vi.fn(),
  registerProject: vi.fn(),
  listProjectBranches: vi.fn(),
  createWorktree: vi.fn(),
}));

const dialog = vi.hoisted(() => ({
  open: vi.fn(),
}));

vi.mock("../lib/tauri", () => native);
vi.mock("@tauri-apps/plugin-dialog", () => dialog);

import { AddProjectDialog, AddWorktreeDialog, deriveWorkspaceId } from "./ProjectDialogs";

afterEach(cleanup);
beforeEach(() => {
  native.isTauriRuntime.mockReset();
  native.registerProject.mockReset();
  native.listProjectBranches.mockReset();
  native.createWorktree.mockReset();
  dialog.open.mockReset();
});

describe("deriveWorkspaceId helper", () => {
  it("derives workspaceId by slugifying basename and deduplicating", () => {
    expect(deriveWorkspaceId("/Users/dev/my-project")).toBe("my-project");
    expect(deriveWorkspaceId("/Users/dev/my-project/")).toBe("my-project");
    expect(deriveWorkspaceId("/Users/dev/My Cool App 123")).toBe("My-Cool-App-123");
    expect(deriveWorkspaceId("/Users/dev/---leading-dash")).toBe("leading-dash");
    expect(deriveWorkspaceId("///")).toBe("project");

    const existing = [
      { workspaceId: "my-project", repoRoot: "/a" },
      { workspaceId: "my-project-2", repoRoot: "/b" },
    ];
    expect(deriveWorkspaceId("/Users/dev/my-project", existing)).toBe("my-project-3");
  });
});

describe("AddProjectDialog Tauri flow", () => {
  it("closes dialog when picker is cancelled (null result)", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue(null);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    await waitFor(() =>
      expect(dialog.open).toHaveBeenCalledWith({
        directory: true,
        multiple: false,
        title: "Add Project",
      }),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("shows confirmation dialog with the selected path", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/orca-project");
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("/Users/dev/orca-project")).toBeInTheDocument());
    expect(screen.getByText("Add this folder as a separate Orca project.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project" })).toBeInTheDocument();
  });

  it("submits with derived slugified id from basename and calls registerProject + onRegistered", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/My Awesome Project");
    const registered = { workspaceId: "My-Awesome-Project", repoRoot: "/Users/dev/My Awesome Project" };
    native.registerProject.mockResolvedValue(registered);
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    await waitFor(() => expect(screen.getByText("/Users/dev/My Awesome Project")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: "My-Awesome-Project",
        repoPath: "/Users/dev/My Awesome Project",
      }),
    );
    expect(onRegistered).toHaveBeenCalledWith(registered);
    expect(onClose).toHaveBeenCalled();
  });

  it("appends -2 suffix when derived workspaceId conflicts with existing projects", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/existing-app");
    const existing = [{ workspaceId: "existing-app", repoRoot: "/other/path/existing-app" }];
    const registered = { workspaceId: "existing-app-2", repoRoot: "/Users/dev/existing-app" };
    native.registerProject.mockResolvedValue(registered);
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog projects={existing} onClose={onClose} onRegistered={onRegistered} />);

    await waitFor(() => expect(screen.getByText("/Users/dev/existing-app")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: "existing-app-2",
        repoPath: "/Users/dev/existing-app",
      }),
    );
    expect(onRegistered).toHaveBeenCalledWith(registered);
    expect(onClose).toHaveBeenCalled();
  });

  it("keeps dialog mounted with manual form and shows error when picker is rejected", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockRejectedValue(new Error("Native dialog capability error"));
    const onClose = vi.fn();
    const onRegistered = vi.fn();
    native.registerProject.mockResolvedValue({ workspaceId: "manual-project", repoRoot: "/custom/path" });

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    await waitFor(() => expect(screen.getByText("Native dialog capability error")).toBeInTheDocument());
    expect(screen.getByLabelText("Workspace id")).toBeInTheDocument();
    expect(screen.getByLabelText("Repository path")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(consoleSpy).toHaveBeenCalled();
    consoleSpy.mockRestore();

    fireEvent.change(screen.getByLabelText("Workspace id"), { target: { value: "manual-project" } });
    fireEvent.change(screen.getByLabelText("Repository path"), { target: { value: "/custom/path" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: "manual-project",
        repoPath: "/custom/path",
      }),
    );
    expect(onRegistered).toHaveBeenCalledWith({ workspaceId: "manual-project", repoRoot: "/custom/path" });
    expect(onClose).toHaveBeenCalled();
  });

  it("does not re-invoke picker when parent re-renders with new onClose identity", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockReturnValue(new Promise(() => {}));
    const onClose1 = vi.fn();

    const { rerender } = render(<AddProjectDialog onClose={onClose1} onRegistered={vi.fn()} />);

    expect(dialog.open).toHaveBeenCalledTimes(1);

    const onClose2 = vi.fn();
    rerender(<AddProjectDialog onClose={onClose2} onRegistered={vi.fn()} />);

    expect(dialog.open).toHaveBeenCalledTimes(1);
  });
});

describe("AddProjectDialog non-Tauri fallback flow", () => {
  it("keeps manual form in non-Tauri runtime and registers project", async () => {
    native.isTauriRuntime.mockReturnValue(false);
    const onRegistered = vi.fn();
    native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={onRegistered} />);
    expect(screen.getByLabelText("Workspace id")).toBeInTheDocument();
    expect(screen.getByLabelText("Repository path")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Workspace id"), { target: { value: "orca-lite" } });
    fireEvent.change(screen.getByLabelText("Repository path"), { target: { value: "/repo/orca-lite" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() =>
      expect(native.registerProject).toHaveBeenCalledWith({
        workspaceId: "orca-lite",
        repoPath: "/repo/orca-lite",
      }),
    );
    expect(onRegistered).toHaveBeenCalledWith({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite" });
  });
  it("uses neutral placeholder for workspace id input", () => {
    native.isTauriRuntime.mockReturnValue(false);
    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);
    const input = screen.getByLabelText("Workspace id");
    expect(input).toHaveAttribute("placeholder", "my-project");
  });
});

describe("AddWorktreeDialog flow", () => {
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

    await waitFor(() =>
      expect(native.createWorktree).toHaveBeenCalledWith({
        workspaceId: "orca-lite",
        worktree: { wsId: "orca-lite", slug: "feature-ui" },
        baseRef: "develop",
      }),
    );
    expect(onCreated).toHaveBeenCalled();
  });
});
