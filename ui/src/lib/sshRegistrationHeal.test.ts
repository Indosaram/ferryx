import { describe, expect, it, vi } from "vitest";
import type { RegisteredProject } from "./tauri";
import { healMissingSshRegistrations } from "./sshRegistrationHeal";

describe("healMissingSshRegistrations", () => {
  it("registers missing ssh project with exact {workspaceId, hostId, repoPath}", async () => {
    const missingProject: RegisteredProject = {
      workspaceId: "ssh:111111",
      repoRoot: "/srv/repo1",
      hostLabel: "build-box",
      target: { kind: "ssh", hostId: "box-1" },
    };

    const hasRegistered = vi.fn().mockResolvedValue(false);
    const register = vi.fn().mockResolvedValue({ ok: true });

    const result = await healMissingSshRegistrations([missingProject], {
      hasRegistered,
      register,
    });

    expect(hasRegistered).toHaveBeenCalledWith("ssh:111111");
    expect(register).toHaveBeenCalledWith({
      workspaceId: "ssh:111111",
      hostId: "box-1",
      repoPath: "/srv/repo1",
    });
    expect(result).toEqual({
      healed: ["ssh:111111"],
      skipped: 0,
      failures: [],
    });
  });

  it("skips already-registered ssh project when hasRegistered is true", async () => {
    const existingProject: RegisteredProject = {
      workspaceId: "ssh:222222",
      repoRoot: "/srv/repo2",
      target: { kind: "ssh", hostId: "box-2" },
    };

    const hasRegistered = vi.fn().mockReturnValue(true);
    const register = vi.fn();

    const result = await healMissingSshRegistrations([existingProject], {
      hasRegistered,
      register,
    });

    expect(hasRegistered).toHaveBeenCalledWith("ssh:222222");
    expect(register).not.toHaveBeenCalled();
    expect(result).toEqual({
      healed: [],
      skipped: 1,
      failures: [],
    });
  });

  it("skips daemon and local kinds", async () => {
    const localProject: RegisteredProject = {
      workspaceId: "local-proj",
      repoRoot: "/Users/user/repo",
    };
    const daemonProject: RegisteredProject = {
      workspaceId: "daemon:333333",
      remoteWorkspaceId: "repo-remote",
      repoRoot: "/srv/daemon",
      target: { kind: "pairedDaemon", hostId: "daemon-host" },
    };

    const hasRegistered = vi.fn();
    const register = vi.fn();

    const result = await healMissingSshRegistrations([localProject, daemonProject], {
      hasRegistered,
      register,
    });

    expect(hasRegistered).not.toHaveBeenCalled();
    expect(register).not.toHaveBeenCalled();
    expect(result).toEqual({
      healed: [],
      skipped: 2,
      failures: [],
    });
  });

  it("registers duplicate ids only once", async () => {
    const project1: RegisteredProject = {
      workspaceId: "ssh:444444",
      repoRoot: "/srv/repo4",
      target: { kind: "ssh", hostId: "box-4" },
    };
    const project2: RegisteredProject = {
      workspaceId: "ssh:444444",
      repoRoot: "/srv/repo4",
      target: { kind: "ssh", hostId: "box-4" },
    };

    const hasRegistered = vi.fn().mockResolvedValue(false);
    const register = vi.fn().mockResolvedValue({ ok: true });

    const result = await healMissingSshRegistrations([project1, project2], {
      hasRegistered,
      register,
    });

    expect(register).toHaveBeenCalledTimes(1);
    expect(register).toHaveBeenCalledWith({
      workspaceId: "ssh:444444",
      hostId: "box-4",
      repoPath: "/srv/repo4",
    });
    expect(result).toEqual({
      healed: ["ssh:444444"],
      skipped: 1,
      failures: [],
    });
  });

  it("lands rejection in failures without throwing", async () => {
    const failingProject: RegisteredProject = {
      workspaceId: "ssh:555555",
      repoRoot: "/srv/repo5",
      target: { kind: "ssh", hostId: "box-5" },
    };
    const successProject: RegisteredProject = {
      workspaceId: "ssh:666666",
      repoRoot: "/srv/repo6",
      target: { kind: "ssh", hostId: "box-6" },
    };

    const error = new Error("Network unreachable");
    const hasRegistered = vi.fn().mockResolvedValue(false);
    const register = vi.fn().mockImplementation((r: { workspaceId: string }) => {
      if (r.workspaceId === "ssh:555555") {
        return Promise.reject(error);
      }
      return Promise.resolve({ ok: true });
    });

    const result = await healMissingSshRegistrations([failingProject, successProject], {
      hasRegistered,
      register,
    });

    expect(result).toEqual({
      healed: ["ssh:666666"],
      skipped: 0,
      failures: [{ workspaceId: "ssh:555555", error }],
    });
  });
});
