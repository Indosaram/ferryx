import React from "react";
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

import { AddProjectDialog, AddWorktreeDialog, RemoveProjectDialog, deriveWorkspaceId } from "./ProjectDialogs";

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
      { workspaceId: "my-project", repoRoot: "/a", gitRoot: "/a" },
      { workspaceId: "my-project-2", repoRoot: "/b", gitRoot: "/b" },
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

  it("surfaces the structured IPC error reason when registration fails", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/not-a-repo");
    native.registerProject.mockRejectedValue({
      code: "INVALID_REPO_ROOT",
      message: "'/Users/dev/not-a-repo' is not a git repository",
      details: {},
    });
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("/Users/dev/not-a-repo")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await waitFor(() =>
      expect(screen.getByText("'/Users/dev/not-a-repo' is not a git repository")).toBeInTheDocument(),
    );
    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows confirmation dialog with the selected path", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/orca-project");
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    await waitFor(() => expect(screen.getByText("/Users/dev/orca-project")).toBeInTheDocument());
    expect(screen.getByText("Add this folder as a separate Ferryx project.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project" })).toBeInTheDocument();
  });

  it("submits with derived slugified id from basename and calls registerProject + onRegistered", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/My Awesome Project");
    const registered = { workspaceId: "My-Awesome-Project", repoRoot: "/Users/dev/My Awesome Project", gitRoot: "/Users/dev/My Awesome Project" };
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
    const existing = [{ workspaceId: "existing-app", repoRoot: "/other/path/existing-app", gitRoot: "/other/path/existing-app" }];
    const registered = { workspaceId: "existing-app-2", repoRoot: "/Users/dev/existing-app", gitRoot: "/Users/dev/existing-app" };
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
    native.registerProject.mockResolvedValue({ workspaceId: "manual-project", repoRoot: "/custom/path", gitRoot: "/custom/path" });

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
    expect(onRegistered).toHaveBeenCalledWith({ workspaceId: "manual-project", repoRoot: "/custom/path", gitRoot: "/custom/path" });
    expect(onClose).toHaveBeenCalled();
  });

  it("shows confirmation dialog after selection under StrictMode double-mounted effects", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockResolvedValue("/Users/dev/strict-mode-project");
    const onClose = vi.fn();

    render(
      <React.StrictMode>
        <AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />
      </React.StrictMode>,
    );

    await waitFor(() => expect(dialog.open).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByText("/Users/dev/strict-mode-project")).toBeInTheDocument());
  });

  it("keeps a dialog surface mounted while the native picker is still pending", async () => {
    // The native picker is a sheet on the main window and the native terminal
    // compositor sits above the webview, yielding only for `[role="dialog"]`.
    // A pending picker must therefore still present a dialog surface, and it
    // must stay cancellable so the parent's open flag cannot latch.
    native.isTauriRuntime.mockReturnValue(true);
    dialog.open.mockReturnValue(new Promise(() => {}));
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    const pending = await screen.findByRole("dialog", { name: "Add Project" });
    expect(pending).toHaveAttribute("aria-busy", "true");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
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
    native.registerProject.mockResolvedValue({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite", gitRoot: "/repo/orca-lite" });

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
    expect(onRegistered).toHaveBeenCalledWith({ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite", gitRoot: "/repo/orca-lite" });
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
        project={{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite", gitRoot: "/repo/orca-lite" }}
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

  it("explains that a plain folder project has no branches instead of leaving a dead dialog", async () => {
    native.listProjectBranches.mockResolvedValue([]);

    render(
      <AddWorktreeDialog
        project={{ workspaceId: "superwiki-mail-otp", repoRoot: "/repos/superwiki", gitRoot: null }}
        onClose={vi.fn()}
        onCreated={vi.fn()}
      />,
    );

    await waitFor(() => expect(screen.getByText(/not a Git repository/i)).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Create Worktree" })).toBeNull();
    expect(screen.getByRole("button", { name: "Close Add Worktree" })).toBeInTheDocument();
  });
});

describe("RemoveProjectDialog flow", () => {
  it("renders project id, path notice, and invokes onConfirm when Remove Project is clicked", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    const project = { workspaceId: "proj-remove", repoRoot: "/repos/proj-remove", gitRoot: "/repos/proj-remove" };

    render(<RemoveProjectDialog project={project} onClose={onClose} onConfirm={onConfirm} />);

    expect(screen.getByRole("dialog", { name: "Remove Project" })).toBeInTheDocument();
    expect(screen.getByText("proj-remove")).toBeInTheDocument();
    expect(screen.getByText("/repos/proj-remove")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Remove Project" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("invokes onClose when Cancel is clicked", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    const project = { workspaceId: "proj-remove", repoRoot: "/repos/proj-remove", gitRoot: null };

    render(<RemoveProjectDialog project={project} onClose={onClose} onConfirm={onConfirm} />);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
