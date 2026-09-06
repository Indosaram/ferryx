import React from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
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

const remote = vi.hoisted(() => ({
  registerRemoteProject: vi.fn(),
  toRegisteredProject: (r: {
    workspaceId: string;
    repoRoot: string;
    gitRoot?: string | null;
    hostId: string;
  }) => ({
    workspaceId: r.workspaceId,
    repoRoot: r.repoRoot,
    gitRoot: r.gitRoot ?? null,
    target: {
      kind: "ssh" as const,
      hostId: r.hostId,
    },
  }),
}));

const ssh = vi.hoisted(() => ({
  useSshHosts: vi.fn(),
  formatSshTarget: (host: { username?: string | null; hostname: string }) => {
    if (host.username && host.username.trim() !== "") {
      return `${host.username.trim()}@${host.hostname.trim()}`;
    }
    return host.hostname.trim();
  },
}));

vi.mock("../lib/tauri", () => native);
vi.mock("@tauri-apps/plugin-dialog", () => dialog);
vi.mock("../lib/remoteProject", () => remote);
vi.mock("../lib/sshHosts", () => ssh);

import {
  AddProjectDialog,
  AddWorktreeDialog,
  RemoveProjectDialog,
  deriveWorkspaceId,
} from "./ProjectDialogs";
import type { RegisteredProject } from "../lib/tauri";
import type { SshHost } from "../lib/sshHosts";

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const mockHost1: SshHost = {
  id: "host-1",
  label: "Dev Server",
  hostname: "dev.internal",
  username: "ubuntu",
  port: 22,
  source: "manual",
  authMethod: "agent",
  disabled: false,
};

const mockHost2: SshHost = {
  id: "host-2",
  label: "Production",
  hostname: "prod.example.com",
  username: "deploy",
  port: 2222,
  source: "config",
  authMethod: "key",
  disabled: true,
};

afterEach(cleanup);

