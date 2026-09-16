import { afterEach, describe, expect, it, vi } from "vitest";
import { listPairedProjectWorktrees } from "./pairedProjectWorktrees";
import { remoteHostStore } from "./remoteHostStore";
import type { RegisteredProject } from "../lib/types";

const adapter = vi.hoisted(() => ({
  capabilities: vi.fn(),
  projects: vi.fn(),
  worktrees: vi.fn(),
}));

vi.mock("../lib/pairedDaemonProject", () => ({
  createPairedDaemonProjectAdapter: vi.fn(() => adapter),
  PairedOperationError: class PairedOperationError extends Error {
    constructor(message: string) {
      super(message);
      this.name = "PairedOperationError";
    }
  },
}));

afterEach(() => {
  remoteHostStore.reset();
  vi.clearAllMocks();
});

const project: RegisteredProject = {
  workspaceId: `daemon:${"a".repeat(64)}`,
  remoteWorkspaceId: "repo",
  repoRoot: "/srv/repo",
  gitRoot: "/srv/repo",
  target: { kind: "pairedDaemon", hostId: "a" },
};

describe("listPairedProjectWorktrees", () => {
  it("throws if project is not a paired daemon target or lacks remoteWorkspaceId", async () => {
    await expect(
      listPairedProjectWorktrees({ ...project, target: { kind: "local" } as any }),
    ).rejects.toThrow("PAIRED_OWNER_REQUIRED");

    await expect(
      listPairedProjectWorktrees({ ...project, remoteWorkspaceId: undefined } as any),
    ).rejects.toThrow("PAIRED_OWNER_REQUIRED");
  });

  it("returns null when remote state is not ready or host is offline", async () => {
    remoteHostStore.setState((s) => ({
      ...s,
      nativeStatus: "ready",
      machineFeaturesEnabled: true,
      hosts: {
        a: {
          hostId: "a",
          machineId: "machine-a",
          generation: "1",
          name: "Alpha",
          address: "",
          transport: "relay",
          authStatus: "paired",
          grantScope: "machine",
          online: false,
        },
      },
    }));

    const result = await listPairedProjectWorktrees(project);
    expect(result).toBeNull();
  });

  it("returns null when host generation changed mid-flight", async () => {
    remoteHostStore.setState((s) => ({
      ...s,
      nativeStatus: "ready",
      machineFeaturesEnabled: true,
      hosts: {
        a: {
          hostId: "a",
          machineId: "machine-a",
          generation: "1",
          name: "Alpha",
          address: "",
          transport: "relay",
          authStatus: "paired",
          grantScope: "machine",
          online: true,
        },
      },
    }));

    adapter.capabilities.mockResolvedValue({});
    adapter.projects.mockResolvedValue({
      completeness: "complete",
      unavailableWorkspaceIds: [],
      projects: [{ ...project, availability: "ready" }],
    });
    adapter.worktrees.mockImplementation(async () => {
      // Simulate generation bump mid-flight
      remoteHostStore.setState((s) => ({
        ...s,
        hosts: {
          a: {
            ...s.hosts.a,
            generation: "2",
          },
        },
      }));
      return { worktrees: [{ workspaceId: "repo", path: "/srv/repo" }] };
    });

    const result = await listPairedProjectWorktrees(project);
    expect(result).toBeNull();
  });

  it("maps worktrees to the local workspaceId upon authoritative success", async () => {
    remoteHostStore.setState((s) => ({
      ...s,
      nativeStatus: "ready",
      machineFeaturesEnabled: true,
      hosts: {
        a: {
          hostId: "a",
          machineId: "machine-a",
          generation: "1",
          name: "Alpha",
          address: "",
          transport: "relay",
          authStatus: "paired",
          grantScope: "machine",
          online: true,
        },
      },
    }));

    adapter.capabilities.mockResolvedValue({});
    adapter.projects.mockResolvedValue({
      completeness: "complete",
      unavailableWorkspaceIds: [],
      projects: [{ ...project, availability: "ready" }],
    });
    adapter.worktrees.mockResolvedValue({
      worktrees: [{ workspaceId: "repo", path: "/srv/repo" }],
    });

    const result = await listPairedProjectWorktrees(project);
    expect(result).toEqual([{ workspaceId: project.workspaceId, path: "/srv/repo" }]);
  });
});
