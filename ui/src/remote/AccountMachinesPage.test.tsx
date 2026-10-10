import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useAccountWorktrees } from "./useAccountWorktrees";
import * as accountSessionModule from "./accountSession";
import * as accountAttachModule from "./accountAttach";

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (err: unknown) => void;
};

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useAccountWorktrees hook", () => {
  const relayUrl = "https://relay.example.com";
  const sessionToken = "account-session-token";

  const machine1: accountSessionModule.AccountMachineView = {
    machineRecordId: "rec-mach-1",
    machineId: "machine-1",
    displayName: "Primary Mac",
    publicKey: "machine-1-public-key",
    attachPublicKey: "machine-1-attach-key",
    relayOrigin: relayUrl,
    platform: "macos",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: 1,
  };

  const machine2: accountSessionModule.AccountMachineView = {
    machineRecordId: "rec-mach-2",
    machineId: "machine-2",
    displayName: "Linux Server",
    publicKey: "machine-2-public-key",
    attachPublicKey: "machine-2-attach-key",
    relayOrigin: relayUrl,
    platform: "linux",
    online: true,
    enrollmentEpoch: "1",
    lastSeenAt: 2,
  };

  const machine3Offline: accountSessionModule.AccountMachineView = {
    machineRecordId: "rec-mach-3",
    machineId: "machine-3",
    displayName: "Offline Host",
    publicKey: "machine-3-public-key",
    attachPublicKey: "machine-3-attach-key",
    relayOrigin: relayUrl,
    platform: "windows",
    online: false,
    enrollmentEpoch: "1",
    lastSeenAt: 3,
  };

  let mockTunnel1: {
    transport: { fetchLike: ReturnType<typeof vi.fn> };
    close: ReturnType<typeof vi.fn>;
  };
  let mockTunnel2: {
    transport: { fetchLike: ReturnType<typeof vi.fn> };
    close: ReturnType<typeof vi.fn>;
  };

  beforeEach(() => {
    vi.restoreAllMocks();

    mockTunnel1 = {
      transport: {
        fetchLike: vi.fn(async (path: string) => {
          if (path === "/api/v1/workspace/projects") {
            const projects = {
              revision: 1,
              completeness: "complete",
              projects: [
                {
                  workspaceId: "ferryx-repo",
                  repoRoot: "/Users/dev/ferryx",
                  availability: "ready",
                  revision: 1,
                },
              ],
              unavailableWorkspaceIds: [],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(projects)),
            };
          }
          if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=ferryx-repo")) {
            const worktrees = {
              revision: 1,
              worktrees: [
                {
                  workspaceId: "ferryx-repo",
                  identity: { wsId: "ferryx-repo", slug: "main" },
                  path: "/Users/dev/ferryx",
                  head: "head1",
                  branch: "refs/heads/main",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: false,
                },
                {
                  workspaceId: "ferryx-repo",
                  identity: { wsId: "ferryx-repo", slug: "feature-ui" },
                  path: "/Users/dev/ferryx-feature-ui",
                  head: "head2",
                  branch: "refs/heads/feature-ui",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: true,
                },
              ],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(worktrees)),
            };
          }
          if (path === "/api/v1/sessions") {
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(
                JSON.stringify({
                  revision: "1",
                  completeness: "complete",
                  sessions: [],
                }),
              ),
            };
          }
          return { status: 404, headers: {}, body: new Uint8Array(0) };
        }),
      },
      close: vi.fn(),
    };

    mockTunnel2 = {
      transport: {
        fetchLike: vi.fn(async (path: string) => {
          if (path === "/api/v1/workspace/projects") {
            const projects = {
              revision: 1,
              completeness: "complete",
              projects: [
                {
                  workspaceId: "cloud-infra",
                  repoRoot: "/srv/infra",
                  availability: "ready",
                  revision: 1,
                },
              ],
              unavailableWorkspaceIds: [],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(projects)),
            };
          }
          if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=cloud-infra")) {
            const worktrees = {
              revision: 1,
              worktrees: [
                {
                  workspaceId: "cloud-infra",
                  identity: { wsId: "cloud-infra", slug: "prod" },
                  path: "/srv/infra/prod",
                  head: "head3",
                  branch: "refs/heads/prod",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: true,
                },
              ],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(worktrees)),
            };
          }
          if (path === "/api/v1/sessions") {
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(
                JSON.stringify({
                  revision: "1",
                  completeness: "complete",
                  sessions: [],
                }),
              ),
            };
          }
          return { status: 404, headers: {}, body: new Uint8Array(0) };
        }),
      },
      close: vi.fn(),
    };

    vi.spyOn(accountSessionModule, "listMachines").mockResolvedValue([
      machine1,
      machine2,
      machine3Offline,
    ]);

    vi.spyOn(accountAttachModule, "getOrCreateAttachKey").mockResolvedValue({
      publicKey: "device-attach-public-key",
      privateKey: "device-attach-private-key",
    } as never);

    vi.spyOn(accountSessionModule, "requestGrant").mockImplementation(
      async (_relayUrl, _token, machine, _attachKey, options) => ({
        grantId: `grant-${machine.machineId}`,
        machineId: machine.machineId,
        relayOrigin: relayUrl,
        pairingToken: `pairing-token-${machine.machineId}`,
        machineAttachPublicKey: `attach-pub-${machine.machineId}`,
        grantScope: (options?.grantScope ?? "machine") as "machine",
        expiresAt: Date.now() + 600_000,
      }),
    );

    vi.spyOn(accountSessionModule, "allocateSession").mockImplementation(
      async (_relayUrl, _token, machineId) => ({
        sessionId: `session-${machineId}`,
      }),
    );

    vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(
      async (params) => {
        if (params.machineId === machine1.machineId) return mockTunnel1 as never;
        if (params.machineId === machine2.machineId) return mockTunnel2 as never;
        throw new Error(`Unexpected machine ${params.machineId}`);
      },
    );

    vi.spyOn(accountSessionModule, "redeemInTunnel").mockImplementation(
      async (_transport, pairingToken) => ({
        token: `device-token-${pairingToken}`,
        machineId: "mach-resolved",
        displayName: "Work Machine",
        device: { id: "dev-1", name: "Device" },
      }),
    );
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("probes multiple online machines in parallel and aggregates account options", async () => {
    const grantSpy = vi.spyOn(accountSessionModule, "requestGrant");

    const { result } = renderHook(() =>
      useAccountWorktrees(relayUrl, sessionToken, true),
    );

    await act(async () => {});

    expect(result.current.accountOptions.length).toBeGreaterThanOrEqual(3);
    expect(
      result.current.accountOptions.find((o) => o.workspaceId === "cloud-infra")?.machineId,
    ).toBe(machine2.machineId);

    const grantCalls = grantSpy.mock.calls;
    expect(grantCalls.length).toBe(2);
    expect(grantCalls.every((call) => call[4]?.grantScope === "machine")).toBe(true);

    expect(result.current.machineStatuses[machine3Offline.machineId]?.status).toBe("offline");

    expect(mockTunnel1.transport.fetchLike).toHaveBeenCalledWith(
      "/api/v1/workspace/projects",
      expect.objectContaining({
        headers: expect.objectContaining({
          Authorization: expect.stringContaining("Bearer device-token-"),
        }),
      }),
    );
    expect(
      mockTunnel1.transport.fetchLike.mock.calls.some(([path]) =>
        path.includes("/api/v1/workspace/select"),
      ),
    ).toBe(false);
  });

  it("closes all other exploratory tunnels when closeAllExcept is invoked", async () => {
    const { result } = renderHook(() =>
      useAccountWorktrees(relayUrl, sessionToken, true),
    );

    await act(async () => {});

    act(() => {
      result.current.closeAllExcept(machine1.machineId);
    });

    expect(mockTunnel2.close).toHaveBeenCalledTimes(1);
    expect(mockTunnel1.close).not.toHaveBeenCalled();
  });

  it("closes tunnels on unmount when component unmounts while exploratory probes are pending", async () => {
    vi.spyOn(accountSessionModule, "listMachines").mockResolvedValue([machine1]);

    const tunnelDeferred = deferred<typeof mockTunnel1>();
    const openTunnelCalled = deferred<void>();

    vi.spyOn(accountSessionModule, "openTunnel").mockImplementation(() => {
      openTunnelCalled.resolve();
      return tunnelDeferred.promise as never;
    });

    const { unmount } = renderHook(() =>
      useAccountWorktrees(relayUrl, sessionToken, true),
    );

    await openTunnelCalled.promise;
    unmount();

    await act(async () => {
      tunnelDeferred.resolve(mockTunnel1);
    });

    expect(mockTunnel1.close).toHaveBeenCalledTimes(1);
  });

  it("parses server contract fields worktreeSlug and worktreeLabel into distinct options without phantom rows", async () => {
    vi.spyOn(accountSessionModule, "listMachines").mockResolvedValue([machine1]);

    const tunnelServerFields = {
      transport: {
        fetchLike: vi.fn(async (path: string) => {
          if (path === "/api/v1/workspace/projects") {
            const projects = {
              revision: 1,
              completeness: "complete",
              projects: [
                {
                  workspaceId: "ws-canonical",
                  repoRoot: "/srv/repo",
                  availability: "ready",
                  revision: 1,
                },
              ],
              unavailableWorkspaceIds: [],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(projects)),
            };
          }
          if (path.startsWith("/api/v1/workspace/worktrees?workspaceId=ws-canonical")) {
            const worktrees = {
              revision: 1,
              worktrees: [
                {
                  workspaceId: "ws-canonical",
                  identity: { wsId: "ws-canonical", slug: "main" },
                  path: "/srv/repo",
                  head: "h1",
                  branch: "refs/heads/main",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: false,
                },
                {
                  workspaceId: "ws-canonical",
                  identity: { wsId: "ws-canonical", slug: "feat-auth" },
                  path: "/srv/repo-feat",
                  head: "h2",
                  branch: "refs/heads/feat-auth",
                  bare: false,
                  detached: false,
                  locked: null,
                  prunable: null,
                  managed: true,
                },
              ],
            };
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(JSON.stringify(worktrees)),
            };
          }
          if (path === "/api/v1/sessions") {
            return {
              status: 200,
              headers: { "content-type": "application/json" },
              body: new TextEncoder().encode(
                JSON.stringify({
                  revision: "1",
                  completeness: "complete",
                  sessions: [],
                }),
              ),
            };
          }
          return { status: 404, headers: {}, body: new Uint8Array(0) };
        }),
      },
      close: vi.fn(),
    };

    vi.spyOn(accountSessionModule, "openTunnel").mockResolvedValue(
      tunnelServerFields as never,
    );

    const { result } = renderHook(() =>
      useAccountWorktrees(relayUrl, sessionToken, true),
    );

    await act(async () => {});

    expect(result.current.accountOptions).toHaveLength(2);
    expect(result.current.accountOptions.map((o) => o.worktreeSlug)).toEqual(["main", "feat-auth"]);
  });
});
