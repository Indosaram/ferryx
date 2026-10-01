import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  cancelRemoteDropUpload,
  pasteClipboardImageLocally,
  pasteClipboardImageToRemote,
  quoteRemotePath,
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
      gitBranch: null,
      gitHead: null,
      hostLabel: "Production Web",
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
    expect(project.hostLabel).toBe("Data Store");
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
      gitBranch: null, gitHead: null, hostLabel: "Linux",
      target: { kind: "ssh", hostId: "linux" },
    });
  });

  it("passes the backend session ID to cmd_local_paste_clipboard_image and returns its path", async () => {
    invokeMock.mockResolvedValueOnce({ localPath: "C:\\Users\\dev\\a.png", byteLength: 3 });

    await expect(pasteClipboardImageLocally("daemon-backend-1")).resolves.toEqual({
      localPath: "C:\\Users\\dev\\a.png",
      byteLength: 3,
    });
    expect(invokeMock).toHaveBeenCalledWith("cmd_local_paste_clipboard_image", {
      sessionId: "daemon-backend-1",
    });
  });

  it("maps a null local clipboard result to null and propagates upload errors", async () => {
    invokeMock.mockResolvedValueOnce(null);
    await expect(pasteClipboardImageLocally(null)).resolves.toBeNull();
    expect(invokeMock).toHaveBeenCalledWith("cmd_local_paste_clipboard_image", { sessionId: null });

    const ipcError = { code: "INTERNAL_ERROR", message: "upload failed", details: {} };
    invokeMock.mockRejectedValueOnce(ipcError);
    await expect(pasteClipboardImageLocally("daemon-backend-1")).rejects.toEqual(ipcError);
  });

  it("keeps registered ssh:/daemon: clipboard routing keyed by workspace only", async () => {
    invokeMock.mockResolvedValue({ remotePath: "/tmp/x.png", byteLength: 1 });

    await pasteClipboardImageToRemote("ssh:abc");
    await pasteClipboardImageToRemote("daemon:def");

    expect(invokeMock.mock.calls).toEqual([
      ["cmd_ssh_paste_clipboard_image", { workspaceId: "ssh:abc" }],
      ["cmd_daemon_paste_clipboard_image", { workspaceId: "daemon:def" }],
    ]);
  });

  it("maps remote gitBranch and gitHead through to RegisteredProject", () => {
    const response = {
      workspaceId: "ssh:branch-test", repoRoot: "/srv/app", gitRoot: "/srv/app",
      gitBranch: "feature/new-ui", gitHead: "deadbeef123",
      hostId: "server", hostLabel: "Server",
    };
    expect(toRegisteredProject(response)).toEqual({
      workspaceId: "ssh:branch-test", repoRoot: "/srv/app", gitRoot: "/srv/app",
      gitBranch: "feature/new-ui", gitHead: "deadbeef123", hostLabel: "Server",
      target: { kind: "ssh", hostId: "server" },
    });
  });

  it("quotes remote paths correctly according to detected platform", () => {
    expect(quoteRemotePath("/tmp/ferryx-paste/u1/file.txt", "posix")).toBe("/tmp/ferryx-paste/u1/file.txt");
    expect(quoteRemotePath("/tmp/ferryx-paste/u1/my file.txt", "posix")).toBe("'/tmp/ferryx-paste/u1/my file.txt'");
    expect(quoteRemotePath("C:\\Users\\sook\\AppData\\Local\\Temp\\ferryx-paste\\u1\\file.txt", "windows")).toBe(
      "C:\\Users\\sook\\AppData\\Local\\Temp\\ferryx-paste\\u1\\file.txt",
    );
    expect(quoteRemotePath("C:\\Users\\sook\\AppData\\Local\\Temp\\ferryx-paste\\u1\\my file.txt", "windows")).toBe(
      '"C:\\Users\\sook\\AppData\\Local\\Temp\\ferryx-paste\\u1\\my file.txt"',
    );
  });
});
