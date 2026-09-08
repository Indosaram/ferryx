import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  registerRemoteProject,
  toRegisteredProject,
  type RegisterRemoteProjectRequest,
  type RegisteredRemoteProject,
} from "./remoteProject";

const invokeMock = vi.fn();
const isTauriMock = vi.fn(() => true);

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  isTauri: () => isTauriMock(),
}));

describe("remoteProject adapter", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    isTauriMock.mockReturnValue(true);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("invokes cmd_project_register_remote with request and returns server response", async () => {
    const request: RegisterRemoteProjectRequest = {
      workspaceId: "my-project",
      hostId: "host-1",
      repoPath: "/home/ubuntu/repo",
    };
    const response: RegisteredRemoteProject = {
      workspaceId: "ssh:8c6976e5b5410415bde908bd4dee15dfb167a9c873fc4bb8a81f6f2ab448a918",
      repoRoot: "/home/ubuntu/repo",
      gitRoot: "/home/ubuntu/repo",
      hostId: "host-1",
      hostLabel: "Dev Server",
    };
    invokeMock.mockResolvedValueOnce(response);

    const result = await registerRemoteProject(request);

    expect(invokeMock).toHaveBeenCalledWith("cmd_project_register_remote", { request });
    expect(result).toEqual(response);
  });

  it("rejects outside Tauri runtime", async () => {
    isTauriMock.mockReturnValue(false);

    await expect(
      registerRemoteProject({
        workspaceId: "test",
        hostId: "h1",
        repoPath: "/path",
      }),
    ).rejects.toThrow("Remote project registration is available only in the Ferryx desktop runtime");

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("propagates structured IPC errors from backend", async () => {
    const request: RegisterRemoteProjectRequest = {
      workspaceId: "missing-project",
      hostId: "missing-host",
      repoPath: "/path",
    };
    const ipcError = {
      code: "NOT_FOUND",
      message: "SSH host 'missing-host' not found",
      details: {},
    };
    invokeMock.mockRejectedValueOnce(ipcError);

    await expect(registerRemoteProject(request)).rejects.toEqual(ipcError);
  });

  it("maps server-returned identity to RegisteredProject target { kind: 'ssh', hostId }", () => {
    const response: RegisteredRemoteProject = {
      workspaceId: "ssh:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      repoRoot: "/var/www/site",
      gitRoot: "/var/www/site",
      hostId: "host-prod",
      hostLabel: "Production Web",
    };

    const project = toRegisteredProject(response);

    expect(project).toEqual({
      workspaceId: "ssh:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      repoRoot: "/var/www/site",
      gitRoot: "/var/www/site",
      target: {
        kind: "ssh",
        hostId: "host-prod",
      },
    });
  });

  it("maps null gitRoot correctly to RegisteredProject", () => {
    const response: RegisteredRemoteProject = {
      workspaceId: "ssh:cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce",
      repoRoot: "/opt/data",
      gitRoot: null,
      hostId: "host-data",
      hostLabel: "Data Store",
    };

    const project = toRegisteredProject(response);

    expect(project.gitRoot).toBeNull();
    expect(project.workspaceId).toBe(
      "ssh:cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce",
    );
    expect(project.target).toEqual({
      kind: "ssh",
      hostId: "host-data",
    });
  });

  it("preserves Git identity metadata without changing the remote execution target", () => {
    const response = {
      workspaceId: "ssh:linked", repoRoot: "/srv/feature", gitRoot: "/srv/feature",
      gitRemote: "git@github.com:org/app.git", gitCommonDir: "/srv/main/.git",
      hostId: "linux", hostLabel: "Linux",
    };
    expect(toRegisteredProject(response)).toEqual({
      workspaceId: "ssh:linked", repoRoot: "/srv/feature", gitRoot: "/srv/feature",
      gitRemote: "git@github.com:org/app.git", gitCommonDir: "/srv/main/.git",
      target: { kind: "ssh", hostId: "linux" },
    });
  });
});