beforeEach(() => {
  native.isTauriRuntime.mockReset();
  native.registerProject.mockReset();
  native.listProjectBranches.mockReset();
  native.createWorktree.mockReset();
  dialog.open.mockReset();
  remote.registerRemoteProject.mockReset();
  ssh.useSshHosts.mockReset();

  ssh.useSshHosts.mockReturnValue({
    hosts: [mockHost1],
    loading: false,
    error: null,
    refresh: vi.fn().mockResolvedValue([mockHost1]),
  });
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

describe("AddProjectDialog Location Chooser", () => {
  it("shows Local and Remote chooser on initial mount and makes zero native picker calls", () => {
    native.isTauriRuntime.mockReturnValue(true);
    const onClose = vi.fn();
    const onRegistered = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    expect(screen.getByRole("dialog", { name: "Add Project" })).toBeInTheDocument();
    expect(dialog.open).not.toHaveBeenCalled();
    expect(screen.getByTestId("project-type-local")).toBeInTheDocument();
    expect(screen.getByTestId("project-type-remote")).toBeInTheDocument();
  });

  it("closes dialog when Cancel button is clicked in the location chooser", () => {
    native.isTauriRuntime.mockReturnValue(true);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("add-project-cancel"));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("preserves role='dialog' and aria-label='Add Project' across all states", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);
    expect(screen.getByRole("dialog", { name: "Add Project" })).toBeInTheDocument();

    // In local pending
    fireEvent.click(screen.getByTestId("project-type-local"));
    expect(screen.getByRole("dialog", { name: "Add Project" })).toBeInTheDocument();

    // In local confirm
    await act(async () => {
      pickerDef.resolve("/Users/dev/project");
    });
    expect(screen.getByRole("dialog", { name: "Add Project" })).toBeInTheDocument();
  });
});

describe("AddProjectDialog Local flow", () => {
  it("opens native picker on explicit Local click and closes dialog when cancelled (null result)", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    expect(dialog.open).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("project-type-local"));

    expect(dialog.open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Add Project",
    });

    await act(async () => {
      pickerDef.resolve(null);
    });

    expect(onClose).toHaveBeenCalledOnce();
  });

  it("surfaces the structured IPC error reason when registration fails", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    const regDef = deferred<unknown>();
    dialog.open.mockReturnValue(pickerDef.promise);
    native.registerProject.mockReturnValue(regDef.promise);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    await act(async () => {
      pickerDef.resolve("/Users/dev/not-a-repo");
    });

    expect(screen.getByText("/Users/dev/not-a-repo")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await act(async () => {
      regDef.reject({
        code: "INVALID_REPO_ROOT",
        message: "'/Users/dev/not-a-repo' is not a git repository",
        details: {},
      });
    });

    expect(screen.getByText("'/Users/dev/not-a-repo' is not a git repository")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows confirmation dialog with the selected path", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    await act(async () => {
      pickerDef.resolve("/Users/dev/orca-project");
    });

    expect(screen.getByText("/Users/dev/orca-project")).toBeInTheDocument();
    expect(screen.getByText("Add this folder as a separate Ferryx project.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Project" })).toBeInTheDocument();
  });

  it("submits with derived slugified id from basename and calls registerProject + onRegistered", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    const regDef = deferred<unknown>();
    dialog.open.mockReturnValue(pickerDef.promise);
    native.registerProject.mockReturnValue(regDef.promise);
    const registered = {
      workspaceId: "My-Awesome-Project",
      repoRoot: "/Users/dev/My Awesome Project",
      gitRoot: "/Users/dev/My Awesome Project",
    };
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    await act(async () => {
      pickerDef.resolve("/Users/dev/My Awesome Project");
    });

    expect(screen.getByText("/Users/dev/My Awesome Project")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await act(async () => {
      regDef.resolve(registered);
    });

    expect(native.registerProject).toHaveBeenCalledWith({
      workspaceId: "My-Awesome-Project",
      repoPath: "/Users/dev/My Awesome Project",
    });
    expect(onRegistered).toHaveBeenCalledWith(registered);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("appends -2 suffix when derived workspaceId conflicts with existing projects", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    const regDef = deferred<unknown>();
    dialog.open.mockReturnValue(pickerDef.promise);
    native.registerProject.mockReturnValue(regDef.promise);
    const existing = [
      {
        workspaceId: "existing-app",
        repoRoot: "/other/path/existing-app",
        gitRoot: "/other/path/existing-app",
      },
    ];
    const registered = {
      workspaceId: "existing-app-2",
      repoRoot: "/Users/dev/existing-app",
      gitRoot: "/Users/dev/existing-app",
    };
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog projects={existing} onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    await act(async () => {
      pickerDef.resolve("/Users/dev/existing-app");
    });

    expect(screen.getByText("/Users/dev/existing-app")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await act(async () => {
      regDef.resolve(registered);
    });

    expect(native.registerProject).toHaveBeenCalledWith({
      workspaceId: "existing-app-2",
      repoPath: "/Users/dev/existing-app",
    });
    expect(onRegistered).toHaveBeenCalledWith(registered);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps dialog mounted with manual form and shows error when picker is rejected", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    const regDef = deferred<unknown>();
    dialog.open.mockReturnValue(pickerDef.promise);
    native.registerProject.mockReturnValue(regDef.promise);
    const onClose = vi.fn();
    const onRegistered = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    await act(async () => {
      pickerDef.reject(new Error("Native dialog capability error"));
    });

    expect(screen.getByText("Native dialog capability error")).toBeInTheDocument();
    expect(screen.getByLabelText("Workspace id")).toBeInTheDocument();
    expect(screen.getByLabelText("Repository path")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    expect(consoleSpy).toHaveBeenCalled();
    consoleSpy.mockRestore();

    fireEvent.change(screen.getByLabelText("Workspace id"), { target: { value: "manual-project" } });
    fireEvent.change(screen.getByLabelText("Repository path"), { target: { value: "/custom/path" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await act(async () => {
      regDef.resolve({
        workspaceId: "manual-project",
        repoRoot: "/custom/path",
        gitRoot: "/custom/path",
      });
    });

    expect(native.registerProject).toHaveBeenCalledWith({
      workspaceId: "manual-project",
      repoPath: "/custom/path",
    });
    expect(onRegistered).toHaveBeenCalledWith({
      workspaceId: "manual-project",
      repoRoot: "/custom/path",
      gitRoot: "/custom/path",
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("allows retrying the native picker when navigating Back from the manual error view", async () => {
    const consoleSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef1 = deferred<string | null>();
    const pickerDef2 = deferred<string | null>();
    dialog.open
      .mockReturnValueOnce(pickerDef1.promise)
      .mockReturnValueOnce(pickerDef2.promise);

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    // First attempt: click Local
    fireEvent.click(screen.getByTestId("project-type-local"));
    expect(dialog.open).toHaveBeenCalledTimes(1);

    // Picker fails, routing to local-manual view with error
    await act(async () => {
      pickerDef1.reject(new Error("Native picker initialization failure"));
    });

    expect(screen.getByText("Native picker initialization failure")).toBeInTheDocument();
    consoleSpy.mockRestore();

    // Click Back to return to location chooser
    fireEvent.click(screen.getByTestId("add-project-back"));
    expect(screen.getByTestId("project-type-local")).toBeInTheDocument();

    // Second attempt: click Local again
    fireEvent.click(screen.getByTestId("project-type-local"));

    // Guard must have been released upon settlement, invoking the picker a second time
    expect(dialog.open).toHaveBeenCalledTimes(2);

    // Second attempt succeeds
    await act(async () => {
      pickerDef2.resolve("/Users/dev/retried-project");
    });

    expect(screen.getByText("/Users/dev/retried-project")).toBeInTheDocument();
  });

  it("shows confirmation dialog after selection under StrictMode double-mounted effects", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);
    const onClose = vi.fn();

    render(
      <React.StrictMode>
        <AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />
      </React.StrictMode>,
    );

    fireEvent.click(screen.getByTestId("project-type-local"));
    expect(dialog.open).toHaveBeenCalledTimes(1);

    await act(async () => {
      pickerDef.resolve("/Users/dev/strict-mode-project");
    });

    expect(screen.getByText("/Users/dev/strict-mode-project")).toBeInTheDocument();
  });

  it("keeps a dialog surface mounted while the native picker is still pending", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    const pending = screen.getByRole("dialog", { name: "Add Project" });
    expect(pending).toHaveAttribute("aria-busy", "true");

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("does not re-invoke picker when parent re-renders with new onClose identity", () => {
    native.isTauriRuntime.mockReturnValue(true);
    const pickerDef = deferred<string | null>();
    dialog.open.mockReturnValue(pickerDef.promise);
    const onClose1 = vi.fn();

    const { rerender } = render(<AddProjectDialog onClose={onClose1} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-local"));
    expect(dialog.open).toHaveBeenCalledTimes(1);

    const onClose2 = vi.fn();
    rerender(<AddProjectDialog onClose={onClose2} onRegistered={vi.fn()} />);

    expect(dialog.open).toHaveBeenCalledTimes(1);
  });
});

describe("AddProjectDialog non-Tauri fallback flow", () => {
  it("keeps manual form in non-Tauri runtime and registers project", async () => {
    native.isTauriRuntime.mockReturnValue(false);
    const regDef = deferred<unknown>();
    native.registerProject.mockReturnValue(regDef.promise);
    const onRegistered = vi.fn();

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    expect(screen.getByLabelText("Workspace id")).toBeInTheDocument();
    expect(screen.getByLabelText("Repository path")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Workspace id"), { target: { value: "orca-lite" } });
    fireEvent.change(screen.getByLabelText("Repository path"), { target: { value: "/repo/orca-lite" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Project" }));

    await act(async () => {
      regDef.resolve({
        workspaceId: "orca-lite",
        repoRoot: "/repo/orca-lite",
        gitRoot: "/repo/orca-lite",
      });
    });

    expect(native.registerProject).toHaveBeenCalledWith({
      workspaceId: "orca-lite",
      repoPath: "/repo/orca-lite",
    });
    expect(onRegistered).toHaveBeenCalledWith({
      workspaceId: "orca-lite",
      repoRoot: "/repo/orca-lite",
      gitRoot: "/repo/orca-lite",
    });
  });

  it("uses neutral placeholder for workspace id input", () => {
    native.isTauriRuntime.mockReturnValue(false);
    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-local"));

    const input = screen.getByLabelText("Workspace id");
    expect(input).toHaveAttribute("placeholder", "my-project");
  });
});

describe("AddProjectDialog Remote flow", () => {
  it("renders empty state and Settings CTA button when 0 active hosts exist and makes zero native picker calls", () => {
    native.isTauriRuntime.mockReturnValue(true);
    ssh.useSshHosts.mockReturnValue({
      hosts: [],
      loading: false,
      error: null,
      refresh: vi.fn().mockResolvedValue([]),
    });
    const onClose = vi.fn();
    const onOpenSettings = vi.fn();

    render(
      <AddProjectDialog
        onClose={onClose}
        onRegistered={vi.fn()}
        onOpenSettings={onOpenSettings}
      />,
    );

    fireEvent.click(screen.getByTestId("project-type-remote"));

    // Zero native picker calls!
    expect(dialog.open).not.toHaveBeenCalled();

    // Explanatory message and disabled submit
    expect(
      screen.getByText(/No active SSH machines found/i),
    ).toBeInTheDocument();
    expect(screen.getByTestId("add-project-confirm-remote")).toBeDisabled();

    // CTA button navigates to Settings "ssh" and closes dialog
    const cta = screen.getByTestId("configure-ssh-settings");
    expect(cta).toBeInTheDocument();

    fireEvent.click(cta);

    expect(onOpenSettings).toHaveBeenCalledWith("ssh");
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("renders empty state when all configured hosts are disabled", () => {
    native.isTauriRuntime.mockReturnValue(true);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost2], // disabled: true
      loading: false,
      error: null,
      refresh: vi.fn().mockResolvedValue([mockHost2]),
    });

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));

    expect(screen.getByText(/No active SSH machines found/i)).toBeInTheDocument();
    expect(screen.getByTestId("configure-ssh-settings")).toBeInTheDocument();
  });

  it("renders loading indicator when SSH hosts are loading", () => {
    native.isTauriRuntime.mockReturnValue(true);
    ssh.useSshHosts.mockReturnValue({
      hosts: [],
      loading: true,
      error: null,
      refresh: vi.fn().mockResolvedValue([]),
    });

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));

    expect(screen.getByText(/Loading SSH machines.../i)).toBeInTheDocument();
  });

  it("allows returning to location chooser using the Back button", () => {
    native.isTauriRuntime.mockReturnValue(true);

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    expect(screen.getByRole("heading", { name: "Add Remote Project" })).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("add-project-back"));
    expect(screen.getByRole("heading", { name: "Add Project" })).toBeInTheDocument();
    expect(screen.getByTestId("project-type-local")).toBeInTheDocument();
  });

  it("auto-derives workspace id from remote repo path and allows manual override", () => {
    native.isTauriRuntime.mockReturnValue(true);

    render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));

    const pathInput = screen.getByTestId("remote-repo-path-input");
    const wsInput = screen.getByTestId("remote-workspace-id-input");

    fireEvent.change(pathInput, { target: { value: "/srv/apps/cool-service" } });
    expect(wsInput).toHaveValue("cool-service");

    // Manual edit decouples auto-derive
    fireEvent.change(wsInput, { target: { value: "custom-slug" } });
    fireEvent.change(pathInput, { target: { value: "/srv/apps/another-dir" } });
    expect(wsInput).toHaveValue("custom-slug");
  });

  it("registers remote project successfully and maps identity to RegisteredProject target { kind: 'ssh', hostId }", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const refreshDef = deferred<SshHost[]>();
    const refreshMock = vi.fn().mockReturnValue(refreshDef.promise);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const regDef = deferred<unknown>();
    remote.registerRemoteProject.mockReturnValue(regDef.promise);
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));

    const hostSelect = screen.getByTestId("remote-host-select");
    expect(hostSelect).toHaveValue("host-1");

    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/apps/my-repo" },
    });
    expect(screen.getByTestId("remote-workspace-id-input")).toHaveValue("my-repo");

    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));

    // Verifies authoritative host refresh at submit
    expect(refreshMock).toHaveBeenCalledOnce();

    await act(async () => {
      refreshDef.resolve([mockHost1]);
    });

    expect(remote.registerRemoteProject).toHaveBeenCalledWith({
      workspaceId: "my-repo",
      hostId: "host-1",
      repoPath: "/srv/apps/my-repo",
    });

    // Zero native picker or local registerProject calls
    expect(dialog.open).not.toHaveBeenCalled();
    expect(native.registerProject).not.toHaveBeenCalled();

    await act(async () => {
      regDef.resolve({
        workspaceId: "ssh:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
        repoRoot: "/srv/apps/my-repo",
        gitRoot: "/srv/apps/my-repo",
        hostId: "host-1",
        hostLabel: "Dev Server",
      });
    });

    // Server-returned opaque identity adopted, not assuming ID equals typed slug
    expect(onRegistered).toHaveBeenCalledWith({
      workspaceId: "ssh:a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90",
      repoRoot: "/srv/apps/my-repo",
      gitRoot: "/srv/apps/my-repo",
      target: {
        kind: "ssh",
        hostId: "host-1",
      },
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("fails closed when authoritative refreshHosts fails at submit and does not call registerRemoteProject", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const refreshMock = vi.fn().mockRejectedValue(new Error("Network IPC timeout fetching hosts"));
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/apps/my-repo" },
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("add-project-confirm-remote"));
    });

    expect(refreshMock).toHaveBeenCalledOnce();
    // Must NOT call registerRemoteProject
    expect(remote.registerRemoteProject).not.toHaveBeenCalled();
    // Must display extracted error from refresh failure
    expect(screen.getByText("Network IPC timeout fetching hosts")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("clears selection when selected host is removed or disabled instead of silently retargeting to another host", () => {
    native.isTauriRuntime.mockReturnValue(true);
    let currentHosts = [mockHost1, { ...mockHost2, disabled: false }];
    const refreshMock = vi.fn().mockImplementation(async () => currentHosts);

    ssh.useSshHosts.mockReturnValue({
      hosts: currentHosts,
      loading: false,
      error: null,
      refresh: refreshMock,
    });

    const { rerender } = render(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));

    const hostSelect = screen.getByTestId("remote-host-select") as HTMLSelectElement;
    expect(hostSelect.value).toBe("host-1");

    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/apps/my-repo" },
    });

    // Now host-1 is removed from inventory, leaving only host-2
    currentHosts = [{ ...mockHost2, disabled: false }];
    ssh.useSshHosts.mockReturnValue({
      hosts: currentHosts,
      loading: false,
      error: null,
      refresh: refreshMock,
    });

    // Re-render to trigger hook update
    rerender(<AddProjectDialog onClose={vi.fn()} onRegistered={vi.fn()} />);

    // Selection MUST NOT silently retarget to host-2! It must be cleared ("").
    expect(hostSelect.value).toBe("");
    expect(screen.getByTestId("add-project-confirm-remote")).toBeDisabled();

    // Verify submission is blocked and registerRemoteProject is never called for host-2
    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));
    expect(remote.registerRemoteProject).not.toHaveBeenCalled();
  });

  it("suppresses callbacks when externally unmounted while registration is in flight", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const refreshDef = deferred<SshHost[]>();
    const refreshMock = vi.fn().mockReturnValue(refreshDef.promise);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const regDef = deferred<unknown>();
    remote.registerRemoteProject.mockReturnValue(regDef.promise);
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    const { unmount } = render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/repo" },
    });

    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));

    await act(async () => {
      refreshDef.resolve([mockHost1]);
    });

    expect(remote.registerRemoteProject).toHaveBeenCalledOnce();

    // Parent externally unmounts AddProjectDialog
    unmount();

    // Now registration resolves
    await act(async () => {
      regDef.resolve({
        workspaceId: "ssh:external-unmount",
        repoRoot: "/srv/repo",
        gitRoot: null,
        hostId: "host-1",
        hostLabel: "Dev Server",
      });
    });

    // onRegistered must NOT be called
    expect(onRegistered).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("prevents stale host submission when host was removed or disabled before submit", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    // Initial inventory has mockHost1, but authoritative refresh returns empty (host deleted)
    const refreshDef = deferred<SshHost[]>();
    const refreshMock = vi.fn().mockReturnValue(refreshDef.promise);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/apps/repo" },
    });

    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));

    expect(refreshMock).toHaveBeenCalledOnce();

    await act(async () => {
      refreshDef.resolve([]);
    });

    // Submission blocked
    expect(remote.registerRemoteProject).not.toHaveBeenCalled();
    expect(
      screen.getByText("The selected SSH machine is no longer available or has been disabled."),
    ).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces structured IPC error without closing dialog on remote registration failure", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const refreshDef = deferred<SshHost[]>();
    const refreshMock = vi.fn().mockReturnValue(refreshDef.promise);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const regDef = deferred<unknown>();
    remote.registerRemoteProject.mockReturnValue(regDef.promise);
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={vi.fn()} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/bad-path" },
    });

    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));

    await act(async () => {
      refreshDef.resolve([mockHost1]);
    });

    expect(remote.registerRemoteProject).toHaveBeenCalledWith({
      workspaceId: "bad-path",
      hostId: "host-1",
      repoPath: "/srv/bad-path",
    });

    await act(async () => {
      regDef.reject({
        code: "INVALID_PATH",
        message: "Remote path validation failed: directory does not exist",
        details: {},
      });
    });

    expect(
      screen.getByText("Remote path validation failed: directory does not exist"),
    ).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("prevents callbacks and state updates when remote registration resolves after dialog dismissal", async () => {
    native.isTauriRuntime.mockReturnValue(true);
    const refreshDef = deferred<SshHost[]>();
    const refreshMock = vi.fn().mockReturnValue(refreshDef.promise);
    ssh.useSshHosts.mockReturnValue({
      hosts: [mockHost1],
      loading: false,
      error: null,
      refresh: refreshMock,
    });
    const regDef = deferred<unknown>();
    remote.registerRemoteProject.mockReturnValue(regDef.promise);
    const onRegistered = vi.fn();
    const onClose = vi.fn();

    render(<AddProjectDialog onClose={onClose} onRegistered={onRegistered} />);

    fireEvent.click(screen.getByTestId("project-type-remote"));
    fireEvent.change(screen.getByTestId("remote-repo-path-input"), {
      target: { value: "/srv/repo" },
    });

    fireEvent.click(screen.getByTestId("add-project-confirm-remote"));

    await act(async () => {
      refreshDef.resolve([mockHost1]);
    });

    expect(remote.registerRemoteProject).toHaveBeenCalledOnce();

    // User dismisses dialog while registration is in-flight
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();

    // Now registration resolves late
    await act(async () => {
      regDef.resolve({
        workspaceId: "ssh:late-ws",
        repoRoot: "/srv/repo",
        gitRoot: null,
        hostId: "host-1",
        hostLabel: "Dev Server",
      });
    });

    // onRegistered must NOT be called after dismissal
    expect(onRegistered).not.toHaveBeenCalled();
  });
});

