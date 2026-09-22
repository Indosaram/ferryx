import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { listPairedProjectWorktrees } from "./pairedProjectWorktrees";
import { createPairedDaemonProjectAdapter, PairedOperationError } from "../lib/pairedDaemonProject";
import { remoteHostStore } from "./remoteHostStore";

vi.mock("../lib/pairedDaemonProject", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/pairedDaemonProject")>();
  return { ...actual, createPairedDaemonProjectAdapter: vi.fn() };
});

const readyState = {
  machineFeaturesEnabled: true,
  nativeStatus: "ready",
  hosts: {
    "host-a": { hostId: "host-a", online: true, authStatus: "paired", grantScope: "machine", generation: "1", machineId: "m1", name: "H" },
  },
};

function makeAdapter(caps: unknown, projects: unknown, worktrees: unknown) {
  const adapter = {
    context: Object.freeze({ hostId: "host-a", generation: "1" }),
    capabilities: vi.fn(() => Promise.resolve(caps)),
    projects: vi.fn(() => Promise.resolve(projects)),
    worktrees: vi.fn(() => Promise.resolve(worktrees)),
  };
  return adapter as unknown as ReturnType<typeof createPairedDaemonProjectAdapter>;
}

const project = {
  workspaceId: "daemon:" + "a".repeat(64),
  repoRoot: "/h/w",
  gitRoot: "/h/w",
  target: { kind: "pairedDaemon" as const, hostId: "host-a" },
  remoteWorkspaceId: "project-abc",
};

describe("listPairedProjectWorktrees transport retry", () => {
  beforeEach(() => {
    remoteHostStore.setState(() => readyState as never);
    vi.useFakeTimers();
  });
  afterEach(() => { vi.useRealTimers(); });

  it("retries PAIRED_HOST_INVALID_RESPONSE and lists rows once the tunnel recovers", async () => {
    const caps = { apiVersion: 1, accessScope: "machine", permission: "control", capabilities: ["directoryBrowseV1", "machineWorkspaceV1", "managedWorktreesV1", "terminalCreateV1"] };
    let projectsCalls = 0;
    const adapter = makeAdapter(caps, null, null) as any;
    adapter.projects = vi.fn(() => {
      projectsCalls += 1;
      if (projectsCalls <= 2) return Promise.reject(new PairedOperationError("PAIRED_HOST_INVALID_RESPONSE"));
      return Promise.resolve({ revision: "41", completeness: "complete", unavailableWorkspaceIds: [], projects: [{ workspaceId: project.workspaceId, remoteWorkspaceId: "project-abc", availability: "ready", target: { kind: "pairedDaemon", hostId: "host-a" }, repoRoot: "/h/w" }] });
    });
    (createPairedDaemonProjectAdapter as ReturnType<typeof vi.fn>).mockReturnValue(adapter);
    adapter.worktrees = vi.fn(() => Promise.resolve({ worktrees: [{ path: "/h/w", workspaceId: "project-abc", branch: "m", head: "h", detached: false }] }));

    const pending = listPairedProjectWorktrees(project as never);
    await vi.runAllTimersAsync();
    const rows = await pending;
    expect(projectsCalls).toBe(3);
    expect(rows?.map(r => r.path)).toEqual(["/h/w"]);
    expect(adapter.projects).toHaveBeenCalledTimes(3);
  });

  it("does not retry definitive host verdicts", async () => {
    const adapter = makeAdapter(null, null, null) as any;
    (createPairedDaemonProjectAdapter as ReturnType<typeof vi.fn>).mockReturnValue(adapter);
    adapter.capabilities = vi.fn(() => Promise.reject(new PairedOperationError("INVALID_REQUEST")));
    await expect(listPairedProjectWorktrees(project as never)).rejects.toThrow("INVALID_REQUEST");
    expect(adapter.capabilities).toHaveBeenCalledTimes(1);
  });

  it("caps retry attempts at 2 resubmits", async () => {
    const adapter = makeAdapter(null, null, null) as any;
    (createPairedDaemonProjectAdapter as ReturnType<typeof vi.fn>).mockReturnValue(adapter);
    adapter.capabilities = vi.fn(() => Promise.reject(new PairedOperationError("PAIRED_HOST_INVALID_RESPONSE")));
    const pending = listPairedProjectWorktrees(project as never);
    pending.catch(() => undefined);
    await vi.runAllTimersAsync();
    await expect(pending).rejects.toThrow("PAIRED_HOST_INVALID_RESPONSE");
    expect(adapter.capabilities).toHaveBeenCalledTimes(3);
  });
});