describe("AddWorktreeDialog flow", () => {
  it("loads local branches and uses a real branch dropdown for Add Worktree", async () => {
    const branchesDef = deferred<unknown>();
    const createDef = deferred<unknown>();
    native.listProjectBranches.mockReturnValue(branchesDef.promise);
    native.createWorktree.mockReturnValue(createDef.promise);
    const onCreated = vi.fn();

    render(
      <AddWorktreeDialog
        project={{ workspaceId: "orca-lite", repoRoot: "/repo/orca-lite", gitRoot: "/repo/orca-lite" }}
        onClose={vi.fn()}
        onCreated={onCreated}
      />,
    );

    expect(native.listProjectBranches).toHaveBeenCalledWith("orca-lite");

    await act(async () => {
      branchesDef.resolve([
        { name: "develop", isCurrent: false },
        { name: "main", isCurrent: true },
      ]);
    });

    const branch = screen.getByRole("combobox", { name: "Base branch" });
    expect(branch.tagName).toBe("SELECT");
    expect(screen.getByRole("option", { name: "main (current)" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Base branch" })).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Worktree slug"), { target: { value: "feature-ui" } });
    fireEvent.change(branch, { target: { value: "develop" } });
    fireEvent.click(screen.getByRole("button", { name: "Create Worktree" }));

    await act(async () => {
      createDef.resolve({ path: "/repo/worktrees/feature-ui" });
    });

    expect(native.createWorktree).toHaveBeenCalledWith({
      workspaceId: "orca-lite",
      worktree: { wsId: "orca-lite", slug: "feature-ui" },
      baseRef: "develop",
    });
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it("explains that a plain folder project has no branches instead of leaving a dead dialog", () => {
    native.listProjectBranches.mockResolvedValue([]);

    render(
      <AddWorktreeDialog
        project={{ workspaceId: "superwiki-mail-otp", repoRoot: "/repos/superwiki", gitRoot: null }}
        onClose={vi.fn()}
        onCreated={vi.fn()}
      />,
    );

    expect(screen.getByText(/not a Git repository/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create Worktree" })).toBeNull();
    expect(screen.getByRole("button", { name: "Close Add Worktree" })).toBeInTheDocument();
  });
});

describe("RemoveProjectDialog flow", () => {
  it("renders remote folder and configured SSH host label for remote project while preserving backend removal", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    const project: RegisteredProject = {
      workspaceId: "ssh:8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918",
      repoRoot: "/srv/apps/my-service",
      gitRoot: "/srv/apps/my-service",
      target: { kind: "ssh", hostId: "host-1" },
    };

    render(<RemoveProjectDialog project={project} onClose={onClose} onConfirm={onConfirm} />);

    expect(screen.getByRole("dialog", { name: "Remove Project" })).toBeInTheDocument();
    // Must NOT render raw opaque workspace ID
    expect(
      screen.queryByText("ssh:8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918"),
    ).toBeNull();
    // Must render remote folder + host label (from mockHost1: label is "Dev Server")
    expect(screen.getByText("my-service (Dev Server)")).toBeInTheDocument();
    expect(screen.getByText("/srv/apps/my-service")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Remove Project" }));
    expect(onConfirm).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("falls back to hostId when host is not in SSH inventory for remote project", () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn();
    const project: RegisteredProject = {
      workspaceId: "ssh:opaque-hash-value",
      repoRoot: "/srv/apps/legacy-repo",
      gitRoot: null,
      target: { kind: "ssh", hostId: "unknown-host-id" },
    };

    render(<RemoveProjectDialog project={project} onClose={onClose} onConfirm={onConfirm} />);

    // Must fall back to hostId when host is unknown
    expect(screen.getByText("legacy-repo (unknown-host-id)")).toBeInTheDocument();
  });

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
